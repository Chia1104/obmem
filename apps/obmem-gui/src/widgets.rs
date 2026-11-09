//! Small pieces the pages share, drawn with the theme.

use nsh_base::{button, theme};
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
