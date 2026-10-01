//! The cards' gestures through Kit's harness, over the daemon's mock: each
//! menu opened where a reader opens it, closed by any key or a press off
//! it; the options written one key at a time; the character menu's two
//! kinds of row; the toasts; and the Esc walk in the iced window's order.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext, ElementId, TestAppContext};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::home::{CharLine, FOLLOW_NOTE};
use wowdps_gui_logic::rail::Pull;
use wowdps_gui_logic::toast::{NO_CARD, NO_STORED_PAIR, NOT_STORED, TOAST_FOR};
use wowdps_model::{Action, Class, Screen, Spec, View};

use super::super::Place;
use super::super::rail::tests::{current, earlier_nights, key, rig_over as rail_rig};
use super::super::tests::{Rig, settle};
use super::Menu;
use super::menu::row_id;

/// The window over the fixture, motion reduced: a card stands where it
/// rests from its first frame, so the harness can press it (the
/// entrance's own test runs with motion).
fn rig(cx: &mut TestAppContext, w: f32, h: f32) -> Rig {
    cx.update(|cx| cx.set_reduce_motion(true));
    super::super::tests::rig(cx, w, h)
}

fn press(cx: &mut TestAppContext, rig: &Rig, id: impl Into<ElementId>) {
    let id = id.into();
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    settle(cx, &rig.session);
}

fn typed(cx: &mut TestAppContext, rig: &Rig, keys: &str) {
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.press(keys, cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(cx, &rig.session);
}

fn open(cx: &mut TestAppContext, rig: &Rig, menu: Menu) {
    cx.update_window(rig.window, |_, window, cx| {
        rig.gui.update(cx, |g, cx| g.toggle_menu(menu, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

fn shown(cx: &mut TestAppContext, rig: &Rig, id: &'static str) -> bool {
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

fn toast(cx: &mut TestAppContext, rig: &Rig) -> Option<String> {
    rig.gui.read_with(cx, |g, _| {
        g.cards_ui.toast.as_ref().map(|t| t.words.clone())
    })
}

fn view(cx: &mut TestAppContext, rig: &Rig) -> View {
    rig.gui.read_with(cx, |g, cx| g.fight(cx).view)
}

/// The gear opens the ⚙ card under it; it is modal for the keys: any key
/// closes it and does nothing else — "h" switches no view.
#[gpui_kit::test]
fn the_gear_opens_the_options_and_any_key_closes_them(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    press(cx, &rig, "top-gear");
    assert!(shown(cx, &rig, "options"));
    typed(cx, &rig, "h");
    assert!(!shown(cx, &rig, "options"), "the key closed it");
    assert_eq!(view(cx, &rig), View::Damage, "and did nothing else");
    // The gear again: open, and the gear once more closes it.
    press(cx, &rig, "top-gear");
    press(cx, &rig, "top-gear");
    assert!(!shown(cx, &rig, "options"));
}

/// Each option writes its one key to the config file as it changes, and
/// the window follows it.
#[gpui_kit::test]
fn the_options_write_one_key_each(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let before = rig.gui.read_with(cx, |g, _| g.cfg.show_ranks);
    open(cx, &rig, Menu::Options);
    press(cx, &rig, "option-ranks");
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.cfg.show_ranks, !before));
    assert_eq!(Config::load().show_ranks, !before, "written");
    press(cx, &rig, "option-realms");
    rig.gui.read_with(cx, |g, _| assert!(g.cfg.hide_realms));
    assert!(Config::load().hide_realms);
    press(cx, &rig, "chrome-class");
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.cfg.chrome, "class"));
    assert_eq!(Config::load().chrome, "class");
    press(cx, &rig, "chrome-gold");
    assert_eq!(Config::load().chrome, "gold");
    // A key the card never touched is the file's own: a zoom saved
    // meanwhile survives every write.
    Config::store(|c| c.zoom = 1.7);
    press(cx, &rig, "option-ranks");
    assert!(
        (Config::load().zoom - 1.7).abs() < 1e-5,
        "one key at a time"
    );
}

/// The pointer wandering off the ⚙ card closes it.
#[gpui_kit::test]
fn the_pointer_leaving_the_options_closes_them(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    open(cx, &rig, Menu::Options);
    cx.update_window(rig.window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("options", cx);
        window.render_frame(cx);
        window.hover("top-live", cx);
    })
    .unwrap();
    rig.gui.read_with(cx, |g, _| assert!(!g.cards.options));
}

/// `?` opens the sheet of what works here; any key closes it and does
/// nothing else, and so does a press anywhere on it.
#[gpui_kit::test]
fn the_sheet_opens_on_question_mark_and_any_key_or_press_closes_it(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let sel = rig.session.read_with(cx, |s, _| s.state().row_sel);
    typed(cx, &rig, "?");
    assert!(shown(cx, &rig, "sheet"), "the sheet");
    typed(cx, &rig, "j");
    assert!(!shown(cx, &rig, "sheet"));
    rig.session.read_with(cx, |s, _| {
        assert_eq!(s.state().row_sel, sel, "j moved nobody")
    });
    typed(cx, &rig, "?");
    press(cx, &rig, "sheet");
    assert!(!shown(cx, &rig, "sheet"), "a press on the card");
    typed(cx, &rig, "?");
    press(cx, &rig, "sheet-scrim");
    assert!(!shown(cx, &rig, "sheet"), "a press on the scrim");
}

/// The sheet is keyed on the surface: the meter's, then Home's — where
/// Esc is not listed, its chain ending there.
#[gpui_kit::test]
fn the_sheet_knows_which_surface_it_is_on(cx: &mut TestAppContext) {
    use wowdps_gui_logic::keys::Surface;
    let rig = rig(cx, 1440., 900.);
    rig.gui
        .read_with(cx, |g, cx| assert_eq!(g.surface(cx), Surface::Meter));
    typed(cx, &rig, "~");
    rig.gui
        .read_with(cx, |g, cx| assert_eq!(g.surface(cx), Surface::Home));
    // A pull the store holds no card of dims `p`.
    rig.gui.read_with(cx, |g, cx| {
        assert!(g.inert_keys(cx).is_empty(), "nothing of a pull's on Home")
    });
    typed(cx, &rig, "~");
    rig.gui
        .read_with(cx, |g, cx| assert!(g.inert_keys(cx).contains(&"p")));
}

/// Under a menu the zoom chords still zoom and the menu stays; Ctrl K
/// closes it for the palette.
#[gpui_kit::test]
fn zoom_works_under_a_menu_and_ctrl_k_replaces_it(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    typed(cx, &rig, "?");
    typed(cx, &rig, "ctrl-=");
    rig.gui.read_with(cx, |g, _| {
        assert!((g.cfg.zoom - 1.1).abs() < 1e-5, "{}", g.cfg.zoom);
        assert!(g.cards.sheet, "the sheet stays");
    });
    typed(cx, &rig, "ctrl-k");
    rig.gui.read_with(cx, |g, _| {
        assert!(!g.cards.sheet);
        assert!(g.cards.palette, "the palette over everything");
    });
}

fn known() -> Vec<CharLine> {
    vec![
        CharLine {
            guid: "Player-1-A".into(),
            name: "Alpha-Realm-US".into(),
            class: Some(Class::Mage),
            spec: Some(Spec::Fire),
            fights: 9,
            last_utc_ms: 0,
            last_local_ms: 0,
        },
        CharLine {
            guid: "Player-1-B".into(),
            name: "Beta-Realm-US".into(),
            class: None,
            spec: None,
            fights: 1,
            last_utc_ms: 0,
            last_local_ms: 0,
        },
    ]
}

/// The character menu: its check item says what the window does and
/// changes nothing; a character's row scopes Home to them, remembered;
/// a press off the card closes it.
#[gpui_kit::test]
fn the_character_menu_follows_and_scopes_home(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    rig.gui.update(cx, |g, _| g.hist.known = known());
    open(cx, &rig, Menu::Picker);
    assert!(shown(cx, &rig, "picker-menu"));
    press(cx, &rig, row_id(None));
    rig.gui.read_with(cx, |g, _| {
        assert!(!g.cards.picker, "the follow item closes the menu");
        assert_eq!(g.place, Place::Fights, "and changes nothing");
    });
    assert_eq!(toast(cx, &rig).as_deref(), Some(FOLLOW_NOTE));
    open(cx, &rig, Menu::Picker);
    press(cx, &rig, row_id(Some("Player-1-B")));
    rig.gui.read_with(cx, |g, _| {
        assert!(!g.cards.picker);
        assert_eq!(g.place, Place::Home);
        assert_eq!(g.cfg.character.as_deref(), Some("Player-1-B"));
    });
    assert_eq!(Config::load().character.as_deref(), Some("Player-1-B"));
    open(cx, &rig, Menu::Picker);
    let lit = cx
        .update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            window.find(row_id(Some("Player-1-B"))).selected()
        })
        .unwrap();
    assert_eq!(lit, Some(true), "Home's scope is the lit row");
    press(cx, &rig, "picker-scrim");
    rig.gui.read_with(cx, |g, _| assert!(!g.cards.picker));
}

/// The menu's names honour `hide_realms`.
#[gpui_kit::test]
fn the_character_menu_hides_realms_when_asked(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    rig.gui.update(cx, |g, _| {
        g.hist.known = known();
        g.cfg.hide_realms = true;
    });
    open(cx, &rig, Menu::Picker);
    let text = cx
        .update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            window
                .find(row_id(Some("Player-1-A")))
                .label()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap();
    assert!(text.contains("Alpha"), "{text:?}");
    assert!(!text.contains("Realm"), "{text:?}");
}

/// A toast passes after its 2.6 s.
#[gpui_kit::test]
fn a_toast_passes(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    rig.gui.update(cx, |g, cx| g.say("hello", cx));
    assert!(shown(cx, &rig, "toast"));
    cx.executor().advance_clock(TOAST_FOR);
    cx.run_until_parked();
    assert_eq!(toast(cx, &rig), None);
    assert!(!shown(cx, &rig, "toast"));
}

/// `p` on a pull the store holds no card of says so.
#[gpui_kit::test]
fn p_without_a_card_says_so(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    typed(cx, &rig, "p");
    assert_eq!(toast(cx, &rig).as_deref(), Some(NO_CARD));
}

/// A pin says so until the pair forms, which takes the word back.
#[gpui_kit::test]
fn a_pin_says_so_until_the_pair_forms(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    typed(cx, &rig, "v");
    let words = toast(cx, &rig).expect("the pin's word");
    assert!(words.starts_with("Pinned "), "{words}");
    typed(cx, &rig, "j");
    rig.gui
        .read_with(cx, |g, cx| assert_eq!(g.fight(cx).screen, Screen::Compare));
    assert_eq!(toast(cx, &rig), None, "the pair took it back");
}

/// A stored pull refuses what the store keeps no answer for, with a word:
/// `v` pins nobody, the enemies' view stays shut.
#[gpui_kit::test]
fn a_stored_pull_says_what_it_cannot_answer(cx: &mut TestAppContext) {
    let rig = rail_rig(cx, earlier_nights(), 1440., 900.);
    while !matches!(current(cx, &rig), Some(Pull::Stored(_))) {
        key(cx, &rig, Action::OlderSegment);
    }
    key(cx, &rig, Action::PickCompare);
    assert_eq!(toast(cx, &rig).as_deref(), Some(NO_STORED_PAIR));
    rig.gui.read_with(cx, |g, cx| {
        assert!(g.fight(cx).compare_picks().is_empty(), "nobody pinned")
    });
    key(cx, &rig, Action::SetView(View::EnemyTaken));
    assert_eq!(toast(cx, &rig).as_deref(), Some(NOT_STORED));
    assert_ne!(view(cx, &rig), View::EnemyTaken);
}

/// On a stored pull the inspector's Compare stands inert, saying why, and
/// the enemies' tab stays on the strip leading nowhere — the shapes a
/// pull of the log gives them, as iced keeps them.
#[gpui_kit::test]
fn a_stored_pull_s_compare_and_enemies_tab_stand_inert(cx: &mut TestAppContext) {
    let rig = rail_rig(cx, earlier_nights(), 1440., 900.);
    while !matches!(current(cx, &rig), Some(Pull::Stored(_))) {
        key(cx, &rig, Action::OlderSegment);
    }
    cx.update_window(rig.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    rig.gui.read_with(cx, |g, _| {
        let insp = g.insp_frame.as_ref().expect("the inspector");
        let compare = insp
            .acts
            .iter()
            .find(|a| a.glyph == wowdps_gui_logic::glyph::Glyph::Compare)
            .expect("Compare stands on the row");
        assert!(compare.press.is_none(), "inert");
        assert!(compare.tip.contains("no pair"), "{}", compare.tip);
    });
    let before = view(cx, &rig);
    press(cx, &rig, super::super::tabs::tab_id(View::EnemyTaken));
    assert_eq!(view(cx, &rig), before, "the enemies' tab leads nowhere");
}

/// Esc walks up one level at a time, in the iced window's order: the
/// filter's text, the inspector's keys, the comparison, then Home — where
/// the chain ends.
#[gpui_kit::test]
fn esc_walks_up_one_level_at_a_time(cx: &mut TestAppContext) {
    let rig = rig(cx, 1440., 900.);
    let esc = |cx: &mut TestAppContext| key_back(cx, &rig);
    // The filter's text first, wherever the field shows.
    rig.gui.update(cx, |g, cx| g.set_filter("zz".into(), cx));
    esc(cx);
    rig.gui.read_with(cx, |g, _| assert_eq!(g.filter_text, ""));
    // The inspector's keys.
    key(cx, &rig, Action::Open);
    rig.gui
        .read_with(cx, |g, cx| assert!(g.fight(cx).inspecting()));
    esc(cx);
    rig.gui
        .read_with(cx, |g, cx| assert!(!g.fight(cx).inspecting()));
    // The comparison.
    key(cx, &rig, Action::PickCompare);
    key(cx, &rig, Action::Down);
    rig.gui
        .read_with(cx, |g, cx| assert_eq!(g.fight(cx).screen, Screen::Compare));
    esc(cx);
    rig.gui.read_with(cx, |g, cx| {
        assert_eq!(g.fight(cx).screen, Screen::Meter);
        assert!(g.fight(cx).compare_picks().is_empty());
    });
    // Home, where it ends.
    esc(cx);
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Home));
    esc(cx);
    rig.gui
        .read_with(cx, |g, _| assert_eq!(g.place, Place::Home));
}

/// In a tile the rail's drawer goes before the stage's chain.
#[gpui_kit::test]
fn esc_shuts_the_drawer_first(cx: &mut TestAppContext) {
    let rig = rig(cx, 960., 880.);
    rig.gui.update(cx, |g, cx| g.open_drawer(cx));
    key_back(cx, &rig);
    rig.gui.read_with(cx, |g, _| {
        assert!(!g.cards.rail, "the drawer shut");
        assert_eq!(g.place, Place::Fights, "and nothing else");
    });
}

fn key_back(cx: &mut TestAppContext, rig: &Rig) {
    typed(cx, rig, "escape");
}

/// The cards' entrance (the delight): with motion a card fades in from
/// nothing and rests after its 160 ms; under reduced motion it rests from
/// its first frame — the iced window's pixels.
#[gpui_kit::test]
fn a_card_enters_and_rests_and_reduced_motion_skips_it(cx: &mut TestAppContext) {
    let rig = super::super::tests::rig(cx, 1440., 900.);
    let seen = |cx: &mut TestAppContext| {
        cx.update_window(rig.window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("sheet").map(|s| s.visible())
        })
        .unwrap()
    };
    open(cx, &rig, Menu::Sheet);
    assert_eq!(seen(cx), Some(false), "it fades in from nothing");
    cx.executor().advance_clock(super::ENTER);
    cx.run_until_parked();
    assert_eq!(seen(cx), Some(true), "and rests");
    open(cx, &rig, Menu::Sheet);
    cx.update(|cx| cx.set_reduce_motion(true));
    open(cx, &rig, Menu::Sheet);
    assert_eq!(
        seen(cx),
        Some(true),
        "reduced: at rest from the first frame"
    );
}

/// The sheet holds as many 180 px columns as it has room for: three at
/// its widest, two in a narrow window, one in the narrowest.
#[test]
fn the_sheet_s_columns_follow_its_width() {
    assert_eq!(super::sheet::columns(1440., 4), 3);
    assert_eq!(super::sheet::columns(460., 4), 2);
    assert_eq!(super::sheet::columns(320., 4), 1);
    assert_eq!(super::sheet::columns(1440., 2), 2, "never more than groups");
}
