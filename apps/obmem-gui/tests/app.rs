//! The window's flows, with the steps a user takes, against a throwaway config, vaults and
//! project (OBMEM_CONFIG points obmem's own files at a temp folder).

use nsh_core::prelude::{Points, Size};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SLOW: Duration = Duration::from_secs(20);

fn sandbox() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obmem-gui-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/config.toml"),
        format!("vault = {:?}\n", dir.join("personal").display().to_string()),
    )
    .unwrap();
    // SAFETY: set before the app or any other thread of this test binary reads the environment;
    // this file has a single test.
    unsafe { std::env::set_var("OBMEM_CONFIG", dir.join("config/config.toml")) };
    dir
}

fn project(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::create_dir_all(p.join(".git")).unwrap();
    p
}

#[test]
fn a_new_project_is_initialized_then_an_agent_set_up() {
    let dir = sandbox();
    let repo = project(&dir, "demo-repo");
    obmem_core::projects::record(&repo).unwrap();

    // A desktop window, not the default phone-sized one.
    let mut config = EngineConfig::new(Root::of::<obmem_gui::App>());
    config.viewport = Size::new(Points(960.0), Points(680.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for_within("text:demo-repo", SLOW).unwrap();
    app.wait_for_within("obmem is not set up in this folder", SLOW).unwrap();

    app.press("role:button[Set up obmem here]").unwrap();
    app.wait_for_within("Agents", SLOW).unwrap();
    assert!(repo.join(".obmem.toml").exists());
    assert!(repo.join("docs/kb").is_dir());

    app.press("id:agent-claude >> role:button[Set up]").unwrap();
    app.wait_for_within("Claude Code is set up", SLOW).unwrap();
    assert!(repo.join(".mcp.json").exists());
    assert!(repo.join(".claude/settings.json").exists());
    // The row follows: Claude Code no longer offers to set up, the others still do.
    app.wait_for_gone("id:agent-claude >> role:button[Set up]", SLOW).unwrap();
    assert!(app.screen().shows("Not set up: 2 file(s) to update"), "{:?}", app.screen().texts());

    let _ = std::fs::remove_dir_all(&dir);
}
