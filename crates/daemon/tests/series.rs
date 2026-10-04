//! v39: the history store's SERIES tier — a fight's abilities and targets
//! second by second, kept for kills, keys and pinned fights while their
//! details last — and what it is for: a stored pull's drill answering a
//! zoom window with exactly the rows the live drill answered.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::path::Path;

use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{Row, SegmentId, View};
use wowdps_proto::{
    Breakdown, ClientMsg, Cursor, DaemonMsg, FightSort, HistoryAnswer, StoredFight,
};
use wowdps_proto::{HistoryQuery, SegmentRef};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");

/// Replay a whole log the way the tail thread would, every `Closed` fight.
fn closed_fights(path: &Path) -> Vec<ClosedFight> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.to_path_buf()), &mut events);
    engine.on_tail(
        TailEvent::Lines(text.lines().map(str::to_string).collect()),
        &mut events,
    );
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Closed(id) => engine.take_closed(*id),
            EngineEvent::Opened(_) => None,
        })
        .collect()
}

fn stored(cfg: Retention) -> (Store<MemBackend>, Vec<ClosedFight>, Vec<String>) {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let facts = LogFacts::read(path);
    let mut store = Store::open(MemBackend::new(), cfg);
    let ids = fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    (store, fights, ids)
}

fn id_of(store: &Store<MemBackend>, name: &str) -> String {
    store
        .cards()
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.id.clone())
        .unwrap_or_else(|| panic!("{name} is stored"))
}

/// The kill keeps the series tier, a player's block read alone; the short
/// wipe (no details) and the raid's Σ (no verdict) keep none.
#[test]
fn a_kill_keeps_the_series_tier_and_a_short_wipe_does_not() {
    let (store, _, _) = stored(Retention::default());
    let kill = id_of(&store, "The Ashen Warden");
    let wipe = id_of(&store, "Verkath the Hollow");
    assert!(store.has_series(&kill));
    assert!(!store.has_series(&wipe), "a 45 s wipe earns no details");
    assert!(
        store
            .cards()
            .iter()
            .filter(|c| store.has_series(&c.id))
            .count()
            == 1,
        "the Σ has no verdict: no details, no series"
    );
    let guid = store
        .card(&kill)
        .and_then(|c| c.players.first())
        .map(|p| p.guid.clone())
        .expect("a player");
    let p = store.player_series(&kill, &guid).expect("their block");
    assert!(!p.damage.is_empty() && !p.targets.is_empty(), "{p:?}");
    let nobody = store
        .player_series(&kill, "Player-0-nobody")
        .expect("a readable file");
    assert!(
        nobody.is_empty(),
        "a player it does not hold had no seconds"
    );
    assert!(
        store.player_series("0-0", &guid).is_none(),
        "no file, no answer"
    );
}

/// A wipe that earns details earns the series tier when it is pinned —
/// rewritten from its log, the daemon queues it — and loses it when the pin
/// goes; a fight whose details are demoted loses it with them.
#[test]
fn a_pin_earns_a_wipe_its_series_and_letting_go_drops_it() {
    let cfg = Retention {
        details_min_wipe_secs: 30,
        ..Retention::default()
    };
    let (mut store, fights, _) = stored(cfg);
    let wipe = id_of(&store, "Verkath the Hollow");
    assert!(store.has_details(&wipe) && !store.has_series(&wipe));
    assert!(!store.wants_rewrite(&wipe), "unpinned, it wants none");
    assert!(store.pin(&wipe, true));
    assert!(store.wants_rewrite(&wipe), "pinned, it wants a rewrite");
    let facts = LogFacts::read(Path::new(SAMPLE));
    let fight = fights
        .iter()
        .find(|f| f.segment.name == "Verkath the Hollow")
        .expect("the wipe");
    assert_eq!(store.regrade(fight, facts), Some(wipe.clone()));
    assert!(store.has_series(&wipe), "the rewrite wrote it");
    assert!(!store.wants_rewrite(&wipe));
    assert!(store.pin(&wipe, false));
    assert!(!store.has_series(&wipe), "let go, it goes");
    assert!(store.has_details(&wipe), "the details stay");

    // Details demoted: the series goes with them.
    let cfg = Retention {
        keep_details_per_encounter: 0,
        ..Retention::default()
    };
    let (store, _, _) = stored(cfg);
    let kill = id_of(&store, "The Ashen Warden");
    assert!(
        store.has_details(&kill) == store.has_series(&kill),
        "never a series without its details"
    );
}

/// The kill's segment as the mock lists it, and its stored fight id.
fn kill(mock: &mut MockDaemon) -> (SegmentId, String) {
    let out = mock.handle(ClientMsg::Watch(Cursor::List));
    let seg = out
        .iter()
        .find_map(|m| match m {
            DaemonMsg::SegmentList { entries, .. } => entries
                .iter()
                .find(|e| e.row.name == "The Ashen Warden")
                .map(|e| e.id),
            _ => None,
        })
        .expect("the kill's segment");
    let out = mock.handle(ClientMsg::GetHistory {
        req_id: 1,
        query: HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Fastest,
            limit: 1,
            after_id: None,
            role: None,
        },
    });
    let id = out
        .iter()
        .find_map(|m| match m {
            DaemonMsg::History {
                answer: HistoryAnswer::Fights { cards, .. },
                ..
            } => cards.first().map(|c| c.id.clone()),
            _ => None,
        })
        .expect("the stored kill");
    (seg, id)
}

/// One row's words: key, label, amount, extra, count, crits, rate, share,
/// spell id, school.
type Words = (String, String, u64, u64, u64, u64, String, String, u32, u32);

/// What a breakdown's rows say, the window's own words — never `mine`,
/// which the two paths mark by different routes.
fn words(rows: &[Row]) -> Vec<Words> {
    rows.iter()
        .map(|r| {
            (
                r.key.clone(),
                r.label.clone(),
                r.amount,
                r.extra,
                r.count,
                r.crits,
                format!("{:.6}", r.per_sec),
                format!("{:.6}", r.pct),
                r.spell_id,
                r.school,
            )
        })
        .collect()
}

/// v39's gate: for every player of the kill, on Damage and Healing, over
/// several windows, the stored drill answers exactly what the live one
/// did — abilities, targets, the tree's groups and the echo.
#[test]
fn a_stored_window_answers_what_the_live_one_did() {
    let mut mock = MockDaemon::fixture_at(Path::new(SAMPLE)).with_history();
    let (seg, fight_id) = kill(&mut mock);
    let live = |mock: &mut MockDaemon, view: View, guid: &str, range| -> Breakdown {
        mock.handle(ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Id(seg),
            view,
            top_n: None,
            drill: Some(guid.to_string()),
            death: None,
            spell: None,
            range,
        }))
        .into_iter()
        .rev()
        .find_map(|m| match m {
            DaemonMsg::Snapshot {
                view: v,
                breakdown: Some(b),
                ..
            } if v == view => Some(b),
            _ => None,
        })
        .expect("the live drill")
    };
    let stored = |mock: &mut MockDaemon, view: View, guid: &str, range| {
        mock.handle(ClientMsg::GetFight {
            req_id: 9,
            fight_id: fight_id.clone(),
            view,
            drill: Some(guid.to_string()),
            death: None,
            boss: None,
            range,
            spell: None,
            pair: None,
        })
        .into_iter()
        .find_map(|m| match m {
            DaemonMsg::Fight { fight: Some(f), .. } => Some(f),
            _ => None,
        })
        .expect("the stored kill")
    };
    let guids: Vec<String> = stored(&mut mock, View::Damage, "x", None)
        .card
        .players
        .iter()
        .filter(|p| !p.enemy)
        .map(|p| p.guid.clone())
        .collect();
    assert!(guids.len() >= 3, "{guids:?}");
    let mut compared = 0;
    for guid in &guids {
        for view in [View::Damage, View::Healing] {
            for range in [
                (0, 60_000),
                (5_500, 21_200),
                (30_000, 31_000),
                (59_000, 90_000),
            ] {
                let a = live(&mut mock, view, guid, Some(range));
                let f = stored(&mut mock, view, guid, Some(range));
                assert!(f.series, "the kill keeps the series tier");
                let Some(b) = f.breakdown else {
                    // No details row for them on this view: no live list
                    // either.
                    assert!(a.by_spell.is_empty(), "{guid} {view:?}");
                    continue;
                };
                let what = format!("{guid} {view:?} {range:?}");
                assert_eq!(b.range, Some(range), "{what}: echoed");
                assert_eq!(b.range, a.range, "{what}: both echo it");
                assert_eq!(words(&b.by_spell), words(&a.by_spell), "{what}: abilities");
                assert_eq!(words(&b.by_target), words(&a.by_target), "{what}: targets");
                assert_eq!(b.tree, a.tree, "{what}: the tree's groups");
                compared += 1;
            }
        }
    }
    assert!(compared >= 12, "{compared} windows compared");
    // A fight without the tier answers whole and echoes nothing.
    let short_wipe = mock.handle(ClientMsg::GetHistory {
        req_id: 2,
        query: HistoryQuery::Fights {
            encounter: Some(3131),
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Newest,
            limit: 1,
            after_id: None,
            role: None,
        },
    });
    let wipe_id = short_wipe
        .iter()
        .find_map(|m| match m {
            DaemonMsg::History {
                answer: HistoryAnswer::Fights { cards, .. },
                ..
            } => cards.first().map(|c| c.id.clone()),
            _ => None,
        })
        .expect("the wipe");
    let out = mock.handle(ClientMsg::GetFight {
        req_id: 10,
        fight_id: wipe_id,
        view: View::Damage,
        drill: guids.first().cloned(),
        death: None,
        boss: None,
        range: Some((0, 10_000)),
        spell: None,
        pair: None,
    });
    let Some(DaemonMsg::Fight { fight: Some(f), .. }) = out.first() else {
        panic!("{out:?}");
    };
    assert!(!f.series && f.breakdown.as_ref().is_none_or(|b| b.range.is_none()));
}

/// v39 over a real log (`WOWDPS_REAL_LOG`): every kill or key it holds
/// stored in a scratch directory, every player's stored window equal to
/// the live meter's over the same seconds, and the tier's size on disk
/// beside the details tier's, printed.
///
/// `WOWDPS_REAL_LOG=… cargo test --release -p wowdps-daemon --test series -- --ignored --nocapture`
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_stored_windows_match_the_live_meter() {
    use wowdps_daemon::history::DirBackend;
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let path = Path::new(&path);
    let tmp = TempDir::new("wowdps-series");
    let dir = tmp.0.clone();
    let fights = closed_fights(path);
    let facts = LogFacts::read(path);
    let mut store = Store::open(DirBackend::new(dir.clone()), Retention::default());
    let size = |sub: &str, id: &str, ext: &str| {
        std::fs::metadata(dir.join(sub).join(format!("{id}.{ext}"))).map_or(0, |m| m.len())
    };
    let (mut checked, mut series_bytes, mut details_bytes) = (0usize, 0u64, 0u64);
    for fight in &fights {
        let Some(id) = store.store(fight, facts) else {
            continue;
        };
        if !store.has_series(&id) {
            continue;
        }
        let seg = &fight.segment;
        let card = store.card(&id).cloned().expect("the card");
        let secs = (card.duration_ms / 1000).max(0) as u32;
        let (s, d) = (size("series", &id, "bin"), size("details", &id, "json"));
        series_bytes += s;
        details_bytes += d;
        eprintln!(
            "{:>32} {:>4}s series {:>7} B details {:>7} B",
            seg.name.chars().take(32).collect::<String>(),
            secs,
            s,
            d
        );
        for p in card.players.iter().filter(|p| !p.enemy) {
            for view in [View::Damage, View::Healing] {
                for (lo, hi) in [(0, secs / 3), (secs / 3, secs / 2), (secs / 2, secs)] {
                    let range = (lo * 1000, hi * 1000 + 1);
                    if range.1 <= range.0 {
                        continue;
                    }
                    let (spells, targets) = seg.breakdown_ranged(
                        &p.guid,
                        view,
                        None,
                        Some((i64::from(range.0), i64::from(range.1))),
                    );
                    let f = store
                        .stored_fight_ranged(&id, view, Some(&p.guid), None, Some(range))
                        .expect("stored");
                    let Some(b) = f.breakdown else {
                        assert!(spells.is_empty(), "{} {view:?}", p.name);
                        continue;
                    };
                    assert_eq!(
                        words(&b.by_spell),
                        words(&spells),
                        "{} {view:?} {range:?}",
                        p.name
                    );
                    assert_eq!(
                        words(&b.by_target),
                        words(&targets),
                        "{} {view:?} {range:?}",
                        p.name
                    );
                    checked += 1;
                }
            }
        }
    }
    drop(tmp);
    eprintln!(
        "{checked} windows matched; series {series_bytes} B beside details {details_bytes} B"
    );
    assert!(checked > 0, "the log holds a kill or a key");
}

// ---- v42: a stored kill answers as the live one did ------------------------------

/// The live drill (a window's) and the stored one, for one ask.
fn live_drill(
    mock: &mut MockDaemon,
    seg: SegmentId,
    view: View,
    guid: &str,
    spell: Option<&str>,
    range: Option<(u32, u32)>,
) -> Breakdown {
    mock.handle(ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Id(seg),
        view,
        top_n: None,
        drill: Some(guid.to_string()),
        death: None,
        spell: spell.map(str::to_string),
        range,
    }))
    .into_iter()
    .rev()
    .find_map(|m| match m {
        DaemonMsg::Snapshot {
            view: v,
            breakdown: Some(b),
            ..
        } if v == view => Some(b),
        _ => None,
    })
    .expect("the live drill")
}

fn stored_ask(
    mock: &mut MockDaemon,
    fight_id: &str,
    view: View,
    guid: &str,
    spell: Option<&str>,
    range: Option<(u32, u32)>,
    pair: Option<&str>,
) -> StoredFight {
    mock.handle(ClientMsg::GetFight {
        req_id: 11,
        fight_id: fight_id.to_string(),
        view,
        drill: Some(guid.to_string()),
        death: None,
        boss: None,
        range,
        spell: spell.map(str::to_string),
        pair: pair.map(str::to_string),
    })
    .into_iter()
    .find_map(|m| match m {
        DaemonMsg::Fight { fight: Some(f), .. } => Some(f),
        _ => None,
    })
    .expect("the stored kill")
}

/// Everything an opened drill shows, `mine` aside (the two paths mark it by
/// different routes).
fn same_drill(stored: &Breakdown, live: &Breakdown, what: &str) {
    assert_eq!(
        words(&stored.by_spell),
        words(&live.by_spell),
        "{what}: abilities"
    );
    assert_eq!(
        words(&stored.by_target),
        words(&live.by_target),
        "{what}: targets"
    );
    assert_eq!(stored.range, live.range, "{what}: the echo");
    assert_eq!(stored.tree, live.tree, "{what}: the tree");
    assert_eq!(
        stored.ability_series, live.ability_series,
        "{what}: the stack"
    );
    assert_eq!(
        stored.target_series, live.target_series,
        "{what}: the target stack"
    );
    assert_eq!(
        stored.spell_timeline, live.spell_timeline,
        "{what}: the ability's curve"
    );
    assert_eq!(
        stored.spell_targets.as_deref().map(words),
        live.spell_targets.as_deref().map(words),
        "{what}: the ability's targets"
    );
    assert_eq!(stored.timeline, live.timeline, "{what}: the curve");
}

/// v42's gate: on every player of the stored kill, the drill's stacked
/// graph, every opened ability (its curve, its targets, their stack) whole
/// and over windows, and the count views' drills are the live ones.
#[test]
fn a_stored_kill_stacks_and_opens_abilities_as_the_live_one_does() {
    let mut mock = MockDaemon::fixture_at(Path::new(SAMPLE)).with_history();
    let (seg, fight_id) = kill(&mut mock);
    let guids: Vec<String> = stored_ask(&mut mock, &fight_id, View::Damage, "x", None, None, None)
        .card
        .players
        .iter()
        .filter(|p| !p.enemy)
        .map(|p| p.guid.clone())
        .collect();
    let (mut stacks, mut opened) = (0, 0);
    for guid in &guids {
        for view in [View::Damage, View::Healing] {
            for range in [None, Some((5_500, 21_200)), Some((0, 60_000))] {
                let live = live_drill(&mut mock, seg, view, guid, None, range);
                let f = stored_ask(&mut mock, &fight_id, view, guid, None, range, None);
                let what = format!("{guid} {view:?} {range:?}");
                let Some(b) = f.breakdown else {
                    assert!(live.by_spell.is_empty(), "{what}");
                    continue;
                };
                same_drill(&b, &live, &what);
                stacks += usize::from(!b.ability_series.is_empty());
                // Every ability the drill lists, opened.
                for key in live.by_spell.iter().map(|r| r.key.clone()) {
                    let live = live_drill(&mut mock, seg, view, guid, Some(&key), range);
                    let f = stored_ask(&mut mock, &fight_id, view, guid, Some(&key), range, None);
                    let b = f.breakdown.expect("the opened ability");
                    same_drill(&b, &live, &format!("{what} {key}"));
                    opened += 1;
                }
            }
        }
        // The count views' drills ride the details tier.
        for view in [View::Interrupts, View::CrowdControl, View::Dispels] {
            let live = live_drill(&mut mock, seg, view, guid, None, None);
            let f = stored_ask(&mut mock, &fight_id, view, guid, None, None, None);
            let what = format!("{guid} {view:?}");
            match f.breakdown {
                Some(b) => same_drill(&b, &live, &what),
                None => assert!(
                    live.by_spell.is_empty() && live.by_target.is_empty(),
                    "{what}: nothing kept, nothing live"
                ),
            }
        }
    }
    assert!(stacks >= 3, "{stacks} stacks compared");
    assert!(opened >= 10, "{opened} abilities opened");
}

/// v42's gate for a pair: a stored kill's comparison, on every view a
/// comparison is opened from, whole and windowed, with and without an
/// ability, is the live `CompareSnapshot`'s.
#[test]
fn a_stored_kill_compares_as_the_live_one_does() {
    let mut mock = MockDaemon::fixture_at(Path::new(SAMPLE)).with_history();
    let (seg, fight_id) = kill(&mut mock);
    let guids: Vec<String> = stored_ask(&mut mock, &fight_id, View::Damage, "x", None, None, None)
        .card
        .players
        .iter()
        .filter(|p| !p.enemy)
        .map(|p| p.guid.clone())
        .collect();
    let (a, b) = (&guids[0], &guids[1]);
    let spell = live_drill(&mut mock, seg, View::Damage, a, None, None)
        .by_spell
        .first()
        .map(|r| r.key.clone())
        .expect("an ability of theirs");
    let mut compared = 0;
    for view in [
        View::Damage,
        View::Healing,
        View::Taken,
        View::Interrupts,
        View::CrowdControl,
        View::Dispels,
    ] {
        for range in [None, Some((5_500, 21_200))] {
            for spell in [None, Some(spell.as_str())] {
                let what = format!("{view:?} {range:?} {spell:?}");
                let live = mock
                    .handle(ClientMsg::Watch(Cursor::Compare {
                        segment: SegmentRef::Id(seg),
                        a: a.clone(),
                        b: b.clone(),
                        view,
                        range,
                        spell: spell.map(str::to_string),
                    }))
                    .into_iter()
                    .rev()
                    .find_map(|m| match m {
                        DaemonMsg::CompareSnapshot { a, b, range, .. } => Some((a, b, range)),
                        _ => None,
                    })
                    .expect("the live pair");
                let f = stored_ask(&mut mock, &fight_id, view, a, spell, range, Some(b));
                let pair = f.pair.expect("the stored pair");
                assert_eq!(pair.range, live.2, "{what}: the echo");
                for (s, l) in [(&pair.a, &*live.0), (&pair.b, &*live.1)] {
                    let what = format!("{what} {}", s.guid);
                    assert_eq!(s.guid, l.guid, "{what}");
                    assert_eq!(
                        words(std::slice::from_ref(&s.total)),
                        words(std::slice::from_ref(&l.total)),
                        "{what}: total"
                    );
                    assert_eq!(s.total.mine, l.total.mine, "{what}: whose");
                    assert_eq!(words(&s.spells), words(&l.spells), "{what}: tables");
                    assert_eq!(s.timeline, l.timeline, "{what}: curve");
                    assert_eq!(s.spell_timeline, l.spell_timeline, "{what}: ability");
                    assert_eq!(s.mitigation, l.mitigation, "{what}: record");
                }
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 24);
}

/// v42: a boss kill (and a timed key) is kept whole — no cap demotes or
/// evicts it, so its series stays; the caps count the rest.
#[test]
fn a_kill_is_kept_whole_and_the_caps_count_the_rest() {
    let cfg = Retention {
        keep_per_encounter: 0,
        keep_details_per_encounter: 0,
        details_min_wipe_secs: 30,
        ..Retention::default()
    };
    let (store, _, _) = stored(cfg.clone());
    let kill = id_of(&store, "The Ashen Warden");
    assert!(store.has_details(&kill) && store.has_series(&kill));
    assert!(
        store.card(&kill).is_some_and(|c| cfg.keeps_whole(c)),
        "a boss kill"
    );
    assert!(
        store
            .cards()
            .iter()
            .all(|c| c.id == kill || !cfg.keeps_whole(c)),
        "the wipe and the Σ are not"
    );
    assert!(
        !store.cards().iter().any(|c| c.name == "Verkath the Hollow"),
        "a cap of 0 takes every wipe"
    );
    let key = wowdps_proto::history::FightCard {
        kind: wowdps_proto::history::FightKind::Key,
        success: Some(true),
        ..wowdps_proto::history::FightCard::default()
    };
    assert!(cfg.keeps_whole(&key), "a timed key");
    let depleted = wowdps_proto::history::FightCard {
        success: Some(false),
        ..key.clone()
    };
    assert!(!cfg.keeps_whole(&depleted), "an over-time key is not");

    // `history_keep_kills_whole = false` turns it off (what the caps then do
    // is `tests/history.rs`'s `a_kill_is_kept_whole_unless_the_config_says_not`).
    let off = Retention {
        keep_whole: false,
        ..cfg
    };
    assert!(!off.keeps_whole(&key));
}

/// v42: the store names what it promised and lost — a kill whose details
/// an older build demoted, a series file in format 1 — and a rewrite from
/// the log makes it whole again.
#[test]
fn a_store_rewrites_what_it_promised_and_lost() {
    use wowdps_daemon::history::Backend;
    use wowdps_proto::series::{FightSeries, format_of};
    let (store, fights, _) = stored(Retention::default());
    let kill = id_of(&store, "The Ashen Warden");
    assert!(store.rewrites().is_empty(), "a fresh store is whole");
    assert_eq!(
        store.series_format(&kill),
        Some(wowdps_proto::series::FORMAT)
    );

    // An older build's files: the details demoted, the series in format 1.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "loadouts", "series"] {
        for name in store.backend().list(dir) {
            let bytes = store.backend().read(dir, &name).unwrap();
            let bytes = if dir == "series" {
                FightSeries::decode(&bytes).unwrap().encode_as(1)
            } else {
                bytes
            };
            backend.write(dir, &name, &bytes).unwrap();
        }
    }
    let mut old = Store::open(backend, Retention::default());
    assert!(!old.has_details(&kill));
    assert_eq!(old.series_format(&kill), Some(1));
    assert!(old.wants_rewrite(&kill), "a kill short of its details");
    assert_eq!(old.rewrites(), vec![kill.clone()]);
    // A format-1 file still answers what it holds: its windows.
    let facts = LogFacts::read(Path::new(SAMPLE));
    let fight = fights
        .iter()
        .find(|f| f.segment.name == "The Ashen Warden")
        .unwrap();
    assert_eq!(old.regrade(fight, facts), Some(kill.clone()));
    assert!(old.has_details(&kill));
    assert_eq!(old.series_format(&kill), Some(wowdps_proto::series::FORMAT));
    assert!(!old.wants_rewrite(&kill) && old.rewrites().is_empty());
    let head = old
        .backend()
        .read("series", &format!("{kill}.bin"))
        .unwrap();
    assert_eq!(format_of(&head), Some(2));
}

/// v42 over a real log (`WOWDPS_REAL_LOG`): every kill or key it holds
/// stored in a scratch directory; for every player, the stored drill's
/// stack, its largest abilities opened (curve, targets whole and windowed,
/// their stack) and the 1 s damage taken equal the live meter's — and the
/// series file's size in format 2 beside what format 1 kept, printed.
///
/// `WOWDPS_REAL_LOG=… cargo test --release -p wowdps-daemon --test series -- --ignored real_log_stored_kills --nocapture`
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_stored_kills_stack_and_open_as_the_live_meter() {
    use wowdps_daemon::history::{Ask, DirBackend};
    use wowdps_proto::series::FightSeries;
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let path = Path::new(&path);
    let tmp = TempDir::new("wowdps-series-v42");
    let dir = tmp.0.clone();
    let fights = closed_fights(path);
    let facts = LogFacts::read(path);
    let mut store = Store::open(DirBackend::new(dir.clone()), Retention::default());
    let mine = store.mine();
    let (mut checked, mut v2_bytes, mut v1_bytes) = (0usize, 0u64, 0u64);
    for fight in &fights {
        let Some(id) = store.store(fight, facts) else {
            continue;
        };
        if !store.has_series(&id) {
            continue;
        }
        let seg = &fight.segment;
        let bytes = std::fs::read(dir.join("series").join(format!("{id}.bin"))).unwrap();
        let v1 = FightSeries::decode(&bytes).unwrap().encode_as(1).len() as u64;
        v2_bytes += bytes.len() as u64;
        v1_bytes += v1;
        let card = store.card(&id).cloned().expect("the card");
        eprintln!(
            "{:>32} {:>5}s series {:>8} B (format 1: {:>8} B)",
            seg.name.chars().take(32).collect::<String>(),
            card.duration_ms / 1000,
            bytes.len(),
            v1
        );
        let secs = (card.duration_ms / 1000).max(2) as u32;
        let window = (secs / 3 * 1000, secs / 2 * 1000 + 1);
        for p in card.players.iter().filter(|p| !p.enemy) {
            let g = p.guid.as_str();
            let ask = |view, spell: Option<&str>, range| Ask {
                range,
                spell: spell.map(str::to_string),
                stacked: true,
                ..Ask::of(view, Some(g), None)
            };
            for view in [View::Damage, View::Healing] {
                let (rows, _) = seg.breakdown(g, view);
                let tree = seg.spell_tree(g, view);
                let f = store
                    .stored_fight_in(&mine, &id, &ask(view, None, None))
                    .unwrap();
                let b = f.breakdown.unwrap_or_default();
                assert_eq!(
                    b.ability_series,
                    seg.ability_series(g, view, &rows, &tree, 6),
                    "{} {view:?}: the stack",
                    p.name
                );
                for r in rows.iter().take(if view == View::Damage { 8 } else { 3 }) {
                    let what = format!("{} {view:?} {}", p.name, r.label);
                    let b = store
                        .stored_fight_in(&mine, &id, &ask(view, Some(&r.key), None))
                        .unwrap()
                        .breakdown
                        .unwrap();
                    assert_eq!(
                        b.spell_targets.as_deref().map(words),
                        Some(words(&seg.spell_targets(g, &r.key, view))),
                        "{what}: targets"
                    );
                    if view == View::Damage {
                        assert_eq!(
                            b.spell_timeline,
                            Some(seg.spell_timeline(g, &r.key)),
                            "{what}: curve"
                        );
                        assert_eq!(b.target_series, seg.target_series(g, &r.key, 6), "{what}");
                        let w = (i64::from(window.0), i64::from(window.1));
                        let b = store
                            .stored_fight_in(&mine, &id, &ask(view, Some(&r.key), Some(window)))
                            .unwrap()
                            .breakdown
                            .unwrap();
                        assert_eq!(
                            b.spell_targets.as_deref().map(words),
                            Some(words(&seg.spell_targets_ranged(g, &r.key, view, Some(w)))),
                            "{what}: a window's targets"
                        );
                    }
                    checked += 1;
                }
            }
            let taken = store
                .stored_fight_in(&mine, &id, &ask(View::Taken, None, None))
                .unwrap()
                .breakdown
                .and_then(|b| b.timeline);
            if let Some(t) = taken.filter(|t| t.bucket_ms == 1_000) {
                assert_eq!(t, seg.taken_timeline(g), "{}: the 1 s taken", p.name);
            }
        }
    }
    drop(tmp);
    eprintln!(
        "{checked} abilities matched; series {v2_bytes} B in format 2, {v1_bytes} B in format 1 \
         (+{:.0} %)",
        (v2_bytes as f64 / v1_bytes.max(1) as f64 - 1.0) * 100.0
    );
    assert!(checked > 0, "the log holds a kill or a key");
}

/// A scratch directory under the system's temp dir, removed when dropped —
/// a failing assertion included, so a real log's store (real names) is
/// never left behind.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A copy of `store`'s files, each series file through `series` first.
fn copied(store: &Store<MemBackend>, series: impl Fn(Vec<u8>) -> Vec<u8>) -> Store<MemBackend> {
    use wowdps_daemon::history::Backend;
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "details", "loadouts", "series"] {
        for name in store.backend().list(dir) {
            let bytes = store.backend().read(dir, &name).unwrap();
            let bytes = if dir == "series" {
                series(bytes)
            } else {
                bytes
            };
            backend.write(dir, &name, &bytes).unwrap();
        }
    }
    Store::open(backend, Retention::default())
}

/// v42: a format-1 file — a kill stored before v42 whose log is gone —
/// still answers its windows and stack, and an opened ability's curve, but
/// says it keeps no ability's targets (`abilities` false, no target list)
/// rather than answering an empty list that reads "hit nobody"; and a file
/// in a NEWER format than this build's is a later build's, left alone.
#[test]
fn a_format_one_file_opens_no_ability_and_a_newer_one_is_left_alone() {
    use wowdps_daemon::history::Ask;
    use wowdps_proto::series::{FORMAT, FightSeries};
    let (store, _, _) = stored(Retention::default());
    let kill = id_of(&store, "The Ashen Warden");
    let old = copied(&store, |b| FightSeries::decode(&b).unwrap().encode_as(1));
    assert!(old.has_series(&kill) && !old.has_abilities(&kill));
    assert!(store.has_abilities(&kill), "a v42 file does");
    let guid = old
        .card(&kill)
        .and_then(|c| c.players.iter().find(|p| p.damage > 0))
        .map(|p| p.guid.clone())
        .expect("a damage dealer");
    let key = old
        .details(&kill)
        .and_then(|d| d.players.into_iter().find(|p| p.guid == guid))
        .and_then(|p| p.damage_spells.first().map(|r| r.key.clone()))
        .expect("an ability");
    let ask = Ask {
        spell: Some(key),
        stacked: true,
        ..Ask::of(View::Damage, Some(&guid), None)
    };
    let f = old.stored_fight_in(&old.mine(), &kill, &ask).unwrap();
    assert!(f.series && !f.abilities);
    let b = f.breakdown.expect("the drill");
    assert!(
        b.spell_timeline.is_some(),
        "its curve: the ability's seconds are kept"
    );
    assert_eq!(
        b.spell_targets, None,
        "no target list, rather than an empty one"
    );
    assert!(b.target_series.is_empty());
    let whole = store.stored_fight_in(&store.mine(), &kill, &ask).unwrap();
    assert!(whole.abilities && whole.breakdown.unwrap().spell_targets.is_some());

    let newer = copied(&store, |mut b| {
        b[4] = FORMAT + 1;
        b
    });
    assert_eq!(newer.series_format(&kill), Some(FORMAT + 1));
    assert!(
        !newer.has_series(&kill),
        "a later build's file reads as absent"
    );
    let windowed = Ask {
        range: Some((5_500, 21_200)),
        ..Ask::of(View::Damage, Some(&guid), None)
    };
    let f = newer
        .stored_fight_in(&newer.mine(), &kill, &windowed)
        .unwrap();
    assert!(
        !f.series && !f.abilities,
        "no windows offered it cannot answer"
    );
    assert!(!newer.has_abilities(&kill), "this build cannot read it");
    assert!(
        !newer.wants_rewrite(&kill) && newer.rewrites().is_empty(),
        "a later build's file is never rewritten down"
    );
}

/// The (offset, length) of `guid`'s block in a series file, from its index.
fn block_of(bytes: &[u8], guid: &str) -> (usize, usize) {
    fn varint(b: &[u8], at: &mut usize) -> usize {
        let mut v = 0usize;
        let mut shift = 0;
        loop {
            let byte = b[*at];
            *at += 1;
            v |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }
    let head = wowdps_proto::series::HEAD_LEN;
    let index_len = u32::from_le_bytes(bytes[5..9].try_into().unwrap()) as usize;
    let mut at = head;
    let n = varint(bytes, &mut at);
    for _ in 0..n {
        let len = varint(bytes, &mut at);
        let g = std::str::from_utf8(&bytes[at..at + len])
            .unwrap()
            .to_string();
        at += len;
        let offset = varint(bytes, &mut at);
        let block = varint(bytes, &mut at);
        if g == guid {
            return (head + index_len + offset, block);
        }
    }
    panic!("{guid} has no block");
}

/// v42: a pair's window is echoed only where BOTH sides' seconds read —
/// one player's block unreadable, both sides answer whole and the pair
/// echoes no window, rather than pairing a whole side with a zoomed graph.
#[test]
fn a_pair_whose_side_cannot_read_its_seconds_echoes_no_window() {
    use wowdps_daemon::history::Ask;
    let (store, _, _) = stored(Retention::default());
    let kill = id_of(&store, "The Ashen Warden");
    let guids: Vec<String> = store
        .card(&kill)
        .unwrap()
        .players
        .iter()
        .filter(|p| !p.enemy && p.damage > 0)
        .map(|p| p.guid.clone())
        .collect();
    let (a, b) = (&guids[0], &guids[1]);
    let broken = copied(&store, |mut bytes| {
        let (at, len) = block_of(&bytes, b);
        // A row count no block that size could hold.
        bytes[at] = 0x7f;
        assert!(len > 1);
        bytes
    });
    assert!(broken.player_series(&kill, a).is_some());
    assert!(
        broken.player_series(&kill, b).is_none(),
        "b's block is unreadable"
    );
    let ask = Ask {
        range: Some((5_500, 21_200)),
        pair: Some(b.clone()),
        ..Ask::of(View::Damage, Some(a), None)
    };
    let pair = broken
        .stored_fight_in(&broken.mine(), &kill, &ask)
        .unwrap()
        .pair
        .expect("the pair");
    assert_eq!(pair.range, None, "no window echoed");
    let rows = broken.rows(&kill).unwrap();
    let row = |g: &str| {
        rows.rows(View::Damage)
            .iter()
            .find(|r| r.key == g)
            .map(|r| r.amount)
    };
    assert_eq!(Some(pair.a.total.amount), row(a), "a: whole, like b");
    assert_eq!(Some(pair.b.total.amount), row(b));
    // Readable, the same ask windows both and echoes it.
    let f = store.stored_fight_in(&store.mine(), &kill, &ask).unwrap();
    assert_eq!(f.pair.unwrap().range, Some((5_500, 21_200)));
}
