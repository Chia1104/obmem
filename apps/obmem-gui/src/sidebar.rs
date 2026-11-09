//! The project list: every project obmem has seen, and adding one by hand.

use crate::data::ProjectRow;
use crate::state::{self, Load, Screen};
use crate::widgets::{action, heading, muted};
use nsh_base::{button, theme};
use nsh_core::prelude::*;

pub(crate) fn sidebar() -> impl IntoElement {
    view()
        .w(px(260.0))
        .flex_shrink_0()
        .gap_2()
        .p_3()
        .border_r_1()
        .border_r_color(move || theme().border)
        .bg(move || theme().sidebar)
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(heading("Projects"))
                .child(action("Add…", state::add_project)),
        )
        .child(
            scroll()
                .flex_1()
                .child(dynamic(move || match state::projects() {
                    Load::Loading => Some(muted("Loading…")),
                    Load::Failed(e) => Some(muted(e)),
                    Load::Ready(rows) if rows.is_empty() => Some(muted(
                        "No projects yet. A project shows up here once an agent session starts in it, or add one.",
                    )),
                    Load::Ready(_) => None,
                }))
                .child(each(rows, |r: &ProjectRow| r.path.clone(), |row, _| project_row(row))),
        )
        .child(screen_button("Review", Screen::Review, state::show_review))
        .child(screen_button("Personal settings", Screen::Personal, state::show_personal))
}

/// An entry for a screen that is not a project.
fn screen_button(label: &'static str, screen: Screen, show: fn()) -> impl IntoElement {
    view().test_id(&format!("screen-{}", label.to_lowercase().replace(' ', "-"))).child(
        button()
            .px_2()
            .py_2()
            .rounded(move || theme().radius)
            .bg(move || if state::screen() == screen { theme().sidebar_accent } else { theme().sidebar })
            .active(|s| s.opacity(0.7))
            .on_press(show)
            .child(text(label)),
    )
}

/// The listed projects; none while loading.
fn rows() -> Vec<ProjectRow> {
    match state::projects() {
        Load::Ready(rows) => rows,
        _ => Vec::new(),
    }
}

fn project_row(row: ReadSignal<ProjectRow>) -> impl IntoElement {
    let path = move || row.with(|r| r.path.clone());
    let chosen = move || state::screen() == Screen::Project && state::selected().is_some_and(|p| p == path());
    button()
        .gap_1()
        .px_2()
        .py_2()
        .rounded(move || theme().radius)
        .bg(move || if chosen() { theme().sidebar_accent } else { theme().sidebar })
        .active(|s| s.opacity(0.7))
        .on_press(move || state::open(path()))
        .child(text(move || row.with(|r| r.name.clone())).font_weight(FontWeight::MEDIUM))
        .child(
            text(move || {
                row.with(|r| {
                    let p = r.path.display().to_string();
                    if r.missing { format!("{p} (missing)") } else { p }
                })
            })
            .text_xs()
            .number_of_lines(1u32)
            .text_color(move || theme().muted_foreground),
        )
}
