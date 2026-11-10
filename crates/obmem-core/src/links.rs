//! Which file a `[[link]]` opens, by Obsidian's rules: a link names a file by its filename or by
//! the end of its path, ignoring case, and a note's `.md` is optional.

use std::collections::HashMap;

/// The files of one vault, by the names a link can use.
#[derive(Default)]
pub(crate) struct Names {
    /// Lowercased last path component (a note's without `.md`) → the files that have it.
    by_base: HashMap<String, Vec<Entry>>,
}

struct Entry {
    /// The lowercased vault-relative path, a note's without `.md`.
    key: String,
    rel: String,
}

impl Names {
    /// Add a file by its vault-relative, `/`-separated path.
    pub fn add(&mut self, rel: &str) {
        let lower = rel.to_lowercase();
        let key = lower.strip_suffix(".md").unwrap_or(&lower).to_string();
        let base = key.rsplit('/').next().unwrap_or(&key).to_string();
        self.by_base.entry(base).or_default().push(Entry {
            key,
            rel: rel.to_string(),
        });
    }

    /// The path of the file `[[target]]` opens, if there is one. When several files fit, the one
    /// nearest the vault root wins, then the first by path.
    pub fn resolve(&self, target: &str) -> Option<&str> {
        let t = target.trim().trim_start_matches('/').to_lowercase();
        let t = t.strip_suffix(".md").unwrap_or(&t);
        let base = t.rsplit('/').next()?;
        let suffix = format!("/{t}");
        self.by_base
            .get(base)?
            .iter()
            .filter(|e| e.key == t || e.key.ends_with(&suffix))
            .min_by_key(|e| (e.key.matches('/').count(), &e.rel))
            .map(|e| e.rel.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_like_obsidian() {
        let mut n = Names::default();
        for rel in [
            "Decisions/JWT Rotation.md",
            "Knowledge/auth/tokens.md",
            "Archive/tokens.md",
            "assets/flow.png",
            "_system/Findings.base",
        ] {
            n.add(rel);
        }
        // by filename, any case, `.md` optional
        assert_eq!(n.resolve("jwt rotation"), Some("Decisions/JWT Rotation.md"));
        assert_eq!(
            n.resolve("JWT Rotation.md"),
            Some("Decisions/JWT Rotation.md")
        );
        // by the end of the path
        assert_eq!(n.resolve("auth/tokens"), Some("Knowledge/auth/tokens.md"));
        assert_eq!(
            n.resolve("/Knowledge/auth/tokens"),
            Some("Knowledge/auth/tokens.md")
        );
        // two notes share the name: the one nearest the root
        assert_eq!(n.resolve("tokens"), Some("Archive/tokens.md"));
        // other files need their extension
        assert_eq!(n.resolve("flow.png"), Some("assets/flow.png"));
        assert_eq!(n.resolve("flow"), None);
        assert_eq!(n.resolve("Findings.base"), Some("_system/Findings.base"));
        // a name is whole path components, never part of one
        assert_eq!(n.resolve("okens"), None);
        assert_eq!(n.resolve("th/tokens"), None);
        assert_eq!(n.resolve("missing"), None);
    }
}
