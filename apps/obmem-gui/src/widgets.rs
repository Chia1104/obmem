//! Small pieces the pages share, drawn with the theme.

use nsh_base::{button, input, theme};
use nsh_core::prelude::*;

/// A section title.
pub(crate) fn heading(label: impl Into<SharedString>) -> Text {
    text(label.into()).text_sm().font_weight(FontWeight::SEMIBOLD).text_color(move || theme().muted_foreground)
}

/// Secondary text.
pub(crate) fn muted(label: impl Into<SharedString>) -> Text {
    text(label.into()).text_sm().text_color(move || theme().muted_foreground)
}

/// A bordered button for secondary actions.
pub(crate) fn action(label: impl Into<SharedString>, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .px_3()
        .py_1()
        .border_1()
        .border_color(move || theme().border)
        .rounded(move || theme().radius)
        .active(|s| s.bg(move || theme().muted))
        .on_press(on_press)
        .child(text(label.into()).text_sm())
}

/// The button for the main action of a block.
pub(crate) fn primary(label: impl Into<SharedString>, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .px_3()
        .py_1()
        .rounded(move || theme().radius)
        .bg(move || theme().primary)
        .active(|s| s.opacity(0.7))
        .on_press(on_press)
        .child(text(label.into()).text_sm().text_color(move || theme().primary_foreground))
}

/// How a check came out.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Level {
    Ok,
    Warn,
    Error,
}

/// A check: its outcome, what it is, and what it found.
pub(crate) fn check(level: Level, title: impl Into<SharedString>, detail: impl Into<SharedString>) -> View {
    let mark = match level {
        Level::Ok => "✓",
        Level::Warn => "!",
        Level::Error => "✕",
    };
    view()
        .flex_row()
        .gap_3()
        .py_2()
        .child(
            text(mark)
                .font_weight(FontWeight::BOLD)
                .text_color(move || if level == Level::Ok { theme().foreground } else { theme().destructive }),
        )
        .child(view().flex_1().gap_1().child(text(title.into())).child(muted(detail)))
}

/// A bordered block of related rows.
pub(crate) fn card() -> View {
    view().gap_1().p_4().border_1().border_color(move || theme().border).rounded(move || theme().radius)
}

/// One choice among a few, as a row of buttons; the chosen one is filled.
pub(crate) fn choice<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
) -> View {
    let mut row = view().flex_row().gap_1();
    for &(value, label) in options {
        let on = move || current() == value;
        row = row.child(
            button()
                .px_3()
                .py_1()
                .border_1()
                .border_color(move || theme().border)
                .rounded(move || theme().radius)
                .bg(move || if on() { theme().primary } else { theme().background })
                .active(|s| s.opacity(0.7))
                .on_press(move || pick(value))
                .child(
                    text(label)
                        .text_sm()
                        .text_color(move || if on() { theme().primary_foreground } else { theme().foreground }),
                ),
        );
    }
    row
}

/// A labelled form row: what it is, what it does, and its control.
pub(crate) fn field(label: &str, help: &str, control: impl IntoElement) -> View {
    view()
        .flex_row()
        .gap_4()
        .py_2()
        .child(view().w(px(220.0)).gap_1().child(text(label.to_string())).child(muted(help.to_string())))
        .child(view().flex_1().justify_center().child(control))
}

/// A one-line text box bound to `value`.
pub(crate) fn text_box(value: Signal<String>, placeholder: &str) -> Input {
    input()
        .value(move || value.get())
        .on_change(move |v: &str| value.set(v.to_owned()))
        .placeholder(placeholder.to_string())
        .px_3()
        .py_1()
        .border_1()
        .border_color(move || theme().input)
        .rounded(move || theme().radius)
        .text_color(move || theme().foreground)
}

/// What the last action reported, with a way to dismiss it.
pub(crate) fn notice_area() -> AnyElement {
    use crate::state::{self, Notice};
    dynamic(move || {
        state::notice().map(|n| {
            let body = match n {
                Notice::Error(e) => card().child(check(Level::Error, "That did not work", e)),
                Notice::Done(msg) => card().child(check(Level::Ok, msg, "")),
                Notice::SetUp(done) => {
                    let mut c = card().child(text(format!("{} is set up", done.agent.label())).font_weight(FontWeight::MEDIUM));
                    for f in &done.changed {
                        c = c.child(muted(format!("Updated {f}")));
                    }
                    for m in &done.manual {
                        c = c.child(muted(format!("Next: {m}")));
                    }
                    c
                }
            };
            body.child(view().flex_row().pt_2().child(action("Dismiss", state::dismiss_notice)))
        })
    })
}
