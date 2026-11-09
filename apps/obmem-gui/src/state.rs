//! The window's state. Each part has a signal of its own, so a view re-renders only when what it
//! reads changes: a notice appearing must not rebuild a form the user is typing in. The signals
//! stay private: views read them through these functions and change them through the actions,
//! which run obmem's blocking work off the engine thread and write the result back. A global is
//! per surface, so each test has its own.

use crate::data::{self, AuditDone, Detail, Hit, Personal, ProjectRow, ReviewGroup, Sessions, SetupDone, Vault};
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use obmem_agents::hooks::Agent;
use obmem_core::allowlist::Kind;
use obmem_core::settings::{GlobalSettings, ProjectSettings};
use obmem_core::note::Scope;
use obmem_core::store::Verdict;
use std::path::PathBuf;

/// Something loaded in the background.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Load<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(String),
}

/// What the last action reported, shown above the page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Notice {
    SetUp(SetupDone),
    Audit(AuditDone),
    /// A title and one line per thing that happened.
    Lines(String, Vec<String>),
    Done(String),
    Error(String),
}

/// What the main area shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum Screen {
    /// The selected project, on one of its tabs.
    #[default]
    Project,
    /// The user's own settings, which apply to every project.
    Personal,
    /// Drafts and findings waiting for a person, across projects.
    Review,
}

/// A project's tab.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum Tab {
    #[default]
    Overview,
    /// What an agent is given at session start and for a task.
    Agent,
    Sessions,
    Settings,
    Secrets,
}

#[derive(Clone, Default)]
struct Projects(Load<Vec<ProjectRow>>);
#[derive(Clone, Default)]
struct Selected(Option<PathBuf>);
#[derive(Clone, Default)]
struct Shown(Load<Detail>);
#[derive(Clone, Default)]
struct Notices(Option<Notice>);
#[derive(Clone, Default)]
struct Screens(Screen);
#[derive(Clone, Default)]
struct Tabs(Tab);
#[derive(Clone, Default)]
struct PersonalState(Load<Personal>);
#[derive(Clone, Default)]
struct ReviewState(Load<Vec<ReviewGroup>>);
#[derive(Clone, Default)]
struct SessionStart(Load<String>);
/// The task last previewed and what `memory_context` gave for it.
#[derive(Clone, Default)]
struct TaskPreview(Option<(String, Load<String>)>);
#[derive(Clone, Default)]
struct SessionsState(Load<Sessions>);
/// The text last scanned and what the scanner found in it.
#[derive(Clone, Default)]
struct ScanResult(Option<(String, Vec<Hit>)>);

pub(crate) fn projects() -> Load<Vec<ProjectRow>> {
    global::<Projects>().with(|v| v.0.clone())
}

pub(crate) fn selected() -> Option<PathBuf> {
    global::<Selected>().with(|v| v.0.clone())
}

pub(crate) fn detail() -> Load<Detail> {
    global::<Shown>().with(|v| v.0.clone())
}

pub(crate) fn notice() -> Option<Notice> {
    global::<Notices>().with(|v| v.0.clone())
}

pub(crate) fn screen() -> Screen {
    global::<Screens>().with(|v| v.0)
}

pub(crate) fn tab() -> Tab {
    global::<Tabs>().with(|v| v.0)
}

pub(crate) fn personal() -> Load<Personal> {
    global::<PersonalState>().with(|v| v.0.clone())
}

pub(crate) fn review() -> Load<Vec<ReviewGroup>> {
    global::<ReviewState>().with(|v| v.0.clone())
}

pub(crate) fn session_start() -> Load<String> {
    global::<SessionStart>().with(|v| v.0.clone())
}

pub(crate) fn task_preview() -> Option<Load<String>> {
    global::<TaskPreview>().with(|v| v.0.as_ref().map(|(_, r)| r.clone()))
}

/// The task last previewed, to seed the box when the tab is drawn again (untracked).
pub(crate) fn last_task() -> String {
    global::<TaskPreview>().get_untracked().0.map(|(t, _)| t).unwrap_or_default()
}

pub(crate) fn sessions() -> Load<Sessions> {
    global::<SessionsState>().with(|v| v.0.clone())
}

/// The secrets the last test found; `None` before a test.
pub(crate) fn scan_result() -> Option<Vec<Hit>> {
    global::<ScanResult>().with(|v| v.0.as_ref().map(|(_, hits)| hits.clone()))
}

/// The text last scanned, so the box keeps it when the tab is drawn again. Untracked: a scan
/// must not redraw the tab that shows the box.
pub(crate) fn last_scanned() -> String {
    global::<ScanResult>().get_untracked().0.map(|(input, _)| input).unwrap_or_default()
}

fn set_notice(n: Option<Notice>) {
    global::<Notices>().set(Notices(n));
}

fn fail(message: String) {
    set_notice(Some(Notice::Error(message)));
}

/// Shows a problem the view found itself (input it cannot use).
pub(crate) fn report(message: String) {
    fail(message);
}

pub(crate) fn dismiss_notice() {
    set_notice(None);
}

/// The owner the app's tasks run under. A task ends with its owner, and the button that starts
/// one usually goes away when the task changes the page; the window's owner outlives both.
#[derive(Clone, Default)]
struct Tasks(Option<Owner>);

/// Called by the root component, with its owner current.
pub(crate) fn own_tasks() {
    global::<Tasks>().set(Tasks(Some(Owner::current())));
}

/// Runs `task` under the window's owner.
fn run_task(task: impl Future<Output = ()> + 'static) {
    let owner = global::<Tasks>().with(|t| t.0).unwrap_or_else(Owner::current);
    owner.run(|| drop(spawn(task)));
}

/// Runs `work` on a blocking thread and hands its result to `then` on the engine thread.
fn in_background<R: Send + 'static>(work: impl FnOnce() -> R + Send + 'static, then: impl FnOnce(R) + 'static) {
    run_task(async move {
        match blocking(work).await {
            Ok(r) => then(r),
            Err(e) => fail(format!("the background task failed: {e}")),
        }
    });
}

/// Runs a change to the shown project, then reports `done` and reads the project again.
fn change_project(work: impl FnOnce(PathBuf) -> Result<(), String> + Send + 'static, done: impl Into<String>) {
    let Some(path) = selected() else { return };
    let again = path.clone();
    let done = done.into();
    in_background(
        move || work(path),
        move |r| {
            match r {
                Ok(()) => set_notice(Some(Notice::Done(done))),
                Err(e) => fail(e),
            }
            reload(again);
        },
    );
}

// --- Projects ----------------------------------------------------------------

/// Reads the project list again; selects `select`, or keeps the selection, or picks the first.
pub(crate) fn load_projects(wanted: Option<PathBuf>) {
    in_background(data::projects, move |r| {
        let rows = match r {
            Ok(rows) => rows,
            Err(e) => {
                global::<Projects>().set(Projects(Load::Failed(e)));
                return;
            }
        };
        let current = wanted.or_else(selected).filter(|p| rows.iter().any(|r| &r.path == p));
        let next = current.or_else(|| rows.first().map(|r| r.path.clone()));
        global::<Projects>().set(Projects(Load::Ready(rows)));
        match next {
            Some(path) => select(path),
            None => {
                global::<Selected>().set(Selected(None));
                global::<Shown>().set(Shown(Load::Loading));
            }
        }
    });
}

/// Shows a project: the user picked it.
pub(crate) fn open(path: PathBuf) {
    select(path);
    global::<Screens>().set(Screens(Screen::Project));
}

/// Makes `path` the selected project and reads it, without leaving the screen the user is on:
/// the project list loads in the background and must not pull them away from another screen.
fn select(path: PathBuf) {
    if selected().as_ref() != Some(&path) {
        if screen() == Screen::Project {
            set_notice(None);
        }
        global::<Tabs>().set(Tabs(Tab::Overview));
        global::<ScanResult>().set(ScanResult(None));
        global::<TaskPreview>().set(TaskPreview(None));
        global::<Shown>().set(Shown(Load::Loading));
    }
    global::<Selected>().set(Selected(Some(path.clone())));
    reload(path);
}

/// Reads the shown project again, keeping it on screen until the new state arrives.
fn reload(path: PathBuf) {
    let wanted = path.clone();
    in_background(
        move || data::detail(&path),
        move |r| {
            // The user may have picked another project meanwhile.
            if selected().as_ref() != Some(&wanted) {
                return;
            }
            global::<Shown>().set(Shown(match r {
                Ok(d) => Load::Ready(d),
                Err(e) => Load::Failed(e),
            }));
        },
    );
}

pub(crate) fn refresh() {
    if let Some(path) = selected() {
        reload(path);
    }
}

pub(crate) fn show_tab(t: Tab) {
    global::<Tabs>().set(Tabs(t));
    let Some(path) = selected() else { return };
    match t {
        Tab::Agent => {
            global::<SessionStart>().set(SessionStart(Load::Loading));
            in_background(move || data::session_start(&path), |r| {
                global::<SessionStart>().set(SessionStart(r.map_or_else(Load::Failed, Load::Ready)));
            });
        }
        Tab::Sessions => {
            global::<SessionsState>().set(SessionsState(Load::Loading));
            in_background(move || data::sessions(&path), |r| {
                global::<SessionsState>().set(SessionsState(r.map_or_else(Load::Failed, Load::Ready)));
            });
        }
        _ => {}
    }
}

/// Shows what `memory_context` gives an agent for `task` in the shown project.
pub(crate) fn preview_task(task: String) {
    let Some(path) = selected() else { return };
    global::<TaskPreview>().set(TaskPreview(Some((task.clone(), Load::Loading))));
    let wanted = task.clone();
    in_background(move || data::context_for(&path, &task), move |r| {
        // A newer preview may have started meanwhile.
        if last_task() == wanted {
            global::<TaskPreview>().set(TaskPreview(Some((wanted, r.map_or_else(Load::Failed, Load::Ready)))));
        }
    });
}

/// Imports the chosen Claude Code memory files as drafts.
pub(crate) fn import_claude(chosen: Vec<PathBuf>) {
    let Some(path) = selected() else { return };
    let again = path.clone();
    in_background(move || data::import_claude(&path, &chosen), move |r| {
        set_notice(Some(match r {
            Ok(lines) => Notice::Lines("Claude Code memory imported".into(), lines),
            Err(e) => Notice::Error(e),
        }));
        reload(again);
    });
}

/// Asks for a folder and lists the project it is in.
pub(crate) fn add_project() {
    run_task(async {
        let dialog = OpenDialog::folders().title("Add a project").button_label("Add");
        match pick_paths(dialog).await {
            Ok(Some(paths)) => {
                let Some(folder) = paths.into_iter().next() else { return };
                in_background(
                    move || data::add_project(&folder),
                    |r| match r {
                        Ok(root) => load_projects(Some(root)),
                        Err(e) => fail(e),
                    },
                );
            }
            Ok(None) => {}
            Err(e) => fail(format!("could not show the folder dialog: {e}")),
        }
    });
}

/// Takes the shown project off the list (its files stay).
pub(crate) fn remove_selected() {
    let Some(path) = selected() else { return };
    in_background(
        move || data::remove_project(&path),
        |r| match r {
            Ok(()) => {
                global::<Selected>().set(Selected(None));
                load_projects(None);
            }
            Err(e) => fail(e),
        },
    );
}

pub(crate) fn initialize() {
    let Some(path) = selected() else { return };
    in_background(
        move || data::initialize(&path),
        |r| match r {
            Ok(()) => load_projects(None),
            Err(e) => fail(e),
        },
    );
}

pub(crate) fn set_up(agent: Agent) {
    let Some(path) = selected() else { return };
    let again = path.clone();
    in_background(
        move || data::set_up(&path, agent),
        move |r| {
            set_notice(Some(match r {
                Ok(done) => Notice::SetUp(done),
                Err(e) => Notice::Error(e),
            }));
            reload(again);
        },
    );
}

// --- Settings and secrets ----------------------------------------------------

pub(crate) fn save_settings(s: ProjectSettings) {
    change_project(move |path| data::save_settings(&path, &s), "Settings saved.");
}

pub(crate) fn set_codex_auto_approve(on: bool) {
    let done = if on { "Codex now runs obmem's tools without asking." } else { "Codex asks before running obmem's tools again." };
    change_project(move |path| data::set_codex_auto_approve(&path, on), done);
}

/// Tests text against the shown project's scanner.
pub(crate) fn scan(input: String) {
    let Some(path) = selected() else { return };
    let text = input.clone();
    in_background(
        move || data::scan(&path, &input),
        move |r| match r {
            Ok(hits) => global::<ScanResult>().set(ScanResult(Some((text, hits)))),
            Err(e) => fail(e),
        },
    );
}

/// Adds an allowlist entry, then reads the project and the personal settings again.
pub(crate) fn allow(vault: Vault, kind: Kind, entry: String) {
    let path = selected().unwrap_or_default();
    let done = match vault {
        Vault::Project => "Allowed in this project.",
        Vault::Personal => "Allowed in every project.",
    };
    in_background(
        move || data::allow(&path, vault, kind, &entry),
        move |r| after_allowlist_change(r, done),
    );
}

pub(crate) fn disallow(vault: Vault, kind: Kind, entry: String) {
    let path = selected().unwrap_or_default();
    in_background(
        move || data::disallow(&path, vault, kind, &entry),
        move |r| after_allowlist_change(r, "Removed from the allowlist."),
    );
}

fn after_allowlist_change(r: Result<(), String>, done: &str) {
    match r {
        Ok(()) => set_notice(Some(Notice::Done(done.into()))),
        Err(e) => fail(e),
    }
    refresh();
    // The allowlist changed: scan the last text again so its result shows what now passes.
    if let Some(input) = global::<ScanResult>().with(|v| v.0.as_ref().map(|(input, _)| input.clone())) {
        scan(input);
    }
    if screen() == Screen::Personal {
        load_personal();
    }
}

// --- Personal settings -------------------------------------------------------

pub(crate) fn show_personal() {
    set_notice(None);
    global::<Screens>().set(Screens(Screen::Personal));
    load_personal();
}

fn load_personal() {
    in_background(data::personal, |r| {
        global::<PersonalState>().set(PersonalState(match r {
            Ok(p) => Load::Ready(p),
            Err(e) => Load::Failed(e),
        }));
    });
}

pub(crate) fn save_personal(s: GlobalSettings) {
    in_background(
        move || data::save_personal(&s),
        |r| {
            match r {
                Ok(()) => set_notice(Some(Notice::Done("Personal settings saved.".into()))),
                Err(e) => fail(e),
            }
            load_personal();
        },
    );
}

// --- Review ------------------------------------------------------------------

pub(crate) fn show_review() {
    set_notice(None);
    global::<Screens>().set(Screens(Screen::Review));
    load_review();
}

pub(crate) fn load_review() {
    in_background(data::review, |r| {
        global::<ReviewState>().set(ReviewState(match r {
            Ok(groups) => Load::Ready(groups),
            Err(e) => Load::Failed(e),
        }));
    });
}

/// Runs a review action, reports its message, and reads the review list (and the shown project,
/// whose counts it changed) again.
fn review_action(work: impl FnOnce() -> Result<String, String> + Send + 'static) {
    in_background(work, |r| {
        set_notice(Some(match r {
            Ok(msg) => Notice::Done(msg),
            Err(e) => Notice::Error(e),
        }));
        load_review();
        refresh();
    });
}

pub(crate) fn approve(root: PathBuf, scope: Scope, rel: String) {
    review_action(move || data::approve(&root, scope, &rel));
}

pub(crate) fn resolve(root: PathBuf, scope: Scope, rel: String, verdict: Verdict) {
    review_action(move || data::resolve(&root, scope, &rel, verdict));
}

/// Verifies a note in `scope`'s vault: for a finding, the vault the finding is in.
pub(crate) fn verify(root: PathBuf, scope: Scope, rel: String) {
    review_action(move || data::verify(&root, scope, &rel));
}

pub(crate) fn run_audit(root: PathBuf) {
    in_background(
        move || data::run_audit(&root),
        |r| {
            set_notice(Some(match r {
                Ok(done) => Notice::Audit(done),
                Err(e) => Notice::Error(e),
            }));
            load_review();
            refresh();
        },
    );
}
