//! A sync as the front ends run it: a vault and its bucket ([`Target`]), this machine's
//! credentials, and the outcome kept for the status line (`obmem status`, the desktop app).

use crate::files::AppFiles;
use crate::remote::location;
use crate::sync::{MIN_INTERVAL, Queued, SyncOptions, SyncReport, state_path, sync};
use crate::{Credentials, Remote, VaultKey, keychain};
use age::secrecy::ExposeSecret;
use anyhow::{Context, Result, bail};
use obmem_core::config::{self, Config, SyncConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How long a sync started by an agent hook waits for one already running.
pub const HOOK_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// What one sync carries: a vault, through the bucket it is kept in. The personal vault is one,
/// and so is each project vault that is private and has a bucket of its own.
#[derive(Debug, Clone)]
pub struct Target {
    pub vault: PathBuf,
    /// The bucket, with this machine's `[sync]` settings.
    pub cfg: SyncConfig,
    kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Personal,
    Project,
}

impl Target {
    /// The personal vault, through `[sync]`.
    pub fn personal(cfg: SyncConfig) -> Result<Self> {
        Ok(Target {
            vault: config::global_vault()?,
            cfg,
            kind: Kind::Personal,
        })
    }

    /// The private vault of `project`, through the bucket the user's config names for it
    /// (`[project.sync]`). `None`: the vault is in the repo, where git carries it, or the config
    /// names no bucket.
    pub fn project(project: &Config) -> Result<Option<Self>> {
        if !project.vault_is_private() {
            return Ok(None);
        }
        let Some(cfg) = config::project_sync(&project.project_root)? else {
            return Ok(None);
        };
        apart(&config::sync_config()?, &cfg)?;
        Ok(Some(Target {
            vault: project.project_vault.clone(),
            cfg,
            kind: Kind::Project,
        }))
    }

    pub fn is_project(&self) -> bool {
        self.kind == Kind::Project
    }

    /// The command that sets this device up for the target.
    pub fn setup_command(&self) -> &'static str {
        match self.kind {
            Kind::Personal => "obmem sync setup",
            Kind::Project => "obmem sync setup --vault project",
        }
    }

    /// This device's access key and vault key for the target. The environment's credentials are
    /// the personal vault's: a project vault's come from the keychain only, or its notes would be
    /// written to the team's bucket under the wrong key.
    pub fn keys(&self) -> Result<(Credentials, VaultKey)> {
        match self.kind {
            Kind::Personal => keychain::load(&self.cfg),
            Kind::Project => {
                let stored = keychain::stored(&self.cfg, self.setup_command());
                if keychain::in_environment() {
                    return stored.context(
                        "a project vault's keys come from this device's keychain: the \
                         OBMEM_SYNC_* variables hold the personal vault's",
                    );
                }
                stored
            }
        }
    }

    /// The vault key as text. For the personal vault it goes in `OBMEM_SYNC_VAULT_KEY` on a
    /// machine without a keychain. Whoever holds it and can read the bucket can read the backup.
    pub fn key_text(&self) -> Result<String> {
        let (_, key) = self.keys()?;
        Ok(key.to_secret().expose_secret().to_string())
    }

    fn app_files(&self) -> AppFiles {
        match self.kind {
            Kind::Project => AppFiles::None,
            Kind::Personal if self.cfg.community_plugins.unwrap_or(false) => AppFiles::WithPlugins,
            Kind::Personal => AppFiles::Settings,
        }
    }

    fn state(&self) -> PathBuf {
        state_path(&self.cfg, &self.vault)
    }
}

/// A project vault has a backup of its own. In the personal vault's bucket and prefix it would
/// share that backup's key and snapshots, and each sync would copy one vault's notes into the
/// other.
fn apart(personal: &SyncConfig, project: &SyncConfig) -> Result<()> {
    if personal.bucket.is_some() && location(personal) == location(project) {
        bail!(
            "the project vault's bucket and prefix ({}) are the personal vault's: give \
             `[project.sync]` a `prefix` of its own",
            location(project)
        );
    }
    Ok(())
}

/// The last sync on this machine: when, and how it went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// RFC 3339, local time.
    pub at: String,
    pub outcome: Outcome,
}

/// How a sync went. Written out (`Display`) it is the status line's text; a front end in another
/// language says it in its own words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Synced {
        summary: Summary,
        cleanup: Cleanup,
    },
    /// Why it failed.
    Failed(String),
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Synced { summary, cleanup } => write!(f, "{summary}{cleanup}"),
            Outcome::Failed(why) => f.write_str(why),
        }
    }
}

/// What a sync did, in numbers. Written out (`Display`) it is one line:
/// `snapshot 12: 3 up, 1 down, 1 conflict`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Summary {
    /// The snapshot the vault now matches; 0 when the bucket and the vault are both empty.
    pub seq: u64,
    /// `false`: the bucket already matched, no new snapshot was made.
    pub created: bool,
    /// Files sent to the bucket or deleted there.
    pub up: usize,
    /// Files fetched from the bucket or deleted here.
    pub down: usize,
    pub conflicts: usize,
    /// Files that changed while the sync ran; the next sync takes them.
    pub deferred: usize,
    /// Files this device cannot sync.
    pub skipped: usize,
}

impl From<&SyncReport> for Summary {
    fn from(r: &SyncReport) -> Self {
        Summary {
            seq: r.seq,
            created: r.created,
            up: r.uploaded.len() + r.deleted_remote.len(),
            down: r.downloaded.len() + r.deleted_local.len(),
            conflicts: r.conflicts.len(),
            deferred: r.deferred.len(),
            skipped: r.skipped.len(),
        }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.seq, self.created) {
            (0, _) => f.write_str("nothing to sync yet")?,
            (seq, true) => write!(f, "snapshot {seq}")?,
            (seq, false) => write!(f, "up to date with snapshot {seq}")?,
        }
        let conflicts = if self.conflicts == 1 {
            "conflict"
        } else {
            "conflicts"
        };
        let parts = [
            (self.up, "up"),
            (self.down, "down"),
            (self.conflicts, conflicts),
            (self.deferred, "left for the next sync"),
            (self.skipped, "skipped"),
        ];
        let mut sep = ": ";
        for (n, label) in parts {
            if n > 0 {
                write!(f, "{sep}{n} {label}")?;
                sep = ", ";
            }
        }
        Ok(())
    }
}

/// What the cleanup after a sync did. Written out (`Display`) it continues the summary's line,
/// and is empty when there is nothing worth saying.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cleanup {
    /// Not due, or nothing to remove.
    #[default]
    Nothing,
    Removed {
        snapshots: usize,
        blobs: usize,
    },
    /// Why it failed. The sync itself still counts.
    Failed(String),
}

impl std::fmt::Display for Cleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cleanup::Nothing => Ok(()),
            Cleanup::Removed { snapshots, blobs } => write!(
                f,
                "; removed {snapshots} old snapshots and {blobs} unused blobs"
            ),
            Cleanup::Failed(why) => write!(f, "; cleaning up old snapshots failed: {why}"),
        }
    }
}

/// Sync `target` now. `Ok(None)`: another sync is already queued and will take these changes
/// too. A sync that waits (`wait` above zero) keeps `MIN_INTERVAL` from the last one, except on
/// an ephemeral machine, where a hook waits for it to finish.
pub fn run(
    target: &Target,
    allow_mass_delete: bool,
    wait: std::time::Duration,
) -> Result<Option<SyncReport>> {
    let (cfg, vault) = (&target.cfg, &target.vault);
    let state = target.state();
    let result = (|| {
        let (creds, key) = target.keys()?;
        let remote = Remote::s3(cfg, &creds)?;
        if cfg.is_ephemeral() {
            create_new_vault(vault, &state)?;
        }
        let device = config::device_name(cfg);
        let opts = SyncOptions {
            device: &device,
            app_files: target.app_files(),
            allow_mass_delete,
            wait,
            spacing: if cfg.is_ephemeral() {
                std::time::Duration::ZERO
            } else {
                MIN_INTERVAL
            },
        };
        let report = sync(&remote, &key, vault, &state, &opts)?;
        // An ephemeral machine has no record of its last prune, so it would prune every session.
        let cleanup = if cfg.is_ephemeral() {
            Cleanup::Nothing
        } else {
            prune_if_due(&remote, &key, &state)
        };
        Ok::<_, anyhow::Error>((report, cleanup))
    })();
    match result {
        Err(e) if e.downcast_ref::<Queued>().is_some() => Ok(None),
        result => {
            let status = Status {
                at: now(),
                outcome: match &result {
                    Ok((r, cleanup)) => Outcome::Synced {
                        summary: r.into(),
                        cleanup: cleanup.clone(),
                    },
                    Err(e) => Outcome::Failed(format!("{e:#}")),
                },
            };
            if let Err(e) = write_status(&status_path(&state), &status) {
                tracing::warn!("could not record the sync status: {e:#}");
            }
            result.map(|(r, _)| Some(r))
        }
    }
}

/// On an ephemeral machine a vault does not exist until the first sync fills it.
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
/// says what happened.
fn prune_if_due(remote: &Remote, key: &crate::VaultKey, state: &Path) -> Cleanup {
    let marker = state.with_extension("pruned");
    let due = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age >= PRUNE_EVERY);
    if !due {
        return Cleanup::Nothing;
    }
    match crate::prune(remote, key, chrono::Utc::now(), false) {
        Ok(p) => {
            if let Err(e) = std::fs::write(&marker, b"") {
                tracing::warn!("could not record the prune: {e}");
            }
            if p.deleted_snapshots.is_empty() && p.deleted_blobs == 0 {
                Cleanup::Nothing
            } else {
                Cleanup::Removed {
                    snapshots: p.deleted_snapshots.len(),
                    blobs: p.deleted_blobs,
                }
            }
        }
        Err(e) => Cleanup::Failed(format!("{e:#}")),
    }
}

/// Connect this device to the target's bucket: unlock its key with the passphrase, or create
/// the key when the bucket holds none yet, and keep both in the keychain. Without `[sync] device`,
/// give the device a name of its own ([`config::new_device_name`]). `true`: created.
pub fn connect(
    target: &Target,
    creds: crate::Credentials,
    passphrase: &age::secrecy::SecretString,
) -> Result<bool> {
    let cfg = &target.cfg;
    let remote = Remote::s3(cfg, &creds)?;
    let (key, created) = crate::unlock_or_create(&remote, passphrase)?;
    keychain::save(cfg, &creds, &key)?;
    if cfg.device.as_deref().is_none_or(|d| d.trim().is_empty()) {
        obmem_core::settings::set_sync_device(&config::new_device_name())?;
    }
    Ok(created)
}

/// The last sync's outcome for the target, if there was one.
pub fn last_status(target: &Target) -> Option<Status> {
    let bytes = std::fs::read(status_path(&target.state())).ok()?;
    serde_json::from_slice(&bytes).ok()
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
    fn a_project_vault_has_a_backup_of_its_own() {
        let at = |bucket: &str, prefix: Option<&str>| SyncConfig {
            bucket: Some(bucket.into()),
            prefix: prefix.map(String::from),
            ..Default::default()
        };
        let personal = at("mine", None);
        assert!(apart(&personal, &at("mine", Some("acme"))).is_ok());
        assert!(apart(&personal, &at("team", None)).is_ok());
        // The same bucket with no prefix is the personal backup itself, as is its default prefix.
        for same in [None, Some(crate::remote::DEFAULT_PREFIX)] {
            let err = apart(&personal, &at("mine", same)).unwrap_err().to_string();
            assert!(err.contains("prefix"), "{err}");
        }
        // Without a personal backup there is nothing to collide with.
        assert!(apart(&SyncConfig::default(), &at("mine", None)).is_ok());

        // A team's vault carries none of the app's settings, whatever this machine syncs of its own.
        let target = |kind| Target {
            vault: PathBuf::from("vault"),
            cfg: SyncConfig {
                community_plugins: Some(true),
                ..at("team", None)
            },
            kind,
        };
        assert_eq!(target(Kind::Project).app_files(), AppFiles::None);
        assert_eq!(target(Kind::Personal).app_files(), AppFiles::WithPlugins);
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
        let summary = |r: &SyncReport| Summary::from(r).to_string();
        assert_eq!(summary(&r), "snapshot 3: 1 up, 1 conflict");
        assert_eq!(
            summary(&SyncReport {
                seq: 3,
                ..Default::default()
            }),
            "up to date with snapshot 3"
        );
        assert_eq!(summary(&SyncReport::default()), "nothing to sync yet");
        let status = Status {
            at: "2026-11-12T10:00:00+08:00".into(),
            outcome: Outcome::Synced {
                summary: (&r).into(),
                cleanup: Cleanup::Removed {
                    snapshots: 2,
                    blobs: 5,
                },
            },
        };
        assert_eq!(
            status.outcome.to_string(),
            "snapshot 3: 1 up, 1 conflict; removed 2 old snapshots and 5 unused blobs"
        );
        // The status file keeps the numbers, so a front end can say them in its own language.
        let kept: Status = serde_json::from_slice(&serde_json::to_vec(&status).unwrap()).unwrap();
        assert_eq!(kept, status);
    }
}
