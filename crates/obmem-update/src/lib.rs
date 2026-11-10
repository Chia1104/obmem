//! How this `obmem` was installed, what the newest version is, and the update of an install
//! that no package manager owns. The API is blocking: the HTTP client runs on a runtime of its
//! own, as in obmem-sync.

use anyhow::{Context, Result, anyhow, bail};
use semver::Version;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The name in `npm/obmem/package.json`.
pub const NPM_PACKAGE: &str = "@chia1104/obmem";
/// `https://github.com/<owner>/<name>`.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// How this binary got onto the machine. Whoever put it there owns the file and updates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// By npm or another Node package manager: the binary is inside a `node_modules` folder.
    Npm,
    /// Built from a clone: in cargo's bin folder, or in a build's `target` folder.
    Source,
    /// By the install script, or copied by hand. obmem replaces this one itself.
    Standalone,
}

impl Install {
    /// The running binary, with links resolved, and how it was installed.
    pub fn current() -> Result<(PathBuf, Self)> {
        let exe = std::env::current_exe().context("finding the running obmem")?;
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        let cargo_bin = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| obmem_core::config::home().join(".cargo"))
            .join("bin");
        let install = Self::of(&exe, &cargo_bin);
        Ok((exe, install))
    }

    /// Decided from where the binary is: nothing is recorded when it is installed.
    pub fn of(exe: &Path, cargo_bin: &Path) -> Self {
        let has = |name: &str| exe.components().any(|c| c.as_os_str() == name);
        if has("node_modules") {
            Install::Npm
        } else if exe.parent() == Some(cargo_bin) || has("target") {
            Install::Source
        } else {
            Install::Standalone
        }
    }

    /// For `obmem status`.
    pub fn label(self) -> &'static str {
        match self {
            Install::Npm => "installed with npm",
            Install::Source => "built from source",
            Install::Standalone => "standalone install",
        }
    }

    /// The command that updates an install obmem does not replace itself.
    pub fn update_command(self) -> Option<String> {
        match self {
            Install::Npm => Some(format!("npm install -g {NPM_PACKAGE}@latest")),
            Install::Source => Some("cargo install --path crates/obmem".into()),
            Install::Standalone => None,
        }
    }
}

/// The version of the running binary.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the workspace version is a semver version")
}

/// The newest version this install can get. An npm install asks npm, because a release is on
/// GitHub minutes before npm serves it; every other install asks GitHub.
pub fn latest(install: Install) -> Result<Version> {
    block_on(async {
        match install {
            Install::Npm => {
                let url = format!("https://registry.npmjs.org/{NPM_PACKAGE}/latest");
                let body = get(&url).await?.text().await?;
                npm_version(&body).with_context(|| format!("reading {url}"))
            }
            Install::Source | Install::Standalone => {
                // GitHub answers with a redirect to the latest release's own page.
                let url = format!("{REPOSITORY}/releases/latest");
                let page = get(&url).await?;
                release_version(page.url().path())
                    .with_context(|| format!("{url} led to {}", page.url()))
            }
        }
    })
}

/// Replace the standalone binary `exe` with `version`, by running the install script that
/// release carries: it picks the archive for this machine, checks it against the release's
/// SHA256SUMS and swaps the file. The script reports what it did on stdout.
pub fn install(exe: &Path, version: &Version) -> Result<()> {
    let name = format!("obmem{}", std::env::consts::EXE_SUFFIX);
    let dir = match (exe.file_name(), exe.parent()) {
        (Some(file), Some(dir)) if file == name.as_str() => dir,
        _ => bail!(
            "{} is not named {name}: the install script would put a second binary next to it",
            exe.display()
        ),
    };
    let (script, shell, args): (&str, &str, &[&str]) = if cfg!(windows) {
        (
            "install.ps1",
            "powershell",
            &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"],
        )
    } else {
        ("install.sh", "sh", &[])
    };
    let url = format!("{REPOSITORY}/releases/download/v{version}/{script}");
    let text = block_on(async { Ok(get(&url).await?.text().await?) })?;

    let mut file = tempfile::Builder::new()
        .prefix("obmem-")
        .suffix(script)
        .tempfile()
        .context("creating a temp file for the install script")?;
    file.write_all(text.as_bytes())?;
    // Closed, so that the shell can read it on Windows; removed when `path` is dropped.
    let path = file.into_temp_path();
    let status = Command::new(shell)
        .args(args)
        .arg(&path)
        .env("OBMEM_VERSION", version.to_string())
        .env("OBMEM_INSTALL_DIR", dir)
        .status()
        .with_context(|| format!("running the install script with {shell}"))?;
    if !status.success() {
        bail!("the install script of obmem {version} failed ({status}); nothing was replaced");
    }
    Ok(())
}

fn block_on<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(work)
}

async fn get(url: &str) -> Result<reqwest::Response> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("obmem/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;
    response
        .error_for_status()
        .with_context(|| format!("asking {url}"))
}

/// `version` of the document npm serves for a package's `latest` tag.
fn npm_version(body: &str) -> Result<Version> {
    let doc: serde_json::Value = serde_json::from_str(body)?;
    let version = doc["version"]
        .as_str()
        .ok_or_else(|| anyhow!("no `version` in npm's answer"))?;
    Ok(Version::parse(version)?)
}

/// The version in the path of a release's page, `/<owner>/<name>/releases/tag/v<version>`.
fn release_version(path: &str) -> Result<Version> {
    let tag = path
        .rsplit_once("/releases/tag/")
        .map(|(_, tag)| tag)
        .ok_or_else(|| anyhow!("not a release page"))?;
    Ok(Version::parse(tag.strip_prefix('v').unwrap_or(tag))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_read_from_the_path() {
        let cargo_bin = Path::new("/home/me/.cargo/bin");
        let of = |exe: &str| Install::of(Path::new(exe), cargo_bin);
        assert_eq!(
            of(
                "/usr/lib/node_modules/@chia1104/obmem/node_modules/@chia1104/obmem-linux-x64/obmem"
            ),
            Install::Npm
        );
        assert_eq!(of("/home/me/.cargo/bin/obmem"), Install::Source);
        assert_eq!(of("/home/me/obmem/target/debug/obmem"), Install::Source);
        assert_eq!(
            of("/home/me/obmem/target/aarch64-apple-darwin/dist/obmem"),
            Install::Source
        );
        assert_eq!(of("/home/me/.obmem/bin/obmem"), Install::Standalone);
        assert_eq!(of("/usr/local/bin/obmem"), Install::Standalone);
        // a folder that only has cargo's bin folder as a prefix is not it
        assert_eq!(of("/home/me/.cargo/bin/tools/obmem"), Install::Standalone);

        assert!(Install::Standalone.update_command().is_none());
        assert!(
            Install::Npm
                .update_command()
                .unwrap()
                .contains("@chia1104/obmem@latest")
        );
    }

    #[test]
    fn versions_are_read_from_npm_and_from_a_release_page() {
        let npm = r#"{"name":"@chia1104/obmem","version":"0.3.1","bin":{"obmem":"bin/obmem.js"}}"#;
        assert_eq!(npm_version(npm).unwrap(), Version::new(0, 3, 1));
        assert!(npm_version(r#"{"error":"Not found"}"#).is_err());

        assert_eq!(
            release_version("/Chia1104/obmem/releases/tag/v0.2.0").unwrap(),
            Version::new(0, 2, 0)
        );
        // a repository without releases stays on the list page
        assert!(release_version("/Chia1104/obmem/releases").is_err());
        assert!(release_version("/Chia1104/obmem/releases/tag/nightly").is_err());
        assert!(Version::new(0, 10, 0) > Version::new(0, 9, 5));
    }

    #[test]
    fn the_npm_package_is_the_one_the_release_publishes() {
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../npm/obmem/package.json");
        let package: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
        assert_eq!(package["name"], NPM_PACKAGE);
        assert!(
            REPOSITORY.starts_with("https://github.com/"),
            "{REPOSITORY}"
        );
    }

    #[test]
    fn a_renamed_binary_is_not_replaced() {
        let err = install(Path::new("/opt/tools/obmem-old"), &Version::new(9, 9, 9)).unwrap_err();
        assert!(err.to_string().contains("is not named obmem"), "{err}");
    }
}
