mod audit;
mod config;
mod context;
mod drift;
mod guide;
mod hooks;
mod import;
mod catalog;
mod init;
mod note;
mod obsidian;
mod secrets;
mod setup;
mod server;
mod store;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "obmem", version, about = "Obsidian-backed memory for coding agents (MCP server + CLI)")]
struct Cli {
    /// Project directory (defaults to cwd, walking up to .obmem.toml or .git).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create the project vault, global vault and config files (never overwrites).
    Init,
    /// Run the MCP server on stdio.
    Serve,
    /// Search memory from the shell.
    Search {
        query: Vec<String>,
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Print one note.
    Get { id: String },
    /// The notes most relevant to a task description.
    Context {
        task: Vec<String>,
        #[arg(short, long, default_value_t = 8)]
        limit: usize,
    },
    /// Scan files (or stdin, or staged git files) for secrets. Exit code 1 if any found.
    Scan {
        files: Vec<PathBuf>,
        /// Scan files staged in git that are inside the project vault (for a pre-commit hook;
        /// source code is out of scope, e.g. test fixtures with fake tokens).
        #[arg(long)]
        staged: bool,
    },
    /// Hook entry points for agent harnesses.
    Hook {
        /// Session id, for agents that call obmem without piping a JSON payload (Amp plugin);
        /// when given, stdin is not read.
        #[arg(long, global = true)]
        session: Option<String>,
        #[command(subcommand)]
        which: HookCmd,
    },
    /// Rule-based audit of both vaults: broken links, orphans, schema, stale notes,
    /// links to superseded notes, near-duplicates. Run nightly with --write.
    Audit {
        /// Only audit one vault.
        #[arg(long, value_enum)]
        scope: Option<note::Scope>,
        /// Only run these rules (repeatable): broken-link, orphan, schema, stale, supersede, duplicate, code-drift.
        #[arg(long = "rule")]
        rules: Vec<String>,
        /// Lowest severity to show and (with --write) to turn into finding notes.
        #[arg(long, value_enum, default_value_t = audit::Severity::Warn)]
        min_severity: audit::Severity,
        /// Write finding notes, close fixed ones, record rejected ones as suppressions.
        #[arg(long)]
        write: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// Exit with code 1 if any issue at or above this severity is found (for CI).
        #[arg(long, value_enum)]
        fail_on: Option<audit::Severity>,
    },
    /// Set a finding's status: accepted, fixed, rejected (suppresses it for good) or open.
    Resolve {
        id: String,
        #[arg(value_enum)]
        verdict: store::Verdict,
        /// Why (appended to the finding's history).
        #[arg(long)]
        comment: Option<String>,
    },
    /// Record that you re-checked a note against the current code (verified_commit = HEAD).
    Verify { id: String },
    /// Import Claude Code's file-based memory (~/.claude/projects/<project>/memory) as draft notes.
    ImportClaude {
        /// Memory folder (default: this project's Claude Code memory folder).
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Show what would be imported without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Register obmem (MCP server + lifecycle hooks) in an agent's project config. Merges into
    /// existing files and skips what is already there.
    Setup {
        #[arg(value_enum, required = true)]
        agents: Vec<hooks::Agent>,
    },
    /// Print the agent usage guide (paste into AGENTS.md / CLAUDE.md for agents without hooks).
    Guide,
    /// Show resolved paths and counts.
    Status,
}

#[derive(Subcommand)]
enum HookCmd {
    /// Session start (and after compaction): print the memory context.
    SessionStart {
        #[arg(long, default_value_t = 6000)]
        max_chars: usize,
        #[arg(long, value_enum, default_value_t = hooks::Agent::Claude)]
        agent: hooks::Agent,
    },
    /// Agent stop: once per session, ask the agent to log unlogged work.
    Stop {
        #[arg(long, value_enum, default_value_t = hooks::Agent::Claude)]
        agent: hooks::Agent,
    },
    /// Before compaction: log the session mechanically.
    PreCompact {
        #[arg(long, value_enum, default_value_t = hooks::Agent::Claude)]
        agent: hooks::Agent,
    },
}

/// Hooks resolve the project from what the agent says (cwd / workspace_roots), do nothing in
/// projects that have not opted in with `.obmem.toml` (so user-level hooks are harmless), and
/// never fail the agent: errors go to stderr, stdout gets the agent's "no-op" answer.
fn run_hook(which: &HookCmd, project: Option<&std::path::Path>, session: Option<&str>) {
    let input = match session {
        Some(s) => hooks::HookInput { session_id: Some(s.to_string()), ..Default::default() },
        None => hooks::HookInput::from_stdin(),
    };
    let agent = match which {
        HookCmd::SessionStart { agent, .. } | HookCmd::Stop { agent } | HookCmd::PreCompact { agent } => *agent,
    };
    let dir = project.map(PathBuf::from).or_else(|| input.project_dir().map(PathBuf::from));
    let out = (|| -> Result<String> {
        let cfg = config::Config::load(dir.as_deref())?;
        if !cfg.project_root.join(config::PROJECT_CONFIG_FILE).exists() {
            return Ok(agent.empty().to_string());
        }
        let now = std::time::SystemTime::now();
        Ok(match which {
            HookCmd::SessionStart { max_chars, .. } => {
                let mut store = store::Store::open(cfg)?;
                hooks::session_start(&mut store, &input, agent, *max_chars, now)?
            }
            HookCmd::Stop { .. } => agent.stop_output(hooks::stop(&cfg, &input, agent, now)?.as_deref()),
            HookCmd::PreCompact { .. } => {
                let mut store = store::Store::open(cfg)?;
                hooks::pre_compact(&mut store, &input, agent)?
            }
        })
    })();
    match out {
        Ok(s) if s.is_empty() => {}
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("obmem: {e:#}");
            if !agent.empty().is_empty() {
                println!("{}", agent.empty());
            }
        }
    }
}

fn main() -> Result<()> {
    // Logs go to stderr: stdout is the MCP channel.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("OBMEM_LOG"))
        .init();
    let mut cli = Cli::parse();
    // `--project ${workspaceFolder}` from a client that does not interpolate variables: ignore it
    // and fall back to the cwd (and, for `serve`, the client's roots) instead of failing to start.
    if cli.project.as_ref().is_some_and(|p| p.to_string_lossy().contains("${")) {
        eprintln!("obmem: ignoring uninterpolated --project {}", cli.project.as_ref().unwrap().display());
        cli.project = None;
    }
    if let Cmd::Hook { which, session } = &cli.cmd {
        run_hook(which, cli.project.as_deref(), session.as_deref());
        return Ok(());
    }
    let cfg = config::Config::load(cli.project.as_deref())?;

    match cli.cmd {
        Cmd::Init => {
            let created = init::run(&cfg)?;
            println!("Project vault: {}\nGlobal vault:  {}", cfg.project_vault.display(), cfg.global_vault.display());
            if created.is_empty() {
                println!("Nothing to do, everything already exists.");
            } else {
                println!("Created:");
                for c in created {
                    println!("  {c}");
                }
            }
            let bin = std::env::current_exe()?.display().to_string();
            println!("\n{}", init::setup_snippets(&bin, &cfg));
        }
        Cmd::Serve => {
            // An explicit project is never replaced by the client's roots.
            let pinned = cli.project.is_some() || std::env::var_os("OBMEM_PROJECT_ROOT").is_some();
            let store = store::Store::open(cfg)?;
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(server::serve(store, pinned))?;
        }
        Cmd::Search { query, limit } => {
            let mut store = store::Store::open(cfg)?;
            let (hits, problems) = store.search(&query.join(" "), None, None, limit)?;
            for h in hits {
                println!("[[{}]] ({}/{}) {}\n    {}", h.id, h.scope, h.note_type, h.title, h.snippet.replace('\n', " "));
            }
            for p in problems {
                eprintln!("note: {p}");
            }
        }
        Cmd::Get { id } => {
            let mut store = store::Store::open(cfg)?;
            let (loc, content, bl) = store.get(&id)?;
            let bl = bl.map(|b| b.join(", ")).unwrap_or_else(|e| format!("unavailable ({e})"));
            println!("<!-- {loc} | backlinks: {bl} -->\n{content}");
        }
        Cmd::Context { task, limit } => {
            let mut store = store::Store::open(cfg)?;
            print!("{}", context::context(&mut store, &task.join(" "), limit, 6000)?);
        }
        Cmd::Scan { files, staged } => {
            let store = store::Store::open(cfg.clone())?;
            let mut inputs: Vec<(String, String)> = Vec::new();
            let mut paths = files;
            if staged {
                let out = std::process::Command::new("git")
                    .arg("-C").arg(&cfg.project_root)
                    .args(["diff", "--cached", "--name-only", "--diff-filter=ACM"])
                    .output()?;
                for l in String::from_utf8_lossy(&out.stdout).lines() {
                    let p = cfg.project_root.join(l);
                    if p.starts_with(&cfg.project_vault) {
                        paths.push(p);
                    }
                }
            }
            if paths.is_empty() && !staged {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                inputs.push(("<stdin>".into(), s));
            }
            for p in paths {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    inputs.push((p.display().to_string(), s));
                }
            }
            let mut found = 0;
            for (name, text) in inputs {
                for h in store.scanner().scan(&text) {
                    found += 1;
                    println!("{name}:{}: {} ({}) {} fingerprint={}", h.line, h.rule, h.description, h.preview, h.fingerprint);
                }
            }
            if found > 0 {
                std::process::exit(1);
            }
        }
        Cmd::Audit { scope, rules, min_severity, write, json, fail_on } => {
            let mut store = store::Store::open(cfg)?;
            let opts = audit::AuditOptions { scope, rules, today: chrono::Local::now().date_naive() };
            let (report, stats) = store.audit(&opts, write.then_some(min_severity))?;
            let shown: Vec<&audit::Issue> = report.issues.iter().filter(|i| i.severity >= min_severity).collect();
            if json {
                let v = serde_json::json!({
                    "checked_notes": report.checked_notes,
                    "suppressed": report.suppressed,
                    "issues": shown,
                    "findings": stats,
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print!("{}", audit_text(&report, &shown, stats.as_ref(), min_severity, usize::MAX));
            }
            if let Some(level) = fail_on
                && report.issues.iter().any(|i| i.severity >= level)
            {
                std::process::exit(1);
            }
        }
        Cmd::Resolve { id, verdict, comment } => {
            let mut store = store::Store::open(cfg)?;
            println!("{}", store.resolve_finding(&id, verdict, comment.as_deref(), "human (cli)", true)?);
        }
        Cmd::ImportClaude { dir, dry_run } => {
            let dir = dir.unwrap_or_else(|| import::default_dir(&cfg.project_root));
            let items = import::plan(&dir)?;
            if dry_run {
                for i in &items {
                    println!("{} → {}/{} [[{}]] {}", i.source.display(), i.scope.as_str(), i.note_type.as_str(), i.name, i.title);
                }
                println!("{} memories in {} (dry run, nothing written)", items.len(), dir.display());
            } else {
                let mut store = store::Store::open(cfg)?;
                for (src, r) in import::run(&mut store, items)? {
                    match r {
                        import::ImportResult::Imported(p) => println!("imported {} → {p}", src.display()),
                        import::ImportResult::Skipped(why) => println!("skipped  {}: {why}", src.display()),
                    }
                }
            }
        }
        Cmd::Setup { agents } => {
            for agent in agents {
                let r = setup::setup(&cfg, agent)?;
                println!("{agent:?}:");
                for p in &r.changed {
                    println!("  updated   {p}");
                }
                for p in &r.unchanged {
                    println!("  unchanged {p}");
                }
                for m in &r.manual {
                    println!("  next: {m}");
                }
            }
        }
        Cmd::Guide => {
            println!("## Project memory (obmem)\n\n{}", guide::GUIDE);
        }
        Cmd::Verify { id } => {
            let mut store = store::Store::open(cfg)?;
            println!("{}", store.verify(&id, "human (cli)", true)?);
        }
        Cmd::Hook { .. } => unreachable!("handled before loading config"),
        Cmd::Status => {
            let store = store::Store::open(cfg.clone())?;
            println!("project root:   {}", cfg.project_root.display());
            println!("project name:   {}", cfg.project_name);
            println!("project vault:  {}", cfg.project_vault.display());
            println!("global vault:   {}", cfg.global_vault.display());
            for (name, v) in [("project", &cfg.project_vault), ("global", &cfg.global_vault)] {
                let state = match obsidian::vault_id(v) {
                    Ok(Some(id)) => format!("registered (id {id})"),
                    Ok(None) => "NOT registered: open the folder in Obsidian (\"Open folder as vault\")".into(),
                    Err(e) => format!("unknown ({e:#})"),
                };
                println!("obsidian {name:<7} {state}");
            }
            println!("secret rules:   {}", store.scanner().rule_count());
            println!("known secrets:  {} values loaded (from .env files / env vars, memory only)", store.scanner().known_value_count());
            println!("secret mode:    {:?}", cfg.secrets.mode);
        }
    }
    Ok(())
}

/// Human-readable audit report (shared by the CLI and the MCP tool).
pub fn audit_text(
    report: &audit::Audit,
    shown: &[&audit::Issue],
    stats: Option<&audit::WriteStats>,
    min: audit::Severity,
    max_issues: usize,
) -> String {
    let mut out = String::new();
    let count = |s: audit::Severity| report.issues.iter().filter(|i| i.severity == s).count();
    out.push_str(&format!(
        "obmem audit: {} notes checked; {} error, {} warn, {} info ({} suppressed). Showing >= {}.\n",
        report.checked_notes,
        count(audit::Severity::Error),
        count(audit::Severity::Warn),
        count(audit::Severity::Info),
        report.suppressed,
        min.as_str()
    ));
    for n in &report.notices {
        out.push_str(&format!("note: {n}\n"));
    }
    for i in shown.iter().take(max_issues) {
        out.push_str(&format!(
            "{:<5} {:<11} {}:{}\n      {}\n      fix: {}\n",
            i.severity.as_str(), i.rule, i.scope, i.path, i.message, i.fix
        ));
    }
    if shown.len() > max_issues {
        out.push_str(&format!("… and {} more (narrow with scope / rules)\n", shown.len() - max_issues));
    }
    if let Some(st) = stats {
        out.push_str(&format!(
            "Findings: {} created, {} reopened, {} closed, {} newly suppressed.\n",
            st.created.len(), st.reopened.len(), st.closed.len(), st.newly_suppressed
        ));
        for c in &st.created {
            out.push_str(&format!("  + {c}\n"));
        }
        for c in &st.reopened {
            out.push_str(&format!("  ↺ {c}\n"));
        }
        for c in &st.closed {
            out.push_str(&format!("  ✓ {c}\n"));
        }
    }
    out
}
