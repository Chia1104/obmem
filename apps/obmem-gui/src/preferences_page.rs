//! The Preferences page of the settings: the window's language, its mode, the theme each
//! appearance shows (picked in a dialog), and the user's own theme files.

use crate::appearance::{self, Language, Mode, Scheme, ThemeFile};
use crate::motion::{Eased, pressed};
use crate::state::{self, Work};
use crate::widgets::{
    FirstRow, Level, about, action, card, check, choice, dialog_box, field, heading, muted, page,
    page_bar, path_text, primary, select,
};
use crate::words::tr;
use nsh_base::theme;
use nsh_core::prelude::*;

pub(crate) fn preferences_page() -> impl IntoElement {
    // The appearance whose theme is being picked, while the dialog is open.
    let picking = signal(None::<Scheme>);
    page(
        page_bar(tr!("prefs.preferences"), view(), vec![]),
        view()
            .gap_4()
            .child(about(tr!("prefs.preferences_about")))
            .child(
                card()
                    .child(
                        field(
                            tr!("language.title"),
                            tr!("language.about"),
                            // A language is named in itself, so that its speakers find it.
                            select(
                                &[
                                    (Language::System, "language.system"),
                                    (Language::English, "language.english"),
                                    (Language::Chinese, "language.chinese"),
                                ],
                                state::language,
                                state::set_language,
                                "language.title",
                            ),
                        )
                        .first_row()
                        .test_id("appearance-language"),
                    )
                    .child(
                        field(
                            tr!("appearance.mode"),
                            tr!("appearance.mode_about"),
                            choice(
                                &[
                                    (Mode::System, "appearance.system"),
                                    (Mode::Light, "appearance.light"),
                                    (Mode::Dark, "appearance.dark"),
                                ],
                                state::mode,
                                state::set_mode,
                            ),
                        )
                        .test_id("appearance-mode"),
                    )
                    .child(theme_row(Scheme::Light, picking))
                    .child(theme_row(Scheme::Dark, picking)),
            )
            // Rebuilt when the theme files change.
            .child(dynamic(move || own_themes(&state::themes())))
            .child(dynamic(move || {
                picking
                    .get()
                    .map(|scheme| theme_dialog(scheme, move || picking.set(None)))
            })),
    )
}

/// The translation keys of an appearance's theme: its name and what it is.
fn scheme_words(scheme: Scheme) -> (&'static str, &'static str) {
    match scheme {
        Scheme::Light => ("appearance.light_theme", "appearance.light_theme_about"),
        Scheme::Dark => ("appearance.dark_theme", "appearance.dark_theme_about"),
    }
}

/// The theme `scheme` shows, by name, and the way to pick another one.
fn theme_row(scheme: Scheme, picking: Signal<Option<Scheme>>) -> View {
    let (title, said) = scheme_words(scheme);
    let name = move || {
        let id = state::shown_theme(scheme);
        state::themes()
            .into_iter()
            .find(|t| t.id == id)
            .map(|t| t.name)
            .unwrap_or(id)
    };
    view()
        .test_id(&format!("{}-theme-row", scheme.label()))
        .child(field(
            tr!(title),
            tr!(said),
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .child(text(name).text_sm().font_weight(FontWeight::MEDIUM))
                .child(action(tr!("prefs.change_theme"), move || {
                    picking.set(Some(scheme))
                })),
        ))
        .child(dynamic(move || gone(scheme)))
}

/// Said while the theme chosen for `scheme` cannot be shown.
fn gone(scheme: Scheme) -> Option<View> {
    let chosen = state::chosen_theme(scheme);
    (chosen != state::shown_theme(scheme)).then(|| {
        check(
            Level::Warn,
            tr!("appearance.theme_gone", theme = chosen),
            tr!("appearance.theme_gone_about"),
        )
    })
}

/// The themes to pick from for `scheme`, each drawn in its own colors. A pick shows at once, in
/// the window behind the dialog.
fn theme_dialog(scheme: Scheme, close: impl Fn() + Clone + 'static) -> AnyElement {
    let (title, _) = scheme_words(scheme);
    // Rebuilt when the theme files change (a copy was made), not when one is picked: a pick only
    // moves the mark, so the keyboard focus stays in the list.
    let tiles = dynamic(move || {
        let mut list = nsh_base::radio_group().flex_row().flex_wrap().gap_3();
        for file in state::themes().iter().filter(|t| t.tokens.is_ok()) {
            list = list.child(tile(file, scheme));
        }
        list
    });
    dialog_box(
        &format!("{}-theme", scheme.label()),
        tr!(title),
        620.0,
        view()
            .gap_3()
            .child(dynamic(move || gone(scheme)))
            .child(tiles)
            .child(muted(tr!("appearance.duplicate_about")).text_xs()),
        vec![
            action(tr!("appearance.duplicate"), move || {
                state::duplicate_theme(scheme)
            })
            .busy(state::running(Work::DuplicateTheme(scheme)))
            .into_element(),
            primary(tr!("common.done"), close.clone()).into_element(),
        ],
        close,
    )
}

/// A theme to pick: a small window in its colors for `scheme`, and its name.
fn tile(file: &ThemeFile, scheme: Scheme) -> impl IntoElement {
    let id = file.id.clone();
    let on = {
        let id = id.clone();
        move || state::shown_theme(scheme) == id
    };
    let color = |name: &str| {
        file.color(name, scheme)
            .unwrap_or(AdaptiveColor::TRANSPARENT)
    };
    let bar = |name: &str, width: f32| {
        view()
            .w(px(width))
            .h(px(4.0))
            .rounded_full()
            .bg(color(name))
    };
    let preview = view()
        .flex_row()
        .w(px(124.0))
        .h(px(72.0))
        .overflow_hidden()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(color("border"))
        .bg(color("background"))
        .child(
            view()
                .w(px(30.0))
                .gap(px(4.0))
                .p(px(6.0))
                .bg(color("sidebar"))
                .border_r_1()
                .border_r_color(color("sidebar-border"))
                .child(bar("sidebar-foreground", 14.0))
                .child(bar("muted-foreground", 10.0)),
        )
        .child(
            view().flex_1().p(px(8.0)).child(
                view()
                    .flex_1()
                    .gap(px(5.0))
                    .p(px(7.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(color("border"))
                    .bg(color("card"))
                    .child(bar("foreground", 44.0))
                    .child(bar("muted-foreground", 30.0))
                    .child(
                        view()
                            .mt(px(3.0))
                            .w(px(26.0))
                            .h(px(10.0))
                            .rounded(px(3.0))
                            .bg(color("primary")),
                    ),
            ),
        );
    nsh_base::radio()
        .checked(on.clone())
        .accessibility_label(file.name.clone())
        .on_change(move |_: &bool| state::pick_theme(scheme, id.clone()))
        .gap_1()
        .presses()
        .active(pressed)
        .child(
            // The mark of the chosen one: a ring around its window.
            view()
                .p(px(2.0))
                .rounded(move || theme().radius + px(3.0))
                .border_2()
                .border_color({
                    let on = on.clone();
                    move || {
                        if on() {
                            theme().primary
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    }
                })
                .child(preview),
        )
        .child(
            text(file.name.clone())
                .text_xs()
                .px(px(4.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if on() {
                        theme().foreground
                    } else {
                        theme().muted_foreground
                    }
                }),
        )
}

/// The user's theme files, with what is wrong with the ones that cannot be used.
fn own_themes(themes: &[ThemeFile]) -> AnyElement {
    let mut block = card()
        .test_id("own-themes")
        .child(heading(tr!("appearance.own")))
        .child(muted(tr!("appearance.own_about")))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_3()
                .py_2()
                .child(action(tr!("appearance.open_folder"), || {
                    state::reveal_theme(None)
                }))
                .child(path_text(appearance::themes_dir().display().to_string())),
        );
    let own: Vec<&ThemeFile> = themes.iter().filter(|t| t.file.is_some()).collect();
    if own.is_empty() {
        return block
            .child(muted(tr!("appearance.own_none")))
            .into_element();
    }
    for own in own {
        let file = own.file.clone();
        let file_name = file
            .as_ref()
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Why a file cannot be used, and what Native Shell left out of one, are their words.
        let row = match &own.tokens {
            Err(why) => check(
                Level::Error,
                own.name.clone(),
                format!("{file_name}. {why}"),
            ),
            Ok(_) if !own.ignored.is_empty() => {
                let what = own.ignored.join("; ");
                check(
                    Level::Warn,
                    own.name.clone(),
                    tr!("appearance.left_out", file = file_name, what = what),
                )
            }
            Ok(_) => check(Level::Ok, own.name.clone(), file_name),
        };
        block = block.child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .border_t_1()
                .border_t_color(move || theme().border)
                .child(row.flex_1())
                .child(action(tr!("appearance.show_file"), move || {
                    state::reveal_theme(file.clone())
                })),
        );
    }
    block.into_element()
}
