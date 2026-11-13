//! What the window shows and changes, through obmem's libraries. Everything here blocks (files,
//! git, Obsidian's registry, child processes), so the UI runs it with `nsh_core::blocking`.
//!
//! Nothing here is in the window's language: this runs off the engine thread, where the language
//! cannot be read. An outcome is a typed value that `crate::words` and the views put into words;
//! a failure is the library's error, in English.

use obmem_agents::hooks::Agent;
use obmem_agents::setup::{self, ManualStep};
use obmem_core::allowlist::{self, Allowlist, Kind};
use obmem_core::audit::{AuditOptions, Severity};
use obmem_core::config::{self, Config};
use obmem_core::note::Scope;
use obmem_core::settings::{self, GlobalSettings, ProjectSettings, SyncSettings};
use obmem_core::status::{self, Status};
use obmem_core::store::{Decided, NoteRef, Store, Verdict};
use obmem_core::{import, init, obsidian, projects, review, sessions};
use std::path::{Path, PathBuf};

/// A project in the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub path: PathBuf,
    pub name: String,
    /// The folder is gone (moved or deleted since obmem saw it).
    pub missing: bool,
}

/// One project, as its page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub status: Status,
    pub agents: Vec<AgentState>,
    /// The version of the `obmem` on PATH, which every hook and MCP config runs.
    pub cli: Option<String>,
    /// What `.obmem.toml` sets.
    pub settings: Result<ProjectSettings, String>,
    /// Whether Codex runs obmem's tools without asking; `None` when Codex is not set up.
    pub codex_auto_approve: Result<Option<bool>, String>,
    /// The project vault's allowlist (fingerprints only count there).
    pub allowlist: Result<Allowlist, String>,
    /// Claude Code's memory for this project, to import.
    pub claude_memory: Result<ClaudeMemory, String>,
}

/// The user's own settings and their personal vault's allowlist.
#[derive(Debug, Clone, PartialEq)]
pub struct Personal {
    pub settings: GlobalSettings,
    pub vault: PathBuf,
    pub allowlist: Result<Allowlist, String>,
    pub sync: Result<SyncInfo, String>,
}

/// Sync of the personal vault: the settings, whether this device is connected, the last outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncInfo {
    pub settings: SyncSettings,
    /// The keychain holds this device's access key and vault key for the configured bucket.
    pub connected: Result<bool, String>,
    pub last: Option<obmem_sync::Status>,
}

/// A suspected secret in text the user tested, shown masked.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub rule: String,
    pub description: String,
    pub preview: String,
    pub fingerprint: String,
}

/// Which vault an allowlist edit goes to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Vault {
    Project,
    Personal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentState {
    pub agent: Agent,
    /// The files `obmem setup` would still write: empty when the agent is set up.
    pub missing: Result<Vec<String>, String>,
}

/// What `obmem setup` did for one agent.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupDone {
    pub agent: Agent,
    pub changed: Vec<String>,
    /// Steps only the user can take (trust prompts, app settings).
    pub manual: Vec<ManualStep>,
}

fn text(e: anyhow::Error) -> String {
    format!("{e:#}")
}

pub fn projects() -> Result<Vec<ProjectRow>, String> {
    Ok(projects::list()
        .map_err(text)?
        .into_iter()
        .map(|p| {
            let missing = !p.path.is_dir();
            let name = Config::load(Some(&p.path))
                .ok()
                .filter(|_| !missing)
                .map(|c| c.project_name)
                .unwrap_or_else(|| {
                    p.path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
            ProjectRow {
                path: p.path,
                name,
                missing,
            }
        })
        .collect())
}

/// Lists a folder (or the project it is in), and returns the project's root.
pub fn add_project(folder: &Path) -> Result<PathBuf, String> {
    let cfg = Config::load(Some(folder)).map_err(text)?;
    projects::record(&cfg.project_root).map_err(text)?;
    Ok(cfg.project_root)
}

pub fn remove_project(path: &Path) -> Result<(), String> {
    projects::remove(path).map(|_| ()).map_err(text)
}

pub fn detail(path: &Path) -> Result<Detail, String> {
    let cfg = Config::load(Some(path)).map_err(text)?;
    let store = Store::open(cfg.clone()).map_err(text)?;
    let agents = Agent::ALL
        .into_iter()
        .map(|agent| AgentState {
            agent,
            missing: setup::check(&cfg, agent).map(|r| r.changed).map_err(text),
        })
        .collect();
    let root = &cfg.project_root;
    Ok(Detail {
        status: status::status(&store),
        agents,
        cli: setup::cli_version(),
        settings: settings::read_project(root).map_err(text),
        codex_auto_approve: setup::codex_auto_approve(&cfg).map_err(text),
        allowlist: allowlist::read(&cfg.project_vault).map_err(text),
        claude_memory: claude_memory(&store, root),
    })
}

pub fn save_settings(path: &Path, s: &ProjectSettings) -> Result<(), String> {
    let cfg = Config::load(Some(path)).map_err(text)?;
    settings::save_project(&cfg.project_root, s).map_err(text)
}

pub fn set_codex_auto_approve(path: &Path, on: bool) -> Result<(), String> {
    let cfg = Config::load(Some(path)).map_err(text)?;
    setup::set_codex_auto_approve(&cfg, on).map_err(text)
}

/// Runs the project's secret scanner over `input`, with its allowlists applied.
pub fn scan(path: &Path, input: &str) -> Result<Vec<Hit>, String> {
    let store = Store::open(Config::load(Some(path)).map_err(text)?).map_err(text)?;
    Ok(store
        .scanner()
        .scan(input)
        .into_iter()
        .map(|h| Hit {
            rule: h.rule,
            description: h.description,
            preview: h.preview,
            fingerprint: h.fingerprint,
        })
        .collect())
}

fn vault_dir(path: &Path, vault: Vault) -> Result<PathBuf, String> {
    match vault {
        Vault::Project => Config::load(Some(path))
            .map(|c| c.project_vault)
            .map_err(text),
        Vault::Personal => config::global_vault().map_err(text),
    }
}

/// `path` is the shown project; it is not needed for the personal vault.
pub fn allow(path: &Path, vault: Vault, kind: Kind, entry: &str) -> Result<(), String> {
    allowlist::add(&vault_dir(path, vault)?, kind, entry)
        .map(|_| ())
        .map_err(text)
}

pub fn disallow(path: &Path, vault: Vault, kind: Kind, entry: &str) -> Result<(), String> {
    allowlist::remove(&vault_dir(path, vault)?, kind, entry)
        .map(|_| ())
        .map_err(text)
}

pub fn personal() -> Result<Personal, String> {
    let vault = config::global_vault().map_err(text)?;
    Ok(Personal {
        settings: settings::read_global().map_err(text)?,
        allowlist: allowlist::read(&vault).map_err(text),
        sync: sync_info(),
        vault,
    })
}

fn sync_info() -> Result<SyncInfo, String> {
    let settings = settings::read_sync().map_err(text)?;
    let cfg = config::sync_config().map_err(text)?;
    let configured = cfg.bucket.is_some();
    Ok(SyncInfo {
        connected: if configured {
            obmem_sync::keychain::is_set_up(&cfg).map_err(text)
        } else {
            Ok(false)
        },
        last: if configured {
            obmem_sync::Target::personal(cfg)
                .ok()
                .and_then(|target| obmem_sync::last_status(&target))
        } else {
            None
        },
        settings,
    })
}

pub fn save_sync(s: &SyncSettings) -> Result<(), String> {
    settings::save_sync(s).map_err(text)
}

/// What connecting a device asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectField {
    KeyId,
    Secret,
    Passphrase,
    Repeat,
}

/// The fields of the connect form whose value breaks its rule. Each has one: the key ID, the
/// secret and the passphrase are needed, and the passphrase is typed the same twice.
pub fn connect_problems(
    access_key_id: &str,
    secret_access_key: &str,
    passphrase: &str,
    repeat: &str,
) -> Vec<ConnectField> {
    [
        (access_key_id.trim().is_empty(), ConnectField::KeyId),
        (secret_access_key.is_empty(), ConnectField::Secret),
        (passphrase.is_empty(), ConnectField::Passphrase),
        (
            !passphrase.is_empty() && passphrase != repeat,
            ConnectField::Repeat,
        ),
    ]
    .into_iter()
    .filter_map(|(broken, field)| broken.then_some(field))
    .collect()
}

/// A device connected to the vault in `bucket`. `created`: the bucket held no vault, so this
/// device started one.
#[derive(Debug, Clone, PartialEq)]
pub struct Connected {
    pub bucket: String,
    pub created: bool,
}

/// Connect this device to the bucket the saved settings name.
pub fn connect_sync(
    access_key_id: String,
    secret_access_key: String,
    passphrase: String,
    repeat: String,
) -> Result<Connected, String> {
    let cfg = config::sync_config().map_err(text)?;
    let bucket = cfg.bucket.clone().ok_or("Save a bucket first.")?;
    // The form checks this before it sends, and says which field.
    if !connect_problems(&access_key_id, &secret_access_key, &passphrase, &repeat).is_empty() {
        return Err("The access key and the passphrase are needed, the passphrase twice.".into());
    }
    let creds = obmem_sync::Credentials {
        access_key_id: access_key_id.trim().to_string(),
        secret_access_key,
    };
    let target = obmem_sync::Target::personal(cfg).map_err(text)?;
    let created = obmem_sync::connect(&target, creds, &passphrase.into()).map_err(text)?;
    Ok(Connected { bucket, created })
}

/// One sync, now; the outcome also goes to the status the page shows. `None`: a sync is already
/// queued, and takes these changes too.
pub fn sync_now() -> Result<Option<obmem_sync::Summary>, String> {
    let target =
        obmem_sync::Target::personal(config::sync_config().map_err(text)?).map_err(text)?;
    Ok(obmem_sync::run(&target, false, std::time::Duration::ZERO)
        .map_err(text)?
        .map(|r| (&r).into()))
}

pub fn save_personal(s: &GlobalSettings) -> Result<(), String> {
    settings::save_global(s).map_err(text)
}

/// `obmem init`: the vaults and `.obmem.toml`.
pub fn initialize(path: &Path) -> Result<(), String> {
    let cfg = Config::load(Some(path)).map_err(text)?;
    init::run(&cfg).map_err(text)?;
    projects::record(&cfg.project_root).map_err(text)
}

pub fn set_up(path: &Path, agent: Agent) -> Result<SetupDone, String> {
    let cfg = Config::load(Some(path)).map_err(text)?;
    let r = setup::setup(&cfg, agent).map_err(text)?;
    Ok(SetupDone {
        agent,
        changed: r.changed,
        manual: r.manual,
    })
}

/// One project's review items, or the personal vault's (shared by every project, listed once).
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewGroup {
    /// The project's name. The personal vault has none.
    pub name: String,
    /// The project actions go through; for the personal vault, any listed project.
    pub root: PathBuf,
    /// `None` for the personal vault, which has no audit of its own here.
    pub project: Option<PathBuf>,
    /// Obsidian's id for the vault, to open its notes; `None` when Obsidian does not know it.
    pub vault_id: Option<String>,
    pub items: Vec<review::Item>,
}

/// Drafts and unresolved findings of every listed project that is set up, then the personal vault's.
pub fn review() -> Result<Vec<ReviewGroup>, String> {
    let mut groups = Vec::new();
    let mut personal: Option<ReviewGroup> = None;
    for p in projects::list().map_err(text)? {
        let Ok(cfg) = Config::load(Some(&p.path)) else {
            continue;
        };
        if !cfg.project_root.join(config::PROJECT_CONFIG_FILE).exists() {
            continue;
        }
        let mut store = Store::open(cfg.clone()).map_err(text)?;
        let (mine, shared): (Vec<_>, Vec<_>) = store
            .review()
            .map_err(text)?
            .into_iter()
            .partition(|i| i.scope == "project");
        if personal.is_none() {
            personal = Some(ReviewGroup {
                name: String::new(),
                root: cfg.project_root.clone(),
                project: None,
                vault_id: obsidian::vault_id(&cfg.global_vault).ok().flatten(),
                items: shared,
            });
        }
        groups.push(ReviewGroup {
            name: cfg.project_name.clone(),
            root: cfg.project_root.clone(),
            project: Some(cfg.project_root.clone()),
            vault_id: obsidian::vault_id(&cfg.project_vault).ok().flatten(),
            items: mine,
        });
    }
    groups.extend(personal);
    Ok(groups)
}

fn store_at(root: &Path) -> Result<Store, String> {
    Store::open(Config::load(Some(root)).map_err(text)?).map_err(text)
}

const BY: &str = "human (gui)";

pub fn approve(root: &Path, scope: Scope, rel: &str) -> Result<Decided, String> {
    store_at(root)?
        .approve(NoteRef::At { scope, rel }, BY)
        .map_err(text)
}

pub fn resolve(root: &Path, scope: Scope, rel: &str, verdict: Verdict) -> Result<Decided, String> {
    store_at(root)?
        .resolve_finding(NoteRef::At { scope, rel }, verdict, None, BY, true)
        .map_err(text)
}

/// Records that the note was re-checked against the current code.
pub fn verify(root: &Path, scope: Scope, rel: &str) -> Result<Decided, String> {
    store_at(root)?
        .verify(NoteRef::At { scope, rel }, BY, true)
        .map_err(text)
}

/// What an audit run reported.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditDone {
    /// How many notes it checked, and the errors and warnings it found in them.
    pub checked: usize,
    pub errors: usize,
    pub warnings: usize,
    /// How the finding notes changed: new, reopened, closed.
    pub findings: Option<(usize, usize, usize)>,
    /// What the audit could not check, and why.
    pub notes: Vec<String>,
}

/// `obmem audit --write`: checks both vaults and keeps the finding notes in step.
pub fn run_audit(root: &Path) -> Result<AuditDone, String> {
    let mut store = store_at(root)?;
    let opts = AuditOptions {
        scope: None,
        rules: vec![],
        today: chrono::Local::now().date_naive(),
    };
    let (report, stats) = store.audit(&opts, Some(Severity::Warn)).map_err(text)?;
    let count = |s: Severity| report.issues.iter().filter(|i| i.severity == s).count();
    Ok(AuditDone {
        checked: report.checked_notes,
        errors: count(Severity::Error),
        warnings: count(Severity::Warn),
        findings: stats.map(|st| (st.created.len(), st.reopened.len(), st.closed.len())),
        notes: report.notices,
    })
}

// --- What agents see, session logs, importing Claude Code memory -------------------------------

/// The context the session-start hook gives an agent in this project.
pub fn session_start(root: &Path) -> Result<String, String> {
    store_at(root)?.session_context(6000).map_err(text)
}

/// What `memory_context` returns for a task in this project.
pub fn context_for(root: &Path, task: &str) -> Result<String, String> {
    obmem_core::context::context(&mut store_at(root)?, task, 8, 5000).map_err(text)
}

/// The project's session logs, newest first, and the personal vault's id to open them.
#[derive(Debug, Clone, PartialEq)]
pub struct Sessions {
    pub logs: Vec<sessions::SessionLog>,
    pub vault_id: Option<String>,
}

pub fn sessions(root: &Path) -> Result<Sessions, String> {
    let cfg = Config::load(Some(root)).map_err(text)?;
    Ok(Sessions {
        logs: sessions::logs(&cfg, 60).map_err(text)?,
        vault_id: obsidian::vault_id(&cfg.global_vault).ok().flatten(),
    })
}

/// Claude Code's memory for this project, as `obmem import-claude` would bring it in.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeMemory {
    pub dir: PathBuf,
    pub items: Vec<ImportItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportItem {
    pub source: PathBuf,
    pub title: String,
    /// Where it goes: `project/fact`, `global/feedback`, ...
    pub destination: String,
    /// A note with its name is already there, so importing skips it.
    pub exists: bool,
}

fn claude_memory(store: &Store, root: &Path) -> Result<ClaudeMemory, String> {
    let dir = import::default_dir(root);
    if !dir.is_dir() {
        return Ok(ClaudeMemory { dir, items: vec![] });
    }
    let items = import::plan(&dir)
        .map_err(text)?
        .into_iter()
        .map(|p| ImportItem {
            exists: import::target(store, &p).exists(),
            destination: format!("{}/{}", p.scope.as_str(), p.note_type.as_str()),
            title: p.title,
            source: p.source,
        })
        .collect();
    Ok(ClaudeMemory { dir, items })
}

/// What importing one memory file did: the note it is now, or why it was left out.
pub type Imported = Result<String, import::Skip>;

/// Imports the chosen memory files as drafts. Returns what happened to each file, by its name.
pub fn import_claude(root: &Path, chosen: &[PathBuf]) -> Result<Vec<(String, Imported)>, String> {
    let mut store = store_at(root)?;
    let dir = import::default_dir(&store.config().project_root);
    let items = import::plan(&dir)
        .map_err(text)?
        .into_iter()
        .filter(|p| chosen.contains(&p.source))
        .collect();
    Ok(import::run(&mut store, items)
        .map_err(text)?
        .into_iter()
        .map(|(src, r)| {
            let name = src
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let done = match r {
                import::ImportResult::Imported(note) => Ok(note),
                import::ImportResult::Skipped(why) => Err(why),
            };
            (name, done)
        })
        .collect())
}
