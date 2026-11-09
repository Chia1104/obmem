//! What waits for a person: agent drafts to approve and findings to resolve, in both vaults.

use crate::note::Note;
use crate::store::Store;
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An agent's note waiting for approval.
    Draft,
    /// A problem someone has to look at (open, or accepted and not fixed yet).
    Finding,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: Kind,
    /// `project` or `global`.
    pub scope: &'static str,
    /// Vault-relative path; it also names the note in actions (`approve`, `resolve_finding`).
    pub rel: String,
    pub id: String,
    pub title: String,
    pub note_type: String,
    pub status: String,
    pub author: String,
    pub updated: String,
    /// The first lines of the body, without its title.
    pub summary: String,
    /// Findings from the audit: the rule, its severity, and the note the problem is in
    /// (vault-relative, in the same vault).
    pub rule: Option<String>,
    pub severity: Option<String>,
    pub target: Option<String>,
}

impl Store {
    /// Drafts and unresolved findings, findings first, most recently changed first within each.
    pub fn review(&mut self) -> Result<Vec<Item>> {
        self.sync()?;
        let mut items = Vec::new();
        for m in self.notes() {
            let kind = match (m.note_type.as_str(), m.status.as_str()) {
                ("finding", "open" | "accepted") => Kind::Finding,
                ("finding" | "session", _) => continue,
                (_, "draft") => Kind::Draft,
                _ => continue,
            };
            let Ok(note) = std::fs::read_to_string(&m.path)
                .map_err(anyhow::Error::from)
                .and_then(|s| Note::parse(&s))
            else {
                continue;
            };
            let extra = |k: &str| {
                note.fm
                    .extra
                    .get(k)
                    .and_then(|v| v.as_str())
                    .map(String::from)
            };
            items.push((
                m.mtime,
                Item {
                    kind,
                    scope: m.scope,
                    rel: m.rel.clone(),
                    id: m.id.clone(),
                    title: m.title.clone(),
                    note_type: m.note_type.clone(),
                    status: m.status.clone(),
                    author: note.fm.author.clone().unwrap_or_default(),
                    updated: note
                        .fm
                        .updated
                        .clone()
                        .or(note.fm.created.clone())
                        .unwrap_or_default(),
                    summary: summary(&note.body),
                    rule: extra("rule"),
                    severity: extra("severity"),
                    target: extra("target"),
                },
            ));
        }
        items.sort_by_key(|(mtime, item)| (item.kind == Kind::Draft, std::cmp::Reverse(*mtime)));
        Ok(items.into_iter().map(|(_, item)| item).collect())
    }
}

/// The first paragraph of text after the note's own `# title`, cut to a few hundred characters.
fn summary(body: &str) -> String {
    let text: Vec<&str> = body
        .lines()
        .map(str::trim)
        .skip_while(|l| l.is_empty() || l.starts_with("# "))
        .take_while(|l| !l.is_empty())
        .collect();
    let s = text.join(" ");
    if s.chars().count() > 280 {
        format!("{}…", s.chars().take(280).collect::<String>())
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::note::{NoteType, Scope};
    use crate::store::{WriteOutcome, WriteRequest};

    #[test]
    fn lists_drafts_and_unresolved_findings() {
        let dir = std::env::temp_dir().join(format!("obmem-review-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/kb/Findings")).unwrap();
        std::fs::write(dir.join(".obmem.toml"), "").unwrap();
        let finding = |status: &str| {
            format!(
                "---\ntype: finding\ntitle: \"stale: old-note\"\nstatus: {status}\nauthor: agent:audit\nrule: stale\n\
                 severity: warn\ntarget: Knowledge/old-note.md\n---\n# stale: old-note\n\nNot checked for 200 days.\n"
            )
        };
        std::fs::write(dir.join("docs/kb/Findings/a.md"), finding("open")).unwrap();
        std::fs::write(dir.join("docs/kb/Findings/b.md"), finding("fixed")).unwrap();
        let mut store = Store::open(Config {
            project_root: dir.clone(),
            project_name: "p".into(),
            project_vault: dir.join("docs/kb"),
            global_vault: dir.join("global"),
            secrets: Default::default(),
            audit: Default::default(),
            auto_approve: false,
            device: "test".into(),
        })
        .unwrap();
        let req = WriteRequest {
            note_type: NoteType::Decision,
            title: "Use X".into(),
            body: "We chose X.\n\nBecause Y.".into(),
            scope: Some(Scope::Project),
            tags: vec![],
            code_refs: vec![],
            confidence: None,
            update_id: None,
            force_new: false,
            author: "agent:test".into(),
            file_stem: None,
        };
        assert!(matches!(store.write(req), Ok(WriteOutcome::Written { .. })));

        let items = store.review().unwrap();
        assert_eq!(items.len(), 2, "{items:?}"); // the fixed finding is done
        assert_eq!(items[0].kind, Kind::Finding);
        assert_eq!(items[0].target.as_deref(), Some("Knowledge/old-note.md"));
        assert_eq!(items[0].summary, "Not checked for 200 days.");
        assert_eq!(
            (items[1].kind, items[1].title.as_str()),
            (Kind::Draft, "Use X")
        );
        assert_eq!(items[1].summary, "We chose X.");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
