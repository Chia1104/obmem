//! Obsidian itself does search, links, backlinks, unresolved links and orphans (official CLI,
//! 1.12+).
//! obmem only maps vault folders to Obsidian vault ids and parses the output. On a machine
//! without the CLI, [`crate::headless`] answers the same questions from the files
//! ([`Backend`]).
//!
//! Notes on the CLI that shape this module (verified against Obsidian 1.14):
//! - Without `vault=`, a folder that is not a registered vault silently falls back to the
//!   *active* vault, so every call passes `vault=<id>` looked up by path in `obsidian.json`.
//! - Vaults cannot be registered from the CLI or a URI; the user opens the folder once in
//!   Obsidian ("Open folder as vault").
//! - Errors and empty results are plain text on stdout with exit code 0
//!   (`Error: …`, `Vault not found.`, `No matches found.`), even with `format=json`.
//! - If Obsidian is not running the CLI fails ("unable to find Obsidian", exit 1), although
//!   the docs say it launches the app; obmem starts the app in the background and retries.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Vault-relative path.
    pub file: String,
    /// Matching (1-based line number, text), deduplicated, in order.
    pub lines: Vec<(usize, String)>,
}

#[derive(Debug, Clone)]
pub struct Unresolved {
    pub link: String,
    /// Vault-relative paths of the notes containing the link.
    pub sources: Vec<String>,
}

/// Where one of a note's links leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// The vault-relative path of the file it opens.
    File(String),
    /// The link's text, when no file has that name.
    Unresolved(String),
}

/// What obmem needs from Obsidian. [`Cli`] asks the app; [`crate::headless::Headless`] reads
/// the files where there is no app.
pub trait Api: Send {
    fn search(&self, vault: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>>;
    /// Where the links of the note at `rel` lead: each place once, by name, and never the note
    /// itself.
    fn links(&self, vault: &Path, rel: &str) -> Result<Vec<Link>>;
    fn backlinks(&self, vault: &Path, rel: &str) -> Result<Vec<String>>;
    fn unresolved(&self, vault: &Path) -> Result<Vec<Unresolved>>;
    fn orphans(&self, vault: &Path) -> Result<Vec<String>>;
}

/// Which [`Api`] a store uses for a vault. It is chosen once, when the store opens, and never
/// changed after a failed call: the two do not agree in every detail, and a vault whose answers
/// come from one and then the other would give answers that cannot be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The Obsidian app, through its CLI.
    App,
    /// obmem's own reading of the vault files, for a machine without Obsidian.
    Headless,
}

impl Backend {
    /// [`Backend::App`] when the Obsidian CLI command is installed, whether or not the app is
    /// running or knows the vaults: on a machine with Obsidian, a problem with it is reported,
    /// not hidden behind the other backend.
    pub fn detect() -> Self {
        if crate::config::find_program(&cli_bin()).is_some() {
            Backend::App
        } else {
            Backend::Headless
        }
    }

    /// The backend for the project's vault, on a machine whose backend is `self`.
    ///
    /// A linked git worktree has its own copy of the repo's vault, at a path of its own. Obsidian
    /// has not opened that copy, and nobody will open it: it is gone with the branch. Its files
    /// are read instead, so search and links answer from the branch's notes. This follows from
    /// where the vault is, when the store opens, and never from a failed call. A copy that
    /// Obsidian does know is asked there, and a registry that cannot be read is still reported.
    pub fn for_project_vault(self, cfg: &crate::config::Config) -> Self {
        let copy = cfg.project_root != cfg.main_root && !cfg.vault_is_private();
        if self == Backend::App && copy && matches!(vault_id(&cfg.project_vault), Ok(None)) {
            Backend::Headless
        } else {
            self
        }
    }

    pub fn api(self) -> Box<dyn Api> {
        match self {
            Backend::App => Box::new(Cli::default()),
            Backend::Headless => Box::new(crate::headless::Headless),
        }
    }
}

/// The Obsidian CLI command: `OBMEM_OBSIDIAN_BIN`, else `obsidian`.
fn cli_bin() -> String {
    std::env::var("OBMEM_OBSIDIAN_BIN").unwrap_or_else(|_| "obsidian".into())
}

pub struct Cli {
    bin: String,
    timeout: Duration,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            bin: cli_bin(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// An `obsidian://` link that opens a note (vault-relative path) in the vault with this id.
pub fn open_uri(vault_id: &str, rel: &str) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    format!(
        "obsidian://open?vault={}&file={}",
        utf8_percent_encode(vault_id, NON_ALPHANUMERIC),
        utf8_percent_encode(rel, NON_ALPHANUMERIC)
    )
}

/// Obsidian's own vault registry.
pub fn registry_path() -> PathBuf {
    if let Ok(p) = std::env::var("OBMEM_OBSIDIAN_CONFIG") {
        return PathBuf::from(p);
    }
    // macOS ~/Library/Application Support, Linux ~/.config, Windows %APPDATA%
    dirs::config_dir()
        .unwrap_or_else(|| crate::config::home().join(".config"))
        .join("obsidian/obsidian.json")
}

#[derive(Deserialize)]
struct Registry {
    #[serde(default)]
    vaults: std::collections::HashMap<String, RegistryVault>,
}

#[derive(Deserialize)]
struct RegistryVault {
    path: String,
}

/// The Obsidian vault id whose folder is exactly `vault`, if it is registered.
pub fn vault_id(vault: &Path) -> Result<Option<String>> {
    let reg_path = registry_path();
    if !reg_path.exists() {
        return Ok(None);
    }
    let reg: Registry = serde_json::from_str(&std::fs::read_to_string(&reg_path)?)
        .with_context(|| format!("parsing {}", reg_path.display()))?;
    let want = std::fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
    Ok(reg.vaults.into_iter().find_map(|(id, v)| {
        let p = PathBuf::from(&v.path);
        (std::fs::canonicalize(&p).unwrap_or(p) == want).then_some(id)
    }))
}

pub fn not_registered(vault: &Path) -> anyhow::Error {
    anyhow!(
        "{} is not an Obsidian vault yet. One-time setup: in Obsidian choose \"Open folder as vault\" \
         and pick that folder (Obsidian has no way to register vaults from the command line).",
        vault.display()
    )
}

/// Outcome of one CLI invocation.
struct Output {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// The app is not running (the CLI talks to the running app; despite the docs it does not
/// start it, verified on 1.14.5). Only a failed run counts: a successful search result can
/// legitimately contain this very sentence (a note quoting the error).
fn app_missing(o: &Output) -> bool {
    !o.ok
        && (o.stderr.contains("unable to find Obsidian")
            || o.stdout.contains("unable to find Obsidian"))
}

/// Right after launch the app answers before its CLI commands are registered: a single-line
/// `Error: Command "…" not found…`, never a real result.
fn still_starting(o: &Output) -> bool {
    !o.stdout.contains('\n')
        && o.stdout.starts_with("Error: Command \"")
        && o.stdout.contains("not found")
}

/// Start Obsidian in the background without taking focus. Best effort per platform.
fn launch_app() -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").args(["-g", "-a", "Obsidian"]).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "", "obsidian://"])
            .status()
    } else {
        Command::new("xdg-open").arg("obsidian://").status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("could not start Obsidian ({s})"),
        Err(e) => bail!("could not start Obsidian: {e}"),
    }
}

impl Cli {
    fn exec(&self, id: &str, args: &[&str]) -> Result<Output> {
        let mut child = Command::new(&self.bin)
            .arg(format!("vault={id}"))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => anyhow!(
                    "Obsidian CLI (`{}`) not found. Install Obsidian 1.12+ and enable Settings → General → \
                     Command line interface.",
                    self.bin
                ),
                _ => anyhow!("running {}: {e}", self.bin),
            })?;
        // Drain both pipes concurrently: large results would otherwise fill a pipe and block the CLI.
        fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<String> {
            std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_string(&mut s);
                }
                s
            })
        }
        let out_t = drain(child.stdout.take());
        let err_t = drain(child.stderr.take());
        let start = Instant::now();
        let status = loop {
            if let Some(st) = child.try_wait()? {
                break st;
            }
            if start.elapsed() > self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "Obsidian CLI timed out after {}s ({})",
                    self.timeout.as_secs(),
                    args.join(" ")
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok(Output {
            ok: status.success(),
            stdout: out_t.join().unwrap_or_default().trim().to_string(),
            stderr: err_t.join().unwrap_or_default().trim().to_string(),
        })
    }

    fn run(&self, vault: &Path, args: &[&str]) -> Result<String> {
        let id = vault_id(vault)?.ok_or_else(|| not_registered(vault))?;
        let mut o = self.exec(&id, args)?;
        if app_missing(&o) || still_starting(&o) {
            if app_missing(&o) {
                launch_app()?;
            }
            let deadline = Instant::now() + self.timeout;
            while (app_missing(&o) || still_starting(&o)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(500));
                o = self.exec(&id, args)?;
            }
            if app_missing(&o) {
                bail!(
                    "started Obsidian but its CLI did not answer within {}s",
                    self.timeout.as_secs()
                );
            }
        }
        if !o.ok {
            let msg = if o.stderr.is_empty() {
                &o.stdout
            } else {
                &o.stderr
            };
            bail!("Obsidian CLI failed: {msg}");
        }
        let out = o.stdout;
        if out == "Vault not found." {
            bail!(
                "Obsidian does not know vault id {id} for {}; reopen the folder as a vault.",
                vault.display()
            );
        }
        if let Some(e) = out.strip_prefix("Error:") {
            bail!("Obsidian: {}", e.trim());
        }
        Ok(out)
    }
}

/// `No matches found.`, `No backlinks found.`, … : a one-line plain-text empty result.
fn is_empty_result(out: &str) -> bool {
    out.is_empty() || (!out.contains('\n') && out.starts_with("No ") && out.ends_with("found."))
}

pub fn parse_search(out: &str) -> Result<Vec<SearchHit>> {
    if is_empty_result(out) {
        return Ok(vec![]);
    }
    #[derive(Deserialize)]
    struct File {
        file: String,
        #[serde(default)]
        matches: Vec<Match>,
    }
    #[derive(Deserialize)]
    struct Match {
        #[serde(default)]
        line: usize,
        text: String,
    }
    let files: Vec<File> =
        serde_json::from_str(out).context("unexpected `obsidian search:context` output")?;
    Ok(files
        .into_iter()
        .map(|f| {
            let mut lines: Vec<(usize, String)> = Vec::new();
            for m in f.matches {
                let t = (m.line, m.text.trim().to_string());
                if !lines.contains(&t) {
                    lines.push(t);
                }
            }
            SearchHit {
                file: f.file,
                lines,
            }
        })
        .collect())
}

/// `obsidian links`: one target per line, a file by its path and a link that opens nothing as
/// `<link text> (unresolved)`.
pub fn parse_links(out: &str) -> Vec<Link> {
    if is_empty_result(out) {
        return vec![];
    }
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| match l.strip_suffix(" (unresolved)") {
            Some(text) => Link::Unresolved(text.to_string()),
            None => Link::File(l.to_string()),
        })
        .collect()
}

pub fn parse_backlinks(out: &str) -> Result<Vec<String>> {
    if is_empty_result(out) {
        return Ok(vec![]);
    }
    #[derive(Deserialize)]
    struct B {
        file: String,
    }
    let v: Vec<B> = serde_json::from_str(out).context("unexpected `obsidian backlinks` output")?;
    Ok(v.into_iter().map(|b| b.file).collect())
}

pub fn parse_unresolved(out: &str) -> Result<Vec<Unresolved>> {
    if is_empty_result(out) {
        return Ok(vec![]);
    }
    #[derive(Deserialize)]
    struct U {
        link: String,
        #[serde(default)]
        sources: String,
    }
    let v: Vec<U> = serde_json::from_str(out).context("unexpected `obsidian unresolved` output")?;
    Ok(v.into_iter()
        .map(|u| Unresolved {
            link: u.link,
            // one string, ", "-separated
            sources: u
                .sources
                .split(", ")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        })
        .collect())
}

pub fn parse_orphans(out: &str) -> Vec<String> {
    if is_empty_result(out) {
        return vec![];
    }
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

impl Api for Cli {
    fn search(&self, vault: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let q = format!("query={query}");
        let l = format!("limit={limit}");
        parse_search(&self.run(vault, &["search:context", &q, &l, "format=json"])?)
    }

    fn links(&self, vault: &Path, rel: &str) -> Result<Vec<Link>> {
        let p = format!("path={rel}");
        let mut links = parse_links(&self.run(vault, &["links", &p])?);
        // Obsidian lists a note's link to itself
        links.retain(|l| !matches!(l, Link::File(f) if f == rel));
        Ok(links)
    }

    fn backlinks(&self, vault: &Path, rel: &str) -> Result<Vec<String>> {
        let p = format!("path={rel}");
        parse_backlinks(&self.run(vault, &["backlinks", &p, "format=json"])?)
    }

    fn unresolved(&self, vault: &Path) -> Result<Vec<Unresolved>> {
        parse_unresolved(&self.run(vault, &["unresolved", "verbose", "format=json"])?)
    }

    fn orphans(&self, vault: &Path) -> Result<Vec<String>> {
        Ok(parse_orphans(&self.run(vault, &["orphans"])?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Outputs captured from Obsidian 1.14.5.
    #[test]
    fn parses_real_outputs() {
        let s = parse_search(r#"[{"file":"Knowledge/lonely.md","matches":[{"line":6,"text":"token token"},{"line":6,"text":"token token"}]}]"#).unwrap();
        assert_eq!(
            (s[0].file.as_str(), s[0].lines.clone()),
            ("Knowledge/lonely.md", vec![(6, "token token".to_string())])
        );
        assert!(parse_search("No matches found.").unwrap().is_empty());
        let u = parse_unresolved(
            r#"[{"link": "token-重放防護", "count": "2", "sources": "Decisions/jwt-輪替策略.md, Decisions/legacy-token.md"}]"#,
        )
        .unwrap();
        assert_eq!(
            u[0].sources,
            vec!["Decisions/jwt-輪替策略.md", "Decisions/legacy-token.md"]
        );
        assert_eq!(
            parse_backlinks("[\n  {\n    \"file\": \"Decisions/a.md\"\n  }\n]").unwrap(),
            vec!["Decisions/a.md"]
        );
        assert!(parse_backlinks("No backlinks found.").unwrap().is_empty());
        assert_eq!(
            parse_links("Index.md\nno-such-note (unresolved)\n未命名.canvas\n歡迎.md"),
            vec![
                Link::File("Index.md".into()),
                Link::Unresolved("no-such-note".into()),
                Link::File("未命名.canvas".into()),
                Link::File("歡迎.md".into()),
            ]
        );
        assert!(parse_links("No links found.").is_empty());
        assert_eq!(
            parse_orphans("Knowledge/lonely.md\n歡迎.md"),
            vec!["Knowledge/lonely.md", "歡迎.md"]
        );
    }

    #[test]
    fn vault_id_by_canonical_path() {
        let dir = std::env::temp_dir().join(format!("obmem-obs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("v")).unwrap();
        let reg = dir.join("obsidian.json");
        // registered via a non-canonical spelling of the same folder
        let spelled = format!("{}/./v", dir.display());
        std::fs::write(
            &reg,
            format!(r#"{{"vaults":{{"abc123":{{"path":"{spelled}","ts":1}}}}}}"#),
        )
        .unwrap();
        // SAFETY: tests touching this env var run in this single test only
        unsafe { std::env::set_var("OBMEM_OBSIDIAN_CONFIG", &reg) };
        assert_eq!(vault_id(&dir.join("v")).unwrap().as_deref(), Some("abc123"));
        assert_eq!(vault_id(&dir).unwrap(), None);
        unsafe { std::env::remove_var("OBMEM_OBSIDIAN_CONFIG") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_an_installed_command() {
        use crate::config::find_program;
        let exe = std::env::current_exe().unwrap();
        assert_eq!(find_program(&exe.to_string_lossy()), Some(exe.clone()));
        assert!(find_program(&exe.with_extension("missing").to_string_lossy()).is_none());
        assert!(find_program("obmem-no-such-command").is_none());
    }

    #[test]
    fn recognises_startup_states() {
        let o = |out: &str, err: &str| Output {
            ok: false,
            stdout: out.into(),
            stderr: err.into(),
        };
        assert!(app_missing(&o(
            "",
            "The CLI is unable to find Obsidian. Please make sure Obsidian is running and try again."
        )));
        // regression: a successful search whose results quote that error is not "app missing"
        let hit = Output {
            ok: true,
            stdout: r#"[{"file":"Knowledge/obsidian-cli-behaviour.md","matches":[{"line":9,"text":"It prints \"The CLI is unable to find Obsidian\""}]}]"#.into(),
            stderr: String::new(),
        };
        assert!(!app_missing(&hit) && !still_starting(&hit));
        assert!(still_starting(&o(
            "Error: Command \"vault\" not found. Did you mean: daily?",
            ""
        )));
        assert!(!still_starting(&o(
            "Error: File \"nope.md\" not found.",
            ""
        )));
    }
}
