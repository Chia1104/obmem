//! Configuration: where the two vaults live and how writes are policed.
//!
//! - Project vault: inside the repo (default `docs/kb`), shared with the team via git.
//! - Global vault:  a personal vault (default `~/Obsidian/AgentMemory`) for preferences,
//!   cross-project feedback and noisy session logs that should not land in the repo.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_CONFIG_FILE: &str = ".obmem.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectFile {
    /// Vault directory, relative to the project root.
    pub vault: Option<String>,
    /// Project name used in the global vault (`Projects/<name>/`).
    pub name: Option<String>,
    pub secrets: SecretsConfig,
    pub audit: AuditConfig,
    pub review: ReviewConfig,
}

/// `[review]`: whether agent writes skip the review queue. Only the user's own config can turn
/// auto-approve on; a repo's `.obmem.toml` arrives with a clone and can only turn it off.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ReviewConfig {
    /// Notes are written `active` instead of `draft`, as if a person had approved them.
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    /// Active knowledge notes not verified/updated for this many days are reported as stale.
    pub stale_days: i64,
    /// Title similarity (trigram Jaccard) at or above which two notes count as duplicates.
    pub duplicate_threshold: f64,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self { stale_days: 180, duplicate_threshold: 0.75 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GlobalFile {
    pub vault: Option<String>,
    pub secrets: GlobalSecrets,
    pub review: ReviewConfig,
    pub sync: SyncConfig,
}

/// `[sync]`: where the personal vault is backed up, an S3-compatible bucket (AWS S3, Cloudflare R2,
/// MinIO). Only the user's config has it. Credentials and the vault key live in the OS keychain,
/// never in this file.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct SyncConfig {
    /// S3 API endpoint, e.g. `https://<account>.r2.cloudflarestorage.com`. Unset: AWS S3.
    pub endpoint: Option<String>,
    /// Bucket region. Unset: `auto` (what R2 expects); AWS needs the real region.
    pub region: Option<String>,
    pub bucket: Option<String>,
    /// Key prefix inside the bucket, so one bucket can hold more than one vault. Unset: `obmem`.
    pub prefix: Option<String>,
    /// This machine's name in snapshots. Unset: the host name.
    pub device: Option<String>,
    /// Also back up `.obsidian/plugins/` and `community-plugins.json`.
    pub community_plugins: Option<bool>,
}

/// Secret-scanning switches that may only come from the user's own config: a repo's
/// `.obmem.toml` arrives with a clone and must not be able to weaken the scanner.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GlobalSecrets {
    /// Set to false to stop treating sensitive-looking environment variables as known secrets.
    pub scan_process_env: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecretMode {
    /// Refuse the write and tell the agent what to remove (default).
    #[default]
    Block,
    /// Replace the secret with `[REDACTED:<rule>]` and write anyway.
    Redact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretsConfig {
    pub mode: SecretMode,
    /// Files (relative to project root) whose values are treated as known secrets.
    pub env_files: Vec<String>,
    /// Also treat values of sensitive-looking environment variables as known secrets.
    pub scan_process_env: bool,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            mode: SecretMode::Block,
            env_files: vec![
                ".env".into(),
                ".env.local".into(),
                ".env.development".into(),
                ".env.production".into(),
            ],
            scan_process_env: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub project_root: PathBuf,
    pub project_name: String,
    pub project_vault: PathBuf,
    pub global_vault: PathBuf,
    pub secrets: SecretsConfig,
    pub audit: AuditConfig,
    /// Writes land `active` instead of `draft` (see [`auto_approve`]).
    pub auto_approve: bool,
    /// This machine's name (see [`device_name`]).
    pub device: String,
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else if p == "~" {
        home()
    } else {
        PathBuf::from(p)
    }
}

/// `~/.config/obmem/config.toml` (XDG style on every OS, easier to find than
/// `~/Library/Application Support`).
pub fn global_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("OBMEM_CONFIG") {
        return PathBuf::from(p);
    }
    home().join(".config/obmem/config.toml")
}

/// Walk up from `start` until a `.obmem.toml` or `.git` is found.
pub fn find_project_root(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(PROJECT_CONFIG_FILE).exists() || dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    start.to_path_buf()
}

impl Config {
    pub fn load(project_override: Option<&Path>) -> Result<Self> {
        let start = match project_override {
            Some(p) => p.to_path_buf(),
            None => match std::env::var("OBMEM_PROJECT_ROOT") {
                Ok(p) => PathBuf::from(p),
                Err(_) => std::env::current_dir()?,
            },
        };
        let project_root = find_project_root(&start);

        let pf: ProjectFile = read_toml(&project_root.join(PROJECT_CONFIG_FILE))?.unwrap_or_default();
        let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();

        let project_name = pf.name.clone().unwrap_or_else(|| {
            project_root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "default".into())
        });
        let project_vault = project_vault_path(&project_root, pf.vault.as_deref().unwrap_or(DEFAULT_VAULT))?;
        let global_vault = global_vault_from(gf.vault);

        let secrets = secrets_policy(&project_root, pf.secrets, &gf.secrets)?;
        let auto_approve = auto_approve(&pf.review, &gf.review);
        let device = device_name(&gf.sync);
        Ok(Self {
            project_root,
            project_name,
            project_vault,
            global_vault,
            secrets,
            audit: pf.audit,
            auto_approve,
            device,
        })
    }

    /// Session logs etc. for this project inside the global vault.
    pub fn global_project_dir(&self) -> PathBuf {
        // The name comes from repo config: keep it a single, harmless path component.
        self.global_vault.join("Projects").join(sanitize(&self.project_name))
    }
}

/// Repo config can only make secret scanning stricter: its `env_files` are added to the defaults
/// (and must be plain relative paths inside the project), and it can enable but not disable
/// process-environment scanning. `mode` may switch to `redact`, which still never stores a secret.
fn secrets_policy(project_root: &Path, repo: SecretsConfig, user: &GlobalSecrets) -> Result<SecretsConfig> {
    let mut env_files = SecretsConfig::default().env_files;
    for f in repo.env_files {
        if !is_plain_relative(&f) {
            anyhow::bail!(
                "{}: secrets.env_files entry \"{f}\" must be a relative path inside the project",
                project_root.join(PROJECT_CONFIG_FILE).display()
            );
        }
        if !env_files.contains(&f) {
            env_files.push(f);
        }
    }
    Ok(SecretsConfig {
        mode: repo.mode,
        env_files,
        scan_process_env: repo.scan_process_env || user.scan_process_env.unwrap_or(true),
    })
}

/// Auto-approve is on only when the user's config turns it on and the repo's does not turn it off:
/// a clone must not be able to skip the review of what agents write, in the personal vault least of all.
fn auto_approve(repo: &ReviewConfig, user: &ReviewConfig) -> bool {
    user.auto_approve.unwrap_or(false) && repo.auto_approve.unwrap_or(true)
}

/// The personal vault: `OBMEM_GLOBAL_VAULT`, else the user's config, else `~/Obsidian/AgentMemory`.
pub fn global_vault() -> Result<PathBuf> {
    let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();
    Ok(global_vault_from(gf.vault))
}

/// This machine's name, which keeps what two devices write to the synced personal vault apart:
/// `[sync] device`, else the host name without its domain.
pub fn device_name(sync: &SyncConfig) -> String {
    sync.device.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| {
        let host = gethostname::gethostname().to_string_lossy().into_owned();
        let host = host.split('.').next().unwrap_or_default();
        if host.is_empty() { "device".into() } else { host.into() }
    })
}

/// The user's `[sync]` settings.
pub fn sync_config() -> Result<SyncConfig> {
    let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();
    Ok(gf.sync)
}

fn global_vault_from(configured: Option<String>) -> PathBuf {
    std::env::var("OBMEM_GLOBAL_VAULT")
        .ok()
        .or(configured)
        .map(|s| expand_tilde(&s))
        .unwrap_or_else(|| home().join("Obsidian/AgentMemory"))
}

/// The project vault when `.obmem.toml` sets none.
pub const DEFAULT_VAULT: &str = "docs/kb";

/// A path relative to the project that stays inside it: no `..`, `~` or absolute paths.
pub fn is_plain_relative(p: &str) -> bool {
    let path = Path::new(p);
    !p.is_empty()
        && !path.is_absolute()
        && !p.starts_with('~')
        && path.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
}

/// The project vault for `.obmem.toml`'s `vault`. The file ships with the repo, so a clone must
/// not be able to point writes elsewhere: the path must be plain relative, and must not leave the
/// project through a symlink either.
pub fn project_vault_path(project_root: &Path, vault: &str) -> Result<PathBuf> {
    if !is_plain_relative(vault) {
        anyhow::bail!(
            "{}: `vault = \"{vault}\"` must be a relative path inside the project (no `..`, `~` or absolute paths)",
            project_root.join(PROJECT_CONFIG_FILE).display()
        );
    }
    let project_vault = project_root.join(vault);
    ensure_within(project_root, &project_vault)?;
    Ok(project_vault)
}

/// The deepest existing ancestor of `p` (or `p` itself), with symlinks resolved.
fn real_existing(p: &Path) -> Result<PathBuf> {
    let mut cur = p;
    loop {
        if cur.exists() {
            return Ok(std::fs::canonicalize(cur)?);
        }
        cur = cur.parent().ok_or_else(|| anyhow::anyhow!("no existing ancestor of {}", p.display()))?;
    }
}

/// Refuse paths that leave `root` once symlinks are resolved (checks what exists so far, so it
/// can run before any directory is created).
pub fn ensure_within(root: &Path, p: &Path) -> Result<()> {
    if !p.starts_with(root) {
        anyhow::bail!("refusing {}: not inside {}", p.display(), root.display());
    }
    let real_root = real_existing(root)?;
    let real = real_existing(p)?;
    if !real.starts_with(&real_root) {
        anyhow::bail!(
            "refusing {}: it resolves to {} (outside {}) through a symlink",
            p.display(),
            real.display(),
            real_root.display()
        );
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() { "default".into() } else { s }
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let v = toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_with(toml: &str) -> Result<Config> {
        let dir = std::env::temp_dir().join(format!("obmem-cfg-{}-{}", std::process::id(), sanitize(toml)));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(PROJECT_CONFIG_FILE), toml)?;
        let r = Config::load(Some(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    #[test]
    fn repo_config_cannot_weaken_secret_scanning() {
        let c = load_with("[secrets]\nenv_files = [\"config/.env.test\"]\nscan_process_env = false\n").unwrap();
        assert!(c.secrets.scan_process_env, "repo cannot turn process-env scanning off");
        assert!(c.secrets.env_files.contains(&".env".to_string()), "defaults stay");
        assert!(c.secrets.env_files.contains(&"config/.env.test".to_string()), "additions allowed");
        for bad in ["/etc/passwd", "../../.aws/credentials", "~/.ssh/id_rsa"] {
            assert!(load_with(&format!("[secrets]\nenv_files = [\"{bad}\"]\n")).is_err(), "{bad}");
        }
        let empty = load_with("[secrets]\nenv_files = []\n").unwrap();
        assert_eq!(empty.secrets.env_files, SecretsConfig::default().env_files);
    }

    #[test]
    fn only_the_user_turns_auto_approve_on() {
        let on = ReviewConfig { auto_approve: Some(true) };
        let off = ReviewConfig { auto_approve: Some(false) };
        let unset = ReviewConfig::default();
        assert!(!auto_approve(&unset, &unset), "off by default");
        assert!(!auto_approve(&on, &unset), "a repo cannot turn it on");
        assert!(!auto_approve(&on, &off));
        assert!(auto_approve(&unset, &on));
        assert!(auto_approve(&on, &on));
        assert!(!auto_approve(&off, &on), "a repo can turn it off");
    }

    #[test]
    fn repo_config_cannot_escape() {
        for bad in ["vault = \"../../etc\"", "vault = \"/tmp/x\"", "vault = \"~/x\"", "vault = \"docs/../../x\""] {
            assert!(load_with(bad).is_err(), "{bad} should be rejected");
        }
        // a symlinked vault folder that points outside the repo
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("obmem-cfg-link-{}", std::process::id()));
            let outside = std::env::temp_dir().join(format!("obmem-cfg-outside-{}", std::process::id()));
            std::fs::create_dir_all(dir.join("docs")).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, dir.join("docs/kb")).unwrap();
            let err = Config::load(Some(&dir)).unwrap_err().to_string();
            assert!(err.contains("symlink"), "{err}");
            assert!(ensure_within(&dir, &dir.join("docs/kb/Decisions/x.md")).is_err());
            assert!(ensure_within(&dir, &dir.join("ok/new/dir/x.md")).is_ok());
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&outside);
        }

        let c = load_with("vault = \"./docs/kb\"\nname = \"../../evil\"").unwrap();
        let dir = c.global_project_dir();
        assert_eq!(dir.parent().unwrap(), c.global_vault.join("Projects"));
        assert!(!dir.file_name().unwrap().to_string_lossy().contains(".."));
    }
}
