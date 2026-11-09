//! obmem's desktop control panel: the projects obmem knows, their health, agent setup, settings,
//! secret scanning, and reviewing what agents wrote.
//! `nsh.json` `"root"` names [`App`]. Notes themselves are read and edited in Obsidian.

mod agent_tab;
mod data;
mod login_env;
mod personal_page;
mod project_page;
mod review_page;
mod secrets_tab;
mod sessions_tab;
mod settings_tab;
mod sidebar;
mod state;
mod widgets;

use nsh_base::theme;
use nsh_core::prelude::*;

/// The app's UI set (`nsh.json` `"rust"`). The runners call this before they start the engine,
/// which makes it the place for process setup.
#[native_module("obmem-gui/native")]
fn native(registry: &mut Registry) {
    prepare_process();
    registry.register::<App>();
}

/// Once per process, before any thread starts: see [`login_env`].
pub fn prepare_process() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(login_env::adopt_login_path);
}

/// The window: the project list beside the shown project.
#[component]
pub fn App() -> impl IntoElement {
    state::own_tasks();
    state::load_projects(None);
    view()
        .flex_1()
        .flex_row()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(sidebar::sidebar())
        .child(dynamic(move || match state::screen() {
            state::Screen::Project => project_page::project_page().into_element(),
            state::Screen::Personal => personal_page::personal_page().into_element(),
            state::Screen::Review => review_page::review_page().into_element(),
        }))
}
