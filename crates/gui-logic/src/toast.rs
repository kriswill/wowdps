//! A passing word over the stage (`.toast`) and what the window says in
//! one: a pin's, the store's answer to `p`, and a stored pull's refusals —
//! the store keeps no comparison, no enemies' view and no ability's own
//! curve, so those are answered with a word rather than asked of it.

use std::time::Duration;

use wowdps_model::Action;

/// How long a toast stays up (the prototype's `toast()`: 2.6 s).
pub const TOAST_FOR: Duration = Duration::from_millis(2_600);

/// What `p` says on a pull the store holds no card of (yet).
pub const NO_CARD: &str = "No stored card for this pull yet: the store writes one when it ends";
/// What the store's answer to `p` says.
pub const PINNED: &str = "Pinned: retention keeps this pull";
pub const UNPINNED: &str = "Unpinned: retention may remove this pull";
/// What a stored pull says of what the store keeps no answer for: a
/// comparison, an ability's own curve and the enemies' view.
pub const NO_STORED_PAIR: &str = "The history store keeps no comparison";
pub const NO_STORED_ABILITY: &str = "The history store keeps no ability's own curve";
pub const NOT_STORED: &str = "The history store keeps no enemy damage";

/// What a pin says until the pair forms: the meter's "A" is small, and a
/// narrow window's button that says so may be off screen.
pub fn pinned_player(name: &str) -> String {
    format!("Pinned {name}. Move to another player to compare.")
}

/// What a stored pull says to `action` instead of asking the store for
/// what it does not keep: `v` (a comparison), the enemies' view, and Enter
/// inside the inspector (which opens an ability) — but Enter on a group of
/// the ability tree folds it, stored or not (`keyed_folds`). `None`: the
/// action is the pull's to answer.
pub fn stored_refusal(action: Action, inspecting: bool, keyed_folds: bool) -> Option<&'static str> {
    match action {
        Action::PickCompare => Some(NO_STORED_PAIR),
        Action::SetView(v) if !v.is_stored() => Some(NOT_STORED),
        Action::Open if inspecting && !keyed_folds => Some(NO_STORED_ABILITY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::View;

    /// A stored pull refuses what the store keeps no answer for, and only
    /// that: a fold of the ability tree, a stored view and a step pass.
    #[test]
    fn a_stored_pull_refuses_what_the_store_keeps_no_answer_for() {
        assert_eq!(
            stored_refusal(Action::PickCompare, false, false),
            Some(NO_STORED_PAIR)
        );
        assert_eq!(
            stored_refusal(Action::SetView(View::EnemyTaken), false, false),
            Some(NOT_STORED)
        );
        assert_eq!(
            stored_refusal(Action::SetView(View::Healing), false, false),
            None
        );
        assert_eq!(
            stored_refusal(Action::Open, true, false),
            Some(NO_STORED_ABILITY)
        );
        assert_eq!(stored_refusal(Action::Open, true, true), None, "a fold");
        assert_eq!(stored_refusal(Action::Open, false, false), None);
        assert_eq!(stored_refusal(Action::Down, true, false), None);
    }
}
