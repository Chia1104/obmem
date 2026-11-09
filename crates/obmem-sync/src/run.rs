//! A sync as the front ends run it: the configured bucket, the keychain's credentials, the
//! personal vault, and the outcome kept for the status line (`obmem status`, the desktop app).

use crate::sync::{Queued, SyncOptions, SyncReport, state_path, sync};
use crate::{Remote, keychain};
use anyhow::Result;
use obmem_core::config::{self, SyncConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How a sync started by an agent hook waits for one already running.
pub const BACKGROUND_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// The last sync's outcome on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// RFC 3339, local time.
    pub at: String,
    pub ok: bool,
    /// What happened, or why it failed.
    pub message: String,
}

/// Sync the personal vault now. `Ok(None)`: another sync is already queued and will take these
/// changes too.
pub fn run(cfg: &SyncConfig, allow_mass_delete: bool, wait: std::time::Duration) -> Result<Option<SyncReport>> {
    let vault = config::global_vault()?;
    let state = state_path(cfg, &vault);
    let result = (|| {
        let (creds, key) = keychain::load(cfg)?;
        let remote = Remote::s3(cfg, &creds)?;
        let device = config::device_name(cfg);
        let opts = SyncOptions { device: &device, community_plugins: cfg.community_plugins.unwrap_or(false), allow_mass_delete, wait };
        sync(&remote, &key, &vault, &state, &opts)
    })();
    match result {
        Err(e) if e.downcast_ref::<Queued>().is_some() => Ok(None),
        result => {
            let status = match &result {
                Ok(r) => Status { at: now(), ok: true, message: summary(r) },
                Err(e) => Status { at: now(), ok: false, message: format!("{e:#}") },
            };
            if let Err(e) = write_status(&status_path(&state), &status) {
                tracing::warn!("could not record the sync status: {e:#}");
            }
            result.map(Some)
        }
    }
}

/// Connect this device to the configured bucket: unlock its key with the passphrase, or create
/// the key when the bucket holds none yet, and keep both in the keychain. `true`: created.
pub fn connect(cfg: &SyncConfig, creds: crate::Credentials, passphrase: &age::secrecy::SecretString) -> Result<bool> {
    let remote = Remote::s3(cfg, &creds)?;
    let (key, created) = crate::unlock_or_create(&remote, passphrase)?;
    keychain::save(cfg, &creds, &key)?;
    Ok(created)
}

/// The last sync's outcome for the configured bucket and personal vault, if there was one.
pub fn last_status(cfg: &SyncConfig) -> Option<Status> {
    let vault = config::global_vault().ok()?;
    let bytes = std::fs::read(status_path(&state_path(cfg, &vault))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// One line: `snapshot 12: 3 up, 1 down, 1 conflict`.
pub fn summary(r: &SyncReport) -> String {
    let mut parts = Vec::new();
    for (n, label) in [
        (r.uploaded.len() + r.deleted_remote.len(), "up"),
        (r.downloaded.len() + r.deleted_local.len(), "down"),
        (r.conflicts.len(), if r.conflicts.len() == 1 { "conflict" } else { "conflicts" }),
        (r.deferred.len(), "left for the next sync"),
        (r.skipped.len(), "skipped"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    let head = match (r.seq, r.created) {
        (0, _) => "nothing to sync yet".to_string(),
        (seq, true) => format!("snapshot {seq}"),
        (seq, false) => format!("up to date with snapshot {seq}"),
    };
    if parts.is_empty() { head } else { format!("{head}: {}", parts.join(", ")) }
}

fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn status_path(state: &Path) -> PathBuf {
    state.with_extension("status.json")
}

fn write_status(path: &Path, status: &Status) -> Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir)?;
    obmem_core::store::atomic_write_bytes(dir, path, &serde_json::to_vec(status)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let r = SyncReport { seq: 3, created: true, uploaded: vec!["a".into()], conflicts: vec!["b".into()], ..Default::default() };
        assert_eq!(summary(&r), "snapshot 3: 1 up, 1 conflict");
        assert_eq!(summary(&SyncReport { seq: 3, ..Default::default() }), "up to date with snapshot 3");
        assert_eq!(summary(&SyncReport::default()), "nothing to sync yet");
    }
}
