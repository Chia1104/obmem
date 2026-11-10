//! obmem's desktop control panel: the projects obmem knows, their health, agent setup, settings,
//! secret scanning, and reviewing what agents wrote.
//! `nsh.json` `"root"` names [`App`]. Notes themselves are read and edited in Obsidian.

mod agent_tab;
mod data;
mod icons;
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

/// The app's palette over NSH's default theme: blue-black ink on paper.
const THEME: &str = include_str!("theme.json");

/// The window: the project list beside the shown project.
#[component]
pub fn App() -> impl IntoElement {
    set_tokens(nsh_base::Theme::from_json(THEME).expect("theme.json parses (see the test below)"));
    // The sidebar and the page run up to the window's top edge; each keeps a strip there that
    // drags the window (`widgets::title_strip`).
    let mut title_bar = nsh_core::desktop::TitleBarOptions::default();
    title_bar.style = nsh_core::desktop::TitleBarStyle::Hidden;
    title_bar.height = Some(px(40.0));
    let mut attributes = nsh_core::desktop::WindowAttributes::default();
    attributes.title_bar = Some(title_bar);
    nsh_core::element::desktop::current_window().set(attributes);
    state::own_tasks();
    state::load_projects(None);
    view()
        .flex_1()
        .relative()
        .flex_row()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(sidebar::sidebar())
        .child(dynamic(move || match state::screen() {
            state::Screen::Project => project_page::project_page().into_element(),
            state::Screen::Personal => personal_page::personal_page().into_element(),
            state::Screen::Review => review_page::review_page().into_element(),
        }))
        .child(sidebar::drawer())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_theme_parses_and_has_the_app_tokens() {
        let t = nsh_base::Theme::from_json(super::THEME).unwrap();
        for name in ["ok", "warn", "highlight"] {
            assert!(t.color(name).is_some(), "{name}");
        }
    }
}
