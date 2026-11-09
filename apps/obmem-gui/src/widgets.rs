//! Small pieces the pages share, drawn with the theme, and how the layout follows the window.

use crate::sidebar;
use nsh_base::{button, input, theme};
use nsh_core::element::desktop::title_bar;
use nsh_core::prelude::*;

/// Below this window width the sidebar stops taking room and opens over the page instead.
pub(crate) const NARROW: f32 = 760.0;
/// Below this page width a form row puts its label above its control.
const STACK_FIELDS: f32 = 620.0;
/// The widest the page's column gets: lines stay readable in a wide window.
const PAGE_MAX: f32 = 920.0;

/// The window is too narrow to keep the sidebar beside the page.
pub(crate) fn narrow() -> bool {
    window().width.0 < NARROW
}

/// The width left for the page beside the sidebar.
fn page_width() -> f32 {
    (window().width.0 - sidebar::docked_width()).min(PAGE_MAX)
}

/// One of the app's own theme tokens (`theme.json`), else the text color.
pub(crate) fn tone(name: &'static str) -> impl Fn() -> AdaptiveColor + Copy {
    move || theme().color(name).unwrap_or(theme().foreground)
}

/// The strip along the window's top edge where the system's title bar was: it drags the window,
/// and the window's controls sit in it. Nothing where the system draws its own title bar.
pub(crate) fn title_strip() -> View {
    view().flex_shrink_0().h(move || title_bar().top).window_drag(true)
}

/// A page: its header, then its blocks, in a column that scrolls and stops growing in a wide
/// window.
pub(crate) fn page(header: impl IntoElement, body: impl IntoElement) -> impl IntoElement {
    view().flex_1().min_w(px(0.0)).child(title_strip()).child(
        scroll().flex_1().child(
            view()
                .w_full()
                .max_w(px(PAGE_MAX))
                .gap_4()
                .px_6()
                .pb_6()
                .pt_2()
                .style_when(narrow, |s| s.px_4().pb_4())
                .child(header)
                .child(body),
        ),
    )
}

/// A page's title, what it is about below it, and its actions, which move under the title when
/// they do not fit beside it.
pub(crate) fn page_header(title: impl Into<SharedString>, about: impl IntoElement, actions: Vec<AnyElement>) -> View {
    let mut buttons = view().flex_row().flex_wrap().gap_2();
    for a in actions {
        buttons = buttons.child(a);
    }
    view()
        .flex_row()
        .flex_wrap()
        .items_end()
        .gap_3()
        .pb_2()
        .child(
            view()
                .flex_1()
                .min_w(px(220.0))
                .gap_1()
                .child(text(title.into()).text_2xl().font_weight(FontWeight::SEMIBOLD))
                .child(about),
        )
        .child(buttons)
}

/// A section title.
pub(crate) fn heading(label: impl Into<SharedString>) -> Text {
    text(label.into()).text_sm().font_weight(FontWeight::SEMIBOLD)
}

/// Secondary text.
pub(crate) fn muted(label: impl Into<SharedString>) -> Text {
    text(label.into()).text_sm().text_color(move || theme().muted_foreground)
}

/// A file system path, in the monospaced face paths are typed in.
pub(crate) fn path_text(label: impl IntoReactive<SharedString>) -> Text {
    text(label).text_xs().font_family("monospace").text_color(move || theme().muted_foreground)
}

/// A bordered button for secondary actions.
pub(crate) fn action(label: impl Into<SharedString>, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .px_3()
        .py_1()
        .border_1()
        .border_color(move || theme().input)
        .rounded(move || theme().radius)
        .bg(move || theme().card)
        .hover(|s| s.bg(move || theme().muted))
        .active(|s| s.opacity(0.7))
        .on_press(on_press)
        .child(text(label.into()).text_sm())
}

/// The button for the main action of a block.
pub(crate) fn primary(label: impl Into<SharedString>, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .px_3()
        .py_1()
        .border_1()
        .border_color(move || theme().primary)
        .rounded(move || theme().radius)
        .bg(move || theme().primary)
        .hover(|s| s.opacity(0.88))
        .active(|s| s.opacity(0.7))
        .on_press(on_press)
        .child(text(label.into()).text_sm().font_weight(FontWeight::MEDIUM).text_color(move || theme().primary_foreground))
}

/// A square button holding an icon, for chrome such as the sidebar's.
pub(crate) fn icon_button(paths: &'static str, label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .w(px(28.0))
        .h(px(28.0))
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .text_color(move || theme().muted_foreground)
        .hover(|s| s.bg(move || theme().sidebar_accent).text_color(move || theme().foreground))
        .active(|s| s.opacity(0.7))
        .accessibility_label(label)
        .on_press(on_press)
        .child(crate::icons::icon(paths, 16.0))
        .with_hint(label)
}

/// Shows `label` above an element while the pointer rests on it.
pub(crate) trait WithHint: IntoElement + Sized {
    fn with_hint(self, label: impl Into<SharedString>) -> View {
        let label: SharedString = label.into();
        view().tooltip(
            nsh_base::tooltip()
                .px_2()
                .py_1()
                .rounded(move || theme().radius)
                .bg(move || theme().foreground)
                .child(text(label).text_xs().text_color(move || theme().background)),
        )
        .child(self)
    }
}

impl<T: IntoElement> WithHint for T {}

/// How a check came out.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Level {
    Ok,
    Warn,
    Error,
}

/// A check: its outcome, what it is, and what it found. Buttons added to it sit at its right,
/// level with the title.
pub(crate) fn check(level: Level, title: impl Into<SharedString>, detail: impl Into<SharedString>) -> View {
    let (mark, color) = match level {
        Level::Ok => ("✓", "ok"),
        Level::Warn => ("!", "warn"),
        Level::Error => ("✕", "destructive"),
    };
    let color = tone(color);
    view()
        .flex_row()
        .items_start()
        .gap_3()
        .py_2()
        .child(
            view()
                .w(px(18.0))
                .h(px(18.0))
                .mt(px(1.0))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(color)
                .child(text(mark).text_xs().font_weight(FontWeight::BOLD).text_color(color)),
        )
        .child({
            let detail: SharedString = detail.into();
            let body = view().flex_1().min_w(px(0.0)).gap_1().child(text(title.into()).font_weight(FontWeight::MEDIUM));
            if detail.is_empty() { body } else { body.child(muted(detail)) }
        })
}

/// A bordered block of related rows.
pub(crate) fn card() -> View {
    view()
        .gap_1()
        .p_4()
        .bg(move || theme().card)
        .border_1()
        .border_color(move || theme().border)
        .rounded(move || theme().radius)
}

/// A row of tabs over the page, the shown one underlined in ink. Tabs that do not fit wrap.
pub(crate) fn tabs<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
) -> View {
    let mut row = view().flex_row().flex_wrap().gap_x_4().border_b_1().border_b_color(move || theme().border);
    for &(value, label) in options {
        let on = move || current() == value;
        row = row.child(
            button()
                .py_2()
                .mb(px(-1.0))
                .border_b(px(2.0))
                .border_b_color(move || if on() { theme().primary } else { AdaptiveColor::TRANSPARENT })
                .hover(|s| s.opacity(0.8))
                .on_press(move || pick(value))
                .child(
                    text(label)
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if on() { theme().foreground } else { theme().muted_foreground }),
                ),
        );
    }
    row
}

/// One choice among a few, as a segmented control; the chosen one is filled.
pub(crate) fn choice<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
) -> View {
    let mut row = view()
        .flex_row()
        .self_start()
        .p(px(2.0))
        .gap(px(2.0))
        .rounded(move || theme().radius)
        .bg(move || theme().muted);
    for &(value, label) in options {
        let on = move || current() == value;
        row = row.child(
            button()
                .px_3()
                .py_1()
                .rounded(move || theme().radius - px(2.0))
                .bg(move || if on() { theme().card } else { AdaptiveColor::TRANSPARENT })
                .style_when(on, |s| s.shadow_xs())
                .active(|s| s.opacity(0.7))
                .on_press(move || pick(value))
                .child(
                    text(label)
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || if on() { theme().foreground } else { theme().muted_foreground }),
                ),
        );
    }
    row
}

/// A labelled form row: what it is, what it does, and its control. In a narrow page the label
/// goes above the control.
pub(crate) fn field(label: &str, help: &str, control: impl IntoElement) -> View {
    let stacked = move || page_width() < STACK_FIELDS;
    view()
        .flex_row()
        .gap_4()
        .py_3()
        .border_t_1()
        .border_t_color(move || theme().border)
        .style_when(stacked, |s| s.flex_col().gap_2())
        .child(
            view()
                .w(px(240.0))
                .flex_shrink_0()
                .gap_1()
                .style_when(stacked, |s| s.w_full())
                .child(text(label.to_string()).text_sm().font_weight(FontWeight::MEDIUM))
                .child(muted(help.to_string()).text_xs()),
        )
        .child(view().flex_1().min_w(px(0.0)).justify_center().child(control))
}

/// A one-line text box bound to `value`.
pub(crate) fn text_box(value: Signal<String>, placeholder: &str) -> Input {
    input()
        .value(move || value.get())
        .on_change(move |v: &str| value.set(v.to_owned()))
        .placeholder(placeholder.to_string())
        .px_3()
        .py_1()
        .bg(move || theme().background)
        .border_1()
        .border_color(move || theme().input)
        .rounded(move || theme().radius)
        .text_color(move || theme().foreground)
        .focus(|s| s.border_color(move || theme().ring))
}

/// What the last action reported, with a way to dismiss it.
pub(crate) fn notice_area() -> AnyElement {
    use crate::state::{self, Notice};
    dynamic(move || {
        state::notice().map(|n| {
            let (body, level) = match n {
                Notice::Error(e) => (card().child(check(Level::Error, "That did not work", e)), Level::Error),
                Notice::Done(msg) => (card().child(check(Level::Ok, msg, "")), Level::Ok),
                Notice::Lines(title, lines) => {
                    let mut c = card().child(check(Level::Ok, title, ""));
                    for l in lines {
                        c = c.child(muted(l).pl(px(30.0)));
                    }
                    (c, Level::Ok)
                }
                Notice::Audit(done) => {
                    let mut c = card().child(check(Level::Ok, done.summary, ""));
                    for n in done.notes {
                        c = c.child(muted(n).pl(px(30.0)));
                    }
                    (c, Level::Ok)
                }
                Notice::SetUp(done) => {
                    let mut c = card().child(check(Level::Ok, format!("{} is set up", done.agent.label()), ""));
                    for f in &done.changed {
                        c = c.child(muted(format!("Updated {f}")).pl(px(30.0)));
                    }
                    for m in &done.manual {
                        c = c.child(muted(format!("Next: {m}")).pl(px(30.0)));
                    }
                    (c, Level::Ok)
                }
            };
            let edge = tone(if level == Level::Error { "destructive" } else { "ok" });
            body.test_id("notice")
                .border_l_color(edge)
                .border_l(px(3.0))
                .child(view().flex_row().pt_2().pl(px(30.0)).child(action("Dismiss", state::dismiss_notice)))
        })
    })
}
