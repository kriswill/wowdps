//! The palette's model: what it lists for a query and how its selection
//! steps.

use wowdps_proto::history::{FightCard, FightKind};

use super::fixture::*;
use super::*;
use crate::rail;

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
