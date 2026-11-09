//! Write-time secret scanning.
//!
//! Three layers, cheapest first:
//! 1. **Known values** — the actual secret values from the project's `.env*` files and
//!    sensitive-looking env vars, matched exactly with Aho-Corasick. Catches any format
//!    (including in-house tokens no regex knows about) with zero false positives. Values are
//!    held in memory only, never persisted.
//! 2. **Format rules** — gitleaks-compatible TOML rules for tokens with fixed prefixes
//!    (`ghp_`, `sk-ant-`, `AKIA`, PEM headers, …). A keyword pre-filter means a rule's regex
//!    only runs when one of its keywords appears.
//! 3. **Context + entropy** — `password = <value>` style assignments where the value's
//!    Shannon entropy is high enough to look random, which filters out `password = changeme`.
//!
//! Placeholders (`xxx`, `<YOUR_KEY>`, `${VAR}`, `example`, …) and fingerprints listed in
//! `<vault>/_system/secret-allowlist.toml` are ignored.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

const BUILTIN_RULES: &str = include_str!("secret_rules.toml");

#[derive(Debug, Deserialize, Default)]
struct RuleFile {
    #[serde(default)]
    rules: Vec<RuleDef>,
    #[serde(default)]
    allowlist: Option<AllowlistDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleDef {
    id: String,
    #[serde(default)]
    description: String,
    /// Some gitleaks rules are path-only (no regex); those are skipped here.
    regex: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    entropy: Option<f64>,
    secret_group: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct AllowlistDef {
    #[serde(default)]
    regexes: Vec<String>,
    #[serde(default)]
    stopwords: Vec<String>,
    #[serde(default)]
    fingerprints: Vec<String>,
}

struct Rule {
    id: String,
    description: String,
    re: Regex,
    keywords: Vec<String>,
    entropy: Option<f64>,
    secret_group: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SecretHit {
    pub rule: String,
    pub description: String,
    pub line: usize,
    /// Masked preview, never the full value.
    pub preview: String,
    /// Stable id to put in the allowlist if this is a false positive.
    pub fingerprint: String,
    #[serde(skip)]
    pub start: usize,
    #[serde(skip)]
    pub end: usize,
}

pub struct Scanner {
    rules: Vec<Rule>,
    keyword_ac: Option<AhoCorasick>,
    /// keyword index -> rule indices
    keyword_rules: Vec<Vec<usize>>,
    known: Option<AhoCorasick>,
    known_count: usize,
    allow_regexes: Vec<Regex>,
    stopwords: Vec<String>,
    allow_fingerprints: HashSet<String>,
}

pub struct ScannerSources<'a> {
    pub extra_rule_files: Vec<&'a Path>,
    pub allowlist_files: Vec<&'a Path>,
    /// Files (from the lists above) whose allowlist regexes and stopwords are honored. All
    /// other files are treated as untrusted and contribute exact fingerprints only.
    pub trusted_files: Vec<&'a Path>,
    pub env_files: Vec<std::path::PathBuf>,
    pub scan_process_env: bool,
}

/// Strings an allowlist entry must not match: if it does, it would silence the scanner for
/// (nearly) everything rather than for one known false positive.
// Token-shaped fixtures are split with concat! so the source never contains a literal that
// GitHub push protection (or any secret scanner) would flag; the compiled strings are unchanged.
const ALLOWLIST_PROBES: &[&str] = &[
    "",
    "Zx9qL2vR7tYp4mK8wB3nQ6sD",
    concat!("gh", "p_Qm4Zr8Lx2Vb6Nc9Tw3Yh7Kp1Sd5Fg0Jk2LmA"),
];
const MIN_STOPWORD_LEN: usize = 4;

fn read_rule_file(p: &Path) -> Result<Option<RuleFile>> {
    if !p.exists() {
        return Ok(None);
    }
    let f = toml::from_str(&std::fs::read_to_string(p)?)
        .with_context(|| format!("parsing {}", p.display()))?;
    Ok(Some(f))
}

fn merge_allowlist(into: &mut AllowlistDef, a: AllowlistDef, src: &Path, trusted: bool) {
    into.fingerprints.extend(a.fingerprints);
    if !trusted {
        if !a.regexes.is_empty() || !a.stopwords.is_empty() {
            tracing::warn!(
                "{}: ignoring allowlist regexes/stopwords from the project vault (untrusted); \
                 use fingerprints, or put broad entries in the global vault's allowlist",
                src.display()
            );
        }
        return;
    }
    for r in a.regexes {
        match regex_problem(&r) {
            Some(why) => tracing::warn!("{}: ignoring allowlist regex {r:?}: {why}", src.display()),
            None => into.regexes.push(r),
        }
    }
    for w in a.stopwords {
        let w = w.trim().to_lowercase();
        match stopword_problem(&w) {
            Some(why) => tracing::warn!("{}: ignoring stopword {w:?}: {why}", src.display()),
            None => into.stopwords.push(w),
        }
    }
}

/// Why the scanner would ignore this allowlist regex, if it would.
pub fn regex_problem(r: &str) -> Option<String> {
    match Regex::new(r) {
        Ok(re) if ALLOWLIST_PROBES.iter().any(|p| re.is_match(p)) => {
            Some("it matches almost anything".into())
        }
        Ok(_) => None,
        Err(e) => Some(format!("it is not a valid regex: {e}")),
    }
}

/// Why the scanner would ignore this allowlist stopword (compared lower-cased), if it would.
pub fn stopword_problem(w: &str) -> Option<String> {
    let w = w.trim().to_lowercase();
    if w.chars().count() < MIN_STOPWORD_LEN {
        Some(format!("it is shorter than {MIN_STOPWORD_LEN} characters"))
    } else if ALLOWLIST_PROBES
        .iter()
        .any(|p| p.to_lowercase().contains(&w))
    {
        Some("it is too generic".into())
    } else {
        None
    }
}

impl Scanner {
    pub fn new(src: ScannerSources<'_>) -> Result<Self> {
        let mut defs: Vec<RuleDef> = Vec::new();
        let mut allow = AllowlistDef::default();

        let builtin: RuleFile = toml::from_str(BUILTIN_RULES).context("built-in secret rules")?;
        let builtin_ids: HashSet<String> = builtin.rules.iter().map(|r| r.id.clone()).collect();
        defs.extend(builtin.rules);
        // Vault files may come from an untrusted clone: they can add rules, never weaken
        // built-in ones, and allowlist files contribute only allowlist entries.
        for p in &src.extra_rule_files {
            let Some(f) = read_rule_file(p)? else {
                continue;
            };
            for r in f.rules {
                if builtin_ids.contains(&r.id) {
                    tracing::warn!(
                        "{}: ignoring rule '{}', built-in rules cannot be overridden",
                        p.display(),
                        r.id
                    );
                } else {
                    defs.push(r);
                }
            }
            if let Some(a) = f.allowlist {
                merge_allowlist(&mut allow, a, p, src.trusted_files.contains(p));
            }
        }
        for p in &src.allowlist_files {
            let Some(f) = read_rule_file(p)? else {
                continue;
            };
            if !f.rules.is_empty() {
                tracing::warn!("{}: ignoring [[rules]] in an allowlist file", p.display());
            }
            if let Some(a) = f.allowlist {
                merge_allowlist(&mut allow, a, p, src.trusted_files.contains(p));
            }
        }

        // Among extra rule files, later definitions with the same id override earlier ones.
        let mut seen = HashSet::new();
        let mut rules = Vec::new();
        for d in defs.into_iter().rev() {
            if !seen.insert(d.id.clone()) {
                continue;
            }
            let Some(re_src) = d.regex else { continue };
            match Regex::new(&re_src) {
                Ok(re) => rules.push(Rule {
                    id: d.id,
                    description: d.description,
                    re,
                    keywords: d.keywords.iter().map(|k| k.to_lowercase()).collect(),
                    entropy: d.entropy,
                    secret_group: d.secret_group,
                }),
                // gitleaks uses Go RE2; nearly all of it is valid Rust `regex`. Skip the rest.
                Err(e) => tracing::warn!("skipping secret rule {}: {e}", d.id),
            }
        }
        rules.reverse();

        let mut keywords: Vec<String> = Vec::new();
        let mut keyword_rules: Vec<Vec<usize>> = Vec::new();
        for (ri, r) in rules.iter().enumerate() {
            for k in &r.keywords {
                match keywords.iter().position(|x| x == k) {
                    Some(ki) => keyword_rules[ki].push(ri),
                    None => {
                        keywords.push(k.clone());
                        keyword_rules.push(vec![ri]);
                    }
                }
            }
        }
        let keyword_ac = if keywords.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .build(&keywords)?,
            )
        };

        let known_values = collect_known_values(&src.env_files, src.scan_process_env);
        let known_count = known_values.len();
        let known = if known_values.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(&known_values)?,
            )
        };

        Ok(Self {
            rules,
            keyword_ac,
            keyword_rules,
            known,
            known_count,
            allow_regexes: allow
                .regexes
                .iter()
                .filter_map(|r| Regex::new(r).ok())
                .collect(),
            stopwords: allow.stopwords,
            allow_fingerprints: allow.fingerprints.into_iter().collect(),
        })
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn known_value_count(&self) -> usize {
        self.known_count
    }

    pub fn scan(&self, text: &str) -> Vec<SecretHit> {
        let mut hits: Vec<SecretHit> = Vec::new();

        // 1. exact known values
        if let Some(ac) = &self.known {
            for m in ac.find_iter(text) {
                self.push_hit(
                    &mut hits,
                    text,
                    m.start(),
                    m.end(),
                    "known-secret-value",
                    "Exact value of a secret from .env / environment",
                );
            }
        }

        // 2+3. rules, gated by keyword pre-filter
        let mut active: HashSet<usize> = HashSet::new();
        if let Some(ac) = &self.keyword_ac {
            for m in ac.find_iter(text) {
                active.extend(self.keyword_rules[m.pattern().as_usize()].iter().copied());
            }
        }
        for (ri, rule) in self.rules.iter().enumerate() {
            if !rule.keywords.is_empty() && !active.contains(&ri) {
                continue;
            }
            for caps in rule.re.captures_iter(text) {
                let group = rule
                    .secret_group
                    .unwrap_or(if caps.len() > 1 { 1 } else { 0 });
                let Some(m) = caps.get(group).or_else(|| caps.get(0)) else {
                    continue;
                };
                let value = m.as_str();
                if let Some(min) = rule.entropy
                    && shannon_entropy(value) < min
                {
                    continue;
                }
                if is_placeholder(value) {
                    continue;
                }
                self.push_hit(
                    &mut hits,
                    text,
                    m.start(),
                    m.end(),
                    &rule.id,
                    &rule.description,
                );
            }
        }

        hits.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
        // drop hits fully contained in an earlier one (same secret matched by two layers)
        let mut out: Vec<SecretHit> = Vec::new();
        for h in hits {
            if out
                .last()
                .is_some_and(|l| h.start >= l.start && h.end <= l.end)
            {
                continue;
            }
            out.push(h);
        }
        out
    }

    fn push_hit(
        &self,
        hits: &mut Vec<SecretHit>,
        text: &str,
        start: usize,
        end: usize,
        rule: &str,
        desc: &str,
    ) {
        let value = &text[start..end];
        let fingerprint = fingerprint(value);
        if self.allow_fingerprints.contains(&fingerprint) {
            return;
        }
        let lower = value.to_lowercase();
        if self.stopwords.iter().any(|s| lower.contains(s)) {
            return;
        }
        if self.allow_regexes.iter().any(|r| r.is_match(value)) {
            return;
        }
        hits.push(SecretHit {
            rule: rule.to_string(),
            description: desc.to_string(),
            line: text[..start].matches('\n').count() + 1,
            preview: mask(value),
            fingerprint,
            start,
            end,
        });
    }

    /// Replace every hit with `[REDACTED:<rule>]`. A private key is redacted through its END
    /// line (see [`redaction_end`]). Spans that overlap become one, named after the first hit.
    pub fn redact(text: &str, hits: &[SecretHit]) -> String {
        let mut spans: Vec<(usize, usize, &str)> = hits
            .iter()
            .map(|h| (h.start, redaction_end(text, h), h.rule.as_str()))
            .collect();
        spans.sort_by_key(|&(start, end, _)| (start, std::cmp::Reverse(end)));
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        for (start, end, rule) in spans {
            if start >= pos {
                out.push_str(&text[pos..start]);
                out.push_str(&format!("[REDACTED:{rule}]"));
            }
            pos = pos.max(end);
        }
        out.push_str(&text[pos..]);
        out
    }
}

/// The BEGIN line of a PEM private key, as the built-in `private-key` rule matches it, and the
/// END line that closes the key.
static KEY_BEGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN[ A-Z0-9_-]{0,100}PRIVATE KEY(?: BLOCK)?-----").unwrap()
});
static KEY_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-----END[ A-Z0-9_-]{0,100}PRIVATE KEY(?: BLOCK)?-----").unwrap());

/// Where the redaction of a hit ends. A hit that holds a private key's BEGIN line (the
/// `private-key` rule, or a known value that is that line) is followed by the key material,
/// laid out however the text lays it out. So the redaction runs through the next END line, or
/// to the end of the text when there is none: whatever the layout, no part of the key is kept.
fn redaction_end(text: &str, h: &SecretHit) -> usize {
    match KEY_BEGIN.find_iter(&text[h.start..h.end]).last() {
        Some(b) => KEY_END
            .find_at(text, h.start + b.end())
            .map_or(text.len(), |e| e.end().max(h.end)),
        None => h.end,
    }
}

pub fn shannon_entropy(s: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    let n = s.chars().count() as f64;
    if n == 0.0 {
        return 0.0;
    }
    for c in s.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    counts
        .values()
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

fn is_placeholder(v: &str) -> bool {
    let l = v.to_lowercase();
    const MARKERS: &[&str] = &[
        "xxxx",
        "****",
        "example",
        "placeholder",
        "changeme",
        "dummy",
        "your_",
        "your-",
        "<",
        "${",
        "{{",
        "redacted",
        "sample",
        "fake",
        "test1234",
        "replace_me",
        "replaceme",
    ];
    if MARKERS.iter().any(|m| l.contains(m)) {
        return true;
    }
    let first = l.chars().next();
    l.chars().all(|c| Some(c) == first)
}

fn fingerprint(v: &str) -> String {
    hex::encode(Sha256::digest(v.as_bytes()))[..16].to_string()
}

fn mask(v: &str) -> String {
    let n = v.chars().count();
    let head: String = v.chars().take(if n > 12 { 4 } else { 2 }).collect();
    format!("{head}…({n} chars)")
}

const SENSITIVE_NAME: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "PWD",
    "CREDENTIAL",
    "DSN",
    "PRIVATE",
    "AUTH",
];

/// Names that contain a sensitive-looking word but hold paths: `PWD` (cwd, not a password),
/// `SSH_AUTH_SOCK` (agent socket).
const NOT_SENSITIVE: &[&str] = &["PWD", "OLDPWD", "SSH_AUTH_SOCK"];

fn is_sensitive_name(name: &str) -> bool {
    let u = name.to_uppercase();
    !NOT_SENSITIVE.contains(&u.as_str()) && SENSITIVE_NAME.iter().any(|s| u.contains(s))
}

/// A path to a key file is not the secret (the file's content is), and paths are everywhere in notes.
fn is_existing_path(v: &str) -> bool {
    v.starts_with(['/', '~', '.']) && std::path::Path::new(v).exists()
}

fn usable_value(v: &str) -> bool {
    v.len() >= 8 && !is_placeholder(v) && !matches!(v, "true" | "false") && !is_existing_path(v)
}

/// Collect real secret values. Every value from `.env*` files counts (they exist to hold
/// config, and a non-secret false positive there is harmless); from the process environment
/// only variables whose *name* looks sensitive, to avoid flagging PATH and friends.
fn collect_known_values(env_files: &[std::path::PathBuf], scan_process_env: bool) -> Vec<String> {
    let mut vals: HashSet<String> = HashSet::new();
    for f in env_files {
        let Ok(s) = std::fs::read_to_string(f) else {
            continue;
        };
        for line in s.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            if let Some((_, v)) = line.split_once('=') {
                let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
                if usable_value(v) {
                    vals.insert(v.to_string());
                }
            }
        }
    }
    if scan_process_env {
        for (k, v) in std::env::vars() {
            if is_sensitive_name(&k) && usable_value(&v) {
                vals.insert(v);
            }
        }
    }
    vals.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanner_with_env(env: &str) -> Scanner {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("obmem-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(".env");
        std::fs::write(&f, env).unwrap();
        Scanner::new(ScannerSources {
            extra_rule_files: vec![],
            allowlist_files: vec![],
            trusted_files: vec![],
            env_files: vec![f],
            scan_process_env: false,
        })
        .unwrap()
    }

    #[test]
    fn catches_prefixed_tokens() {
        let s = scanner_with_env("");
        let t = concat!(
            "token is gh",
            "p_abcdefghijklmnopqrstuvwxyz0123456789 ok\nkey AKIA",
            "IOSFODNN7EXAMPLF"
        );
        let hits = s.scan(t);
        let ids: Vec<_> = hits.iter().map(|h| h.rule.as_str()).collect();
        assert!(ids.contains(&"github-token"), "{ids:?}");
        assert!(ids.contains(&"aws-access-key-id"), "{ids:?}");
        assert!(!hits[0].preview.contains("abcdefghij"));
    }

    #[test]
    fn known_value_any_format() {
        let s = scanner_with_env("INTERNAL_THING=acme.v1.Zq81kfPQ\n");
        let hits = s.scan("we call the api with acme.v1.Zq81kfPQ header");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rule, "known-secret-value");
    }

    #[test]
    fn entropy_filters_weak_values() {
        let s = scanner_with_env("");
        assert!(s.scan("password: aaaaaaaaaaaaaaaa").is_empty());
        assert!(s.scan("password = changeme12345678").is_empty());
        assert_eq!(s.scan("client_secret = \"Xk92jfL0qPz7RtWm4vBn\"").len(), 1);
    }

    #[test]
    fn placeholders_and_prose_ignored() {
        let s = scanner_with_env("");
        assert!(s.scan("set API_KEY=<YOUR_KEY_HERE> in .env").is_empty());
        assert!(
            s.scan("the token refresh flow rotates every 15 minutes")
                .is_empty()
        );
        assert!(
            s.scan("postgres://user:${DB_PASSWORD}@localhost/db")
                .is_empty()
        );
    }

    #[test]
    fn redact_replaces() {
        let s = scanner_with_env("");
        let t = concat!("use sk_li", "ve_abcdefghijklmnopqrstuvwx now");
        let hits = s.scan(t);
        assert_eq!(Scanner::redact(t, &hits), "use [REDACTED:stripe-key] now");
    }

    const KEY_HEAD: &str = concat!("-----BEGIN ", "RSA PRIVATE KEY-----");
    const KEY_TAIL: &str = concat!("-----END ", "RSA PRIVATE KEY-----");
    const KEY_BODY: [&str; 2] = [
        "Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MGFiY2RlZmdoaWprbG1ub3Bx",
        "cXdlcnR5dWlvcGFzZGZnaGprbHp4Y3Zibm0xMjM0NTY3ODkwUVdF",
    ];

    fn assert_no_key(redacted: &str) {
        assert!(
            KEY_BODY.iter().all(|b| !redacted.contains(b)) && !redacted.contains("-----END"),
            "{redacted}"
        );
    }

    #[test]
    fn redact_removes_the_whole_private_key_in_any_layout() {
        let s = scanner_with_env("");
        let [b1, b2] = KEY_BODY;
        let layouts = [
            format!("{KEY_HEAD}\n{b1}\n{b2}\n{KEY_TAIL}"),
            format!("{KEY_HEAD} \n{b1}\n{b2}\n{KEY_TAIL}"),
            format!("{KEY_HEAD} {b1} {b2} {KEY_TAIL}"),
            format!("> {KEY_HEAD}\n> {b1}\n> {b2}\n> {KEY_TAIL}"),
            format!("# {KEY_HEAD}\n# {b1}\n# {b2}\n# {KEY_TAIL}"),
            format!("{KEY_HEAD}\r{b1}\r{b2}\r{KEY_TAIL}"),
            format!("{{\"key\": \"{KEY_HEAD}\\\\n{b1}\\\\n{b2}\\\\n{KEY_TAIL}\\\\n\"}}"),
            format!(
                "{KEY_HEAD}\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,0A1B2C\n\n{b1}\n{b2}\n{KEY_TAIL}"
            ),
        ];
        for key in layouts {
            let t = format!("before the key\n{key}\nafter the key");
            let hits = s.scan(&t);
            assert_eq!(hits.len(), 1, "{t}");
            // Allowlists and fingerprints still see only the BEGIN line.
            assert_eq!(&t[hits[0].start..hits[0].end], KEY_HEAD);
            let red = Scanner::redact(&t, &hits);
            assert_no_key(&red);
            assert!(
                red.starts_with("before the key\n") && red.ends_with("\nafter the key"),
                "{red}"
            );
            assert!(red.contains("[REDACTED:private-key]"), "{red}");
        }
    }

    #[test]
    fn redact_stops_at_each_keys_end_line() {
        let s = scanner_with_env("");
        let [b1, b2] = KEY_BODY;
        let t = format!("a\n{KEY_HEAD}\n{b1}\n{KEY_TAIL}\nmiddle\n{KEY_HEAD}\n{b2}\n{KEY_TAIL}\nz");
        let hits = s.scan(&t);
        assert_eq!(
            Scanner::redact(&t, &hits),
            "a\n[REDACTED:private-key]\nmiddle\n[REDACTED:private-key]\nz"
        );
    }

    #[test]
    fn redact_without_an_end_line_removes_the_rest_of_the_text() {
        let s = scanner_with_env("");
        let [b1, b2] = KEY_BODY;
        let t = format!(
            concat!(
                "note\n{}\n{}\n{} see also gh",
                "p_abcdefghijklmnopqrstuvwxyz0123456789 ok"
            ),
            KEY_HEAD, b1, b2
        );
        let hits = s.scan(&t);
        // Both secrets are still reported on their own.
        let ids: Vec<_> = hits.iter().map(|h| h.rule.as_str()).collect();
        assert_eq!(ids, ["private-key", "github-token"]);
        assert_eq!(Scanner::redact(&t, &hits), "note\n[REDACTED:private-key]");
    }

    #[test]
    fn redact_removes_the_key_when_its_begin_line_is_a_known_value() {
        // A multi-line .env value: the line parser keeps only the BEGIN line as a known value,
        // and that hit replaces the identical `private-key` one.
        let [b1, b2] = KEY_BODY;
        let s = scanner_with_env(&format!(
            "JWT_PRIVATE_KEY=\"{KEY_HEAD}\n{b1}\n{b2}\n{KEY_TAIL}\"\n"
        ));
        let t = format!("x\n{KEY_HEAD}\n{b1}\n{b2}\n{KEY_TAIL}\ny");
        let hits = s.scan(&t);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rule, "known-secret-value");
        assert_eq!(
            Scanner::redact(&t, &hits),
            "x\n[REDACTED:known-secret-value]\ny"
        );
    }

    #[test]
    fn vault_files_cannot_weaken_the_scanner() {
        let dir = std::env::temp_dir().join(format!("obmem-test-harden-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rules = dir.join("rules.toml");
        // tries to neuter a built-in rule and allowlist everything
        std::fs::write(&rules, "[[rules]]\nid = \"github-token\"\nregex = 'x^'\n\n[allowlist]\nregexes = ['.*']\nstopwords = ['a', 'ghp']\n").unwrap();
        let allow = dir.join("allow.toml");
        std::fs::write(&allow, "[[rules]]\nid = \"extra\"\nregex = 'zzz'\n[allowlist]\nstopwords = ['example-token']\n").unwrap();
        // untrusted (project vault): broad entries are dropped, fingerprints kept
        let untrusted = dir.join("project-allow.toml");
        std::fs::write(
            &untrusted,
            "[allowlist]\nregexes = ['^ghp_']\nstopwords = ['qm4zr8']\nfingerprints = ['abc']\n",
        )
        .unwrap();
        let sc = Scanner::new(ScannerSources {
            extra_rule_files: vec![&rules],
            allowlist_files: vec![&allow, &untrusted],
            trusted_files: vec![&rules, &allow],
            env_files: vec![],
            scan_process_env: false,
        })
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            !sc.scan(concat!(
                "token gh",
                "p_Qm4Zr8Lx2Vb6Nc9Tw3Yh7Kp1Sd5Fg0Jk2LmA here"
            ))
            .is_empty()
        );
        assert_eq!(sc.stopwords, vec!["example-token".to_string()]);
        assert_eq!(sc.allow_regexes.len(), 0);
        assert!(sc.allow_fingerprints.contains("abc"));
        assert!(sc.scan("zzz zzz").is_empty()); // rules from allowlist files are ignored
    }

    #[test]
    fn path_like_env_values_are_not_secrets() {
        assert!(
            !is_sensitive_name("PWD")
                && !is_sensitive_name("OLDPWD")
                && !is_sensitive_name("SSH_AUTH_SOCK")
        );
        assert!(is_sensitive_name("DB_PWD") && is_sensitive_name("GITHUB_TOKEN"));
        let here = std::env::temp_dir();
        assert!(!usable_value(&here.to_string_lossy()));
        assert!(usable_value("/not/an/existing/path/but/a/secret-ish/value"));
    }
}
