//! The sidebar: what waits for review, every project obmem has seen, adding one by hand, and the
//! way into the settings. While a page of the settings shows, it lists those pages instead.
//!
//! The sidebar is resized by dragging its edge and collapses to a rail of project initials, by
//! its button or by dragging it narrower than [`SNAP`]. Its width and whether it is collapsed are
//! kept between runs. In a narrow window ([`widgets::narrow`]) it stays a rail, and the full list
//! opens over the page as a drawer.

use crate::data::ProjectRow;
use crate::icons::{
    self, CHEVRON_LEFT, CLOUD, INBOX, PERSON, PLUS, SETTINGS, SHIELD, SIDEBAR, SLIDERS,
};
use crate::motion::{Eased, pressed, pressed_icon};
use crate::state::{self, Load, Screen, Section};
use crate::widgets::{self, WithHint, icon_button, loading, muted};
use crate::words::{named, tr};
use nsh_base::{HandleEdge, ResizeHandleState, SheetEvent, button, resize_handle, sheet, theme};
use nsh_core::element::desktop::title_bar;
use nsh_core::geometry::Axis;
use nsh_core::prelude::*;
use nsh_core::storage::Storage;

const MIN: f32 = 200.0;
const MAX: f32 = 420.0;
const DEFAULT: f32 = 264.0;
/// The rail's width.
const RAIL: f32 = 56.0;
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
        return if raw < SNAP {
            Shape::Rail
        } else {
            Shape::Full(raw.clamp(MIN, MAX))
        };
    }
    if global::<Collapsed>().with(|c| c.0) {
        Shape::Rail
    } else {
        Shape::Full(global::<Width>().with(|w| w.0))
    }
}

/// The width the sidebar takes beside the page.
pub(crate) fn docked_width() -> f32 {
    match shape() {
        Shape::Rail => RAIL,
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
    // Rebuilt when it lists something else, not for each page of the settings.
    let settings = computed(in_settings);
    view()
        .relative()
        .flex_shrink_0()
        .w(move || px(docked_width()))
        .child(dynamic(move || match shape() {
            Shape::Rail => rail(settings.get()).into_element(),
            Shape::Full(_) => panel(settings.get()).into_element(),
        }))
        .child(
            resize_handle()
                .axis(Axis::Horizontal)
                .edge(HandleEdge::Trailing)
                .disabled(widgets::narrow)
                .accessibility_label(named("sidebar.resize"))
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
                    let Some((from, _)) = global::<Drag>().get_untracked().0 else {
                        return;
                    };
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
                // The edge's line: none at rest, since the page's panel has its own edge. While
                // the pointer is on it or drags it, a line that fades out toward both ends.
                .child(
                    view()
                        .w(px(2.0))
                        .rounded_full()
                        .bg_linear_to_b()
                        .gradient_from(AdaptiveColor::TRANSPARENT)
                        .gradient_via(move || theme().muted_foreground.opacity(0.7))
                        .gradient_to(AdaptiveColor::TRANSPARENT)
                        .opacity(move || {
                            if edge.get() == ResizeHandleState::Idle {
                                0.0_f32
                            } else {
                                1.0
                            }
                        })
                        .washes(),
                ),
        )
}

/// The strip along the window's top edge, where the system's title bar was: it drags the window,
/// the system's window controls sit in it, and the sidebar's button comes after them. The window
/// has one, and so has the drawer, which covers the window's.
pub(crate) fn top_strip(in_drawer: bool) -> View {
    view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        // Where the system draws its own title bar there is none to stand in for: the strip
        // still holds the button.
        .h(move || px(title_bar().top.0.max(36.0)))
        .pl(move || px(title_bar().left.0 + 12.0))
        .window_drag(true)
        .child(icon_button(
            SIDEBAR,
            move || {
                if in_drawer {
                    t!("sidebar.close")
                } else if shape() == Shape::Rail {
                    t!("sidebar.show")
                } else {
                    t!("sidebar.collapse")
                }
            },
            toggle,
        ))
}

/// The full list over the page, while it is open in a narrow window (NSH's `Sheet`): it slides in
/// from the left over a scrim, and a press on the scrim or Escape closes it.
pub(crate) fn drawer() -> impl IntoElement {
    sheet()
        .side(Placement::Left)
        .open(move || Some(widgets::narrow() && global::<Drawer>().with(|d| d.0)))
        .on_open_change(|e: &SheetEvent| {
            if let SheetEvent::OpenChange(false, _) = e {
                close_drawer();
            }
        })
        .popup_class(format!(
            "w-[{DEFAULT}px] bg-sidebar border-r border-sidebar-border shadow-xl"
        ))
        .backdrop_class(widgets::SCRIM)
        .child(top_strip(true))
        .child(dynamic(move || panel(in_settings()).into_element()))
}

/// What the sidebar lists: the projects, or the pages of the settings while one of them shows.
fn in_settings() -> bool {
    matches!(state::screen(), Screen::Settings(_))
}

/// The full sidebar. Beside the projects: what waits for review above them, the projects, and
/// the way into the settings below. In the settings: the way back, then their pages.
fn panel(settings: bool) -> View {
    let column = view().flex_1().gap_3().pt_1().pb_2();
    if settings {
        let mut pages = view().gap(px(2.0)).px_2();
        for section in SECTIONS {
            pages = pages.child(section_entry(section));
        }
        return column
            .child(view().px_2().child(entry(
                "settings-back",
                CHEVRON_LEFT,
                "prefs.back",
                || false,
                go(state::leave_settings),
            )))
            .child(
                view()
                    .flex_1()
                    .gap_1()
                    .child(group_title("prefs.title", None))
                    .child(pages),
            );
    }
    column
        .child(view().px_2().child(entry(
            "screen-review",
            INBOX,
            "review.title",
            || state::screen() == Screen::Review,
            go(state::show_review),
        )))
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .gap_1()
                .child(group_title(
                    "sidebar.projects",
                    Some(
                        icon_button(PLUS, tr!("sidebar.add"), go(state::add_project))
                            .into_element(),
                    ),
                ))
                .child(
                    scroll().flex_1().child(
                        view()
                            .gap(px(2.0))
                            .px_2()
                            .child(dynamic(move || match state::projects() {
                                Load::Loading => {
                                    Some(loading(tr!("common.loading")).px_2().into_element())
                                }
                                Load::Failed(e) => Some(muted(e).px_2().into_element()),
                                Load::Ready(rows) if rows.is_empty() => {
                                    Some(muted(tr!("sidebar.empty")).px_2().into_element())
                                }
                                Load::Ready(_) => None,
                            }))
                            .child(each(
                                rows,
                                |r: &ProjectRow| r.path.clone(),
                                |row, _| project_row(row),
                            )),
                    ),
                ),
        )
        .child(view().px_2().child(entry(
            "screen-settings",
            SETTINGS,
            "prefs.title",
            || false,
            go(|| state::show_settings(Section::Preferences)),
        )))
}

/// The name of a group of entries, with the button that adds to it. `name` is a translation key.
fn group_title(name: &'static str, add: Option<AnyElement>) -> View {
    let row = view()
        .flex_row()
        .items_center()
        .h(px(28.0))
        .pl_4()
        .pr_2()
        .child(
            text(move || t!(name))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || theme().muted_foreground)
                .flex_1(),
        );
    match add {
        Some(add) => row.child(add),
        None => row,
    }
}

/// The settings' pages, in the order the sidebar lists them.
const SECTIONS: [Section; 4] = [
    Section::Preferences,
    Section::Vault,
    Section::Sync,
    Section::Secrets,
];

/// What tests find a page of the settings by, its icon, and the translation key of its name.
pub(crate) fn section_names(section: Section) -> (&'static str, &'static str, &'static str) {
    match section {
        Section::Preferences => ("settings-preferences", SLIDERS, "prefs.preferences"),
        Section::Vault => ("settings-vault", PERSON, "common.personal_vault"),
        Section::Sync => ("settings-sync", CLOUD, "sync.title"),
        Section::Secrets => ("settings-secrets", SHIELD, "prefs.secrets"),
    }
}

fn section_entry(section: Section) -> impl IntoElement {
    let (id, paths, name) = section_names(section);
    entry(
        id,
        paths,
        name,
        move || state::screen() == Screen::Settings(section),
        go(move || state::show_settings(section)),
    )
}

/// The background of an entry: lit while it is the one shown.
fn entry_bg(chosen: impl Fn() -> bool + Copy + 'static) -> impl Fn() -> AdaptiveColor {
    move || {
        if chosen() {
            theme().sidebar_accent
        } else {
            AdaptiveColor::TRANSPARENT
        }
    }
}

/// What the pointer does to an entry that is not the one shown: a wash of the text color.
fn entry_hover() -> AdaptiveColor {
    theme().foreground.opacity(0.06)
}

/// A line of the sidebar that opens something: an icon and a name. `id` is what tests find it by,
/// and `name` a translation key.
fn entry(
    id: &str,
    paths: &'static str,
    name: &'static str,
    chosen: impl Fn() -> bool + Copy + 'static,
    show: impl Fn() + 'static,
) -> impl IntoElement {
    view().test_id(id).child(
        button()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(30.0))
            .rounded(move || theme().radius)
            .bg(entry_bg(chosen))
            .presses()
            .hover(move |s| {
                s.bg(move || {
                    if chosen() {
                        theme().sidebar_accent
                    } else {
                        entry_hover()
                    }
                })
            })
            .active(pressed)
            .text_color(move || {
                if chosen() {
                    theme().foreground
                } else {
                    theme().muted_foreground
                }
            })
            .on_press(show)
            .child(icons::icon(paths, 16.0))
            .child(
                text(move || t!(name))
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || theme().foreground),
            ),
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

/// A project: its initials and its name. Its folder shows while the pointer rests on it.
fn project_row(row: ReadSignal<ProjectRow>) -> impl IntoElement {
    let path = move || row.with(|r| r.path.clone());
    let chosen = move || is_chosen(&path());
    let missing = move || row.with(|r| r.missing);
    button()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .h(px(32.0))
        .rounded(move || theme().radius)
        .bg(entry_bg(chosen))
        .presses()
        .hover(move |s| {
            s.bg(move || {
                if chosen() {
                    theme().sidebar_accent
                } else {
                    entry_hover()
                }
            })
        })
        .active(pressed)
        .on_press(move || {
            close_drawer();
            state::open(path());
        })
        .child(monogram(move || row.with(|r| r.name.clone()), chosen, 20.0))
        .child(
            text(move || row.with(|r| r.name.clone()))
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1u32)
                .text_color(move || {
                    if missing() {
                        theme().muted_foreground
                    } else {
                        theme().foreground
                    }
                }),
        )
        .with_hint(move || {
            row.with(|r| {
                let p = tilde(&r.path);
                if r.missing {
                    t!("sidebar.missing", path = p)
                } else {
                    p
                }
            })
        })
}

/// The collapsed sidebar: what the full one lists, as icons.
fn rail(settings: bool) -> View {
    let column = view().flex_1().items_center().gap_2().pt_1().pb_2();
    if settings {
        let mut pages = view().items_center().gap_1();
        for section in SECTIONS {
            let (id, paths, name) = section_names(section);
            pages = pages.child(rail_entry(
                id,
                paths,
                name,
                move || state::screen() == Screen::Settings(section),
                move || state::show_settings(section),
            ));
        }
        return column
            .child(rail_entry(
                "settings-back",
                CHEVRON_LEFT,
                "prefs.back",
                || false,
                state::leave_settings,
            ))
            .child(view().flex_1().child(pages));
    }
    column
        .child(rail_entry(
            "screen-review",
            INBOX,
            "review.title",
            || state::screen() == Screen::Review,
            state::show_review,
        ))
        .child(
            scroll().flex_1().w_full().child(
                view()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .child(each(
                        rows,
                        |r: &ProjectRow| r.path.clone(),
                        |row, _| rail_project(row),
                    ))
                    .child(icon_button(
                        PLUS,
                        tr!("sidebar.add"),
                        go(state::add_project),
                    )),
            ),
        )
        .child(rail_entry(
            "screen-settings",
            SETTINGS,
            "prefs.title",
            || false,
            || state::show_settings(Section::Preferences),
        ))
}

fn rail_project(row: ReadSignal<ProjectRow>) -> impl IntoElement {
    let path = move || row.with(|r| r.path.clone());
    let chosen = move || is_chosen(&path());
    let name = row.with_untracked(|r| r.name.clone());
    button()
        .rounded(move || theme().radius)
        .presses()
        .active(pressed_icon)
        .accessibility_label(name.clone())
        .on_press(move || state::open(path()))
        .child(monogram(move || row.with(|r| r.name.clone()), chosen, 32.0))
        .with_hint(name)
}

/// An entry of the rail: the icon of what the full sidebar names. `id` and `name` as in
/// [`entry`]; tests find it by `rail-` and `id`.
fn rail_entry(
    id: &str,
    paths: &'static str,
    name: &'static str,
    chosen: impl Fn() -> bool + Copy + 'static,
    show: impl Fn() + 'static,
) -> impl IntoElement {
    view().test_id(&format!("rail-{id}")).child(
        button()
            .w(px(32.0))
            .h(px(32.0))
            .items_center()
            .justify_center()
            .rounded(move || theme().radius)
            .bg(entry_bg(chosen))
            .presses()
            .hover(move |s| {
                s.bg(move || {
                    if chosen() {
                        theme().sidebar_accent
                    } else {
                        entry_hover()
                    }
                })
            })
            .active(pressed_icon)
            .text_color(move || {
                if chosen() {
                    theme().foreground
                } else {
                    theme().muted_foreground
                }
            })
            .accessibility_label(named(name))
            .on_press(show)
            .child(icons::icon(paths, 16.0))
            .with_hint(move || t!(name)),
    )
}

/// A project's initials on a tile, inked when it is the one shown.
fn monogram(
    name: impl Fn() -> String + 'static,
    chosen: impl Fn() -> bool + Copy + 'static,
    size: f32,
) -> View {
    view()
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .bg(move || {
            if chosen() {
                theme().primary
            } else {
                theme().accent
            }
        })
        .child(
            text(move || initials(&name()))
                .text_size(px((size * 0.42).max(9.0)))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || {
                    if chosen() {
                        theme().primary_foreground
                    } else {
                        theme().primary
                    }
                }),
        )
}

/// `obmem` → `Ob`, `demo-repo` → `DR`: the first letters of the first two words, or the first
/// two letters of a single word.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
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
