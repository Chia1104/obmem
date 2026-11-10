//! A project's state as `obmem status` and the GUI show it.

use crate::config::{PROJECT_CONFIG_FILE, SecretMode};
use crate::note::Scope;
use crate::obsidian;
use crate::store::Store;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub project_root: PathBuf,
    pub project_name: String,
    /// The project has `.obmem.toml` (`obmem init` ran). Without it, hooks do nothing and the
    /// MCP server refuses project-vault writes.
    pub initialized: bool,
    /// Where search, backlinks and link checks come from.
    pub backend: obsidian::Backend,
    pub project_vault: VaultStatus,
    pub global_vault: VaultStatus,
    pub secret_rules: usize,
    pub known_secrets: usize,
    pub secret_mode: SecretMode,
    /// Agent drafts waiting for review, in the project vault.
    pub drafts: usize,
    /// Open findings, in the project vault.
    pub open_findings: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VaultStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub obsidian: Registration,
}

/// Whether Obsidian knows the vault. With [`obsidian::Backend::App`], search, backlinks and
/// link checks need it.
#[derive(Debug, Clone, PartialEq)]
pub enum Registration {
    Registered {
        id: String,
    },
    NotRegistered,
    /// Obsidian's registry could not be read.
    Unknown(String),
}

pub fn status(store: &Store) -> Status {
    let cfg = store.config();
    let vault = |path: &PathBuf| VaultStatus {
        path: path.clone(),
        exists: path.is_dir(),
        obsidian: match obsidian::vault_id(path) {
            Ok(Some(id)) => Registration::Registered { id },
            Ok(None) => Registration::NotRegistered,
            Err(e) => Registration::Unknown(format!("{e:#}")),
        },
    };
    Status {
        project_root: cfg.project_root.clone(),
        project_name: cfg.project_name.clone(),
        initialized: cfg.project_root.join(PROJECT_CONFIG_FILE).exists(),
        backend: store.backend(),
        project_vault: vault(&cfg.project_vault),
        global_vault: vault(&cfg.global_vault),
        secret_rules: store.scanner().rule_count(),
        known_secrets: store.scanner().known_value_count(),
        secret_mode: cfg.secrets.mode,
        drafts: store.count_by_status("draft", Scope::Project),
        open_findings: store.count_by_status("open", Scope::Project),
    }
}
