//! Encrypted backups of the personal vault in an S3-compatible bucket (AWS S3, Cloudflare R2,
//! MinIO), restorable on macOS and Windows alike.
//!
//! Layout under the configured prefix:
//! - `key.age`: the vault key, encrypted with the user's passphrase.
//! - `snapshots/<seq>.age`: one backup, an encrypted manifest of every file's name and blob.
//! - `blobs/<id[..2]>/<id>.age`: file content, encrypted. `id` is an HMAC of the content, so a
//!   blob is written once and never changes, and unchanged files are not uploaded again.
//!
//! Old snapshots and the blobs only they needed are removed by `prune`, the only code that
//! deletes objects.
//!
//! A snapshot takes the next sequence number with a create-only write, so two devices syncing at
//! the same moment cannot overwrite each other: the one that loses merges again on top of the
//! winner's snapshot (see `sync`).

pub mod files;
pub mod key;
pub mod keychain;
pub mod remote;
mod prune;
mod run;
mod sync;

pub use key::VaultKey;
pub use remote::{Credentials, Remote};
pub use prune::{PruneReport, prune};
pub use run::{BACKGROUND_WAIT, Status, connect, last_status, run, summary};
pub use sync::{Queued, SyncOptions, SyncReport, state_path, sync};

use age::secrecy::SecretString;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const FORMAT: u32 = 1;
const KEY_FILE: &str = "key.age";
const SNAPSHOTS: &str = "snapshots";
const BLOBS: &str = "blobs";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub seq: u64,
    /// RFC 3339, UTC.
    pub created: String,
    pub device: String,
    /// Portable vault-relative name → content.
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub blob: String,
    pub size: u64,
}

fn snapshot_key(seq: u64) -> String {
    format!("{SNAPSHOTS}/{seq:012}.age")
}

/// The snapshot number in a key `snapshot_key` made; `None` for any other object.
fn seq_of(key: &str) -> Option<u64> {
    key.rsplit('/').next()?.strip_suffix(".age")?.parse().ok()
}

fn blob_key(id: &str) -> String {
    format!("{BLOBS}/{}/{id}.age", &id[..2])
}

/// Whether the bucket already holds a backup (and so a passphrase to unlock, not to choose).
pub fn has_key(remote: &Remote) -> Result<bool> {
    Ok(remote.get(KEY_FILE)?.is_some())
}

/// Unlock the backup's key with the passphrase, or create the key on first use. `true`: created.
pub fn unlock_or_create(remote: &Remote, passphrase: &SecretString) -> Result<(VaultKey, bool)> {
    if let Some(wrapped) = remote.get(KEY_FILE)? {
        return Ok((VaultKey::unwrap(&wrapped, passphrase)?, false));
    }
    let key = VaultKey::generate();
    if remote.create(KEY_FILE, key.wrap(passphrase)?)? {
        return Ok((key, true));
    }
    // Another device created it in the meantime: that one counts.
    let wrapped = remote.get(KEY_FILE)?.ok_or_else(|| anyhow!("key.age vanished from the bucket"))?;
    Ok((VaultKey::unwrap(&wrapped, passphrase)?, false))
}

/// Snapshot numbers in the bucket, oldest first.
pub fn snapshot_seqs(remote: &Remote) -> Result<Vec<u64>> {
    let mut seqs: Vec<u64> = remote
        .list(SNAPSHOTS)?
        .iter()
        .filter_map(|k| seq_of(k))
        .collect();
    seqs.sort_unstable();
    Ok(seqs)
}

pub fn load_snapshot(remote: &Remote, key: &VaultKey, seq: u64) -> Result<Manifest> {
    let cipher = remote.get(&snapshot_key(seq))?.ok_or_else(|| anyhow!("there is no snapshot {seq}"))?;
    let plain = key.decrypt(&cipher).with_context(|| format!("decrypting snapshot {seq}"))?;
    let m: Manifest = serde_json::from_slice(&plain).with_context(|| format!("reading snapshot {seq}"))?;
    if m.format != FORMAT {
        bail!("snapshot {seq} has format {}; this obmem reads format {FORMAT}", m.format);
    }
    if m.seq != seq {
        bail!("snapshot {seq} says it is snapshot {}: the bucket was tampered with", m.seq);
    }
    Ok(m)
}

#[derive(Debug)]
pub struct RestoreReport {
    pub seq: u64,
    pub files: usize,
}

/// Write snapshot `seq` (default: the latest) into `target`, which must be missing or empty:
/// restoring never overwrites a file.
pub fn restore(remote: &Remote, key: &VaultKey, seq: Option<u64>, target: &Path) -> Result<RestoreReport> {
    let seq = match seq {
        Some(s) => s,
        None => *snapshot_seqs(remote)?.last().ok_or_else(|| anyhow!("the bucket has no snapshots yet"))?,
    };
    let manifest = load_snapshot(remote, key, seq)?;
    // Names come from the bucket: check them all before writing anything.
    for name in manifest.files.keys() {
        files::check_portable(name).map_err(|why| anyhow!("snapshot {seq} holds `{name}`: {why}"))?;
    }
    if let Some((a, b)) = files::case_collisions(manifest.files.keys().map(String::as_str)).first() {
        bail!("snapshot {seq} holds both {a} and {b}, which are the same file here");
    }
    if std::fs::symlink_metadata(target).is_ok() {
        let empty = std::fs::read_dir(target)
            .with_context(|| format!("{} is not a folder", target.display()))?
            .next()
            .is_none();
        if !empty {
            bail!("{} is not empty: restore into an empty or new folder", target.display());
        }
    }
    std::fs::create_dir_all(target).with_context(|| format!("creating {}", target.display()))?;
    let root = std::fs::canonicalize(target)?;

    for (name, entry) in &manifest.files {
        let cipher = remote
            .get(&blob_key(&entry.blob))?
            .ok_or_else(|| anyhow!("the bucket is missing the content of {name}"))?;
        let content = key.decrypt(&cipher).with_context(|| format!("decrypting {name}"))?;
        if key.blob_id(&content) != entry.blob {
            bail!("the content of {name} does not match its snapshot entry");
        }
        let path = sync::path_in(&root, name);
        obmem_core::store::atomic_write_bytes(&root, &path, &content)?;
    }
    Ok(RestoreReport { seq, files: manifest.files.len() })
}

#[cfg(test)]
pub(crate) fn tempdir() -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("obmem-sync-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::{device, pass, put};

    #[test]
    fn restore_on_a_new_device_needs_only_the_passphrase() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "alpha");
        put(&mac.vault, "Projects/x/Sessions/2026-11-09-mac.md", "log");
        put(&mac.vault, ".obsidian/app.json", "{}");
        put(&mac.vault, ".obsidian/workspace.json", "{}");
        mac.sync().unwrap();

        assert!(has_key(&remote).unwrap());
        let (key, created) = unlock_or_create(&remote, &pass("pw")).unwrap();
        assert!(!created);
        let target = tempdir().join("restored");
        let r = restore(&remote, &key, None, &target).unwrap();
        assert_eq!((r.seq, r.files), (1, 3));
        assert_eq!(std::fs::read_to_string(target.join("Global/a.md")).unwrap(), "alpha");
        assert!(!target.join(".obsidian/workspace.json").exists());
    }

    #[test]
    fn older_snapshots_stay_restore_points() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "one");
        mac.sync().unwrap();
        put(&mac.vault, "Global/a.md", "two");
        mac.sync().unwrap();
        let old = tempdir().join("old");
        restore(&remote, &mac.key, Some(1), &old).unwrap();
        assert_eq!(std::fs::read_to_string(old.join("Global/a.md")).unwrap(), "one");
    }

    #[test]
    fn the_bucket_content_is_encrypted() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/secret-plan.md", "the plan is secret");
        mac.sync().unwrap();
        for k in remote.list("").unwrap() {
            assert!(!k.contains("secret-plan"), "{k}");
            let bytes = remote.get(&k).unwrap().unwrap();
            assert!(!bytes.windows(6).any(|w| w == b"secret"), "{k} holds plain text");
        }
    }

    #[test]
    fn a_wrong_passphrase_does_not_unlock() {
        let remote = remote::in_memory();
        unlock_or_create(&remote, &pass("pw")).unwrap();
        assert!(unlock_or_create(&remote, &pass("nope")).is_err());
    }

    #[test]
    fn restore_never_overwrites() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "one");
        mac.sync().unwrap();
        let err = restore(&remote, &mac.key, None, &mac.vault).unwrap_err();
        assert!(err.to_string().contains("not empty"), "{err}");
    }

    #[test]
    fn restore_refuses_names_from_the_bucket_that_leave_the_folder() {
        let remote = remote::in_memory();
        let (key, _) = unlock_or_create(&remote, &pass("pw")).unwrap();
        let blob = key.blob_id(b"x");
        remote.create(&blob_key(&blob), key.encrypt(b"x").unwrap()).unwrap();
        let manifest = Manifest {
            format: FORMAT,
            seq: 1,
            created: String::new(),
            device: "evil".into(),
            files: BTreeMap::from([("../escape.md".to_string(), FileEntry { blob, size: 1 })]),
        };
        remote.create(&snapshot_key(1), key.encrypt(&serde_json::to_vec(&manifest).unwrap()).unwrap()).unwrap();
        let target = tempdir().join("t");
        assert!(restore(&remote, &key, None, &target).is_err());
        assert!(!target.parent().unwrap().join("escape.md").exists());
    }

    #[test]
    fn a_snapshot_stored_under_another_number_is_rejected() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "one");
        mac.sync().unwrap();
        let first = remote.get(&snapshot_key(1)).unwrap().unwrap();
        remote.create(&snapshot_key(7), first).unwrap();
        assert!(load_snapshot(&remote, &mac.key, 7).is_err());
    }
}
