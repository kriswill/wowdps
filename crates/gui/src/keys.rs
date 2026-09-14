//! The window keymap: iced key events translated into core `Action`s.
//! Bindings mirror the TUI's exactly; see `wowdps-tui/src/keys.rs`.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use wowdps_model::Action;
use wowdps_model::View;

/// Which screen the window is showing — what the `?` sheet keys its "here"
/// column on. The sheet answers "what can I press NOW", so a binding names
/// the surfaces it works on and the sheet sorts the rest into "elsewhere".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The segment list.
    List,
    /// The meter's player rows.
    Meter,
    /// A player's drill: the by-spell / by-target panes.
    Drill,
    /// The second drill level — one ability.
    Ability,
    /// R12: two players side by side.
    Compare,
    /// The Home dashboard.
    Home,
    /// The talent viewer.
    Talents,
    /// The History screen, and the stored fight it opens.
    History,
}

impl Surface {
    pub fn name(self) -> &'static str {
        match self {
            Surface::List => "fight list",
            Surface::Meter => "meter",
            Surface::Drill => "player drill",
            Surface::Ability => "ability drill",
            Surface::Compare => "comparison",
            Surface::Home => "home",
            Surface::Talents => "talents",
            Surface::History => "history",
        }
    }
}

const EVERYWHERE: &[Surface] = &[
    Surface::List,
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
    Surface::Talents,
    Surface::History,
];
/// Every surface the shared state machine draws (not the window-local ones).
const FIGHTS: &[Surface] = &[
    Surface::List,
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
];
/// The meter and everything under it: where a view key changes the numbers.
const METERS: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::History,
];
/// Where j/k walk a list.
const LISTS: &[Surface] = &[
    Surface::List,
    Surface::Meter,
    Surface::Drill,
    Surface::History,
];
/// Everywhere the talent viewer can be opened from: it is not modal over
/// itself.
const NOT_TALENTS: &[Surface] = &[
    Surface::List,
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
];
/// Where Esc backs out a level — everywhere but the front door.
const BACKABLE: &[Surface] = &[
    Surface::List,
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
    Surface::Talents,
    Surface::History,
];

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
    /// The surfaces the key does something on. The sheet lists a binding
    /// under "here" when the current surface is one of them.
    pub on: &'static [Surface],
}

impl Binding {
    pub fn applies(&self, surface: Surface) -> bool {
        self.on.contains(&surface)
    }
}

const fn b(
    keys: &'static str,
    what: &'static str,
    group: &'static str,
    window_local: bool,
    on: &'static [Surface],
) -> Binding {
    Binding {
        keys,
        what,
        group,
        window_local,
        on,
    }
}

pub const BINDINGS: &[Binding] = &[
    b("d", "damage", "views", false, METERS),
    b("h", "healing", "views", false, METERS),
    b("T", "damage taken", "views", false, METERS),
    b("i", "interrupts", "views", false, METERS),
    b("c", "crowd control", "views", false, METERS),
    b("x", "dispels", "views", false, METERS),
    b("K", "deaths", "views", false, METERS),
    b("j", "move down", "move", false, LISTS),
    b("k", "move up", "move", false, LISTS),
    b("[", "older segment", "move", false, FIGHTS),
    b("]", "newer segment", "move", false, FIGHTS),
    b(
        "enter",
        "open / drill in",
        "move",
        false,
        &[
            Surface::List,
            Surface::Meter,
            Surface::Drill,
            Surface::History,
        ],
    ),
    b(
        "tab",
        "swap drill pane",
        "move",
        false,
        &[Surface::Drill, Surface::Talents],
    ),
    b("esc", "back one level", "move", false, BACKABLE),
    b(
        "v",
        "pick for comparison",
        "screens",
        false,
        &[Surface::Meter, Surface::Compare],
    ),
    b(
        "g",
        "graph mode",
        "screens",
        false,
        &[Surface::Drill, Surface::Ability, Surface::Compare],
    ),
    b("t", "talents", "screens", true, NOT_TALENTS),
    b("~", "home", "screens", true, EVERYWHERE),
    b("H", "history", "screens", true, EVERYWHERE),
    b(
        "p",
        "pin / release the stored fight",
        "screens",
        true,
        &[Surface::History],
    ),
    b("m", "back to the live meter", "screens", true, EVERYWHERE),
    b(
        "/",
        "filter rows — name, class, spec, role",
        "screens",
        true,
        &[Surface::Meter],
    ),
    b("?", "this sheet", "screens", true, EVERYWHERE),
    b("q", "quit", "screens", false, EVERYWHERE),
    b("ctrl +", "zoom in", "zoom", true, EVERYWHERE),
    b("ctrl -", "zoom out", "zoom", true, EVERYWHERE),
    b("ctrl 0", "reset zoom", "zoom", true, EVERYWHERE),
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
