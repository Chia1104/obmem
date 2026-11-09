//! Note catalog: id / title / type / status of every note, read straight from the vault files.
//! Full-text search, backlinks and link resolution are Obsidian's job (see `obsidian.rs`);
//! this only answers "which file is note X" and "which notes are drafts", which Obsidian has no
//! command for. Rescanning is cheap (frontmatter of a few thousand files), so there is no index.

use crate::note::Note;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Hit {
    pub id: String,
    pub scope: String,
    pub path: String,
    pub title: String,
    #[serde(rename = "type")]
    pub note_type: String,
    pub status: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct Meta {
    pub path: PathBuf,
    /// Vault-relative, `/`-separated.
    pub rel: String,
    pub scope: &'static str,
    pub id: String,
    pub title: String,
    pub note_type: String,
    pub status: String,
    pub mtime: SystemTime,
}

pub struct VaultRoot<'a> {
    pub scope: &'static str,
    pub root: &'a Path,
}

#[derive(Default)]
pub struct Catalog {
    notes: Vec<Meta>,
}

impl Catalog {
    pub fn sync(&mut self, roots: &[VaultRoot<'_>]) -> Result<usize> {
        let mut notes = Vec::new();
        for vr in roots {
            if !vr.root.exists() {
                continue;
            }
            // skip .obsidian, .git, .trash and similar
            let walker = WalkDir::new(vr.root)
                .into_iter()
                .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
            for entry in walker.filter_map(|e| e.ok()) {
                let p = entry.path();
                if !entry.file_type().is_file() || p.extension().is_none_or(|e| e != "md") {
                    continue;
                }
                let Ok(src) = std::fs::read_to_string(p) else { continue };
                let fm = Note::parse(&src).map(|n| n.fm).unwrap_or_default();
                let id = p.file_stem().unwrap().to_string_lossy().into_owned();
                notes.push(Meta {
                    rel: p.strip_prefix(vr.root).unwrap_or(p).to_string_lossy().replace('\\', "/"),
                    path: p.to_path_buf(),
                    scope: vr.scope,
                    title: fm.title.clone().unwrap_or_else(|| id.clone()),
                    note_type: fm.note_type.unwrap_or_default(),
                    status: fm.status.unwrap_or_default(),
                    mtime: entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH),
                    id,
                });
            }
        }
        let n = notes.len();
        self.notes = notes;
        Ok(n)
    }

    pub fn by_rel(&self, scope: &str, rel: &str) -> Option<&Meta> {
        self.notes.iter().find(|m| m.scope == scope && m.rel == rel)
    }

    /// Notes matching an id (filename stem), exact title, or vault-relative path (with or
    /// without `.md`). Returns (absolute path, scope).
    pub fn resolve(&self, key: &str) -> Result<Vec<(String, String)>> {
        let key = key.trim();
        let rel = key.trim_start_matches('/');
        Ok(self
            .notes
            .iter()
            .filter(|m| m.id == key || m.title == key || m.rel == rel || m.rel.strip_suffix(".md") == Some(rel))
            .map(|m| (m.path.to_string_lossy().into_owned(), m.scope.to_string()))
            .collect())
    }

    /// Titles of all notes of a type in a scope — used for duplicate detection.
    pub fn titles(&self, note_type: &str, scope: &str) -> Result<Vec<(String, String, PathBuf)>> {
        Ok(self
            .notes
            .iter()
            .filter(|m| m.note_type == note_type && m.scope == scope)
            .map(|m| (m.id.clone(), m.title.clone(), m.path.clone()))
            .collect())
    }

    /// Most recently modified first.
    pub fn list_by_status(&self, status: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
        let mut v: Vec<&Meta> = self.notes.iter().filter(|m| m.status == status).collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.mtime));
        Ok(v.into_iter().take(limit).map(|m| (m.id.clone(), m.title.clone(), m.note_type.clone())).collect())
    }

    pub fn count_by_status(&self, status: &str, scope: &str) -> usize {
        self.notes.iter().filter(|m| m.status == status && m.scope == scope).count()
    }

    /// Feedback and preference notes from the global vault (always relevant).
    pub fn global_guidance(&self) -> Result<Vec<(String, String)>> {
        let mut v = Vec::new();
        for ty in ["feedback", "preference"] {
            v.extend(self.titles(ty, "global")?.into_iter().map(|(id, t, _)| (id, t)));
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_is_exact() {
        let dir = std::env::temp_dir().join(format!("obmem-cat-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Decisions")).unwrap();
        std::fs::write(dir.join("Decisions/a_b.md"), "---\ntitle: A B\nstatus: draft\n---\n").unwrap();
        std::fs::write(dir.join("Decisions/axb.md"), "x").unwrap();
        let mut c = Catalog::default();
        c.sync(&[VaultRoot { scope: "project", root: &dir }]).unwrap();
        assert_eq!(c.resolve("a_b").unwrap().len(), 1);
        assert_eq!(c.resolve("A B").unwrap().len(), 1);
        assert_eq!(c.resolve("Decisions/a_b").unwrap().len(), 1);
        assert_eq!(c.resolve("%").unwrap().len(), 0); // no wildcard semantics
        assert_eq!(c.list_by_status("draft", 5).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
