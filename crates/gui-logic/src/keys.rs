//! The window keymap as data: which [`Chord`] does which core `Action`
//! ([`ACTIONS`]), and the `?` sheet's rows ([`BINDINGS`]). A GUI translates
//! its framework's key event into a `Chord` and asks [`action_for`]; the
//! iced GUI's `keys.rs` is that translation and nothing else. Bindings
//! mirror the TUI's exactly (`wowdps-tui/src/keys.rs`), which
//! `crates/tui/tests/keybind_parity.rs` holds by iterating [`ACTIONS`].

use wowdps_model::Action;
use wowdps_model::View;

/// A key the GUIs bind beyond the characters, under the names iced gives
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    ArrowDown,
    ArrowUp,
    ArrowLeft,
    ArrowRight,
    Enter,
    Escape,
    Tab,
}

/// A key press as the keymap reads it, before any framework names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord<'a> {
    /// A character key with no Ctrl: the *modified* character, so shift-k
    /// arrives as "K", as it does in a terminal.
    Char(&'a str),
    /// A character with Ctrl held, likewise modified ("c" for ctrl-c).
    Ctrl(&'a str),
    /// A named key, whatever modifiers were held.
    Named(Named),
}

/// Every chord the meter keymap answers, and its action: the TUI's
/// bindings, mirrored, plus R12's `v` and `g`. A chord in no row does
/// nothing — a Ctrl chord too, but for ctrl-c.
pub const ACTIONS: &[(Chord<'static>, Action)] = &[
    (Chord::Char("q"), Action::Quit),
    (Chord::Ctrl("c"), Action::Quit),
    (Chord::Char("d"), Action::SetView(View::Damage)),
    (Chord::Char("h"), Action::SetView(View::Healing)),
    (Chord::Char("i"), Action::SetView(View::Interrupts)),
    (Chord::Char("c"), Action::SetView(View::CrowdControl)),
    (Chord::Char("x"), Action::SetView(View::Dispels)),
    // Shift-K, because lowercase k is vim-style "move up".
    (Chord::Char("K"), Action::SetView(View::Deaths)),
    // R17: Shift-T — lowercase t is the window's talent viewer.
    (Chord::Char("T"), Action::SetView(View::Taken)),
    // R24: Shift-E — the enemy's side of the same ledger.
    (Chord::Char("E"), Action::SetView(View::EnemyTaken)),
    // R12. Not "c" (CrowdControl) and not "p" (free, but "v" for versus is
    // what the footer can say in one letter).
    (Chord::Char("v"), Action::PickCompare),
    (Chord::Char("g"), Action::ToggleGraph),
    (Chord::Char("j"), Action::Down),
    (Chord::Char("k"), Action::Up),
    (Chord::Char("["), Action::OlderSegment),
    (Chord::Char("]"), Action::NewerSegment),
    (Chord::Named(Named::ArrowDown), Action::Down),
    (Chord::Named(Named::ArrowUp), Action::Up),
    (Chord::Named(Named::ArrowLeft), Action::OlderSegment),
    (Chord::Named(Named::ArrowRight), Action::NewerSegment),
    (Chord::Named(Named::Enter), Action::Open),
    (Chord::Named(Named::Escape), Action::Back),
    (Chord::Named(Named::Tab), Action::SwapPane),
];

/// The meter's action for `chord`, if it has one.
pub fn action_for(chord: Chord<'_>) -> Option<Action> {
    ACTIONS
        .iter()
        .find(|(c, _)| *c == chord)
        .map(|&(_, action)| action)
}

/// Which screen the window is showing — what the `?` sheet keys its "here"
/// column on. The sheet answers "what can I press NOW", so a binding names
/// the surfaces it works on and the sheet sorts the rest into "elsewhere".
/// A pull is one workspace, the tailed log's or a stored one: the meter and
/// what opens beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
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
    /// The pull rail's drawer, open over the stage (or Home) with the keys.
    Rail,
}

impl Surface {
    /// The surface as a sentence names it — "What works on Home", "… on
    /// the meter" — its article with it, since Home is a name and the
    /// meter is not.
    pub fn name(self) -> &'static str {
        match self {
            Surface::Meter => "the meter",
            Surface::Drill => "a player's drill",
            Surface::Ability => "an ability's drill",
            Surface::Compare => "the comparison",
            Surface::Home => "Home",
            Surface::Talents => "the talent viewer",
            Surface::Rail => "the pull list",
        }
    }
}

const EVERYWHERE: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
    Surface::Talents,
    Surface::Rail,
];
/// The meter and everything under it: where a view key changes the numbers.
const METERS: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
];
/// Where a view key works: the meter and everything under it, and Home,
/// which it leaves for the pull on that view.
const VIEW_KEYS: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
];
/// Where `[` and `]` walk the pull rail: a pull's workspace, Home, which
/// they leave for the pull they land on (from the rail's top), and the
/// drawer, which stays open on the row they land on.
const PULLS: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
    Surface::Rail,
];
/// Where j/k walk a list — a comparison's walk the meter, whose selection
/// is the pair's second half; the drawer's, its rows.
const LISTS: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Compare,
    Surface::Rail,
];
/// Everywhere the talent viewer can be opened from: it is not modal over
/// itself.
const NOT_TALENTS: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Home,
];
/// Where Esc backs out a level — everywhere but Home, where the chain ends
/// (the rail's drawer over Home is the drawer's surface, and Esc shuts it).
const BACKABLE: &[Surface] = &[
    Surface::Meter,
    Surface::Drill,
    Surface::Ability,
    Surface::Compare,
    Surface::Talents,
    Surface::Rail,
];

/// One row of the `?` sheet. The table is the documentation source for that
/// sheet AND a test surface: `bindings_table_covers_every_action_key` holds
/// it against `action_for`, so a key that stops working stops being
/// advertised in the same commit.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// As the user types it: "d", "esc", "ctrl +" — and two keys that do
    /// one thing in two directions on one line, "j k", "[ ]".
    pub keys: &'static str,
    pub what: &'static str,
    /// One of [`GROUPS`].
    pub group: &'static str,
    /// Handled window-side (Home, the command palette, the talent viewer,
    /// the filter, the sheet) rather than by `action_for`. Window-local
    /// keys are deliberately NOT in [`ACTIONS`]:
    /// `crates/tui/tests/keybind_parity.rs` iterates it and would call
    /// them un-mirrored TUI bindings. This module's own test holds the rest
    /// of the table against `action_for`, and [`key_for`] reads only the
    /// rest; the sheet draws every key alike, as the prototype's does.
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
    // ---- Move ----------------------------------------------------------
    b("j k", "next or previous player", "move", false, LISTS),
    // The window's own reading of j/k on the Deaths table (R25): the deaths
    // in the order they happened, each step its recap — and Enter there
    // hands the recap nothing (a narrow window's pushes it over).
    b(
        "j k",
        "deaths: each in turn",
        "move",
        true,
        &[Surface::Meter],
    ),
    // `action_for`'s older and newer segment, which the window walks over
    // the pull rail — tonight's log, then the stored nights.
    b("[ ]", "older or newer pull", "move", false, PULLS),
    // The window's own: ← → walk the rail as `[` `]` do — but in a Deaths
    // recap the keys are in, they step the player's deaths.
    b("← →", "pull, or recap's deaths", "move", true, PULLS),
    b(
        "enter",
        "open or inspect",
        "move",
        false,
        &[
            Surface::Meter,
            Surface::Drill,
            Surface::Compare,
            Surface::Rail,
        ],
    ),
    b("esc", "back one level", "move", false, BACKABLE),
    // ---- Views: the prototype's order and names (`view::WINDOW_VIEWS`) --
    b("d", "damage", "views", false, VIEW_KEYS),
    b("h", "healing", "views", false, VIEW_KEYS),
    b("T", "taken", "views", false, VIEW_KEYS),
    b("K", "deaths", "views", false, VIEW_KEYS),
    b("i", "interrupts", "views", false, VIEW_KEYS),
    b("c", "crowd control", "views", false, VIEW_KEYS),
    b("x", "dispels", "views", false, VIEW_KEYS),
    b("E", "enemies", "views", false, VIEW_KEYS),
    // ---- Inspector -----------------------------------------------------
    b(
        "tab",
        "abilities or targets",
        "inspector",
        false,
        &[Surface::Meter, Surface::Drill, Surface::Talents],
    ),
    b(
        "v",
        "pin for comparison, or stop",
        "inspector",
        false,
        &[Surface::Meter, Surface::Drill, Surface::Compare],
    ),
    b(
        "g",
        "per second or cumulative",
        "inspector",
        false,
        &[
            Surface::Meter,
            Surface::Drill,
            Surface::Ability,
            Surface::Compare,
        ],
    ),
    // The window's own (R26): in a Damage or Healing ability list the keys
    // are in, ← → fold the ability tree — a pet's summon, a trinket, a
    // spell's direct and over-time parts — and Enter on a group folds it.
    b(
        "← →",
        "fold or unfold an ability",
        "inspector",
        true,
        &[Surface::Drill],
    ),
    b("t", "talents and gear", "inspector", true, NOT_TALENTS),
    // The window's own: the pull on the stage's stored card, pinned or let
    // go — what keeps it from retention — from anywhere on its stage.
    b("p", "pin the pull, or let it go", "inspector", true, METERS),
    // ---- Go to ---------------------------------------------------------
    // The command palette: every pull, player, view and screen by name.
    b("ctrl K", "jump to anything", "go to", true, EVERYWHERE),
    b("/", "filter players", "go to", true, &[Surface::Meter]),
    b("m", "live pull", "go to", true, EVERYWHERE),
    b("~", "home", "go to", true, EVERYWHERE),
    b("H", "earlier nights", "go to", true, EVERYWHERE),
    b("?", "this sheet", "go to", true, EVERYWHERE),
    // How large the window draws it all — the window's, not a view's: in
    // the last group, so Views lists the views alone, as the prototype's
    // does, and the first row of groups keeps its height.
    b("ctrl +", "zoom in", "go to", true, EVERYWHERE),
    b("ctrl -", "zoom out", "go to", true, EVERYWHERE),
    b("ctrl 0", "reset zoom", "go to", true, EVERYWHERE),
    b("q", "quit", "go to", false, EVERYWHERE),
];

/// The sheet's groups, in the prototype's order (`.sheet .cols`): moving,
/// what the numbers are, the inspector's own keys, and where to go.
pub const GROUPS: [&str; 4] = ["move", "views", "inspector", "go to"];

/// The single key `action_for` answers with `action`, as the sheet spells
/// it — the key the command palette prints beside a view. Read off the
/// table, so the two cannot disagree.
pub fn key_for(action: Action) -> Option<&'static str> {
    BINDINGS.iter().find_map(|b| {
        let k = b.keys;
        let single = !b.window_local && !k.contains(' ');
        let answered = action_for(Chord::Char(k));
        (single && answered == Some(action)).then_some(k)
    })
}

/// Zoom chords, checked before the meter keymap. Browser-standard bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

pub fn zoom_for(chord: Chord<'_>) -> Option<Zoom> {
    match chord {
        Chord::Ctrl("=" | "+") => Some(Zoom::In),
        Chord::Ctrl("-") => Some(Zoom::Out),
        Chord::Ctrl("0") => Some(Zoom::Reset),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Option<Action> {
        action_for(Chord::Char(c))
    }

    fn named(n: Named) -> Option<Action> {
        action_for(Chord::Named(n))
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
        assert_eq!(ch("E"), Some(Action::SetView(View::EnemyTaken)));
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
        assert_eq!(action_for(Chord::Ctrl("c")), Some(Action::Quit));
        assert_eq!(action_for(Chord::Ctrl("d")), None);
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
        assert_eq!(action_for(Chord::Ctrl("z")), None);
    }

    /// `action_for` takes the first row a chord matches, so a second row
    /// for the same chord would be dead — and the parity test would count
    /// it as a binding.
    #[test]
    fn every_chord_is_bound_once() {
        for (i, (chord, _)) in ACTIONS.iter().enumerate() {
            let again = ACTIONS.iter().skip(i + 1).any(|(c, _)| c == chord);
            assert!(!again, "{chord:?} is bound twice");
        }
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
            // A line of two keys ("j k", "[ ]") advertises both.
            for key in b.keys.split_whitespace() {
                let answered = match key {
                    "enter" => named(Named::Enter),
                    "tab" => named(Named::Tab),
                    "esc" => named(Named::Escape),
                    c => ch(c),
                };
                assert!(
                    answered.is_some(),
                    "{key} of {b:?} is advertised but does nothing"
                );
            }
        }
        for c in [
            "q", "d", "h", "i", "c", "x", "K", "T", "E", "v", "g", "j", "k", "[", "]",
        ] {
            assert!(
                BINDINGS
                    .iter()
                    .any(|b| !b.window_local && b.keys.split_whitespace().any(|k| k == c)),
                "{c} works but is not on the sheet"
            );
        }
    }

    /// The sheet's four groups, the prototype's: every group holds keys,
    /// and the keys the redesign added are on it, each in its group —
    /// every one of them the window's own, kept out of `action_for`.
    #[test]
    fn the_sheet_groups_are_the_prototype_s() {
        assert_eq!(GROUPS, ["move", "views", "inspector", "go to"]);
        for g in GROUPS {
            assert!(BINDINGS.iter().any(|b| b.group == g), "{g} is empty");
        }
        // Views lists the views and nothing else, as the prototype's does.
        let views: Vec<&str> = BINDINGS
            .iter()
            .filter(|b| b.group == "views")
            .map(|b| b.keys)
            .collect();
        assert_eq!(views, ["d", "h", "T", "K", "i", "c", "x", "E"]);
        let group_of = |keys: &str| BINDINGS.iter().find(|b| b.keys == keys).map(|b| b.group);
        for (keys, group) in [
            ("tab", "inspector"),
            ("v", "inspector"),
            ("g", "inspector"),
            ("t", "inspector"),
            ("ctrl K", "go to"),
            ("/", "go to"),
            ("m", "go to"),
            ("~", "go to"),
            ("H", "go to"),
            ("?", "go to"),
        ] {
            assert_eq!(group_of(keys), Some(group), "{keys}");
        }
        for keys in ["t", "ctrl K", "/", "m", "~", "H", "?"] {
            let b = BINDINGS.iter().find(|b| b.keys == keys).unwrap();
            assert!(b.window_local, "{keys} is the window's own");
            if let Some(c) = keys.strip_prefix("ctrl ") {
                assert_eq!(
                    action_for(Chord::Ctrl(&c.to_lowercase())),
                    None,
                    "{keys} must stay out of action_for"
                );
            } else {
                assert_eq!(ch(keys), None, "{keys} must stay out of action_for");
            }
        }
    }

    /// The palette's keycap for each view is the key that switches to it.
    #[test]
    fn every_view_s_key_is_read_off_the_table() {
        for v in View::ALL {
            let key = key_for(Action::SetView(v)).expect("every view has a key");
            assert_eq!(ch(key), Some(Action::SetView(v)), "{v:?}");
        }
        assert_eq!(key_for(Action::SetView(View::Taken)), Some("T"));
        assert_eq!(key_for(Action::Quit), Some("q"));
        assert_eq!(key_for(Action::Open), None, "a named key is not a char");
    }

    #[test]
    fn zoom_needs_ctrl_and_uses_browser_bindings() {
        assert_eq!(zoom_for(Chord::Ctrl("=")), Some(Zoom::In));
        assert_eq!(zoom_for(Chord::Ctrl("+")), Some(Zoom::In));
        assert_eq!(zoom_for(Chord::Ctrl("-")), Some(Zoom::Out));
        assert_eq!(zoom_for(Chord::Ctrl("0")), Some(Zoom::Reset));
        assert_eq!(zoom_for(Chord::Char("=")), None);
        assert_eq!(zoom_for(Chord::Ctrl("z")), None);
    }
}
