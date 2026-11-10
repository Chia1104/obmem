//! Search, backlinks and link checks on a machine without the Obsidian app, such as a cloud
//! sandbox: the [`obsidian::Api`](crate::obsidian::Api) that reads the vault's files itself.
//!
//! It answers the way Obsidian does, within these limits:
//! - Search takes the part of Obsidian's syntax that [`crate::query`] lists and refuses the rest.
//! - Only `[[wikilinks]]` count as links, embeds included. Markdown links (`[text](note.md)`) do
//!   not, so a note that is linked only that way is reported as an orphan.
//! - A note's link to itself is not a backlink and does not keep it from being an orphan.
//!
//! Every call reads the vault again, like the catalog: there is no index to go stale.

use crate::links::Names;
use crate::note::{Note, links_in};
use crate::obsidian::{Api, SearchHit, Unresolved};
use crate::query;
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

pub struct Headless;

/// Every file of a vault as (vault-relative `/`-separated path, text when it is a note), by
/// path. Dot-entries (`.obsidian`, `.git`, `.trash`) are left out, as in Obsidian.
fn files(vault: &Path) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = WalkDir::new(vault)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let p = e.path();
            let rel = p
                .strip_prefix(vault)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            let src = p
                .extension()
                .is_some_and(|x| x == "md")
                .then(|| std::fs::read_to_string(p).ok())
                .flatten();
            (rel, src)
        })
        .collect();
    out.sort();
    out
}

/// A vault's links: each note with the files its links open, and the links that open nothing.
struct Graph {
    /// (note, link text, the file it opens), without a note's links to itself.
    links: Vec<(String, String, Option<String>)>,
    notes: Vec<String>,
}

fn graph(vault: &Path) -> Graph {
    let files = files(vault);
    let mut names = Names::default();
    for (rel, _) in &files {
        names.add(rel);
    }
    let mut g = Graph {
        links: Vec::new(),
        notes: Vec::new(),
    };
    for (rel, src) in files {
        let Some(src) = src else { continue };
        let (fm_text, body) = match Note::parse(&src) {
            Ok(n) => src.split_at(src.len() - n.body.len()),
            Err(_) => ("", src.as_str()),
        };
        for link in links_in(fm_text, body) {
            let target = names.resolve(&link).map(str::to_string);
            if target.as_deref() != Some(rel.as_str()) {
                g.links.push((rel.clone(), link, target));
            }
        }
        g.notes.push(rel);
    }
    g
}

impl Api for Headless {
    fn search(&self, vault: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let expr = query::parse(query)?;
        let mut hits = Vec::new();
        for (rel, src) in files(vault) {
            if hits.len() >= limit {
                break;
            }
            let Some(src) = src else { continue };
            let doc = query::Doc::new(&rel, &src);
            if expr.matches(&doc) {
                hits.push(SearchHit {
                    lines: expr.matching_lines(&doc, &src),
                    file: rel,
                });
            }
        }
        Ok(hits)
    }

    fn backlinks(&self, vault: &Path, rel: &str) -> Result<Vec<String>> {
        let mut sources: Vec<String> = graph(vault)
            .links
            .into_iter()
            .filter(|(_, _, target)| target.as_deref() == Some(rel))
            .map(|(source, _, _)| source)
            .collect();
        sources.dedup();
        Ok(sources)
    }

    fn unresolved(&self, vault: &Path) -> Result<Vec<Unresolved>> {
        let mut by_link: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (source, link, target) in graph(vault).links {
            if target.is_none() {
                by_link.entry(link).or_default().push(source);
            }
        }
        Ok(by_link
            .into_iter()
            .map(|(link, sources)| Unresolved { link, sources })
            .collect())
    }

    fn orphans(&self, vault: &Path) -> Result<Vec<String>> {
        let g = graph(vault);
        let linked: HashSet<&str> = g
            .links
            .iter()
            .filter_map(|(_, _, target)| target.as_deref())
            .collect();
        Ok(g.notes
            .iter()
            .filter(|n| !linked.contains(n.as_str()))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn vault(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("obmem-headless-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, content) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        dir
    }

    #[test]
    fn link_graph() {
        let dir = vault(
            "links",
            &[
                ("Index.md", "[[jwt-rotation]] [[Index]] ![[flow.png]]"),
                (
                    "Decisions/jwt-rotation.md",
                    "---\ntype: decision\nsupersedes: \"[[legacy-token]]\"\n---\nSee [[Refresh Tokens|refresh]], [[gone#part]] and `[[in-code]]`.\n```\n[[fenced]]\n```\n",
                ),
                ("Decisions/legacy-token.md", "[[gone]]"),
                (
                    "Knowledge/refresh tokens.md",
                    "[[Decisions/jwt-rotation.md]]",
                ),
                ("Knowledge/lonely.md", "nothing links here"),
                ("assets/flow.png", "not a note"),
                (".obsidian/app.json", "{}"),
                (".trash/old.md", "[[lonely]]"),
            ],
        );
        let h = Headless;
        // a link in the frontmatter counts; one in code does not
        assert_eq!(
            h.backlinks(&dir, "Decisions/legacy-token.md").unwrap(),
            vec!["Decisions/jwt-rotation.md"]
        );
        assert_eq!(
            h.backlinks(&dir, "Decisions/jwt-rotation.md").unwrap(),
            vec!["Index.md", "Knowledge/refresh tokens.md"]
        );
        // an alias, and a different case
        assert_eq!(
            h.backlinks(&dir, "Knowledge/refresh tokens.md").unwrap(),
            vec!["Decisions/jwt-rotation.md"]
        );
        let unresolved = h.unresolved(&dir).unwrap();
        assert_eq!(unresolved.len(), 1, "{unresolved:?}");
        assert_eq!(unresolved[0].link, "gone");
        assert_eq!(
            unresolved[0].sources,
            vec!["Decisions/jwt-rotation.md", "Decisions/legacy-token.md"]
        );
        // Index links only to itself; the trash's link to `lonely` is not seen
        assert_eq!(
            h.orphans(&dir).unwrap(),
            vec!["Index.md", "Knowledge/lonely.md"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_reads_the_notes() {
        let dir = vault(
            "search",
            &[
                (
                    "Decisions/jwt-rotation.md",
                    "---\ntype: decision\nstatus: active\n---\nRotate the signing key.\n",
                ),
                (
                    "Knowledge/old.md",
                    "---\ntype: fact\nstatus: archived\n---\nThe signing key was static.\n",
                ),
                ("Templates/decision.md", "signing key template"),
                ("assets/signing.txt", "signing key"),
                (".obsidian/signing.md", "signing key"),
            ],
        );
        let h = Headless;
        let files = |q: &str, limit| -> Vec<String> {
            h.search(&dir, q, limit)
                .unwrap()
                .into_iter()
                .map(|hit| hit.file)
                .collect()
        };
        assert_eq!(
            files("signing", 10),
            vec![
                "Decisions/jwt-rotation.md",
                "Knowledge/old.md",
                "Templates/decision.md"
            ]
        );
        assert_eq!(files("signing", 1), vec!["Decisions/jwt-rotation.md"]);
        assert_eq!(
            files("(signing) -[status:archived] -path:Templates", 10),
            vec!["Decisions/jwt-rotation.md"]
        );
        let hit = &h
            .search(&dir, "\"signing key\" [type:decision]", 10)
            .unwrap()[0];
        assert_eq!(hit.lines, vec![(5, "Rotate the signing key.".to_string())]);
        assert!(h.search(&dir, "section:(signing)", 10).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
