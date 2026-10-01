//! The rail's model over hand-made log segments and cards: nights by local
//! date, the log's visits, a card of the log's own listed once, a stored
//! night's raid, keys and delve, a loose raid pull named for its instance,
//! the steps, and the scroll arithmetic.

use super::samples::*;
use super::*;
use wowdps_model::Encounter;

/// A night is a LOCAL date — the log's clock, which is the reader's —
/// and a raid past midnight is one night: the small hours count with
/// the evening before (the mcp's 06:00 cutover). "Tonight" is the one
/// it is now, in the timezone the log was written in; the rest are
/// named by weekday and date, the year only when it is not this one.
#[test]
fn nights_are_local_dates_with_the_small_hours_counted_with_the_evening() {
    let evening = day("2026-09-26") + 20 * H;
    let small_hours = day("2026-09-27") + 2 * H;
    let morning = day("2026-09-27") + 7 * H;
    assert_eq!(night_of(evening), night_of(small_hours), "one raid night");
    assert_ne!(night_of(small_hours), night_of(morning));
    assert_eq!(
        night_of(day("2026-09-26")),
        night_of(day("2026-09-25") + 20 * H)
    );
    // 04:00 UTC on the 28th is 21:00 on the 27th seven hours west: the
    // 27th's night, where UTC would call it the 28th's small hours
    // and so the 27th's too — and at 14:00 UTC it is the 28th in both.
    let now = day("2026-09-28") + 4 * H;
    assert_eq!(tonight(now, Some(-420)), night("2026-09-27"));
    assert_eq!(tonight(now + 10 * H, Some(-420)), night("2026-09-28"));
    assert_eq!(tonight(now + 10 * H, None), night("2026-09-28"));
    let tonight = night("2026-09-27");
    assert_eq!(night_label(tonight, tonight), "Tonight");
    assert_eq!(
        night_label(night("2026-09-26"), tonight),
        "Saturday, Sep 26"
    );
    assert_eq!(night_label(night("2026-09-25"), tonight), "Friday, Sep 25");
    assert_eq!(
        night_label(night("2025-12-31"), tonight),
        "Wednesday, Dec 31, 2025"
    );
    assert_eq!(civil(night("2026-09-26")), (2026, 9, 26));
    // Pulls on either side of midnight land under one heading.
    let cards = vec![
        card("a", FightKind::Encounter, "Late", small_hours, 60_000),
        card("b", FightKind::Encounter, "Early", evening, 60_000),
        card("c", FightKind::Encounter, "Next day", morning, 60_000),
    ];
    let rail = build(&[], &cards, tonight);
    let labels: Vec<&str> = rail.nights.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(labels, ["Tonight", "Saturday, Sep 26"]);
    let heads: Vec<Vec<&str>> = rail
        .nights
        .iter()
        .map(|n| n.visits.iter().flat_map(names).collect())
        .collect();
    assert_eq!(heads, [vec!["Next day"], vec!["Late", "Early"]]);
}

/// Tonight's log is a visit of its instance and difficulty, its pulls
/// newest first — the live one on top with its red dot — the visit's Σ
/// last, "Whole visit" (a Σ still while the visit goes on), and trash
/// the quiet dash, called trash whatever the engine named it. A pull
/// still going makes its night "Tonight" whatever the clock.
#[test]
fn the_log_lists_its_pulls_newest_first_and_its_sum_last() {
    let start = day("2026-09-27") + 19 * H;
    let mut entries = raid_night(start);
    entries[0].row.live = true;
    entries[4].row.name = "Ula'tek".to_string();
    let rail = build(&entries, &[], night("2020-01-01"));
    assert_eq!(rail.nights.len(), 1);
    let n = &rail.nights[0];
    assert_eq!(n.label, "Tonight", "a live pull's night");
    let v = &n.visits[0];
    assert_eq!(v.title, "The Venomous Abyss, Heroic");
    assert_eq!(
        names(v),
        [
            "Trash",
            "Ula'tek",
            "Trash",
            "The Coiled Altar",
            "Whole visit"
        ]
    );
    let marks: Vec<Mark> = v.lines.iter().map(|l| l.mark).collect();
    assert_eq!(
        marks,
        [Mark::Live, Mark::Bad, Mark::Dash, Mark::Good, Mark::Sum]
    );
    assert!(v.lines[0].trash && !v.lines[1].trash);
    // The log is the owner's: the visit wears their dot, the rows none.
    assert_eq!(
        v.dot,
        Some(Who {
            color: Color::of_class(Class::Warlock),
            name: "Tranqlock-Proudmoore-US".to_string(),
        }),
        "the window's owner: their colour, and their name on its tip"
    );
    assert!(v.lines.iter().all(|l| l.dot.is_none()));
    // Out in the world, a stray pull is a visit of its own kind; one
    // the daemon filed under no visit but inside a visit's span is the
    // visit's.
    let mut world = raid_night(start);
    world.push(entry(
        6,
        SegmentKind::Trash,
        "Trash",
        start + 50 * 60_000,
        30_000,
    ));
    world.insert(
        3,
        entry(7, SegmentKind::Trash, "Trash", start + 12 * 60_000, 60_000),
    );
    // An arena match out in the world is an arena's, not the world's.
    let mut arena = entry(
        8,
        SegmentKind::Encounter,
        "Nagrand Arena",
        start + 55 * 60_000,
        180_000,
    );
    arena.row.arena = true;
    arena.row.success = Some(true);
    world.push(arena);
    let rail = build(&world, &[], night("2026-09-27"));
    let titles: Vec<&str> = rail.nights[0]
        .visits
        .iter()
        .map(|v| v.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Arena", "Open world", "The Venomous Abyss, Heroic"]
    );
    let raid = &rail.nights[0].visits[2];
    assert_eq!(raid.lines.len(), 6, "the stray inside it joined it");
    assert_eq!(raid.lines[2].pull, Pull::Log(SegmentId(7)));
}

/// A stored card that is also a segment of the tailed log — its id is
/// the log's id and the row's start (a Σ's with its mark) — is listed
/// once, as the log's, and lends the row what only the store knows: a
/// wipe's best health, whose pull it was. Another log's cards stand
/// apart.
#[test]
fn a_card_of_the_log_s_own_pull_is_listed_once() {
    let start = day("2026-09-27") + 19 * H;
    let entries = raid_night(start);
    let row = |i: usize| &entries[i].row;
    let mut wipe = card(
        &fight_id(LOG, row(3).start_ms, false),
        FightKind::Encounter,
        "Ula'tek",
        row(3).start_ms,
        row(3).duration_ms,
    );
    wipe.success = Some(false);
    wipe.best_pct = Some(56);
    let wipe = owned(wipe, "Player-1-9", Class::Warlock);
    let sum = card(
        &fight_id(LOG, row(0).start_ms, true),
        FightKind::Overall,
        "The Venomous Abyss",
        row(0).start_ms,
        row(0).duration_ms,
    );
    let elsewhere = card(
        "other-1",
        FightKind::Encounter,
        "Ula'tek",
        start - 24 * H,
        60_000,
    );
    let wipe_id = wipe.id.clone();
    let cards = vec![wipe, sum, elsewhere];
    let rail = build(&entries, &cards, night("2026-09-27"));
    let log: Vec<&Line> = rail.nights[0].visits[0].lines.iter().collect();
    assert_eq!(log.len(), 5, "the log's own, each once");
    assert!(
        rail.nights[0].visits.len() == 1,
        "no stored visit beside it"
    );
    let ula = log.iter().find(|l| l.name == "Ula'tek").unwrap();
    assert_eq!(ula.pull, Pull::Log(SegmentId(4)));
    assert_eq!(ula.best_pct, Some(56), "the card's best health");
    assert_eq!(rail.nights.len(), 2, "another log's night");
    assert_eq!(
        rail.nights[1].visits[0].lines[0].pull,
        Pull::Stored("other-1".to_string())
    );
    // With no log id nothing can be matched: the same cards stand apart,
    // the wipe's card a stored pull beside the log's own row for it.
    let rail = Rail::build(&Sources {
        entries: &entries,
        log_id: None,
        watched: None,
        cards: &cards,
        owner: None,
        tonight: night("2026-09-27"),
    });
    assert_eq!(rail.lines().count(), 8, "five of the log's, three cards");
    assert!(rail.line(&Pull::Log(SegmentId(4))).is_some());
    assert!(
        rail.line(&Pull::Stored(wipe_id.clone()))
            .is_some_and(|l| l.best_pct == Some(56)),
        "the wipe's card, on its own"
    );
    assert_eq!(
        rail.line(&Pull::Log(SegmentId(4))).and_then(|l| l.best_pct),
        None,
        "lending nothing to a row it is not paired with"
    );
}

/// The store's night: a raid visit (its pulls on its map, newest first,
/// its Σ last), the night's keys and dungeon runs as one visit — a
/// key's zone-in left out, the key being the run — with "+N" on a
/// timed key and "over" on a depleted one, and a delve's boss under
/// "Delve". Several characters' pulls wear a dot each; one character's
/// visit wears it once.
#[test]
fn a_stored_night_is_its_raid_its_keys_and_its_delve() {
    let d = day("2026-09-26");
    let raid = in_map(
        card(
            "r-s",
            FightKind::Overall,
            "The Venomous Abyss",
            d + 19 * H,
            30 * 60_000,
        ),
        3004,
        15,
    );
    let boss = |id: &str, at: i64, ok: bool, pct: Option<u16>| {
        let mut c = card(
            id,
            FightKind::Encounter,
            "The Lost Explorers",
            d + at,
            400_000,
        );
        c.success = Some(ok);
        c.best_pct = pct;
        c.encounter = Some(Encounter {
            id: 3497,
            difficulty: 15,
            group_size: 20,
        });
        owned(c, "Player-1-2", Class::DeathKnight)
    };
    let key = |id: &str, name: &str, at: i64, ok: bool, who: &str, class| {
        let mut c = card(id, FightKind::Key, name, d + at, 1_600_000);
        c.success = Some(ok);
        c.official_ms = Some(1_600_000);
        c.pars_ms = Some((1_800_000, 1_440_000, 1_080_000));
        owned(in_map(c, 2825, 23), who, class)
    };
    let stub = in_map(
        card(
            "z-s",
            FightKind::Overall,
            "Den of Nalorakk",
            d + 13 * H,
            60_000,
        ),
        2825,
        23,
    );
    let pools = owned(
        in_map(
            card(
                "p-s",
                FightKind::Overall,
                "Ruby Life Pools",
                d + 16 * H,
                1_338_000,
            ),
            2521,
            8,
        ),
        "Player-1-3",
        Class::DemonHunter,
    );
    let mut delve = card("dv", FightKind::Encounter, "Drakta", d + 11 * H, 69_000);
    delve.success = Some(true);
    delve.encounter = Some(Encounter {
        id: 3535,
        difficulty: 208,
        group_size: 2,
    });
    // An arena match in the same log after the raid: it was in no
    // visit (the store keeps none for it), so it joins none.
    let mut arena = card(
        "ar",
        FightKind::Arena,
        "Nagrand Arena",
        d + 19 * H + 40 * 60_000,
        180_000,
    );
    arena.success = Some(true);
    let cards = vec![
        owned(raid, "Player-1-2", Class::DeathKnight),
        boss("b1", 19 * H + 60_000, false, Some(97)),
        in_map(boss("b2", 19 * H + 20 * 60_000, false, Some(2)), 3004, 15),
        key(
            "k1",
            "Den of Nalorakk +14",
            13 * H + 90_000,
            true,
            "Player-1-3",
            Class::DemonHunter,
        ),
        key(
            "k2",
            "Kings' Rest +14",
            14 * H,
            false,
            "Player-1-3",
            Class::DemonHunter,
        ),
        key(
            "k3",
            "Altar of Fangs +14",
            17 * H,
            true,
            "Player-1-4",
            Class::Warlock,
        ),
        stub,
        pools,
        delve,
        arena,
    ];
    let rail = build(&[], &cards, night("2026-09-27"));
    assert_eq!(rail.nights.len(), 1);
    let n = &rail.nights[0];
    assert_eq!(n.label, "Saturday, Sep 26");
    let titles: Vec<&str> = n.visits.iter().map(|v| v.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Arena",
            "The Venomous Abyss, Heroic",
            "Mythic+ keys",
            "Delve"
        ]
    );
    let raid = &n.visits[1];
    assert_eq!(
        names(raid),
        ["The Lost Explorers", "The Lost Explorers", "Whole visit"]
    );
    assert_eq!(raid.lines[0].best_pct, Some(2), "newest first");
    assert_eq!(raid.lines[2].mark, Mark::Sum);
    assert_eq!(
        raid.dot.as_ref().map(|w| w.color),
        Some(Color::of_class(Class::DeathKnight))
    );
    let keys = &n.visits[2];
    assert_eq!(
        names(keys),
        [
            "Altar of Fangs +14",
            "Ruby Life Pools",
            "Kings' Rest +14",
            "Den of Nalorakk +14"
        ],
        "the zone-in is the key's, not a run"
    );
    let words: Vec<Option<KeyWord>> = keys.lines.iter().map(|l| l.key).collect();
    assert_eq!(
        words,
        [
            Some(KeyWord::Plus(1)),
            None,
            Some(KeyWord::Over),
            Some(KeyWord::Plus(1))
        ]
    );
    assert_eq!(keys.lines[1].mark, Mark::Dash, "a run with no verdict");
    assert_eq!(keys.dot, None, "two characters played the keys");
    assert_eq!(
        keys.lines[0].dot,
        Some(Who {
            color: Color::of_class(Class::Warlock),
            name: "Player-1-4".to_string(),
        }),
        "a card's owner by the name the card gives them"
    );
    assert_eq!(
        keys.lines[2].dot.as_ref().map(|w| w.color),
        Some(Color::of_class(Class::DemonHunter))
    );
    assert_eq!(n.visits[3].lines[0].mark, Mark::Good);
    assert_eq!(n.visits[3].dot, None, "nobody said whose");
}

/// A stored pull of the tailed log that its list does not hold — a copy
/// of the log cut short — is listed in the log's visit it followed, not
/// as a visit of its own. A raid pull no visit claims is named for its
/// instance: after the log's visit that night at its difficulty, else
/// after any Σ card of its map in hand, and "Raid, Heroic" only when
/// nothing names it.
#[test]
fn a_raid_pull_no_visit_claims_is_named_for_its_instance() {
    let start = day("2026-09-27") + 19 * H;
    let entries = raid_night(start);
    let boss = |id: &str, log: u64, at: i64, map: Option<u32>| {
        let mut c = card(id, FightKind::Encounter, "Ula'tek", at, 400_000);
        c.log = log;
        c.success = Some(false);
        c.encounter = Some(Encounter {
            id: 3429,
            difficulty: 15,
            group_size: 25,
        });
        match map {
            Some(m) => in_map(c, m, 15),
            None => c,
        }
    };
    let late = boss("late", LOG, start + 45 * 60_000, Some(3004));
    let other = boss("other", 9, start + 50 * 60_000, None);
    let rail = build(&entries, &[late, other], night("2026-09-27"));
    assert_eq!(rail.nights.len(), 1);
    let titles: Vec<&str> = rail.nights[0]
        .visits
        .iter()
        .map(|v| v.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["The Venomous Abyss, Heroic", "The Venomous Abyss, Heroic"],
        "no \"Raid, Heroic\" where the night names the instance"
    );
    let log = rail.nights[0]
        .visits
        .iter()
        .find(|v| v.lines.iter().any(|l| l.pull == Pull::Log(SegmentId(1))))
        .expect("the log's visit");
    assert_eq!(
        log.lines.first().map(|l| &l.pull),
        Some(&Pull::Stored("late".to_string())),
        "the log's own card, newest, in the log's visit"
    );
    assert_eq!(log.lines.last().map(|l| l.mark), Some(Mark::Sum));
    // Another night: named by a Σ card of its map from any night…
    let earlier = day("2026-09-20") + 20 * H;
    let sigma = in_map(
        card(
            "sigma",
            FightKind::Overall,
            "The Venomous Abyss",
            day("2026-09-26") + 19 * H,
            60_000,
        ),
        3004,
        15,
    );
    let mapped = boss("mapped", 5, earlier, Some(3004));
    let bare = boss("bare", 6, earlier - 3 * 24 * H, None);
    let rail = build(&[], &[sigma, mapped, bare], night("2026-09-27"));
    let title_of = |id: &str| {
        rail.nights
            .iter()
            .flat_map(|n| n.visits.iter())
            .find(|v| {
                v.lines
                    .iter()
                    .any(|l| l.pull == Pull::Stored(id.to_string()))
            })
            .map(|v| v.title.clone())
    };
    assert_eq!(
        title_of("mapped").as_deref(),
        Some("The Venomous Abyss, Heroic")
    );
    // …and only with nothing to name it, by the kind of content.
    assert_eq!(title_of("bare").as_deref(), Some("Raid, Heroic"));
}

/// `[` walks down the rail, `]` up it, across nights and visits, over
/// the rows the trash toggle leaves; from a row it hides, the nearest
/// shown one; from nowhere, the top; past either end, nothing.
#[test]
fn the_steps_walk_the_rail_over_what_it_shows() {
    let start = day("2026-09-27") + 19 * H;
    let cards = vec![card(
        "old",
        FightKind::Encounter,
        "Ula'tek",
        start - 24 * H,
        60_000,
    )];
    let rail = build(&raid_night(start), &cards, night("2026-09-27"));
    let order: Vec<Pull> = rail.lines().map(|l| l.pull.clone()).collect();
    let log = |i| Pull::Log(SegmentId(i));
    assert_eq!(
        order,
        [
            log(5),
            log(4),
            log(3),
            log(2),
            log(1),
            Pull::Stored("old".to_string())
        ]
    );
    assert_eq!(rail.step(None, true, false), Some(log(5)), "the top");
    assert_eq!(rail.step(Some(&log(4)), true, false), Some(log(3)));
    assert_eq!(rail.step(Some(&log(4)), false, false), Some(log(5)));
    assert_eq!(rail.step(Some(&log(5)), false, false), None, "the top");
    assert_eq!(
        rail.step(Some(&log(1)), true, false),
        Some(Pull::Stored("old".to_string())),
        "into the store's nights"
    );
    assert_eq!(
        rail.step(Some(&Pull::Stored("old".to_string())), true, false),
        None
    );
    // Trash hidden: over it, and from it.
    assert_eq!(rail.step(Some(&log(4)), true, true), Some(log(2)));
    assert_eq!(rail.step(None, true, true), Some(log(4)), "the top shown");
    assert_eq!(rail.step(Some(&log(3)), true, true), Some(log(2)));
    assert_eq!(rail.step(Some(&log(3)), false, true), Some(log(4)));
    // Where H opens it: the first night that is not tonight.
    assert_eq!(rail.earlier(), Some(1));
    assert!(rail.line(&log(2)).is_some_and(|l| l.mark == Mark::Good));
}

/// A step shows its row with the prototype's 40 px under it, the least
/// scroll that does; the drawer opening on a row it must scroll to
/// stands its night's heading at the top when both fit, else centres
/// it; a row in sight stays where it is either way; and neither
/// scrolls past the list's ends.
#[test]
fn the_rail_scrolls_to_a_row_with_room_under_it() {
    let (h, content) = (500.0, 2_000.0);
    // In sight with its room: no move.
    assert_eq!(near_offset(0.0, h, content, 100.0, 128.0), 0.0);
    // Past the bottom: 40 px under it.
    assert_eq!(near_offset(0.0, h, content, 480.0, 508.0), 48.0);
    // Above the top: at the top.
    assert_eq!(near_offset(300.0, h, content, 200.0, 228.0), 200.0);
    // Near the end: no further than the list goes.
    assert_eq!(near_offset(0.0, h, content, 1_972.0, 2_000.0), 1_500.0);
    // Opening: its heading at the top when both fit…
    assert_eq!(
        open_offset(0.0, h, content, (900.0, 928.0), Some(700.0)),
        700.0
    );
    // …else the row in the middle…
    assert_eq!(
        open_offset(0.0, h, content, (900.0, 928.0), Some(100.0)),
        664.0
    );
    assert_eq!(open_offset(0.0, h, content, (900.0, 928.0), None), 664.0);
    // …and a row already in sight stays put.
    assert_eq!(
        open_offset(0.0, h, content, (100.0, 128.0), Some(12.0)),
        0.0
    );
    assert_eq!(
        open_offset(0.0, h, 300.0, (200.0, 228.0), Some(0.0)),
        0.0,
        "a short list never scrolls"
    );
}
