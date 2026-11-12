//! What an agent is given in this project: the session-start context, and `memory_context` for a
//! task. For checking that the notes an agent should find are the ones it gets.

use crate::state::{self, Load, Work};
use crate::widgets::{Level, card, check, heading, loading, muted, primary, text_box};
use crate::words::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn agent_tab() -> View {
    view().gap_4().child(task()).child(session_start())
}

fn task() -> View {
    let input = signal(state::last_task());
    card()
        .test_id("task-preview")
        .child(heading(tr!("agent_view.task")))
        .child(muted(tr!("agent_view.task_about")))
        .child(text_box(input, tr!("agent_view.task_example")).test_id("task-input"))
        .child(
            view().flex_row().pt_2().child(
                primary(tr!("agent_view.preview"), move || {
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
        .child(heading(tr!("agent_view.session_start")))
        .child(muted(tr!("agent_view.session_start_about")))
        .child(dynamic(move || as_given(state::session_start())))
}

/// The text exactly as the agent receives it, line for line.
fn as_given(text: Load<String>) -> AnyElement {
    match text {
        Load::Loading => loading(tr!("common.loading")).mt_2().into_element(),
        Load::Failed(e) => check(Level::Error, tr!("agent_view.error"), e).into_element(),
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
