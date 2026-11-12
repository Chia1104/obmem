//! Small pieces the pages share, drawn with the theme, and how the layout follows the window.
//!
//! A piece that shows text takes it as `impl IntoReactive<SharedString>`: `tr!("key")` for the
//! app's own words, which then follow the language, or a `String` for what is data (a name, a
//! path, what a library reported).

use crate::sidebar;
use crate::words::tr;
use nsh_base::{button, input, theme};
use nsh_core::element::desktop::title_bar;
use nsh_core::prelude::*;
use std::rc::Rc;

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
    view()
        .flex_shrink_0()
        .h(move || title_bar().top)
        .window_drag(true)
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
pub(crate) fn page_header(
    title: impl IntoReactive<SharedString>,
    about: impl IntoElement,
    actions: Vec<AnyElement>,
) -> View {
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
                .child(text(title).text_2xl().font_weight(FontWeight::SEMIBOLD))
                .child(about),
        )
        .child(buttons)
}

/// A section title.
pub(crate) fn heading(label: impl IntoReactive<SharedString>) -> Text {
    text(label).text_sm().font_weight(FontWeight::SEMIBOLD)
}

/// Secondary text.
pub(crate) fn muted(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_sm()
        .text_color(move || theme().muted_foreground)
}

/// A file system path, in the monospaced face paths are typed in.
pub(crate) fn path_text(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_xs()
        .font_family("monospace")
        .text_color(move || theme().muted_foreground)
}

/// A button for an action: bordered for a secondary one ([`action`]), filled for the main one of
/// a block ([`primary`]). While its own work runs it spins and takes no presses.
pub(crate) struct Press {
    label: Reactive<SharedString>,
    main: bool,
    on_press: Box<dyn Fn()>,
    busy: Rc<dyn Fn() -> bool>,
    blocked: Rc<dyn Fn() -> bool>,
}

fn press(
    label: impl IntoReactive<SharedString>,
    main: bool,
    on_press: impl Fn() + 'static,
) -> Press {
    Press {
        label: label.into_reactive(),
        main,
        on_press: Box::new(on_press),
        busy: Rc::new(|| false),
        blocked: Rc::new(|| false),
    }
}

/// A bordered button for secondary actions.
pub(crate) fn action(
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> Press {
    press(label, false, on_press)
}

/// The button for the main action of a block.
pub(crate) fn primary(
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> Press {
    press(label, true, on_press)
}

impl Press {
    /// Whether the work it starts is running: it then spins and takes no presses, so the work
    /// cannot be started twice (`state::running`).
    pub(crate) fn busy(mut self, busy: impl Fn() -> bool + 'static) -> Self {
        self.busy = Rc::new(busy);
        self
    }

    /// Whether other work that rules this one out is running: it then takes no presses.
    pub(crate) fn blocked(mut self, blocked: impl Fn() -> bool + 'static) -> Self {
        self.blocked = Rc::new(blocked);
        self
    }
}

impl IntoElement for Press {
    fn into_element(self) -> AnyElement {
        let Press {
            label,
            main,
            on_press,
            busy,
            blocked,
        } = self;
        let off = {
            let busy = busy.clone();
            move || busy() || blocked()
        };
        let ink = move || {
            if main {
                theme().primary_foreground
            } else {
                theme().foreground
            }
        };
        let label = text(label).text_sm().text_color(ink);
        let label = if main {
            label.font_weight(FontWeight::MEDIUM)
        } else {
            label
        };
        let spins = busy.clone();
        button()
            .relative()
            .items_center()
            .justify_center()
            .px_3()
            .py_1()
            .border_1()
            .border_color(move || if main { theme().primary } else { theme().input })
            .rounded(move || theme().radius)
            .bg(move || if main { theme().primary } else { theme().card })
            .hover(|s| {
                if main {
                    s.opacity(0.88)
                } else {
                    s.bg(move || theme().muted)
                }
            })
            .active(|s| s.opacity(0.7))
            .disabled(off)
            .disabled_style(|s| s.opacity(0.6))
            .on_press(on_press)
            // The spinner takes the label's place, so the button keeps its size and nothing
            // beside it moves; the label stays there for screen readers.
            .child(label.opacity(move || if busy() { 0.0_f32 } else { 1.0 }))
            .child(dynamic(move || {
                spins().then(|| {
                    view()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .child(spinner(ink))
                })
            }))
            .into_element()
    }
}

/// An open ring that turns while work runs, in `color`.
pub(crate) fn spinner(color: impl Fn() -> AdaptiveColor + Copy + 'static) -> View {
    view()
        .flex_shrink_0()
        .text_color(color)
        .animate_spin()
        .child(crate::icons::icon(crate::icons::LOADER, 14.0))
}

/// What shows while something is read: a spinner and what is being read.
pub(crate) fn loading(label: impl IntoReactive<SharedString>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(spinner(move || theme().muted_foreground))
        .child(muted(label))
}

/// A square button holding an icon, for chrome such as the sidebar's.
pub(crate) fn icon_button(
    paths: &'static str,
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let label = label.into_reactive();
    button()
        .w(px(28.0))
        .h(px(28.0))
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .text_color(move || theme().muted_foreground)
        .hover(|s| {
            s.bg(move || theme().sidebar_accent)
                .text_color(move || theme().foreground)
        })
        .active(|s| s.opacity(0.7))
        .accessibility_label({
            let label = label.clone();
            move || Some(label.get())
        })
        .on_press(on_press)
        .child(crate::icons::icon(paths, 16.0))
        .with_hint(label)
}

/// Shows `label` above an element while the pointer rests on it.
pub(crate) trait WithHint: IntoElement + Sized {
    fn with_hint(self, label: impl IntoReactive<SharedString>) -> View {
        let label = label.into_reactive();
        view()
            .tooltip(
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

impl Level {
    /// The theme token of its color.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Error => "destructive",
        }
    }
}

/// The round mark of an outcome.
pub(crate) fn mark(level: Level) -> View {
    let sign = match level {
        Level::Ok => "✓",
        Level::Warn => "!",
        Level::Error => "✕",
    };
    let color = tone(level.token());
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
        .child(
            text(sign)
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(color),
        )
}

/// A check: its outcome, what it is, and what it found. Buttons added to it sit at its right,
/// level with the title.
pub(crate) fn check(
    level: Level,
    title: impl IntoReactive<SharedString>,
    detail: impl IntoReactive<SharedString>,
) -> View {
    view()
        .flex_row()
        .items_start()
        .gap_3()
        .py_2()
        .child(mark(level))
        .child({
            let body = view()
                .flex_1()
                .min_w(px(0.0))
                .gap_1()
                .child(text(title).font_weight(FontWeight::MEDIUM));
            match detail.into_reactive() {
                Reactive::Static(detail) if detail.is_empty() => body,
                detail => body.child(muted(detail)),
            }
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

/// A row of tabs over the page (NSH's `Tabs`): the shown one is underlined in ink, and the
/// underline travels to the next one. Tabs that do not fit wrap. Each option is a value and the
/// translation key of its label.
pub(crate) fn tabs<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
) -> impl IntoElement {
    let mut list = nsh_base::tabs()
        .indicator_class(Some("bottom-0 h-[2px] rounded-full bg-primary".into()))
        .flex_row()
        .flex_wrap()
        .gap_x_4()
        .border_b_1()
        .border_b_color(move || theme().border);
    for &(value, label) in options {
        let on = move || current() == value;
        list = list.child(
            nsh_base::tab()
                .selected(on)
                .py_2()
                .hover(|s| s.opacity(0.8))
                .on_press(move || pick(value))
                .child(
                    text(move || t!(label))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || {
                            if on() {
                                theme().foreground
                            } else {
                                theme().muted_foreground
                            }
                        }),
                ),
        );
    }
    list
}

/// One choice among a few, as a segmented control of NSH `Toggle`s; the chosen one is raised.
/// Each option is a value and the translation key of its label.
pub(crate) fn choice<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
) -> impl IntoElement {
    let mut group = nsh_base::toggle_group()
        .flex_row()
        .self_start()
        .p(px(2.0))
        .gap(px(2.0))
        .rounded(move || theme().radius)
        .bg(move || theme().muted);
    for &(value, label) in options {
        let on = move || current() == value;
        group = group.child(
            nsh_base::toggle()
                .pressed(on)
                .px_3()
                .py_1()
                .rounded(move || theme().radius - px(2.0))
                .bg(move || {
                    if on() {
                        theme().card
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .style_when(on, |s| s.shadow_xs())
                .active(|s| s.opacity(0.7))
                // Pressing the chosen one again keeps it: one of them is always chosen.
                .on_change(move |_: &bool| pick(value))
                .child(
                    text(move || t!(label))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || {
                            if on() {
                                theme().foreground
                            } else {
                                theme().muted_foreground
                            }
                        }),
                ),
        );
    }
    group
}

/// One choice among several, as a list that opens from its trigger (NSH's `Select`); the trigger
/// shows the chosen one. Each option is a value and the translation key of its label, and `name`
/// is the key of what screen readers call the control.
pub(crate) fn select<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
    name: &'static str,
) -> impl IntoElement {
    // An item's value is its place in `options`.
    let options: Rc<[(T, &'static str)]> = options.into();
    let chosen = {
        let options = options.clone();
        move || options.iter().position(|(value, _)| *value == current())
    };
    let label = {
        let (options, chosen) = (options.clone(), chosen.clone());
        move || chosen().map(|i| t!(options[i].1)).unwrap_or_default()
    };
    let items = {
        let options = options.clone();
        move || {
            options
                .iter()
                .enumerate()
                .map(|(i, (_, key))| nsh_base::SelectItem::new(i.to_string(), t!(*key)))
                .collect::<Vec<_>>()
        }
    };
    nsh_base::select()
        .items(items)
        .value(move || {
            nsh_base::SelectValue::from(chosen().map(|i| SharedString::from(i.to_string())))
        })
        .accessibility_label(crate::words::named(name))
        .on_change(move |value: &Option<SharedString>| {
            let picked = value.as_ref().and_then(|v| v.parse::<usize>().ok());
            if let Some((value, _)) = picked.and_then(|i| options.get(i)) {
                pick(*value);
            }
        })
        .self_start()
        .popup_class("p-1 rounded-md border border-border bg-card shadow-md")
        .item_class(
            "px-3 py-1 rounded-sm text-sm text-muted-foreground selected:text-foreground \
             selected:font-medium focus:bg-muted focus:text-foreground",
        )
        .trigger(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .min_w(px(180.0))
                .px_3()
                .py_1()
                .border_1()
                .border_color(move || theme().input)
                .rounded(move || theme().radius)
                .bg(move || theme().card)
                .hover(|s| s.bg(move || theme().muted))
                .focus(|s| s.border_color(move || theme().ring))
                .child(text(label).text_sm())
                .child(
                    view()
                        .text_color(move || theme().muted_foreground)
                        .child(crate::icons::icon(crate::icons::CHEVRONS, 14.0)),
                ),
        )
}

/// An on/off setting that takes effect when the form is saved or at once, as `change` does
/// (NSH's `Switch`).
pub(crate) fn switch_control(
    on: impl Fn() -> bool + Copy + 'static,
    label: impl IntoReactive<SharedString>,
    change: impl Fn(bool) + 'static,
) -> impl IntoElement {
    let label = label.into_reactive();
    nsh_base::switch()
        .checked(on)
        .accessibility_label(move || Some(label.get()))
        .self_start()
        .flex_row()
        .items_center()
        .gap_2()
        .on_change(move |v: &bool| change(*v))
        .track_class(Some("w-9 h-5 p-[2px] rounded-full bg-input checked:bg-primary transition-colors".into()))
        .thumb_class(Some("w-4 h-4 rounded-full bg-background shadow-xs checked:translate-x-4 transition-transform".into()))
        .child(text(move || if on() { t!("common.switched_on") } else { t!("common.switched_off") }).text_sm().text_color(move || theme().muted_foreground))
}

/// A box to tick (NSH's `Checkbox`) with its label after it.
pub(crate) fn checkbox_control(
    on: impl Fn() -> bool + Clone + 'static,
    change: impl Fn(bool) + 'static,
    label: impl IntoElement,
) -> impl IntoElement {
    let shown = on.clone();
    nsh_base::checkbox()
        .checked(on)
        .flex_row()
        .items_start()
        .gap_3()
        .on_change(move |v: &bool| change(*v))
        .indicator_class(Some(
            "mt-[2px] w-4 h-4 rounded-[4px] border border-input bg-background items-center justify-center checked:bg-primary checked:border-primary"
                .into(),
        ))
        .indicator(
            dynamic(move || shown().then(|| view().text_color(move || theme().primary_foreground).child(crate::icons::icon(crate::icons::CHECK, 12.0)))),
        )
        .child(view().flex_1().min_w(px(0.0)).child(label))
}

/// A labelled form row: what it is, what it does, and its control. In a narrow page the label
/// goes above the control.
pub(crate) fn field(
    label: impl IntoReactive<SharedString>,
    help: impl IntoReactive<SharedString>,
    control: impl IntoElement,
) -> View {
    checked_field(label, help, control, || None)
}

/// A [`field`] whose value has rules: the one it breaks (`form::Problems::at`) shows under the
/// control, which marks itself too ([`Marked`]).
pub(crate) fn checked_field(
    label: impl IntoReactive<SharedString>,
    help: impl IntoReactive<SharedString>,
    control: impl IntoElement,
    problem: impl Fn() -> Option<String> + 'static,
) -> View {
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
                .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
                .child(muted(help).text_xs()),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .justify_center()
                .gap_1()
                .child(control)
                .child(dynamic(move || problem().map(problem_text))),
        )
}

/// A rule a value breaks, said under its control.
pub(crate) fn problem_text(message: String) -> Text {
    text(message)
        .text_xs()
        .text_color(move || theme().destructive)
}

/// A one-line text box bound to `value`.
pub(crate) fn text_box(value: Signal<String>, placeholder: impl IntoReactive<String>) -> Input {
    input()
        .value(move || value.get())
        .on_change(move |v: &str| value.set(v.to_owned()))
        .placeholder(placeholder)
        .px_3()
        .py_1()
        .bg(move || theme().background)
        .border_1()
        .border_color(move || theme().input)
        .rounded(move || theme().radius)
        .text_color(move || theme().foreground)
        .focus(|s| s.border_color(move || theme().ring))
}

/// A text box whose value has rules: while it breaks one its border says so, in focus too.
pub(crate) trait Marked {
    fn marked(self, bad: impl Fn() -> bool + Copy + 'static) -> Self;
}

impl Marked for Input {
    fn marked(self, bad: impl Fn() -> bool + Copy + 'static) -> Self {
        self.border_color(move || {
            if bad() {
                theme().destructive
            } else {
                theme().input
            }
        })
        .focus(move |s| {
            s.border_color(move || {
                if bad() {
                    theme().destructive
                } else {
                    theme().ring
                }
            })
        })
    }
}

/// A card with a slim bar of `color` along the inside of its left edge, for what stands out from
/// the page (notices). The bar and what follows it sit close to that edge.
pub(crate) fn callout(
    color: impl Fn() -> AdaptiveColor + Copy + 'static,
    body: impl IntoElement,
) -> View {
    card()
        .flex_row()
        .gap_2()
        .pl_2()
        .child(
            view()
                .w(px(3.0))
                .self_stretch()
                .rounded_full()
                .bg(move || color().opacity(0.55)),
        )
        .child(view().flex_1().min_w(px(0.0)).gap_1().child(body))
}

/// What the last action reported, when that is more than a line: with a way to dismiss it.
/// One-line outcomes and failures are toasts (`crate::toasts`).
pub(crate) fn notice_area() -> AnyElement {
    use crate::state::{self, Notice};
    dynamic(move || {
        state::notice().map(|Notice { title, lines }| {
            let mut body = view().child(check(Level::Ok, title, "").py_0());
            for l in lines {
                body = body.child(muted(l).pl(px(30.0)));
            }
            let body = body.child(
                view()
                    .flex_row()
                    .pt_3()
                    .pl(px(30.0))
                    .child(action(tr!("common.dismiss"), state::dismiss_notice)),
            );
            callout(tone(Level::Ok.token()), body).test_id("notice")
        })
    })
}
