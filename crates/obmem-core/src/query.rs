//! Obsidian's search syntax, for a machine without the app ([`crate::headless`]).
//!
//! Supported, with Obsidian's meaning: words (all must match), `"exact phrases"`, `OR`,
//! `-exclusion`, `( )`, `file:`, `path:`, `content:`, `tag:`, `line:( )`, `[property]` and
//! `[property:value]`. As in Obsidian, matching ignores case and finds text inside words, a bare
//! word is looked for in the file's name and its content, and `a OR b c` means `a OR (b c)`.
//!
//! Not supported: regular expressions (`/…/`) and `block:`, `section:`, `task:`, `task-todo:`,
//! `task-done:`, `match-case:`, `ignore-case:`. A query that uses one is refused, because
//! searching for the operator's name as a word would return the wrong notes without saying so.

use crate::note::{Note, strip_code};
use anyhow::{Result, bail};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

const SUPPORTED: &str = "words, \"phrases\", OR, -exclusion, ( ), file:, path:, content:, tag:, \
                         line:( ), [property] and [property:value]";

/// Obsidian operators this search does not have.
const UNSUPPORTED: &[&str] = &[
    "block",
    "section",
    "task",
    "task-todo",
    "task-done",
    "match-case",
    "ignore-case",
];

/// The part of a note that a word must be found in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Field {
    /// The file's name.
    Name,
    /// The vault-relative path.
    Path,
    /// Any line of the file.
    Content,
    /// One line: every word of the sub-query on the same line.
    Line,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Expr {
    /// A word or phrase, lowercased.
    Text(String),
    In(Field, Box<Expr>),
    /// A tag, lowercased and without `#`. It also matches the tags nested under it.
    Tag(String),
    /// The note has this property, and, with a sub-query, one of its values matches it.
    Property(String, Option<Box<Expr>>),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

pub(crate) fn parse(query: &str) -> Result<Expr> {
    let mut p = Parser {
        chars: query.chars().collect(),
        at: 0,
    };
    let expr = p.or()?;
    if p.peek() == Some(')') {
        bail!("`)` without a matching `(` in the search query");
    }
    Ok(expr)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    /// `OR` as a word of its own, in capitals.
    fn at_or(&self) -> bool {
        self.chars[self.at..].starts_with(&['O', 'R'])
            && self
                .chars
                .get(self.at + 2)
                .is_none_or(|c| c.is_whitespace() || *c == '(')
    }

    fn or(&mut self) -> Result<Expr> {
        let mut alternatives = vec![self.and()?];
        loop {
            self.skip_spaces();
            if !self.at_or() {
                break;
            }
            self.at += 2;
            alternatives.push(self.and()?);
        }
        Ok(one_or(alternatives, Expr::Or))
    }

    fn and(&mut self) -> Result<Expr> {
        let mut parts = Vec::new();
        loop {
            self.skip_spaces();
            if self.peek().is_none_or(|c| c == ')') || self.at_or() {
                break;
            }
            parts.push(self.unary()?);
        }
        if parts.is_empty() {
            bail!("the search query has nothing to search for before or after an `OR` or `( )`");
        }
        Ok(one_or(parts, Expr::And))
    }

    fn unary(&mut self) -> Result<Expr> {
        let negated = self.peek() == Some('-')
            && self
                .chars
                .get(self.at + 1)
                .is_some_and(|c| !c.is_whitespace());
        if negated {
            self.at += 1;
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr> {
        match self.peek() {
            Some('(') => {
                self.at += 1;
                let inner = self.or()?;
                if self.peek() != Some(')') {
                    bail!("`(` without a matching `)` in the search query");
                }
                self.at += 1;
                Ok(inner)
            }
            Some('"') => {
                self.at += 1;
                Ok(Expr::Text(self.take_until('"').to_lowercase()))
            }
            Some('[') => {
                self.at += 1;
                let inside = self.take_until(']');
                let (key, value) = match inside.split_once(':') {
                    Some((k, v)) => (k, Some(Box::new(parse(v)?))),
                    None => (inside.as_str(), None),
                };
                Ok(Expr::Property(key.trim().to_lowercase(), value))
            }
            _ => self.word(),
        }
    }

    /// Everything up to `end`, which is consumed. A missing `end` takes the rest.
    fn take_until(&mut self, end: char) -> String {
        let start = self.at;
        while self.peek().is_some_and(|c| c != end) {
            self.at += 1;
        }
        let text = self.chars[start..self.at].iter().collect();
        self.at = (self.at + 1).min(self.chars.len());
        text
    }

    fn word(&mut self) -> Result<Expr> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| !c.is_whitespace() && !"()\"".contains(c))
        {
            self.at += 1;
            // `path:"a b"` and `line:(a b)`: the operator ends at its colon
            if self.chars[self.at - 1] == ':'
                && is_operator(&self.chars[start..self.at - 1].iter().collect::<String>())
            {
                break;
            }
        }
        let word: String = self.chars[start..self.at].iter().collect();
        if let Some(operator) = word.strip_suffix(':').filter(|o| is_operator(o)) {
            return self.operator(operator);
        }
        if word.len() > 2 && word.starts_with('/') && word.ends_with('/') {
            bail!(
                "regular expressions (`{word}`) are not supported without Obsidian. Supported \
                 here: {SUPPORTED}."
            );
        }
        Ok(Expr::Text(word.to_lowercase()))
    }

    fn operator(&mut self, operator: &str) -> Result<Expr> {
        if UNSUPPORTED.contains(&operator) {
            bail!("`{operator}:` is not supported without Obsidian. Supported here: {SUPPORTED}.");
        }
        if self.peek().is_none_or(|c| c.is_whitespace() || c == ')') {
            bail!("`{operator}:` needs something to look for right after the colon");
        }
        let operand = self.atom()?;
        Ok(match operator {
            "tag" => match operand {
                Expr::Text(t) => Expr::Tag(t.trim_start_matches('#').to_string()),
                _ => bail!("`tag:` takes one tag, such as `tag:#auth`"),
            },
            "file" => Expr::In(Field::Name, Box::new(operand)),
            "path" => Expr::In(Field::Path, Box::new(operand)),
            "content" => Expr::In(Field::Content, Box::new(operand)),
            _ => Expr::In(Field::Line, Box::new(operand)),
        })
    }
}

fn is_operator(word: &str) -> bool {
    ["file", "path", "content", "tag", "line"].contains(&word) || UNSUPPORTED.contains(&word)
}

fn one_or(mut parts: Vec<Expr>, many: fn(Vec<Expr>) -> Expr) -> Expr {
    if parts.len() == 1 {
        parts.remove(0)
    } else {
        many(parts)
    }
}

static INLINE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)(?:^|\s)#([\p{L}\p{N}_/-]+)").unwrap());

/// One note as the search sees it. Everything is lowercased.
pub(crate) struct Doc {
    rel: String,
    name: String,
    lines: Vec<String>,
    /// Frontmatter properties: key → its values as text.
    properties: BTreeMap<String, Vec<String>>,
    /// Frontmatter and inline tags, without `#`.
    tags: Vec<String>,
}

impl Doc {
    /// `rel` is the vault-relative, `/`-separated path of the note whose text is `src`.
    pub fn new(rel: &str, src: &str) -> Self {
        let rel = rel.to_lowercase();
        let note = Note::parse(src).ok();
        let mut properties: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some(yaml_serde::Value::Mapping(map)) =
            note.as_ref().and_then(|n| yaml_serde::to_value(&n.fm).ok())
        {
            for (key, value) in &map {
                if let Some(key) = key.as_str() {
                    let mut values = Vec::new();
                    scalars(value, &mut values);
                    properties.insert(key.to_lowercase(), values);
                }
            }
        }
        let mut tags: Vec<String> = properties
            .get("tags")
            .into_iter()
            .flatten()
            .map(|t| t.trim_start_matches('#').to_string())
            .collect();
        let body = strip_code(note.as_ref().map_or(src, |n| n.body.as_str()));
        tags.extend(
            INLINE_TAG
                .captures_iter(&body)
                .map(|c| c[1].to_lowercase())
                // Obsidian does not take a number for a tag
                .filter(|t| t.chars().any(|c| !c.is_numeric())),
        );
        Doc {
            name: rel.rsplit('/').next().unwrap_or(&rel).to_string(),
            rel,
            lines: src.lines().map(str::to_lowercase).collect(),
            properties,
            tags,
        }
    }
}

/// The text of a property's value: a scalar, or each scalar of a list.
fn scalars(value: &yaml_serde::Value, out: &mut Vec<String>) {
    match value {
        yaml_serde::Value::String(s) => out.push(s.to_lowercase()),
        yaml_serde::Value::Number(n) => out.push(n.to_string()),
        yaml_serde::Value::Bool(b) => out.push(b.to_string()),
        yaml_serde::Value::Sequence(items) => items.iter().for_each(|v| scalars(v, out)),
        _ => {}
    }
}

/// Where a word is looked for.
#[derive(Clone, Copy)]
enum Scope<'a> {
    /// The whole note: its name and its content.
    Note,
    Content,
    Text(&'a str),
}

impl Expr {
    pub fn matches(&self, doc: &Doc) -> bool {
        self.holds(doc, Scope::Note)
    }

    fn holds(&self, doc: &Doc, scope: Scope<'_>) -> bool {
        let in_content = |t: &str| doc.lines.iter().any(|l| l.contains(t));
        match self {
            Expr::Text(t) => match scope {
                Scope::Note => doc.name.contains(t.as_str()) || in_content(t),
                Scope::Content => in_content(t),
                Scope::Text(text) => text.contains(t.as_str()),
            },
            Expr::In(Field::Name, e) => e.holds(doc, Scope::Text(&doc.name)),
            Expr::In(Field::Path, e) => e.holds(doc, Scope::Text(&doc.rel)),
            Expr::In(Field::Content, e) => e.holds(doc, Scope::Content),
            Expr::In(Field::Line, e) => doc.lines.iter().any(|l| e.holds(doc, Scope::Text(l))),
            Expr::Tag(tag) => doc.tags.iter().any(|t| {
                t == tag
                    || t.strip_prefix(tag.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            }),
            Expr::Property(key, None) => doc.properties.contains_key(key),
            Expr::Property(key, Some(e)) => doc
                .properties
                .get(key)
                .is_some_and(|values| values.iter().any(|v| e.holds(doc, Scope::Text(v)))),
            Expr::Not(e) => !e.holds(doc, scope),
            Expr::And(parts) => parts.iter().all(|e| e.holds(doc, scope)),
            Expr::Or(parts) => parts.iter().any(|e| e.holds(doc, scope)),
        }
    }

    /// The lines of a matching note that show why it matched, as (1-based number, trimmed
    /// text) of `src`, which is the text `doc` was made from.
    pub fn matching_lines(&self, doc: &Doc, src: &str) -> Vec<(usize, String)> {
        let mut wanted = Vec::new();
        self.wanted(&mut wanted);
        src.lines()
            .zip(&doc.lines)
            .enumerate()
            .filter(|(_, (_, lower))| {
                wanted.iter().any(|w| match w {
                    Wanted::Word(t) => lower.contains(*t),
                    Wanted::Line(e) => e.holds(doc, Scope::Text(lower)),
                })
            })
            .map(|(i, (line, _))| (i + 1, line.trim().to_string()))
            .collect()
    }

    /// What this query looks for in the content. Exclusions say nothing about a matching line.
    fn wanted<'a>(&'a self, out: &mut Vec<Wanted<'a>>) {
        match self {
            Expr::Text(t) => out.push(Wanted::Word(t)),
            Expr::In(Field::Content, e) => e.wanted(out),
            Expr::In(Field::Line, e) => out.push(Wanted::Line(e)),
            Expr::And(parts) | Expr::Or(parts) => parts.iter().for_each(|e| e.wanted(out)),
            _ => {}
        }
    }
}

enum Wanted<'a> {
    Word(&'a str),
    Line(&'a Expr),
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ntype: decision\ntitle: JWT rotation\nstatus: active\ntags: [auth, security/tokens]\n---\n# JWT rotation\nRotate the signing key every 30 days.\nRefresh tokens are 輪替 too. #ops\n`#notatag` and # heading\n";

    fn hit(query: &str) -> bool {
        parse(query)
            .unwrap()
            .matches(&Doc::new("Decisions/jwt-rotation.md", NOTE))
    }

    #[test]
    fn words_phrases_and_boolean_operators() {
        assert!(hit("rotate key"));
        assert!(!hit("rotate missing"));
        // text inside a word, any case, and in other scripts
        assert!(hit("ROTAT 輪替"));
        assert!(hit("\"signing key\""));
        assert!(!hit("\"key signing\""));
        assert!(hit("missing OR rotate"));
        assert!(!hit("missing OR absent"));
        assert!(hit("rotate -missing"));
        assert!(!hit("rotate -signing"));
        // `a OR b c` is `a OR (b c)`
        assert!(hit("rotate OR missing absent"));
        assert!(!hit("(rotate OR missing) absent"));
        assert!(!hit("-(rotate OR missing)"));
        // lowercase `or` is a word
        assert!(!hit("rotate or signing"));
        // a word also matches the file's name
        assert!(hit("jwt-rotation.md"));
    }

    #[test]
    fn operators() {
        assert!(hit("path:decisions"));
        assert!(!hit("path:Knowledge"));
        assert!(hit("-path:Templates -path:_system"));
        assert!(hit("path:(knowledge OR decisions)"));
        assert!(hit("file:jwt"));
        assert!(!hit("file:decisions"));
        assert!(hit("content:rotate"));
        assert!(!hit("content:jwt-rotation.md"));
        assert!(hit("tag:auth") && hit("tag:#AUTH") && hit("tag:#ops"));
        // a tag covers the tags nested under it, not the ones that only start alike
        assert!(hit("tag:security") && hit("tag:security/tokens"));
        assert!(!hit("tag:sec"));
        // code and headings are not tags
        assert!(!hit("tag:notatag") && !hit("tag:heading"));
        assert!(hit("line:(rotate key)"));
        assert!(!hit("line:(rotate tokens)"));
        assert!(hit("[type:decision]") && hit("[status]"));
        assert!(!hit("[type:fact]") && !hit("[missing]"));
        assert!(hit("[status:draft OR active]"));
        assert!(hit("[tags:auth]"));
        assert!(hit("-[status:archived] -[status:superseded]"));
        // what `Store::search` and `memory_context` send
        assert!(hit(
            "(\"rotate\" OR \"nothing\") [type:decision] -[status:archived] -path:Templates"
        ));
        // not an operator: a word like any other
        assert!(!hit("http://example.com"));
    }

    #[test]
    fn matching_lines_show_what_was_looked_for() {
        let doc = Doc::new("Decisions/jwt-rotation.md", NOTE);
        let lines = |q: &str| parse(q).unwrap().matching_lines(&doc, NOTE);
        assert_eq!(
            lines("signing -missing"),
            vec![(8, "Rotate the signing key every 30 days.".to_string())]
        );
        assert_eq!(
            lines("line:(refresh tokens) OR nothing"),
            vec![(9, "Refresh tokens are 輪替 too. #ops".to_string())]
        );
        // matched by its path or a property: no line to show
        assert!(lines("path:decisions [type:decision]").is_empty());
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        for query in [
            "section:(a b)",
            "task-todo:x",
            "/rot.+/",
            "match-case:Rotate",
        ] {
            let err = parse(query).unwrap_err().to_string();
            assert!(err.contains("not supported without Obsidian"), "{err}");
        }
        for query in ["", "a OR", "()", "(a", "a)", "path:", "tag:(a b)"] {
            assert!(parse(query).is_err(), "{query}");
        }
    }
}
