use obmem_agents::{hooks, setup};
use obmem_core::{audit, config, context, guide, import, init, note, projects, status, store};
use obmem_mcp as server;

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
        /// Only report what is missing or outdated; write nothing.
        #[arg(long)]
        check: bool,
    },
    /// Accept an agent's draft note: draft → active.
    Approve { id: String },
    /// The projects obmem has seen on this machine (recorded by `init` and the session-start hook).
    Projects {
        #[command(subcommand)]
        action: Option<ProjectsCmd>,
    },
    /// Print the agent usage guide (paste into AGENTS.md / CLAUDE.md for agents without hooks).
    Guide,
    /// Show resolved paths and counts.
    Status,
    /// Sync the personal vault with an S3-compatible bucket, encrypted (`[sync]` in
    /// ~/.config/obmem/config.toml). Without a subcommand: sync now; every sync is also a snapshot.
    Sync {
        /// Go ahead even if the sync would delete more than half of the vault.
        #[arg(long)]
        allow_deletes: bool,
        /// Started by an agent hook: wait for a running sync, print nothing (the outcome goes to
        /// the status `obmem status` shows).
        #[arg(long, hide = true)]
        background: bool,
        #[command(subcommand)]
        action: Option<SyncCmd>,
    },
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Save the bucket's access key on this device, and choose the backup passphrase (first
    /// device) or unlock the backup with it (every other device).
    Setup,
    /// List the snapshots in the bucket.
    List,
    /// Restore a snapshot into an empty folder. Default: the latest snapshot, into the personal
    /// vault, which must then be empty or missing (a new device).
    Restore {
        #[arg(long)]
        snapshot: Option<u64>,
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProjectsCmd {
    /// Add a project folder to the list.
    Add { path: PathBuf },
    /// Remove a project folder from the list (its files are not touched).
    Remove { path: PathBuf },
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
    if input.misrouted(agent) {
        return;
    }
    let dir = project.map(PathBuf::from).or_else(|| input.project_dir().map(PathBuf::from));
    // The hook's answer, and whether to sync the personal vault after it.
    let out = (|| -> Result<(String, bool)> {
        let cfg = config::Config::load(dir.as_deref())?;
        // Like every hook, a no-op outside obmem projects: hooks installed user-wide stay harmless.
        if !cfg.project_root.join(config::PROJECT_CONFIG_FILE).exists() {
            return Ok((agent.empty().to_string(), false));
        }
        let now = std::time::SystemTime::now();
        Ok(match which {
            HookCmd::SessionStart { max_chars, .. } => {
                if let Err(e) = projects::record(&cfg.project_root) {
                    eprintln!("obmem: could not record the project: {e:#}");
                }
                let mut store = store::Store::open(cfg)?;
                (hooks::session_start(&mut store, &input, agent, *max_chars, now)?, true)
            }
            HookCmd::Stop { .. } => {
                // A stop that asks the agent to log first is not the end of the turn: sync after
                // the stop that lets it end, so the log entry goes along.
                let ask = hooks::stop(&cfg, &input, agent, now)?;
                (agent.stop_output(ask.as_deref()), ask.is_none())
            }
            HookCmd::PreCompact { .. } => {
                let mut store = store::Store::open(cfg)?;
                (hooks::pre_compact(&mut store, &input, agent)?, false)
            }
        })
    })();
    let sync_after = matches!(out, Ok((_, true)));
    match out {
        Ok((s, _)) if s.is_empty() => {}
        Ok((s, _)) => println!("{s}"),
        Err(e) => {
            eprintln!("obmem: {e:#}");
            if !agent.empty().is_empty() {
                println!("{}", agent.empty());
            }
        }
    }
    if sync_after {
        spawn_background_sync();
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
    if let Cmd::Sync { action, allow_deletes, background } = &cli.cmd {
        // Only the personal vault and the user's config are involved: no project to load.
        if *background {
            if let Ok(cfg) = config::sync_config() {
                let _ = obmem_sync::run(&cfg, false, obmem_sync::BACKGROUND_WAIT);
            }
            return Ok(());
        }
        return run_sync(action.as_ref(), *allow_deletes);
    }
    let cfg = config::Config::load(cli.project.as_deref())?;

    match cli.cmd {
        Cmd::Init => {
            let created = init::run(&cfg)?;
            projects::record(&cfg.project_root)?;
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
            if staged {
                inputs.extend(staged_vault_inputs(&cfg)?);
            }
            if files.is_empty() && !staged {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                inputs.push(("<stdin>".into(), s));
            }
            // Fail closed: a file that cannot be read is an error, not a silent pass.
            for p in files {
                let bytes = std::fs::read(&p).map_err(|e| anyhow::anyhow!("cannot scan {}: {e}", p.display()))?;
                inputs.push((p.display().to_string(), String::from_utf8_lossy(&bytes).into_owned()));
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
                print!("{}", audit::report_text(&report, &shown, stats.as_ref(), min_severity, usize::MAX));
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
        Cmd::Setup { agents, check } => {
            for agent in agents {
                let r = if check { setup::check(&cfg, agent)? } else { setup::setup(&cfg, agent)? };
                println!("{}:", agent.label());
                for p in &r.changed {
                    println!("  {} {p}", if check { "missing  " } else { "updated  " });
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
        Cmd::Approve { id } => {
            let mut store = store::Store::open(cfg)?;
            println!("{}", store.approve(&id, "human (cli)")?);
        }
        Cmd::Projects { action } => match action {
            None => {
                for p in projects::list()? {
                    println!("{}  {}", p.last_seen, p.path.display());
                }
            }
            Some(ProjectsCmd::Add { path }) => {
                projects::record(&path)?;
                println!("added {}", std::path::absolute(&path)?.display());
            }
            Some(ProjectsCmd::Remove { path }) => {
                if projects::remove(&path)? {
                    println!("removed {}", path.display());
                } else {
                    println!("{} was not listed", path.display());
                }
            }
        },
        Cmd::Verify { id } => {
            let mut store = store::Store::open(cfg)?;
            println!("{}", store.verify(&id, "human (cli)", true)?);
        }
        Cmd::Hook { .. } | Cmd::Sync { .. } => unreachable!("handled before loading config"),
        Cmd::Status => {
            let st = status::status(&store::Store::open(cfg)?);
            println!("project root:   {}", st.project_root.display());
            println!("project name:   {}", st.project_name);
            if !st.initialized {
                println!("not initialized: run `obmem init` here");
            }
            for (name, v) in [("project", &st.project_vault), ("global", &st.global_vault)] {
                println!("{:<16}{}", format!("{name} vault:"), v.path.display());
                let state = match &v.obsidian {
                    status::Registration::Registered { id } => format!("registered (id {id})"),
                    status::Registration::NotRegistered => {
                        "NOT registered: open the folder in Obsidian (\"Open folder as vault\")".into()
                    }
                    status::Registration::Unknown(e) => format!("unknown ({e})"),
                };
                println!("obsidian {name:<7} {state}");
            }
            println!("drafts:         {} waiting for review", st.drafts);
            println!("findings:       {} open", st.open_findings);
            println!("secret rules:   {}", st.secret_rules);
            println!("known secrets:  {} values loaded (from .env files / env vars, memory only)", st.known_secrets);
            println!("secret mode:    {:?}", st.secret_mode);
            println!("sync:           {}", sync_status_line());
        }
    }
    Ok(())
}

fn run_sync(action: Option<&SyncCmd>, allow_deletes: bool) -> Result<()> {
    use obmem_sync as sync;
    let cfg = config::sync_config()?;
    if cfg.bucket.is_none() {
        anyhow::bail!(
            "no backup bucket configured: add a [sync] table with `bucket` (and `endpoint` for R2 or MinIO) to {}",
            config::global_config_path().display()
        );
    }
    let Some(action) = action else {
        return sync_now(&cfg, allow_deletes);
    };
    match action {
        SyncCmd::Setup => {
            let creds = sync::Credentials {
                access_key_id: prompt("Access key ID: ")?,
                secret_access_key: rpassword::prompt_password("Secret access key: ")?,
            };
            let remote = sync::Remote::s3(&cfg, &creds)?;
            let passphrase = if sync::has_key(&remote)? {
                rpassword::prompt_password("Backup passphrase: ")?
            } else {
                eprintln!("This bucket holds no backup yet. Choose a passphrase: it encrypts the backup,");
                eprintln!("and every other device unlocks it with the same one. Without it the backup cannot be read.");
                let first = rpassword::prompt_password("New backup passphrase: ")?;
                if first.is_empty() {
                    anyhow::bail!("the passphrase cannot be empty");
                }
                if rpassword::prompt_password("Repeat it: ")? != first {
                    anyhow::bail!("the passphrases differ");
                }
                first
            };
            let created = sync::connect(&cfg, creds, &passphrase.into())?;
            println!(
                "{} the backup in {}; the access key and backup key are in this device's keychain",
                if created { "created" } else { "unlocked" },
                cfg.bucket.as_deref().unwrap_or_default()
            );
        }
        SyncCmd::List => {
            let (creds, key) = sync::keychain::load(&cfg)?;
            let remote = sync::Remote::s3(&cfg, &creds)?;
            let seqs = sync::snapshot_seqs(&remote)?;
            if seqs.is_empty() {
                println!("no snapshots yet: run `obmem sync`");
            }
            for seq in seqs {
                let m = sync::load_snapshot(&remote, &key, seq)?;
                println!("{seq:>6}  {}  {:<20} {} files", m.created, m.device, m.files.len());
            }
        }
        SyncCmd::Restore { snapshot, to } => {
            let (creds, key) = sync::keychain::load(&cfg)?;
            let remote = sync::Remote::s3(&cfg, &creds)?;
            let target = match to {
                Some(p) => p.clone(),
                None => config::global_vault()?,
            };
            let r = sync::restore(&remote, &key, *snapshot, &target)?;
            println!("restored snapshot {} ({} files) into {}", r.seq, r.files, target.display());
            println!("next: open the folder in Obsidian (\"Open folder as vault\"); if it is the personal vault, `obmem sync` keeps it in sync from now on");
        }
    }
    Ok(())
}

fn sync_now(cfg: &config::SyncConfig, allow_mass_delete: bool) -> Result<()> {
    let Some(r) = obmem_sync::run(cfg, allow_mass_delete, std::time::Duration::ZERO)? else {
        return Ok(());
    };
    for (name, why) in &r.skipped {
        eprintln!("skipped {name}: {why}");
    }
    for (label, names) in [
        ("uploaded", &r.uploaded),
        ("downloaded", &r.downloaded),
        ("deleted here", &r.deleted_local),
        ("deleted in the bucket", &r.deleted_remote),
        ("left for the next sync (changed while syncing)", &r.deferred),
    ] {
        for n in names {
            println!("{label}: {n}");
        }
    }
    for n in &r.conflicts {
        println!("CONFLICT: {n} is another device's version; merge it into the original, then delete it");
    }
    println!("{}", obmem_sync::summary(&r));
    Ok(())
}

fn sync_status_line() -> String {
    let cfg = match config::sync_config() {
        Ok(c) if c.bucket.is_some() => c,
        Ok(_) => return "not set up (add [sync] to your config, then `obmem sync setup`)".into(),
        Err(e) => return format!("config unreadable: {e:#}"),
    };
    let auto = if cfg.auto_sync() { "" } else { " (automatic sync off)" };
    match obmem_sync::last_status(&cfg) {
        None => format!("not run on this device yet{auto}"),
        Some(s) if s.ok => format!("{} at {}{auto}", s.message, s.at),
        Some(s) => format!("FAILED at {}: {}{auto}", s.at, s.message),
    }
}

/// After a session starts or a turn ends, sync the personal vault in a process of its own, so the
/// agent never waits on the network. Errors go to the sync status, never to the agent.
fn spawn_background_sync() {
    let Ok(cfg) = config::sync_config() else { return };
    if !cfg.auto_sync() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["sync", "--background"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: the agent ending the hook's group must not end the sync.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        stop_sharing_std_handles();
    }
    if let Err(e) = cmd.spawn() {
        eprintln!("obmem: could not start the background sync: {e}");
    }
}

/// Windows children inherit every inheritable handle, and the agent reads the hook's output
/// until the last holder of its pipe closes it: without this, the agent would wait for the sync.
#[cfg(windows)]
fn stop_sharing_std_handles() {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles.
        unsafe {
            let h = GetStdHandle(id);
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn prompt(label: &str) -> Result<String> {
    eprint!("{label}");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let line = line.trim().to_string();
    if line.is_empty() {
        anyhow::bail!("no input");
    }
    Ok(line)
}

/// The staged files inside the project vault, read from the **index** (exactly what the commit
/// will contain), not from the working tree: otherwise staging a secret and then cleaning the
/// working copy would pass the hook. Paths come NUL-separated so quoting of unusual names
/// (non-ASCII, spaces) cannot make a file unreadable and silently skipped; renames and copies are
/// included; anything git cannot show is an error (fail closed).
fn staged_vault_inputs(cfg: &config::Config) -> Result<Vec<(String, String)>> {
    let git = |args: &[&str]| -> Result<Vec<u8>> {
        let out = std::process::Command::new("git").arg("-C").arg(&cfg.project_root).args(args).output()?;
        if !out.status.success() {
            anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(out.stdout)
    };
    // git prints paths relative to the repository top, which need not be the project root
    let top = PathBuf::from(String::from_utf8_lossy(&git(&["rev-parse", "--show-toplevel"])?).trim());
    let top = std::fs::canonicalize(&top).unwrap_or(top);
    let vault = std::fs::canonicalize(&cfg.project_vault).unwrap_or_else(|_| cfg.project_vault.clone());
    let listing = git(&["diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR"])?;
    let mut inputs = Vec::new();
    for raw in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8(raw.to_vec())
            .map_err(|_| anyhow::anyhow!("staged path is not UTF-8: {}", String::from_utf8_lossy(raw)))?;
        if !top.join(&rel).starts_with(&vault) {
            continue;
        }
        let content = git(&["show", &format!(":{rel}")])?;
        inputs.push((rel, String::from_utf8_lossy(&content).into_owned()));
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_scan_reads_the_index_not_the_working_tree() {
        let root = std::env::temp_dir().join(format!("obmem-staged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs/kb")).unwrap();
        let g = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        g(&["init", "-q"]);
        let cfg = config::Config {
            project_root: root.clone(),
            project_name: "t".into(),
            project_vault: root.join("docs/kb"),
            global_vault: root.join("g"),
            secrets: Default::default(),
            audit: Default::default(),
            auto_approve: false,
            device: "test".into(),
        };
        let token = concat!("gh", "p_Qm4Zr8Lx2Vb6Nc9Tw3Yh7Kp1Sd5Fg0Jk2LmA");

        // 1. stage a secret, then clean the working copy: the index still has it
        std::fs::write(root.join("docs/kb/note.md"), format!("token {token}\n")).unwrap();
        // 2. a non-ASCII name (git would quote it without -z)
        std::fs::write(root.join("docs/kb/筆記 one.md"), format!("also {token}\n")).unwrap();
        // 3. a file outside the vault is out of scope
        std::fs::write(root.join("src.rs"), format!("let t = \"{token}\";\n")).unwrap();
        g(&["add", "-A"]);
        std::fs::write(root.join("docs/kb/note.md"), "clean now\n").unwrap();

        let got = staged_vault_inputs(&cfg).unwrap();
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"docs/kb/note.md") && names.contains(&"docs/kb/筆記 one.md"), "{names:?}");
        assert!(!names.contains(&"src.rs"), "{names:?}");
        assert!(got.iter().all(|(_, text)| text.contains(token)), "staged content, not the cleaned working copy");

        // 4. a rename of a committed vault file is scanned too
        g(&["commit", "-qm", "c"]);
        g(&["mv", "docs/kb/筆記 one.md", "docs/kb/renamed.md"]);
        let got = staged_vault_inputs(&cfg).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "docs/kb/renamed.md");
        let _ = std::fs::remove_dir_all(&root);
    }
}
