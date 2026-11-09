//! A throwaway world for the window: obmem's config, personal vault and projects in a temp
//! folder (OBMEM_CONFIG points obmem there), and the app in a desktop-sized window.

use nsh_core::prelude::{Points, Size};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SLOW: Duration = Duration::from_secs(20);

/// Each test binary is its own process and has one test, so setting the environment here is
/// safe: nothing else in the process reads it yet.
pub fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obmem-gui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(dir.join("config/config.toml"), format!("vault = {:?}\n", dir.join("personal").display().to_string()))
        .unwrap();
    // SAFETY: see above.
    unsafe { std::env::set_var("OBMEM_CONFIG", dir.join("config/config.toml")) };
    dir
}

/// A git repo folder, listed in the project list.
pub fn project(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::create_dir_all(p.join(".git")).unwrap();
    obmem_core::projects::record(&p).unwrap();
    p
}

pub fn start() -> Headless {
    let mut config = EngineConfig::new(Root::of::<obmem_gui::App>());
    // A desktop window, not the default phone-sized one.
    config.viewport = Size::new(Points(1100.0), Points(900.0));
    Headless::start(config).unwrap()
}
