//! The few icons the window draws: 24-point line drawings in `currentColor`, so they take the
//! text color of what holds them. Paths after Lucide (ISC license).

use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

/// Shows or hides the sidebar.
pub(crate) const SIDEBAR: &str =
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#;
pub(crate) const CHECK: &str = r#"<path d="M20 6 9 17l-5-5"/>"#;
/// An open ring, which turns while work runs (`widgets::spinner`).
pub(crate) const LOADER: &str = r#"<path d="M21 12a9 9 0 1 1-6.219-8.56"/>"#;
/// Dismisses a toast.
pub(crate) const X: &str = r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#;
pub(crate) const CHEVRONS: &str = r#"<path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/>"#;
pub(crate) const PLUS: &str = r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#;
/// Drafts and findings waiting for a person.
pub(crate) const INBOX: &str = r#"<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#;
/// The personal vault.
pub(crate) const PERSON: &str = r#"<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>"#;
/// The settings.
pub(crate) const SETTINGS: &str = r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#;
/// Leaves the settings.
pub(crate) const CHEVRON_LEFT: &str = r#"<path d="m15 18-6-6 6-6"/>"#;
/// The language and how the window looks.
pub(crate) const SLIDERS: &str = r#"<path d="M21 4h-7"/><path d="M10 4H3"/><path d="M21 12h-9"/><path d="M8 12H3"/><path d="M21 20h-5"/><path d="M12 20H3"/><path d="M14 2v4"/><path d="M8 10v4"/><path d="M16 18v4"/>"#;
/// Syncing the personal vault.
pub(crate) const CLOUD: &str = r#"<path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"/>"#;
/// What the secret scanner lets pass.
pub(crate) const SHIELD: &str = r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>"#;
/// Reads something again.
pub(crate) const REFRESH: &str = r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>"#;
/// Shows a folder in the file manager.
pub(crate) const FOLDER: &str = r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#;
/// Takes something off a list.
pub(crate) const TRASH: &str = r#"<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M10 11v6"/><path d="M14 11v6"/>"#;

/// `paths` as an icon `size` points square, drawn in the text color around it.
pub(crate) fn icon(paths: &str, size: f32) -> impl IntoElement {
    let markup = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"#
    );
    svg(ImageSrc::svg_markup(&markup))
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
}
