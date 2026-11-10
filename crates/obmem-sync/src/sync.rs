//! Two-way sync of the personal vault with the bucket.
//!
//! Each device remembers the snapshot it last synced with (its *base*). A sync compares, per
//! file, the base, the vault and the latest snapshot:
//! - Only one side changed: that side wins, deletions included.
//! - Both changed: the local version stays, and the other device's version is written next to
//!   it as a conflict copy (`<stem>.sync-conflict-<device>-<time>.<ext>`), which the audit's
//!   `sync-conflict` rule reports until a person merges and deletes it. A deletion on one side
//!   and an edit on the other keep the edit.
//!
//! Blobs the latest snapshot lacks are uploaded even when the bucket already has them: an
//! unreferenced blob may be old enough for a prune to collect at any moment (see `prune`).
//!
//! The merged result becomes a new snapshot before anything changes locally. If another device
//! took that snapshot number first, nothing local has changed yet and the sync starts over on
//! top of the newer snapshot. A local file that changes while the sync runs is left alone and
//! handled by the next sync.

use crate::files;
use crate::key::VaultKey;
use crate::remote::{Remote, location};
use crate::{FORMAT, FileEntry, Manifest, blob_key, load_snapshot, snapshot_key, snapshot_seqs};
use anyhow::{Context, Result, anyhow, bail};
use obmem_core::config::SyncConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// How often a sync starts over when other devices keep taking the next snapshot number, or
/// files keep changing under it.
const ATTEMPTS: usize = 5;
/// The least time between the starts of two syncs that agent hooks leave in the background.
pub(crate) const MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

pub struct SyncOptions<'a> {
    pub device: &'a str,
    pub community_plugins: bool,
    /// Go ahead even when the sync would delete many files (see [`too_many_deletions`]).
    pub allow_mass_delete: bool,
    /// How long to wait for a sync already running on this machine. Zero: fail at once. While
    /// waiting, at most one sync queues behind the running one; another that comes along
    /// returns [`Queued`], because the queued sync will see its changes too.
    pub wait: std::time::Duration,
    /// A sync that waits starts no sooner than this after the last sync started, so a burst of
    /// agent turns makes one sync, not one each. Zero: as soon as the running sync ends.
    pub spacing: std::time::Duration,
}

/// The error of a sync that did not run because another one is already queued.
#[derive(Debug)]
pub struct Queued;

impl std::fmt::Display for Queued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a sync is already queued on this machine")
    }
}

impl std::error::Error for Queued {}

#[derive(Debug, Default)]
pub struct SyncReport {
    /// The snapshot the vault now matches; 0 when the bucket and the vault are both empty.
    pub seq: u64,
    /// `false`: the bucket already matched, no new snapshot was made.
    pub created: bool,
    /// Local changes now in the bucket.
    pub uploaded: Vec<String>,
    pub downloaded: Vec<String>,
    pub deleted_local: Vec<String>,
    pub deleted_remote: Vec<String>,
    /// Conflict copies written next to the files both sides changed.
    pub conflicts: Vec<String>,
    /// Files that changed while the sync ran; the next sync takes them.
    pub deferred: Vec<String>,
    /// Files this device cannot sync, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// What a device remembers between syncs, in `~/.config/obmem/sync/` (not in the vault, which
/// is what gets synced).
#[derive(Default, Serialize, Deserialize)]
struct State {
    /// `blob_id("")` under the key the base was made with: after `setup` against another bucket
    /// or a recreated key, the old base means nothing.
    key_check: String,
    seq: u64,
    /// Name → blob id, as of snapshot `seq`.
    files: BTreeMap<String, String>,
}

/// Where this device keeps its sync state for `vault` and the configured bucket.
pub fn state_path(cfg: &SyncConfig, vault: &Path) -> PathBuf {
    let vault = std::fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
    let mut h = Sha256::new();
    h.update(location(cfg).as_bytes());
    h.update(b"\n");
    h.update(vault.to_string_lossy().as_bytes());
    let id = hex::encode(&h.finalize()[..8]);
    let dir = obmem_core::config::global_config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    dir.join("sync").join(format!("{id}.json"))
}

pub fn sync(
    remote: &Remote,
    key: &VaultKey,
    vault: &Path,
    state_file: &Path,
    opts: &SyncOptions,
) -> Result<SyncReport> {
    if !vault.is_dir() {
        bail!("{} is not a folder", vault.display());
    }
    let _lock = Lock::acquire(state_file, opts.wait, opts.spacing)?;
    let state = read_state(state_file)?;
    for _ in 0..ATTEMPTS {
        if let Some(report) = attempt(remote, key, vault, state_file, &state, opts)? {
            return Ok(report);
        }
    }
    bail!("the bucket or the vault kept changing during the sync; try again")
}

/// One pass. `None`: start over (another device took the snapshot number, or a file to upload
/// changed); nothing local has changed yet.
fn attempt(
    remote: &Remote,
    key: &VaultKey,
    vault: &Path,
    state_file: &Path,
    state: &Option<State>,
    opts: &SyncOptions,
) -> Result<Option<SyncReport>> {
    let mut scan = files::scan(vault, opts.community_plugins)?;
    // Both names would be one file on macOS and Windows: keep the first.
    for (kept, dropped) in files::case_collisions(scan.files.iter().map(|(n, _)| n.as_str())) {
        scan.files.retain(|(n, _)| *n != dropped);
        scan.skipped
            .push((dropped, format!("differs from {kept} only in case")));
    }
    let mut local = BTreeMap::new();
    let mut sizes = std::collections::HashMap::new();
    for (name, path) in &scan.files {
        let content = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let blob = key.blob_id(&content);
        sizes.insert(blob.clone(), content.len() as u64);
        local.insert(name.clone(), blob);
    }

    let latest = snapshot_seqs(remote)?.last().copied();
    let remote_m = latest
        .map(|seq| load_snapshot(remote, key, seq))
        .transpose()?;
    let remote_files: BTreeMap<String, String> = remote_m
        .iter()
        .flat_map(|m| m.files.iter().map(|(n, e)| (n.clone(), e.blob.clone())))
        .collect();
    for e in remote_m.iter().flat_map(|m| m.files.values()) {
        sizes.insert(e.blob.clone(), e.size);
    }

    // Names this pass leaves alone on both sides: files the scan skipped (a symlink, a name that
    // differs from another only in case) are not deletions, and names from the bucket that this
    // file system cannot hold are not written.
    let mut excluded: HashSet<String> = scan.skipped.iter().map(|(n, _)| n.clone()).collect();
    for name in remote_files.keys() {
        if let Err(why) = files::check_portable(name)
            && excluded.insert(name.clone())
        {
            scan.skipped
                .push((name.clone(), format!("from the bucket: {why}")));
        }
    }
    let included = |n: &str| files::included(n, opts.community_plugins) && !excluded.contains(n);
    let empty = BTreeMap::new();
    let base = match (state, latest) {
        (Some(s), Some(l)) if s.key_check == key.blob_id(b"") && s.seq <= l => &s.files,
        _ => &empty,
    };

    let other = remote_m.as_ref().map_or("unknown", |m| m.device.as_str());
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let plan = merge(base, &local, &remote_files, included, |n| {
        conflict_name(n, other, &stamp)
    });
    if let Some((a, b)) = files::case_collisions(plan.result.keys().map(String::as_str)).first() {
        bail!(
            "{a} and {b} differ only in case, which macOS and Windows cannot keep apart: rename one of them"
        );
    }
    if !opts.allow_mass_delete && too_many_deletions(&plan, local.len()) {
        bail!(
            "this sync would delete {} files here and {} in the bucket; if that is intended, run `obmem sync --allow-deletes`",
            plan.delete_local.len(),
            plan.deleted_remote.len()
        );
    }

    // Upload what the bucket lacks, exactly as hashed: a file that changed since means a new pass.
    let in_bucket: HashSet<&String> = remote_files.values().collect();
    let paths: BTreeMap<&String, &PathBuf> = scan.files.iter().map(|(n, p)| (n, p)).collect();
    let mut sent = HashSet::new();
    for name in &plan.uploaded {
        let blob = &local[name];
        if in_bucket.contains(blob) || !sent.insert(blob.clone()) {
            continue;
        }
        let content = std::fs::read(paths[name]).with_context(|| format!("reading {name}"))?;
        if key.blob_id(&content) != *blob {
            return Ok(None);
        }
        remote.put(&blob_key(blob), key.encrypt(&content)?)?;
    }

    let mut report = SyncReport {
        skipped: scan.skipped,
        ..Default::default()
    };
    report.seq = latest.unwrap_or(0);
    if plan.result != remote_files {
        let seq = latest.map_or(1, |s| s + 1);
        let manifest = Manifest {
            format: FORMAT,
            seq,
            created: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            device: opts.device.to_string(),
            files: plan
                .result
                .iter()
                .map(|(n, b)| {
                    (
                        n.clone(),
                        FileEntry {
                            blob: b.clone(),
                            size: sizes[b],
                        },
                    )
                })
                .collect(),
        };
        if !remote.create(
            &snapshot_key(seq),
            key.encrypt(&serde_json::to_vec(&manifest)?)?,
        )? {
            return Ok(None);
        }
        report.seq = seq;
        report.created = true;
    }

    // The bucket now holds the result: bring the vault in line, leaving alone what changed. The
    // base covers only what this device synced: a name it carried along was never here, and
    // must not count as deleted here once this device starts syncing it.
    let mut new_base: BTreeMap<String, String> = plan
        .result
        .iter()
        .filter(|(n, _)| included(n))
        .map(|(n, b)| (n.clone(), b.clone()))
        .collect();
    new_base.extend(
        base.iter()
            .filter(|(n, _)| !included(n))
            .map(|(n, b)| (n.clone(), b.clone())),
    );
    for (name, blob) in &plan.download {
        if current_blob(key, vault, name)?.as_ref() != local.get(name) {
            defer(&mut report, &mut new_base, base, name);
            continue;
        }
        let cipher = remote
            .get(&blob_key(blob))?
            .ok_or_else(|| anyhow!("the bucket is missing the content of {name}"))?;
        let content = key
            .decrypt(&cipher)
            .with_context(|| format!("decrypting {name}"))?;
        if key.blob_id(&content) != *blob {
            bail!("the content of {name} in the bucket does not match its snapshot entry");
        }
        let root = std::fs::canonicalize(vault)?;
        obmem_core::store::atomic_write_bytes(&root, &path_in(&root, name), &content)?;
        if plan.conflicts.contains(name) {
            report.conflicts.push(name.clone());
        } else {
            report.downloaded.push(name.clone());
        }
    }
    for name in &plan.delete_local {
        if current_blob(key, vault, name)?.as_ref() != local.get(name) {
            defer(&mut report, &mut new_base, base, name);
            continue;
        }
        let path = path_in(vault, name);
        if std::fs::symlink_metadata(&path).is_ok() {
            std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
        }
        report.deleted_local.push(name.clone());
    }
    report.uploaded = plan.uploaded;
    report.deleted_remote = plan.deleted_remote;

    write_state(
        state_file,
        &State {
            key_check: key.blob_id(b""),
            seq: report.seq,
            files: new_base,
        },
    )?;
    Ok(Some(report))
}

/// A file that changed while the sync ran keeps its old base, so the next sync sees what both
/// sides did to it instead of mistaking the bucket's version for one this device already has.
fn defer(
    report: &mut SyncReport,
    new_base: &mut BTreeMap<String, String>,
    base: &BTreeMap<String, String>,
    name: &str,
) {
    match base.get(name) {
        Some(b) => new_base.insert(name.to_string(), b.clone()),
        None => new_base.remove(name),
    };
    report.deferred.push(name.to_string());
}

/// The file a portable name stands for under `root`.
pub(crate) fn path_in(root: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(root.to_path_buf(), |p, part| p.join(part))
}

/// The blob id of the file at `name` right now, `None` if there is none. A symlink counts as
/// something that is not ours to replace.
fn current_blob(key: &VaultKey, vault: &Path, name: &str) -> Result<Option<String>> {
    let path = path_in(vault, name);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::Error::from(e).context(format!("reading {}", path.display()))),
        Ok(m) if m.file_type().is_symlink() => Ok(Some(String::new())),
        Ok(_) => Ok(Some(key.blob_id(&std::fs::read(&path)?))),
    }
}

#[derive(Debug, Default, PartialEq)]
struct Plan {
    /// The new snapshot.
    result: BTreeMap<String, String>,
    /// Bucket content to write here (conflict copies included).
    download: Vec<(String, String)>,
    delete_local: Vec<String>,
    /// Local versions the result takes.
    uploaded: Vec<String>,
    deleted_remote: Vec<String>,
    conflicts: BTreeSet<String>,
}

fn merge(
    base: &BTreeMap<String, String>,
    local: &BTreeMap<String, String>,
    remote: &BTreeMap<String, String>,
    included: impl Fn(&str) -> bool,
    conflict_name: impl Fn(&str) -> String,
) -> Plan {
    let mut p = Plan::default();
    let names: BTreeSet<&String> = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .collect();
    for name in names {
        let (b, l, r) = (base.get(name), local.get(name), remote.get(name));
        if !included(name) {
            // Another device syncs this, this one does not: carry it along untouched.
            if let Some(r) = r {
                p.result.insert(name.clone(), r.clone());
            }
            continue;
        }
        if l == r {
            if let Some(l) = l {
                p.result.insert(name.clone(), l.clone());
            }
        } else if l == b {
            // Only the bucket changed.
            match r {
                Some(r) => {
                    p.result.insert(name.clone(), r.clone());
                    p.download.push((name.clone(), r.clone()));
                }
                None => p.delete_local.push(name.clone()),
            }
        } else if r == b {
            // Only this device changed.
            match l {
                Some(l) => {
                    p.result.insert(name.clone(), l.clone());
                    p.uploaded.push(name.clone());
                }
                None => p.deleted_remote.push(name.clone()),
            }
        } else {
            match (l, r) {
                // Deleted here, edited there: keep the edit.
                (None, Some(r)) => {
                    p.result.insert(name.clone(), r.clone());
                    p.download.push((name.clone(), r.clone()));
                }
                // Edited here, deleted there: keep the edit.
                (Some(l), None) => {
                    p.result.insert(name.clone(), l.clone());
                    p.uploaded.push(name.clone());
                }
                (Some(l), Some(r)) => {
                    p.result.insert(name.clone(), l.clone());
                    p.uploaded.push(name.clone());
                    let copy = conflict_name(name);
                    p.result.insert(copy.clone(), r.clone());
                    p.download.push((copy.clone(), r.clone()));
                    p.conflicts.insert(copy);
                }
                (None, None) => unreachable!("l == r"),
            }
        }
    }
    p
}

/// `Global/a.md` → `Global/a.sync-conflict-<device>-<stamp>.md`.
fn conflict_name(name: &str, device: &str, stamp: &str) -> String {
    let device = obmem_core::note::slugify(device);
    let device = if device.is_empty() {
        "device".to_string()
    } else {
        device
    };
    let (dir, file) = name.rsplit_once('/').map_or(("", name), |(d, f)| (d, f));
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (file, String::new()),
    };
    let file = format!(
        "{stem}{}{device}-{stamp}{ext}",
        obmem_core::audit::SYNC_CONFLICT_MARK
    );
    if dir.is_empty() {
        file
    } else {
        format!("{dir}/{file}")
    }
}

/// A sync that would delete more than half the vault (and more than 10 files) is far more often
/// a mistake, such as an emptied or wrong vault folder, than an intent.
fn too_many_deletions(plan: &Plan, local_files: usize) -> bool {
    let deletions = plan.delete_local.len() + plan.deleted_remote.len();
    deletions > 10 && deletions * 2 > local_files + plan.deleted_remote.len()
}

fn read_state(path: &Path) -> Result<Option<State>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::Error::from(e).context(format!("reading {}", path.display()))),
    }
}

fn write_state(path: &Path, state: &State) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir)?;
    obmem_core::store::atomic_write_bytes(dir, path, &serde_json::to_vec(state)?)
}

/// One sync per vault and bucket at a time on this machine: an OS file lock on a file next to the
/// state, which the OS releases when the holder exits, however it ends. No stale locks to guess.
struct Lock(#[allow(dead_code)] std::fs::File);

impl Lock {
    /// With a zero `wait`, fail at once if a sync is running. Otherwise queue behind it: at most
    /// one sync waits (a second lock, `.queued`), and it starts no sooner than `spacing` after
    /// the last sync started.
    fn acquire(
        state_file: &Path,
        wait: std::time::Duration,
        spacing: std::time::Duration,
    ) -> Result<Self> {
        let path = state_file.with_extension("lock");
        if wait.is_zero() {
            let lock = Self::try_take(&path)?.ok_or_else(|| {
                anyhow!("another obmem sync is running (lock {})", path.display())
            })?;
            lock.mark_start();
            return Ok(lock);
        }
        let Some(_queued) = Self::try_take(&state_file.with_extension("queued"))? else {
            return Err(Queued.into());
        };
        let deadline = std::time::Instant::now() + wait;
        loop {
            let since_last = Self::last_start(&path).and_then(|t| t.elapsed().ok());
            if since_last.is_none_or(|e| e >= spacing)
                && let Some(lock) = Self::try_take(&path)?
            {
                lock.mark_start();
                return Ok(lock);
            }
            if std::time::Instant::now() >= deadline {
                bail!(
                    "gave up after {}s: another obmem sync is running or started too recently (lock {})",
                    wait.as_secs(),
                    path.display()
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    fn try_take(path: &Path) -> Result<Option<Self>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self(file))),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => {
                Err(anyhow::Error::from(e).context(format!("locking {}", path.display())))
            }
        }
    }

    /// The lock file's time is when the last sync started.
    fn mark_start(&self) {
        let _ = self.0.set_modified(std::time::SystemTime::now());
    }

    fn last_start(path: &Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{remote, tempdir, unlock_or_create};
    use age::secrecy::SecretString;

    pub(crate) fn pass(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    pub(crate) fn put(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn read(root: &Path, rel: &str) -> Option<String> {
        std::fs::read_to_string(root.join(rel)).ok()
    }

    pub(crate) struct Device<'a> {
        remote: &'a Remote,
        pub key: VaultKey,
        pub vault: PathBuf,
        state: PathBuf,
        name: &'static str,
        pub plugins: bool,
    }

    impl Device<'_> {
        pub fn sync(&self) -> Result<SyncReport> {
            let opts = SyncOptions {
                device: self.name,
                community_plugins: self.plugins,
                allow_mass_delete: false,
                wait: Default::default(),
                spacing: Default::default(),
            };
            sync(self.remote, &self.key, &self.vault, &self.state, &opts)
        }
    }

    pub(crate) fn device<'a>(remote: &'a Remote, name: &'static str) -> Device<'a> {
        let (key, _) = unlock_or_create(remote, &pass("pw")).unwrap();
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("vault")).unwrap();
        Device {
            remote,
            key,
            vault: dir.join("vault"),
            state: dir.join("state.json"),
            name,
            plugins: false,
        }
    }

    #[test]
    fn changes_travel_both_ways() {
        let remote = remote::in_memory();
        let (mac, win) = (device(&remote, "mac"), device(&remote, "win"));
        put(&mac.vault, "Global/a.md", "a1");
        put(&mac.vault, "Global/b.md", "b1");
        let r = mac.sync().unwrap();
        assert_eq!((r.seq, r.created, r.uploaded.len()), (1, true, 2));

        let r = win.sync().unwrap();
        assert_eq!((r.seq, r.created, r.downloaded.len()), (1, false, 2));
        assert_eq!(read(&win.vault, "Global/a.md").as_deref(), Some("a1"));

        put(&win.vault, "Global/a.md", "a2");
        std::fs::remove_file(win.vault.join("Global/b.md")).unwrap();
        put(&win.vault, "Global/c.md", "c1");
        let r = win.sync().unwrap();
        assert_eq!((r.seq, r.created), (2, true));
        assert_eq!(r.deleted_remote, ["Global/b.md"]);

        let r = mac.sync().unwrap();
        assert_eq!((r.seq, r.created), (2, false));
        assert_eq!(read(&mac.vault, "Global/a.md").as_deref(), Some("a2"));
        assert_eq!(read(&mac.vault, "Global/b.md"), None);
        assert_eq!(read(&mac.vault, "Global/c.md").as_deref(), Some("c1"));
        assert_eq!(r.deleted_local, ["Global/b.md"]);

        // Nothing changed anywhere: no new snapshot.
        let r = mac.sync().unwrap();
        assert_eq!((r.seq, r.created), (2, false));
        assert!(r.uploaded.is_empty() && r.downloaded.is_empty());
    }

    #[test]
    fn edits_on_both_sides_keep_both_versions() {
        let remote = remote::in_memory();
        let (mac, win) = (device(&remote, "mac"), device(&remote, "win"));
        put(&mac.vault, "Global/a.md", "base");
        mac.sync().unwrap();
        win.sync().unwrap();

        put(&mac.vault, "Global/a.md", "mac edit");
        put(&win.vault, "Global/a.md", "win edit");
        mac.sync().unwrap();
        let r = win.sync().unwrap();
        assert_eq!(r.conflicts.len(), 1, "{r:?}");
        let copy = &r.conflicts[0];
        assert!(
            copy.starts_with("Global/a.sync-conflict-mac-") && copy.ends_with(".md"),
            "{copy}"
        );
        assert_eq!(read(&win.vault, "Global/a.md").as_deref(), Some("win edit"));
        assert_eq!(read(&win.vault, copy).as_deref(), Some("mac edit"));

        // The first device gets the result: its own edit as the copy, the other as the note.
        mac.sync().unwrap();
        assert_eq!(read(&mac.vault, "Global/a.md").as_deref(), Some("win edit"));
        assert_eq!(read(&mac.vault, copy).as_deref(), Some("mac edit"));

        // Resolving on either device (deleting the copy) travels too.
        std::fs::remove_file(mac.vault.join(copy)).unwrap();
        mac.sync().unwrap();
        win.sync().unwrap();
        assert_eq!(read(&win.vault, copy), None);
    }

    #[test]
    fn a_deletion_loses_to_an_edit() {
        let remote = remote::in_memory();
        let (mac, win) = (device(&remote, "mac"), device(&remote, "win"));
        put(&mac.vault, "Global/a.md", "base");
        put(&mac.vault, "Global/b.md", "base");
        mac.sync().unwrap();
        win.sync().unwrap();

        std::fs::remove_file(mac.vault.join("Global/a.md")).unwrap();
        put(&mac.vault, "Global/b.md", "mac edit");
        put(&win.vault, "Global/a.md", "win edit");
        std::fs::remove_file(win.vault.join("Global/b.md")).unwrap();
        mac.sync().unwrap();
        win.sync().unwrap();
        mac.sync().unwrap();
        for d in [&mac, &win] {
            assert_eq!(read(&d.vault, "Global/a.md").as_deref(), Some("win edit"));
            assert_eq!(read(&d.vault, "Global/b.md").as_deref(), Some("mac edit"));
        }
    }

    #[test]
    fn a_new_device_with_its_own_files_merges_instead_of_replacing() {
        let remote = remote::in_memory();
        let (mac, win) = (device(&remote, "mac"), device(&remote, "win"));
        put(&mac.vault, "Global/a.md", "from mac");
        put(&mac.vault, "Global/same.md", "same");
        mac.sync().unwrap();
        put(&win.vault, "Global/w.md", "from win");
        put(&win.vault, "Global/same.md", "same");
        let r = win.sync().unwrap();
        assert!(r.conflicts.is_empty(), "{r:?}");
        mac.sync().unwrap();
        for d in [&mac, &win] {
            assert_eq!(read(&d.vault, "Global/a.md").as_deref(), Some("from mac"));
            assert_eq!(read(&d.vault, "Global/w.md").as_deref(), Some("from win"));
        }
    }

    #[test]
    fn files_one_device_does_not_sync_are_left_alone() {
        let remote = remote::in_memory();
        let (mut mac, win) = (device(&remote, "mac"), device(&remote, "win"));
        mac.plugins = true;
        put(&mac.vault, ".obsidian/plugins/x/main.js", "code");
        put(&mac.vault, "Global/a.md", "a");
        mac.sync().unwrap();

        win.sync().unwrap();
        assert_eq!(read(&win.vault, ".obsidian/plugins/x/main.js"), None);
        put(&win.vault, "Global/a.md", "a2");
        win.sync().unwrap();

        let m = load_snapshot(&remote, &mac.key, 2).unwrap();
        assert!(
            m.files.contains_key(".obsidian/plugins/x/main.js"),
            "the plugin stays in the bucket"
        );
        mac.sync().unwrap();
        assert_eq!(
            read(&mac.vault, ".obsidian/plugins/x/main.js").as_deref(),
            Some("code")
        );
    }

    #[test]
    fn turning_plugins_on_later_downloads_them_instead_of_deleting_them() {
        let remote = remote::in_memory();
        let (mut mac, mut win) = (device(&remote, "mac"), device(&remote, "win"));
        mac.plugins = true;
        put(&mac.vault, ".obsidian/plugins/x/main.js", "code");
        mac.sync().unwrap();
        win.sync().unwrap();

        win.plugins = true;
        let r = win.sync().unwrap();
        assert!(r.deleted_remote.is_empty(), "{r:?}");
        assert_eq!(
            read(&win.vault, ".obsidian/plugins/x/main.js").as_deref(),
            Some("code")
        );
        mac.sync().unwrap();
        assert_eq!(
            read(&mac.vault, ".obsidian/plugins/x/main.js").as_deref(),
            Some("code")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_the_scan_skips_is_not_a_deletion() {
        let remote = remote::in_memory();
        let (mac, win) = (device(&remote, "mac"), device(&remote, "win"));
        put(&mac.vault, "Global/a.md", "a");
        mac.sync().unwrap();
        win.sync().unwrap();
        // The file becomes something the scan skips (here a symlink; on a case-sensitive file
        // system, a name that differs from another only in case).
        std::fs::remove_file(mac.vault.join("Global/a.md")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", mac.vault.join("Global/a.md")).unwrap();
        let r = mac.sync().unwrap();
        assert!(r.deleted_remote.is_empty(), "{r:?}");
        win.sync().unwrap();
        assert_eq!(read(&win.vault, "Global/a.md").as_deref(), Some("a"));
    }

    #[test]
    fn names_from_the_bucket_this_file_system_cannot_hold_are_not_written() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "a");
        mac.sync().unwrap();
        // A snapshot from another client holds a name Windows cannot hold.
        let mut m = load_snapshot(&remote, &mac.key, 1).unwrap();
        let entry = m.files["Global/a.md"].clone();
        m.files.insert("Global/b:c.md".into(), entry);
        m.seq = 2;
        remote
            .create(
                &snapshot_key(2),
                mac.key.encrypt(&serde_json::to_vec(&m).unwrap()).unwrap(),
            )
            .unwrap();

        let r = mac.sync().unwrap();
        assert!(r.skipped.iter().any(|(n, _)| n == "Global/b:c.md"), "{r:?}");
        assert!(!r.downloaded.contains(&"Global/b:c.md".to_string()));
        assert!(!mac.vault.join("Global/b:c.md").exists());
        assert!(
            load_snapshot(&remote, &mac.key, 2)
                .unwrap()
                .files
                .contains_key("Global/b:c.md"),
            "left in the bucket"
        );
    }

    #[test]
    fn an_emptied_vault_does_not_wipe_the_bucket() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        for i in 0..20 {
            put(&mac.vault, &format!("Global/n{i}.md"), &format!("{i}"));
        }
        mac.sync().unwrap();
        std::fs::remove_dir_all(mac.vault.join("Global")).unwrap();
        let err = mac.sync().unwrap_err();
        assert!(err.to_string().contains("--allow-deletes"), "{err}");
        assert_eq!(snapshot_seqs(&remote).unwrap(), [1]);

        let opts = SyncOptions {
            device: "mac",
            community_plugins: false,
            allow_mass_delete: true,
            wait: Default::default(),
            spacing: Default::default(),
        };
        let r = sync(&remote, &mac.key, &mac.vault, &mac.state, &opts).unwrap();
        assert_eq!(r.deleted_remote.len(), 20);
    }

    #[test]
    fn a_base_from_another_key_is_ignored() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        put(&mac.vault, "Global/a.md", "a");
        mac.sync().unwrap();

        // The bucket is recreated (new key) while this device still remembers the old one.
        let fresh = remote::in_memory();
        let (key, _) = unlock_or_create(&fresh, &pass("other")).unwrap();
        let opts = SyncOptions {
            device: "mac",
            community_plugins: false,
            allow_mass_delete: false,
            wait: Default::default(),
            spacing: Default::default(),
        };
        let r = sync(&fresh, &key, &mac.vault, &mac.state, &opts).unwrap();
        assert_eq!(r.uploaded, ["Global/a.md"]);
        assert_eq!(read(&mac.vault, "Global/a.md").as_deref(), Some("a"));
    }

    #[test]
    fn a_second_sync_at_the_same_time_is_refused() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        let held = Lock::acquire(&mac.state, Default::default(), Default::default()).unwrap();
        let err = mac.sync().unwrap_err();
        assert!(
            err.to_string().contains("another obmem sync is running"),
            "{err}"
        );

        // Waiting: the first waiter queues, a second one leaves it to the first.
        let opts = |wait| SyncOptions {
            device: "mac",
            community_plugins: false,
            allow_mass_delete: false,
            wait,
            spacing: MIN_INTERVAL,
        };
        let (remote2, key, vault, state) =
            (&remote, &mac.key, mac.vault.clone(), mac.state.clone());
        // The running sync started long enough ago for a waiter to go right after it.
        held.0
            .set_modified(std::time::SystemTime::now() - 2 * MIN_INTERVAL)
            .unwrap();
        std::thread::scope(|s| {
            let first = s.spawn(|| {
                sync(
                    remote2,
                    key,
                    &vault,
                    &state,
                    &opts(std::time::Duration::from_secs(20)),
                )
            });
            while !state.with_extension("queued").exists() {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            let second = sync(
                remote2,
                key,
                &vault,
                &state,
                &opts(std::time::Duration::from_secs(20)),
            )
            .unwrap_err();
            assert!(second.downcast_ref::<Queued>().is_some(), "{second}");
            drop(held);
            assert!(first.join().unwrap().is_ok());
        });
        // Every lock is free again.
        assert!(mac.sync().is_ok());
    }

    #[test]
    fn a_waiting_sync_keeps_its_distance_from_the_last_one() {
        let remote = remote::in_memory();
        let mac = device(&remote, "mac");
        mac.sync().unwrap(); // just started one
        let opts = SyncOptions {
            device: "mac",
            community_plugins: false,
            allow_mass_delete: false,
            wait: std::time::Duration::from_secs(1),
            spacing: MIN_INTERVAL,
        };
        let err = sync(&remote, &mac.key, &mac.vault, &mac.state, &opts).unwrap_err();
        assert!(err.to_string().contains("gave up"), "{err}");
        // Without spacing, a waiting sync runs as soon as no other does.
        let at_once = SyncOptions {
            spacing: std::time::Duration::ZERO,
            ..opts
        };
        assert!(sync(&remote, &mac.key, &mac.vault, &mac.state, &at_once).is_ok());
    }

    #[test]
    fn merge_table() {
        let m = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let base = m(&[
            ("same", "1"),
            ("here", "1"),
            ("there", "1"),
            ("both", "1"),
            ("gone-here", "1"),
            ("gone-there", "1"),
        ]);
        let local = m(&[
            ("same", "1"),
            ("here", "2"),
            ("there", "1"),
            ("both", "2"),
            ("gone-there", "1"),
            ("new", "1"),
        ]);
        let remote = m(&[
            ("same", "1"),
            ("here", "1"),
            ("there", "2"),
            ("both", "3"),
            ("gone-here", "1"),
        ]);
        let p = merge(&base, &local, &remote, |_| true, |n| format!("{n}.copy"));
        assert_eq!(
            p.result,
            m(&[
                ("same", "1"),
                ("here", "2"),
                ("there", "2"),
                ("both", "2"),
                ("both.copy", "3"),
                ("new", "1")
            ])
        );
        assert_eq!(
            p.download,
            [
                ("both.copy".to_string(), "3".to_string()),
                ("there".to_string(), "2".to_string())
            ]
        );
        assert_eq!(p.delete_local, ["gone-there"]);
        assert_eq!(p.deleted_remote, ["gone-here"]);
        assert_eq!(p.uploaded, ["both", "here", "new"]);
    }

    #[test]
    fn conflict_names() {
        assert_eq!(
            conflict_name("Global/a.md", "Work PC", "20261109-101500"),
            "Global/a.sync-conflict-work-pc-20261109-101500.md"
        );
        assert_eq!(
            conflict_name("notes", "mac", "t"),
            "notes.sync-conflict-mac-t"
        );
        assert_eq!(
            conflict_name(".obsidian/app.json", "mac", "t"),
            ".obsidian/app.sync-conflict-mac-t.json"
        );
    }
}
