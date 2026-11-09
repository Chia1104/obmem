//! P2 code drift: does a note's `code_refs` still point at code that exists, and has that code
//! changed since the note was verified (`verified_commit`)?
//!
//! `code_refs` entries look like `src/auth/refresh.rs#rotate_token`, `src/auth/refresh.rs:42`,
//! `src/auth/` or `src/auth/refresh.rs#Store::rotate`. Paths are relative to the project root.

use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRef {
    pub raw: String,
    pub path: String,
    pub symbol: Option<String>,
}

impl CodeRef {
    /// `None` when the ref is not a plain repo-relative path (absolute, `..`, `~`, empty).
    pub fn parse(raw: &str) -> Option<Self> {
        let raw_t = raw.trim();
        let (path, symbol) = match raw_t.split_once('#') {
            Some((p, s)) if !s.trim().is_empty() => (p, Some(s.trim().to_string())),
            Some((p, _)) => (p, None),
            None => (raw_t, None),
        };
        // `file.rs:42`, `file.rs:L42-L50`
        let path = match path.rsplit_once(':') {
            Some((p, line))
                if !line.is_empty()
                    && line
                        .trim_start_matches('L')
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '-' || c == 'L') =>
            {
                p
            }
            _ => path,
        };
        let path = path.trim().trim_start_matches("./").trim_end_matches('/');
        let p = Path::new(path);
        if path.is_empty()
            || path.starts_with('~')
            || p.is_absolute()
            || p.components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        Some(Self {
            raw: raw_t.to_string(),
            path: path.to_string(),
            symbol,
        })
    }

    /// Identifier segments to look for: `Store::rotate` → [Store, rotate].
    fn symbol_parts(&self) -> Vec<&str> {
        self.symbol
            .as_deref()
            .map(|s| {
                s.split([':', '.'])
                    .map(|x| x.trim_end_matches("()"))
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Not a repo-relative path.
    InvalidRef,
    /// The path no longer exists. `deleted_in`: commit that removed it, if git knows.
    Missing { deleted_in: Option<String> },
    /// The file exists but the symbol does not appear in it.
    SymbolMissing { symbol: String },
    /// The path changed since `verified_commit`. `symbol_touched`: the diff mentions the symbol
    /// (always true when the ref has no symbol).
    Changed {
        since: String,
        commits: usize,
        symbol_touched: bool,
    },
    /// `verified_commit` is not in this repository (rebased away, shallow clone, other repo).
    UnknownCommit { commit: String },
}

pub struct Repo {
    root: PathBuf,
    is_git: bool,
    commits: RefCell<HashMap<String, bool>>,
}

impl Repo {
    pub fn new(root: &Path) -> Self {
        let is_git =
            git(root, &["rev-parse", "--is-inside-work-tree"]).is_some_and(|o| o.trim() == "true");
        Self {
            root: root.to_path_buf(),
            is_git,
            commits: RefCell::new(HashMap::new()),
        }
    }

    fn commit_exists(&self, c: &str) -> bool {
        if let Some(v) = self.commits.borrow().get(c) {
            return *v;
        }
        let ok = git(
            &self.root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{c}^{{commit}}"),
            ],
        )
        .is_some();
        self.commits.borrow_mut().insert(c.to_string(), ok);
        ok
    }

    /// Existence + symbol checks only (no history). Used at write time and before verifying.
    pub fn check_present(&self, raw: &str) -> Option<Drift> {
        let Some(r) = CodeRef::parse(raw) else {
            return Some(Drift::InvalidRef);
        };
        let abs = self.root.join(&r.path);
        // Never read through a symlink that leaves the repo.
        if crate::config::ensure_within(&self.root, &abs).is_err() {
            return Some(Drift::InvalidRef);
        }
        if !abs.exists() {
            let deleted_in = self
                .is_git
                .then(|| {
                    git(
                        &self.root,
                        &["log", "-1", "--format=%h", "--diff-filter=D", "--", &r.path],
                    )
                })
                .flatten()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            return Some(Drift::Missing { deleted_in });
        }
        let parts = r.symbol_parts();
        if !parts.is_empty() && abs.is_file() {
            let text = std::fs::read_to_string(&abs).unwrap_or_default();
            if !parts.iter().all(|p| contains_word(&text, p)) {
                return Some(Drift::SymbolMissing {
                    symbol: r.symbol.clone().unwrap_or_default(),
                });
            }
        }
        None
    }

    /// Everything that is wrong with one ref, given the commit the note was verified against.
    pub fn check(&self, raw: &str, verified_commit: Option<&str>) -> Vec<Drift> {
        if let Some(d) = self.check_present(raw) {
            return vec![d];
        }
        let (Some(c), true) = (
            verified_commit.map(str::trim).filter(|c| !c.is_empty()),
            self.is_git,
        ) else {
            return vec![];
        };
        if !self.commit_exists(c) {
            return vec![Drift::UnknownCommit {
                commit: c.to_string(),
            }];
        }
        let r = CodeRef::parse(raw).expect("checked above");
        let Some(diff) = git(
            &self.root,
            &["diff", "--unified=3", c, "HEAD", "--", &r.path],
        ) else {
            return vec![];
        };
        if diff.trim().is_empty() {
            return vec![];
        }
        let commits = git(
            &self.root,
            &["rev-list", "--count", &format!("{c}..HEAD"), "--", &r.path],
        )
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
        let parts = r.symbol_parts();
        let symbol_touched = parts.is_empty() || {
            // Changed lines plus their 3 lines of context. Hunk headers are skipped: without a
            // language diff driver git fills them with whatever function precedes the hunk.
            let relevant: String = diff
                .lines()
                .skip_while(|l| !l.starts_with("@@"))
                .filter(|l| {
                    (l.starts_with('+') || l.starts_with('-') || l.starts_with(' '))
                        && !l.starts_with("+++")
                        && !l.starts_with("---")
                })
                .collect::<Vec<_>>()
                .join("\n");
            parts.iter().any(|p| contains_word(&relevant, p))
        };
        vec![Drift::Changed {
            since: c.to_string(),
            commits,
            symbol_touched,
        }]
    }
}

fn contains_word(text: &str, word: &str) -> bool {
    Regex::new(&format!(r"(?m)(^|[^\w]){}([^\w]|$)", regex::escape(word)))
        .is_ok_and(|re| re.is_match(text))
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Human-readable warnings for `code_refs` that do not resolve right now (write time).
pub fn present_warnings(root: &Path, refs: &[String]) -> Vec<String> {
    let repo = Repo::new(root);
    refs.iter()
        .filter_map(|r| {
            repo.check_present(r).map(|d| match d {
                Drift::Missing { .. } => format!("code_ref `{r}`: path does not exist"),
                Drift::SymbolMissing { symbol } => {
                    format!("code_ref `{r}`: `{symbol}` not found in the file")
                }
                Drift::InvalidRef => format!("code_ref `{r}`: not a repo-relative path"),
                _ => unreachable!(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_refs() {
        let p = |s| CodeRef::parse(s).map(|r| (r.path, r.symbol));
        assert_eq!(
            p("src/a.rs#Store::rotate"),
            Some(("src/a.rs".into(), Some("Store::rotate".into())))
        );
        assert_eq!(p("./src/a.rs:L10-L20"), Some(("src/a.rs".into(), None)));
        assert_eq!(p("src/auth/"), Some(("src/auth".into(), None)));
        assert_eq!(p("../etc/passwd"), None);
        assert_eq!(p("/etc/passwd"), None);
        assert_eq!(p("~/x"), None);
        assert_eq!(p("src/../../x"), None);
    }

    #[test]
    fn drift_against_git_history() {
        let dir = std::env::temp_dir().join(format!("obmem-drift-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let g = |args: &[&str]| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                ok.status.success(),
                "{:?}",
                String::from_utf8_lossy(&ok.stderr)
            );
            String::from_utf8_lossy(&ok.stdout).trim().to_string()
        };
        g(&["init", "-q"]);
        std::fs::write(
            dir.join("src/a.rs"),
            "fn rotate_token() {\n    1\n}\n\nfn other() {\n    2\n}\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/gone.rs"), "x").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "one"]);
        let c1 = g(&["rev-parse", "--short", "HEAD"]);
        let repo = Repo::new(&dir);
        assert!(repo.check("src/a.rs#rotate_token", Some(&c1)).is_empty());

        // change only `other`, delete gone.rs
        std::fs::write(
            dir.join("src/a.rs"),
            "fn rotate_token() {\n    1\n}\n\nfn other() {\n    3\n}\n",
        )
        .unwrap();
        std::fs::remove_file(dir.join("src/gone.rs")).unwrap();
        g(&["commit", "-qam", "two"]);
        let repo = Repo::new(&dir);
        assert_eq!(
            repo.check("src/a.rs#rotate_token", Some(&c1)),
            vec![Drift::Changed {
                since: c1.clone(),
                commits: 1,
                symbol_touched: false
            }]
        );
        assert!(matches!(
            repo.check("src/a.rs#other", Some(&c1))[..],
            [Drift::Changed {
                symbol_touched: true,
                ..
            }]
        ));
        assert!(matches!(
            repo.check("src/gone.rs", Some(&c1))[..],
            [Drift::Missing {
                deleted_in: Some(_)
            }]
        ));
        assert!(matches!(
            repo.check("src/a.rs#nope", Some(&c1))[..],
            [Drift::SymbolMissing { .. }]
        ));
        assert!(matches!(
            repo.check("src/a.rs", Some("deadbeef"))[..],
            [Drift::UnknownCommit { .. }]
        ));
        assert_eq!(repo.check("src/a.rs#rotate_token", None), vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
