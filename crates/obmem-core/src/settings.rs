//! Editing obmem's two config files as a person does in a settings screen: `.obmem.toml` (the
//! project's, in git) and `~/.config/obmem/config.toml` (the user's). Values are read as the files
//! state them, not as `Config::load` merges them, and writes keep everything else in the file,
//! comments included.

use crate::config::{
    GlobalFile, PROJECT_CONFIG_FILE, ProjectFile, SecretMode, SecretsConfig, expand_tilde,
    global_config_path, global_vault_from, is_plain_relative, private_vault_path,
    project_vault_path, resolved,
};
use crate::store::atomic_write;
use anyhow::{Context, Result, bail};
use std::path::Path;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value};

/// What `.obmem.toml` sets.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSettings {
    /// The name in the personal vault (`Projects/<name>`); empty: the folder's name.
    pub name: String,
    /// The project vault, relative to the project root. Not read while `private` is set.
    pub vault: String,
    /// The project's memory is kept outside the repo, in the vault the user's config names
    /// ([`make_private`] sets it). A save keeps it as the file has it.
    pub private: bool,
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

/// A rule a setting breaks: which setting, and which rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem<F> {
    pub field: F,
    pub rule: Rule,
}

impl<F> Problem<F> {
    fn new(field: F, rule: Rule) -> Self {
        Problem { field, rule }
    }
}

/// The rules settings have. Written out (`Display`), each is a sentence for the person who typed
/// the value; a front end in another language says them in its own words.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    VaultInsideProject,
    /// The env file that breaks it.
    EnvFileInsideProject(String),
    StaleDaysAtLeastOne,
    DuplicateThresholdInRange,
    PersonalVaultAbsolute,
    EndpointIsUrl,
    BucketIsName,
}

impl std::fmt::Display for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rule::VaultInsideProject => f.write_str(
                "The vault must be a relative path inside the project, without `..` or `~`.",
            ),
            Rule::EnvFileInsideProject(file) => write!(
                f,
                "The env file \"{file}\" must be a relative path inside the project."
            ),
            Rule::StaleDaysAtLeastOne => f.write_str("Stale days must be at least 1."),
            Rule::DuplicateThresholdInRange => {
                f.write_str("The duplicate threshold must be above 0 and at most 1.")
            }
            Rule::PersonalVaultAbsolute => {
                f.write_str("The personal vault must be an absolute path or start with ~/.")
            }
            Rule::EndpointIsUrl => {
                f.write_str("The endpoint must be a URL starting with https://.")
            }
            Rule::BucketIsName => f.write_str("The bucket is a name, without slashes or spaces."),
        }
    }
}

/// The settings of [`ProjectSettings`] that have rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectField {
    Vault,
    EnvFiles,
    StaleDays,
    DuplicateThreshold,
}

/// The settings of [`GlobalSettings`] that have rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalField {
    Vault,
}

/// The settings of [`SyncSettings`] that have rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncField {
    Endpoint,
    Bucket,
}

impl ProjectSettings {
    /// Every rule these settings break, by setting: the rules `Config::load` applies to the
    /// file. A settings screen shows them at the settings; a save refuses the first.
    pub fn problems(&self) -> Vec<Problem<ProjectField>> {
        let mut out = Vec::new();
        if !self.private && !is_plain_relative(self.vault.trim()) {
            out.push(Problem::new(ProjectField::Vault, Rule::VaultInsideProject));
        }
        if let Some(f) = self
            .extra_env_files
            .iter()
            .find(|f| !is_plain_relative(f.trim()))
        {
            out.push(Problem::new(
                ProjectField::EnvFiles,
                Rule::EnvFileInsideProject(f.clone()),
            ));
        }
        if self.stale_days < 1 {
            out.push(Problem::new(
                ProjectField::StaleDays,
                Rule::StaleDaysAtLeastOne,
            ));
        }
        if !(self.duplicate_threshold > 0.0 && self.duplicate_threshold <= 1.0) {
            out.push(Problem::new(
                ProjectField::DuplicateThreshold,
                Rule::DuplicateThresholdInRange,
            ));
        }
        out
    }
}

impl GlobalSettings {
    /// Every rule these settings break, by setting.
    pub fn problems(&self) -> Vec<Problem<GlobalField>> {
        let vault = self.vault.trim();
        if !vault.is_empty() && !expand_tilde(vault).is_absolute() {
            return vec![Problem::new(
                GlobalField::Vault,
                Rule::PersonalVaultAbsolute,
            )];
        }
        Vec::new()
    }
}

impl SyncSettings {
    /// Every rule these settings break, by setting.
    pub fn problems(&self) -> Vec<Problem<SyncField>> {
        let mut out = Vec::new();
        let endpoint = self.endpoint.trim();
        if !endpoint.is_empty()
            && !(endpoint.starts_with("https://") || endpoint.starts_with("http://"))
        {
            out.push(Problem::new(SyncField::Endpoint, Rule::EndpointIsUrl));
        }
        let bucket = self.bucket.trim();
        if bucket.contains('/') || bucket.contains(char::is_whitespace) {
            out.push(Problem::new(SyncField::Bucket, Rule::BucketIsName));
        }
        out
    }
}

/// Refuses settings that break a rule, with the first one's reason.
fn refuse<F>(problems: Vec<Problem<F>>) -> Result<()> {
    match problems.into_iter().next() {
        Some(p) => bail!("{}", p.rule),
        None => Ok(()),
    }
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
    text.parse()
        .with_context(|| format!("parsing {}", path.display()))
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
    let file: ProjectFile =
        toml::from_str(&read_doc(&root.join(PROJECT_CONFIG_FILE))?.to_string())?;
    let defaults = default_env_files();
    Ok(ProjectSettings {
        name: file.name.unwrap_or_default(),
        vault: file
            .vault
            .unwrap_or_else(|| crate::config::DEFAULT_VAULT.into()),
        private: file.private.unwrap_or(false),
        secret_mode: file.secrets.mode,
        extra_env_files: file
            .secrets
            .env_files
            .into_iter()
            .filter(|f| !defaults.contains(f))
            .collect(),
        stale_days: file.audit.stale_days,
        duplicate_threshold: file.audit.duplicate_threshold,
    })
}

/// Checks the settings ([`ProjectSettings::problems`]), then writes them.
pub fn save_project(root: &Path, s: &ProjectSettings) -> Result<()> {
    refuse(s.problems())?;
    let path = root.join(PROJECT_CONFIG_FILE);
    let mut doc = read_doc(&path)?;
    let name = s.name.trim();
    assign_or_remove(&mut doc, "name", (!name.is_empty()).then_some(name));
    if !s.private {
        // Also that the vault does not leave the project through a symlink.
        project_vault_path(root, s.vault.trim())?;
        assign(&mut doc["vault"], s.vault.trim());
    }
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
    assign(
        &mut doc["audit"]["duplicate_threshold"],
        s.duplicate_threshold,
    );
    atomic_write(root, &path, &doc.to_string())
}

/// Makes the project at `root` keep its memory in `vault`, a folder outside the repo. The user's
/// config names the vault (`[[project]]`, in place of what it named for this project before), and
/// an existing `.obmem.toml` is marked `private`, so that a machine whose config names no vault
/// refuses the project instead of writing its notes into the repo. No note is moved.
///
/// `vault` is an absolute path or starts with `~/`. An absolute one is recorded with its links
/// and `..` resolved; one under `~/` as it is, so the same line works on another machine.
pub fn make_private(root: &Path, vault: &str) -> Result<()> {
    make_private_at(&global_config_path(), root, vault)
}

fn make_private_at(config: &Path, root: &Path, vault: &str) -> Result<()> {
    let root = std::path::absolute(root)?;
    let mut doc = read_doc(config)?;
    let file: GlobalFile = toml::from_str(&doc.to_string())?;
    let vault = match vault.trim() {
        home if home.starts_with('~') => home.to_string(),
        path => resolved(Path::new(path)).display().to_string(),
    };
    let vault = vault.as_str();
    private_vault_path(&root, vault, &global_vault_from(file.vault))?;
    let list = doc
        .entry("project")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
    let Some(list) = list.as_array_of_tables_mut() else {
        bail!(
            "`project` in {} is not a list of tables (`[[project]]`)",
            config.display()
        )
    };
    let real_root = resolved(&root);
    let known = list.iter_mut().find(|t| {
        t.get("root")
            .and_then(Item::as_str)
            .is_some_and(|r| resolved(Path::new(r)) == real_root)
    });
    match known {
        Some(entry) => assign(&mut entry["vault"], vault),
        None => {
            let mut entry = Table::new();
            entry.insert("root", toml_edit::value(root.display().to_string()));
            entry.insert("vault", toml_edit::value(vault));
            list.push(entry);
        }
    }
    let dir = config.parent().context("config path has no parent")?;
    std::fs::create_dir_all(dir)?;
    atomic_write(dir, config, &doc.to_string())?;

    let path = root.join(PROJECT_CONFIG_FILE);
    if path.exists() {
        let mut doc = read_doc(&path)?;
        mark_private(&mut doc);
        atomic_write(&root, &path, &doc.to_string())?;
    }
    Ok(())
}

/// `private = true` where `vault` was, under the comment lines `vault` had above it.
fn mark_private(doc: &mut DocumentMut) {
    let order: Vec<String> = doc
        .iter()
        .map(|(key, _)| if key == "vault" { "private" } else { key }.to_string())
        .collect();
    let comments = doc.key("vault").map(|key| key.leaf_decor().clone());
    doc.remove("vault");
    assign(&mut doc["private"], true);
    if let (Some(comments), Some(mut key)) = (comments, doc.key_mut("private")) {
        *key.leaf_decor_mut() = comments;
    }
    let place = |key: &toml_edit::Key| order.iter().position(|k| k == key.get());
    doc.sort_values_by(|a, _, b, _| place(a).cmp(&place(b)));
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
    refuse(s.problems())?;
    let (endpoint, bucket) = (s.endpoint.trim(), s.bucket.trim());
    let mut doc = read_doc(path)?;
    let table = doc.entry("sync").or_insert(toml_edit::table());
    let Some(table) = table.as_table_like_mut() else {
        bail!("[sync] in {} is not a table", path.display())
    };
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
    refuse(s.problems())?;
    let vault = s.vault.trim();
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
        let dir =
            std::env::temp_dir().join(format!("obmem-settings-{name}-{}", std::process::id()));
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
        assert_eq!(
            (s.name.as_str(), s.vault.as_str(), s.stale_days),
            ("", "docs/kb", 180)
        );
        assert!(s.extra_env_files.is_empty());

        s.name = "renamed".into();
        s.secret_mode = SecretMode::Redact;
        s.extra_env_files = vec!["config/.secrets".into()];
        s.stale_days = 90;
        save_project(&root, &s).unwrap();

        let text = std::fs::read_to_string(root.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(
            text.contains("# team config, keep me") && text.contains("# or redact"),
            "{text}"
        );
        assert_eq!(read_project(&root).unwrap(), s);
        let cfg = crate::config::Config::load(Some(&root)).unwrap();
        assert_eq!(cfg.project_name, "renamed");
        assert!(
            cfg.secrets.env_files.contains(&"config/.secrets".into())
                && cfg.secrets.env_files.contains(&".env".into())
        );

        // The rules Config::load enforces refuse the save, and the file stays as it was.
        for bad in ["../outside", "/tmp/x", "~/x", ""] {
            assert!(
                save_project(
                    &root,
                    &ProjectSettings {
                        vault: bad.into(),
                        ..s.clone()
                    }
                )
                .is_err(),
                "{bad}"
            );
        }
        assert!(
            save_project(
                &root,
                &ProjectSettings {
                    extra_env_files: vec!["../.env".into()],
                    ..s.clone()
                }
            )
            .is_err()
        );
        assert!(
            save_project(
                &root,
                &ProjectSettings {
                    duplicate_threshold: 1.5,
                    ..s.clone()
                }
            )
            .is_err()
        );
        assert_eq!(read_project(&root).unwrap(), s);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_settings_round_trip() {
        let dir = temp("global");
        let path = dir.join("config.toml");
        assert_eq!(
            read_global_at(&path).unwrap(),
            GlobalSettings {
                vault: String::new(),
                scan_process_env: true,
                auto_approve: false
            }
        );

        let s = GlobalSettings {
            vault: "~/Notes/Agents".into(),
            scan_process_env: false,
            auto_approve: true,
        };
        save_global_at(&path, &s).unwrap();
        assert_eq!(read_global_at(&path).unwrap(), s);
        assert!(
            save_global_at(
                &path,
                &GlobalSettings {
                    vault: "relative/path".into(),
                    ..s.clone()
                }
            )
            .is_err()
        );

        save_global_at(
            &path,
            &GlobalSettings {
                vault: String::new(),
                scan_process_env: true,
                auto_approve: false,
            },
        )
        .unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("vault"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_settings_round_trip_and_keep_the_rest() {
        let dir = temp("sync");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# mine\nvault = \"~/v\"\n\n[sync]\nbucket = \"old\" # the bucket\n",
        )
        .unwrap();
        let mut s = read_sync_at(&path).unwrap();
        assert_eq!(
            (s.bucket.as_str(), s.auto, s.community_plugins),
            ("old", true, false)
        );

        s.endpoint = "https://acct.r2.cloudflarestorage.com".into();
        s.bucket = "vault".into();
        s.device = "work-mac".into();
        s.auto = false;
        save_sync_at(&path, &s).unwrap();
        assert_eq!(read_sync_at(&path).unwrap(), s);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# mine")
                && text.contains("# the bucket")
                && text.contains("vault = \"~/v\""),
            "{text}"
        );
        let loaded = crate::config::sync_config_at(&path).unwrap();
        assert_eq!(loaded.device.as_deref(), Some("work-mac"));
        assert!(!loaded.auto_sync());

        s.device = String::new();
        save_sync_at(&path, &s).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("device"));
        assert!(
            save_sync_at(
                &path,
                &SyncSettings {
                    endpoint: "acct.r2.dev".into(),
                    ..s.clone()
                }
            )
            .is_err()
        );
        assert!(
            save_sync_at(
                &path,
                &SyncSettings {
                    bucket: "a/b".into(),
                    ..s.clone()
                }
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn make_private_names_the_vault_and_marks_the_repo() {
        let dir = temp("private");
        let (root, config) = (dir.join("repo"), dir.join("config/config.toml"));
        let (vault, personal) = (dir.join("memory"), dir.join("personal"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            root.join(PROJECT_CONFIG_FILE),
            "# team config\nvault = \"docs/kb\" # where notes go\nname = \"acme\"\n\n[secrets]\nmode = \"block\"\n",
        )
        .unwrap();
        // A literal string: a Windows path has backslashes.
        std::fs::write(
            &config,
            format!(
                "# mine\nvault = '{}'\n\n[sync]\nbucket = \"b\"\n",
                personal.display()
            ),
        )
        .unwrap();
        let projects = || {
            let text = std::fs::read_to_string(&config).unwrap();
            assert!(text.contains("# mine") && text.contains("[sync]"), "{text}");
            toml::from_str::<GlobalFile>(&text).unwrap().projects
        };

        // As a person types a folder beside the repo.
        let typed = root.join("../memory").display().to_string();
        make_private_at(&config, &root, &typed).unwrap();
        // The folder as the file system names it: a temp folder is behind a link on macOS.
        let vault = resolved(&vault);
        assert_eq!(
            projects(),
            [crate::config::PrivateProject {
                root: root.display().to_string(),
                vault: vault.display().to_string(),
            }]
        );
        let text = std::fs::read_to_string(root.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(
            text.starts_with("# team config\nprivate = true\nname = \"acme\"\n"),
            "{text}"
        );
        assert!(!text.contains("vault"), "{text}");

        // A settings screen saves the rest and leaves the vault to the user's config.
        let mut s = read_project(&root).unwrap();
        assert!(s.private && s.problems().is_empty());
        s.stale_days = 30;
        save_project(&root, &s).unwrap();
        let text = std::fs::read_to_string(root.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(
            text.contains("private = true") && !text.contains("vault"),
            "{text}"
        );
        assert_eq!(read_project(&root).unwrap(), s);

        // Naming another folder replaces the entry.
        let moved = dir.join("moved");
        make_private_at(&config, &root, &moved.display().to_string()).unwrap();
        let list = projects();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].vault, resolved(&moved).display().to_string());

        // A folder a private vault cannot be changes nothing.
        for bad in [root.join("docs/kb"), personal.clone()] {
            assert!(make_private_at(&config, &root, &bad.display().to_string()).is_err());
        }
        assert_eq!(projects(), list);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn problems_name_every_setting_that_breaks_a_rule() {
        let project = ProjectSettings {
            name: String::new(),
            vault: "../outside".into(),
            private: false,
            secret_mode: SecretMode::Block,
            extra_env_files: vec!["ok/.env".into(), "/etc/passwd".into()],
            stale_days: 0,
            duplicate_threshold: 1.5,
        };
        let fields: Vec<ProjectField> = project.problems().iter().map(|p| p.field).collect();
        assert_eq!(
            fields,
            [
                ProjectField::Vault,
                ProjectField::EnvFiles,
                ProjectField::StaleDays,
                ProjectField::DuplicateThreshold
            ]
        );
        assert!(
            ProjectSettings {
                vault: "docs/kb".into(),
                extra_env_files: vec![],
                stale_days: 1,
                duplicate_threshold: 1.0,
                ..project
            }
            .problems()
            .is_empty()
        );

        let sync = SyncSettings {
            endpoint: "acct.r2.dev".into(),
            bucket: "a b".into(),
            ..SyncSettings::default()
        };
        let fields: Vec<SyncField> = sync.problems().iter().map(|p| p.field).collect();
        assert_eq!(fields, [SyncField::Endpoint, SyncField::Bucket]);
        // Empty means unset, which every setting allows.
        assert!(SyncSettings::default().problems().is_empty());

        let global = GlobalSettings {
            vault: "relative/path".into(),
            scan_process_env: true,
            auto_approve: false,
        };
        assert_eq!(global.problems()[0].field, GlobalField::Vault);
        assert!(
            GlobalSettings {
                vault: String::new(),
                ..global
            }
            .problems()
            .is_empty()
        );
    }

    #[test]
    fn new_device_names_differ_on_one_host() {
        let (a, b) = (
            crate::config::new_device_name(),
            crate::config::new_device_name(),
        );
        assert_ne!(a, b);
        let (host_a, tag) = a.rsplit_once('-').unwrap();
        assert_eq!(host_a, b.rsplit_once('-').unwrap().0);
        assert!(
            tag.len() == 4 && tag.bytes().all(|c| c.is_ascii_hexdigit()),
            "{a}"
        );
    }
}
