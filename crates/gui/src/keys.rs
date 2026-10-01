//! The window keymap: iced key events translated into gui-logic's
//! [`Chord`]s, and answered from its table (`wowdps_gui_logic::keys`, which
//! gui-new reads too). Bindings mirror the TUI's exactly; see
//! `wowdps-tui/src/keys.rs`.

use iced::keyboard::key::Named as IcedNamed;
use iced::keyboard::{Key, Modifiers};
use wowdps_model::Action;

pub use wowdps_gui_logic::keys::{BINDINGS, Binding, GROUPS, Surface, Zoom, key_for};
use wowdps_gui_logic::keys::{Chord, Named};

/// An iced key event as the keymap reads it. Takes the *modified* key so
/// shift-k arrives as "K", like the terminal; Ctrl matters on a character
/// only, as it always has. `None` for a key no chord can name.
pub fn chord(key: &Key, modifiers: Modifiers) -> Option<Chord<'_>> {
    match key {
        Key::Character(c) if modifiers.control() => Some(Chord::Ctrl(c.as_str())),
        Key::Character(c) => Some(Chord::Char(c.as_str())),
        Key::Named(named) => Some(Chord::Named(match named {
            IcedNamed::ArrowDown => Named::ArrowDown,
            IcedNamed::ArrowUp => Named::ArrowUp,
            IcedNamed::ArrowLeft => Named::ArrowLeft,
            IcedNamed::ArrowRight => Named::ArrowRight,
            IcedNamed::Enter => Named::Enter,
            IcedNamed::Escape => Named::Escape,
            IcedNamed::Tab => Named::Tab,
            _ => return None,
        })),
        Key::Unidentified => None,
    }
}

/// Zoom chords, checked before the meter keymap. Browser-standard bindings.
pub fn zoom_for(key: &Key, modifiers: Modifiers) -> Option<Zoom> {
    wowdps_gui_logic::keys::zoom_for(chord(key, modifiers)?)
}

/// The meter's action for an iced key event.
pub fn action_for(key: &Key, modifiers: Modifiers) -> Option<Action> {
    wowdps_gui_logic::keys::action_for(chord(key, modifiers)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_gui_logic::keys::ACTIONS;

    /// The key event that types `chord`, as iced would deliver it.
    fn event(chord: Chord<'_>) -> (Key, Modifiers) {
        match chord {
            Chord::Char(c) => (Key::Character(c.into()), Modifiers::default()),
            Chord::Ctrl(c) => (Key::Character(c.into()), Modifiers::CTRL),
            Chord::Named(n) => (
                Key::Named(match n {
                    Named::ArrowDown => IcedNamed::ArrowDown,
                    Named::ArrowUp => IcedNamed::ArrowUp,
                    Named::ArrowLeft => IcedNamed::ArrowLeft,
                    Named::ArrowRight => IcedNamed::ArrowRight,
                    Named::Enter => IcedNamed::Enter,
                    Named::Escape => IcedNamed::Escape,
                    Named::Tab => IcedNamed::Tab,
                }),
                Modifiers::default(),
            ),
        }
    }

    /// Every chord in the table reaches its action through iced: each key
    /// it names is one iced delivers, read back as the same chord.
    #[test]
    fn every_bound_chord_reaches_its_action_through_iced() {
        for &(chord, action) in ACTIONS {
            let (key, modifiers) = event(chord);
            assert_eq!(super::chord(&key, modifiers), Some(chord), "{chord:?}");
            assert_eq!(action_for(&key, modifiers), Some(action), "{chord:?}");
        }
    }

    /// Shift arrives in the character, Ctrl only matters on a character,
    /// and a key the table has no name for does nothing.
    #[test]
    fn modifiers_reach_the_chord_as_the_terminal_delivers_them() {
        let k = |c: &str| Key::Character(c.into());
        assert_eq!(chord(&k("K"), Modifiers::SHIFT), Some(Chord::Char("K")));
        assert_eq!(chord(&k("c"), Modifiers::CTRL), Some(Chord::Ctrl("c")));
        assert_eq!(
            action_for(&Key::Named(IcedNamed::Enter), Modifiers::CTRL),
            Some(Action::Open),
            "Ctrl leaves a named key alone"
        );
        assert_eq!(
            action_for(&Key::Named(IcedNamed::F5), Modifiers::default()),
            None
        );
        assert_eq!(action_for(&Key::Unidentified, Modifiers::default()), None);
        assert_eq!(zoom_for(&k("="), Modifiers::CTRL), Some(Zoom::In));
        assert_eq!(zoom_for(&k("="), Modifiers::default()), None);
    }
}
