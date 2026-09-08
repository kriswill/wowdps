//! The window keymap: iced key events translated into core `Action`s.
//! Bindings mirror the TUI's exactly; see `wowdps-tui/src/keys.rs`.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use wowdps_model::Action;
use wowdps_model::View;

/// One row of the `?` sheet. The table is the documentation source for that
/// sheet AND a test surface: `bindings_table_covers_every_action_key` holds
/// it against `action_for`, so a key that stops working stops being
/// advertised in the same commit.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// As the user types it: "d", "esc", "ctrl +".
    pub keys: &'static str,
    pub what: &'static str,
    pub group: &'static str,
    /// Handled window-side (Home, the talent viewer, the filter, the sheet)
    /// rather than by `action_for`. Window-local keys are deliberately NOT
    /// in `action_for`: `crates/tui/tests/keybind_parity.rs` reads this
    /// file and would call them un-mirrored TUI bindings.
    pub window_local: bool,
}

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: "d",
        what: "damage",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "h",
        what: "healing",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "T",
        what: "damage taken",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "i",
        what: "interrupts",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "c",
        what: "crowd control",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "x",
        what: "dispels",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "K",
        what: "deaths",
        group: "views",
        window_local: false,
    },
    Binding {
        keys: "j",
        what: "move down",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "k",
        what: "move up",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "[",
        what: "older segment",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "]",
        what: "newer segment",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "enter",
        what: "open / drill in",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "tab",
        what: "swap drill pane",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "esc",
        what: "back one level",
        group: "move",
        window_local: false,
    },
    Binding {
        keys: "v",
        what: "pick for comparison",
        group: "screens",
        window_local: false,
    },
    Binding {
        keys: "g",
        what: "graph mode",
        group: "screens",
        window_local: false,
    },
    Binding {
        keys: "t",
        what: "talents",
        group: "screens",
        window_local: true,
    },
    Binding {
        keys: "~",
        what: "home",
        group: "screens",
        window_local: true,
    },
    Binding {
        keys: "m",
        what: "back to the live meter",
        group: "screens",
        window_local: true,
    },
    Binding {
        keys: "/",
        what: "filter rows — name, class, spec, role",
        group: "screens",
        window_local: true,
    },
    Binding {
        keys: "?",
        what: "this sheet",
        group: "screens",
        window_local: true,
    },
    Binding {
        keys: "q",
        what: "quit",
        group: "screens",
        window_local: false,
    },
    Binding {
        keys: "ctrl +",
        what: "zoom in",
        group: "zoom",
        window_local: true,
    },
    Binding {
        keys: "ctrl -",
        what: "zoom out",
        group: "zoom",
        window_local: true,
    },
    Binding {
        keys: "ctrl 0",
        what: "reset zoom",
        group: "zoom",
        window_local: true,
    },
];

/// The sheet's group order — the order a reader wants them in, not the
/// order the table happens to list them.
pub const GROUPS: [&str; 4] = ["views", "move", "screens", "zoom"];

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
                // R17: Shift-T — lowercase t is the window's talent viewer.
                "T" => Action::SetView(View::Taken),
                // R12. Not "c" (CrowdControl) and not "p" (free, but "v" for
                // versus is what the footer can say in one letter).
                "v" => Action::PickCompare,
                "g" => Action::ToggleGraph,
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
        assert_eq!(ch("T"), Some(Action::SetView(View::Taken)));
        // Lowercase t stays free for the window's talent viewer.
        assert_eq!(ch("t"), None);
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
    fn compare_keys() {
        assert_eq!(ch("v"), Some(Action::PickCompare));
        assert_eq!(ch("g"), Some(Action::ToggleGraph));
        // "c" must stay the CrowdControl view.
        assert_eq!(ch("c"), Some(Action::SetView(View::CrowdControl)));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(ch("z"), None);
        assert_eq!(named(Named::F5), None);
    }

    /// The sheet must describe the keymap that exists: every non-local
    /// binding is answered by `action_for`, and every character key
    /// `action_for` answers is advertised.
    #[test]
    fn bindings_table_covers_every_action_key() {
        for b in BINDINGS {
            assert!(GROUPS.contains(&b.group), "{b:?} is in no group");
            if b.window_local {
                continue;
            }
            let answered = match b.keys {
                "enter" => named(Named::Enter),
                "tab" => named(Named::Tab),
                "esc" => named(Named::Escape),
                c => ch(c),
            };
            assert!(answered.is_some(), "{b:?} is advertised but does nothing");
        }
        for c in [
            "q", "d", "h", "i", "c", "x", "K", "T", "v", "g", "j", "k", "[", "]",
        ] {
            assert!(
                BINDINGS.iter().any(|b| b.keys == c),
                "{c} works but is not on the sheet"
            );
        }
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
