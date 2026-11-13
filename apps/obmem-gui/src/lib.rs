//! obmem's desktop control panel: the projects obmem knows, their health, agent setup, settings,
//! secret scanning, and reviewing what agents wrote.
//! `nsh.json` `"root"` names [`App`]. Notes themselves are read and edited in Obsidian.

mod agent_tab;
mod appearance;
mod data;
mod form;
mod icons;
mod login_env;
mod motion;
mod personal_page;
mod preferences_page;
mod private_vault;
mod project_page;
mod review_page;
mod secrets_tab;
mod sessions_tab;
mod settings_tab;
mod sidebar;
mod state;
mod sync_card;
mod toasts;
mod widgets;
mod words;

use nsh_base::theme;
use nsh_core::prelude::*;

/// The app's UI set (`nsh.json` `"rust"`). The runners call this before they start the engine,
/// which makes it the place for process setup.
#[native_module("obmem-gui/native")]
fn native(registry: &mut Registry) {
    prepare_process();
    registry.register::<App>();
}

/// The app as a host built by Cargo starts it (`cargo run`, the tests): [`App`], with the
/// translations in `locales/`, which `build.rs` read. The nsh CLI's runners make their own root
/// and read `locales/` themselves.
pub fn root() -> nsh_core::element::Root {
    nsh_core::element::Root::of::<App>()
        .translations_json(include_str!(concat!(env!("OUT_DIR"), "/translations.json")))
}

/// Once per process, before any thread starts: see [`login_env`].
pub fn prepare_process() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(login_env::adopt_login_path);
}

/// The window: the project list beside the shown project.
#[component]
pub fn App() -> impl IntoElement {
    // The window has no title bar of the system's: a strip along its top edge drags it and holds
    // the sidebar's button (`sidebar::top_strip`).
    let mut title_bar = nsh_core::desktop::TitleBarOptions::default();
    title_bar.style = nsh_core::desktop::TitleBarStyle::Hidden;
    title_bar.height = Some(px(40.0));
    let mut attributes = nsh_core::desktop::WindowAttributes::default();
    attributes.title_bar = Some(title_bar);
    nsh_core::element::desktop::current_window().set(attributes);
    state::own_tasks();
    state::start_appearance();
    state::load_projects(None);
    view()
        .flex_1()
        .relative()
        // The window is the sidebar's color: the page is a panel set into it (`widgets::page`).
        .bg(move || theme().sidebar)
        .text_color(move || theme().foreground)
        .child(sidebar::top_strip(false))
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(sidebar::sidebar())
                .child(dynamic(move || {
                    use state::{Screen, Section};
                    match state::screen() {
                        Screen::Project => project_page::project_page().into_element(),
                        Screen::Review => review_page::review_page().into_element(),
                        Screen::Settings(Section::Preferences) => {
                            preferences_page::preferences_page().into_element()
                        }
                        Screen::Settings(Section::Vault) => {
                            personal_page::vault_page().into_element()
                        }
                        Screen::Settings(Section::Sync) => {
                            personal_page::sync_page().into_element()
                        }
                        Screen::Settings(Section::Secrets) => {
                            personal_page::secrets_page().into_element()
                        }
                    }
                })),
        )
        .child(sidebar::drawer())
        .child(toasts::toaster())
}
