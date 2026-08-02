//! The window keymap: iced key events translated into core `Action`s.
//! Bindings mirror the TUI's exactly; see `wowdps-tui/src/keys.rs`.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use wowdps_core::app::Action;
use wowdps_core::model::View;

/// Zoom chords, checked before the meter keymap. Browser-standard bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

pub fn zoom_for(key: &Key, modifiers: Modifiers) -> Option<Zoom> {
    if !modifiers.control() {
        return None;
    }
    match key {
        Key::Character(c) => match c.as_str() {
            "=" | "+" => Some(Zoom::In),
            "-" => Some(Zoom::Out),
            "0" => Some(Zoom::Reset),
            _ => None,
        },
        _ => None,
    }
}

/// Takes the *modified* key so shift-k arrives as "K", like the terminal.
pub fn action_for(key: &Key, modifiers: Modifiers) -> Option<Action> {
    Some(match key {
        Key::Character(c) => {
            if modifiers.control() {
                return (c.as_str() == "c").then_some(Action::Quit);
            }
            match c.as_str() {
                "q" => Action::Quit,
                "d" => Action::SetView(View::Damage),
                "h" => Action::SetView(View::Healing),
                "i" => Action::SetView(View::Interrupts),
                "c" => Action::SetView(View::CrowdControl),
                "x" => Action::SetView(View::Dispels),
                // Shift-K, because lowercase k is vim-style "move up".
                "K" => Action::SetView(View::Deaths),
                "j" => Action::Down,
                "k" => Action::Up,
                "[" => Action::OlderSegment,
                "]" => Action::NewerSegment,
                _ => return None,
            }
        }
        Key::Named(named) => match named {
            Named::ArrowDown => Action::Down,
            Named::ArrowUp => Action::Up,
            Named::ArrowLeft => Action::OlderSegment,
            Named::ArrowRight => Action::NewerSegment,
            Named::Enter => Action::Open,
            Named::Escape => Action::Back,
            Named::Tab => Action::SwapPane,
            _ => return None,
        },
        Key::Unidentified => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Option<Action> {
        action_for(&Key::Character(c.into()), Modifiers::default())
    }

    fn named(n: Named) -> Option<Action> {
        action_for(&Key::Named(n), Modifiers::default())
    }

    #[test]
    fn every_view_has_a_key() {
        assert_eq!(ch("d"), Some(Action::SetView(View::Damage)));
        assert_eq!(ch("h"), Some(Action::SetView(View::Healing)));
        assert_eq!(ch("i"), Some(Action::SetView(View::Interrupts)));
        assert_eq!(ch("c"), Some(Action::SetView(View::CrowdControl)));
        assert_eq!(ch("x"), Some(Action::SetView(View::Dispels)));
        assert_eq!(ch("K"), Some(Action::SetView(View::Deaths)));
    }

    #[test]
    fn movement_and_control_keys() {
        assert_eq!(ch("j"), Some(Action::Down));
        assert_eq!(ch("k"), Some(Action::Up));
        assert_eq!(named(Named::ArrowDown), Some(Action::Down));
        assert_eq!(named(Named::ArrowUp), Some(Action::Up));
        assert_eq!(ch("["), Some(Action::OlderSegment));
        assert_eq!(ch("]"), Some(Action::NewerSegment));
        assert_eq!(named(Named::Enter), Some(Action::Open));
        assert_eq!(named(Named::Escape), Some(Action::Back));
        assert_eq!(named(Named::Tab), Some(Action::SwapPane));
        assert_eq!(ch("q"), Some(Action::Quit));
    }

    #[test]
    fn ctrl_c_quits_and_other_ctrl_chords_do_nothing() {
        assert_eq!(
            action_for(&Key::Character("c".into()), Modifiers::CTRL),
            Some(Action::Quit)
        );
        assert_eq!(
            action_for(&Key::Character("d".into()), Modifiers::CTRL),
            None
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(ch("z"), None);
        assert_eq!(named(Named::F5), None);
    }

    #[test]
    fn zoom_needs_ctrl_and_uses_browser_bindings() {
        let k = |c: &str| Key::Character(c.into());
        assert_eq!(zoom_for(&k("="), Modifiers::CTRL), Some(Zoom::In));
        assert_eq!(zoom_for(&k("+"), Modifiers::CTRL), Some(Zoom::In));
        assert_eq!(zoom_for(&k("-"), Modifiers::CTRL), Some(Zoom::Out));
        assert_eq!(zoom_for(&k("0"), Modifiers::CTRL), Some(Zoom::Reset));
        assert_eq!(zoom_for(&k("="), Modifiers::default()), None);
        assert_eq!(zoom_for(&k("z"), Modifiers::CTRL), None);
    }
}
