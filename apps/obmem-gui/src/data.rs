//! What the window shows and changes, through obmem's libraries. Everything here blocks (files,
//! git, Obsidian's registry, child processes), so the UI runs it with `nsh_core::blocking`.

use obmem_agents::hooks::Agent;
use obmem_agents::setup;
use obmem_core::allowlist::{self, Allowlist, Kind};
use obmem_core::config::{self, Config};
use obmem_core::settings::{self, GlobalSettings, ProjectSettings};
use obmem_core::status::{self, Status};
use obmem_core::audit::{AuditOptions, Severity};
use obmem_core::note::Scope;
use obmem_core::store::{NoteRef, Store, Verdict};
use obmem_core::{init, obsidian, projects, review};
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
}

/// The user's own settings and their personal vault's allowlist.
#[derive(Debug, Clone, PartialEq)]
pub struct Personal {
    pub settings: GlobalSettings,
    pub vault: PathBuf,
    pub allowlist: Result<Allowlist, String>,
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
    pub manual: Vec<String>,
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
                .unwrap_or_else(|| p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            ProjectRow { path: p.path, name, missing }
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
        .map(|agent| AgentState { agent, missing: setup::check(&cfg, agent).map(|r| r.changed).map_err(text) })
        .collect();
    let root = &cfg.project_root;
    Ok(Detail {
        status: status::status(&store),
        agents,
        cli: setup::cli_version(),
        settings: settings::read_project(root).map_err(text),
        codex_auto_approve: setup::codex_auto_approve(&cfg).map_err(text),
        allowlist: allowlist::read(&cfg.project_vault).map_err(text),
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
        .map(|h| Hit { rule: h.rule, description: h.description, preview: h.preview, fingerprint: h.fingerprint })
        .collect())
}

fn vault_dir(path: &Path, vault: Vault) -> Result<PathBuf, String> {
    match vault {
        Vault::Project => Config::load(Some(path)).map(|c| c.project_vault).map_err(text),
        Vault::Personal => config::global_vault().map_err(text),
    }
}

/// `path` is the shown project; it is not needed for the personal vault.
pub fn allow(path: &Path, vault: Vault, kind: Kind, entry: &str) -> Result<(), String> {
    allowlist::add(&vault_dir(path, vault)?, kind, entry).map(|_| ()).map_err(text)
}

pub fn disallow(path: &Path, vault: Vault, kind: Kind, entry: &str) -> Result<(), String> {
    allowlist::remove(&vault_dir(path, vault)?, kind, entry).map(|_| ()).map_err(text)
}

pub fn personal() -> Result<Personal, String> {
    let vault = config::global_vault().map_err(text)?;
    Ok(Personal {
        settings: settings::read_global().map_err(text)?,
        allowlist: allowlist::read(&vault).map_err(text),
        vault,
    })
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
    Ok(SetupDone { agent, changed: r.changed, manual: r.manual })
}

/// One project's review items, or the personal vault's (shared by every project, listed once).
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewGroup {
    pub title: String,
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
        let Ok(cfg) = Config::load(Some(&p.path)) else { continue };
        if !cfg.project_root.join(config::PROJECT_CONFIG_FILE).exists() {
            continue;
        }
        let mut store = Store::open(cfg.clone()).map_err(text)?;
        let (mine, shared): (Vec<_>, Vec<_>) = store.review().map_err(text)?.into_iter().partition(|i| i.scope == "project");
        if personal.is_none() {
            personal = Some(ReviewGroup {
                title: "Personal vault".into(),
                root: cfg.project_root.clone(),
                project: None,
                vault_id: obsidian::vault_id(&cfg.global_vault).ok().flatten(),
                items: shared,
            });
        }
        groups.push(ReviewGroup {
            title: cfg.project_name.clone(),
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

pub fn approve(root: &Path, scope: Scope, rel: &str) -> Result<String, String> {
    store_at(root)?.approve(NoteRef::At { scope, rel }, BY).map_err(text)
}

pub fn resolve(root: &Path, scope: Scope, rel: &str, verdict: Verdict) -> Result<String, String> {
    store_at(root)?.resolve_finding(NoteRef::At { scope, rel }, verdict, None, BY, true).map_err(text)
}

/// Records that the note was re-checked against the current code.
pub fn verify(root: &Path, scope: Scope, rel: &str) -> Result<String, String> {
    store_at(root)?.verify(NoteRef::At { scope, rel }, BY, true).map_err(text)
}

/// What an audit run reported.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditDone {
    pub summary: String,
    /// What the audit could not check, and why.
    pub notes: Vec<String>,
}

/// `obmem audit --write`: checks both vaults and keeps the finding notes in step.
pub fn run_audit(root: &Path) -> Result<AuditDone, String> {
    let mut store = store_at(root)?;
    let opts = AuditOptions { scope: None, rules: vec![], today: chrono::Local::now().date_naive() };
    let (report, stats) = store.audit(&opts, Some(Severity::Warn)).map_err(text)?;
    let count = |s: Severity| report.issues.iter().filter(|i| i.severity == s).count();
    let mut summary = format!(
        "Checked {} notes: {} errors, {} warnings.",
        report.checked_notes,
        count(Severity::Error),
        count(Severity::Warn)
    );
    if let Some(st) = stats {
        summary.push_str(&format!(
            " Findings: {} new, {} reopened, {} closed.",
            st.created.len(),
            st.reopened.len(),
            st.closed.len()
        ));
    }
    Ok(AuditDone { summary, notes: report.notices })
}
