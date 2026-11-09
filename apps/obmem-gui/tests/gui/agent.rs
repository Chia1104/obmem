//! Agent view, session logs, and importing Claude Code memory.

use crate::common::{SLOW, project, sandbox, start};
use obmem_core::config::Config;
use obmem_core::store::Store;
use obmem_core::{import, init};

#[test]
fn agent_view_sessions_and_claude_memory() {
    let Some(dir) = sandbox("agent") else { return };
    // Claude Code's memory lives under ~/.claude: point HOME at the sandbox so the real one is
    // never read or written. SAFETY: this test has its own process (see `sandbox`) and the app
    // has not started yet.
    unsafe { std::env::set_var("HOME", &dir) };
    let repo = project(&dir, "agent-repo");
    let cfg = Config::load(Some(&repo)).unwrap();
    init::run(&cfg).unwrap();
    let mut store = Store::open(cfg.clone()).unwrap();
    store.log(None, "Added the retry to the sync job; the backoff is still to tune.", "agent:test").unwrap();

    let memory = import::default_dir(&cfg.project_root);
    std::fs::create_dir_all(&memory).unwrap();
    let mem = |name: &str, ty: &str, desc: &str| {
        format!("---\nname: {name}\ndescription: {desc}\nmetadata:\n  type: {ty}\n---\n{desc}.\n")
    };
    std::fs::write(memory.join("keep-replies-short.md"), mem("keep-replies-short", "feedback", "Keep replies short")).unwrap();
    std::fs::write(memory.join("staging-url.md"), mem("staging-url", "reference", "Where staging lives")).unwrap();
    std::fs::write(memory.join("MEMORY.md"), "- index\n").unwrap();

    let mut app = start();
    app.wait_for_within("id:claude-memory", SLOW).unwrap();
    let pending = |n: usize| format!("{n} of the 2 memories in {} are not in the vaults yet. They come in as drafts for you to approve.", memory.display());
    assert!(app.screen().shows(&pending(2)), "{:?}", app.screen().texts());
    // Leave one out, import the other.
    app.press("id:memory-staging-url >> role:checkbox").unwrap();
    app.press("id:claude-memory >> role:button[Import selected]").unwrap();
    app.wait_for_within("Claude Code memory imported", SLOW).unwrap();
    assert!(cfg.global_vault.join("Global/Feedback/keep-replies-short.md").exists(), "the feedback memory is imported");
    assert!(!cfg.project_vault.join("Knowledge/staging-url.md").exists(), "the unchecked one is not");
    // The card now offers only what is left.
    app.wait_for_within(&format!("text:{}", pending(1)), SLOW).unwrap();

    app.press("role:tab[Agent view]").unwrap();
    app.wait_until("the session-start context", SLOW, |s| s.texts().iter().any(|t| t.starts_with("# Project memory (agent-repo)")))
        .unwrap();
    app.type_text("id:task-input", "tune the sync job backoff").unwrap();
    app.press("role:button[Preview]").unwrap();
    // The sandbox's vaults are not open in Obsidian, so search fails, and the preview shows the
    // error an agent would get.
    app.wait_until("the task preview", SLOW, |s| {
        s.texts().contains(&"The agent would get this error instead")
            && s.texts().iter().any(|t| t.starts_with("project vault not searched:"))
    })
    .unwrap();

    app.press("role:tab[Sessions]").unwrap();
    app.wait_for_within("text:Added the retry to the sync job; the backoff is still to tune.", SLOW).unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}
