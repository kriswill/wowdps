//! The command palette: what it lists for a query, how its keys move and
//! run the selection, and how it closes.

use super::*;
use crate::rail;
use crate::window::testkit::{Bridge, chr, key, named, simulator};
use iced::Point;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::Encounter;
use wowdps_proto::history::{FightCard, FightKind, KeyInfo};

const H: i64 = 3_600_000;

/// 20:00 on a date, on the log's clock.
fn evening(ymd: &str) -> i64 {
    crate::home::parse_ymd(ymd).unwrap() + 20 * H
}

fn card(id: &str, kind: FightKind, name: &str, at: i64) -> FightCard {
    FightCard {
        id: id.to_string(),
        log: 7,
        kind,
        name: name.to_string(),
        start_local_ms: at,
        start_utc_ms: at + 7 * H,
        duration_ms: 300_000,
        ..FightCard::default()
    }
}

fn boss(id: &str, name: &str, at: i64, kill: bool, pct: Option<u16>) -> FightCard {
    FightCard {
        encounter: Some(Encounter {
            id: 3492,
            difficulty: 15,
            group_size: 25,
        }),
        success: Some(kill),
        best_pct: pct,
        ..card(id, FightKind::Encounter, name, at)
    }
}

fn keyed(id: &str, name: &str, at: i64, clock: i64, timed: bool) -> FightCard {
    FightCard {
        key: Some(KeyInfo {
            map_id: 2521,
            difficulty: 8,
            level: Some(14),
            completed: Some(timed),
        }),
        pars_ms: Some((1_800_000, 1_440_000, 1_080_000)),
        official_ms: Some(clock),
        duration_ms: clock,
        success: Some(timed),
        ..card(id, FightKind::Key, name, at)
    }
}

/// Tonight's raid — a kill, a wipe at 56 %, another kill, trash between —
/// and Saturday's two keys, one over; `extra` more cards on top.
fn rail_of(extra: Vec<FightCard>) -> Rail {
    let sun = evening("2026-09-27");
    let sat = evening("2026-09-26");
    let mut cards = vec![
        boss("u2", "Ula'tek", sun + 90 * 60_000, true, None),
        boss("u1", "Ula'tek", sun + 60 * 60_000, false, Some(56)),
        card("t1", FightKind::Trash, "Trash", sun + 30 * 60_000),
        boss("c", "The Coiled Altar", sun + 10 * 60_000, true, Some(0)),
        keyed("k2", "Kings' Rest +14", sat + H, 1_900_000, false),
        keyed("k1", "Kings' Rest +14", sat, 1_700_000, true),
    ];
    cards.extend(extra);
    cards.sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
    rail::Rail::build(&rail::Sources {
        entries: &[],
        log_id: None,
        watched: None,
        cards: &cards,
        owner: None,
        tonight: rail::night_of(sun),
    })
}

fn players() -> Vec<Row> {
    let row = |key: &str, label: &str, spec: Spec, enemy: bool| Row {
        key: key.to_string(),
        label: label.to_string(),
        class: Some(spec.class()),
        spec: Some(spec),
        enemy,
        ..Row::default()
    };
    vec![
        row("Player-1", "Swampert-Proudmoore-US", Spec::Arms, false),
        row("Player-2", "Akanôs-Proudmoore-US", Spec::Devourer, false),
        row("Zul'jan", "Zul'jan", Spec::Fire, true),
    ]
}

/// The characters the window knows you play: one the store resolved, and
/// one only the config names (no guid yet), which Home cannot be scoped to.
fn chars() -> Vec<crate::home::CharLine> {
    vec![
        crate::home::CharLine {
            guid: "Player-ME".to_string(),
            name: "Tranqlock-Proudmoore-US".to_string(),
            class: Some(Class::Warlock),
            spec: Some(Spec::Destruction),
            ..crate::home::CharLine::default()
        },
        crate::home::CharLine {
            name: "Unresolved-Realm-US".to_string(),
            ..crate::home::CharLine::default()
        },
    ]
}

fn all() -> Vec<Item> {
    items(&rail_of(Vec::new()), &players(), &chars(), true)
}

fn titles(items: &[Item], group: Group) -> Vec<String> {
    items
        .iter()
        .filter(|i| i.group == group)
        .map(|i| i.title.clone())
        .collect()
}

/// Everything the window can go to, in the card's groups: the rail's
/// pulls with their night and verdict, our side's players with their spec,
/// the views with their key, and the screens.
#[test]
fn the_items_are_the_rail_the_players_the_views_and_the_screens() {
    let items = all();
    let groups: Vec<Group> = items.iter().map(|i| i.group).collect();
    let mut order = groups.clone();
    order.dedup();
    assert_eq!(
        order,
        [Group::Pulls, Group::Players, Group::Views, Group::Screens],
        "grouped, in the card's order"
    );
    let sub = |id: &str| {
        items
            .iter()
            .find(|i| i.run == Run::Pull(rail::Pull::Stored(id.to_string())))
            .map(|i| i.sub.clone())
            .unwrap_or_else(|| panic!("{id} is listed"))
    };
    assert_eq!(sub("u2"), "Tonight, kill");
    assert_eq!(sub("u1"), "Tonight, wipe at 56%");
    assert_eq!(sub("t1"), "Tonight", "trash has no verdict");
    assert_eq!(sub("k1"), "Saturday, Sep 26, +1");
    assert_eq!(sub("k2"), "Saturday, Sep 26, over");
    // The enemy is no player; realms are hidden as the option says.
    assert_eq!(titles(&items, Group::Players), ["Swampert", "Akanôs"]);
    let akanos = items.iter().find(|i| i.title == "Akanôs").unwrap();
    assert_eq!(akanos.sub, "Devourer Demon Hunter");
    assert_eq!(
        akanos.disc,
        Some((Some(Class::DemonHunter), Some(Spec::Devourer)))
    );
    // Every view, in the tab strip's order, on the key that switches to it.
    let views: Vec<(String, Option<&str>)> = items
        .iter()
        .filter(|i| i.group == Group::Views)
        .map(|i| (i.title.clone(), i.key))
        .collect();
    assert_eq!(views.len(), 8);
    assert_eq!(views[0], ("Damage".to_string(), Some("d")));
    assert_eq!(views[2], ("Taken".to_string(), Some("T")));
    assert_eq!(views[7], ("Enemies".to_string(), Some("E")));
    let screens: Vec<(String, Option<&str>)> = items
        .iter()
        .filter(|i| i.group == Group::Screens)
        .map(|i| (i.title.clone(), i.key))
        .collect();
    assert_eq!(
        screens,
        [
            ("Home".to_string(), Some("~")),
            ("Live pull".to_string(), Some("m")),
            ("Earlier nights".to_string(), Some("H")),
            ("Keyboard shortcuts".to_string(), Some("?")),
            ("Talents".to_string(), Some("t")),
            // Home's scope chips, for the keys: every character with a
            // guid, realms hidden as the option says.
            ("Home: All characters".to_string(), None),
            ("Home: Tranqlock".to_string(), None),
        ]
    );
    let scoped = items.iter().find(|i| i.title == "Home: Tranqlock").unwrap();
    assert_eq!(scoped.run, Run::HomeScope(Some("Player-ME".to_string())));
    assert_eq!(
        scoped.disc,
        Some((Some(Class::Warlock), Some(Spec::Destruction)))
    );
    // Realms shown when the option says so.
    let shown = items_with_realms();
    assert!(titles(&shown, Group::Players).contains(&"Swampert-Proudmoore-US".to_string()));
}

fn items_with_realms() -> Vec<Item> {
    items(&rail_of(Vec::new()), &players(), &chars(), false)
}

/// Before anything is typed: the newest pulls worth jumping to — a boss or
/// a key, not the trash between them — and every other item.
#[test]
fn an_empty_query_offers_the_recent_pulls() {
    let shown = listed(all(), "");
    let pulls: Vec<Run> = shown
        .iter()
        .filter(|i| i.group == Group::Pulls)
        .map(|i| i.run.clone())
        .collect();
    let stored = |id: &str| Run::Pull(rail::Pull::Stored(id.to_string()));
    assert_eq!(
        pulls,
        [stored("u2"), stored("u1"), stored("c"), stored("k2")],
        "the newest four, trash passed over"
    );
    // The screens, but not Home's scopes: those wait for a word.
    assert_eq!(shown.len(), RECENT + 2 + 8 + 5);
    assert!(shown.iter().all(|i| !matches!(i.run, Run::HomeScope(_))));
    assert_eq!(listed(all(), "   "), shown, "blank is empty");
}

/// A query is the row filter's: accent-folded, case aside, a substring of
/// the title or of the words beside it — and every pull the rail holds is
/// searched, not the recent few.
#[test]
fn a_query_folds_accents_and_searches_the_words() {
    let find = |q: &str| -> Vec<String> { listed(all(), q).into_iter().map(|i| i.title).collect() };
    assert_eq!(find("akanos"), ["Akanôs"], "the accent folds");
    assert_eq!(find("AKANÔS"), ["Akanôs"], "and the case");
    assert_eq!(find("devourer"), ["Akanôs"], "the spec beside the name");
    assert_eq!(find("ula"), ["Ula'tek", "Ula'tek"]);
    assert_eq!(find("wipe"), ["Ula'tek"], "the verdict beside it");
    assert_eq!(find("kings"), ["Kings' Rest +14", "Kings' Rest +14"]);
    assert_eq!(find("heal"), ["Healing"]);
    assert_eq!(find("keyboard"), ["Keyboard shortcuts"]);
    // The title and its words are one text: a query may run across them.
    assert_eq!(find("akanos devourer"), ["Akanôs"]);
    assert_eq!(find("ula'tek tonight"), ["Ula'tek", "Ula'tek"]);
    assert_eq!(find("coiled altar tonight, kill"), ["The Coiled Altar"]);
    // Home's scopes answer a name, or "home".
    assert_eq!(find("tranq"), ["Home: Tranqlock"]);
    assert_eq!(find("home: all"), ["Home: All characters"]);
    assert!(find("zzz").is_empty());
    // A query lists at most MAX_PULLS pulls.
    let sun = evening("2026-09-27");
    let trash: Vec<FightCard> = (0..30)
        .map(|i| {
            card(
                &format!("x{i}"),
                FightKind::Trash,
                "Trash",
                sun + 3 * H + i * 60_000,
            )
        })
        .collect();
    let many = listed(items(&rail_of(trash), &[], &[], true), "trash");
    assert_eq!(many.len(), MAX_PULLS);
}

#[test]
fn the_selection_steps_and_stops_at_the_ends() {
    let mut p = Palette::default();
    p.step(false, 5);
    assert_eq!(p.sel, 0, "the top is the top");
    for _ in 0..9 {
        p.step(true, 5);
    }
    assert_eq!(p.sel, 4, "the bottom is the bottom");
    p.step(false, 5);
    assert_eq!(p.sel, 3);
    p.typed("ula".to_string());
    assert_eq!(
        (p.sel, p.query.as_str()),
        (0, "ula"),
        "a new query starts at the top"
    );
    p.step(true, 0);
    assert_eq!(p.sel, 0, "nothing listed, nowhere to go");
    // The list shrank under the selection: it stays on a line it lists.
    p.sel = 9;
    p.clamp(4);
    assert_eq!(p.sel, 3);
    p.sel = 9;
    p.step(false, 4);
    assert_eq!(p.sel, 2, "a step from past the end is a step from the end");
}

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
