//! The memory store: the write policy (dedupe, human-ownership, secrets) on top of the
//! vault files. Search and backlinks come from Obsidian itself (`obsidian.rs`).

use crate::audit::{self, Audit, AuditOptions, Severity, WriteStats};
use crate::config::{Config, SecretMode};
use crate::catalog::{Catalog, Hit, VaultRoot};
use crate::obsidian;
use crate::note::{Frontmatter, Note, NoteType, Scope, slugify, title_similarity};
use crate::secrets::{Scanner, ScannerSources, SecretHit};
use anyhow::{Result, anyhow, bail};
use std::path::{Path, PathBuf};

pub struct Store {
    pub cfg: Config,
    index: Catalog,
    obs: Box<dyn obsidian::Api>,
    scanner: Scanner,
}

pub struct WriteRequest {
    pub note_type: NoteType,
    pub title: String,
    pub body: String,
    pub scope: Option<Scope>,
    pub tags: Vec<String>,
    pub code_refs: Vec<String>,
    pub confidence: Option<String>,
    /// Update this existing note (id / filename stem) instead of creating one.
    pub update_id: Option<String>,
    /// Create even if a similar note exists.
    pub force_new: bool,
    pub author: String,
    /// Filename (link target) to use instead of one derived from the title (imports keep
    /// their original names so existing [[links]] still resolve).
    pub file_stem: Option<String>,
}

/// What happened to a finding. Only humans may reject (a rejection silences the audit rule
/// for that spot forever, so an agent must not be able to do it on its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Real problem; someone will fix it.
    Accepted,
    /// The problem has been fixed.
    Fixed,
    /// False positive (human only); the audit never reports it again.
    Rejected,
    /// Reopen a closed finding.
    Open,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::Fixed => "fixed",
            Verdict::Rejected => "rejected",
            Verdict::Open => "open",
        }
    }
}

pub enum WriteOutcome {
    Written { id: String, path: PathBuf, created: bool, redactions: Vec<SecretHit>, warnings: Vec<String> },
    Duplicate { candidates: Vec<(String, String, f64)> },
}

impl Store {
    pub fn open(cfg: Config) -> Result<Self> {
        let index = Catalog::default();
        let rule_files = [
            cfg.project_vault.join("_system/secret-rules.toml"),
            cfg.global_vault.join("_system/secret-rules.toml"),
        ];
        let allow_files = [
            cfg.project_vault.join("_system/secret-allowlist.toml"),
            cfg.global_vault.join("_system/secret-allowlist.toml"),
        ];
        let scanner = Scanner::new(ScannerSources {
            extra_rule_files: rule_files.iter().map(|p| p.as_path()).collect(),
            allowlist_files: allow_files.iter().map(|p| p.as_path()).collect(),
            // Only the personal global vault may add broad allowlist entries (regexes, stopwords);
            // project-vault files arrive with clones and contribute exact fingerprints only.
            trusted_files: vec![allow_files[1].as_path(), rule_files[1].as_path()],
            // Repo config chose these (validated as relative in Config::load); also refuse ones that
            // leave the project through a symlink.
            env_files: cfg
                .secrets
                .env_files
                .iter()
                .map(|f| cfg.project_root.join(f))
                .filter(|p| crate::config::ensure_within(&cfg.project_root, p).is_ok())
                .collect(),
            scan_process_env: cfg.secrets.scan_process_env,
        })?;
        let mut s = Self { cfg, index, obs: Box::new(obsidian::Cli::default()), scanner };
        s.sync()?;
        Ok(s)
    }

    pub fn scanner(&self) -> &Scanner {
        &self.scanner
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Every note in both vaults, as of the last sync.
    pub(crate) fn notes(&self) -> &[crate::catalog::Meta] {
        self.index.notes()
    }

    /// How many notes in one vault have this status (as of the last sync).
    pub fn count_by_status(&self, status: &str, scope: Scope) -> usize {
        self.index.count_by_status(status, scope.as_str())
    }

    fn roots(&self) -> (PathBuf, PathBuf) {
        (self.cfg.project_vault.clone(), self.cfg.global_vault.clone())
    }

    pub fn sync(&mut self) -> Result<usize> {
        let (p, g) = self.roots();
        self.index.sync(&[
            VaultRoot { scope: "project", root: &p },
            VaultRoot { scope: "global", root: &g },
        ])
    }

    pub fn vault_for(&self, scope: Scope) -> &Path {
        match scope {
            Scope::Project => &self.cfg.project_vault,
            Scope::Global => &self.cfg.global_vault,
        }
    }

    /// Search with Obsidian's own search (same syntax and results as in the app). Returns the
    /// hits plus notes about vaults that could not be searched.
    pub fn search(
        &mut self,
        q: &str,
        ty: Option<NoteType>,
        scope: Option<Scope>,
        limit: usize,
    ) -> Result<(Vec<Hit>, Vec<String>)> {
        self.sync()?;
        if q.trim().is_empty() {
            return Ok((vec![], vec![]));
        }
        // Filters are Obsidian search operators too. Group the user's query so a top-level OR
        // cannot escape them (`a OR b -x` is `a OR (b -x)` in Obsidian).
        let mut query = format!("({})", q.trim());
        if let Some(t) = ty {
            query.push_str(&format!(" [type:{}]", t.as_str()));
        }
        query.push_str(" -[status:archived] -[status:superseded] -path:Templates -path:_system");

        let scopes = match scope {
            Some(s) => vec![s],
            None => vec![Scope::Project, Scope::Global],
        };
        let (mut hits, mut problems, mut tried) = (Vec::new(), Vec::new(), 0);
        for sc in scopes {
            let root = self.vault_for(sc).to_path_buf();
            if sc == Scope::Project && !self.project_ready() {
                problems.push(format!("project vault not searched: {}", self.not_in_project()));
                continue;
            }
            if !root.exists() {
                continue;
            }
            tried += 1;
            match self.obs.search(&root, &query, limit) {
                Ok(found) => {
                    for r in found {
                        let meta = self.index.by_rel(sc.as_str(), &r.file);
                        let id = meta.map(|m| m.id.clone()).unwrap_or_else(|| {
                            Path::new(&r.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                        });
                        let snippet = snippet_for(&root.join(&r.file), &r.lines);
                        hits.push(Hit {
                            title: meta.map(|m| m.title.clone()).unwrap_or_else(|| id.clone()),
                            note_type: meta.map(|m| m.note_type.clone()).unwrap_or_default(),
                            status: meta.map(|m| m.status.clone()).unwrap_or_default(),
                            scope: sc.as_str().into(),
                            path: r.file,
                            snippet: snippet.chars().take(240).collect(),
                            id,
                        });
                    }
                }
                Err(e) => problems.push(format!("{} vault not searched: {e:#}", sc.as_str())),
            }
        }
        if hits.is_empty() && tried > 0 && problems.len() == tried {
            bail!("{}", problems.join("\n"));
        }
        hits.truncate(limit);
        Ok((hits, problems))
    }

    /// Full note text plus backlinks from Obsidian (`Err` text if Obsidian could not be asked;
    /// reading the note itself never depends on Obsidian).
    #[allow(clippy::type_complexity)]
    pub fn get(&mut self, key: &str) -> Result<(String, String, std::result::Result<Vec<String>, String>)> {
        self.sync()?;
        let matches = self.index.resolve(key)?;
        let (path, scope) = match matches.len() {
            0 => bail!("no note matches '{key}'"),
            1 => matches.into_iter().next().unwrap(),
            _ => {
                // prefer the project vault, then exact id match
                let mut m = matches;
                m.sort_by_key(|(_, s)| if s == "project" { 0 } else { 1 });
                m.into_iter().next().unwrap()
            }
        };
        let content = std::fs::read_to_string(&path)?;
        let rel = self.relative(&path);
        let root = if scope == "project" { self.cfg.project_vault.clone() } else { self.cfg.global_vault.clone() };
        let backlinks = self
            .obs
            .backlinks(&root, &rel)
            .map(|files| {
                files
                    .iter()
                    .map(|f| Path::new(f).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone()))
                    .collect()
            })
            .map_err(|e| format!("{e:#}"));
        Ok((format!("{scope}:{rel}"), content, backlinks))
    }

    /// Resolve a key that a write will act on: it must match exactly one note.
    fn resolve_one(&self, key: &str) -> Result<(String, String)> {
        let mut m = self.index.resolve(key)?;
        match m.len() {
            0 => bail!("no note matches '{key}'"),
            1 => Ok(m.remove(0)),
            _ => bail!(
                "'{key}' is ambiguous; use the vault-relative path instead:\n{}",
                m.iter().map(|(p, s)| format!("  - {s}:{}", self.relative(p))).collect::<Vec<_>>().join("\n")
            ),
        }
    }

    fn relative(&self, abs: &str) -> String {
        let (p, g) = self.roots();
        let ap = Path::new(abs);
        ap.strip_prefix(&p)
            .or_else(|_| ap.strip_prefix(&g))
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| abs.to_string())
    }

    /// Run the secret scanner on text that is about to be written. Returns the text to write.
    fn police(&self, text: &str) -> Result<(String, Vec<SecretHit>)> {
        let hits = self.scanner.scan(text);
        if hits.is_empty() {
            return Ok((text.to_string(), hits));
        }
        match self.cfg.secrets.mode {
            SecretMode::Redact => Ok((Scanner::redact(text, &hits), hits)),
            SecretMode::Block => {
                let lines: Vec<String> = hits
                    .iter()
                    .map(|h| format!("  - line {}: {} ({}) preview={} fingerprint={}", h.line, h.rule, h.description, h.preview, h.fingerprint))
                    .collect();
                Err(anyhow!(
                    "WRITE BLOCKED: possible secrets detected. Remove them or replace with a reference \
                     such as `$ENV_VAR_NAME` / `see 1Password: <item>`, then retry.\n{}\n\
                     If this is a false positive, a human can add the fingerprint to _system/secret-allowlist.toml.",
                    lines.join("\n")
                ))
            }
        }
    }

    /// Whether the resolved project opted in to obmem (has `.obmem.toml`). If not, the server
    /// was started outside any obmem project (e.g. an MCP client that launches servers in the
    /// home directory), and the "project vault" path is a guess that must not be written to.
    pub fn project_ready(&self) -> bool {
        self.cfg.project_root.join(crate::config::PROJECT_CONFIG_FILE).exists()
    }

    pub fn not_in_project(&self) -> String {
        format!(
            "no obmem project here: the server resolved {} as the project and it has no {}. \
             If you are an agent, retry the call with `project` set to the absolute path of your \
             workspace root. Otherwise start `obmem serve` in the project, pass \
             `obmem serve --project <dir>`, or set OBMEM_PROJECT_ROOT. Personal (global) notes still work.",
            self.cfg.project_root.display(),
            crate::config::PROJECT_CONFIG_FILE
        )
    }

    pub fn write(&mut self, req: WriteRequest) -> Result<WriteOutcome> {
        self.sync()?;
        let scope = req.scope.unwrap_or_else(|| req.note_type.default_scope());
        if scope == Scope::Project && !self.project_ready() {
            bail!("{}", self.not_in_project());
        }
        let (body, mut redactions) = self.police(&req.body)?;
        let (title, r2) = self.police(&req.title)?;
        redactions.extend(r2);
        // Everything that is persisted goes through the scanner, not just title and body.
        let police_all = |items: Vec<String>, redactions: &mut Vec<SecretHit>| -> Result<Vec<String>> {
            items
                .into_iter()
                .map(|t| {
                    let (t, r) = self.police(&t)?;
                    redactions.extend(r);
                    Ok(t)
                })
                .collect()
        };
        let tags = police_all(req.tags, &mut redactions)?;
        let code_refs = police_all(req.code_refs, &mut redactions)?;
        let confidence = match req.confidence {
            Some(c) => Some(police_all(vec![c], &mut redactions)?.remove(0)),
            None => None,
        };
        if let Some(bad) = code_refs.iter().find(|r| crate::drift::CodeRef::parse(r).is_none()) {
            bail!("code_ref `{bad}` must be a path relative to the project root, e.g. `src/auth/refresh.rs#rotate_token`.");
        }
        let warnings = crate::drift::present_warnings(&self.cfg.project_root, &code_refs);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let commit = git_head(&self.cfg.project_root);

        if let Some(id) = &req.update_id {
            let (path, upd_scope) = self.resolve_one(id)?;
            let upd_root = if upd_scope == "project" { self.cfg.project_vault.clone() } else { self.cfg.global_vault.clone() };
            let src = std::fs::read_to_string(&path)?;
            let mut note = Note::parse(&src)?;
            if !note.fm.is_agent_authored() {
                bail!(
                    "'{id}' is human-authored and read-only for agents. Record your proposed change \
                     as a `finding` note that links to [[{id}]] instead."
                );
            }
            if note.fm.note_type.as_deref() == Some("finding") {
                bail!(
                    "'{id}' is a finding; its status is managed with memory_resolve (and by obmem audit), \
                     not memory_write. Add context with memory_resolve's comment."
                );
            }
            match note.fm.status.as_deref() {
                Some(st @ ("superseded" | "archived")) => bail!(
                    "'{id}' is {st}; write a new note (and link it) instead of editing a retired one."
                ),
                // A human promoted this note; an agent edit needs a fresh review.
                Some("draft") | None => {}
                Some(_) => note.fm.status = Some("draft".into()),
            }
            note.body = ensure_trailing_newline(body);
            note.fm.title = Some(title);
            note.fm.updated = Some(today);
            if !tags.is_empty() {
                note.fm.tags = tags;
            }
            if !code_refs.is_empty() {
                note.fm.code_refs = code_refs;
                note.fm.verified_commit = commit;
            }
            if confidence.is_some() {
                note.fm.confidence = confidence;
            }
            let path = PathBuf::from(path);
            atomic_write(&upd_root, &path, &note.render()?)?;
            self.sync()?;
            let id = path.file_stem().unwrap().to_string_lossy().into_owned();
            return Ok(WriteOutcome::Written { id, path, created: false, redactions, warnings });
        }

        // Duplicate check: same type + scope, similar title.
        if !req.force_new {
            let mut candidates: Vec<(String, String, f64)> = self
                .index
                .titles(req.note_type.as_str(), scope.as_str())?
                .into_iter()
                .map(|(id, t, _)| {
                    let s = title_similarity(&t, &title);
                    (id, t, s)
                })
                .filter(|(_, _, s)| *s >= 0.5)
                .collect();
            if !candidates.is_empty() {
                candidates.sort_by(|a, b| b.2.total_cmp(&a.2));
                candidates.truncate(5);
                return Ok(WriteOutcome::Duplicate { candidates });
            }
        }

        let root = self.vault_for(scope).to_path_buf();
        let dir = root.join(req.note_type.folder());
        crate::config::ensure_within(&root, &dir)?;
        let base = slugify(req.file_stem.as_deref().unwrap_or(&title));
        let mut path = dir.join(format!("{base}.md"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{base}-{n}.md"));
            n += 1;
        }
        let fm = Frontmatter {
            note_type: Some(req.note_type.as_str().into()),
            title: Some(title.clone()),
            // Findings follow their own lifecycle (open → accepted / fixed / rejected).
            status: Some(if req.note_type == NoteType::Finding { "open" } else { "draft" }.into()),
            author: Some(req.author),
            confidence: Some(confidence.unwrap_or_else(|| "medium".into())),
            created: Some(today.clone()),
            updated: Some(today),
            verified_commit: if code_refs.is_empty() { None } else { commit },
            code_refs,
            tags,
            extra: Default::default(),
        };
        let note = Note { fm, body: ensure_trailing_newline(format!("# {title}\n\n{body}")) };
        atomic_write(&root, &path, &note.render()?)?;
        self.sync()?;
        let id = path.file_stem().unwrap().to_string_lossy().into_owned();
        Ok(WriteOutcome::Written { id, path, created: true, redactions, warnings })
    }

    /// Append a timestamped entry to today's session log (global vault, per project).
    pub fn log(&mut self, session: Option<&str>, entry: &str, author: &str) -> Result<(PathBuf, Vec<SecretHit>)> {
        let (entry, redactions) = self.police(entry)?;
        let now = chrono::Local::now();
        let date = now.format("%Y-%m-%d").to_string();
        let dir = self.cfg.global_project_dir().join("Sessions");
        crate::config::ensure_within(&self.cfg.global_vault, &dir)?;
        let name = match session {
            Some(s) if !s.is_empty() => format!("{date}-{}", slugify(&s.chars().take(12).collect::<String>())),
            _ => date.clone(),
        };
        let path = dir.join(format!("{name}.md"));
        let mut content = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            let fm = Frontmatter {
                note_type: Some("session".into()),
                title: Some(format!("{} session {name}", self.cfg.project_name)),
                status: Some("active".into()),
                author: Some(author.to_string()),
                created: Some(date.clone()),
                tags: vec![format!("project/{}", slugify(&self.cfg.project_name))],
                ..Default::default()
            };
            Note { fm, body: format!("# Session {name}\n\nProject: {} (`{}`)\n\n", self.cfg.project_name, self.cfg.project_root.display()) }.render()?
        };
        if !content.ends_with('\n') {
            content.push('\n');
        }
        let indented = entry.trim().replace('\n', "\n  ");
        content.push_str(&format!("- {} {}\n", now.format("%H:%M"), indented));
        atomic_write(&self.cfg.global_vault, &path, &content)?;
        Ok((path, redactions))
    }

    /// Run the rule-based audit. With `write`, findings at or above that severity are written
    /// as notes (and stale ones closed); the index is refreshed afterwards.
    pub fn audit(&mut self, opts: &AuditOptions, write: Option<Severity>) -> Result<(Audit, Option<WriteStats>)> {
        let mut report = audit::run(&self.cfg, opts, self.obs.as_ref())?;
        if !self.project_ready() && opts.scope != Some(Scope::Global) {
            report.notices.push(format!("project vault not audited: {}", self.not_in_project()));
        }
        let stats = match write {
            Some(min) => {
                let st = report.write_findings(min, opts.today)?;
                self.sync()?;
                Some(st)
            }
            None => None,
        };
        Ok((report, stats))
    }

    /// Set a finding's status and append a history line. `human` unlocks rejecting and
    /// resolving human-authored findings (CLI); agents (MCP) get neither.
    pub fn resolve_finding(&mut self, key: &str, verdict: Verdict, comment: Option<&str>, by: &str, human: bool) -> Result<String> {
        self.sync()?;
        if verdict == Verdict::Rejected && !human {
            bail!(
                "Only a human can reject a finding (it silences that check permanently). If you think it is a \
                 false positive, leave it open and tell the user why, so they can reject it in Obsidian or with \
                 `obmem resolve <id> rejected`."
            );
        }
        let (path, scope) = self.resolve_one(key)?;
        let mut note = Note::parse(&std::fs::read_to_string(&path)?)?;
        if note.fm.note_type.as_deref() != Some("finding") {
            bail!("'{key}' is not a finding (type: {})", note.fm.note_type.as_deref().unwrap_or("-"));
        }
        if !human && !note.fm.is_agent_authored() {
            bail!("'{key}' is a human-authored finding; ask the user to update it.");
        }
        let comment = match comment {
            Some(c) if !c.trim().is_empty() => Some(self.police(c.trim())?.0),
            _ => None,
        };
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let prev = note.fm.status.clone().unwrap_or_else(|| "-".into());
        note.fm.status = Some(verdict.as_str().into());
        note.fm.updated = Some(date.clone());
        let mut line = format!("{date}: {prev} → {} by {by}", verdict.as_str());
        if let Some(c) = comment {
            line.push_str(&format!(": {}", c.replace('\n', " ")));
        }
        audit::append_history(&mut note.body, &line);
        let path = PathBuf::from(path);
        let root = if scope == "project" { self.cfg.project_vault.clone() } else { self.cfg.global_vault.clone() };
        atomic_write(&root, &path, &note.render()?)?;
        let mut msg = format!("[[{}]] is now {}", path.file_stem().unwrap().to_string_lossy(), verdict.as_str());
        if verdict == Verdict::Rejected {
            let rel = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().into_owned();
            if audit::suppress_finding(&root, &rel, &note.fm, &date)? {
                msg.push_str(&format!(" and suppressed in {}", root.join(audit::SUPPRESSIONS_FILE).display()));
            }
        }
        self.sync()?;
        Ok(msg)
    }

    /// Record that a note was checked against the current code: `verified_commit` = HEAD,
    /// `verified_at` = today. Clears code-drift and stale findings on the next audit.
    /// Agents may verify only agent-authored notes; the CLI (`human`) may verify any.
    pub fn verify(&mut self, key: &str, by: &str, human: bool) -> Result<String> {
        self.sync()?;
        let (path, scope) = self.resolve_one(key)?;
        let root = if scope == "project" { self.cfg.project_vault.clone() } else { self.cfg.global_vault.clone() };
        let mut note = Note::parse(&std::fs::read_to_string(&path)?)?;
        if note.fm.note_type.as_deref() == Some("finding") {
            bail!("'{key}' is a finding; use memory_resolve / `obmem resolve` instead.");
        }
        if !human && !note.fm.is_agent_authored() {
            bail!("'{key}' is human-authored; ask the user to check it and run `obmem verify {key}`.");
        }
        if let Some(st @ ("superseded" | "archived")) = note.fm.status.as_deref() {
            bail!("'{key}' is {st}; there is nothing to verify.");
        }
        let problems = crate::drift::present_warnings(&self.cfg.project_root, &note.fm.code_refs);
        if !problems.is_empty() {
            bail!(
                "Cannot verify '{key}' while its code_refs are broken. Fix them first (memory_write with \
                 update_id and new code_refs, or edit the note):\n  - {}",
                problems.join("\n  - ")
            );
        }
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let commit = if note.fm.code_refs.is_empty() { None } else { git_head(&self.cfg.project_root) };
        if commit.is_some() {
            note.fm.verified_commit = commit.clone();
        }
        let s = |x: &str| yaml_serde::Value::String(x.to_string());
        note.fm.extra.insert("verified_at".into(), s(&date));
        note.fm.extra.insert("verified_by".into(), s(by));
        let path = PathBuf::from(path);
        atomic_write(&root, &path, &note.render()?)?;
        self.sync()?;
        let id = path.file_stem().unwrap().to_string_lossy().into_owned();
        Ok(match commit {
            Some(c) => format!("[[{id}]] verified against {c} on {date}."),
            None => format!("[[{id}]] verified on {date} (no code_refs, so no commit recorded)."),
        })
    }

    /// A person accepts an agent's draft: `draft` → `active`. For people only (CLI, GUI); the MCP
    /// server does not offer it, because an agent must not promote its own notes.
    pub fn approve(&mut self, key: &str, by: &str) -> Result<String> {
        self.sync()?;
        let (path, scope) = self.resolve_one(key)?;
        let root = if scope == "project" { self.cfg.project_vault.clone() } else { self.cfg.global_vault.clone() };
        let mut note = Note::parse(&std::fs::read_to_string(&path)?)?;
        if note.fm.note_type.as_deref() == Some("finding") {
            bail!("'{key}' is a finding; use `obmem resolve` instead.");
        }
        match note.fm.status.as_deref() {
            Some("draft") => {}
            other => bail!("'{key}' is not a draft (status: {}).", other.unwrap_or("-")),
        }
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        note.fm.status = Some("active".into());
        note.fm.updated = Some(date.clone());
        let s = |x: &str| yaml_serde::Value::String(x.to_string());
        note.fm.extra.insert("approved_at".into(), s(&date));
        note.fm.extra.insert("approved_by".into(), s(by));
        let path = PathBuf::from(path);
        atomic_write(&root, &path, &note.render()?)?;
        self.sync()?;
        Ok(format!("[[{}]] is now active.", path.file_stem().unwrap().to_string_lossy()))
    }

    /// The last `n` entries of the most recently modified session log of this project.
    fn last_session_log(&self, n: usize) -> Option<(String, String)> {
        let dir = self.cfg.global_project_dir().join("Sessions");
        let latest = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?
            .path();
        let body = Note::parse(&std::fs::read_to_string(&latest).ok()?).ok()?.body;
        // an entry is a `- ` line plus its indented continuation lines
        let mut entries: Vec<String> = Vec::new();
        for line in body.lines() {
            if line.starts_with("- ") {
                entries.push(line.to_string());
            } else if line.starts_with("  ")
                && let Some(last) = entries.last_mut()
            {
                last.push('\n');
                last.push_str(line);
            }
        }
        if entries.is_empty() {
            return None;
        }
        let tail = entries[entries.len().saturating_sub(n)..].join("\n");
        Some((latest.file_stem()?.to_string_lossy().into_owned(), tail))
    }

    /// Compact context for the start of a session: the project Index plus review queue.
    pub fn session_context(&mut self, max_chars: usize) -> Result<String> {
        self.sync()?;
        let mut out = String::new();
        let idx = self.cfg.project_vault.join("Index.md");
        out.push_str(&format!(
            "# Project memory ({})\nProject vault: {}\nGlobal vault: {}\n\n{}\n\n",
            self.cfg.project_name,
            self.cfg.project_vault.display(),
            self.cfg.global_vault.display(),
            crate::guide::GUIDE
        ));
        if let Ok(s) = std::fs::read_to_string(&idx) {
            let body = Note::parse(&s).map(|n| n.body).unwrap_or(s);
            out.push_str(&truncate(&body, max_chars / 2));
            out.push('\n');
        }
        if let Some((name, tail)) = self.last_session_log(12) {
            out.push_str(&format!("\n## Where the last session left off ({name})\n{tail}\n"));
        }
        let prefs = self.index.global_guidance()?;
        if !prefs.is_empty() {
            out.push_str("\n## Global feedback & preferences\n");
            for (id, title) in prefs.iter().take(30) {
                out.push_str(&format!("- [[{id}]] {title}\n"));
            }
        }
        let open = self.index.list_by_status("open", 10)?;
        if !open.is_empty() {
            out.push_str("\n## Open findings (review or fix)\n");
            for (id, title, _) in open {
                out.push_str(&format!("- [[{id}]] {title}\n"));
            }
        }
        let drafts = self.index.list_by_status("draft", 10)?;
        if !drafts.is_empty() {
            out.push_str("\n## Recent agent drafts (unreviewed — lower trust)\n");
            for (id, title, ty) in drafts {
                out.push_str(&format!("- [[{id}]] ({ty}) {title}\n"));
            }
        }
        Ok(truncate(&out, max_chars))
    }
}

/// Matching body lines (frontmatter matches are noise for an agent); if the hit came only
/// from tags / properties, the first body line instead.
fn snippet_for(path: &Path, lines: &[(usize, String)]) -> String {
    let src = std::fs::read_to_string(path).unwrap_or_default();
    let body = Note::parse(&src).map(|n| n.body).unwrap_or_else(|_| src.clone());
    let fm_lines = src[..src.len().saturating_sub(body.len())].lines().count();
    let mut picked: Vec<&str> = lines.iter().filter(|(n, _)| *n > fm_lines).map(|(_, t)| t.as_str()).take(2).collect();
    if picked.is_empty() {
        picked.extend(body.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')));
    }
    picked.join(" … ").chars().take(240).collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push_str("\n…(truncated)");
    t
}

fn ensure_trailing_newline(mut s: String) -> String {
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Write via temp file + rename so Obsidian / sync clients never see a half-written note.
/// `root` is the vault the file must stay in, even through symlinked folders.
pub fn atomic_write(root: &Path, path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow!("no parent dir"))?;
    crate::config::ensure_within(root, dir)?;
    std::fs::create_dir_all(dir)?;
    crate::config::ensure_within(root, dir)?;
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("refusing to write {}: it is a symlink", path.display());
    }
    // Random name, created exclusively: with a predictable name, a file or symlink planted in
    // the vault (e.g. by a cloned repo) could redirect the write. `create_new` fails on any
    // existing entry, dangling symlinks included.
    let base = path.file_name().unwrap().to_string_lossy();
    let (tmp, mut file) = {
        let mut attempt = 0u32;
        loop {
            let tmp = dir.join(format!(".{base}.{:016x}.obmem-tmp", temp_nonce(attempt)));
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
                Ok(f) => break (tmp, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 8 => attempt += 1,
                Err(e) => return Err(anyhow!("creating temp file in {}: {e}", dir.display())),
            }
        }
    };
    let written = std::io::Write::write_all(&mut file, content.as_bytes()).and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = written.and_then(|_| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!("writing {}: {e}", path.display()));
    }
    Ok(())
}

/// Unpredictable per process (RandomState is seeded randomly) and distinct per call.
fn temp_nonce(attempt: u32) -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    h.write_u32(attempt);
    h.write_u32(std::process::id());
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.finish()
}

fn git_head(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(scope: Scope) -> WriteRequest {
        WriteRequest {
            note_type: NoteType::Fact,
            title: "A fact".into(),
            body: "body".into(),
            scope: Some(scope),
            tags: vec![],
            code_refs: vec![],
            confidence: None,
            update_id: None,
            force_new: false,
            author: "agent:test".into(),
            file_stem: None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_uses_fresh_temp_files_and_refuses_symlink_targets() {
        let dir = std::env::temp_dir().join(format!("obmem-aw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("v")).unwrap();
        let v = dir.join("v");
        // the old predictable temp name, planted as a symlink, is no longer used at all
        std::os::unix::fs::symlink(dir.join("stolen"), v.join(".n.md.obmem-tmp")).unwrap();
        for i in 0..3 {
            atomic_write(&v, &v.join("n.md"), &format!("v{i}")).unwrap();
        }
        assert_eq!(std::fs::read_to_string(v.join("n.md")).unwrap(), "v2");
        assert!(!dir.join("stolen").exists());
        let leftovers: Vec<_> = std::fs::read_dir(&v)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".obmem-tmp") && e.file_name() != ".n.md.obmem-tmp")
            .collect();
        assert!(leftovers.is_empty(), "no temp files left behind");
        // the target itself being a symlink is refused
        std::os::unix::fs::symlink(dir.join("elsewhere.md"), v.join("link.md")).unwrap();
        assert!(atomic_write(&v, &v.join("link.md"), "x").is_err());
        assert!(!dir.join("elsewhere.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_project_writes_outside_an_obmem_project() {
        // e.g. an MCP client that starts the server in the home directory
        let dir = std::env::temp_dir().join(format!("obmem-noproj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            project_root: dir.clone(),
            project_name: "home".into(),
            project_vault: dir.join("docs/kb"),
            global_vault: dir.join("global"),
            secrets: Default::default(),
            audit: Default::default(),
        };
        let mut store = Store::open(cfg).unwrap();
        assert!(!store.project_ready());
        let err = store.write(req(Scope::Project)).err().expect("refused").to_string();
        assert!(err.contains(".obmem.toml"), "{err}");
        assert!(!dir.join("docs/kb").exists(), "no stray vault created");
        assert!(matches!(store.write(req(Scope::Global)), Ok(WriteOutcome::Written { .. })));

        let (hits, problems) = store.search("fact", None, Some(Scope::Project), 5).unwrap();
        assert!(hits.is_empty() && problems[0].contains("no obmem project here"), "{problems:?}");

        // opting in makes it a project
        std::fs::write(dir.join(".obmem.toml"), "").unwrap();
        assert!(store.project_ready());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_person_approves_drafts_only() {
        let dir = std::env::temp_dir().join(format!("obmem-approve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".obmem.toml"), "").unwrap();
        let cfg = Config {
            project_root: dir.clone(),
            project_name: "p".into(),
            project_vault: dir.join("docs/kb"),
            global_vault: dir.join("global"),
            secrets: Default::default(),
            audit: Default::default(),
        };
        let mut store = Store::open(cfg).unwrap();
        let Ok(WriteOutcome::Written { id, path, .. }) = store.write(req(Scope::Project)) else { panic!("write") };
        assert_eq!(store.count_by_status("draft", Scope::Project), 1);

        store.approve(&id, "human:test").unwrap();
        let fm = Note::parse(&std::fs::read_to_string(&path).unwrap()).unwrap().fm;
        assert_eq!(fm.status.as_deref(), Some("active"));
        assert!(fm.extra.contains_key("approved_at") && fm.extra.contains_key("approved_by"));
        assert_eq!(store.count_by_status("draft", Scope::Project), 0);
        assert_eq!(store.count_by_status("active", Scope::Project), 1);

        let err = store.approve(&id, "human:test").unwrap_err().to_string();
        assert!(err.contains("not a draft"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
