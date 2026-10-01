//! A meter bar's length over time: eased toward a new value when the
//! reader switches what they look at (a view, a pull), set at once while
//! the pull is live. A live pull's snapshots land up to ten times a second,
//! so an ease of any length never settles, and every frame of it redraws
//! the whole surface: the overlay used five times the iced overlay's CPU
//! for as long as a pull went. The iced bars step at the snapshots' pace.

use std::time::Duration;

use gpui_kit::base::{Easing, Transition, transition};
use gpui_kit::{App, ElementId, Window};

/// How long a bar takes to reach a new length on a switch.
pub const BAR_EASE: Duration = Duration::from_millis(280);

/// The bar `id`'s length toward `target`: eased, or `target` itself while
/// `live`.
pub fn bar_frac(id: ElementId, target: f32, live: bool, window: &mut Window, cx: &mut App) -> f32 {
    if live {
        return target;
    }
    transition(
        id,
        target,
        Transition::new(BAR_EASE).easing(Easing::EaseOut),
        window,
        cx,
    )
}
