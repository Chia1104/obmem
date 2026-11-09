//! Review: approving a draft, verifying the note a finding is about, rejecting a finding, and
//! running the audit, across a project and the personal vault.

mod common;
use common::{SLOW, project, sandbox, start};
use obmem_core::config::Config;
use obmem_core::note::{Note, NoteType, Scope};
use obmem_core::store::{Store, WriteOutcome, WriteRequest};
use obmem_core::init;

fn write(store: &mut Store, title: &str, ty: NoteType, scope: Scope) -> String {
    let req = WriteRequest {
        note_type: ty,
        title: title.into(),
        body: format!("{title}: the body."),
        scope: Some(scope),
        tags: vec![],
        code_refs: vec![],
        confidence: None,
        update_id: None,
        force_new: false,
        author: "agent:test".into(),
        file_stem: None,
    };
    match store.write(req).unwrap() {
        WriteOutcome::Written { id, .. } => id,
        WriteOutcome::Duplicate { .. } => panic!("duplicate"),
    }
}

fn status(path: &std::path::Path) -> String {
    Note::parse(&std::fs::read_to_string(path).unwrap()).unwrap().fm.status.unwrap_or_default()
}

#[test]
fn drafts_and_findings_are_decided_on() {
    let dir = sandbox("review");
    let repo = project(&dir, "review-repo");
    let cfg = Config::load(Some(&repo)).unwrap();
    init::run(&cfg).unwrap();
    let mut store = Store::open(cfg.clone()).unwrap();
    write(&mut store, "Use SQLite for the cache", NoteType::Decision, Scope::Project);
    write(&mut store, "Prefer short answers", NoteType::Preference, Scope::Global);
    let old = write(&mut store, "Old fact", NoteType::Fact, Scope::Project);
    let finding = cfg.project_vault.join("Findings/audit-stale-old.md");
    std::fs::write(
        &finding,
        format!(
            "---\ntype: finding\ntitle: \"stale: {old}\"\nstatus: open\nauthor: agent:audit\nrule: stale\n\
             severity: warn\ntarget: Knowledge/{old}.md\n---\n# stale: {old}\n\nNot checked for 200 days.\n"
        ),
    )
    .unwrap();

    let mut app = start();
    app.wait_for_within("id:screen-review", SLOW).unwrap();
    app.press("id:screen-review >> role:button").unwrap();
    app.wait_for_within("Use SQLite for the cache", SLOW).unwrap();
    // The personal vault's draft is listed once, in its own group.
    assert!(app.screen().shows("Personal vault"));
    assert!(app.screen().shows("Prefer short answers"));

    app.press("id:item-use-sqlite-for-the-cache >> role:button[Approve]").unwrap();
    app.wait_for_gone("text:Use SQLite for the cache", SLOW).unwrap();
    assert_eq!(status(&cfg.project_vault.join("Decisions/use-sqlite-for-the-cache.md")), "active");

    app.press("id:item-audit-stale-old >> role:button[Verify note]").unwrap();
    let today = chrono::Local::now().format("%Y-%m-%d");
    app.wait_for_within(&format!("text:[[{old}]] verified on {today} (no code_refs, so no commit recorded)."), SLOW).unwrap();
    let fact = Note::parse(&std::fs::read_to_string(cfg.project_vault.join(format!("Knowledge/{old}.md"))).unwrap()).unwrap();
    assert!(fact.fm.extra.contains_key("verified_at"));

    app.press("id:item-audit-stale-old >> role:button[Reject…]").unwrap();
    app.press("id:item-audit-stale-old >> role:button[Reject]").unwrap();
    app.wait_for_gone("id:item-audit-stale-old", SLOW).unwrap();
    assert_eq!(status(&finding), "rejected");

    app.press("id:notice >> role:button[Dismiss]").unwrap();
    app.wait_for_gone("id:notice", SLOW).unwrap();
    app.press("id:review-review-repo >> role:button[Run audit]").unwrap();
    app.wait_for_within("id:notice", SLOW).unwrap();
    let texts = app.screen().texts().join("\n");
    assert!(texts.contains("Checked 5 notes: 0 errors, 0 warnings. Findings: 0 new, 0 reopened, 0 closed."), "{texts}");

    let _ = std::fs::remove_dir_all(&dir);
}
