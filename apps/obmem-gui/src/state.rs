//! The window's state. Each part has a signal of its own, so a view re-renders only when what it
//! reads changes: a notice appearing must not rebuild a form the user is typing in. The signals
//! stay private: views read them through these functions and change them through the actions,
//! which run obmem's blocking work off the engine thread and write the result back. A global is
//! per surface, so each test has its own.
//!
//! An action says how it went in one of three places. A rule a form's value breaks shows at the
//! field, before anything runs (`crate::form`). An outcome that is one line, and any failure, is
//! a toast (`crate::toasts`). An outcome with more to read is a [`Notice`] on the page.
//! While an action's work runs, the control that started it is busy ([`running`]).

use crate::appearance::Language;
use crate::appearance::{self, Chosen, Mode, Scheme, ThemeFile};
use crate::data::{self, Detail, Hit, Personal, ProjectRow, ReviewGroup, Sessions, Vault};
use crate::toasts::{done, failed as fail};
use crate::words;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use obmem_agents::hooks::Agent;
use obmem_core::allowlist::Kind;
use obmem_core::note::Scope;
use obmem_core::settings::{GlobalSettings, ProjectSettings, SyncSettings};
use obmem_core::store::{Decided, Verdict};
use std::path::PathBuf;

/// Something loaded in the background.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Load<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(String),
}

/// What the last action reported, when that is more than a line: a title and a line per thing
/// that happened, in the language the window had then. Shown above the page until it is
/// dismissed or the user leaves the screen.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Notice {
    pub title: String,
    pub lines: Vec<String>,
}

/// A decision on a review item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Decision {
    Approve,
    Verify,
    Resolve(Verdict),
}

/// Work a control starts and waits for. The control is busy while it runs ([`running`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Work {
    AddProject,
    RemoveProject,
    Initialize,
    /// Reading the shown project again: the Refresh button, and after a change to it.
    Refresh,
    SetUp(Agent),
    ImportClaude,
    PreviewTask,
    SaveSettings,
    Scan,
    Allow(Vault, Kind, String),
    Disallow(Vault, Kind, String),
    SavePersonal,
    SaveSync,
    ConnectSync,
    Sync,
    /// Reading the review list again.
    LoadReview,
    Audit(PathBuf),
    /// A decision on a review item: the item's id, and which decision.
    Review(String, Decision),
    /// Copying the theme an appearance shows into the user's themes folder.
    DuplicateTheme(Scheme),
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
    /// How the window looks.
    Appearance,
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
    global::<TaskPreview>()
        .get_untracked()
        .0
        .map(|(t, _)| t)
        .unwrap_or_default()
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
    global::<ScanResult>()
        .get_untracked()
        .0
        .map(|(input, _)| input)
        .unwrap_or_default()
}

fn set_notice(n: Option<Notice>) {
    global::<Notices>().set(Notices(n));
}

pub(crate) fn dismiss_notice() {
    set_notice(None);
}

#[derive(Clone, Default)]
struct Running(Vec<Work>);

/// Whether `work` is running, for a button's `busy`.
pub(crate) fn running(work: Work) -> impl Fn() -> bool + Clone + 'static {
    move || global::<Running>().with(|r| r.0.contains(&work))
}

/// Whether a decision on the review item `id` is running: its other buttons wait for it.
pub(crate) fn reviewing(id: String) -> impl Fn() -> bool + Clone + 'static {
    move || {
        global::<Running>().with(|r| {
            r.0.iter()
                .any(|w| matches!(w, Work::Review(item, _) if *item == id))
        })
    }
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
    let owner = global::<Tasks>()
        .with(|t| t.0)
        .unwrap_or_else(Owner::current);
    owner.run(|| drop(spawn(task)));
}

/// Runs `work` on a blocking thread and hands its result to `then` on the engine thread.
fn in_background<R: Send + 'static>(
    work: impl FnOnce() -> R + Send + 'static,
    then: impl FnOnce(R) + 'static,
) {
    run_task(async move {
        match blocking(work).await {
            Ok(r) => then(r),
            Err(e) => fail(format!("the background task failed: {e}")),
        }
    });
}

/// [`in_background`], with `what` running until the work ends: the control that started it is
/// busy meanwhile.
fn in_background_as<R: Send + 'static>(
    what: Work,
    work: impl FnOnce() -> R + Send + 'static,
    then: impl FnOnce(R) + 'static,
) {
    global::<Running>().update(|r| r.0.push(what.clone()));
    run_task(async move {
        let result = blocking(work).await;
        global::<Running>().update(|r| {
            if let Some(i) = r.0.iter().position(|w| *w == what) {
                r.0.remove(i);
            }
        });
        match result {
            Ok(r) => then(r),
            Err(e) => fail(format!("the background task failed: {e}")),
        }
    });
}

/// Runs a change to the shown project, then reports `done` and reads the project again.
fn change_project(
    work: impl FnOnce(PathBuf) -> Result<(), String> + Send + 'static,
    done: impl Into<String>,
) {
    let Some(path) = selected() else { return };
    let again = path.clone();
    let done = done.into();
    in_background(
        move || work(path),
        move |r| {
            match r {
                Ok(()) => crate::toasts::done(done),
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
        let current = wanted
            .or_else(selected)
            .filter(|p| rows.iter().any(|r| &r.path == p));
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
    in_background_as(
        Work::Refresh,
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
            in_background(
                move || data::session_start(&path),
                |r| {
                    global::<SessionStart>()
                        .set(SessionStart(r.map_or_else(Load::Failed, Load::Ready)));
                },
            );
        }
        Tab::Sessions => {
            global::<SessionsState>().set(SessionsState(Load::Loading));
            in_background(
                move || data::sessions(&path),
                |r| {
                    global::<SessionsState>()
                        .set(SessionsState(r.map_or_else(Load::Failed, Load::Ready)));
                },
            );
        }
        _ => {}
    }
}

/// Shows what `memory_context` gives an agent for `task` in the shown project.
pub(crate) fn preview_task(task: String) {
    let Some(path) = selected() else { return };
    global::<TaskPreview>().set(TaskPreview(Some((task.clone(), Load::Loading))));
    let wanted = task.clone();
    in_background_as(
        Work::PreviewTask,
        move || data::context_for(&path, &task),
        move |r| {
            // A newer preview may have started meanwhile.
            if last_task() == wanted {
                global::<TaskPreview>().set(TaskPreview(Some((
                    wanted,
                    r.map_or_else(Load::Failed, Load::Ready),
                ))));
            }
        },
    );
}

/// Imports the chosen Claude Code memory files as drafts.
pub(crate) fn import_claude(chosen: Vec<PathBuf>) {
    let Some(path) = selected() else { return };
    let again = path.clone();
    in_background_as(
        Work::ImportClaude,
        move || data::import_claude(&path, &chosen),
        move |r| {
            match r {
                Ok(files) => set_notice(Some(Notice {
                    title: t!("claude_memory.imported"),
                    lines: files
                        .into_iter()
                        .map(|(name, done)| match done {
                            Ok(note) => {
                                t!("claude_memory.imported_as", name = name, note = note)
                            }
                            Err(why) => t!(
                                "claude_memory.skipped",
                                name = name,
                                why = words::skip(&why)
                            ),
                        })
                        .collect(),
                })),
                Err(e) => fail(e),
            }
            reload(again);
        },
    );
}

/// Asks for a folder and lists the project it is in.
pub(crate) fn add_project() {
    run_task(async {
        let dialog = OpenDialog::folders()
            .title(t!("sidebar.add_dialog.title"))
            .button_label(t!("sidebar.add_dialog.button"));
        match pick_paths(dialog).await {
            Ok(Some(paths)) => {
                let Some(folder) = paths.into_iter().next() else {
                    return;
                };
                in_background_as(
                    Work::AddProject,
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
    in_background_as(
        Work::RemoveProject,
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
    in_background_as(
        Work::Initialize,
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
    in_background_as(
        Work::SetUp(agent),
        move || data::set_up(&path, agent),
        move |r| {
            match r {
                Ok(done) => set_notice(Some(Notice {
                    title: t!("agents.set_up_done", agent = done.agent.label()),
                    lines: done
                        .changed
                        .iter()
                        .map(|file| t!("agents.updated", file = file))
                        .chain(
                            done.manual
                                .iter()
                                .map(|step| t!("agents.next", step = words::manual(step))),
                        )
                        .collect(),
                })),
                Err(e) => fail(e),
            }
            reload(again);
        },
    );
}

// --- Settings and secrets ----------------------------------------------------

pub(crate) fn save_settings(s: ProjectSettings) {
    let Some(path) = selected() else { return };
    let again = path.clone();
    in_background_as(
        Work::SaveSettings,
        move || data::save_settings(&path, &s),
        move |r| match r {
            Ok(()) => {
                done(t!("settings.saved"));
                reload(again);
            }
            // Reloading would rebuild the form from the file and lose what the user typed.
            Err(e) => fail(e),
        },
    );
}

pub(crate) fn set_codex_auto_approve(on: bool) {
    let done = if on {
        t!("codex.auto_approve_on")
    } else {
        t!("codex.auto_approve_off")
    };
    change_project(move |path| data::set_codex_auto_approve(&path, on), done);
}

/// Tests text against the shown project's scanner.
pub(crate) fn scan(input: String) {
    let Some(path) = selected() else { return };
    let text = input.clone();
    in_background_as(
        Work::Scan,
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
        Vault::Project => t!("allowlist.allowed_here"),
        Vault::Personal => t!("allowlist.allowed_everywhere"),
    };
    let entry = entry.trim().to_string();
    in_background_as(
        Work::Allow(vault, kind, entry.clone()),
        move || data::allow(&path, vault, kind, &entry),
        move |r| after_allowlist_change(r, done),
    );
}

pub(crate) fn disallow(vault: Vault, kind: Kind, entry: String) {
    let path = selected().unwrap_or_default();
    in_background_as(
        Work::Disallow(vault, kind, entry.clone()),
        move || data::disallow(&path, vault, kind, &entry),
        move |r| after_allowlist_change(r, t!("allowlist.removed")),
    );
}

fn after_allowlist_change(r: Result<(), String>, said: String) {
    match r {
        Ok(()) => done(said),
        Err(e) => fail(e),
    }
    refresh();
    // The allowlist changed: scan the last text again so its result shows what now passes.
    if let Some(input) =
        global::<ScanResult>().with(|v| v.0.as_ref().map(|(input, _)| input.clone()))
    {
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
    in_background_as(
        Work::SavePersonal,
        move || data::save_personal(&s),
        |r| match r {
            Ok(()) => {
                done(t!("personal.saved"));
                load_personal();
            }
            // Reloading would rebuild the form from the file and lose what the user typed.
            Err(e) => fail(e),
        },
    );
}

pub(crate) fn save_sync(s: SyncSettings) {
    in_background_as(
        Work::SaveSync,
        move || data::save_sync(&s),
        |r| match r {
            Ok(()) => {
                done(t!("sync.saved"));
                load_personal();
            }
            // Reloading would rebuild the form from the file and lose what the user typed.
            Err(e) => fail(e),
        },
    );
}

pub(crate) fn connect_sync(
    access_key_id: String,
    secret: String,
    passphrase: String,
    repeat: String,
) {
    in_background_as(
        Work::ConnectSync,
        move || data::connect_sync(access_key_id, secret, passphrase, repeat),
        |r| match r {
            Ok(connected) => {
                done(if connected.created {
                    t!("sync.started_vault", bucket = connected.bucket)
                } else {
                    t!("sync.connected_to", bucket = connected.bucket)
                });
                load_personal();
            }
            Err(e) => fail(e),
        },
    );
}

pub(crate) fn sync_now() {
    in_background_as(Work::Sync, data::sync_now, |r| {
        match r {
            Ok(Some(summary)) => done(t!("sync.synced", summary = words::summary(&summary))),
            Ok(None) => done(t!("sync.queued")),
            Err(e) => fail(e),
        }
        load_personal();
    });
}

// --- Review ------------------------------------------------------------------

pub(crate) fn show_review() {
    set_notice(None);
    global::<Screens>().set(Screens(Screen::Review));
    load_review();
}

pub(crate) fn load_review() {
    in_background_as(Work::LoadReview, data::review, |r| {
        global::<ReviewState>().set(ReviewState(match r {
            Ok(groups) => Load::Ready(groups),
            Err(e) => Load::Failed(e),
        }));
    });
}

/// Runs a decision on a review item, says what it did, and reads the review list (and the shown
/// project, whose counts it changed) again.
fn decide(
    item: String,
    decision: Decision,
    work: impl FnOnce() -> Result<Decided, String> + Send + 'static,
) {
    in_background_as(Work::Review(item, decision), work, |r| {
        match r {
            Ok(decided) => done(words::decided(&decided)),
            Err(e) => fail(e),
        }
        load_review();
        refresh();
    });
}

/// Approves the draft the review item `item` is.
pub(crate) fn approve(item: String, root: PathBuf, scope: Scope, rel: String) {
    decide(item, Decision::Approve, move || {
        data::approve(&root, scope, &rel)
    });
}

pub(crate) fn resolve(item: String, root: PathBuf, scope: Scope, rel: String, verdict: Verdict) {
    decide(item, Decision::Resolve(verdict), move || {
        data::resolve(&root, scope, &rel, verdict)
    });
}

/// Verifies a note in `scope`'s vault: for a finding, the vault the finding is in.
pub(crate) fn verify(item: String, root: PathBuf, scope: Scope, rel: String) {
    decide(item, Decision::Verify, move || {
        data::verify(&root, scope, &rel)
    });
}

pub(crate) fn run_audit(root: PathBuf) {
    in_background_as(
        Work::Audit(root.clone()),
        move || data::run_audit(&root),
        |r| {
            match r {
                Ok(audit) => set_notice(Some(Notice {
                    title: match audit.findings {
                        Some((new, reopened, closed)) => t!(
                            "review.audit_done_with_findings",
                            checked = audit.checked,
                            errors = audit.errors,
                            warnings = audit.warnings,
                            new = new,
                            reopened = reopened,
                            closed = closed
                        ),
                        None => t!(
                            "review.audit_done",
                            checked = audit.checked,
                            errors = audit.errors,
                            warnings = audit.warnings
                        ),
                    },
                    lines: audit.notes,
                })),
                Err(e) => fail(e),
            }
            load_review();
            refresh();
        },
    );
}

// --- Appearance --------------------------------------------------------------

/// Every theme. Before the user's files are read: the built-in ones.
#[derive(Clone)]
struct Themes(Vec<ThemeFile>);
impl Default for Themes {
    fn default() -> Self {
        Themes(appearance::built_in())
    }
}

/// What the user chose.
#[derive(Clone, Default)]
struct Picked(Chosen);

pub(crate) fn themes() -> Vec<ThemeFile> {
    global::<Themes>().with(|t| t.0.clone())
}

pub(crate) fn mode() -> Mode {
    global::<Picked>().with(|p| p.0.mode)
}

/// The language the user chose for the window.
pub(crate) fn language() -> Language {
    global::<Picked>().with(|p| p.0.language)
}

/// The id of the theme chosen for `scheme`, which is gone when no theme has it.
pub(crate) fn chosen_theme(scheme: Scheme) -> String {
    global::<Picked>().with(|p| p.0.theme(scheme).to_owned())
}

/// The id of the theme `scheme` shows: the chosen one, or the default when that one is gone.
pub(crate) fn shown_theme(scheme: Scheme) -> String {
    global::<Picked>().with(|p| {
        global::<Themes>().with(|t| appearance::shown(&t.0, p.0.theme(scheme)).id.clone())
    })
}

/// Called by the root component: draws the window as the user chose, in the language they chose,
/// and keeps it so. The files
/// are read here, on the engine thread, so that the first frame already has the theme; they are
/// a few small ones.
pub(crate) fn start_appearance() {
    let chosen = appearance::chosen();
    set_color_scheme(chosen.mode.into());
    set_locale(chosen.language.locale());
    global::<Picked>().set(Picked(chosen));
    global::<Themes>().set(Themes(appearance::themes()));
    // Again whenever a theme is picked or read again, and when the appearance shown changes: a
    // theme's corner radius and fonts are those of the theme that shows.
    effect(|| {
        let showing_dark = dark();
        set_tokens(
            global::<Picked>().with(|p| {
                global::<Themes>().with(|t| appearance::tokens(&t.0, &p.0, showing_dark))
            }),
        );
    });
    // A theme file edited in another app shows when the user comes back to the window.
    effect(|| {
        if lifecycle() == nsh_core::platform::Lifecycle::Active {
            untracked(load_themes);
        }
    });
}

pub(crate) fn show_appearance() {
    set_notice(None);
    global::<Screens>().set(Screens(Screen::Appearance));
    load_themes();
}

/// Reads the theme files again. What is chosen stays as it is in the window: `appearance.toml`
/// is read once, when the app starts.
fn load_themes() {
    in_background(appearance::themes, set_themes);
}

fn set_themes(themes: Vec<ThemeFile>) {
    if global::<Themes>().with_untracked(|t| t.0 != themes) {
        global::<Themes>().set(Themes(themes));
    }
}

/// Changes what is chosen, at once, and keeps it.
fn choose(change: impl FnOnce(&mut Chosen)) {
    global::<Picked>().update(|p| change(&mut p.0));
    let chosen = global::<Picked>().get_untracked().0;
    in_background(
        move || appearance::save(&chosen),
        |r| {
            if let Err(e) = r {
                fail(format!("Could not save the appearance: {e}"));
            }
        },
    );
}

pub(crate) fn set_mode(mode: Mode) {
    set_color_scheme(mode.into());
    choose(|c| c.mode = mode);
}

/// Shows the window in `language`, at once, and keeps the choice.
pub(crate) fn set_language(language: Language) {
    set_locale(language.locale());
    choose(|c| c.language = language);
}

pub(crate) fn pick_theme(scheme: Scheme, id: String) {
    choose(|c| c.set_theme(scheme, id));
}

/// Copies the theme `scheme` shows into the user's themes folder and shows the copy, which is
/// theirs to edit.
pub(crate) fn duplicate_theme(scheme: Scheme) {
    let id = untracked(|| shown_theme(scheme));
    in_background_as(
        Work::DuplicateTheme(scheme),
        move || appearance::duplicate(&id).map(|copy| (copy, appearance::themes())),
        move |r| match r {
            Ok((copy, themes)) => {
                done(t!("appearance.duplicated", copy = copy));
                set_themes(themes);
                pick_theme(scheme, copy);
            }
            Err(e) => fail(e),
        },
    );
}

/// Shows `file`, or the themes folder, in the file manager.
pub(crate) fn reveal_theme(file: Option<PathBuf>) {
    in_background(
        move || match file {
            Some(file) => Ok(file),
            None => appearance::ensure_themes_dir(),
        },
        |r| match r {
            Ok(path) => drop(reveal_path(path)),
            Err(e) => fail(e),
        },
    );
}
