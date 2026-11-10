//! What an agent is given in this project: the session-start context, and `memory_context` for a
//! task. For checking that the notes an agent should find are the ones it gets.

use crate::state::{self, Load, Work};
use crate::widgets::{Level, card, check, heading, loading, muted, primary, text_box};
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn agent_tab() -> View {
    view().gap_4().child(task()).child(session_start())
}

fn task() -> View {
    let input = signal(state::last_task());
    card()
        .test_id("task-preview")
        .child(heading("For a task"))
        .child(muted(
            "What memory_context returns when an agent describes this task, in any language.",
        ))
        .child(text_box(input, "e.g. add a retry to the sync job").test_id("task-input"))
        .child(
            view().flex_row().pt_2().child(
                primary("Preview", move || {
                    state::preview_task(input.get_untracked())
                })
                .busy(state::running(Work::PreviewTask)),
            ),
        )
        .child(dynamic(move || state::task_preview().map(as_given)))
}

fn session_start() -> View {
    card()
        .test_id("session-start")
        .child(heading("At session start"))
        .child(muted(
            "What the session-start hook gives an agent before its first message.",
        ))
        .child(dynamic(move || as_given(state::session_start())))
}

/// The text exactly as the agent receives it, line for line.
fn as_given(text: Load<String>) -> AnyElement {
    match text {
        Load::Loading => loading("Loading…").mt_2().into_element(),
        Load::Failed(e) => {
            check(Level::Error, "The agent would get this error instead", e).into_element()
        }
        Load::Ready(t) => view()
            .mt_2()
            .p_3()
            .rounded(move || theme().radius)
            .bg(move || theme().muted)
            .child(
                nsh_core::prelude::text(t)
                    .text_xs()
                    .font_family("monospace"),
            )
            .into_element(),
    }
}
