use obmem_agents::{hooks, setup};
use obmem_core::{audit, config, context, import, init, note, projects, settings, status, store};
use obmem_mcp as server;
use obmem_sync::Target;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "obmem",
    version,
    about = "Obsidian-backed memory for coding agents (MCP server + CLI)"
)]
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
    Init {
        /// Keep the project vault in this folder, outside the repo, instead of `docs/kb`: for
        /// memory that must not be in the repo. Your own config records the folder
        /// (`[[project]]` in ~/.obmem/config.toml); `.obmem.toml` only says the vault
        /// is private. Also how another machine says where its copy of the vault is.
        #[arg(long, value_name = "FOLDER")]
        private: Option<PathBuf>,
        /// With --private: the bucket the team syncs the vault through. Asks for the bucket's
        /// access key and the vault's passphrase. When the bucket already holds the vault, it is
        /// fetched into the folder, which must be empty; else the vault here is the first there.
        #[arg(long, requires = "private")]
        bucket: Option<String>,
        /// The bucket's S3 endpoint, for R2 or MinIO (leave out for AWS S3).
        #[arg(long, requires = "bucket")]
        endpoint: Option<String>,
        /// The bucket's region (AWS needs it; R2 uses "auto", the default).
        #[arg(long, requires = "bucket")]
        region: Option<String>,
        /// The key prefix inside the bucket (default: obmem).
        #[arg(long, requires = "bucket")]
        prefix: Option<String>,
    },
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
    /// Show resolved paths and counts.
    Status,
    /// Update obmem to the newest release. A binary from the install script is replaced; for
    /// one that npm or cargo installed, this prints the command that updates it.
    Update {
        /// Only say whether a newer version exists.
        #[arg(long)]
        check: bool,
    },
    /// Sync the personal vault with an S3-compatible bucket, encrypted (`[sync]` in
    /// ~/.obmem/config.toml), and this project's vault when it is private and has a bucket
    /// of its own (`[project.sync]`). Without a subcommand: sync now; every sync is also a snapshot.
    Sync {
        /// The vault a subcommand is about (default: personal). Without a subcommand: sync only
        /// this one, instead of both.
        #[arg(long, value_enum, global = true)]
        vault: Option<SyncVault>,
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

#[derive(Clone, Copy, PartialEq, clap::ValueEnum)]
enum SyncVault {
    Personal,
    /// The current project's private vault.
    Project,
}

impl SyncVault {
    fn of(target: &Target) -> Self {
        if target.is_project() {
            SyncVault::Project
        } else {
            SyncVault::Personal
        }
    }

    fn name(self) -> &'static str {
        match self {
            SyncVault::Personal => "personal",
            SyncVault::Project => "project",
        }
    }
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Save the bucket's access key on this device, and choose the backup passphrase (first
    /// device) or unlock the backup with it (every other device).
    Setup,
    /// Print this device's backup key, for a machine without a keychain such as a server or a
    /// cloud sandbox. Set it there as OBMEM_SYNC_VAULT_KEY, next to the bucket's access key in
    /// OBMEM_SYNC_ACCESS_KEY_ID and OBMEM_SYNC_SECRET_ACCESS_KEY. Whoever has the key and can
    /// read the bucket can read the whole backup.
    Key,
    /// List the snapshots in the bucket.
    List,
    /// Remove old snapshots and the content only they needed. Kept: everything from the last 7
    /// days, the newest of each day for 30 days, of each week for a year, and the latest. Runs on
    /// its own once a day after a sync.
    Prune {
        /// Only report what would be removed.
        #[arg(long)]
        dry_run: bool,
    },
    /// Restore a snapshot into an empty folder. Default: the latest snapshot, into the vault,
    /// which must then be empty or missing (a new device).
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
        Some(s) => hooks::HookInput {
            session_id: Some(s.to_string()),
            ..Default::default()
        },
        None => hooks::HookInput::from_stdin(),
    };
    let agent = match which {
        HookCmd::SessionStart { agent, .. }
        | HookCmd::Stop { agent }
        | HookCmd::PreCompact { agent } => *agent,
    };
    if input.misrouted(agent) {
        return;
    }
    let dir = project
        .map(PathBuf::from)
        .or_else(|| input.project_dir().map(PathBuf::from));
    // What automatic sync carries, when it is on; and whether this machine does not outlive the
    // session.
    let targets = auto_targets(dir.as_deref());
    let ephemeral = targets.iter().any(|t| t.cfg.is_ephemeral());
    // The hook's answer, and whether to sync after it.
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
                // An ephemeral machine starts without its vaults: fetch them before reading the
                // context from them. Nothing is left to send afterwards.
                if ephemeral {
                    targets.iter().for_each(sync_and_wait);
                }
                let mut store = store::Store::open(cfg)?;
                (
                    hooks::session_start(&mut store, &input, agent, *max_chars, now)?,
                    !ephemeral,
                )
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
    if sync_after && !targets.is_empty() {
        if ephemeral {
            targets.iter().for_each(sync_and_wait);
        } else {
            spawn_background_sync(dir.as_deref());
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
    if cli
        .project
        .as_ref()
        .is_some_and(|p| p.to_string_lossy().contains("${"))
    {
        eprintln!(
            "obmem: ignoring uninterpolated --project {}",
            cli.project.as_ref().unwrap().display()
        );
        cli.project = None;
    }
    if let Cmd::Hook { which, session } = &cli.cmd {
        run_hook(which, cli.project.as_deref(), session.as_deref());
        return Ok(());
    }
    if let Cmd::Sync {
        vault,
        action,
        allow_deletes,
        background,
    } = &cli.cmd
    {
        // Sync works outside a project too: then only the personal vault is involved.
        if *background {
            for target in auto_targets(cli.project.as_deref()) {
                let _ = obmem_sync::run(&target, false, obmem_sync::HOOK_WAIT);
            }
            return Ok(());
        }
        return run_sync(
            action.as_ref(),
            *allow_deletes,
            *vault,
            cli.project.as_deref(),
        );
    }
    if let Cmd::Update { check } = &cli.cmd {
        return run_update(*check);
    }
    // A private project loads only once this machine's config names its vault.
    let mut left_in_repo = None;
    if let Cmd::Init {
        private: Some(folder),
        bucket,
        endpoint,
        region,
        prefix,
    } = &cli.cmd
    {
        let root = config::project_root(cli.project.as_deref())?;
        let folder = std::path::absolute(folder)?;
        let bucket = bucket.as_ref().map(|name| config::Bucket {
            endpoint: endpoint.clone(),
            region: region.clone(),
            bucket: Some(name.clone()),
            prefix: prefix.clone(),
        });
        left_in_repo = settings::make_private(&root, &folder.to_string_lossy(), bucket.as_ref())?;
    }
    let cfg = config::Config::load(cli.project.as_deref())?;

    match cli.cmd {
        Cmd::Init { bucket, .. } => {
            // Before the vault's first files: a vault the team already has is fetched, not started.
            let joined = match bucket {
                Some(_) => Some(join_project_vault(&cfg)?),
                None => None,
            };
            let created = match joined {
                Some(obmem_sync::Joined::Fetched { .. }) => init::run_beside_fetched_vault(&cfg)?,
                _ => init::run(&cfg)?,
            };
            projects::record(&cfg.project_root)?;
            println!(
                "Project vault: {}\nGlobal vault:  {}",
                cfg.project_vault.display(),
                cfg.global_vault.display()
            );
            if created.is_empty() {
                println!("Nothing to do, everything already exists.");
            } else {
                println!("Created:");
                for c in created {
                    println!("  {c}");
                }
            }
            match joined {
                Some(obmem_sync::Joined::Fetched { seq, files }) => println!(
                    "\nFetched the team's vault: snapshot {seq}, {files} files. `obmem sync` and \
                     agent sessions keep it in sync from now on."
                ),
                Some(_) => println!(
                    "\nThe bucket holds no notes yet: `obmem sync` sends this vault there. Give \
                     teammates the bucket, an access key and the passphrase."
                ),
                None => {}
            }
            if let Some(old) = left_in_repo {
                println!(
                    "\nThe notes in {} were not moved. Move them to the private vault yourself, \
                     and take the folder out of git.",
                    old.display()
                );
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
                println!(
                    "[[{}]] ({}/{}) {}\n    {}",
                    h.id,
                    h.scope,
                    h.note_type,
                    h.title,
                    h.snippet.replace('\n', " ")
                );
            }
            for p in problems {
                eprintln!("note: {p}");
            }
        }
        Cmd::Get { id } => {
            let mut store = store::Store::open(cfg)?;
            let (loc, content, bl) = store.get(&id)?;
            let bl = bl
                .map(|b| b.join(", "))
                .unwrap_or_else(|e| format!("unavailable ({e})"));
            println!("<!-- {loc} | backlinks: {bl} -->\n{content}");
        }
        Cmd::Context { task, limit } => {
            let mut store = store::Store::open(cfg)?;
            print!(
                "{}",
                context::context(&mut store, &task.join(" "), limit, 6000)?
            );
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
                let bytes = std::fs::read(&p)
                    .map_err(|e| anyhow::anyhow!("cannot scan {}: {e}", p.display()))?;
                inputs.push((
                    p.display().to_string(),
                    String::from_utf8_lossy(&bytes).into_owned(),
                ));
            }
            let mut found = 0;
            for (name, text) in inputs {
                for h in store.scanner().scan(&text) {
                    found += 1;
                    println!(
                        "{name}:{}: {} ({}) {} fingerprint={}",
                        h.line, h.rule, h.description, h.preview, h.fingerprint
                    );
                }
            }
            if found > 0 {
                std::process::exit(1);
            }
        }
        Cmd::Audit {
            scope,
            rules,
            min_severity,
            write,
            json,
            fail_on,
        } => {
            let mut store = store::Store::open(cfg)?;
            let opts = audit::AuditOptions {
                scope,
                rules,
                today: chrono::Local::now().date_naive(),
            };
            let (report, stats) = store.audit(&opts, write.then_some(min_severity))?;
            let shown: Vec<&audit::Issue> = report
                .issues
                .iter()
                .filter(|i| i.severity >= min_severity)
                .collect();
            if json {
                let v = serde_json::json!({
                    "checked_notes": report.checked_notes,
                    "suppressed": report.suppressed,
                    "issues": shown,
                    "findings": stats,
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print!(
                    "{}",
                    audit::report_text(&report, &shown, stats.as_ref(), min_severity, usize::MAX)
                );
            }
            if let Some(level) = fail_on
                && report.issues.iter().any(|i| i.severity >= level)
            {
                std::process::exit(1);
            }
        }
        Cmd::Resolve {
            id,
            verdict,
            comment,
        } => {
            let mut store = store::Store::open(cfg)?;
            println!(
                "{}",
                store.resolve_finding(&id, verdict, comment.as_deref(), "human (cli)", true)?
            );
        }
        Cmd::ImportClaude { dir, dry_run } => {
            let dir = dir.unwrap_or_else(|| import::default_dir(&cfg.project_root));
            let items = import::plan(&dir)?;
            if dry_run {
                for i in &items {
                    println!(
                        "{} → {}/{} [[{}]] {}",
                        i.source.display(),
                        i.scope.as_str(),
                        i.note_type.as_str(),
                        i.name,
                        i.title
                    );
                }
                println!(
                    "{} memories in {} (dry run, nothing written)",
                    items.len(),
                    dir.display()
                );
            } else {
                let mut store = store::Store::open(cfg)?;
                for (src, r) in import::run(&mut store, items)? {
                    match r {
                        import::ImportResult::Imported(p) => {
                            println!("imported {} → {p}", src.display())
                        }
                        import::ImportResult::Skipped(why) => {
                            println!("skipped  {}: {why}", src.display())
                        }
                    }
                }
            }
        }
        Cmd::Setup { agents, check } => {
            for agent in agents {
                let r = if check {
                    setup::check(&cfg, agent)?
                } else {
                    setup::setup(&cfg, agent)?
                };
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
        Cmd::Hook { .. } | Cmd::Sync { .. } | Cmd::Update { .. } => {
            unreachable!("handled before loading config")
        }
        Cmd::Status => {
            let store = store::Store::open(cfg)?;
            let st = status::status(&store);
            print_binary();
            println!("project root:   {}", st.project_root.display());
            println!("project name:   {}", st.project_name);
            if !st.initialized {
                println!("not initialized: run `obmem init` here");
            }
            let headless = st.backend == obmem_core::obsidian::Backend::Headless;
            println!(
                "search:         {}",
                if headless {
                    "built in (no Obsidian CLI on PATH)"
                } else {
                    "Obsidian CLI"
                }
            );
            for (name, v) in [("project", &st.project_vault), ("global", &st.global_vault)] {
                let private = if name == "project" && st.private {
                    " (private: outside the repo)"
                } else {
                    ""
                };
                println!(
                    "{:<16}{}{private}",
                    format!("{name} vault:"),
                    v.path.display()
                );
                // Without Obsidian nothing needs the vault to be registered.
                if headless {
                    continue;
                }
                let state = match &v.obsidian {
                    status::Registration::Registered { id } => format!("registered (id {id})"),
                    status::Registration::NotRegistered => {
                        "NOT registered: open the folder in Obsidian (\"Open folder as vault\")"
                            .into()
                    }
                    status::Registration::Unknown(e) => format!("unknown ({e})"),
                };
                println!("obsidian {name:<7} {state}");
            }
            println!("drafts:         {} waiting for review", st.drafts);
            println!("findings:       {} open", st.open_findings);
            println!("secret rules:   {}", st.secret_rules);
            println!(
                "known secrets:  {} values loaded (from .env files / env vars, memory only)",
                st.known_secrets
            );
            println!("secret mode:    {:?}", st.secret_mode);
            println!(
                "sync:           {}",
                sync_status_line(
                    personal_target(),
                    "not set up (add [sync] to your config, then `obmem sync setup`)"
                )
            );
            if st.private {
                println!(
                    "project sync:   {}",
                    sync_status_line(
                        Target::project(store.config()),
                        "not set up (add [project.sync] after this project's [[project]] in \
                         your config, then `obmem sync setup --vault project`)"
                    )
                );
            }
        }
    }
    Ok(())
}

/// Which `obmem` this is: with several installs on one machine, the one that runs is the first
/// on PATH, which is not always the one just installed or updated.
fn print_binary() {
    use obmem_update::Install;
    let Ok((exe, install)) = Install::current() else {
        return;
    };
    println!("binary:         {} ({})", exe.display(), install.label());
    // npm's command on PATH is its launcher script, never the binary itself.
    if install == Install::Npm {
        return;
    }
    let first = config::find_program("obmem").map(|p| std::fs::canonicalize(&p).unwrap_or(p));
    if let Some(first) = first.filter(|first| *first != exe) {
        println!(
            "                `obmem` on PATH is another install: {}",
            first.display()
        );
    }
}

fn run_update(check: bool) -> Result<()> {
    use obmem_update::Install;
    let (exe, install) = Install::current()?;
    let current = obmem_update::current_version();
    let latest = obmem_update::latest(install)?;
    if latest <= current {
        println!("obmem {current} is the newest version");
        return Ok(());
    }
    println!(
        "obmem {latest} is available: this is {current}, {} at {}",
        install.label(),
        exe.display()
    );
    match install.update_command() {
        Some(command) => println!("update it with: {command}"),
        None if check => println!("update it with: obmem update"),
        None => obmem_update::install(&exe, &latest)?,
    }
    Ok(())
}

/// The personal vault as a sync target, when `[sync]` names a bucket.
fn personal_target() -> Result<Option<Target>> {
    let cfg = config::sync_config()?;
    if cfg.bucket.is_none() {
        return Ok(None);
    }
    Target::personal(cfg).map(Some)
}

/// The project's vault as a sync target, when it is private and has a bucket of its own.
fn project_target(project: Option<&std::path::Path>) -> Result<Option<Target>> {
    Target::project(&config::Config::load(project)?)
}

/// What a hook syncs: every vault that has a bucket, unless `[sync] auto` is off. A project that
/// does not load is for the hook itself to report.
fn auto_targets(project: Option<&std::path::Path>) -> Vec<Target> {
    [personal_target(), project_target(project)]
        .into_iter()
        .filter_map(|target| target.ok().flatten())
        .filter(|target| target.cfg.auto_sync())
        .collect()
}

/// The one vault a command is about, or what is missing to sync it.
fn sync_target(which: SyncVault, project: Option<&std::path::Path>) -> Result<Target> {
    let file = config::global_config_path();
    match which {
        SyncVault::Personal => personal_target()?.ok_or_else(|| {
            anyhow::anyhow!(
                "no backup bucket configured: add a [sync] table with `bucket` (and `endpoint` for R2 or MinIO) to {}",
                file.display()
            )
        }),
        SyncVault::Project => {
            let cfg = config::Config::load(project)?;
            if !cfg.vault_is_private() {
                anyhow::bail!(
                    "{} is in the repo, where git carries it. Only a private vault is synced: `obmem init --private <folder>`",
                    cfg.project_vault.display()
                );
            }
            Target::project(&cfg)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "no bucket configured for this project's vault: add a [project.sync] table with `bucket` (and `endpoint` for R2 or MinIO) after its [[project]] in {}",
                    file.display()
                )
            })
        }
    }
}

fn run_sync(
    action: Option<&SyncCmd>,
    allow_deletes: bool,
    which: Option<SyncVault>,
    project: Option<&std::path::Path>,
) -> Result<()> {
    use obmem_sync as sync;
    let Some(action) = action else {
        let Some(which) = which else {
            return sync_all(project, allow_deletes);
        };
        return sync_now(&sync_target(which, project)?, allow_deletes);
    };
    let target = sync_target(which.unwrap_or(SyncVault::Personal), project)?;
    let cfg = &target.cfg;
    let vault = SyncVault::of(&target).name();
    match action {
        SyncCmd::Setup => {
            let (creds, passphrase) = ask_keys(&target)?;
            let created = sync::connect(&target, creds, &passphrase.into())?;
            println!(
                "{} the backup in {}; the access key and backup key are in this device's keychain",
                if created { "created" } else { "unlocked" },
                cfg.bucket.as_deref().unwrap_or_default()
            );
        }
        SyncCmd::Key => {
            let key = target.key_text()?;
            eprintln!(
                "This key reads the whole backup of the {vault} vault. Keep it in a secret store, never in a file in a repository. On a machine without a keychain it goes in {}.",
                target.key_variable()
            );
            println!("{key}");
        }
        SyncCmd::List => {
            let (creds, key) = target.keys()?;
            let remote = sync::Remote::s3(cfg, &creds)?;
            let seqs = sync::snapshot_seqs(&remote)?;
            if seqs.is_empty() {
                println!("no snapshots yet: run `obmem sync`");
            }
            for seq in seqs {
                let m = sync::load_snapshot(&remote, &key, seq)?;
                println!(
                    "{seq:>6}  {}  {:<20} {} files",
                    m.created,
                    m.device,
                    m.files.len()
                );
            }
        }
        SyncCmd::Prune { dry_run } => {
            let (creds, key) = target.keys()?;
            let remote = sync::Remote::s3(cfg, &creds)?;
            let r = sync::prune(&remote, &key, chrono::Utc::now(), *dry_run)?;
            let verb = if *dry_run { "would remove" } else { "removed" };
            let list: Vec<String> = r.deleted_snapshots.iter().map(u64::to_string).collect();
            println!(
                "{verb} {} snapshots ({}) and {} unused blobs, {:.1} MB; {} snapshots kept",
                list.len(),
                if list.is_empty() {
                    "none".into()
                } else {
                    list.join(", ")
                },
                r.deleted_blobs,
                r.freed_bytes as f64 / 1_000_000.0,
                r.kept
            );
        }
        SyncCmd::Restore { snapshot, to } => {
            let (creds, key) = target.keys()?;
            let remote = sync::Remote::s3(cfg, &creds)?;
            let folder = to.as_ref().unwrap_or(&target.vault);
            let r = sync::restore(&remote, &key, *snapshot, folder)?;
            println!(
                "restored snapshot {} ({} files) into {}",
                r.seq,
                r.files,
                folder.display()
            );
            println!(
                "next: open the folder in Obsidian (\"Open folder as vault\"); if it is the {vault} vault, `obmem sync` keeps it in sync from now on"
            );
        }
    }
    Ok(())
}

/// Ask for the bucket's access key, and for the passphrase: the one that unlocks the backup the
/// bucket holds, or a new one when it holds none.
fn ask_keys(target: &Target) -> Result<(obmem_sync::Credentials, String)> {
    use obmem_sync as sync;
    let creds = sync::Credentials {
        access_key_id: prompt("Access key ID: ")?,
        secret_access_key: rpassword::prompt_password("Secret access key: ")?,
    };
    let remote = sync::Remote::s3(&target.cfg, &creds)?;
    let passphrase = if sync::has_key(&remote)? {
        rpassword::prompt_password("Backup passphrase: ")?
    } else {
        eprintln!("This bucket holds no backup yet. Choose a passphrase: it encrypts the backup,");
        eprintln!(
            "and every other device unlocks it with the same one. Without it the backup cannot be read."
        );
        let first = rpassword::prompt_password("New backup passphrase: ")?;
        if first.is_empty() {
            anyhow::bail!("the passphrase cannot be empty");
        }
        if rpassword::prompt_password("Repeat it: ")? != first {
            anyhow::bail!("the passphrases differ");
        }
        first
    };
    Ok((creds, passphrase))
}

/// `obmem init --private <folder> --bucket <name>`: connect this device to the project vault's
/// bucket, and fetch the vault when the team already has one.
fn join_project_vault(cfg: &config::Config) -> Result<obmem_sync::Joined> {
    let target = Target::project(cfg)?
        .ok_or_else(|| anyhow::anyhow!("the project's vault has no bucket in your config"))?;
    let (creds, passphrase) = ask_keys(&target)?;
    obmem_sync::join(&target, creds, &passphrase.into())
}

/// `obmem sync`: the personal vault, and the project's when it is private, each one that has a
/// bucket. One that fails does not stop the other.
fn sync_all(project: Option<&std::path::Path>, allow_mass_delete: bool) -> Result<()> {
    let mut targets: Vec<Target> = personal_target()?.into_iter().collect();
    match project_target(project) {
        Ok(target) => targets.extend(target),
        // The personal vault syncs anywhere, also beside a project that does not load.
        Err(e) => eprintln!("obmem: the project vault is not synced: {e:#}"),
    }
    match targets.as_slice() {
        [] => sync_target(SyncVault::Personal, project).map(|_| ()),
        [only] => sync_now(only, allow_mass_delete),
        several => {
            let mut failed = Vec::new();
            for target in several {
                let vault = SyncVault::of(target).name();
                println!("{vault} vault:");
                if let Err(e) = sync_now(target, allow_mass_delete) {
                    eprintln!("obmem: the {vault} vault did not sync: {e:#}");
                    failed.push(vault);
                }
            }
            if !failed.is_empty() {
                anyhow::bail!("not synced: the {} vault", failed.join(" and the "));
            }
            Ok(())
        }
    }
}

fn sync_now(target: &Target, allow_mass_delete: bool) -> Result<()> {
    let Some(r) = obmem_sync::run(target, allow_mass_delete, std::time::Duration::ZERO)? else {
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
        (
            "left for the next sync (changed while syncing)",
            &r.deferred,
        ),
    ] {
        for n in names {
            println!("{label}: {n}");
        }
    }
    for n in &r.conflicts {
        println!(
            "CONFLICT: {n} is another device's version; merge it into the original, then delete it"
        );
    }
    println!("{}", obmem_sync::Summary::from(&r));
    Ok(())
}

/// A vault's sync for `obmem status`. `not_set_up` is what to say when it has no bucket.
fn sync_status_line(target: Result<Option<Target>>, not_set_up: &str) -> String {
    let target = match target {
        Ok(Some(target)) => target,
        Ok(None) => return not_set_up.into(),
        Err(e) => return format!("config problem: {e:#}"),
    };
    let cfg = &target.cfg;
    let auto = if !cfg.auto_sync() {
        " (automatic sync off)"
    } else if cfg.is_ephemeral() {
        " (ephemeral machine: agent hooks wait for the sync)"
    } else {
        ""
    };
    match obmem_sync::last_status(&target) {
        None => format!("not run on this device yet{auto}"),
        Some(s) => match s.outcome {
            obmem_sync::Outcome::Failed(why) => format!("FAILED at {}: {why}{auto}", s.at),
            done => format!("{done} at {}{auto}", s.at),
        },
    }
}

/// On a machine that does not outlive the session, the hook itself syncs and waits: the machine
/// could be gone before a background sync ends. A failure never fails the hook: it goes to
/// stderr and to the sync status.
fn sync_and_wait(target: &Target) {
    if let Err(e) = obmem_sync::run(target, false, obmem_sync::HOOK_WAIT) {
        eprintln!("obmem: the sync failed: {e:#}");
    }
}

/// After a session starts or a turn ends, sync in a process of its own, so the agent never waits
/// on the network: the personal vault, and the vault of `project` when it is private. Errors go
/// to the sync status, never to the agent.
fn spawn_background_sync(project: Option<&std::path::Path>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    if let Some(project) = project {
        cmd.arg("--project").arg(project);
    }
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
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
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
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&cfg.project_root)
            .args(args)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    };
    // git prints paths relative to the repository top, which need not be the project root
    let top =
        PathBuf::from(String::from_utf8_lossy(&git(&["rev-parse", "--show-toplevel"])?).trim());
    let top = std::fs::canonicalize(&top).unwrap_or(top);
    let vault =
        std::fs::canonicalize(&cfg.project_vault).unwrap_or_else(|_| cfg.project_vault.clone());
    let listing = git(&[
        "diff",
        "--cached",
        "--name-only",
        "-z",
        "--diff-filter=ACMR",
    ])?;
    let mut inputs = Vec::new();
    for raw in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8(raw.to_vec()).map_err(|_| {
            anyhow::anyhow!("staged path is not UTF-8: {}", String::from_utf8_lossy(raw))
        })?;
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
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
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
        assert!(
            names.contains(&"docs/kb/note.md") && names.contains(&"docs/kb/筆記 one.md"),
            "{names:?}"
        );
        assert!(!names.contains(&"src.rs"), "{names:?}");
        assert!(
            got.iter().all(|(_, text)| text.contains(token)),
            "staged content, not the cleaned working copy"
        );

        // 4. a rename of a committed vault file is scanned too
        g(&["commit", "-qm", "c"]);
        g(&["mv", "docs/kb/筆記 one.md", "docs/kb/renamed.md"]);
        let got = staged_vault_inputs(&cfg).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "docs/kb/renamed.md");
        let _ = std::fs::remove_dir_all(&root);
    }
}
