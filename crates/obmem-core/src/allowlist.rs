//! Editing a vault's secret allowlist (`_system/secret-allowlist.toml`), the false positives the
//! scanner lets through. The scanner decides what counts: the project vault contributes exact
//! fingerprints only, and broad entries (regexes, stopwords) count only in the personal vault.

use crate::secrets::{regex_problem, stopword_problem};
use crate::store::atomic_write;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Value};

pub const FILE: &str = "_system/secret-allowlist.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One exact string, by the fingerprint a blocked write printed.
    Fingerprint,
    /// Values matching a regex (personal vault only).
    Regex,
    /// Values containing a word, case-insensitive (personal vault only).
    Stopword,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Fingerprint => "fingerprints",
            Kind::Regex => "regexes",
            Kind::Stopword => "stopwords",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Allowlist {
    pub fingerprints: Vec<String>,
    pub regexes: Vec<String>,
    pub stopwords: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct File {
    allowlist: Allowlist,
}

pub fn path(vault: &Path) -> PathBuf {
    vault.join(FILE)
}

fn read_doc(vault: &Path) -> Result<DocumentMut> {
    let p = path(vault);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", p.display())),
    };
    text.parse().with_context(|| format!("parsing {}", p.display()))
}

pub fn read(vault: &Path) -> Result<Allowlist> {
    Ok(toml::from_str::<File>(&read_doc(vault)?.to_string())?.allowlist)
}

/// Adds an entry, checked as the scanner checks it. Returns whether it was new.
pub fn add(vault: &Path, kind: Kind, entry: &str) -> Result<bool> {
    let entry = entry.trim();
    let problem = match kind {
        Kind::Fingerprint if entry.is_empty() => Some("it is empty".to_string()),
        Kind::Fingerprint => None,
        Kind::Regex => regex_problem(entry),
        Kind::Stopword => stopword_problem(entry),
    };
    if let Some(why) = problem {
        bail!("the scanner would ignore \"{entry}\": {why}");
    }
    let entry = if kind == Kind::Stopword { entry.to_lowercase() } else { entry.to_string() };
    edit(vault, kind, |list| {
        if list.iter().any(|v| v.as_str() == Some(&entry)) {
            return false;
        }
        list.push(entry.as_str());
        true
    })
}

/// Removes an entry. Returns whether it was there.
pub fn remove(vault: &Path, kind: Kind, entry: &str) -> Result<bool> {
    edit(vault, kind, |list| {
        let before = list.len();
        list.retain(|v| v.as_str() != Some(entry));
        list.len() != before
    })
}

fn edit(vault: &Path, kind: Kind, change: impl FnOnce(&mut Array) -> bool) -> Result<bool> {
    let mut doc = read_doc(vault)?;
    let slot = &mut doc["allowlist"][kind.key()];
    if slot.is_none() {
        *slot = Item::Value(Value::Array(Array::new()));
    }
    let list = slot.as_array_mut().with_context(|| format!("allowlist.{} in {} is not a list", kind.key(), FILE))?;
    if !change(list) {
        return Ok(false);
    }
    atomic_write(vault, &path(vault), &doc.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_and_removes_entries_as_the_scanner_accepts_them() {
        let vault = std::env::temp_dir().join(format!("obmem-allowlist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);
        std::fs::create_dir_all(vault.join("_system")).unwrap();
        std::fs::write(path(&vault), "# keep me\n[allowlist]\nfingerprints = []\n").unwrap();

        assert!(add(&vault, Kind::Fingerprint, "abc123").unwrap());
        assert!(!add(&vault, Kind::Fingerprint, "abc123").unwrap());
        assert!(add(&vault, Kind::Stopword, "Sandbox-Key").unwrap());
        assert!(add(&vault, Kind::Regex, r"^test_[0-9]{4}$").unwrap());
        // What the scanner would ignore is refused up front.
        assert!(add(&vault, Kind::Regex, ".*").is_err());
        assert!(add(&vault, Kind::Regex, "(").is_err());
        assert!(add(&vault, Kind::Stopword, "key").is_err());

        let a = read(&vault).unwrap();
        assert_eq!(a.fingerprints, vec!["abc123"]);
        assert_eq!(a.stopwords, vec!["sandbox-key"]);
        assert_eq!(a.regexes.len(), 1);
        assert!(std::fs::read_to_string(path(&vault)).unwrap().contains("# keep me"));

        assert!(remove(&vault, Kind::Fingerprint, "abc123").unwrap());
        assert!(!remove(&vault, Kind::Fingerprint, "abc123").unwrap());
        assert!(read(&vault).unwrap().fingerprints.is_empty());
        let _ = std::fs::remove_dir_all(&vault);
    }
}
