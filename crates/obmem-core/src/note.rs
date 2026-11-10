//! Note model: YAML frontmatter + Markdown body, Obsidian-compatible.

use anyhow::{Context, Result};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum NoteType {
    /// A verified fact about the project.
    Fact,
    /// A concept / glossary entry.
    Concept,
    /// How the system is built.
    Architecture,
    /// An architectural decision record: what was chosen and why.
    Decision,
    /// A runbook / how-to.
    Procedure,
    /// Guidance from the user on how agents should work (global by default).
    Feedback,
    /// User preference (global by default).
    Preference,
    /// An audit finding: something stale, contradictory or wrong.
    Finding,
    /// A session log entry.
    Session,
}

impl NoteType {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteType::Fact => "fact",
            NoteType::Concept => "concept",
            NoteType::Architecture => "architecture",
            NoteType::Decision => "decision",
            NoteType::Procedure => "procedure",
            NoteType::Feedback => "feedback",
            NoteType::Preference => "preference",
            NoteType::Finding => "finding",
            NoteType::Session => "session",
        }
    }

    /// Folder inside the vault where this type lives.
    pub fn folder(self) -> &'static str {
        match self {
            NoteType::Fact | NoteType::Concept => "Knowledge",
            NoteType::Architecture => "Architecture",
            NoteType::Decision => "Decisions",
            NoteType::Procedure => "Runbooks",
            NoteType::Feedback => "Global/Feedback",
            NoteType::Preference => "Global/Preferences",
            NoteType::Finding => "Findings",
            NoteType::Session => "Sessions",
        }
    }

    /// Feedback and preferences follow the user across projects.
    pub fn default_scope(self) -> Scope {
        match self {
            NoteType::Feedback | NoteType::Preference | NoteType::Session => Scope::Global,
            _ => Scope::Project,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    Global,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::Global => "global",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub note_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Anything else the human (or a plugin) put in the frontmatter is preserved verbatim.
    #[serde(flatten)]
    pub extra: BTreeMap<String, yaml_serde::Value>,
}

impl Frontmatter {
    /// Notes without an `agent:*` author are treated as human-owned (read-only for agents).
    pub fn is_agent_authored(&self) -> bool {
        self.author
            .as_deref()
            .is_some_and(|a| a.starts_with("agent"))
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub fm: Frontmatter,
    pub body: String,
}

impl Note {
    pub fn parse(src: &str) -> Result<Self> {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        if let Some(rest) = src
            .strip_prefix("---\n")
            .or_else(|| src.strip_prefix("---\r\n"))
        {
            // closing fence: a line that is exactly `---`
            let mut offset = 0;
            for line in rest.split_inclusive('\n') {
                if line.trim_end_matches(['\r', '\n']) == "---" {
                    let yaml = &rest[..offset];
                    let body = &rest[offset + line.len()..];
                    let fm: Frontmatter = if yaml.trim().is_empty() {
                        Frontmatter::default()
                    } else {
                        yaml_serde::from_str(yaml).context("invalid YAML frontmatter")?
                    };
                    return Ok(Note {
                        fm,
                        body: body.to_string(),
                    });
                }
                offset += line.len();
            }
        }
        Ok(Note {
            fm: Frontmatter::default(),
            body: src.to_string(),
        })
    }

    pub fn render(&self) -> Result<String> {
        let yaml = yaml_serde::to_string(&self.fm)?;
        Ok(format!("---\n{}---\n{}", yaml, self.body))
    }
}

static WIKILINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]\|#\n]+)(?:#[^\]\|\n]*)?(?:\|[^\]\n]*)?\]\]").unwrap());

/// Targets of `[[wikilinks]]` (without heading / alias parts).
pub fn wikilinks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = WIKILINK
        .captures_iter(text)
        .map(|c| c[1].trim().to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]*`").unwrap());

/// Drop fenced and inline code so `[[examples]]` in code are not treated as links.
pub fn strip_code(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut fence: Option<&str> = None;
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if t.starts_with("```") {
            fence = Some("```");
            continue;
        }
        if t.starts_with("~~~") {
            fence = Some("~~~");
            continue;
        }
        out.push_str(&INLINE_CODE.replace_all(line, ""));
        out.push('\n');
    }
    out
}

/// The link targets Obsidian counts in a note: `[[wikilinks]]` in its frontmatter block
/// (`fm_text`) and in its body outside code.
pub fn links_in(fm_text: &str, body: &str) -> Vec<String> {
    wikilinks(&format!("{fm_text}{}", strip_code(body)))
}

/// Filename-safe slug that keeps CJK and other letters (Obsidian links by filename).
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let s = out.trim_end_matches('-').to_string();
    let s: String = s.chars().take(80).collect();
    if s.is_empty() { "untitled".into() } else { s }
}

/// Character trigrams of a title (lowercased, letters/digits only).
pub fn title_grams(s: &str) -> std::collections::HashSet<String> {
    let chars: Vec<char> = s
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    if chars.len() < 3 {
        return [chars.iter().collect::<String>()].into_iter().collect();
    }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

pub fn jaccard(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f64 {
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// Character-trigram Jaccard similarity, used to catch near-duplicate titles.
pub fn title_similarity(a: &str, b: &str) -> f64 {
    jaccard(&title_grams(a), &title_grams(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_unknown_fields() {
        let src = "---\ntitle: JWT\ncustom: 42\ntags: [auth]\n---\n# Body\nsee [[other-note|alias]] and [[x#h]]\n";
        let n = Note::parse(src).unwrap();
        assert_eq!(n.fm.title.as_deref(), Some("JWT"));
        assert!(n.fm.extra.contains_key("custom"));
        let out = n.render().unwrap();
        assert!(out.contains("custom: 42"));
        assert_eq!(wikilinks(&n.body), vec!["other-note", "x"]);
    }

    #[test]
    fn no_frontmatter() {
        let n = Note::parse("just text").unwrap();
        assert!(n.fm.title.is_none());
        assert_eq!(n.body, "just text");
    }

    #[test]
    fn slug_keeps_cjk() {
        assert_eq!(slugify("JWT 輪替策略 (v2)"), "jwt-輪替策略-v2");
    }
}
