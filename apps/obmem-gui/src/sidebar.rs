//! The project list: every project obmem has seen, adding one by hand, and the other screens.
//!
//! The sidebar is resized by dragging its edge and collapses to a rail of project initials, by
//! its button or by dragging it narrower than [`SNAP`]. Its width and whether it is collapsed are
//! kept between runs. In a narrow window ([`widgets::narrow`]) it stays a rail, and the full list
//! opens over the page as a drawer.

use crate::data::ProjectRow;
use crate::icons::{self, INBOX, PERSON, PLUS, SIDEBAR};
use crate::state::{self, Load, Screen};
use crate::widgets::{self, WithHint, icon_button, muted, path_text};
use nsh_base::{HandleEdge, ResizeHandleState, button, resize_handle, theme};
use nsh_core::geometry::Axis;
use nsh_core::element::desktop::title_bar;
use nsh_core::prelude::*;
use nsh_core::storage::Storage;

const MIN: f32 = 200.0;
const MAX: f32 = 420.0;
const DEFAULT: f32 = 264.0;
/// The rail's width, unless the window's controls need more (macOS's traffic lights sit over it).
const RAIL: f32 = 56.0;

fn rail_width() -> f32 {
    RAIL.max(title_bar().left.0 + 8.0)
}
/// Dragged narrower than this, the sidebar collapses to the rail; the rail dragged wider than
/// this opens again.
const SNAP: f32 = 140.0;

const WIDTH_KEY: &str = "sidebar.width";
const COLLAPSED_KEY: &str = "sidebar.collapsed";

/// The width the user gave the full sidebar.
#[derive(Clone)]
struct Width(f32);
impl Default for Width {
    fn default() -> Self {
        let saved = Storage::get_item(WIDTH_KEY).and_then(|w| w.parse::<f32>().ok());
        Width(saved.unwrap_or(DEFAULT).clamp(MIN, MAX))
    }
}

#[derive(Clone)]
struct Collapsed(bool);
impl Default for Collapsed {
    fn default() -> Self {
        Collapsed(Storage::get_item(COLLAPSED_KEY).is_some_and(|v| v == "true"))
    }
}

/// The width a drag of the edge has reached so far, before it is kept.
#[derive(Clone, Default)]
struct Drag(Option<(f32, f32)>);

/// The drawer over the page in a narrow window is open.
#[derive(Clone, Default)]
struct Drawer(bool);

/// How the sidebar shows right now.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Rail,
    Full(f32),
}

fn shape() -> Shape {
    if widgets::narrow() {
        return Shape::Rail;
    }
    if let Some((_, raw)) = global::<Drag>().with(|d| d.0) {
        return if raw < SNAP { Shape::Rail } else { Shape::Full(raw.clamp(MIN, MAX)) };
    }
    if global::<Collapsed>().with(|c| c.0) { Shape::Rail } else { Shape::Full(global::<Width>().with(|w| w.0)) }
}

/// The width the sidebar takes beside the page.
pub(crate) fn docked_width() -> f32 {
    match shape() {
        Shape::Rail => rail_width(),
        Shape::Full(w) => w,
    }
}

fn set_collapsed(on: bool) {
    global::<Collapsed>().set(Collapsed(on));
    let _ = Storage::set_item(COLLAPSED_KEY, on.to_string());
}

/// The sidebar's button: collapses and expands it, or opens the drawer in a narrow window.
fn toggle() {
    if widgets::narrow() {
        global::<Drawer>().update(|d| d.0 = !d.0);
    } else {
        set_collapsed(!global::<Collapsed>().get_untracked().0);
    }
}

fn close_drawer() {
    global::<Drawer>().set(Drawer(false));
}

/// Opens a screen from the sidebar, closing the drawer it may be in.
fn go(f: impl Fn()) -> impl Fn() {
    move || {
        close_drawer();
        f();
    }
}

pub(crate) fn sidebar() -> impl IntoElement {
    let edge = signal(ResizeHandleState::Idle);
    view()
        .relative()
        .flex_shrink_0()
        .w(move || px(docked_width()))
        .bg(move || theme().sidebar)
        .child(widgets::title_strip())
        .child(dynamic(move || match shape() {
            Shape::Rail => rail().into_element(),
            Shape::Full(_) => panel(false).into_element(),
        }))
        .child(
            resize_handle()
                .axis(Axis::Horizontal)
                .edge(HandleEdge::Trailing)
                .disabled(widgets::narrow)
                .accessibility_label("Sidebar width")
                .on_state_change(move |s: &ResizeHandleState| edge.set(*s))
                .on_drag_start(move || {
                    let from = docked_width();
                    global::<Drag>().set(Drag(Some((from, from))));
                })
                .on_drag_move(move |offset: &Points| {
                    global::<Drag>().update(|d| {
                        if let Some((from, raw)) = &mut d.0 {
                            *raw = *from + offset.0;
                        }
                    });
                })
                .on_drag_end(move |offset: &Points| {
                    let Some((from, _)) = global::<Drag>().get_untracked().0 else { return };
                    global::<Drag>().set(Drag(None));
                    let raw = from + offset.0;
                    if raw < SNAP {
                        set_collapsed(true);
                    } else {
                        let w = raw.clamp(MIN, MAX);
                        global::<Width>().set(Width(w));
                        let _ = Storage::set_item(WIDTH_KEY, w.to_string());
                        set_collapsed(false);
                    }
                })
                // The edge's line: a hairline, inked while the pointer is on it.
                .child(view().w(move || px(if edge.get() == ResizeHandleState::Idle { 1.0 } else { 2.0 })).bg(move || {
                    if edge.get() == ResizeHandleState::Idle { theme().sidebar_border } else { theme().primary }
                })),
        )
}

/// The full list over the page, while it is open in a narrow window, with a scrim behind it that
/// closes it.
pub(crate) fn drawer() -> AnyElement {
    dynamic(move || {
        (widgets::narrow() && global::<Drawer>().with(|d| d.0)).then(|| {
            view()
                .absolute()
                .inset_0()
                .flex_row()
                .z_index(10)
                .child(
                    view()
                        .w(px(DEFAULT))
                        .h_full()
                        .bg(move || theme().sidebar)
                        .border_r_1()
                        .border_r_color(move || theme().sidebar_border)
                        .shadow_xl()
                        .child(widgets::title_strip())
                        .child(panel(true)),
                )
                .child(button().flex_1().bg(move || theme().foreground.opacity(0.18)).on_press(close_drawer).accessibility_label("Close the project list"))
        })
    })
}

/// The full sidebar: the projects, then the other screens.
fn panel(in_drawer: bool) -> View {
    view()
        .flex_1()
        .gap_2()
        .pt_1()
        .pb_3()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .pl_4()
                .pr_2()
                .child(text("Projects").text_sm().font_weight(FontWeight::SEMIBOLD).flex_1())
                .child(icon_button(PLUS, "Add a project…", go(state::add_project)))
                .child(icon_button(SIDEBAR, if in_drawer { "Close the project list" } else { "Collapse the sidebar" }, toggle)),
        )
        .child(
            scroll()
                .flex_1()
                .child(
                    view()
                        .gap(px(2.0))
                        .px_2()
                        .child(dynamic(move || match state::projects() {
                            Load::Loading => Some(muted("Loading…").px_2()),
                            Load::Failed(e) => Some(muted(e).px_2()),
                            Load::Ready(rows) if rows.is_empty() => Some(
                                muted("No projects yet. A project shows up here once an agent session starts in it, or add one with +.")
                                    .px_2(),
                            ),
                            Load::Ready(_) => None,
                        }))
                        .child(each(rows, |r: &ProjectRow| r.path.clone(), |row, _| project_row(row))),
                ),
        )
        .child(
            view()
                .gap(px(2.0))
                .px_2()
                .pt_2()
                .border_t_1()
                .border_t_color(move || theme().sidebar_border)
                .child(screen_entry(INBOX, "Review", Screen::Review, state::show_review))
                .child(screen_entry(PERSON, "Personal settings", Screen::Personal, state::show_personal)),
        )
}

/// The background of an entry: the page's own color when it is the one shown, as if the page were
/// pulled out from under it.
fn entry_bg(chosen: impl Fn() -> bool + Copy + 'static) -> impl Fn() -> AdaptiveColor {
    move || if chosen() { theme().background } else { AdaptiveColor::TRANSPARENT }
}

fn screen_id(label: &str) -> String {
    format!("screen-{}", label.to_lowercase().replace(' ', "-"))
}

/// An entry for a screen that is not a project.
fn screen_entry(paths: &'static str, label: &'static str, screen: Screen, show: fn()) -> impl IntoElement {
    let chosen = move || state::screen() == screen;
    view().test_id(&screen_id(label)).child(
        button()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(7.0))
            .rounded(move || theme().radius)
            .bg(entry_bg(chosen))
            .hover(|s| s.bg(move || theme().sidebar_accent))
            .active(|s| s.opacity(0.7))
            .text_color(move || if chosen() { theme().foreground } else { theme().muted_foreground })
            .on_press(go(show))
            .child(icons::icon(paths, 16.0))
            .child(text(label).text_sm().font_weight(FontWeight::MEDIUM).text_color(move || theme().foreground)),
    )
}

/// The listed projects; none while loading.
fn rows() -> Vec<ProjectRow> {
    match state::projects() {
        Load::Ready(rows) => rows,
        _ => Vec::new(),
    }
}

fn is_chosen(path: &std::path::Path) -> bool {
    state::screen() == Screen::Project && state::selected().is_some_and(|p| p == path)
}

fn project_row(row: ReadSignal<ProjectRow>) -> impl IntoElement {
    let path = move || row.with(|r| r.path.clone());
    let chosen = move || is_chosen(&path());
    button()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .py(px(6.0))
        .rounded(move || theme().radius)
        .bg(entry_bg(chosen))
        .hover(|s| s.bg(move || theme().sidebar_accent))
        .active(|s| s.opacity(0.7))
        .on_press(move || {
            close_drawer();
            state::open(path());
        })
        .child(monogram(move || row.with(|r| r.name.clone()), chosen, 28.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(text(move || row.with(|r| r.name.clone())).text_sm().font_weight(FontWeight::MEDIUM).number_of_lines(1u32))
                .child(
                    path_text(move || {
                        row.with(|r| {
                            let p = tilde(&r.path);
                            if r.missing { format!("{p} (missing)") } else { p }
                        })
                    })
                    .number_of_lines(1u32),
                ),
        )
}

/// The collapsed sidebar: its button, a tile per project, and the other screens as icons.
fn rail() -> View {
    view()
        .flex_1()
        .items_center()
        .gap_2()
        .pt_1()
        .pb_3()
        .child(icon_button(SIDEBAR, "Show the project list", toggle))
        .child(
            scroll().flex_1().w_full().child(
                view()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .child(each(rows, |r: &ProjectRow| r.path.clone(), |row, _| rail_project(row)))
                    .child(icon_button(PLUS, "Add a project…", go(state::add_project))),
            ),
        )
        .child(
            view()
                .items_center()
                .gap_1()
                .pt_2()
                .border_t_1()
                .border_t_color(move || theme().sidebar_border)
                .child(rail_screen(INBOX, "Review", Screen::Review, state::show_review))
                .child(rail_screen(PERSON, "Personal settings", Screen::Personal, state::show_personal)),
        )
}

fn rail_project(row: ReadSignal<ProjectRow>) -> impl IntoElement {
    let path = move || row.with(|r| r.path.clone());
    let chosen = move || is_chosen(&path());
    let name = row.with_untracked(|r| r.name.clone());
    button()
        .rounded(move || theme().radius)
        .active(|s| s.opacity(0.7))
        .accessibility_label(name.clone())
        .on_press(move || state::open(path()))
        .child(monogram(move || row.with(|r| r.name.clone()), chosen, 34.0))
        .with_hint(name)
}

fn rail_screen(paths: &'static str, label: &'static str, screen: Screen, show: fn()) -> impl IntoElement {
    let chosen = move || state::screen() == screen;
    view().test_id(&format!("rail-{}", screen_id(label))).child(
        button()
            .w(px(34.0))
            .h(px(34.0))
            .items_center()
            .justify_center()
            .rounded(move || theme().radius)
            .bg(entry_bg(chosen))
            .hover(|s| s.bg(move || theme().sidebar_accent))
            .active(|s| s.opacity(0.7))
            .text_color(move || if chosen() { theme().primary } else { theme().muted_foreground })
            .accessibility_label(label)
            .on_press(go(show))
            .child(icons::icon(paths, 18.0))
            .with_hint(label),
    )
}

/// A project's initials on a tile, inked when it is the one shown.
fn monogram(name: impl Fn() -> String + 'static, chosen: impl Fn() -> bool + Copy + 'static, size: f32) -> View {
    view()
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .bg(move || if chosen() { theme().primary } else { theme().accent })
        .child(
            text(move || initials(&name()))
                .text_size(px(size * 0.4))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || if chosen() { theme().primary_foreground } else { theme().primary }),
        )
}

/// `obmem` → `Ob`, `demo-repo` → `DR`: the first letters of the first two words, or the first
/// two letters of a single word.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut out = String::new();
    match words.as_slice() {
        [] => out.push('?'),
        [one] => {
            let mut chars = one.chars();
            out.extend(chars.next().map(|c| c.to_ascii_uppercase()));
            out.extend(chars.next());
        }
        [a, b, ..] => {
            out.extend(a.chars().next().map(|c| c.to_ascii_uppercase()));
            out.extend(b.chars().next().map(|c| c.to_ascii_uppercase()));
        }
    }
    out
}

/// The path with the home folder as `~`, which is what fits in the sidebar.
fn tilde(path: &std::path::Path) -> String {
    let p = path.display().to_string();
    match std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        Some(home) => {
            let home = home.to_string_lossy();
            match p.strip_prefix(home.as_ref()) {
                Some(rest) if !home.is_empty() => format!("~{rest}"),
                _ => p,
            }
        }
        None => p,
    }
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_take_two_words_or_two_letters() {
        assert_eq!(initials("obmem"), "Ob");
        assert_eq!(initials("demo-repo"), "DR");
        assert_eq!(initials("my_cool app"), "MC");
        assert_eq!(initials(""), "?");
    }
}
