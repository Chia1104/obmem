//! What the window shows and changes, through obmem's libraries. Everything here blocks (files,
//! git, Obsidian's registry, child processes), so the UI runs it with `nsh_core::blocking`.

use obmem_agents::hooks::Agent;
use obmem_agents::setup;
use obmem_core::config::Config;
use obmem_core::status::{self, Status};
use obmem_core::store::Store;
use obmem_core::{init, projects};
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
    Ok(Detail { status: status::status(&store), agents, cli: setup::cli_version() })
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
