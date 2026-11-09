//! The window's state. Its signals stay private: views read them through these functions, and
//! change them through the actions, which run obmem's blocking work off the engine thread and
//! write the result back. A global is per surface, so each test has its own.

use crate::data::{self, Detail, ProjectRow, SetupDone};
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use obmem_agents::hooks::Agent;
use std::path::PathBuf;

/// Something loaded in the background.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Load<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(String),
}

/// What the last action reported, shown above the project.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Notice {
    SetUp(SetupDone),
    Error(String),
}

#[derive(Clone, Default)]
struct Window {
    projects: Load<Vec<ProjectRow>>,
    selected: Option<PathBuf>,
    detail: Load<Detail>,
    notice: Option<Notice>,
}

fn window() -> Signal<Window> {
    global::<Window>()
}

pub(crate) fn projects() -> Load<Vec<ProjectRow>> {
    window().with(|w| w.projects.clone())
}

pub(crate) fn selected() -> Option<PathBuf> {
    window().with(|w| w.selected.clone())
}

pub(crate) fn detail() -> Load<Detail> {
    window().with(|w| w.detail.clone())
}

pub(crate) fn notice() -> Option<Notice> {
    window().with(|w| w.notice.clone())
}

fn fail(message: String) {
    window().update(|w| w.notice = Some(Notice::Error(message)));
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

/// Reads the project list again; selects `select`, or keeps the selection, or picks the first.
pub(crate) fn load_projects(select: Option<PathBuf>) {
    in_background(data::projects, move |r| {
        let rows = match r {
            Ok(rows) => rows,
            Err(e) => {
                window().update(|w| w.projects = Load::Failed(e));
                return;
            }
        };
        let current = select.or_else(selected).filter(|p| rows.iter().any(|r| &r.path == p));
        let next = current.or_else(|| rows.first().map(|r| r.path.clone()));
        window().update(|w| w.projects = Load::Ready(rows));
        match next {
            Some(path) => open(path),
            None => window().update(|w| {
                w.selected = None;
                w.detail = Load::Loading;
            }),
        }
    });
}

/// Shows a project.
pub(crate) fn open(path: PathBuf) {
    window().update(|w| {
        if w.selected.as_ref() != Some(&path) {
            w.notice = None;
        }
        w.selected = Some(path.clone());
        w.detail = Load::Loading;
    });
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
            window().update(|w| {
                w.detail = match r {
                    Ok(d) => Load::Ready(d),
                    Err(e) => Load::Failed(e),
                }
            });
        },
    );
}

pub(crate) fn refresh() {
    if let Some(path) = selected() {
        reload(path);
    }
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
                window().update(|w| w.selected = None);
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
            window().update(|w| {
                w.notice = Some(match r {
                    Ok(done) => Notice::SetUp(done),
                    Err(e) => Notice::Error(e),
                })
            });
            reload(again);
        },
    );
}

pub(crate) fn dismiss_notice() {
    window().update(|w| w.notice = None);
}
