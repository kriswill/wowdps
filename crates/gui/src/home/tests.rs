//! Home's derivation over hand-made cards — the rank in the role, keys
//! against their timers, the raid's dots, each character's best — its
//! paging, and the screen those panels draw.

use super::charts::{ParBar, axis, axis_label};
use super::panels::{
    MAX_TILES, boss_outcome, columns_for, grid_columns, key_widths, long_date, night_cap,
    panel_count,
};
use super::*;
use crate::window::testkit::simulator;
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{Encounter, Spec};
use wowdps_proto::history::KeyInfo;

const H: i64 = 3_600_000;
const DAY: i64 = 86_400_000;
/// Every card here is 100 s long, so a player's damage is 100× their dps.
const SECS: i64 = 100;

/// 20:00 on a date, on the log's clock.
fn evening(ymd: &str) -> i64 {
    parse_ymd(ymd).unwrap() + 20 * H
}

/// A player of `spec` at `rate`: effective dps for a damage dealer or a
/// tank, hps for a healer.
fn player(guid: &str, spec: Spec, rate: f64) -> CardPlayer {
    let healer = spec.role() == Role::Healer;
    CardPlayer {
        guid: guid.to_string(),
        name: format!("{guid}-Realm-US"),
        class: Some(spec.class()),
        spec: Some(spec),
        damage: if healer {
            0
        } else {
            (rate * SECS as f64) as u64
        },
        dps: if healer { 0.0 } else { rate },
        hps: if healer { rate } else { 0.0 },
        ..CardPlayer::default()
    }
}

/// A card of `kind` owned by `owner`, starting at `at` on the log's clock.
fn card(id: &str, kind: FightKind, name: &str, at: i64, owner: &str) -> FightCard {
    FightCard {
        id: id.to_string(),
        log: 7,
        kind,
        name: name.to_string(),
        start_local_ms: at,
        start_utc_ms: at + 7 * H,
        duration_ms: SECS * 1000,
        owner: Some(owner.to_string()),
        ..FightCard::default()
    }
}

/// A raid boss pull: `enc` at `difficulty`, a kill or a wipe.
fn boss(
    id: &str,
    name: &str,
    enc: u32,
    at: i64,
    kill: bool,
    players: Vec<CardPlayer>,
) -> FightCard {
    FightCard {
        encounter: Some(Encounter {
            id: enc,
            difficulty: 15,
            group_size: 20,
        }),
        success: Some(kill),
        players,
        ..card(id, FightKind::Encounter, name, at, "Me")
    }
}

/// A raid visit's Σ card: the instance's name, at `difficulty`.
fn sigma(id: &str, name: &str, at: i64, difficulty: u32) -> FightCard {
    FightCard {
        key: Some(KeyInfo {
            map_id: 2769,
            difficulty,
            ..KeyInfo::default()
        }),
        ..card(id, FightKind::Overall, name, at, "Me")
    }
}

/// A key run on the key clock `clock` against (par, +2, +3), `who` at
/// their rate over the whole of it.
fn key(
    id: &str,
    name: &str,
    at: i64,
    clock: i64,
    success: Option<bool>,
    who: CardPlayer,
) -> FightCard {
    let who = CardPlayer {
        damage: (who.dps * clock as f64 / 1000.0) as u64,
        ..who
    };
    FightCard {
        key: Some(KeyInfo {
            map_id: 2521,
            difficulty: 8,
            level: Some(14),
            completed: success,
        }),
        pars_ms: Some((1_800_000, 1_440_000, 1_080_000)),
        official_ms: Some(clock),
        duration_ms: clock,
        success,
        owner: Some(who.guid.clone()),
        players: vec![who],
        ..card(id, FightKind::Key, name, at, "Me")
    }
}

/// A raid of 19 damage dealers and 4 healers, `me` among the dealers at
/// `rate` (the others at 100..1900).
fn raid_of(me: f64) -> Vec<CardPlayer> {
    let mut players = vec![player("Me", Spec::Destruction, me)];
    players.extend((1..=18).map(|i| player(&format!("D{i}"), Spec::Fire, f64::from(i) * 100.0)));
    players.extend((1..=4).map(|i| player(&format!("H{i}"), Spec::HolyPriest, f64::from(i) * 1e4)));
    players
}

#[test]
fn parse_ymd_round_trips_and_rejects_nonsense() {
    assert_eq!(parse_ymd("1970-01-01"), Some(0));
    assert_eq!(parse_ymd("2000-03-01"), Some(951_868_800_000));
    assert_eq!(parse_ymd("2026-08-12"), Some(1_786_492_800_000));
    // 2024 is a leap year, 2026 is not.
    assert!(parse_ymd("2024-02-29").is_some());
    assert_eq!(parse_ymd("2026-02-29"), None);
    for bad in [
        "2026-13-01",
        "2026-02-30",
        "today",
        "",
        "2026-1-1",
        "2026-01-01-01",
    ] {
        assert_eq!(parse_ymd(bad), None, "{bad} parsed");
    }
}

#[test]
fn a_season_is_half_open() {
    let s = Season {
        start_utc_ms: parse_ymd("2026-01-01"),
        end_utc_ms: parse_ymd("2026-02-01"),
    };
    assert!(s.contains(parse_ymd("2026-01-01").unwrap()));
    assert!(s.contains(parse_ymd("2026-01-31").unwrap()));
    assert!(!s.contains(parse_ymd("2026-02-01").unwrap()));
    assert!(!s.contains(parse_ymd("2025-12-31").unwrap()));
    assert!(Season::default().contains(0), "an unset season is all time");
}

/// The rank is a place among the ROLE — the dealers by effective dps, the
/// healers by healing per second — never among everyone, and its
/// percentile puts 17th of 19 and 3rd of 5 on one scale.
#[test]
fn the_rank_is_a_percentile_within_the_role() {
    let c = boss("f", "Boss", 1, 0, true, raid_of(250.0));
    let me = standing(&c, "Me").unwrap();
    // 18 others at 100..1800: 16 of them above 250.
    assert_eq!((me.place, me.of), (17, 19), "among the dealers alone");
    assert_eq!(me.value, 250.0);
    assert!((me.percentile() - (1.0 - 16.0 / 18.0)).abs() < 1e-6);
    let healer = standing(&c, "H3").unwrap();
    assert_eq!((healer.place, healer.of), (2, 4), "a healer among healers");
    assert_eq!(healer.value, 3e4, "by hps");
    let top = standing(&c, "D18").unwrap();
    assert_eq!((top.place, top.percentile()), (1, 1.0));
    let bottom = standing(&c, "D1").unwrap();
    assert_eq!((bottom.place, bottom.percentile()), (19, 0.0));
    // Alone in the role is its top, and nobody's standing is invented.
    let solo = boss(
        "s",
        "Boss",
        1,
        0,
        true,
        vec![player("Me", Spec::Blood, 5.0)],
    );
    assert_eq!(standing(&solo, "Me").unwrap().percentile(), 1.0);
    assert_eq!(standing(&c, "Nobody"), None);
    // An enemy (an arena's other side) is never a peer.
    let mut arena = boss("a", "Arena", 1, 0, true, raid_of(250.0));
    let mut foe = player("Foe", Spec::Fire, 9999.0);
    foe.enemy = true;
    arena.players.push(foe);
    assert_eq!(standing(&arena, "Me").unwrap().place, 17);
    assert_eq!(standing(&arena, "Foe"), None);
}

/// "Last night": the scope's newest night, its pulls oldest first with
/// their outcome and standing, the place its raid's Σ names, on whom.
#[test]
fn the_night_is_the_newest_one_played() {
    let sun = evening("2026-09-27");
    let sat = evening("2026-09-26");
    let cards = vec![
        boss("u2", "Ula'tek", 2, sun + 3 * H, true, raid_of(1500.0)),
        FightCard {
            best_pct: Some(56),
            ..boss("u1", "Ula'tek", 2, sun + 2 * H, false, raid_of(900.0))
        },
        boss("c", "The Coiled Altar", 1, sun + H, true, raid_of(250.0)),
        // Trash and the visit's Σ are no pulls of the night.
        card("t", FightKind::Trash, "Trash", sun + 90 * 60_000, "Me"),
        sigma("v", "The Venomous Abyss", sun, 15),
        key(
            "k",
            "Kings' Rest +14",
            sat,
            1_700_000,
            Some(true),
            player("Me", Spec::Destruction, 200.0),
        ),
    ];
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    let n = panels.night.expect("a night");
    assert_eq!(n.day, rail::night_of(sun));
    assert_eq!(n.place, "The Venomous Abyss, Heroic");
    assert_eq!(n.who.guid, "Me");
    assert_eq!(n.measure, "effective dps");
    let names: Vec<&str> = n.pulls.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["The Coiled Altar", "Ula'tek", "Ula'tek"],
        "oldest first"
    );
    let marks: Vec<Mark> = n.pulls.iter().map(|p| p.mark).collect();
    assert_eq!(marks, [Mark::Good, Mark::Bad, Mark::Good]);
    assert_eq!(n.pulls[1].wipe_pct, Some(56), "a wipe's best, beside it");
    assert_eq!(
        n.pulls.iter().map(|p| p.standing.place).collect::<Vec<_>>(),
        [17, 10, 4]
    );
    // A night of keys is "Mythic+ keys", and a healer's is by hps.
    let keys = vec![key(
        "k",
        "Kings' Rest +14",
        sat,
        1_700_000,
        Some(true),
        player("Me", Spec::HolyPriest, 200.0),
    )];
    let n = derive(&keys, None, &Season::default(), &[], &[])
        .night
        .unwrap();
    assert_eq!(n.place, "Mythic+ keys");
    assert_eq!(n.measure, "healing per second");
    assert_eq!(n.pulls[0].mark, Mark::Good, "a timed key");
}

/// Without a Σ of its own night, a boss pull is named for the instance
/// another pull of the same boss was filed under; with nothing to go by,
/// for the kind of content it was.
#[test]
fn a_raid_pull_finds_its_instance() {
    let sun = evening("2026-09-27");
    let wed = evening("2026-09-23");
    let cards = vec![
        boss("c2", "The Coiled Altar", 1, sun + H, true, raid_of(250.0)),
        boss("c1", "The Coiled Altar", 1, wed + H, false, raid_of(250.0)),
        sigma("v", "The Venomous Abyss", wed, 15),
    ];
    let n = derive(&cards, None, &Season::default(), &[], &[])
        .night
        .unwrap();
    assert_eq!(n.place, "The Venomous Abyss, Heroic");
    let lone = vec![boss("x", "Somebody", 9, sun, true, raid_of(250.0))];
    let n = derive(&lone, None, &Season::default(), &[], &[])
        .night
        .unwrap();
    assert_eq!(
        n.place, "Heroic raid",
        "no placeholder that reads as a name"
    );
    // A raid night still going: no Σ card yet (the daemon stores it when
    // the visit closes), but the tailed log's own visit holds the pull.
    let tailed = |log| rail::LogInstance {
        log,
        lo: sun - H,
        hi: sun + H,
        name: "The Venomous Abyss".to_string(),
    };
    let n = derive(&lone, None, &Season::default(), &[], &[tailed(7)])
        .night
        .unwrap();
    assert_eq!(n.place, "The Venomous Abyss, Heroic");
    let n = derive(&lone, None, &Season::default(), &[], &[tailed(8)])
        .night
        .unwrap();
    assert_eq!(n.place, "Heroic raid", "another log's visit names nothing");
    // A Σ card of its night still wins over the log.
    let closed = vec![lone[0].clone(), sigma("v", "Sepulcher", sun - H, 15)];
    let n = derive(&closed, None, &Season::default(), &[], &[tailed(7)])
        .night
        .unwrap();
    assert_eq!(n.place, "Sepulcher, Heroic");
    // A pull whose card names its map finds the instance a Σ on that map
    // was, of another log and another month.
    let mapped = vec![
        FightCard {
            key: Some(KeyInfo {
                map_id: 2769,
                difficulty: 15,
                ..KeyInfo::default()
            }),
            ..boss("x", "Somebody", 9, sun, true, raid_of(250.0))
        },
        FightCard {
            log: 8,
            ..sigma("o", "Sepulcher of the Ashen Vow", sun - 30 * DAY, 15)
        },
    ];
    let n = derive(&mapped, None, &Season::default(), &[], &[])
        .night
        .unwrap();
    assert_eq!(n.place, "Sepulcher of the Ashen Vow, Heroic");
    // A Σ on its map outranks the tailed log's visit holding it.
    let n = derive(&mapped, None, &Season::default(), &[], &[tailed(7)])
        .night
        .unwrap();
    assert_eq!(n.place, "Sepulcher of the Ashen Vow, Heroic");
    assert_eq!(places(vec!["A".into(), "B".into(), "A".into()]), "A and B");
    assert_eq!(
        places(vec!["A".into(), "B".into(), "C".into()]),
        "A, B and more"
    );
    assert_eq!(
        places(vec!["Dungeons".into(), "Mythic+ keys".into()]),
        "Mythic+ keys",
        "a dungeon run on a keys night is one of the keys"
    );
}

/// A key's result against its timers: the chests it earned, or "over" —
/// the game's verdict where the card has one, the clock's where not.
#[test]
fn keys_are_read_against_their_timers() {
    let me = || player("Me", Spec::Destruction, 200.0);
    let at = evening("2026-09-26");
    let run = |id: &str, clock, success| key(id, "Kings' Rest +14", at, clock, success, me());
    let cards = vec![
        run("plus3", 1_000_000, Some(true)),
        FightCard {
            start_utc_ms: at + 7 * H - 1,
            ..run("plus2", 1_300_000, Some(true))
        },
        FightCard {
            start_utc_ms: at + 7 * H - 2,
            ..run("plus1", 1_700_000, Some(true))
        },
        FightCard {
            start_utc_ms: at + 7 * H - 3,
            ..run("over", 1_900_000, Some(false))
        },
        FightCard {
            start_utc_ms: at + 7 * H - 4,
            ..run("unsaid", 1_900_000, None)
        },
    ];
    let keys = derive(&cards, None, &Season::default(), &[], &[]).keys;
    let results: Vec<(&str, String, bool)> = keys
        .iter()
        .map(|k| (k.fight_id.as_str(), k.result(), k.timed))
        .collect();
    assert_eq!(
        results,
        [
            ("plus3", "+3".to_string(), true),
            ("plus2", "+2".to_string(), true),
            ("plus1", "+1".to_string(), true),
            ("over", "over".to_string(), false),
            ("unsaid", "over".to_string(), false),
        ],
        "newest first"
    );
    assert_eq!(keys[0].name, "Kings' Rest +14", "the level said once");
    assert_eq!(keys[0].who.guid, "Me");
    // The bar: a quarter past the timer, its ticks at +3, +2 and par.
    let bar = ParBar {
        clock_ms: 1_900_000,
        pars: (1_800_000, 1_440_000, 1_080_000),
        timed: false,
    };
    assert_eq!(bar.x_of(1_800_000, 250.0), 200.0, "par at 80%");
    assert_eq!(bar.x_of(1_440_000, 250.0), 160.0);
    assert_eq!(bar.x_of(1_080_000, 250.0), 120.0);
    assert_eq!(bar.x_of(9_000_000, 250.0), 250.0, "clamped at the end");
    assert_eq!(dungeon_name("Skyreach +15"), "Skyreach");
    assert_eq!(dungeon_name("Halls of Valor +"), "Halls of Valor +");
}

/// The raid panel: one per instance and difficulty, bosses in the raid's
/// order by encounter id, the fastest kill, the closest OBSERVED wipe, and
/// a dot per pull in whose colour.
#[test]
fn the_raid_panel_has_a_dot_per_pull() {
    let sun = evening("2026-09-27");
    let fri = evening("2026-09-25");
    let alt = |id: &str, enc, at, kill, pct| FightCard {
        best_pct: pct,
        owner: Some("Alt".to_string()),
        players: vec![player("Alt", Spec::Blood, 100.0)],
        ..boss(
            id,
            if enc == 3 {
                "The Lost Explorers"
            } else {
                "Ula'tek"
            },
            enc,
            at,
            kill,
            vec![],
        )
    };
    let cards = vec![
        sigma("v1", "The Venomous Abyss", sun, 15),
        FightCard {
            duration_ms: 400_000,
            ..boss("u2", "Ula'tek", 2, sun + 2 * H, true, raid_of(500.0))
        },
        boss("u1", "Ula'tek", 2, sun + H, false, raid_of(500.0)),
        sigma("v0", "The Venomous Abyss", fri, 15),
        alt("e3", 3, fri + 3 * H, false, Some(0)),
        alt("e2", 3, fri + 2 * H, false, Some(2)),
        alt("e1", 3, fri + H, false, Some(55)),
    ];
    let raids = derive(&cards, None, &Season::default(), &[], &[]).raids;
    assert_eq!(raids.len(), 1, "one raid at one difficulty");
    let r = &raids[0];
    assert_eq!(r.title, "The Venomous Abyss, Heroic");
    let bosses: Vec<(&str, usize)> = r
        .bosses
        .iter()
        .map(|b| (b.name.as_str(), b.pulls.len()))
        .collect();
    assert_eq!(
        bosses,
        [("Ula'tek", 2), ("The Lost Explorers", 3)],
        "by encounter id"
    );
    let ula = &r.bosses[0];
    assert_eq!(ula.best_kill_ms, Some(400_000));
    assert_eq!(ula.fight_id, "u2", "the row leads to the kill");
    assert_eq!(
        ula.pulls.iter().map(|d| d.kill).collect::<Vec<_>>(),
        [false, true],
        "oldest first"
    );
    assert_eq!(boss_outcome(ula).0, "Killed in 6:40");
    let lost = &r.bosses[1];
    assert_eq!(
        lost.best_pct,
        Some(2),
        "the 0 was never seen, and is no best"
    );
    assert_eq!(lost.fight_id, "e3", "unkilled, the newest pull");
    assert!(lost.pulls.iter().all(|d| d.who.guid == "Alt"), "whose dots");
    assert_eq!(boss_outcome(lost).0, "Best 2%");
    let blind = BossLine {
        best_pct: None,
        ..lost.clone()
    };
    assert_eq!(boss_outcome(&blind).0, "No kill", "never 0%, never 100%");
    // Scoped to one character, only their pulls are dots.
    let raids = derive(&cards, Some("Alt"), &Season::default(), &[], &[]).raids;
    assert_eq!(raids[0].bosses.len(), 1);
    assert_eq!(raids[0].bosses[0].name, "The Lost Explorers");
}

/// Each character's personal best of the week is marked once; a
/// healer's run is no point on an effective-dps chart; the points run in
/// the order they were played.
#[test]
fn each_character_s_best_run_is_marked() {
    let fri = evening("2026-09-25");
    let sat = evening("2026-09-26");
    let run =
        |id: &str, at, who: CardPlayer| key(id, "Murder Row +14", at, 1_600_000, Some(true), who);
    let cards = vec![
        run("b3", sat + 3 * H, player("Alt", Spec::Havoc, 180.0)),
        run("b2", sat + 2 * H, player("Alt", Spec::Havoc, 150.0)),
        run("h1", sat + H, player("Alt", Spec::HolyPriest, 90.0)),
        run("a3", fri + 3 * H, player("Me", Spec::Destruction, 270.0)),
        run("a2", fri + 2 * H, player("Me", Spec::Destruction, 292.0)),
        run("a1", fri + H, player("Me", Spec::Destruction, 245.0)),
    ];
    let trend = derive(&cards, Some("Alt"), &Season::default(), &[], &[]).trend;
    assert_eq!(trend.len(), 2, "the healer's run is no point");
    let trend = derive(
        &cards
            .iter()
            .map(|c| FightCard {
                owner: c.players.first().map(|p| p.guid.clone()),
                ..c.clone()
            })
            .collect::<Vec<_>>(),
        None,
        &Season::default(),
        &[],
        &[],
    )
    .trend;
    let ids: Vec<&str> = trend.iter().map(|t| t.fight_id.as_str()).collect();
    assert_eq!(ids, ["a1", "a2", "a3", "b2", "b3"], "oldest first");
    let best: Vec<&str> = trend
        .iter()
        .filter(|t| t.best)
        .map(|t| t.fight_id.as_str())
        .collect();
    assert_eq!(best, ["a2", "b3"], "one best per character");
    assert_eq!(trend[0].day, rail::night_of(fri));
    assert!((trend[1].value - 292.0).abs() < 1e-9);
}

/// The week is the seven days back from the store's newest card; an
/// aborted pull is never one; the chips are the owners in hand, and a
/// configured character with none.
#[test]
fn the_week_is_the_newest_card_s() {
    let sun = evening("2026-09-27");
    let me = || raid_of(250.0);
    let cards = vec![
        boss("new", "Boss", 1, sun, true, me()),
        boss("old", "Boss", 1, sun - 8 * DAY, true, me()),
        FightCard {
            aborted: true,
            ..boss("cut", "Boss", 1, sun - DAY, true, me())
        },
    ];
    let panels = derive(
        &cards,
        None,
        &Season::default(),
        &["Alt-Realm-US".to_string()],
        &[],
    );
    let r = &panels.raids[0];
    assert_eq!(r.bosses[0].pulls.len(), 1, "the week's pull alone");
    assert_eq!(panels.owner.as_ref().map(|o| o.guid.as_str()), Some("Me"));
    let chars: Vec<&str> = panels.characters.iter().map(|c| c.name.as_str()).collect();
    assert!(chars.contains(&"Me-Realm-US") && chars.contains(&"Alt-Realm-US"));
    // A store whose cards name no owner: nothing is "yours", and it says so.
    let unowned: Vec<FightCard> = cards
        .iter()
        .map(|c| FightCard {
            owner: None,
            ..c.clone()
        })
        .collect();
    let panels = derive(&unowned, None, &Season::default(), &[], &[]);
    assert!(panels.unowned && panels.night.is_none() && panels.owner.is_none());
    // …and a card with no owner names no character: the chips are the
    // configured ones alone.
    assert!(panels.characters.is_empty(), "nobody's pulls name nobody");
    let configured = derive(
        &unowned,
        None,
        &Season::default(),
        &["Alt-Realm-US".to_string()],
        &[],
    );
    let names: Vec<&str> = configured
        .characters
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Alt-Realm-US"]);
    // …which a scope answers regardless: the player is on the cards.
    let scoped = derive(&unowned, Some("Me"), &Season::default(), &[], &[]);
    assert!(!scoped.unowned && scoped.night.is_some());
}

#[test]
fn an_empty_store_derives_empty_panels_without_panicking() {
    let panels = derive(&[], None, &Season::default(), &[], &[]);
    assert_eq!(panels, Panels::default());
    let panels = derive(&[], Some("Me"), &Season::default(), &[], &[]);
    assert!(panels.night.is_none() && panels.keys.is_empty() && !panels.unowned);
}

#[test]
fn derive_over_the_fixture_store() {
    let mock = MockDaemon::fixture()
        .with_characters(&["Thraxx-Nebula-US".to_string()])
        .with_history();
    let cards = mock.history().cards().to_vec();
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    assert_eq!(
        panels,
        derive(&cards, None, &Season::default(), &[], &[]),
        "pure"
    );
    let owner = panels.owner.clone().expect("the store names Thraxx");
    assert_eq!(owner.name, "Thraxx-Nebula-US");
    let n = panels.night.expect("Thraxx's night");
    // The fixture's visit never closed, so no Σ names its instance, and
    // without the tailed log's list the pulls say what content they were.
    assert_eq!(n.place, "Heroic raid");
    assert_eq!(n.pulls.len(), 2, "the fixture's two bosses");
    assert_eq!(panels.raids.len(), 1);
    assert!(panels.keys.is_empty() && panels.trend.is_empty());
}

/// The tailed log's visits as Home reads them (`rail::log_instances`), built
/// from the daemon's own list: the fixture's raid visit, by its Σ row's
/// name, spanning its members on the log's clock, of the list's log — the
/// clock the store's cards are on, so with it the fixture's night, whose
/// visit never closed (no Σ card), is named for its instance. A key's
/// visit is none of them: its Σ is named for the key, not a place.
#[test]
fn the_tailed_log_names_a_night_still_going() {
    let mock = MockDaemon::fixture()
        .with_characters(&["Thraxx-Nebula-US".to_string()])
        .with_history();
    let cards = mock.history().cards().to_vec();
    let (state, _) = crate::window::testkit::indexed_on(mock);
    let log = state.log_id().expect("the list names its log");
    let visits = rail::log_instances(state.entries(), Some(log));
    assert_eq!(visits.len(), 1, "{visits:?}");
    let v = &visits[0];
    assert_eq!(v.name, "Sepulcher of the Ashen Vow");
    assert_eq!(v.log, log);
    let members: Vec<&wowdps_model::ListRow> = state
        .entries()
        .iter()
        .map(|e| &e.row)
        .filter(|r| r.instance.is_some())
        .collect();
    let lo = members.iter().map(|r| r.start_ms).min().unwrap();
    let hi = members
        .iter()
        .map(|r| r.start_ms + r.duration_ms)
        .max()
        .unwrap();
    assert_eq!((v.lo, v.hi), (lo, hi), "its members' span");
    // Every boss card of the fixture is held by it: the list's clock and
    // the cards' are one.
    let bosses: Vec<&FightCard> = cards
        .iter()
        .filter(|c| c.kind == FightKind::Encounter)
        .collect();
    assert_eq!(bosses.len(), 2);
    assert!(bosses.iter().all(|c| v.holds(c)), "{visits:?}");
    let n = derive(&cards, None, &Season::default(), &[], &visits)
        .night
        .expect("Thraxx's night");
    assert_eq!(n.place, "Sepulcher of the Ashen Vow, Heroic");
    // No log, no visits.
    assert!(rail::log_instances(state.entries(), None).is_empty());

    // A key's visit is left out.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/instance.txt");
    let (state, _) =
        crate::window::testkit::indexed_on(MockDaemon::fixture_at(std::path::Path::new(path)));
    let keys: Vec<&str> = state
        .entries()
        .iter()
        .filter(|e| e.row.kind == wowdps_model::SegmentKind::Overall && e.row.pars_ms.is_some())
        .map(|e| e.row.name.as_str())
        .collect();
    assert!(!keys.is_empty(), "the fixture holds a key's visit");
    let visits = rail::log_instances(state.entries(), state.log_id());
    assert!(
        visits.iter().all(|v| !keys.contains(&v.name.as_str())),
        "{visits:?} {keys:?}"
    );
}

/// An empty tail is not the top of the list: rewinding the cursor would
/// re-request page one forever.
#[test]
fn an_empty_page_keeps_the_paging_cursor() {
    let cards = vec![boss("a", "Boss", 1, 0, true, raid_of(1.0))];
    let mut home = Home::new();
    let _ = home.next_request(1, &Season::default());
    home.absorb(
        1,
        &HistoryAnswer::Fights {
            cards,
            total: u32::MAX,
        },
    );
    let cursor = home.cursor.clone();
    assert!(cursor.is_some());
    let _ = home.next_request(2, &Season::default());
    home.absorb(
        2,
        &HistoryAnswer::Fights {
            cards: Vec::new(),
            total: u32::MAX,
        },
    );
    assert_eq!(home.cursor, cursor, "the cursor survives an empty answer");
}

/// One request in flight; a stale answer lands nowhere; pages dedupe; and
/// the reads stop once the week is in hand, or the store is whole.
#[test]
fn home_reads_one_page_at_a_time_until_the_week_is_in_hand() {
    let sun = evening("2026-09-27");
    let page = |from: i64| -> Vec<FightCard> {
        (0..3)
            .map(|i| {
                boss(
                    &format!("f{from}-{i}"),
                    "Boss",
                    1,
                    from - i * DAY,
                    true,
                    vec![],
                )
            })
            .collect()
    };
    let mut home = Home::new();
    assert!(home.next_request(1, &Season::default()).is_some());
    assert_eq!(
        home.next_request(2, &Season::default()),
        None,
        "one request in flight at a time"
    );
    let answer = HistoryAnswer::Fights {
        cards: page(sun),
        total: 100,
    };
    home.absorb(99, &answer);
    assert!(home.cards.is_empty(), "an unknown req_id lands nowhere");
    home.absorb(1, &answer);
    assert_eq!(home.cards.len(), 3);
    assert!(!home.week_read(), "three days of the week in hand");
    assert!(home.next_request(3, &Season::default()).is_some(), "more");
    home.absorb(3, &answer);
    assert_eq!(home.cards.len(), 3, "a second fold dedupes");
    let _ = home.next_request(4, &Season::default());
    home.absorb(
        4,
        &HistoryAnswer::Fights {
            cards: page(sun - 6 * DAY),
            total: 100,
        },
    );
    assert!(home.week_read(), "a card from before the week");
    assert_eq!(home.next_request(5, &Season::default()), None, "done");
    // A store that is all in hand stops too.
    let mut whole = Home::new();
    let _ = whole.next_request(1, &Season::default());
    whole.absorb(
        1,
        &HistoryAnswer::Fights {
            cards: page(sun),
            total: 3,
        },
    );
    assert!(whole.complete());
    assert_eq!(whole.next_request(2, &Season::default()), None);
    // And a week too big to read says so.
    let mut big = Home::new();
    for i in 0..MAX_PAGES {
        assert!(big.next_request(i, &Season::default()).is_some());
        big.absorb(
            i,
            &HistoryAnswer::Fights {
                cards: vec![boss(&format!("b{i}"), "Boss", 1, sun, true, vec![])],
                total: u32::MAX,
            },
        );
    }
    assert!(big.stalled());
    assert_eq!(big.next_request(99, &Season::default()), None);
}

/// A wide grid of three, one column at the default 460 — and at the
/// prototype's 330 px a panel, two across a tiled window.
#[test]
fn the_panel_grid_follows_the_width() {
    assert_eq!(columns_for(436.0, 18.0), 1, "the default window");
    assert_eq!(columns_for(916.0, 18.0), 2, "a tiled window");
    assert_eq!(columns_for(1159.0, 18.0), 3, "wide, beside the rail");
    assert_eq!(columns_for(5000.0, 18.0), 3, "never more than three");
    // Degenerate widths must not divide by anything.
    assert_eq!(columns_for(0.0, 18.0), 1);
    assert_eq!(columns_for(f32::NAN, 18.0), 1);
    assert_eq!(columns_for(f32::INFINITY, 18.0), 1);
    // Every week shows the prototype's three panels, and one more per
    // further raid; never more columns than panels (`auto-fit` collapses
    // an empty track), so no row leaves a hole beside its panels.
    let mut panels = Panels::default();
    assert_eq!(panel_count(&panels), 3, "an empty week's three");
    assert_eq!(grid_columns(1159.0, panel_count(&panels)), 3);
    assert_eq!(grid_columns(1159.0, 2), 2, "two panels share the row");
    assert_eq!(grid_columns(1159.0, 0), 1);
    assert_eq!(grid_columns(436.0, 3), 1);
    let raid = || RaidPanel {
        title: String::new(),
        bosses: Vec::new(),
        newest: 0,
    };
    panels.raids = vec![raid(), raid()];
    assert_eq!(panel_count(&panels), 4, "a second raid of the week");
}

/// A key's row keeps its bar at 90 px at least, and shares the rest with
/// the name 1 : 1.1.
#[test]
fn a_key_row_keeps_its_bar() {
    let (name, bar) = key_widths(340.0);
    assert!((name + bar - (340.0 - 8.0 - 66.0 - 30.0)).abs() < 1e-3);
    assert!((bar / name - 1.1).abs() < 1e-3);
    let (_, bar) = key_widths(220.0);
    assert_eq!(bar, 90.0, "never under its least");
    assert_eq!(key_widths(0.0), (0.0, 0.0));
}

/// The value axis the prototype drew for runs of 143k–293k: 120k to 320k,
/// lines at 150k, 200k, 250k and 300k.
#[test]
fn the_throughput_axis_is_round() {
    let (lo, hi, ticks) = axis(&[142_613.0, 292_570.0, 184_083.0]);
    assert!(
        (lo - 120_113.0).abs() < 1.0 && (hi - 320_070.0).abs() < 1.0,
        "{lo} {hi}"
    );
    assert_eq!(ticks, [150_000.0, 200_000.0, 250_000.0, 300_000.0]);
    assert_eq!(
        ticks.iter().map(|t| axis_label(*t)).collect::<Vec<_>>(),
        ["150k", "200k", "250k", "300k"]
    );
    assert_eq!(axis_label(2_500_000.0), "2.5M");
    assert_eq!(axis_label(800.0), "800");
    // One run, or none: an axis all the same.
    let (lo, hi, ticks) = axis(&[200_000.0]);
    assert!(lo < 200_000.0 && hi > 200_000.0 && !ticks.is_empty());
    assert_eq!(axis(&[]), (0.0, 1.0, Vec::new()));
}

#[test]
fn the_night_is_worded_by_how_long_ago() {
    let sun = rail::night_of(evening("2026-09-27"));
    assert_eq!(long_date(sun, sun), "Sunday, September 27");
    assert_eq!(
        long_date(sun - 365, sun),
        "Saturday, September 27, 2025",
        "another year says so"
    );
    assert_eq!(night_cap(sun, sun), "Tonight you played");
    assert_eq!(night_cap(sun - 1, sun), "Last night you played");
}

impl Home {
    /// Fold cards in without a round trip, for render tests.
    fn absorb_for_test(&mut self, cards: Vec<FightCard>) {
        let total = cards.len() as u32;
        self.pending = Some(0);
        self.absorb(0, &HistoryAnswer::Fights { cards, total });
    }
}

fn chars() -> Vec<CharLine> {
    ["Me", "Alt"]
        .into_iter()
        .map(|g| CharLine {
            guid: g.to_string(),
            name: format!("{g}-Realm-US"),
            class: Some(wowdps_model::Class::Warlock),
            ..CharLine::default()
        })
        .collect()
}

/// Loading, empty, off and degraded are four different screens — never
/// the same confident nothing. Until the store is on and has answered,
/// no panel says "none" of what nobody read: the night card, captioned
/// neutrally, carries the state's own words, and the grid waits.
#[test]
fn the_screen_words_each_empty_store_apart() {
    let draw = |home: &Home, panels: &Panels| {
        simulator(screen(home, panels, &chars(), theme::GOLD_ACCENT, false, 0))
    };
    let loading = Home::new();
    let mut ui = draw(&loading, &Panels::default());
    assert!(ui.find(panels::EMPTY_CAP).is_ok(), "a neutral caption");
    assert!(ui.find("Last night you played").is_err());
    assert!(
        ui.find("Your week is on its way from the history store.")
            .is_ok()
    );
    for none in [
        "No pulls stored this week.",
        "No keys this week.",
        "No raid pulls this week.",
    ] {
        assert!(ui.find(none).is_err(), "{none}: not empty, unread");
    }
    let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();

    let mut empty = Home::new();
    empty.answered = true;
    let mut ui = draw(&empty, &Panels::default());
    assert!(ui.find(panels::EMPTY_CAP).is_ok());
    assert!(ui.find("No pulls stored this week.").is_ok());
    assert!(ui.find("No keys this week.").is_ok());
    assert!(ui.find("No raid pulls this week.").is_ok());
    assert!(
        ui.find("Your week is on its way from the history store.")
            .is_err()
    );

    let mut off = empty;
    off.disabled_reason = Some("history_enabled = false".to_string());
    let mut ui = draw(&off, &Panels::default());
    assert!(
        ui.find("The history store is off: history_enabled = false.")
            .is_ok()
    );
    assert!(ui.find("No keys this week.").is_err(), "off is not empty");

    let mut degraded = Home::new();
    degraded.answered = true;
    degraded.dropped = 3;
    let mut ui = draw(&degraded, &Panels::default());
    assert!(
        ui.find("The daemon dropped 3 requests: this is not the whole week.")
            .is_ok()
    );
    assert!(ui.find("No keys this week.").is_ok(), "the week it has");
    degraded.dropped = 1;
    let mut ui = draw(&degraded, &Panels::default());
    assert!(
        ui.find("The daemon dropped 1 request: this is not the whole week.")
            .is_ok()
    );

    let unowned = Panels {
        unowned: true,
        ..Panels::default()
    };
    let mut ui = draw(&degraded, &unowned);
    assert!(
        ui.find(
            "The store has not named a character of yours yet: list them in \
             history_characters, or install the wowdps addon."
        )
        .is_ok()
    );
}

/// The chips: all characters, then each one; the scoped one pressed and
/// named in the title; a press is the scope's message.
#[test]
fn the_scope_chips_name_and_scope_the_week() {
    let sun = evening("2026-09-27");
    let cards = vec![
        boss("c", "The Coiled Altar", 1, sun + H, true, raid_of(250.0)),
        sigma("v", "The Venomous Abyss", sun, 15),
    ];
    let mut home = Home::new();
    home.absorb_for_test(cards.clone());
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    let mut ui = simulator(screen(
        &home,
        &panels,
        &chars(),
        theme::GOLD_ACCENT,
        true,
        0,
    ));
    assert!(ui.find("You, this week").is_ok());
    for chip in ["All characters", "Me", "Alt"] {
        assert!(ui.find(chip).is_ok(), "{chip}");
    }
    // The night, its tile and its standing.
    assert!(ui.find("The Venomous Abyss, Heroic").is_ok());
    assert!(ui.find("The Coiled Altar").is_ok());
    assert!(ui.find("17th").is_ok() && ui.find(" of 19, 250").is_ok());
    ui.click("Alt").unwrap();
    ui.click("All characters").unwrap();
    ui.click("The Coiled Altar").unwrap();
    let sent: Vec<crate::window::Message> = ui.into_messages().collect();
    assert!(
        matches!(
            sent.as_slice(),
            [
                crate::window::Message::HomeCharacter(Some(alt)),
                crate::window::Message::HomeCharacter(None),
                crate::window::Message::OpenStored(id),
            ] if alt == "Alt" && id == "c"
        ),
        "{sent:?}"
    );
    // Scoped: the title is theirs, and their empty week says so by name.
    home.scope = Some("Alt".to_string());
    let panels = derive(&cards, Some("Alt"), &Season::default(), &[], &[]);
    let mut ui = simulator(screen(
        &home,
        &panels,
        &chars(),
        theme::GOLD_ACCENT,
        true,
        0,
    ));
    assert!(ui.find("Alt, this week").is_ok());
    assert!(ui.find("No pulls on Alt this week.").is_ok());
    assert!(ui.find("No keys on Alt this week.").is_ok());
    let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();

    // A long night: a wipe's tile says how close it came, and the pulls
    // before the newest few are counted rather than listed.
    let mut cards = vec![sigma("v", "The Venomous Abyss", sun, 15)];
    for i in 0..(MAX_TILES + 2) as i64 {
        cards.push(boss(
            &format!("w{i}"),
            "Ula'tek",
            2,
            sun + (i + 1) * 10 * 60_000,
            false,
            raid_of(250.0),
        ));
    }
    if let Some(last) = cards.last_mut() {
        last.best_pct = Some(56);
    }
    let mut home = Home::new();
    home.absorb_for_test(cards.clone());
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    let mut ui = simulator(screen(
        &home,
        &panels,
        &chars(),
        theme::GOLD_ACCENT,
        true,
        0,
    ));
    assert!(ui.find("at 56%").is_ok(), "a wipe's best after its name");
    assert!(
        ui.find("2 pulls earlier that night, on the chart and the rail")
            .is_ok()
    );
}

/// A personal best's words on the throughput chart (canvas text, which no
/// selector finds): the figure as the meter prints it.
#[test]
fn a_personal_best_is_labelled_with_its_figure() {
    assert_eq!(super::charts::best_label(292_600.0), "best 292.6k");
    assert_eq!(super::charts::best_label(1_234_567.0), "best 1.2M");
    assert_eq!(super::charts::best_label(-1.0), "best 0");
}

/// The week's panels drawn: a key's result and time, a boss's outcome and
/// count, the best run's figure on the chart's legend — at a wide and a
/// narrow width alike.
#[test]
fn the_week_s_panels_render_at_every_width() {
    let fri = evening("2026-09-25");
    let cards = vec![
        key(
            "k1",
            "Kings' Rest +14",
            fri + 2 * H,
            1_900_000,
            Some(false),
            player("Me", Spec::Destruction, 150_000.0),
        ),
        key(
            "k2",
            "Murder Row +14",
            fri + H,
            1_300_000,
            Some(true),
            player("Me", Spec::Destruction, 292_600.0),
        ),
        FightCard {
            best_pct: Some(31),
            ..boss("b", "The Lost Explorers", 3, fri, false, raid_of(250.0))
        },
    ];
    let mut home = Home::new();
    home.absorb_for_test(cards.clone());
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    for w in [1440.0, 460.0] {
        let el = screen(&home, &panels, &chars(), theme::GOLD_ACCENT, true, 0);
        let mut ui = crate::window::testkit::simulator_as(
            crate::window::settings(),
            iced::Size::new(w, 1400.0),
            el,
        );
        for words in [
            "Keys this week",
            "over",
            " 31:40",
            "+2",
            " 21:40",
            "Best 31%",
            ", 1 pull",
            "Effective dps on keys",
            "personal best",
            "Ticks mark +3, +2 and the timer, left to right",
        ] {
            assert!(ui.find(words).is_ok(), "{words} at {w}");
        }
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
    }
}

/// The charts' rules: which ranks a crowded night keeps, hollow for what
/// went wrong, a rank that would print over the top guide's words hung
/// under its dot, and a night named under its first run alone.
#[test]
fn the_charts_mark_what_they_should() {
    use super::charts::{RankSlope, day_labels, hollow, rank_under};
    assert!(hollow(Mark::Bad), "a wipe, a key over time");
    assert!(!hollow(Mark::Good) && !hollow(Mark::Dash));
    let pull = |place: usize, mark| NightPull {
        fight_id: format!("p{place}"),
        name: "Boss".to_string(),
        mark,
        wipe_pct: None,
        standing: Standing {
            place,
            of: 19,
            value: 1.0,
        },
    };
    let slope = |pulls: &[NightPull]| RankSlope::new(pulls, iced::Color::WHITE);
    // A night of three: every rank is said.
    let few = [
        pull(17, Mark::Good),
        pull(10, Mark::Bad),
        pull(6, Mark::Good),
    ];
    assert_eq!(slope(&few).labelled(300.0), [true, true, true]);
    // A crowded night: the first, the lowest, the highest and the last.
    let crowd: Vec<NightPull> = [9, 12, 3, 15, 19, 8, 11, 14, 2, 10, 7, 13, 16, 5]
        .into_iter()
        .map(|p| pull(p, Mark::Bad))
        .collect();
    let kept: Vec<usize> = slope(&crowd)
        .labelled(300.0)
        .into_iter()
        .enumerate()
        .filter_map(|(i, k)| k.then_some(i))
        .collect();
    assert_eq!(kept, [0, 4, 8, 13]);
    // The night's best at its end tops the role under "top of the role":
    // its rank hangs under its dot. At the left end, or lower, above.
    let topped = slope(&[pull(17, Mark::Good), pull(1, Mark::Good)]).dots(300.0);
    assert!(!rank_under(topped[0].0, 300.0), "low: above its dot");
    assert!(rank_under(topped[1].0, 300.0), "under the words: under it");
    let early = slope(&[pull(1, Mark::Good), pull(17, Mark::Good)]).dots(300.0);
    assert!(!rank_under(early[0].0, 300.0), "clear of the words");
    // The day is said where it changes, and nowhere else.
    let run = |day| TrendPoint {
        fight_id: String::new(),
        who: Char::default(),
        day,
        value: 1.0,
        best: false,
    };
    assert_eq!(
        day_labels(&[run(1), run(1), run(2), run(2), run(1)]),
        [true, false, true, false, true]
    );
    assert!(day_labels(&[]).is_empty());
}
