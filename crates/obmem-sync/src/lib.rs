//! Encrypted backups of the personal vault in an S3-compatible bucket (AWS S3, Cloudflare R2,
//! MinIO), restorable on macOS and Windows alike.
//!
//! Layout under the configured prefix:
//! - `key.age`: the vault key, encrypted with the user's passphrase.
//! - `snapshots/<seq>.age`: one backup, an encrypted manifest of every file's name and blob.
//! - `blobs/<id[..2]>/<id>.age`: file content, encrypted. `id` is an HMAC of the content, so a
//!   blob is written once and never changes, and unchanged files are not uploaded again.
//!
//! A snapshot takes the next sequence number with a create-only write, so two devices backing up
//! at the same moment cannot overwrite each other: the one that loses takes the number after.

pub mod files;
pub mod key;
pub mod keychain;
pub mod remote;

pub use key::VaultKey;
pub use remote::{Credentials, Remote};

use age::secrecy::SecretString;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

const FORMAT: u32 = 1;
const KEY_FILE: &str = "key.age";
const SNAPSHOTS: &str = "snapshots";
const BLOBS: &str = "blobs";
/// How often a backup tries the next sequence number when other devices keep taking it.
const ATTEMPTS: usize = 5;

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

fn blob_key(id: &str) -> String {
    format!("{BLOBS}/{}/{id}.age", &id[..2])
}

/// This machine's name in snapshots: `[sync] device`, else the host name.
pub fn device_name(cfg: &obmem_core::config::SyncConfig) -> String {
    cfg.device.clone().unwrap_or_else(|| gethostname::gethostname().to_string_lossy().into_owned())
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
        .filter_map(|k| k.rsplit('/').next()?.strip_suffix(".age")?.parse().ok())
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

pub struct BackupOptions<'a> {
    pub device: &'a str,
    pub community_plugins: bool,
}

#[derive(Debug)]
pub struct BackupReport {
    /// The snapshot that now holds the vault.
    pub seq: u64,
    /// `false`: nothing changed since snapshot `seq`, so no new one was made.
    pub created: bool,
    pub files: usize,
    /// Blobs this backup uploaded (new or changed content).
    pub uploaded: usize,
    /// Files left out, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// Back up `vault` as a new snapshot, uploading only content the latest snapshot lacks.
pub fn backup(remote: &Remote, key: &VaultKey, vault: &Path, opts: &BackupOptions) -> Result<BackupReport> {
    if !vault.is_dir() {
        bail!("{} is not a folder", vault.display());
    }
    let mut scan = files::scan(vault, opts.community_plugins)?;
    // Both names would land on the same file on macOS and Windows: keep the first.
    for (kept, dropped) in files::case_collisions(scan.files.iter().map(|(n, _)| n.as_str())) {
        scan.files.retain(|(n, _)| *n != dropped);
        scan.skipped.push((dropped, format!("differs from {kept} only in case")));
    }

    let latest = snapshot_seqs(remote)?.last().copied();
    let previous = latest.map(|seq| load_snapshot(remote, key, seq)).transpose()?;
    let mut known: HashSet<String> =
        previous.iter().flat_map(|m| m.files.values().map(|e| e.blob.clone())).collect();

    let mut entries = BTreeMap::new();
    let mut uploaded = 0;
    for (name, path) in &scan.files {
        let content = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let blob = key.blob_id(&content);
        if known.insert(blob.clone()) && remote.create(&blob_key(&blob), key.encrypt(&content)?)? {
            uploaded += 1;
        }
        entries.insert(name.clone(), FileEntry { blob, size: content.len() as u64 });
    }

    let mut base = (latest, previous);
    for _ in 0..ATTEMPTS {
        let (latest, previous) = &base;
        if let (Some(seq), Some(prev)) = (latest, previous)
            && prev.files == entries
        {
            return Ok(report(*seq, false, &entries, uploaded, scan.skipped));
        }
        let seq = latest.map_or(1, |s| s + 1);
        let manifest = Manifest {
            format: FORMAT,
            seq,
            created: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            device: opts.device.to_string(),
            files: entries.clone(),
        };
        if remote.create(&snapshot_key(seq), key.encrypt(&serde_json::to_vec(&manifest)?)?)? {
            return Ok(report(seq, true, &entries, uploaded, scan.skipped));
        }
        // Another device took this number: build on what it wrote.
        let latest = snapshot_seqs(remote)?.last().copied();
        base = (latest, latest.map(|s| load_snapshot(remote, key, s)).transpose()?);
    }
    bail!("other devices kept taking the next snapshot number; try again")
}

fn report(seq: u64, created: bool, entries: &BTreeMap<String, FileEntry>, uploaded: usize, skipped: Vec<(String, String)>) -> BackupReport {
    BackupReport { seq, created, files: entries.len(), uploaded, skipped }
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
        let path = name.split('/').fold(root.clone(), |p, part| p.join(part));
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

    fn pass(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn put(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    const OPTS: BackupOptions = BackupOptions { device: "mac", community_plugins: false };

    #[test]
    fn backup_then_restore_on_another_device() {
        let remote = remote::in_memory();
        let (key, created) = unlock_or_create(&remote, &pass("pw")).unwrap();
        assert!(created);
        let vault = tempdir();
        put(&vault, "Global/a.md", "alpha");
        put(&vault, "Global/b.md", "alpha");
        put(&vault, "Projects/x/Sessions/2026-11-09.md", "log");
        put(&vault, ".obsidian/app.json", "{}");
        put(&vault, ".obsidian/workspace.json", "{}");

        let r = backup(&remote, &key, &vault, &OPTS).unwrap();
        assert_eq!((r.seq, r.created, r.files, r.uploaded), (1, true, 4, 3), "equal content is uploaded once");

        // A second device knows only the passphrase.
        assert!(has_key(&remote).unwrap());
        let (key2, created) = unlock_or_create(&remote, &pass("pw")).unwrap();
        assert!(!created);
        let target = tempdir().join("restored");
        let rr = restore(&remote, &key2, None, &target).unwrap();
        assert_eq!((rr.seq, rr.files), (1, 4));
        assert_eq!(std::fs::read_to_string(target.join("Global/b.md")).unwrap(), "alpha");
        assert_eq!(std::fs::read_to_string(target.join("Projects/x/Sessions/2026-11-09.md")).unwrap(), "log");
        assert!(!target.join(".obsidian/workspace.json").exists());
    }

    #[test]
    fn unchanged_vault_makes_no_new_snapshot_and_changes_upload_only_the_difference() {
        let remote = remote::in_memory();
        let (key, _) = unlock_or_create(&remote, &pass("pw")).unwrap();
        let vault = tempdir();
        put(&vault, "Global/a.md", "one");
        put(&vault, "Global/b.md", "two");
        backup(&remote, &key, &vault, &OPTS).unwrap();

        let r = backup(&remote, &key, &vault, &OPTS).unwrap();
        assert_eq!((r.seq, r.created, r.uploaded), (1, false, 0));

        put(&vault, "Global/b.md", "two, edited");
        std::fs::remove_file(vault.join("Global/a.md")).unwrap();
        let r = backup(&remote, &key, &vault, &OPTS).unwrap();
        assert_eq!((r.seq, r.created, r.files, r.uploaded), (2, true, 1, 1));
        assert_eq!(snapshot_seqs(&remote).unwrap(), [1, 2]);

        // The older snapshot is still a restore point.
        let old = tempdir().join("old");
        restore(&remote, &key, Some(1), &old).unwrap();
        assert_eq!(std::fs::read_to_string(old.join("Global/a.md")).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(old.join("Global/b.md")).unwrap(), "two");
    }

    #[test]
    fn devices_share_one_numbering() {
        let remote = remote::in_memory();
        let (key, _) = unlock_or_create(&remote, &pass("pw")).unwrap();
        let vault = tempdir();
        put(&vault, "Global/a.md", "one");
        let other = tempdir();
        put(&other, "Global/z.md", "theirs");
        backup(&remote, &key, &other, &BackupOptions { device: "win", ..OPTS }).unwrap();
        let r = backup(&remote, &key, &vault, &OPTS).unwrap();
        assert_eq!((r.seq, r.created), (2, true));
        let m = load_snapshot(&remote, &key, 1).unwrap();
        assert_eq!(m.device, "win");
    }

    #[test]
    fn the_bucket_content_is_encrypted() {
        let remote = remote::in_memory();
        let (key, _) = unlock_or_create(&remote, &pass("pw")).unwrap();
        let vault = tempdir();
        put(&vault, "Global/secret-plan.md", "the plan is secret");
        backup(&remote, &key, &vault, &OPTS).unwrap();
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
        let (key, _) = unlock_or_create(&remote, &pass("pw")).unwrap();
        let vault = tempdir();
        put(&vault, "Global/a.md", "one");
        backup(&remote, &key, &vault, &OPTS).unwrap();
        let err = restore(&remote, &key, None, &vault).unwrap_err();
        assert!(err.to_string().contains("not empty"), "{err}");
        assert_eq!(std::fs::read_to_string(vault.join("Global/a.md")).unwrap(), "one");
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
        let (key, _) = unlock_or_create(&remote, &pass("pw")).unwrap();
        let vault = tempdir();
        put(&vault, "Global/a.md", "one");
        backup(&remote, &key, &vault, &OPTS).unwrap();
        let first = remote.get(&snapshot_key(1)).unwrap().unwrap();
        remote.create(&snapshot_key(7), first).unwrap();
        assert!(load_snapshot(&remote, &key, 7).is_err());
    }
}
