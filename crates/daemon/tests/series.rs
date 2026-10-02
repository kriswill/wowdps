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
use wowdps_proto::{Breakdown, ClientMsg, Cursor, DaemonMsg, FightSort, HistoryAnswer};
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
    assert!(
        !store.wants_series_backfill(&wipe),
        "unpinned, it wants none"
    );
    assert!(store.pin(&wipe, true));
    assert!(
        store.wants_series_backfill(&wipe),
        "pinned, it wants a rewrite"
    );
    let facts = LogFacts::read(Path::new(SAMPLE));
    let fight = fights
        .iter()
        .find(|f| f.segment.name == "Verkath the Hollow")
        .expect("the wipe");
    assert_eq!(store.regrade(fight, facts), Some(wipe.clone()));
    assert!(store.has_series(&wipe), "the rewrite wrote it");
    assert!(!store.wants_series_backfill(&wipe));
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
    let dir = std::env::temp_dir().join(format!("wowdps-series-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!(
        "{checked} windows matched; series {series_bytes} B beside details {details_bytes} B"
    );
    assert!(checked > 0, "the log holds a kill or a key");
}
