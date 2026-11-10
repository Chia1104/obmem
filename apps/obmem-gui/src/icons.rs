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
pub(crate) const PLUS: &str = r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#;
/// Drafts and findings waiting for a person.
pub(crate) const INBOX: &str = r#"<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#;
/// The user's own settings.
pub(crate) const PERSON: &str = r#"<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>"#;

/// How the window looks.
pub(crate) const PALETTE: &str = r#"<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z"/><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/>"#;

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
