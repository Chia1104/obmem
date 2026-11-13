//! MCP server (stdio) — agent-agnostic: Claude Code, Codex, Cursor, … all speak MCP.

use obmem_core::audit::{AuditOptions, Severity};
use obmem_core::note::{NoteType, Scope};
use obmem_core::store::{Store, Verdict, WriteOutcome, WriteRequest};
use rmcp::{
    ErrorData as McpError, Peer, RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MemoryServer {
    store: Arc<Mutex<Store>>,
    author: String,
    /// Project roots were already looked at (or the project was given explicitly).
    roots_checked: Arc<AtomicBool>,
    /// The project was given explicitly (`--project` / OBMEM_PROJECT_ROOT): never switch it.
    pinned: bool,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// Obsidian search syntax (any language): words are ANDed; also `OR`, "exact phrase",
    /// -exclude, tag:auth, path:Decisions, [status:draft], [property:value], line:(a b).
    pub query: String,
    /// Restrict to one note type.
    #[serde(rename = "type")]
    pub note_type: Option<NoteType>,
    /// Restrict to the project vault or the global (personal) vault.
    pub scope: Option<Scope>,
    /// Max results (default 8).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ContextArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// What you are about to do, in your own words or the user's (any language).
    pub task: String,
    /// Max notes (default 8).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// Note id (filename stem, as used in [[wikilinks]]), title, or vault-relative path.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WriteArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    #[serde(rename = "type")]
    pub note_type: NoteType,
    /// Short, specific title. It becomes the filename / link target.
    pub title: String,
    /// Markdown body. Link related notes with [[note-id]]. For decisions include context,
    /// the decision, alternatives considered and consequences.
    pub body: String,
    /// Defaults: feedback/preference -> global, everything else -> project.
    pub scope: Option<Scope>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Code anchors this note describes, e.g. `src/auth/refresh.rs#rotate_token`.
    /// Used later to detect when the code changed and the note may be stale.
    #[serde(default)]
    pub code_refs: Vec<String>,
    /// high | medium | low
    pub confidence: Option<String>,
    /// Id of an existing agent-authored note to replace instead of creating a new one.
    pub update_id: Option<String>,
    /// Create a new note even though similar titles exist.
    #[serde(default)]
    pub force_new: bool,
    /// For feedback and preferences: true when it holds in this project only (a convention of
    /// this repo, how its branches or reviews work). It is then the user's guidance at session
    /// start in this project and in no other. False for what the user wants everywhere, such as
    /// the language of replies. When you cannot tell, pass true.
    #[serde(default)]
    pub this_project_only: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LogArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// What happened / was learned / is left to do. One or a few lines.
    pub entry: String,
    /// Optional session id so parallel sessions get separate logs.
    pub session: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuditArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// Only audit one vault (default: both).
    pub scope: Option<Scope>,
    /// Only run these rules: broken-link, orphan, schema, stale, supersede, duplicate, code-drift (default: all).
    #[serde(default)]
    pub rules: Vec<String>,
    /// Lowest severity to report (default warn; info adds orphans and cross-vault links).
    pub min_severity: Option<Severity>,
    /// Also write finding notes for humans to review and close findings that are fixed.
    #[serde(default)]
    pub write_findings: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VerifyArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// Note id (filename stem) you re-checked against the current code.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResolveArgs {
    /// Absolute path of your workspace root. Only needed when obmem reports "no obmem project
    /// here" (some MCP hosts start the server outside the project).
    #[serde(default)]
    pub project: Option<String>,
    /// Finding note id (filename stem).
    pub id: String,
    /// accepted (real, will fix) | fixed (you fixed it) | open (reopen). Rejecting is human-only.
    pub verdict: Verdict,
    /// What you did or why, appended to the finding's history.
    pub comment: Option<String>,
}

fn ok(text: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn fail(e: impl std::fmt::Display) -> Result<CallToolResult, McpError> {
    // Tool-level error (isError=true) so the agent sees the message and can react.
    Ok(CallToolResult::error(vec![ContentBlock::text(
        e.to_string(),
    )]))
}

#[tool_router]
impl MemoryServer {
    pub fn new(store: Store, pinned: bool) -> Self {
        let author = std::env::var("OBMEM_AGENT").unwrap_or_else(|_| "agent".into());
        let author = if author.starts_with("agent") {
            author
        } else {
            format!("agent:{author}")
        };
        Self {
            store: Arc::new(Mutex::new(store)),
            author,
            roots_checked: Arc::new(AtomicBool::new(pinned)),
            pinned,
            tool_router: Self::tool_router(),
        }
    }

    /// The caller named its workspace (`project` argument). Switch to it if the current project
    /// is not an obmem project and the named one is; keep using it for the rest of the session.
    /// Only ever *binds* an unbound server: a server that already has a project (from its cwd,
    /// `--project`, roots, or an earlier `project` argument) is never switched to another one, so
    /// a misled agent cannot redirect writes into some other project's vault.
    fn use_project(&self, project: Option<&str>) -> Result<(), String> {
        let Some(dir) = project.map(str::trim).filter(|p| !p.is_empty()) else {
            return Ok(());
        };
        let dir = std::path::Path::new(dir);
        if !dir.is_absolute() {
            return Err(format!(
                "`project` must be an absolute path, got {}",
                dir.display()
            ));
        }
        let real =
            std::fs::canonicalize(dir).map_err(|e| format!("`project` {}: {e}", dir.display()))?;
        {
            let current = self.store.lock().unwrap();
            if current.project_ready() {
                let root = std::fs::canonicalize(&current.cfg.project_root)
                    .unwrap_or(current.cfg.project_root.clone());
                return if real.starts_with(&root) {
                    Ok(())
                } else {
                    Err(format!(
                        "this obmem server is bound to {}; `project` cannot switch it to {}.",
                        root.display(),
                        real.display()
                    ))
                };
            }
            if self.pinned {
                return Err(format!(
                    "this obmem server was started with an explicit project ({}); `project` cannot change it.",
                    current.cfg.project_root.display()
                ));
            }
        }
        let dir = real.as_path();
        let cfg = obmem_core::config::Config::load(Some(dir)).map_err(|e| format!("{e:#}"))?;
        if !cfg
            .project_root
            .join(obmem_core::config::PROJECT_CONFIG_FILE)
            .exists()
        {
            return Err(format!(
                "{} is not an obmem project (no {} in {} or above it).",
                dir.display(),
                obmem_core::config::PROJECT_CONFIG_FILE,
                cfg.project_root.display()
            ));
        }
        let store = Store::open(cfg).map_err(|e| format!("{e:#}"))?;
        tracing::info!(
            "obmem: using project from the `project` argument: {}",
            store.cfg.project_root.display()
        );
        *self.store.lock().unwrap() = store;
        Ok(())
    }

    /// MCP clients may launch servers outside the project (T3 Code's Cursor driver uses the home
    /// directory). If the cwd-derived project has not opted in, ask the client for its roots once
    /// and switch to the first one that has `.obmem.toml`.
    // Roots are deprecated by SEP-2577 but still the only way a client can say where its
    // workspace is; this is a fallback for clients that ignore the cwd convention.
    #[allow(deprecated)]
    async fn ensure_project(&self, peer: &Peer<RoleServer>) {
        if self.roots_checked.swap(true, Ordering::SeqCst)
            || self.store.lock().unwrap().project_ready()
        {
            return;
        }
        let supports_roots = peer
            .peer_info()
            .is_some_and(|i| i.capabilities.roots.is_some());
        if !supports_roots {
            return;
        }
        let Ok(roots) = peer.list_roots().await else {
            return;
        };
        for root in roots.roots {
            let Some(dir) = file_uri_to_path(&root.uri) else {
                continue;
            };
            let Ok(cfg) = obmem_core::config::Config::load(Some(&dir)) else {
                continue;
            };
            if !cfg
                .project_root
                .join(obmem_core::config::PROJECT_CONFIG_FILE)
                .exists()
            {
                continue;
            }
            match Store::open(cfg) {
                Ok(store) => {
                    tracing::info!("obmem: using project from client roots: {}", dir.display());
                    *self.store.lock().unwrap() = store;
                    return;
                }
                Err(e) => tracing::warn!("obmem: root {} unusable: {e:#}", dir.display()),
            }
        }
    }

    #[tool(
        description = "Search project + personal memory with Obsidian's search syntax (Obsidian's \
        own search, the same results as its search box, where Obsidian is installed). Call this before re-deriving facts, making a decision, \
        or asking the user something they may have told you before. Archived/superseded notes and \
        templates are excluded. Results are not relevance-ranked: prefer specific words and operators. \
        Returns ids, types and matching lines; use memory_get for full text."
    )]
    async fn memory_search(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        match s.search(&a.query, a.note_type, a.scope, a.limit.unwrap_or(8).min(30)) {
            Ok((hits, problems)) if hits.is_empty() => {
                let mut msg = "No matching notes.".to_string();
                for p in problems {
                    msg.push_str(&format!("\nNote: {p}"));
                }
                ok(msg)
            }
            Ok((hits, problems)) => {
                let mut lines: Vec<String> = hits
                    .iter()
                    .map(|h| {
                        format!(
                            "- [[{}]] ({}/{}, {}) {}\n  {}",
                            h.id,
                            h.scope,
                            h.note_type,
                            if h.status.is_empty() { "-" } else { &h.status },
                            h.title,
                            h.snippet.replace('\n', " ")
                        )
                    })
                    .collect();
                lines.extend(problems.into_iter().map(|p| format!("Note: {p}")));
                ok(lines.join("\n"))
            }
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Get the memory most relevant to a task before starting it: pass the task \
        description (any language) and receive the best-matching notes, ranked by the task's terms they \
        contain (a rare term counts most), with the terms each one matched, the notes one link away from the best \
        ones, and open findings about them. Matching is literal: the answer names the words no note \
        contains, so that you can search again with other words or in the notes' language. Use at \
        the start of a non-trivial task; use memory_search for precise lookups."
    )]
    async fn memory_context(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<ContextArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        match obmem_core::context::context(&mut s, &a.task, a.limit.unwrap_or(8).min(20), 5000) {
            Ok(text) => ok(text),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Read one memory note in full. The first line lists the notes it links to and \
        the notes that link to it: read those with memory_get to follow a topic."
    )]
    async fn memory_get(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<GetArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        match s.get(&a.id) {
            Ok(got) => ok(got.to_string()),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Record durable knowledge as an Obsidian note: decisions (with rationale), \
        facts, architecture, procedures, user feedback/preferences, or findings (problems you noticed in \
        code or docs). New notes start as status=draft for human review, or active if the user turned on \
        auto-approve. Refuses near-duplicates (update \
        the existing note via update_id instead), edits to human-authored notes (write a finding instead), \
        and content containing secrets. Feedback and preferences are the user's guidance in every \
        project unless this_project_only is true. Do not record things derivable from the code or git \
        history."
    )]
    async fn memory_write(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<WriteArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        let req = WriteRequest {
            note_type: a.note_type,
            title: a.title,
            body: a.body,
            scope: a.scope,
            tags: a.tags,
            code_refs: a.code_refs,
            confidence: a.confidence,
            update_id: a.update_id,
            force_new: a.force_new,
            author: self.author.clone(),
            file_stem: None,
            needs_review: false,
            this_project_only: a.this_project_only,
        };
        match s.write(req) {
            Ok(WriteOutcome::Written {
                id,
                path,
                created,
                status,
                redactions,
                warnings,
            }) => {
                let mut msg = format!(
                    "{} [[{id}]] ({status}) at {}",
                    if created { "Created" } else { "Updated" },
                    path.display()
                );
                if !redactions.is_empty() {
                    msg.push_str(&format!(
                        "\nNote: {} possible secret(s) were redacted.",
                        redactions.len()
                    ));
                }
                if !warnings.is_empty() {
                    msg.push_str("\nWarning, these code_refs do not resolve right now (fix them or the audit will flag the note):");
                    for w in warnings {
                        msg.push_str(&format!("\n  - {w}"));
                    }
                }
                ok(msg)
            }
            Ok(WriteOutcome::Duplicate { candidates }) => {
                let list: Vec<String> = candidates
                    .iter()
                    .map(|(id, t, sim)| format!("- [[{id}]] {t} (similarity {sim:.2})"))
                    .collect();
                fail(format!(
                    "Similar notes already exist. Read them with memory_get, then either update one \
                     (update_id) or retry with force_new=true if this is genuinely different:\n{}",
                    list.join("\n")
                ))
            }
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Append a short entry to today's session log for this project (kept in the \
        personal vault, not the repo). Use for progress, what was tried, and open threads for next time."
    )]
    async fn memory_log(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<LogArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        match s.log(a.session.as_deref(), &a.entry, &self.author) {
            Ok((path, _)) => ok(format!("Logged to {}", path.display())),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Audit the memory vaults with deterministic rules: broken [[links]], orphan \
        notes, invalid frontmatter, stale notes, links to superseded decisions, near-duplicate titles, and \
        code drift (code_refs that vanished or changed since the note was verified). \
        Use it before relying heavily on memory, after big refactors, or when asked to clean up docs. \
        Each issue includes a suggested fix; fix agent-authored notes yourself, report the rest."
    )]
    async fn memory_audit(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<AuditArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        let min = a.min_severity.unwrap_or(Severity::Warn);
        let opts = AuditOptions {
            scope: a.scope,
            rules: a.rules,
            today: chrono::Local::now().date_naive(),
        };
        match s.audit(&opts, a.write_findings.then_some(min)) {
            Ok((report, stats)) => {
                let shown: Vec<_> = report.issues.iter().filter(|i| i.severity >= min).collect();
                ok(obmem_core::audit::report_text(
                    &report,
                    &shown,
                    stats.as_ref(),
                    min,
                    40,
                ))
            }
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Record that you re-checked an agent-authored note against the current code \
        and it is still accurate: sets verified_commit to HEAD and verified_at to today, which clears \
        code-drift and stale findings. Only call it after actually reading the referenced code; if the \
        note is wrong, update it with memory_write instead."
    )]
    async fn memory_verify(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<VerifyArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        match s.verify(&a.id, &self.author, false) {
            Ok(done) => ok(done.to_string()),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Update a finding note's status after you act on it: `fixed` when you fixed \
        the problem, `accepted` when it is real but not fixed yet, `open` to reopen. Add a short comment. \
        Only humans can reject findings as false positives."
    )]
    async fn memory_resolve(
        &self,
        peer: Peer<RoleServer>,
        Parameters(a): Parameters<ResolveArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_project(&peer).await;
        if let Err(e) = self.use_project(a.project.as_deref()) {
            return fail(e);
        }
        let mut s = self.store.lock().unwrap();
        match s.resolve_finding(&a.id, a.verdict, a.comment.as_deref(), &self.author, false) {
            Ok(done) => ok(done.to_string()),
            Err(e) => fail(e),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemoryServer {
    // Written out (instead of the macro's `instructions = "..."`) so the guide is shared with
    // the SessionStart context.
    fn get_info(&self) -> rmcp::model::ServerConfig {
        rmcp::model::ServerConfig::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new(
            "obmem",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(obmem_core::guide::GUIDE.to_string())
    }
}

pub async fn serve(store: Store, pinned: bool) -> anyhow::Result<()> {
    let service = MemoryServer::new(store, pinned)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

/// `file:///Users/me/My%20Repo` → `/Users/me/My Repo` (other schemes are not local projects).
fn file_uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(std::path::PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris() {
        assert_eq!(
            file_uri_to_path("file:///Users/me/My%20Repo").unwrap(),
            std::path::PathBuf::from("/Users/me/My Repo")
        );
        assert_eq!(
            file_uri_to_path("file://localhost/tmp/x").unwrap(),
            std::path::PathBuf::from("/tmp/x")
        );
        assert_eq!(
            file_uri_to_path("file:///tmp/%E8%BC%AA").unwrap(),
            std::path::PathBuf::from("/tmp/輪")
        );
        assert!(file_uri_to_path("https://example.com").is_none());
    }
}
