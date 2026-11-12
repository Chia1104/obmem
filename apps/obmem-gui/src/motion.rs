//! How the controls move: one set of durations, curves and scales. Change a value here, not in
//! a control.
//!
//! Native Shell runs no transition when the user asks for less motion, so every change here
//! then lands at once, and a press only dims.

use nsh_core::element::StyleRefinement;
use nsh_core::prelude::*;
use nsh_core::{LayoutAnimation, LayoutCurve, LayoutTransition};

/// A color changing under the pointer, in milliseconds.
const INSTANT: f32 = 120.0;
/// A control pressed in and settling back.
const FAST: f32 = 180.0;
/// Something moving into place: a toast coming and going.
const BASE: f32 = 240.0;

/// A long, soft settle: most of the way at once, then slowly home.
const SMOOTH_OUT: [f32; 4] = [0.22, 1.0, 0.36, 1.0];

/// How far what comes in travels, in points.
const NEAR: f32 = 8.0;

/// How small a button or a row gets while pressed.
const PRESS: f32 = 0.97;
/// The same for an icon button, which is too small for [`PRESS`] to show.
const PRESS_ICON: f32 = 0.95;
/// How much a pressed control dims.
const DIM: f32 = 0.8;

/// How an element's changes of look are timed.
pub(crate) trait Eased: Styled {
    /// For what only changes color under the pointer: quick, and eased out.
    fn washes(self) -> Self {
        self.transition().duration(INSTANT).ease_out()
    }

    /// For what is pressed as a whole: its `hover` and `active` styles ([`pressed`]) come in
    /// and settle back softly.
    fn presses(self) -> Self {
        let [x1, y1, x2, y2] = SMOOTH_OUT;
        self.transition().duration(FAST).ease_bezier(x1, y1, x2, y2)
    }

    /// For what comes into place and leaves it: everything that changes on the way moves.
    fn settles(self) -> Self {
        self.transition_all().duration(BASE).ease_out()
    }
}

impl<T: Styled> Eased for T {}

/// What an element does as it comes into the window. It plays each time the element is built:
/// build it where a `dynamic` runs only when the element comes, not when what it shows changes.
pub(crate) trait Enters: Element {
    /// Fades in, rising a little: a page, a notice.
    fn rises(self) -> Self {
        self.bind_motion(
            &motion().entering(coming().values(MotionChannel::TranslateY, [NEAR, 0.0])),
        )
    }

    /// Fades in where it is: a line of text.
    fn fades_in(self) -> Self {
        self.bind_motion(&motion().entering(coming()))
    }
}

impl<T: Element> Enters for T {}

/// Makes the next change of layout a motion: what moves or resizes glides there, and what comes
/// or goes fades. Call it before the change, when the arrangement changes (the sidebar collapses,
/// the window crosses a breakpoint), not for a size that follows the pointer.
pub(crate) fn glide() {
    if reduce_motion() {
        return;
    }
    animate_layout(LayoutAnimation::all(LayoutTransition::new(
        LayoutCurve::EaseInEaseOut,
        BASE,
    )));
}

/// Out of sight to in sight, as things settle.
fn coming() -> MotionSpec {
    let [x1, y1, x2, y2] = SMOOTH_OUT;
    MotionSpec::new(KeyframeTiming::new(BASE).easing(Easing::Bezier(x1, y1, x2, y2)))
        .values(MotionChannel::Opacity, [0.0, 1.0])
}

/// The `active` style of a button or a row: `.presses().active(pressed)`.
pub(crate) fn pressed(style: StyleRefinement) -> StyleRefinement {
    sunk(style, PRESS)
}

/// The `active` style of an icon button.
pub(crate) fn pressed_icon(style: StyleRefinement) -> StyleRefinement {
    sunk(style, PRESS_ICON)
}

/// Dimmed and, unless the user asks for less motion, a little smaller.
fn sunk(style: StyleRefinement, scale: f32) -> StyleRefinement {
    style
        .opacity(DIM)
        .scale(move || if reduce_motion() { 1.0 } else { scale })
}
