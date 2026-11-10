//! obmem's memory model, shared by every front end (CLI, MCP server, agent hooks, GUI):
//! configuration, the project and global vaults, notes, search (through the Obsidian CLI, or
//! by reading the files where there is no Obsidian), the audit, and the secret scanner.

pub mod allowlist;
pub mod audit;
pub mod catalog;
pub mod config;
pub mod context;
pub mod drift;
pub mod guide;
pub mod headless;
pub mod import;
pub mod init;
mod links;
pub mod note;
pub mod obsidian;
pub mod projects;
mod query;
pub mod review;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod status;
pub mod store;
