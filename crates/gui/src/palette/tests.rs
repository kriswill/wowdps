//! The command palette's gestures: its keys move and run the selection,
//! a press runs or closes it (what it lists is gui-logic's, tested there).

use super::*;
use crate::window::testkit::{Bridge, chr, key, named, simulator};
use iced::Point;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use wowdps_daemon::mock::MockDaemon;
use wowdps_gui_logic::palette::fixture::all;
use wowdps_model::View;

/// The card: the groups' headings, the selection raised, a press on an
/// item runs it, a press off the card closes it and one on the card's own
/// heading does not; the field types into the query and gives its focus
/// back on release.
#[test]
fn the_card_runs_what_is_pressed_and_closes_off_it() {
    let shown = listed(all(), "");
    let p = Palette {
        sel: 1,
        ..Palette::default()
    };
    let mut ui = simulator(overlay(&p, &shown, theme::GOLD_ACCENT));
    for heading in ["Pulls", "Players in this pull", "Views", "Screens"] {
        assert!(ui.find(heading).is_ok(), "{heading}");
    }
    assert!(ui.find(selected_id()).is_ok(), "the selection is marked");
    assert!(ui.find(input_id()).is_ok(), "the field is on the card");
    assert_eq!(
        PLACEHOLDER, "Jump to a pull, player or view",
        "the prototype's own words in the empty field"
    );
    ui.click("Pulls").unwrap();
    assert!(ui.into_messages().next().is_none(), "the card's own press");

    let mut ui = simulator(overlay(&p, &shown, theme::GOLD_ACCENT));
    ui.click("Swampert").unwrap();
    let sent: Vec<Message> = ui.into_messages().collect();
    assert!(
        // The pointer lights nothing on its way (no hover wash): the press
        // is the one message.
        matches!(
            sent.as_slice(),
            [Message::PaletteRun(Run::Player { key, .. })] if key == "Player-1"
        ),
        "{sent:?}"
    );

    let mut ui = simulator(overlay(&p, &shown, theme::GOLD_ACCENT));
    ui.point_at(Point::new(3.0, 3.0));
    let _ = ui.simulate(iced_test::simulator::click());
    let sent: Vec<Message> = ui.into_messages().collect();
    assert!(
        matches!(sent.as_slice(), [Message::PaletteClose]),
        "{sent:?}"
    );

    let mut ui = simulator(overlay(&p, &shown, theme::GOLD_ACCENT));
    ui.click(input_id()).unwrap();
    let _ = ui.typewrite("u");
    let sent: Vec<Message> = ui.into_messages().collect();
    assert!(
        sent.iter().any(|m| matches!(m, Message::PaletteFocus)),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::PaletteQuery(q) if q == "u")),
        "{sent:?}"
    );

    let mut ui = simulator(overlay(&Palette::default(), &[], theme::GOLD_ACCENT));
    assert!(ui.find(NOTHING).is_ok(), "nothing matches, and it says so");
    let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
}

/// What the palette covers hears nothing of the pointer: no hover, no
/// wheel — the scrim is opaque to it, as the card is — and a press on the
/// scrim closes the palette all the same.
#[test]
fn nothing_under_the_card_answers_the_pointer() {
    use iced::widget::{Space, container, mouse_area, stack};
    let under = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_enter(Message::Tick)
    .on_scroll(|_| Message::Tick)
    .interaction(mouse::Interaction::Pointer);
    let shown = listed(all(), "");
    let mut ui = simulator(
        stack![
            under,
            overlay(&Palette::default(), &shown, theme::GOLD_ACCENT)
        ]
        .into(),
    );
    ui.point_at(Point::new(3.0, 3.0));
    let _ = ui.simulate([iced::Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    })]);
    let _ = ui.simulate(iced_test::simulator::click());
    let sent: Vec<Message> = ui.into_messages().collect();
    assert!(
        matches!(sent.as_slice(), [Message::PaletteClose]),
        "{sent:?}"
    );
}

// ---- in the window ------------------------------------------------------------

fn on_a_pull() -> Bridge {
    let mut b = Bridge::new(MockDaemon::fixture().with_history());
    b.open(b.gui.state.entries().len() - 2);
    b
}

fn ctrl(c: &str) -> Message {
    key(Key::Character(c.into()), Modifiers::CTRL)
}

fn sel(b: &Bridge) -> usize {
    b.gui.palette.as_ref().map_or(usize::MAX, |p| p.sel)
}

/// The arrows and Ctrl N / Ctrl P move the selection, stopping at the
/// ends; what is typed narrows the list; Enter runs the selection and the
/// palette closes.
#[test]
fn the_keys_move_the_selection_and_enter_runs_it() {
    let mut b = on_a_pull();
    b.send(ctrl("k"));
    let n = b.gui.palette_items().len();
    assert!(n > 12, "pulls, players, views and screens: {n}");
    b.send(named(Named::ArrowDown));
    assert_eq!(sel(&b), 1);
    b.send(ctrl("n"));
    assert_eq!(sel(&b), 2);
    b.send(ctrl("p"));
    assert_eq!(sel(&b), 1);
    b.send(named(Named::ArrowUp));
    b.send(named(Named::ArrowUp));
    assert_eq!(sel(&b), 0);
    for _ in 0..n + 3 {
        b.send(named(Named::ArrowDown));
    }
    assert_eq!(sel(&b), n - 1);
    // The list shrinking under the selection (a pull, a page, the players
    // refilled): the next message puts it back on a line the card lists.
    if let Some(p) = b.gui.palette.as_mut() {
        p.sel = n + 40;
    }
    b.send(Message::Tick);
    assert_eq!(sel(&b), n - 1, "clamped to the list");
    for c in "heal".chars() {
        b.send(chr(&c.to_string()));
    }
    assert_eq!(sel(&b), 0, "a query starts at the top");
    let items = b.gui.palette_items();
    assert_eq!(items.first().map(|i| i.title.as_str()), Some("Healing"));
    b.send(named(Named::Enter));
    assert!(b.gui.palette.is_none(), "run, and closed");
    assert_eq!(b.gui.fight().view, View::Healing);
    // Enter in the focused field, which captured it, is the same run.
    b.send(ctrl("k"));
    for c in "damage".chars() {
        b.send(chr(&c.to_string()));
    }
    b.send(Message::PaletteSubmit);
    assert!(b.gui.palette.is_none());
    assert_eq!(b.gui.fight().view, View::Damage);
}

/// A pull runs onto the stage; a player is selected on a chart they are
/// on — a count view gives way to Damage — the inspector following them.
#[test]
fn pulls_and_players_run_to_the_stage() {
    let mut b = on_a_pull();
    let at = b.gui.current_pull();
    let other = b
        .gui
        .rail()
        .lines()
        .map(|l| l.pull.clone())
        .find(|p| Some(p) != at.as_ref())
        .expect("a second pull");
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::Pull(other.clone())));
    assert!(b.gui.palette.is_none());
    assert_eq!(b.gui.current_pull(), Some(other));
    b.send(Message::Pull(at.clone().unwrap()));

    let rows = b.gui.fight().rows();
    let target = rows.last().cloned().expect("players on the meter");
    assert_ne!(b.gui.fight().row_sel, rows.len() - 1);
    // On a count view the players are still the pull's, as Damage listed
    // them.
    b.send(chr("i"));
    assert_eq!(b.gui.fight().view, View::Interrupts);
    b.send(Message::Jump);
    let run = b
        .gui
        .palette_items()
        .into_iter()
        .find(|i| matches!(&i.run, Run::Player { key, .. } if *key == target.key))
        .map(|i| i.run)
        .expect("the player is offered");
    b.send(Message::PaletteRun(run));
    let app = b.gui.fight();
    assert_eq!(app.view, View::Damage, "to a chart they are on");
    assert_eq!(
        app.rows().get(app.row_sel).map(|r| r.key.as_str()),
        Some(target.key.as_str())
    );
    assert_eq!(
        app.drill.as_ref().map(|d| d.key.as_str()),
        Some(target.key.as_str()),
        "the inspector follows them"
    );
    assert_eq!(
        b.gui.reveal_player, None,
        "their row landed, and was brought into sight"
    );
}

/// The screens: Home, the live pull, the earlier nights, Home's scopes,
/// the sheet, the talent viewer — and Esc closes the palette with nothing
/// run.
#[test]
fn the_screens_run_and_esc_runs_nothing() {
    let mut b = on_a_pull();
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::Home));
    assert!(b.gui.home.is_some() && b.gui.palette.is_none());
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::Live));
    assert!(b.gui.home.is_none(), "the live pull, Home aside");
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::Earlier));
    assert!(b.gui.palette.is_none());
    b.gui.rail_open = false;
    // Home's scopes, for the keys: the chips' own message, Home opened on
    // the scope and the scope remembered.
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::HomeScope(Some(
        "Player-X".to_string(),
    ))));
    assert_eq!(
        b.gui.home.as_ref().and_then(|h| h.scope.as_deref()),
        Some("Player-X")
    );
    assert_eq!(b.gui.cfg.character.as_deref(), Some("Player-X"));
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::HomeScope(None)));
    assert!(b.gui.home.as_ref().is_some_and(|h| h.scope.is_none()));
    assert_eq!(b.gui.cfg.character, None);
    b.send(Message::PaletteRun(Run::Live));
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::Sheet));
    assert!(b.gui.shortcuts_open);
    b.send(chr("x"));
    b.send(Message::Jump);
    b.send(Message::PaletteRun(Run::Talents));
    assert!(b.gui.talents.is_some());
    b.gui.talents = None;
    let view = b.gui.fight().view;
    b.send(Message::Jump);
    b.send(chr("h"));
    b.send(named(Named::Escape));
    assert!(b.gui.palette.is_none());
    assert_eq!(b.gui.fight().view, view, "Esc ran nothing");
    // Drawn over the window, with its field: the palette is up.
    b.send(Message::Jump);
    let mut ui = simulator(crate::view::view(&b.gui));
    assert!(ui.find(input_id()).is_ok());
    assert!(ui.find("Players in this pull").is_ok());
}
