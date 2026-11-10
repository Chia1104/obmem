//! A sync as the front ends run it: the configured bucket, this machine's credentials, the
//! personal vault, and the outcome kept for the status line (`obmem status`, the desktop app).

use crate::sync::{MIN_INTERVAL, Queued, SyncOptions, SyncReport, state_path, sync};
use crate::{Remote, keychain};
use anyhow::Result;
use obmem_core::config::{self, SyncConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How long a sync started by an agent hook waits for one already running.
pub const HOOK_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

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
/// changes too. A sync that waits (`wait` above zero) keeps `MIN_INTERVAL` from the last one,
/// except on an ephemeral machine, where a hook waits for it to finish.
pub fn run(
    cfg: &SyncConfig,
    allow_mass_delete: bool,
    wait: std::time::Duration,
) -> Result<Option<SyncReport>> {
    let vault = config::global_vault()?;
    let state = state_path(cfg, &vault);
    let result = (|| {
        let (creds, key) = keychain::load(cfg)?;
        let remote = Remote::s3(cfg, &creds)?;
        if cfg.is_ephemeral() {
            create_new_vault(&vault, &state)?;
        }
        let device = config::device_name(cfg);
        let opts = SyncOptions {
            device: &device,
            community_plugins: cfg.community_plugins.unwrap_or(false),
            allow_mass_delete,
            wait,
            spacing: if cfg.is_ephemeral() {
                std::time::Duration::ZERO
            } else {
                MIN_INTERVAL
            },
        };
        let report = sync(&remote, &key, &vault, &state, &opts)?;
        // An ephemeral machine has no record of its last prune, so it would prune every session.
        let cleanup = if cfg.is_ephemeral() {
            String::new()
        } else {
            prune_if_due(&remote, &key, &state)
        };
        Ok::<_, anyhow::Error>((report, cleanup))
    })();
    match result {
        Err(e) if e.downcast_ref::<Queued>().is_some() => Ok(None),
        result => {
            let status = match &result {
                Ok((r, cleanup)) => Status {
                    at: now(),
                    ok: true,
                    message: format!("{}{}", summary(r), cleanup),
                },
                Err(e) => Status {
                    at: now(),
                    ok: false,
                    message: format!("{e:#}"),
                },
            };
            if let Err(e) = write_status(&status_path(&state), &status) {
                tracing::warn!("could not record the sync status: {e:#}");
            }
            result.map(|(r, _)| Some(r))
        }
    }
}

/// On an ephemeral machine the personal vault does not exist until the first sync fills it.
/// Only a machine that has never synced gets the folder: with a sync state, a missing vault is a
/// lost one, and a sync into a new, empty folder would delete everything in the bucket.
fn create_new_vault(vault: &Path, state: &Path) -> Result<()> {
    if !vault.exists() && !state.exists() {
        std::fs::create_dir_all(vault)?;
    }
    Ok(())
}

/// How often a device prunes after a sync.
const PRUNE_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Prune once a day per device, after a sync. A failed prune does not fail the sync; the status
/// says what happened (empty when nothing worth saying did).
fn prune_if_due(remote: &Remote, key: &crate::VaultKey, state: &Path) -> String {
    let marker = state.with_extension("pruned");
    let due = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age >= PRUNE_EVERY);
    if !due {
        return String::new();
    }
    match crate::prune(remote, key, chrono::Utc::now(), false) {
        Ok(p) => {
            if let Err(e) = std::fs::write(&marker, b"") {
                tracing::warn!("could not record the prune: {e}");
            }
            if p.deleted_snapshots.is_empty() && p.deleted_blobs == 0 {
                String::new()
            } else {
                format!(
                    "; removed {} old snapshots and {} unused blobs",
                    p.deleted_snapshots.len(),
                    p.deleted_blobs
                )
            }
        }
        Err(e) => format!("; cleaning up old snapshots failed: {e:#}"),
    }
}

/// Connect this device to the configured bucket: unlock its key with the passphrase, or create
/// the key when the bucket holds none yet, and keep both in the keychain. Without `[sync] device`,
/// give the device a name of its own ([`config::new_device_name`]). `true`: created.
pub fn connect(
    cfg: &SyncConfig,
    creds: crate::Credentials,
    passphrase: &age::secrecy::SecretString,
) -> Result<bool> {
    let remote = Remote::s3(cfg, &creds)?;
    let (key, created) = crate::unlock_or_create(&remote, passphrase)?;
    keychain::save(cfg, &creds, &key)?;
    if cfg.device.as_deref().is_none_or(|d| d.trim().is_empty()) {
        obmem_core::settings::set_sync_device(&config::new_device_name())?;
    }
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
        (
            r.conflicts.len(),
            if r.conflicts.len() == 1 {
                "conflict"
            } else {
                "conflicts"
            },
        ),
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
    if parts.is_empty() {
        head
    } else {
        format!("{head}: {}", parts.join(", "))
    }
}

fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn status_path(state: &Path) -> PathBuf {
    state.with_extension("status.json")
}

fn write_status(path: &Path, status: &Status) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir)?;
    obmem_core::store::atomic_write_bytes(dir, path, &serde_json::to_vec(status)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_machine_that_never_synced_gets_a_new_vault() {
        let dir = std::env::temp_dir().join(format!("obmem-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (vault, state) = (dir.join("vault"), dir.join("state.json"));
        create_new_vault(&vault, &state).unwrap();
        assert!(vault.is_dir());
        // it has synced before and the vault is gone: left for the sync to refuse
        std::fs::remove_dir(&vault).unwrap();
        std::fs::write(&state, "{}").unwrap();
        create_new_vault(&vault, &state).unwrap();
        assert!(!vault.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summaries() {
        let r = SyncReport {
            seq: 3,
            created: true,
            uploaded: vec!["a".into()],
            conflicts: vec!["b".into()],
            ..Default::default()
        };
        assert_eq!(summary(&r), "snapshot 3: 1 up, 1 conflict");
        assert_eq!(
            summary(&SyncReport {
                seq: 3,
                ..Default::default()
            }),
            "up to date with snapshot 3"
        );
        assert_eq!(summary(&SyncReport::default()), "nothing to sync yet");
    }
}
