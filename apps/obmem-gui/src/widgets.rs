//! Small pieces the pages share, drawn with the theme, and how the layout follows the window.
//!
//! A piece that shows text takes it as `impl IntoReactive<SharedString>`: `tr!("key")` for the
//! app's own words, which then follow the language, or a `String` for what is data (a name, a
//! path, what a library reported).

use crate::motion::{Eased, Enters, pressed, pressed_icon};
use crate::words::tr;
use nsh_base::{button, input, theme};
use nsh_core::prelude::*;
use std::rc::Rc;

/// Below this window width the sidebar stops taking room and opens over the page instead.
pub(crate) const NARROW: f32 = 760.0;
/// The widest the page's column gets: lines stay readable in a wide window, where the column
/// stands in the middle of the panel.
const PAGE_MAX: f32 = 1120.0;

/// The window is too narrow to keep the sidebar beside the page.
pub(crate) fn narrow() -> bool {
    window().width.0 < NARROW
}

/// One of the app's own theme tokens (`theme.json`), else the text color.
pub(crate) fn tone(name: &'static str) -> impl Fn() -> AdaptiveColor + Copy {
    move || theme().color(name).unwrap_or(theme().foreground)
}

/// How far the page's panel stands in from the window's edges.
const INSET: f32 = 8.0;

/// A page: one panel set into the window beside the sidebar, under the strip along the window's
/// top edge (`sidebar::top_strip`). Its `bar` stays at the top
/// ([`page_bar`], and a project's tabs under it); its `body` scrolls below, in a column that stops
/// growing in a wide window, where it and the bar stand in the middle, and rises in when the page
/// is opened.
pub(crate) fn page(bar: impl IntoElement, body: impl IntoElement) -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .pr(px(INSET))
        .pb(px(INSET))
        .child(
            view()
                .flex_1()
                .overflow_hidden()
                .rounded(move || theme().radius + px(4.0))
                // The user may let the panel show some of what is behind it, which it then
                // blurs, or have no panel: the page keeps its place, without ground or edge.
                .border_1()
                .border_color(move || {
                    if crate::state::panel_shows() {
                        theme().border
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .bg(move || theme().background.opacity(crate::state::panel_cover()))
                .style_when(
                    || (0.01..1.0).contains(&crate::state::panel_cover()),
                    |s| s.backdrop_blur(px(24.0)),
                )
                // The bar is as wide as the column under it and stands over it; the strip around
                // it drags the window across the panel's whole width.
                .child(
                    view()
                        .flex_shrink_0()
                        .window_drag(true)
                        .child(view().w_full().max_w(px(PAGE_MAX)).self_center().child(bar)),
                )
                .child(
                    scroll().flex_1().child(
                        view()
                            .w_full()
                            .max_w(px(PAGE_MAX))
                            .self_center()
                            .gap_4()
                            .px_6()
                            .pb_6()
                            .pt_1()
                            .style_when(narrow, |s| s.px_4().pb_4())
                            .child(body)
                            .rises(),
                    ),
                ),
        )
}

/// The top row of a page's panel: its title, what stands beside the title (a project's folder),
/// and its actions at the right.
pub(crate) fn page_bar(
    title: impl IntoReactive<SharedString>,
    beside: impl IntoElement,
    actions: Vec<AnyElement>,
) -> View {
    let mut buttons = view().flex_row().items_center().gap_1();
    for a in actions {
        buttons = buttons.child(a);
    }
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(52.0))
        .pl_6()
        .pr_3()
        .style_when(narrow, |s| s.pl_4())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_baseline()
                .gap_3()
                .child(
                    text(title)
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .number_of_lines(1u32),
                )
                .child(view().flex_1().min_w(px(0.0)).child(beside)),
        )
        .child(buttons)
}

/// What a page is about, said once above its blocks.
pub(crate) fn about(label: impl IntoReactive<SharedString>) -> Text {
    muted(label)
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
            .presses()
            .hover(|s| {
                if main {
                    s.opacity(0.88)
                } else {
                    s.bg(move || theme().muted)
                }
            })
            .active(pressed)
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

/// A square button holding an icon, named by a hint: for the sidebar and a page's bar. While its
/// own work runs it spins and takes no presses ([`IconPress::busy`]).
pub(crate) struct IconPress {
    paths: &'static str,
    label: Reactive<SharedString>,
    on_press: Box<dyn Fn()>,
    busy: Rc<dyn Fn() -> bool>,
    washes: bool,
}

pub(crate) fn icon_button(
    paths: &'static str,
    label: impl IntoReactive<SharedString>,
    on_press: impl Fn() + 'static,
) -> IconPress {
    IconPress {
        paths,
        label: label.into_reactive(),
        on_press: Box::new(on_press),
        busy: Rc::new(|| false),
        washes: true,
    }
}

impl IconPress {
    /// For a button that stands on a material of its own (the sidebar's, on glass): under the
    /// pointer only its icon changes, with no wash behind it.
    pub(crate) fn on_material(mut self) -> Self {
        self.washes = false;
        self
    }

    /// Whether the work it starts is running (`state::running`).
    pub(crate) fn busy(mut self, busy: impl Fn() -> bool + 'static) -> Self {
        self.busy = Rc::new(busy);
        self
    }
}

impl IntoElement for IconPress {
    fn into_element(self) -> AnyElement {
        let IconPress {
            paths,
            label,
            on_press,
            busy,
            washes,
        } = self;
        let ink = move || theme().muted_foreground;
        let spins = busy.clone();
        button()
            .w(px(28.0))
            .h(px(28.0))
            .items_center()
            .justify_center()
            .rounded(move || theme().radius)
            .text_color(ink)
            .presses()
            // A wash of the text color: the button sits on the sidebar and on the page alike.
            .hover(move |s| {
                let s = s.text_color(move || theme().foreground);
                if washes {
                    s.bg(move || theme().foreground.opacity(0.08))
                } else {
                    s
                }
            })
            .active(pressed_icon)
            .disabled(move || busy())
            .accessibility_label({
                let label = label.clone();
                move || Some(label.get())
            })
            .on_press(on_press)
            .child(dynamic(move || {
                if spins() {
                    spinner(ink).into_element()
                } else {
                    crate::icons::icon(paths, 16.0).into_element()
                }
            }))
            .with_hint(label)
            .into_element()
    }
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

/// A row of tabs under a page's bar (NSH's `Tabs`): the shown one stands on a pill, which travels
/// to the next one. Tabs that do not fit wrap. Each option is a value and the translation key of
/// its label.
pub(crate) fn tabs<T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    current: impl Fn() -> T + Copy + 'static,
    pick: impl Fn(T) + Copy + 'static,
) -> impl IntoElement {
    let mut list = nsh_base::tabs()
        .indicator_class(Some("top-0 bottom-0 rounded-full bg-secondary".into()))
        .flex_row()
        .flex_wrap()
        .gap_1();
    for &(value, label) in options {
        let on = move || current() == value;
        list = list.child(
            nsh_base::tab()
                .selected(on)
                .px_3()
                .py_1()
                .rounded_full()
                .washes()
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
    // The pill spans its list, so the room around the tabs is a view's around the list. The
    // first pill then starts where the page's title does.
    view()
        .px_6()
        .pb_3()
        .style_when(narrow, |s| s.px_4())
        .child(list)
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
                .presses()
                .active(pressed)
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
                .washes()
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

/// A row of a card's settings: what it is and what it does at the left, and its control at the
/// right. For a control that is small: a switch, a choice, a list, a button. Where the two do
/// not fit side by side the control goes under the words.
pub(crate) fn field(
    label: impl IntoReactive<SharedString>,
    help: impl IntoReactive<SharedString>,
    control: impl IntoElement,
) -> View {
    row()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x_6()
        .gap_y_2()
        .child(words(label, help).flex_1().min_w(px(220.0)))
        .child(view().flex_shrink_0().child(control))
}

/// A row of a card's settings whose control is a text box: what it is and what it does, then
/// the box under them, as wide as the card.
pub(crate) fn input_field(
    label: impl IntoReactive<SharedString>,
    help: impl IntoReactive<SharedString>,
    control: impl IntoElement,
) -> View {
    checked_field(label, help, control, || None)
}

/// An [`input_field`] whose value has rules: the one it breaks (`form::Problems::at`) shows under
/// the box, which marks itself too ([`Marked`]).
pub(crate) fn checked_field(
    label: impl IntoReactive<SharedString>,
    help: impl IntoReactive<SharedString>,
    control: impl IntoElement,
    problem: impl Fn() -> Option<String> + 'static,
) -> View {
    row()
        .gap_2()
        .child(words(label, help))
        .child(control)
        .child(problem_line(problem))
}

/// A row of a card's settings, parted from the row above it by a hairline.
fn row() -> View {
    view()
        .py_3()
        .border_t_1()
        .border_t_color(move || theme().border)
}

/// What a row is and what it does.
fn words(label: impl IntoReactive<SharedString>, help: impl IntoReactive<SharedString>) -> View {
    view()
        .gap_1()
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
        .child(muted(help).text_xs())
}

/// The first of a card's rows has nothing above it to be parted from.
pub(crate) trait FirstRow {
    fn first_row(self) -> Self;
}

impl FirstRow for View {
    fn first_row(self) -> Self {
        self.border_t(px(0.0)).pt_1()
    }
}

/// The rule a value breaks, said under its control while it breaks one. The line fades in when
/// there is a rule to say, and follows what is typed after that without coming in again.
pub(crate) fn problem_line(problem: impl Fn() -> Option<String> + 'static) -> AnyElement {
    let problem = Rc::new(problem);
    let broken = computed({
        let problem = problem.clone();
        move || problem().is_some()
    });
    dynamic(move || {
        broken.get().then(|| {
            let problem = problem.clone();
            text(move || problem().unwrap_or_default())
                .text_xs()
                .text_color(move || theme().destructive)
                .fades_in()
        })
    })
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

/// The veil over the window behind a dialog or the drawer, as a Tailwind class.
pub(crate) const SCRIM: &str = "bg-background/70";

/// A dialog over the window (NSH's `Dialog`), for a flow done once or a decision to confirm: its
/// title, what it shows, and its buttons at the bottom right. Put it in a region that holds it
/// only while it is open. Its ✕, a press outside it and Escape call `close`. `id` is what tests
/// find its panel by.
pub(crate) fn dialog_box(
    id: &str,
    title: impl IntoReactive<SharedString>,
    width: f32,
    body: impl IntoElement,
    buttons: Vec<AnyElement>,
    close: impl Fn() + Clone + 'static,
) -> AnyElement {
    let title = title.into_reactive();
    let mut foot = view()
        .flex_row()
        .flex_wrap()
        .justify_end()
        .gap_2()
        .px_5()
        .pb_5();
    for b in buttons {
        foot = foot.child(b);
    }
    let panel = view()
        .test_id(id)
        .w_full()
        .max_w(px(width))
        // The dialog stands in from the window's edges by the padding around it.
        .max_h(move || px(window().height.0 - 48.0))
        .overflow_hidden()
        .rounded(move || theme().radius + px(4.0))
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .shadow_xl()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .pl_5()
                .pr_3()
                .pt_4()
                .pb_2()
                .child(
                    text(title.clone())
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(icon_button(
                    crate::icons::X,
                    tr!("common.close"),
                    close.clone(),
                )),
        )
        .child(
            scroll()
                .flex_shrink(1.0)
                .child(view().gap_3().px_5().pb_5().child(body)),
        )
        .child(foot)
        .rises();
    nsh_base::dialog()
        .animation(nsh_core::ModalAnimation::Fade)
        .label(title)
        .on_cancel(close.clone())
        .child(
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .p_6()
                .child(
                    view()
                        .absolute()
                        .inset_0()
                        .app_class(Some(SharedString::from(SCRIM)))
                        .on_press(close),
                )
                .child(panel),
        )
        .into_element()
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
            callout(tone(Level::Ok.token()), body)
                .test_id("notice")
                .rises()
        })
    })
}
