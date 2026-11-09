//! Overview: a new project is initialized, then an agent is set up.

mod common;
use common::{SLOW, project, sandbox, start};

#[test]
fn a_new_project_is_initialized_then_an_agent_set_up() {
    let dir = sandbox("overview");
    let repo = project(&dir, "demo-repo");

    let mut app = start();
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
