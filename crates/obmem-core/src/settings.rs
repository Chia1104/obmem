//! Editing obmem's two config files as a person does in a settings screen: `.obmem.toml` (the
//! project's, in git) and `~/.config/obmem/config.toml` (the user's). Values are read as the files
//! state them, not as `Config::load` merges them, and writes keep everything else in the file,
//! comments included.

use crate::config::{
    GlobalFile, PROJECT_CONFIG_FILE, ProjectFile, SecretMode, SecretsConfig, expand_tilde, global_config_path,
    is_plain_relative, project_vault_path,
};
use crate::store::atomic_write;
use anyhow::{Context, Result, bail};
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item, Value};

/// What `.obmem.toml` sets.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSettings {
    /// The name in the personal vault (`Projects/<name>`); empty: the folder's name.
    pub name: String,
    /// The project vault, relative to the project root.
    pub vault: String,
    pub secret_mode: SecretMode,
    /// Env files scanned for known secrets besides the defaults, which are always scanned.
    pub extra_env_files: Vec<String>,
    pub stale_days: i64,
    pub duplicate_threshold: f64,
}

/// What the user's own config sets.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSettings {
    /// The personal vault as written (`~` allowed); empty: the default.
    pub vault: String,
    /// Treat sensitive-looking environment variables as known secrets. Only the user's config can
    /// turn this off; a repo's `.obmem.toml` cannot.
    pub scan_process_env: bool,
    /// Write agents' notes `active` instead of `draft`. Only the user's config can turn this on;
    /// a repo's `.obmem.toml` can only turn it off for that project.
    pub auto_approve: bool,
}

/// `[sync]` in the user's config, as the file states it; empty strings stand for unset keys.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncSettings {
    pub endpoint: String,
    pub region: String,
    /// Empty: sync is off.
    pub bucket: String,
    pub prefix: String,
    pub device: String,
    /// Sync in the background when agent sessions start and stop.
    pub auto: bool,
    pub community_plugins: bool,
}

/// The env files obmem always scans for known secrets.
pub fn default_env_files() -> Vec<String> {
    SecretsConfig::default().env_files
}

fn read_doc(path: &Path) -> Result<DocumentMut> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    text.parse().with_context(|| format!("parsing {}", path.display()))
}

/// Sets a value and keeps the comment and spacing the old one had (`mode = "block" # or redact`).
fn assign(slot: &mut Item, new: impl Into<Value>) {
    let mut v = new.into();
    if let Some(old) = slot.as_value() {
        *v.decor_mut() = old.decor().clone();
    }
    *slot = Item::Value(v);
}

fn assign_or_remove(doc: &mut DocumentMut, key: &str, v: Option<&str>) {
    match v {
        Some(v) => assign(&mut doc[key], v),
        None => {
            doc.remove(key);
        }
    }
}

pub fn read_project(root: &Path) -> Result<ProjectSettings> {
    let file: ProjectFile = toml::from_str(&read_doc(&root.join(PROJECT_CONFIG_FILE))?.to_string())?;
    let defaults = default_env_files();
    Ok(ProjectSettings {
        name: file.name.unwrap_or_default(),
        vault: file.vault.unwrap_or_else(|| crate::config::DEFAULT_VAULT.into()),
        secret_mode: file.secrets.mode,
        extra_env_files: file.secrets.env_files.into_iter().filter(|f| !defaults.contains(f)).collect(),
        stale_days: file.audit.stale_days,
        duplicate_threshold: file.audit.duplicate_threshold,
    })
}

/// Checks the settings with the rules `Config::load` applies, then writes them.
pub fn save_project(root: &Path, s: &ProjectSettings) -> Result<()> {
    project_vault_path(root, s.vault.trim())?;
    if let Some(f) = s.extra_env_files.iter().find(|f| !is_plain_relative(f.trim())) {
        bail!("env file \"{f}\" must be a relative path inside the project");
    }
    if s.stale_days < 1 {
        bail!("stale days must be at least 1");
    }
    if !(s.duplicate_threshold > 0.0 && s.duplicate_threshold <= 1.0) {
        bail!("the duplicate threshold must be above 0 and at most 1");
    }
    let path = root.join(PROJECT_CONFIG_FILE);
    let mut doc = read_doc(&path)?;
    let name = s.name.trim();
    assign_or_remove(&mut doc, "name", (!name.is_empty()).then_some(name));
    assign(&mut doc["vault"], s.vault.trim());
    let mode = match s.secret_mode {
        SecretMode::Block => "block",
        SecretMode::Redact => "redact",
    };
    assign(&mut doc["secrets"]["mode"], mode);
    let mut files: Array = default_env_files().into_iter().collect();
    for f in &s.extra_env_files {
        let f = f.trim();
        if !f.is_empty() && !files.iter().any(|v| v.as_str() == Some(f)) {
            files.push(f);
        }
    }
    assign(&mut doc["secrets"]["env_files"], files);
    assign(&mut doc["audit"]["stale_days"], s.stale_days);
    assign(&mut doc["audit"]["duplicate_threshold"], s.duplicate_threshold);
    atomic_write(root, &path, &doc.to_string())
}

pub fn read_global() -> Result<GlobalSettings> {
    read_global_at(&global_config_path())
}

pub fn save_global(s: &GlobalSettings) -> Result<()> {
    save_global_at(&global_config_path(), s)
}

pub fn read_sync() -> Result<SyncSettings> {
    read_sync_at(&global_config_path())
}

pub fn save_sync(s: &SyncSettings) -> Result<()> {
    save_sync_at(&global_config_path(), s)
}

/// Set `[sync] device`, keeping the rest.
pub fn set_sync_device(name: &str) -> Result<()> {
    let mut s = read_sync()?;
    s.device = name.to_string();
    save_sync(&s)
}

fn read_sync_at(path: &Path) -> Result<SyncSettings> {
    let file: GlobalFile = toml::from_str(&read_doc(path)?.to_string())?;
    let sync = file.sync;
    Ok(SyncSettings {
        endpoint: sync.endpoint.unwrap_or_default(),
        region: sync.region.unwrap_or_default(),
        bucket: sync.bucket.unwrap_or_default(),
        prefix: sync.prefix.unwrap_or_default(),
        device: sync.device.unwrap_or_default(),
        auto: sync.auto.unwrap_or(true),
        community_plugins: sync.community_plugins.unwrap_or(false),
    })
}

fn save_sync_at(path: &Path, s: &SyncSettings) -> Result<()> {
    let endpoint = s.endpoint.trim();
    if !endpoint.is_empty() && !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
        bail!("the endpoint must be a URL starting with https://");
    }
    let bucket = s.bucket.trim();
    if bucket.contains('/') || bucket.contains(char::is_whitespace) {
        bail!("the bucket is a name, without slashes or spaces");
    }
    let mut doc = read_doc(path)?;
    let table = doc.entry("sync").or_insert(toml_edit::table());
    let Some(table) = table.as_table_like_mut() else { bail!("[sync] in {} is not a table", path.display()) };
    for (key, value) in [
        ("endpoint", endpoint),
        ("region", s.region.trim()),
        ("bucket", bucket),
        ("prefix", s.prefix.trim()),
        ("device", s.device.trim()),
    ] {
        if value.is_empty() {
            table.remove(key);
        } else {
            match table.get_mut(key) {
                Some(slot) => assign(slot, value),
                None => {
                    table.insert(key, toml_edit::value(value));
                }
            }
        }
    }
    for (key, value) in [("auto", s.auto), ("community_plugins", s.community_plugins)] {
        match table.get_mut(key) {
            Some(slot) => assign(slot, value),
            None => {
                table.insert(key, toml_edit::value(value));
            }
        }
    }
    let dir = path.parent().context("config path has no parent")?;
    std::fs::create_dir_all(dir)?;
    atomic_write(dir, path, &doc.to_string())
}

fn read_global_at(path: &Path) -> Result<GlobalSettings> {
    let file: GlobalFile = toml::from_str(&read_doc(path)?.to_string())?;
    Ok(GlobalSettings {
        vault: file.vault.unwrap_or_default(),
        scan_process_env: file.secrets.scan_process_env.unwrap_or(true),
        auto_approve: file.review.auto_approve.unwrap_or(false),
    })
}

fn save_global_at(path: &Path, s: &GlobalSettings) -> Result<()> {
    let vault = s.vault.trim();
    if !vault.is_empty() && !expand_tilde(vault).is_absolute() {
        bail!("the personal vault must be an absolute path or start with ~/");
    }
    let mut doc = read_doc(path)?;
    assign_or_remove(&mut doc, "vault", (!vault.is_empty()).then_some(vault));
    assign(&mut doc["secrets"]["scan_process_env"], s.scan_process_env);
    assign(&mut doc["review"]["auto_approve"], s.auto_approve);
    let dir = path.parent().context("config path has no parent")?;
    std::fs::create_dir_all(dir)?;
    atomic_write(dir, path, &doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("obmem-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn project_settings_round_trip_and_keep_comments() {
        let root = temp("project");
        std::fs::write(
            root.join(PROJECT_CONFIG_FILE),
            "# team config, keep me\nvault = \"docs/kb\"\n\n[secrets]\nmode = \"block\" # or redact\n",
        )
        .unwrap();
        let mut s = read_project(&root).unwrap();
        assert_eq!((s.name.as_str(), s.vault.as_str(), s.stale_days), ("", "docs/kb", 180));
        assert!(s.extra_env_files.is_empty());

        s.name = "renamed".into();
        s.secret_mode = SecretMode::Redact;
        s.extra_env_files = vec!["config/.secrets".into()];
        s.stale_days = 90;
        save_project(&root, &s).unwrap();

        let text = std::fs::read_to_string(root.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(text.contains("# team config, keep me") && text.contains("# or redact"), "{text}");
        assert_eq!(read_project(&root).unwrap(), s);
        let cfg = crate::config::Config::load(Some(&root)).unwrap();
        assert_eq!(cfg.project_name, "renamed");
        assert!(cfg.secrets.env_files.contains(&"config/.secrets".into()) && cfg.secrets.env_files.contains(&".env".into()));

        // The rules Config::load enforces refuse the save, and the file stays as it was.
        for bad in ["../outside", "/tmp/x", "~/x", ""] {
            assert!(save_project(&root, &ProjectSettings { vault: bad.into(), ..s.clone() }).is_err(), "{bad}");
        }
        assert!(save_project(&root, &ProjectSettings { extra_env_files: vec!["../.env".into()], ..s.clone() }).is_err());
        assert!(save_project(&root, &ProjectSettings { duplicate_threshold: 1.5, ..s.clone() }).is_err());
        assert_eq!(read_project(&root).unwrap(), s);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_settings_round_trip() {
        let dir = temp("global");
        let path = dir.join("config.toml");
        assert_eq!(
            read_global_at(&path).unwrap(),
            GlobalSettings { vault: String::new(), scan_process_env: true, auto_approve: false }
        );

        let s = GlobalSettings { vault: "~/Notes/Agents".into(), scan_process_env: false, auto_approve: true };
        save_global_at(&path, &s).unwrap();
        assert_eq!(read_global_at(&path).unwrap(), s);
        assert!(save_global_at(&path, &GlobalSettings { vault: "relative/path".into(), ..s.clone() }).is_err());

        save_global_at(&path, &GlobalSettings { vault: String::new(), scan_process_env: true, auto_approve: false }).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("vault"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_settings_round_trip_and_keep_the_rest() {
        let dir = temp("sync");
        let path = dir.join("config.toml");
        std::fs::write(&path, "# mine\nvault = \"~/v\"\n\n[sync]\nbucket = \"old\" # the bucket\n").unwrap();
        let mut s = read_sync_at(&path).unwrap();
        assert_eq!((s.bucket.as_str(), s.auto, s.community_plugins), ("old", true, false));

        s.endpoint = "https://acct.r2.cloudflarestorage.com".into();
        s.bucket = "vault".into();
        s.device = "work-mac".into();
        s.auto = false;
        save_sync_at(&path, &s).unwrap();
        assert_eq!(read_sync_at(&path).unwrap(), s);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine") && text.contains("# the bucket") && text.contains("vault = \"~/v\""), "{text}");
        let loaded = crate::config::sync_config_at(&path).unwrap();
        assert_eq!(loaded.device.as_deref(), Some("work-mac"));
        assert!(!loaded.auto_sync());

        s.device = String::new();
        save_sync_at(&path, &s).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("device"));
        assert!(save_sync_at(&path, &SyncSettings { endpoint: "acct.r2.dev".into(), ..s.clone() }).is_err());
        assert!(save_sync_at(&path, &SyncSettings { bucket: "a/b".into(), ..s.clone() }).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_device_names_differ_on_one_host() {
        let (a, b) = (crate::config::new_device_name(), crate::config::new_device_name());
        assert_ne!(a, b);
        let (host_a, tag) = a.rsplit_once('-').unwrap();
        assert_eq!(host_a, b.rsplit_once('-').unwrap().0);
        assert!(tag.len() == 4 && tag.bytes().all(|c| c.is_ascii_hexdigit()), "{a}");
    }
}
