//! Configuration: where the two vaults live and how writes are policed.
//!
//! - Project vault: inside the repo (default `docs/kb`), shared with the team via git. Or
//!   private: a vault outside the repo, which the user's own config names (`[[project]]`).
//! - Global vault:  a personal vault (default `~/Obsidian/AgentMemory`) for preferences,
//!   cross-project feedback and noisy session logs that should not land in the repo.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_CONFIG_FILE: &str = ".obmem.toml";

/// What a project's tag starts with ([`Config::project_tag`]).
pub const PROJECT_TAG: &str = "project/";

/// A tag as it is compared: Obsidian reads tags without regard to case, and a person may write
/// one with its `#`.
pub fn tag_key(tag: &str) -> String {
    tag.trim_start_matches('#').to_lowercase()
}

/// Whether a tag names a project a note holds in.
pub fn is_project_tag(tag: &str) -> bool {
    tag_key(tag).starts_with(PROJECT_TAG)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectFile {
    /// Vault directory, relative to the project root.
    pub vault: Option<String>,
    /// The project's memory is kept outside the repo, and `vault` is not read: the vault is the
    /// one each user's own config names for this project (`[[project]]`). A machine that names
    /// none refuses to load the project, so nothing private is written into the repo.
    pub private: Option<bool>,
    /// Project name used in the global vault (`Projects/<name>/`).
    pub name: Option<String>,
    pub secrets: SecretsConfig,
    pub audit: AuditConfig,
    pub review: ReviewConfig,
}

/// `[review]`: whether agent writes skip the review queue. Only the user's own config can turn
/// auto-approve on; a repo's `.obmem.toml` arrives with a clone and can only turn it off.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ReviewConfig {
    /// Notes are written `active` instead of `draft`, as if a person had approved them.
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    /// Active knowledge notes not verified/updated for this many days are reported as stale.
    pub stale_days: i64,
    /// Title similarity (trigram Jaccard) at or above which two notes count as duplicates.
    pub duplicate_threshold: f64,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            stale_days: 180,
            duplicate_threshold: 0.75,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GlobalFile {
    pub vault: Option<String>,
    pub secrets: GlobalSecrets,
    pub review: ReviewConfig,
    pub sync: SyncConfig,
    /// `[[project]]`: the projects on this machine whose vault is outside their repo.
    #[serde(rename = "project")]
    pub projects: Vec<PrivateProject>,
}

/// `[[project]]`: a project whose memory is private, and where its vault is on this machine.
/// Only the user's config has it: a repo's `.obmem.toml` arrives with a clone and must not be
/// able to send writes anywhere outside the repo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrivateProject {
    /// The project's main checkout ([`main_checkout`]), an absolute path.
    pub root: String,
    /// The vault: an absolute path, or one starting with `~/`.
    pub vault: String,
    /// `[project.sync]`: the bucket the vault is synced through, which the team shares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<Bucket>,
}

/// Where a vault is kept in an S3-compatible bucket: the keys of `[sync]` that say where, without
/// the ones about this machine. A different bucket or prefix is a different backup, with a key of
/// its own.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Bucket {
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
}

/// `[sync]`: where the personal vault is backed up, an S3-compatible bucket (AWS S3, Cloudflare R2,
/// MinIO). Only the user's config has it. Credentials and the vault key live in the OS keychain,
/// or in the environment on a machine without one, never in this file.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct SyncConfig {
    /// S3 API endpoint, e.g. `https://<account>.r2.cloudflarestorage.com`. Unset: AWS S3.
    pub endpoint: Option<String>,
    /// Bucket region. Unset: `auto` (what R2 expects); AWS needs the real region.
    pub region: Option<String>,
    pub bucket: Option<String>,
    /// Key prefix inside the bucket, so one bucket can hold more than one vault. Unset: `obmem`.
    pub prefix: Option<String>,
    /// This machine's name in snapshots, conflict copies and session log names. Connecting a
    /// device sets it (see [`new_device_name`]). Unset: the host name.
    pub device: Option<String>,
    /// Also sync `.obsidian/plugins/` and `community-plugins.json`.
    pub community_plugins: Option<bool>,
    /// Sync in the background when an agent session starts and stops. Unset: on.
    pub auto: Option<bool>,
    /// This machine does not outlive an agent session (a cloud sandbox). Agent hooks then wait
    /// for the sync instead of leaving it in the background, the first sync creates the vault
    /// folder, and the machine never prunes. Unset: off.
    pub ephemeral: Option<bool>,
}

impl SyncConfig {
    /// These settings for another bucket: a private project vault syncs like the personal one
    /// on this machine (its name, automatic sync, ephemeral), to where the team keeps it.
    fn at(&self, bucket: &Bucket) -> SyncConfig {
        SyncConfig {
            endpoint: bucket.endpoint.clone(),
            region: bucket.region.clone(),
            bucket: bucket.bucket.clone(),
            prefix: bucket.prefix.clone(),
            ..self.clone()
        }
    }

    /// A bucket is configured and automatic sync is not turned off.
    pub fn auto_sync(&self) -> bool {
        self.bucket.is_some() && self.auto.unwrap_or(true)
    }

    pub fn is_ephemeral(&self) -> bool {
        self.ephemeral.unwrap_or(false)
    }
}

/// Secret-scanning switches that may only come from the user's own config: a repo's
/// `.obmem.toml` arrives with a clone and must not be able to weaken the scanner.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GlobalSecrets {
    /// Set to false to stop treating sensitive-looking environment variables as known secrets.
    pub scan_process_env: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecretMode {
    /// Refuse the write and tell the agent what to remove (default).
    #[default]
    Block,
    /// Replace the secret with `[REDACTED:<rule>]` and write anyway.
    Redact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretsConfig {
    pub mode: SecretMode,
    /// Files (relative to project root) whose values are treated as known secrets.
    pub env_files: Vec<String>,
    /// Also treat values of sensitive-looking environment variables as known secrets.
    pub scan_process_env: bool,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            mode: SecretMode::Block,
            env_files: vec![
                ".env".into(),
                ".env.local".into(),
                ".env.development".into(),
                ".env.production".into(),
            ],
            scan_process_env: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The folder the project's files are in: `.obmem.toml`, a vault in the repo, the code.
    pub project_root: PathBuf,
    /// The checkout that stands for the project ([`main_checkout`]): `project_root`, or the
    /// repo's main checkout when `project_root` is a linked git worktree.
    pub main_root: PathBuf,
    pub project_name: String,
    /// Inside `project_root`, or a private vault outside it (see [`Config::vault_is_private`]).
    pub project_vault: PathBuf,
    pub global_vault: PathBuf,
    pub secrets: SecretsConfig,
    pub audit: AuditConfig,
    /// Writes land `active` instead of `draft` (see [`auto_approve`]).
    pub auto_approve: bool,
    /// This machine's name (see [`device_name`]).
    pub device: String,
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The file that running `program` would start: a path that is a file, or a name in a `PATH`
/// folder, with the platform's `.exe` added when it is missing, as `Command` does.
pub fn find_program(program: &str) -> Option<PathBuf> {
    let with_suffix = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    let in_dir = |dir: &Path| {
        [program, with_suffix.as_str()]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|file| file.is_file())
    };
    if Path::new(program).components().count() > 1 {
        return in_dir(Path::new(""));
    }
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| in_dir(&dir))
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else if p == "~" {
        home()
    } else {
        PathBuf::from(p)
    }
}

/// `~/.obmem/config.toml`: one folder in the home directory on every OS, for the user's config
/// and everything obmem keeps beside it (the project list, the sync state, the desktop app's
/// themes). A standalone install has its binary there too, in `bin/`.
pub fn global_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("OBMEM_CONFIG") {
        return PathBuf::from(p);
    }
    home().join(".obmem/config.toml")
}

/// Walk up from `start` until a `.obmem.toml` or `.git` is found.
pub fn find_project_root(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(PROJECT_CONFIG_FILE).exists() || dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    start.to_path_buf()
}

/// The checkout that stands for the whole project: `root`, or the repo's main checkout when
/// `root` is in a linked git worktree (`git worktree add`). What obmem keeps about a project
/// outside its folder is kept under this path (the user's `[[project]]`, the list of projects,
/// the session logs), so every worktree of a repo is the same project. A function that takes a
/// project's root and reads one of those calls this itself.
///
/// A folder says it is a worktree with one line in its `.git` file, and anyone can write that
/// line. So the repo is asked: a folder is a worktree only when the repo's own list has it.
/// Otherwise a downloaded folder could name the repo of a private project and be given its vault.
pub fn main_checkout(root: &Path) -> PathBuf {
    main_of_linked_worktree(root).unwrap_or_else(|| root.to_path_buf())
}

fn main_of_linked_worktree(root: &Path) -> Option<PathBuf> {
    let root = std::path::absolute(root).ok()?;
    // The project can be a folder inside the repo. A checkout has a `.git` folder, and only a
    // linked worktree (or a submodule) a `.git` file, so git runs for those alone.
    let top = root.ancestors().find(|dir| dir.join(".git").exists())?;
    if !top.join(".git").is_file() {
        return None;
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut listed = text
        .split('\0')
        .filter_map(|line| line.strip_prefix("worktree "));
    // The first is the main checkout (of a bare repository, the repository itself).
    let main = std::path::absolute(listed.next()?).ok()?;
    let here = resolved(top);
    if resolved(&main) == here || !listed.any(|w| resolved(Path::new(w)) == here) {
        return None;
    }
    // Joining an empty path would end the root with a separator, and a session log's header
    // compares roots as text.
    Some(match root.strip_prefix(top) {
        Ok(inside) if inside.as_os_str().is_empty() => main,
        Ok(inside) => main.join(inside),
        Err(_) => return None,
    })
}

/// The project a command is about: found from `project_override`, else `OBMEM_PROJECT_ROOT`,
/// else the current directory.
pub fn project_root(project_override: Option<&Path>) -> Result<PathBuf> {
    let start = match project_override {
        Some(p) => p.to_path_buf(),
        None => match std::env::var("OBMEM_PROJECT_ROOT") {
            Ok(p) => PathBuf::from(p),
            Err(_) => std::env::current_dir()?,
        },
    };
    Ok(find_project_root(&start))
}

impl Config {
    pub fn load(project_override: Option<&Path>) -> Result<Self> {
        let project_root = project_root(project_override)?;
        let pf: ProjectFile =
            read_toml(&project_root.join(PROJECT_CONFIG_FILE))?.unwrap_or_default();
        let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();
        let main_root = main_checkout(&project_root);
        Self::resolve(project_root, main_root, pf, gf)
    }

    /// What the project's file and the user's file come to, for the project at `project_root`
    /// whose main checkout is `main_root`.
    fn resolve(
        project_root: PathBuf,
        main_root: PathBuf,
        pf: ProjectFile,
        gf: GlobalFile,
    ) -> Result<Self> {
        // A worktree's folder is named after a branch or a task, not after the project.
        let project_name = pf.name.clone().unwrap_or_else(|| {
            main_root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "default".into())
        });
        let global_vault = global_vault_from(gf.vault);
        let private = private_vault(&gf.projects, &main_root, &project_root, &global_vault)?;
        let project_vault = match private {
            Some(vault) => vault,
            None if pf.private.unwrap_or(false) => {
                return Err(NoVaultHere {
                    project_file: project_root.join(PROJECT_CONFIG_FILE),
                }
                .into());
            }
            None => {
                project_vault_path(&project_root, pf.vault.as_deref().unwrap_or(DEFAULT_VAULT))?
            }
        };

        let secrets = secrets_policy(&project_root, pf.secrets, &gf.secrets)?;
        let auto_approve = auto_approve(&pf.review, &gf.review);
        let device = device_name(&gf.sync);
        Ok(Self {
            project_root,
            main_root,
            project_name,
            project_vault,
            global_vault,
            secrets,
            audit: pf.audit,
            auto_approve,
            device,
        })
    }

    /// The project vault is outside the repo: the user's config names it, git does not carry it.
    /// A private vault is never inside the project ([`private_vault_path`]), and a repo's own
    /// vault always is, so the paths say which one this is.
    pub fn vault_is_private(&self) -> bool {
        !self.project_vault.starts_with(&self.project_root)
    }

    /// The tag of what is this project's in the global vault: its session logs, and the feedback
    /// that holds only here (see `Catalog::global_guidance`).
    pub fn project_tag(&self) -> String {
        format!("{PROJECT_TAG}{}", crate::note::slugify(&self.project_name))
    }

    /// Session logs etc. for this project inside the global vault.
    pub fn global_project_dir(&self) -> PathBuf {
        // The name comes from repo config: keep it a single, harmless path component.
        self.global_vault
            .join("Projects")
            .join(sanitize(&self.project_name))
    }
}

/// Repo config can only make secret scanning stricter: its `env_files` are added to the defaults
/// (and must be plain relative paths inside the project), and it can enable but not disable
/// process-environment scanning. `mode` may switch to `redact`, which still never stores a secret.
fn secrets_policy(
    project_root: &Path,
    repo: SecretsConfig,
    user: &GlobalSecrets,
) -> Result<SecretsConfig> {
    let mut env_files = SecretsConfig::default().env_files;
    for f in repo.env_files {
        if !is_plain_relative(&f) {
            anyhow::bail!(
                "{}: secrets.env_files entry \"{f}\" must be a relative path inside the project",
                project_root.join(PROJECT_CONFIG_FILE).display()
            );
        }
        if !env_files.contains(&f) {
            env_files.push(f);
        }
    }
    Ok(SecretsConfig {
        mode: repo.mode,
        env_files,
        scan_process_env: repo.scan_process_env || user.scan_process_env.unwrap_or(true),
    })
}

/// Auto-approve is on only when the user's config turns it on and the repo's does not turn it off:
/// a clone must not be able to skip the review of what agents write, in the personal vault least of all.
fn auto_approve(repo: &ReviewConfig, user: &ReviewConfig) -> bool {
    user.auto_approve.unwrap_or(false) && repo.auto_approve.unwrap_or(true)
}

/// The personal vault: `OBMEM_GLOBAL_VAULT`, else the user's config, else `~/Obsidian/AgentMemory`.
pub fn global_vault() -> Result<PathBuf> {
    let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();
    Ok(global_vault_from(gf.vault))
}

/// This machine's name, which keeps what two devices write to the synced personal vault apart:
/// `[sync] device`, else the host name without its domain. Connecting a device to a bucket sets
/// `[sync] device` (see [`new_device_name`]), because two machines can share a host name.
pub fn device_name(sync: &SyncConfig) -> String {
    sync.device
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(host_name)
}

/// A name for a device joining sync: the host name and four random hex digits, so that two
/// laptops that both call themselves `MacBook-Pro` still write different session logs.
pub fn new_device_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!("{}-{:04x}", host_name(), h.finish() & 0xffff)
}

fn host_name() -> String {
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    let host = host.split('.').next().unwrap_or_default();
    if host.is_empty() {
        "device".into()
    } else {
        host.into()
    }
}

/// The user's `[sync]` settings.
pub fn sync_config() -> Result<SyncConfig> {
    sync_config_at(&global_config_path())
}

pub(crate) fn sync_config_at(path: &Path) -> Result<SyncConfig> {
    let gf: GlobalFile = read_toml(path)?.unwrap_or_default();
    Ok(gf.sync)
}

/// How the private vault of the project at `root` is synced: through its own bucket
/// (`[project.sync]`), with this machine's `[sync]` settings for the rest. `None`: the user's
/// config names no bucket for it.
pub fn project_sync(root: &Path) -> Result<Option<SyncConfig>> {
    let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();
    Ok(project_sync_in(&gf, &main_checkout(root)))
}

/// How many projects the user's config names a bucket for.
pub fn synced_projects() -> Result<usize> {
    let gf: GlobalFile = read_toml(&global_config_path())?.unwrap_or_default();
    Ok(gf
        .projects
        .iter()
        .filter(|p| p.sync.as_ref().is_some_and(|b| b.bucket.is_some()))
        .count())
}

fn project_sync_in(gf: &GlobalFile, root: &Path) -> Option<SyncConfig> {
    let real_root = resolved(root);
    gf.projects
        .iter()
        .find(|p| resolved(Path::new(&p.root)) == real_root)
        .and_then(|p| p.sync.as_ref())
        .filter(|bucket| bucket.bucket.is_some())
        .map(|bucket| gf.sync.at(bucket))
}

pub(crate) fn global_vault_from(configured: Option<String>) -> PathBuf {
    std::env::var("OBMEM_GLOBAL_VAULT")
        .ok()
        .or(configured)
        .map(|s| expand_tilde(&s))
        .unwrap_or_else(|| home().join("Obsidian/AgentMemory"))
}

/// The project vault when `.obmem.toml` sets none.
pub const DEFAULT_VAULT: &str = "docs/kb";

/// A path relative to the project that stays inside it: no `..`, `~` or absolute paths.
pub fn is_plain_relative(p: &str) -> bool {
    let path = Path::new(p);
    !p.is_empty()
        && !path.is_absolute()
        && !p.starts_with('~')
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// The project vault for `.obmem.toml`'s `vault`. The file ships with the repo, so a clone must
/// not be able to point writes elsewhere: the path must be plain relative, and must not leave the
/// project through a symlink either.
pub fn project_vault_path(project_root: &Path, vault: &str) -> Result<PathBuf> {
    if !is_plain_relative(vault) {
        anyhow::bail!(
            "{}: `vault = \"{vault}\"` must be a relative path inside the project (no `..`, `~` or absolute paths)",
            project_root.join(PROJECT_CONFIG_FILE).display()
        );
    }
    let project_vault = project_root.join(vault);
    ensure_within(project_root, &project_vault)?;
    Ok(project_vault)
}

/// The private vault the user's config names for the project whose main checkout is `main`, when
/// it names one. `worktree` is the folder the project is used from.
fn private_vault(
    projects: &[PrivateProject],
    main: &Path,
    worktree: &Path,
    personal: &Path,
) -> Result<Option<PathBuf>> {
    let real_root = resolved(main);
    projects
        .iter()
        .find(|p| resolved(Path::new(&p.root)) == real_root)
        .map(|p| {
            // Outside the worktree too: the paths say which kind of vault a project has
            // (`Config::vault_is_private`).
            private_vault_path(worktree, &p.vault, personal)?;
            private_vault_path(main, &p.vault, personal)
        })
        .transpose()
}

/// The error of loading a project whose memory is private on a machine whose config names no
/// vault for it. A front end that can ask for the folder finds it by its type.
#[derive(Debug)]
pub struct NoVaultHere {
    /// The `.obmem.toml` that says the vault is private.
    pub project_file: PathBuf,
}

impl std::fmt::Display for NoVaultHere {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: this project keeps its memory in a private vault outside the repo, and this \
             machine has none for it. Run `obmem init --private <folder>` here to say where it is.",
            self.project_file.display()
        )
    }
}

impl std::error::Error for NoVaultHere {}

/// Why a folder cannot be a project's private vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Misplaced {
    /// Not an absolute path or one under `~/`, or it has `..` in it.
    NotAbsolute,
    InsideProject,
    /// It is the personal vault, in it, or around it.
    WithPersonalVault,
}

/// The private vault of the project at `root`, for `vault` as the user's config states it. It is
/// a folder of its own: outside the project, where git would carry it, and apart from the
/// personal vault, whose sync and whose Obsidian vault would otherwise take it in.
pub fn private_vault_place(
    root: &Path,
    vault: &str,
    personal: &Path,
) -> std::result::Result<PathBuf, Misplaced> {
    let path = expand_tilde(vault.trim());
    let plain = path
        .components()
        .all(|c| !matches!(c, std::path::Component::ParentDir));
    if !path.is_absolute() || !plain {
        return Err(Misplaced::NotAbsolute);
    }
    let (real, real_root, real_personal) = (resolved(&path), resolved(root), resolved(personal));
    if path.starts_with(root) || real.starts_with(&real_root) {
        return Err(Misplaced::InsideProject);
    }
    if real.starts_with(&real_personal) || real_personal.starts_with(&real) {
        return Err(Misplaced::WithPersonalVault);
    }
    Ok(path)
}

/// [`private_vault_place`], with an error that names the folders.
pub fn private_vault_path(root: &Path, vault: &str, personal: &Path) -> Result<PathBuf> {
    private_vault_place(root, vault, personal).map_err(|why| match why {
        Misplaced::NotAbsolute => anyhow::anyhow!(
            "the private vault \"{vault}\" must be an absolute path or start with ~/, without `..`"
        ),
        Misplaced::InsideProject => anyhow::anyhow!(
            "the private vault {} is inside the project {}: a private vault is kept outside the repo",
            vault.trim(),
            root.display()
        ),
        Misplaced::WithPersonalVault => anyhow::anyhow!(
            "the private vault {} and the personal vault {} must be separate folders",
            vault.trim(),
            personal.display()
        ),
    })
}

/// `p` with symlinks resolved as far as it exists; what does not exist yet stays as written.
/// Two paths to one folder compare equal this way, whether or not the folder is there.
pub(crate) fn resolved(p: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(real) = std::fs::canonicalize(cur) {
            return missing
                .iter()
                .rev()
                .fold(real, |path, name| path.join(name));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// The deepest existing ancestor of `p` (or `p` itself), with symlinks resolved.
fn real_existing(p: &Path) -> Result<PathBuf> {
    let mut cur = p;
    loop {
        if cur.exists() {
            return Ok(std::fs::canonicalize(cur)?);
        }
        cur = cur
            .parent()
            .ok_or_else(|| anyhow::anyhow!("no existing ancestor of {}", p.display()))?;
    }
}

/// Refuse paths that leave `root` once symlinks are resolved (checks what exists so far, so it
/// can run before any directory is created).
pub fn ensure_within(root: &Path, p: &Path) -> Result<()> {
    if !p.starts_with(root) {
        anyhow::bail!("refusing {}: not inside {}", p.display(), root.display());
    }
    let real_root = real_existing(root)?;
    let real = real_existing(p)?;
    if !real.starts_with(&real_root) {
        anyhow::bail!(
            "refusing {}: it resolves to {} (outside {}) through a symlink",
            p.display(),
            real.display(),
            real_root.display()
        );
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "default".into() } else { s }
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let v = toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(v))
}

/// A repo with one commit and a linked worktree of it, for tests: (main checkout, worktree), as
/// the paths git prints. They are `main` and `linked` in one folder, which the test removes.
#[cfg(test)]
pub(crate) fn linked_worktree(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("obmem-worktree-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("main")).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.join("main"))
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&["worktree", "add", "-q", "../linked"]);
    (dir.join("main"), dir.join("linked"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_with(toml: &str) -> Result<Config> {
        let dir = std::env::temp_dir().join(format!(
            "obmem-cfg-{}-{}",
            std::process::id(),
            sanitize(toml)
        ));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(PROJECT_CONFIG_FILE), toml)?;
        let r = Config::load(Some(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    #[test]
    fn repo_config_cannot_weaken_secret_scanning() {
        let c =
            load_with("[secrets]\nenv_files = [\"config/.env.test\"]\nscan_process_env = false\n")
                .unwrap();
        assert!(
            c.secrets.scan_process_env,
            "repo cannot turn process-env scanning off"
        );
        assert!(
            c.secrets.env_files.contains(&".env".to_string()),
            "defaults stay"
        );
        assert!(
            c.secrets
                .env_files
                .contains(&"config/.env.test".to_string()),
            "additions allowed"
        );
        for bad in ["/etc/passwd", "../../.aws/credentials", "~/.ssh/id_rsa"] {
            assert!(
                load_with(&format!("[secrets]\nenv_files = [\"{bad}\"]\n")).is_err(),
                "{bad}"
            );
        }
        let empty = load_with("[secrets]\nenv_files = []\n").unwrap();
        assert_eq!(empty.secrets.env_files, SecretsConfig::default().env_files);
    }

    #[test]
    fn only_the_user_turns_auto_approve_on() {
        let on = ReviewConfig {
            auto_approve: Some(true),
        };
        let off = ReviewConfig {
            auto_approve: Some(false),
        };
        let unset = ReviewConfig::default();
        assert!(!auto_approve(&unset, &unset), "off by default");
        assert!(!auto_approve(&on, &unset), "a repo cannot turn it on");
        assert!(!auto_approve(&on, &off));
        assert!(auto_approve(&unset, &on));
        assert!(auto_approve(&on, &on));
        assert!(!auto_approve(&off, &on), "a repo can turn it off");
    }

    #[test]
    fn repo_config_cannot_escape() {
        for bad in [
            "vault = \"../../etc\"",
            "vault = \"/tmp/x\"",
            "vault = \"~/x\"",
            "vault = \"docs/../../x\"",
        ] {
            assert!(load_with(bad).is_err(), "{bad} should be rejected");
        }
        // a symlinked vault folder that points outside the repo
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("obmem-cfg-link-{}", std::process::id()));
            let outside =
                std::env::temp_dir().join(format!("obmem-cfg-outside-{}", std::process::id()));
            std::fs::create_dir_all(dir.join("docs")).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, dir.join("docs/kb")).unwrap();
            let err = Config::load(Some(&dir)).unwrap_err().to_string();
            assert!(err.contains("symlink"), "{err}");
            assert!(ensure_within(&dir, &dir.join("docs/kb/Decisions/x.md")).is_err());
            assert!(ensure_within(&dir, &dir.join("ok/new/dir/x.md")).is_ok());
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&outside);
        }

        let c = load_with("vault = \"./docs/kb\"\nname = \"../../evil\"").unwrap();
        let dir = c.global_project_dir();
        assert_eq!(dir.parent().unwrap(), c.global_vault.join("Projects"));
        assert!(!dir.file_name().unwrap().to_string_lossy().contains(".."));
    }

    /// A project root, a folder beside it for a private vault, and a personal vault.
    fn private_setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("obmem-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        (dir.join("repo"), dir.join("memory"), dir.join("personal"))
    }

    fn user_file(root: &Path, vault: &Path, personal: &Path) -> GlobalFile {
        GlobalFile {
            vault: Some(personal.display().to_string()),
            projects: vec![PrivateProject {
                root: root.display().to_string(),
                vault: vault.display().to_string(),
                sync: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_users_config_names_a_private_vault() {
        let (root, vault, personal) = private_setup("private");
        let repo = ProjectFile {
            vault: Some("docs/kb".into()),
            ..Default::default()
        };
        let c = Config::resolve(
            root.clone(),
            root.clone(),
            repo,
            user_file(&root, &vault, &personal),
        )
        .unwrap();
        assert_eq!(c.project_vault, vault, "the user's choice beats the repo's");
        assert!(c.vault_is_private());

        // Another project on the same machine keeps its vault in its repo.
        let other = root.with_file_name("other");
        let c = Config::resolve(
            other.clone(),
            other.clone(),
            ProjectFile::default(),
            user_file(&root, &vault, &personal),
        )
        .unwrap();
        assert_eq!(c.project_vault, other.join(DEFAULT_VAULT));
        assert!(!c.vault_is_private());
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn a_linked_worktree_is_its_main_checkout() {
        let (main, linked) = linked_worktree("main");
        let same = |found: PathBuf, want: &Path| assert_eq!(resolved(&found), resolved(want));
        same(main_checkout(&main), &main);
        same(main_checkout(&linked), &main);
        assert_eq!(
            main_checkout(&linked).display().to_string(),
            std::path::absolute(&main).unwrap().display().to_string(),
            "as the main checkout writes its own root in a session log"
        );
        // A project in a folder of the repo.
        std::fs::create_dir_all(linked.join("apps/web")).unwrap();
        same(
            main_checkout(&linked.join("apps/web")),
            &main.join("apps/web"),
        );
        // A folder that names the repo in its `.git` file, and is not one of its worktrees.
        for (name, gitdir) in [
            ("copy", main.join(".git")),
            ("claim", main.join(".git/worktrees/linked")),
        ] {
            let forged = main.with_file_name(name);
            std::fs::create_dir_all(&forged).unwrap();
            std::fs::write(
                forged.join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();
            same(main_checkout(&forged), &forged);
        }
        let plain = main.with_file_name("plain");
        assert_eq!(main_checkout(&plain), plain, "no repo at all");
        let _ = std::fs::remove_dir_all(main.parent().unwrap());
    }

    #[test]
    fn a_worktree_is_the_project_of_its_main_checkout() {
        let (main, linked) = linked_worktree("project");
        let (vault, personal) = (
            main.with_file_name("memory"),
            main.with_file_name("personal"),
        );
        // The worktree has no `.obmem.toml`: the file is not committed, or the branch is older.
        let load = |root: &Path, user: GlobalFile| {
            let main_root = main_checkout(root);
            Config::resolve(root.to_path_buf(), main_root, ProjectFile::default(), user)
        };
        let c = load(&linked, user_file(&main, &vault, &personal)).unwrap();
        assert_eq!(c.project_vault, vault, "not the worktree's docs/kb");
        assert!(c.vault_is_private());
        assert_eq!(c.project_name, "main", "not the worktree's folder name");
        assert_eq!(c.project_root, linked, "the code is still the worktree's");
        // A project whose vault is in the repo keeps the worktree's copy: it is the branch's.
        let c = load(&linked, GlobalFile::default()).unwrap();
        assert_eq!(c.project_vault, linked.join(DEFAULT_VAULT));
        // A private vault is outside the worktree as well.
        assert!(load(&linked, user_file(&main, &linked.join("kb"), &personal)).is_err());
        // A folder that only claims the repo is a project of its own.
        let forged = main.with_file_name("forged");
        std::fs::create_dir_all(&forged).unwrap();
        std::fs::write(
            forged.join(".git"),
            format!("gitdir: {}\n", main.join(".git").display()),
        )
        .unwrap();
        let c = load(&forged, user_file(&main, &vault, &personal)).unwrap();
        assert_eq!(c.project_vault, forged.join(DEFAULT_VAULT));
        let _ = std::fs::remove_dir_all(main.parent().unwrap());
    }

    #[test]
    fn a_private_vault_syncs_through_its_own_bucket() {
        let (root, vault, personal) = private_setup("bucket");
        let text = format!(
            "[sync]\nbucket = \"mine\"\ndevice = \"work-mac\"\nauto = false\n\n\
             [[project]]\nroot = '{}'\nvault = '{}'\n\n\
             [project.sync]\nendpoint = \"https://team.example\"\nbucket = \"team\"\nprefix = \"acme\"\n\n\
             [[project]]\nroot = '{}'\nvault = '{}'\n",
            root.display(),
            vault.display(),
            root.with_file_name("other").display(),
            personal.with_file_name("other-memory").display(),
        );
        let gf: GlobalFile = toml::from_str(&text).unwrap();
        let sync = project_sync_in(&gf, &root).unwrap();
        assert_eq!(
            (
                sync.endpoint.as_deref(),
                sync.bucket.as_deref(),
                sync.prefix.as_deref()
            ),
            (Some("https://team.example"), Some("team"), Some("acme")),
            "the team's bucket"
        );
        assert_eq!(
            sync.device.as_deref(),
            Some("work-mac"),
            "this machine's name"
        );
        assert!(!sync.auto_sync(), "and its choice about automatic sync");
        // A private vault without a bucket is not synced, and neither is a vault in the repo.
        assert_eq!(project_sync_in(&gf, &root.with_file_name("other")), None);
        assert_eq!(project_sync_in(&gf, &root.with_file_name("third")), None);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn a_private_project_without_a_vault_here_does_not_load() {
        let (root, _, _) = private_setup("unlinked");
        let repo = ProjectFile {
            private: Some(true),
            // What a clone could add to get notes written into the repo after all.
            vault: Some("docs/kb".into()),
            ..Default::default()
        };
        let err =
            Config::resolve(root.clone(), root.clone(), repo, GlobalFile::default()).unwrap_err();
        assert!(err.to_string().contains("obmem init --private"), "{err}");
        // A front end that can ask for the folder tells this failure from any other.
        assert!(err.downcast_ref::<NoVaultHere>().is_some());
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn a_private_vault_is_a_folder_of_its_own() {
        let (root, vault, personal) = private_setup("apart");
        assert_eq!(
            private_vault_path(&root, &vault.display().to_string(), &personal).unwrap(),
            vault
        );
        let refused = |v: &Path| private_vault_path(&root, &v.display().to_string(), &personal);
        assert!(refused(Path::new("memory")).is_err(), "relative");
        let place = |v: &Path| private_vault_place(&root, &v.display().to_string(), &personal);
        assert_eq!(place(Path::new("memory")), Err(Misplaced::NotAbsolute));
        assert_eq!(place(&root.join("docs/kb")), Err(Misplaced::InsideProject));
        assert_eq!(place(&personal), Err(Misplaced::WithPersonalVault));
        assert!(refused(&root.join("docs/kb")).is_err(), "inside the repo");
        assert!(refused(&root).is_err(), "the repo itself");
        assert!(
            refused(&root.join("../repo/kb")).is_err(),
            "inside the repo, by `..`"
        );
        assert!(refused(&personal).is_err(), "the personal vault");
        assert!(refused(&personal.join("Projects/x")).is_err(), "inside it");
        assert!(refused(personal.parent().unwrap()).is_err(), "around it");
        // A link beside the repo that leads back into it.
        #[cfg(unix)]
        {
            let link = root.with_file_name("link");
            std::os::unix::fs::symlink(&root, &link).unwrap();
            assert!(
                refused(&link.join("kb")).is_err(),
                "inside the repo, by a link"
            );
        }
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }
}
