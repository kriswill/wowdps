//! A passing word over the stage (`.toast`) and what the window says in
//! one: a pin's, the store's answer to `p`, and a stored pull's refusals —
//! the store keeps no enemies' view, and (v42) a comparison only where it
//! kept the pull's details, an ability's own curve only where it kept its
//! seconds (kills, timed keys and pinned pulls), so the rest is answered
//! with a word rather than asked of it.

use std::time::Duration;

use wowdps_model::Action;

use crate::history::Kept;

/// How long a toast stays up (the prototype's `toast()`: 2.6 s).
pub const TOAST_FOR: Duration = Duration::from_millis(2_600);

/// What `p` says on a pull the store holds no card of (yet).
pub const NO_CARD: &str = "No stored card for this pull yet: the store writes one when it ends";
/// What the store's answer to `p` says.
pub const PINNED: &str = "Pinned: retention keeps this pull";
pub const UNPINNED: &str = "Unpinned: retention may remove this pull";
/// What a stored pull says of what the store keeps no answer for: a
/// comparison where it kept the rows alone, an ability's own curve where
/// it kept no seconds, and the enemies' view anywhere.
pub const NO_STORED_PAIR: &str =
    "The history store kept this pull's rows, not what a comparison needs";
pub const NO_STORED_ABILITY: &str =
    "The history store keeps an ability's own curve for kills, timed keys and pinned pulls";
pub const NOT_STORED: &str = "The history store keeps no enemy damage";

/// What a pin says until the pair forms: the meter's "A" is small, and a
/// narrow window's button that says so may be off screen.
pub fn pinned_player(name: &str) -> String {
    format!("Pinned {name}. Move to another player to compare.")
}

/// What a stored pull says to `action` instead of asking the store for
/// what it does not keep: `v` PINNING where it kept no details (`v` with a
/// pin or a pair up lets it go, always — `comparing`), the enemies' view,
/// and Enter inside the inspector (which opens an ability) where it kept
/// no ability's targets — but Enter on a group of the ability tree folds
/// it, stored or not (`keyed_folds`). `kept` is what the window offers
/// (`history::Stored::offered`: everything before the store has
/// answered). `None`: the action is the pull's to answer.
pub fn stored_refusal(
    action: Action,
    inspecting: bool,
    keyed_folds: bool,
    comparing: bool,
    kept: Kept,
) -> Option<&'static str> {
    match action {
        Action::PickCompare if !comparing && !kept.details => Some(NO_STORED_PAIR),
        Action::SetView(v) if !v.is_stored() => Some(NOT_STORED),
        Action::Open if inspecting && !keyed_folds && !kept.abilities => Some(NO_STORED_ABILITY),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::View;

    /// A stored pull refuses what the store keeps no answer for, and only
    /// that: a fold of the ability tree, a stored view and a step pass; a
    /// pull whose details it kept compares, one whose abilities it kept
    /// opens one; a pin already up is always let go; before the store has
    /// answered, everything is offered.
    #[test]
    fn a_stored_pull_refuses_what_the_store_keeps_no_answer_for() {
        let rows = Kept::default();
        let details = Kept {
            details: true,
            ..Kept::default()
        };
        let series = Kept {
            series: true,
            ..details
        };
        let refusal = |a, inspecting, folds, comparing, kept| {
            stored_refusal(a, inspecting, folds, comparing, kept)
        };
        assert_eq!(
            refusal(Action::PickCompare, false, false, false, rows),
            Some(NO_STORED_PAIR)
        );
        assert_eq!(
            refusal(Action::PickCompare, false, false, true, rows),
            None,
            "a pin is let go whatever the store kept"
        );
        assert_eq!(
            refusal(Action::PickCompare, false, false, false, details),
            None
        );
        assert_eq!(
            refusal(
                Action::SetView(View::EnemyTaken),
                false,
                false,
                false,
                Kept::ALL
            ),
            Some(NOT_STORED)
        );
        assert_eq!(
            refusal(Action::SetView(View::Healing), false, false, false, rows),
            None
        );
        assert_eq!(
            refusal(Action::Open, true, false, false, series),
            Some(NO_STORED_ABILITY),
            "a pre-v42 file: seconds, no ability's targets"
        );
        assert_eq!(refusal(Action::Open, true, false, false, Kept::ALL), None);
        assert_eq!(
            refusal(Action::Open, true, true, false, rows),
            None,
            "a fold"
        );
        assert_eq!(refusal(Action::Open, false, false, false, rows), None);
        assert_eq!(refusal(Action::Down, true, false, false, rows), None);
    }
}
