//! The projects obmem has seen on this machine, for front ends that work across projects (the
//! GUI). The session-start hook and `obmem init` record a project; people add and remove them by
//! hand. Only paths are kept: everything else comes from each project's own config.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub path: PathBuf,
    /// The last day obmem recorded it (`YYYY-MM-DD`).
    pub last_seen: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default, rename = "project")]
    projects: Vec<Project>,
}

/// `projects.toml` next to the global config (`~/.obmem/`).
pub fn registry_path() -> PathBuf {
    crate::config::global_config_path().with_file_name("projects.toml")
}

/// Most recently seen first.
pub fn list() -> Result<Vec<Project>> {
    list_at(&registry_path())
}

/// Add a project, or mark it seen today. Writes only when something changed.
pub fn record(path: &Path) -> Result<()> {
    record_at(&registry_path(), path, &today())
}

/// Returns whether the project was listed.
pub fn remove(path: &Path) -> Result<bool> {
    remove_at(&registry_path(), path)
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn load(file: &Path) -> Result<Registry> {
    match std::fs::read_to_string(file) {
        Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

fn save(file: &Path, reg: &Registry) -> Result<()> {
    let dir = file.parent().context("registry path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let body = format!(
        "# Projects obmem has seen on this machine. Maintained by obmem.\n\n{}",
        toml::to_string(reg)?
    );
    crate::store::atomic_write(dir, file, &body)
}

fn list_at(file: &Path) -> Result<Vec<Project>> {
    let mut v = load(file)?.projects;
    v.sort_by(|a, b| {
        b.last_seen
            .cmp(&a.last_seen)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(v)
}

fn record_at(file: &Path, path: &Path, today: &str) -> Result<()> {
    let path = std::path::absolute(path)?;
    let mut reg = load(file)?;
    match reg.projects.iter_mut().find(|p| p.path == path) {
        Some(p) if p.last_seen == today => return Ok(()),
        Some(p) => p.last_seen = today.to_string(),
        None => reg.projects.push(Project {
            path,
            last_seen: today.to_string(),
        }),
    }
    save(file, &reg)
}

fn remove_at(file: &Path, path: &Path) -> Result<bool> {
    let path = std::path::absolute(path)?;
    let mut reg = load(file)?;
    let before = reg.projects.len();
    reg.projects.retain(|p| p.path != path);
    if reg.projects.len() == before {
        return Ok(false);
    }
    save(file, &reg)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_lists_and_removes() {
        let dir = std::env::temp_dir().join(format!("obmem-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("projects.toml");
        let (a, b) = (dir.join("a"), dir.join("b"));

        assert!(list_at(&file).unwrap().is_empty());
        record_at(&file, &a, "2026-11-01").unwrap();
        record_at(&file, &b, "2026-11-02").unwrap();
        record_at(&file, &a, "2026-11-03").unwrap(); // seen again: updated, not duplicated
        let listed = list_at(&file).unwrap();
        assert_eq!(
            listed.iter().map(|p| p.path.clone()).collect::<Vec<_>>(),
            vec![a.clone(), b.clone()]
        );
        assert_eq!(listed[0].last_seen, "2026-11-03");

        let written = std::fs::metadata(&file).unwrap().modified().unwrap();
        record_at(&file, &a, "2026-11-03").unwrap(); // same day: no write
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            written
        );

        assert!(remove_at(&file, &b).unwrap());
        assert!(!remove_at(&file, &b).unwrap());
        assert_eq!(list_at(&file).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
