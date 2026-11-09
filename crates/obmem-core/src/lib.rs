//! obmem's memory model, shared by every front end (CLI, MCP server, agent hooks, GUI):
//! configuration, the project and global vaults, notes, search through the Obsidian CLI,
//! the audit, and the secret scanner.

pub mod audit;
pub mod catalog;
pub mod config;
pub mod context;
pub mod drift;
pub mod guide;
pub mod import;
pub mod init;
pub mod note;
pub mod obsidian;
pub mod projects;
pub mod secrets;
pub mod status;
pub mod store;
