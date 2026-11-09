//! `obmem import-claude`: bring Claude Code's file-based memory
//! (`~/.claude/projects/<project>/memory/*.md`) into the vaults as draft notes.
//!
//! Claude memory files carry `name`, `description` and `metadata.type` (user | feedback |
//! project | reference) and link each other with `[[name]]`; the note keeps `name` as its
//! filename so those links still resolve, and uses `description` as the title.

use crate::note::{Note, NoteType, Scope};
use crate::store::{Store, WriteOutcome, WriteRequest};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub const IMPORT_AUTHOR: &str = "agent:import";

/// Claude Code keeps per-project memory under the project path with every
/// non-alphanumeric character replaced by `-`.
pub fn default_dir(project_root: &Path) -> PathBuf {
    let enc: String = project_root
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    crate::config::home()
        .join(".claude/projects")
        .join(enc)
        .join("memory")
}

#[derive(Debug, PartialEq)]
pub struct Planned {
    pub source: PathBuf,
    pub name: String,
    pub note_type: NoteType,
    pub scope: Scope,
    pub title: String,
    pub tags: Vec<String>,
    pub body: String,
}

pub fn map_type(t: &str) -> (NoteType, Scope, Option<&'static str>) {
    match t {
        "user" => (NoteType::Preference, Scope::Global, None),
        "feedback" => (NoteType::Feedback, Scope::Global, None),
        "reference" => (NoteType::Fact, Scope::Project, Some("reference")),
        _ => (NoteType::Fact, Scope::Project, None), // "project" and anything unknown
    }
}

pub fn plan(dir: &Path) -> Result<Vec<Planned>> {
    if !dir.is_dir() {
        bail!(
            "{} not found (pass --dir to point at a Claude Code memory folder)",
            dir.display()
        );
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "md")
                && p.file_name().is_some_and(|n| n != "MEMORY.md")
        })
        .collect();
    files.sort();
    let mut out = Vec::new();
    for p in files {
        let note = Note::parse(&std::fs::read_to_string(&p)?)?;
        let extra = |k: &str| {
            note.fm
                .extra
                .get(k)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        let name =
            extra("name").unwrap_or_else(|| p.file_stem().unwrap().to_string_lossy().into_owned());
        let ty = note
            .fm
            .extra
            .get("metadata")
            .and_then(|m| m.get("type"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| note.fm.note_type.clone())
            .unwrap_or_else(|| "project".into());
        let (note_type, scope, tag) = map_type(&ty);
        let mut tags = vec!["imported/claude-code".to_string()];
        tags.extend(tag.map(String::from));
        out.push(Planned {
            title: extra("description")
                .or(note.fm.title.clone())
                .unwrap_or_else(|| name.clone()),
            body: format!(
                "{}\n\n> Imported from Claude Code memory (`{}`, type `{ty}`).\n",
                note.body.trim(),
                p.display()
            ),
            source: p,
            name,
            note_type,
            scope,
            tags,
        });
    }
    Ok(out)
}

/// Where an import would write the note. An existing file there means it is skipped.
pub fn target(store: &Store, it: &Planned) -> PathBuf {
    store
        .vault_for(it.scope)
        .join(it.note_type.folder())
        .join(format!("{}.md", crate::note::slugify(&it.name)))
}

pub enum ImportResult {
    Imported(String),
    Skipped(String),
}

/// Write the planned notes. Existing notes with the same filename are left alone, so running
/// it again is safe.
pub fn run(store: &mut Store, items: Vec<Planned>) -> Result<Vec<(PathBuf, ImportResult)>> {
    let mut out = Vec::new();
    for it in items {
        let target = target(store, &it);
        if target.exists() {
            out.push((
                it.source,
                ImportResult::Skipped(format!("{} already exists", target.display())),
            ));
            continue;
        }
        let req = WriteRequest {
            note_type: it.note_type,
            title: it.title,
            body: it.body,
            scope: Some(it.scope),
            tags: it.tags,
            code_refs: vec![],
            confidence: Some("medium".into()),
            update_id: None,
            force_new: true,
            author: IMPORT_AUTHOR.into(),
            file_stem: Some(it.name),
        };
        let r = match store.write(req) {
            Ok(WriteOutcome::Written { path, .. }) => {
                ImportResult::Imported(path.display().to_string())
            }
            Ok(WriteOutcome::Duplicate { .. }) => unreachable!("force_new"),
            Err(e) => ImportResult::Skipped(format!("{e:#}")),
        };
        out.push((it.source, r));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_claude_memory_files() {
        let dir = std::env::temp_dir().join(format!("obmem-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("MEMORY.md"), "- [x](x.md) — index").unwrap();
        std::fs::write(
            dir.join("prefer-x.md"),
            "---\nname: prefer-x\ndescription: Prefer X over Y\nmetadata:\n  type: feedback\n---\nUse X. See [[setup]].\n",
        )
        .unwrap();
        std::fs::write(dir.join("setup.md"), "---\nname: setup\ndescription: Where things live\nmetadata:\n  type: reference\n---\nPaths.\n").unwrap();
        let p = plan(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(p.len(), 2); // MEMORY.md is the index, not a memory
        assert_eq!(
            (p[0].name.as_str(), p[0].note_type, p[0].scope),
            ("prefer-x", NoteType::Feedback, Scope::Global)
        );
        assert_eq!(p[0].title, "Prefer X over Y");
        assert!(p[0].body.contains("[[setup]]"));
        assert_eq!(
            (p[1].note_type, p[1].scope),
            (NoteType::Fact, Scope::Project)
        );
        assert!(p[1].tags.contains(&"reference".to_string()));
        assert_eq!(
            default_dir(Path::new("/Users/me/Documents/GitHub/obmem")),
            crate::config::home().join(".claude/projects/-Users-me-Documents-GitHub-obmem/memory")
        );
    }
}
