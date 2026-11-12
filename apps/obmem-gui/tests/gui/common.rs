//! A throwaway world for the window: obmem's config, personal vault and projects in a temp
//! folder (OBMEM_CONFIG points obmem there), and the app in a desktop-sized window.

use nsh_core::platform::{Languages, Locale};
use nsh_core::prelude::{Points, Size};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

pub const SLOW: Duration = Duration::from_secs(20);

/// Set in a child process that runs one test.
const CHILD: &str = "OBMEM_GUI_TEST_CHILD";

/// Children run one after another, as separate test binaries used to.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The tests set process-wide environment (`OBMEM_CONFIG`, `OBMEM_OBSIDIAN_BIN`, and `HOME` for
/// the Claude Code import), so each one runs in a process of its own. All tests share one binary, which links
/// Native Shell once.
///
/// In the test harness's process this runs the calling test in a child process of the same
/// binary, checks it passed and returns `None`: the caller returns. In the child it sets up the
/// sandbox and returns its folder. Setting the environment there is safe: the child runs only
/// this test, and the app has not started yet.
pub fn sandbox(name: &str) -> Option<PathBuf> {
    if std::env::var_os(CHILD).is_none() {
        in_child();
        return None;
    }
    let dir = std::env::temp_dir().join(format!("obmem-gui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/config.toml"),
        format!("vault = {:?}\n", dir.join("personal").display().to_string()),
    )
    .unwrap();
    // SAFETY: see above.
    unsafe { std::env::set_var("OBMEM_CONFIG", dir.join("config/config.toml")) };
    // The tests expect the Obsidian backend on every machine, with or without Obsidian: any
    // existing program stands for the CLI, and it never runs, because the sandbox's vaults are
    // not registered in Obsidian.
    // SAFETY: see above.
    unsafe { std::env::set_var("OBMEM_OBSIDIAN_BIN", std::env::current_exe().unwrap()) };
    Some(dir)
}

/// Runs the current test again in a child process and fails unless exactly it passed there.
/// libtest names each test's thread after the test.
fn in_child() {
    let test = std::thread::current()
        .name()
        .expect("libtest names the test thread")
        .to_owned();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let out = Command::new(std::env::current_exe().unwrap())
        .args([&test, "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{test} failed in its own process\n--- stdout\n{stdout}\n--- stderr\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A git repo folder, listed in the project list.
pub fn project(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::create_dir_all(p.join(".git")).unwrap();
    obmem_core::projects::record(&p).unwrap();
    p
}

pub fn start() -> Headless {
    start_in(None)
}

/// The app on a computer whose language is `language` (a BCP 47 tag); `None` is the test
/// host's default, English.
pub fn start_in(language: Option<&str>) -> Headless {
    let mut config = EngineConfig::new(obmem_gui::root());
    // A desktop window, not the default phone-sized one.
    config.viewport = Size::new(Points(1100.0), Points(900.0));
    if let Some(tag) = language {
        config.pinned.locale = Locale::parse(tag);
        config.pinned.languages = Languages::parse([tag]);
    }
    Headless::start(config).unwrap()
}
