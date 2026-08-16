//! CLAUDE.md/CONTRACT.md: "GUI keybinds mirror the TUI's". The two keymaps
//! live in crates that don't depend on each other (tui pulls crossterm, gui
//! pulls iced), so the compiler can't compare them — like `no_engine.rs`,
//! this test reads the sibling crate's source instead and compares the
//! extracted key→action tables.
//!
//! Extraction is deliberately dumb line parsing of the match arms:
//!   tui:  `KeyCode::Char('d') => Action::SetView(View::Damage),`
//!   gui:  `"d" => Action::SetView(View::Damage),`
//! and named keys:
//!   tui:  `KeyCode::Enter => Action::Open,`
//!   gui:  `Named::Enter => Action::Open,`
//! If either file restructures beyond that shape, the self-checks below fail
//! loudly rather than letting the parity assertion pass on an empty table.

use std::collections::BTreeMap;

const TUI_KEYS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/keys.rs");
const GUI_KEYS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../gui/src/keys.rs");

/// GUI-only bindings the contract itself carves out (R12: the TUI binds
/// neither, so `Screen::Compare` is unreachable there).
const GUI_ONLY_CHARS: &[(&str, &str)] = &[("v", "PickCompare"), ("g", "ToggleGraph")];

/// The terminal delivers shift-tab as its own key code; iced has no BackTab.
const TUI_ONLY_NAMED: &[&str] = &["BackTab"];

fn read(path: &str) -> String {
    let text = std::fs::read_to_string(path);
    assert!(text.is_ok(), "{path}: unreadable — keymap moved?");
    text.unwrap_or_default()
}

/// The action text after `=> Action::`, e.g. `SetView(View::Damage)`.
fn action_of(line: &str) -> Option<String> {
    let (_, action) = line.split_once("=> Action::")?;
    Some(action.trim().trim_end_matches(',').to_string())
}

/// tui char arms: `KeyCode::Char('x') ... => Action::Y`. Arms guarded by a
/// modifier (`if ctrl`) are skipped — chords aren't part of the mirror.
fn tui_chars(src: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in src.lines() {
        if line.contains(" if ") {
            continue;
        }
        let Some(rest) = line.split_once("KeyCode::Char('").map(|(_, r)| r) else {
            continue;
        };
        let Some((ch, _)) = rest.split_once("')") else {
            continue;
        };
        if let Some(action) = action_of(line) {
            map.insert(ch.to_string(), action);
        }
    }
    map
}

/// gui char arms: `"x" => Action::Y` (the zoom table and the test module
/// never match that shape — zoom arms say `Zoom::`, tests say `Some(`).
fn gui_chars(src: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        if !line.contains("=> Action::") {
            continue;
        }
        let Some(after_quote) = trimmed.strip_prefix('"') else {
            continue;
        };
        let Some((ch, _)) = after_quote.split_once('"') else {
            continue;
        };
        if let Some(action) = action_of(line) {
            map.insert(ch.to_string(), action);
        }
    }
    map
}

/// tui named-key arms: every non-Char `KeyCode::X` ident on an action line
/// (an arm like `Char('j') | KeyCode::Down` contributes `Down`).
fn tui_named(src: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in src.lines() {
        // Guarded arms (modifier chords) aren't part of the mirror; pure
        // char arms are handled by `tui_chars`, but a mixed arm like
        // `Char('j') | KeyCode::Down` still contributes its named half here.
        if line.contains(" if ") {
            continue;
        }
        let Some(action) = action_of(line) else {
            continue;
        };
        let mut rest = line;
        while let Some((_, tail)) = rest.split_once("KeyCode::") {
            let ident: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if ident != "Char" && !ident.is_empty() {
                map.insert(ident, action.clone());
            }
            rest = tail;
        }
    }
    map
}

/// gui named-key arms: `Named::X => Action::Y`.
fn gui_named(src: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in src.lines() {
        let Some(action) = action_of(line) else {
            continue;
        };
        let Some((_, tail)) = line.split_once("Named::") else {
            continue;
        };
        let ident: String = tail
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !ident.is_empty() {
            map.insert(ident, action);
        }
    }
    map
}

/// crossterm and iced name the same physical keys differently.
fn normalize_tui_named(name: &str) -> &str {
    match name {
        "Down" => "ArrowDown",
        "Up" => "ArrowUp",
        "Left" => "ArrowLeft",
        "Right" => "ArrowRight",
        "Esc" => "Escape",
        other => other,
    }
}

#[test]
fn gui_char_bindings_mirror_the_tui() {
    let tui = tui_chars(&read(TUI_KEYS));
    let gui = gui_chars(&read(GUI_KEYS));

    // Self-check: the TUI binds 11 chars and the GUI 13 today. Far fewer
    // means the extraction broke, not that the keymaps shrank.
    assert!(
        tui.len() >= 10,
        "extracted only {} tui char bindings ({tui:?}) — match-arm shape changed?",
        tui.len()
    );
    assert!(
        gui.len() >= 12,
        "extracted only {} gui char bindings ({gui:?}) — match-arm shape changed?",
        gui.len()
    );

    for (ch, action) in &tui {
        assert_eq!(
            gui.get(ch),
            Some(action),
            "tui binds {ch:?} to {action} but the gui does not mirror it"
        );
    }
    for (ch, action) in &gui {
        if tui.get(ch) == Some(action) {
            continue;
        }
        assert!(
            GUI_ONLY_CHARS.contains(&(ch.as_str(), action.as_str())),
            "gui binds {ch:?} to {action}, absent from the tui and not a \
             contract-sanctioned GUI-only binding (R12: v/g only)"
        );
    }
}

#[test]
fn gui_named_key_bindings_mirror_the_tui() {
    let tui = tui_named(&read(TUI_KEYS));
    let gui = gui_named(&read(GUI_KEYS));

    // Self-check: arrows, Enter, Esc, Tab (+BackTab tui-side) exist today.
    assert!(
        tui.len() >= 6,
        "extracted only {} tui named bindings ({tui:?}) — match-arm shape changed?",
        tui.len()
    );
    assert!(
        gui.len() >= 6,
        "extracted only {} gui named bindings ({gui:?}) — match-arm shape changed?",
        gui.len()
    );

    for (name, action) in &tui {
        if TUI_ONLY_NAMED.contains(&name.as_str()) {
            continue;
        }
        let normalized = normalize_tui_named(name);
        assert_eq!(
            gui.get(normalized),
            Some(action),
            "tui binds {name} to {action} but the gui does not mirror it as {normalized}"
        );
    }
    for (name, action) in &gui {
        let mirrored = tui
            .iter()
            .any(|(t, a)| normalize_tui_named(t) == name && a == action);
        assert!(
            mirrored,
            "gui binds {name} to {action}, absent from the tui keymap"
        );
    }
}
