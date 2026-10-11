//! v46: fights imported from outside this machine's logs (`ImportReplay`):
//! each is stored as a card (`Origin::Imported`) and a replay tier and
//! nothing else, protected from every cap like a pin, listed only to a
//! client that asks for imports, and never read by the store's judgments of
//! this machine's history — the owner, the account's characters, a boss's
//! first kill, the fastest kill, progression, trend and the role roster.
//! Over the in-memory backend and the mock; the socket path is in
//! `tests/history.rs`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use wowdps_core::index::{self, load_segment_text};
use wowdps_core::meter::SegmentKind;
use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{Backend, ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_daemon::mock::MockDaemon;
use wowdps_daemon::replay::cut_text;
use wowdps_model::{Class, Encounter, Spec, View};
use wowdps_proto::history::{
    FightCard, FightKind, ImportedCard, ImportedPlayer, Origin, imported_id,
};
use wowdps_proto::{
    ClientMsg, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery, OriginFilter, TrendBucket,
    TrendMeasure, replay,
};

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("wowdps-import-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const ANA: &str = "Player-1-A";
/// The import's own player: nobody this machine's logs ever named.
const ZED: &str = "Player-9-Z";
const SOURCE: &str = "import:test-1";

/// One boss's pulls — (seconds, kill, difficulty) — one log, ten minutes
/// apart, each with a hit and a hostile cast (`tests/replay.rs`'s log).
fn boss_log(pulls: &[(u32, bool, u32)]) -> String {
    let mut out = String::from(
        "7/27/2026 20:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1\n\
         7/27/2026 20:00:01.000-4  ZONE_CHANGE,2769,\"Nerub-ar Palace\",15\n",
    );
    for (i, (dur_s, kill, difficulty)) in pulls.iter().enumerate() {
        let h = 20 + (i / 6) as u32;
        let m = ((i % 6) * 10) as u32;
        let ts = |sec: u32| format!("7/27/2026 {h:02}:{:02}:{:02}.000-4", m + sec / 60, sec % 60);
        out.push_str(&format!(
            "{}  ENCOUNTER_START,3130,\"The Ashen Warden\",{difficulty},20,2769\n",
            ts(0)
        ));
        out.push_str(&format!(
            "{}  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"The Ashen Warden\",0xa48,0x0,116,\"Frostbolt\",16,{},{},0,0,0,0,0,nil,nil\n",
            ts(1),
            1000 * (i as u64 + 1),
            1000 * (i as u64 + 1)
        ));
        out.push_str(&format!(
            "{}  SPELL_CAST_START,Creature-0-9,\"The Ashen Warden\",0xa48,0x0,0000000000000000,nil,0x80000000,0x80000000,7001,\"Spit\",0x8\n",
            ts(2)
        ));
        out.push_str(&format!(
            "{}  ENCOUNTER_END,3130,\"The Ashen Warden\",{difficulty},20,{},{}\n",
            ts(*dur_s),
            u8::from(*kill),
            dur_s * 1000
        ));
    }
    out
}

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

/// A log of `pulls`, written under `tmp`.
fn log_of(tmp: &Temp, pulls: &[(u32, bool, u32)]) -> PathBuf {
    let path = tmp.0.join("WoWCombatLog-boss.txt");
    std::fs::write(&path, boss_log(pulls)).unwrap();
    path
}

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
/// sample.txt's warrior.
const THRAXX: &str = "Player-1168-0A1B2C01";

/// The fixture's first boss pull cut as the store cuts one, its warrior the
/// "you": the replay tier's bytes an import hands the daemon (posted,
/// unlike the boss logs here, which carry no advanced block).
fn replay_bytes() -> Vec<u8> {
    let path = Path::new(SAMPLE);
    let mut file = std::fs::File::open(path).unwrap();
    let idx = index::scan(&mut file);
    let meta = idx
        .segments
        .iter()
        .find(|m| m.kind == SegmentKind::Encounter)
        .unwrap();
    let text = load_segment_text(path, meta).unwrap();
    let cut = cut_text(&text, Some(THRAXX));
    assert!(cut.posts() > 0 && cut.units.iter().any(|u| u.you));
    replay::encode(&cut)
}

/// An import's facts: the warden at `difficulty`, starting at `start`.
fn imported(start: i64, success: bool, difficulty: u32) -> ImportedCard {
    ImportedCard {
        name: "The Ashen Warden".to_string(),
        encounter: Some(Encounter {
            id: 3130,
            difficulty,
            group_size: 20,
        }),
        kind: FightKind::Encounter,
        start_utc_ms: start,
        duration_ms: 30_000,
        success: Some(success),
        best_pct: Some(if success { 0 } else { 40 }),
        players: vec![
            ImportedPlayer {
                guid: ANA.to_string(),
                name: "Ana-Realm".to_string(),
                class: Some(Class::Mage),
                spec: Some(Spec::FrostMage),
            },
            ImportedPlayer {
                guid: ZED.to_string(),
                name: "Zed-Elsewhere".to_string(),
                class: Some(Class::Priest),
                spec: Some(Spec::Discipline),
            },
        ],
        owner: Some(ZED.to_string()),
        source: SOURCE.to_string(),
        ..ImportedCard::default()
    }
}

fn mem(cfg: Retention) -> Store<MemBackend> {
    Store::open(MemBackend::new(), cfg)
}

/// The store's files, read whole: a store opened over them is the same
/// store.
fn copied(from: &MemBackend) -> MemBackend {
    let mut to = MemBackend::new();
    for dir in ["fights", "rows", "details", "series", "replay", "loadouts"] {
        for name in from.list(dir) {
            to.write(dir, &name, &from.read(dir, &name).unwrap())
                .unwrap();
        }
    }
    to
}

fn fights(store: &Store<MemBackend>, origin: OriginFilter, sort: FightSort) -> Vec<FightCard> {
    match store.answer(&HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: None,
        since_utc_ms: None,
        kind: None,
        sort,
        limit: 0,
        after_id: None,
        role: None,
        origin,
    }) {
        HistoryAnswer::Fights { cards, .. } => cards,
        other => panic!("{other:?}"),
    }
}

// ---- the stored fight ---------------------------------------------------------

/// An import is its card and its replay: the id from the source, no log,
/// no rows, details or series, the replay decoding to the cut it was made
/// from with the import's owner as its "you" — and it reads back after a
/// restart, origin and all.
#[test]
fn an_import_is_a_card_and_a_replay_and_nothing_else() {
    let bytes = replay_bytes();
    let mut store = mem(Retention::default());
    let (id, replaced) = store
        .import_replay(&imported(1_000_000, true, 15), &bytes)
        .unwrap();
    assert!(!replaced);
    assert_eq!(id, imported_id(SOURCE, 1_000_000));

    let card = store.card(&id).unwrap().clone();
    assert!(card.is_imported());
    assert_eq!(card.origin.source(), Some(SOURCE));
    assert_eq!((card.log, card.byte_range, card.aborted), (0, None, false));
    assert_eq!(card.owner.as_deref(), Some(ZED));
    let file = String::from_utf8(
        store
            .backend()
            .read("fights", &format!("{id}.json"))
            .unwrap(),
    )
    .unwrap();
    assert!(
        file.ends_with(r#","origin":{"kind":"imported","source":"import:test-1"}}"#),
        "{file}"
    );
    for dir in ["rows", "details"] {
        assert!(!store.backend().exists(dir, &format!("{id}.json")), "{dir}");
    }
    assert!(!store.has_series(&id));

    // The replay: the cut it was made from, its "you" the import's owner
    // (the import's ZED is in no cut of the fixture, so nobody is "you").
    let original = replay::decode(&bytes).unwrap();
    assert!(original.units.iter().any(|u| u.you), "cut with a you");
    let kept = replay::decode(&store.replay_file(&id).unwrap()).unwrap();
    assert_eq!(kept.units.len(), original.units.len());
    assert_eq!(kept.posts(), original.posts());
    assert_eq!(kept.events.len(), original.events.len());
    assert!(kept.units.iter().all(|u| !u.you), "you is the card's owner");

    // A stored fight answers the card alone: tier 1, no rows, no drill.
    let f = store
        .stored_fight(&id, View::Damage, Some(ANA), None)
        .unwrap();
    assert_eq!(f.tier, 1);
    assert!(f.rows.is_empty() && f.breakdown.is_none() && !f.series);
    assert_eq!(f.card.origin, card.origin);

    let st = store.status();
    assert_eq!((st.fights, st.replays), (1, 1));
    assert_eq!(st.kept_imports, 1);
    assert_eq!(
        (
            st.kept_kills,
            st.kept_keys,
            st.kept_progression,
            st.kept_pins
        ),
        (0, 0, 0, 0),
        "an imported kill is none of ours"
    );
    // No log to rewrite or recut it from: the queue never names it.
    assert!(!store.wants_rewrite(&id) && !store.wants_recut(&id));
    assert!(store.rewrites().is_empty() && store.recuts().is_empty());

    let reopened = Store::open(copied(store.backend()), Retention::default());
    assert_eq!(reopened.card(&id), Some(&card));
    assert!(reopened.has_replay(&id));
    assert_eq!(reopened.status().kept_imports, 1);
}

/// The owner the import names is the replay's "you" when the cut holds
/// them.
#[test]
fn the_imports_owner_is_its_replays_you() {
    let mut store = mem(Retention::default());
    let card = ImportedCard {
        owner: Some(THRAXX.to_string()),
        ..imported(1_000_000, true, 15)
    };
    let (id, _) = store.import_replay(&card, &replay_bytes()).unwrap();
    let kept = replay::decode(&store.replay_file(&id).unwrap()).unwrap();
    let you: Vec<&str> = kept
        .units
        .iter()
        .filter(|u| u.you)
        .map(|u| u.guid.as_str())
        .collect();
    assert_eq!(you, [THRAXX]);
}

/// Protected like a pin: caps of nothing and a one-byte replay cap take
/// every local wipe and its replay, never the import.
#[test]
fn an_import_is_never_trimmed() {
    let tmp = Temp::new("trim");
    let path = log_of(&tmp, &[(30, false, 15), (30, false, 15), (30, false, 15)]);
    let facts = LogFacts::read(&path);
    let cfg = Retention {
        keep_per_encounter: 0,
        keep_details_per_encounter: 0,
        keep_progression: false,
        replay_bytes: 1,
        ..Retention::default()
    };
    let mut store = mem(cfg);
    let (id, _) = store
        .import_replay(&imported(1_000_000, false, 15), &replay_bytes())
        .unwrap();
    for f in closed_fights(&path) {
        store.store(&f, facts);
    }
    assert_eq!(
        store
            .cards()
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        [id.as_str()],
        "every local wipe answered to the caps; the import stayed"
    );
    assert!(store.has_replay(&id));
    assert_eq!(store.status().replays, 1);
}

/// An import is somebody's fight: listed only to a client that asks for
/// imports, it ends no progression of ours, counts in no progression,
/// trend or role roster, and its owner is none of the account's
/// characters.
#[test]
fn an_import_is_not_this_machines_history() {
    let tmp = Temp::new("history");
    let path = log_of(&tmp, &[(30, false, 15), (40, false, 15)]);
    let facts = LogFacts::read(&path);
    let mut store = mem(Retention::default());
    let local: Vec<String> = closed_fights(&path)
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    assert_eq!(local.len(), 2);
    let first = store.card(&local[0]).unwrap().clone();
    // A KILL of the same boss at the same difficulty, the same night.
    let (id, _) = store
        .import_replay(
            &imported(first.start_utc_ms + 1_000, true, 15),
            &replay_bytes(),
        )
        .unwrap();

    // Both wipes are still progression: the imported kill ends nothing.
    assert!(
        local
            .iter()
            .all(|w| store.is_progression(store.card(w).unwrap()))
    );
    assert_eq!(store.kept_counts(), (0, 0, 2, 0, 1));

    // Listed only on request.
    let ids = |cards: Vec<FightCard>| -> Vec<String> { cards.into_iter().map(|c| c.id).collect() };
    let newest = ids(fights(&store, OriginFilter::Local, FightSort::Newest));
    assert_eq!(newest.len(), 2);
    assert!(!newest.contains(&id));
    assert_eq!(
        ids(fights(&store, OriginFilter::Imported, FightSort::Newest)),
        std::slice::from_ref(&id)
    );
    assert_eq!(
        fights(&store, OriginFilter::Any, FightSort::Newest).len(),
        3
    );
    assert!(
        fights(&store, OriginFilter::Local, FightSort::Fastest).is_empty(),
        "no kill of ours"
    );
    assert_eq!(
        ids(fights(&store, OriginFilter::Any, FightSort::Fastest)),
        std::slice::from_ref(&id)
    );

    // Progression, trend and the night's roster are this machine's.
    let HistoryAnswer::Progression {
        pulls,
        kills,
        first_kill,
        nights,
        ..
    } = store.answer(&HistoryQuery::Progression {
        encounter: 3130,
        difficulty: 15,
        local_cutover_hour: None,
    })
    else {
        panic!("progression");
    };
    assert_eq!((pulls, kills, first_kill), (2, 0, None));
    let HistoryAnswer::Trend(points) = store.answer(&HistoryQuery::Trend {
        guid: ANA.to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure: TrendMeasure::Dps,
        bucket: TrendBucket::None,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
    }) else {
        panic!("trend");
    };
    assert_eq!(points.len(), 2);
    assert!(points.iter().all(|p| p.fight_id != id));
    let HistoryAnswer::RoleNight { night, rows } = store.answer(&HistoryQuery::RoleNight {
        encounter: 3130,
        difficulty: 15,
        night: nights[0].day_utc_ms,
        local_cutover_hour: None,
    }) else {
        panic!("role night");
    };
    assert_eq!((night.pulls, night.kills), (2, 0));
    assert!(rows.iter().all(|r| r.guid != ZED), "{rows:?}");

    // The import's owner (its "you") is not one of ours.
    let mine = store.mine();
    assert!(!mine.owns(ZED, "Zed-Elsewhere"));
}

/// An imported kill races for no protection: with kills not kept whole and
/// caps of nothing, this machine's own kill stays as the boss's fastest
/// though the import was faster.
#[test]
fn an_imported_kill_takes_no_fastest_kill() {
    let tmp = Temp::new("fastest");
    let path = log_of(&tmp, &[(90, true, 15)]);
    let facts = LogFacts::read(&path);
    let cfg = Retention {
        keep_per_encounter: 0,
        keep_whole: false,
        keep_progression: false,
        ..Retention::default()
    };
    let mut store = mem(cfg);
    let mut fast = imported(1_000_000, true, 15);
    fast.duration_ms = 10_000;
    store.import_replay(&fast, &replay_bytes()).unwrap();
    let local = store.store(&closed_fights(&path)[0], facts).unwrap();
    assert!(
        store.card(&local).is_some(),
        "our kill is still our fastest"
    );
    assert_eq!(store.cards().len(), 2);
}

/// The same source again replaces the import — the same id, the pin
/// carried, the answer saying so — and an import whose start moved since
/// takes the old one's place under its new id.
#[test]
fn the_same_source_again_replaces_it() {
    let bytes = replay_bytes();
    let mut store = mem(Retention::default());
    let (id, replaced) = store
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    assert!(!replaced);
    assert!(store.pin(&id, true));
    let (again, replaced) = store
        .import_replay(&imported(1_000_000, true, 15), &bytes)
        .unwrap();
    assert_eq!((again.as_str(), replaced), (id.as_str(), true));
    assert_eq!(store.cards().len(), 1);
    let card = store.card(&id).unwrap();
    assert!(card.pinned, "the pin is the user's");
    assert_eq!(card.success, Some(true), "the new facts");

    let (moved, replaced) = store
        .import_replay(&imported(1_005_000, true, 15), &bytes)
        .unwrap();
    assert!(replaced);
    assert_ne!(moved, id);
    assert_eq!(store.cards().len(), 1);
    assert!(store.card(&id).is_none() && !store.has_replay(&id));
    assert!(!store.backend().exists("fights", &format!("{id}.json")));
    assert!(!store.backend().exists("replay", &format!("{id}.bin")));
    assert!(store.card(&moved).unwrap().pinned);
    assert!(store.has_replay(&moved));
}

/// A replay that is not one, a format not this build's (older or newer), one that
/// does not decode, a nameless source, anything but a boss pull, and an
/// id a local card holds: each refused, saying why, nothing written.
#[test]
fn a_bad_import_is_refused_and_writes_nothing() {
    let good = replay_bytes();
    let mut store = mem(Retention::default());
    let card = imported(1_000_000, false, 15);
    let mut newer = good.clone();
    newer[4] = replay::FORMAT + 1;
    // An older format decodes, but its number names cut rules this build's
    // cut no longer follows, and an import is never recut: refused.
    let mut older = good.clone();
    older[4] = replay::FORMAT - 1;
    assert!(replay::decode(&older).is_some(), "the older head decodes");
    let err = store.import_replay(&card, &older).unwrap_err();
    assert!(
        err.contains(&format!(
            "replay format {} is not this build's format {}",
            replay::FORMAT - 1,
            replay::FORMAT
        )),
        "{err}"
    );
    let truncated = good[..good.len() / 2].to_vec();
    for (bytes, why) in [
        (b"not a replay".to_vec(), "replay head"),
        (newer, "is not this build's format"),
        (older, "is not this build's format"),
        (truncated, "does not decode"),
    ] {
        let err = store.import_replay(&card, &bytes).unwrap_err();
        assert!(err.contains(why), "{err}");
    }
    for (bad, why) in [
        (
            ImportedCard {
                source: " ".to_string(),
                ..card.clone()
            },
            "source",
        ),
        (
            ImportedCard {
                kind: FightKind::Key,
                ..card.clone()
            },
            "only boss pulls",
        ),
        (
            ImportedCard {
                encounter: None,
                ..card.clone()
            },
            "encounter",
        ),
    ] {
        let err = store.import_replay(&bad, &good).unwrap_err();
        assert!(err.contains(why), "{err}");
    }
    assert!(store.cards().is_empty());
    assert_eq!(store.status().replays, 0);

    // A local card under the import's id (an fnv64 collision, however
    // unlikely) is never overwritten.
    let id = imported_id(SOURCE, 1_000_000);
    let local = FightCard {
        id: id.clone(),
        origin: Origin::Local,
        ..FightCard::default()
    };
    let mut backend = MemBackend::new();
    backend
        .write(
            "fights",
            &format!("{id}.json"),
            local.to_json().to_line().as_bytes(),
        )
        .unwrap();
    let mut store = Store::open(backend, Retention::default());
    let err = store.import_replay(&card, &good).unwrap_err();
    assert!(err.contains("local fight"), "{err}");
    assert!(!store.card(&id).unwrap().is_imported());
    assert!(!store.has_replay(&id));
}

// ---- the mock -----------------------------------------------------------------

/// The mock answers `ImportReplay` as the history thread does: the stored
/// id, then `HistoryChanged`; the replay answers `GetReplay`, the card
/// `GetFight` (tier 1), and the import is listed only on request; a bad
/// replay is `Refused`.
#[test]
fn the_mock_answers_an_import_as_the_thread_does() {
    let bytes = replay_bytes();
    let mut mock = MockDaemon::fixture().with_history();
    let before = mock.history().cards().len();
    let out = mock.handle(ClientMsg::ImportReplay {
        req_id: 7,
        card: Box::new(imported(1_000_000, true, 15)),
        replay: bytes.clone(),
    });
    let id = imported_id(SOURCE, 1_000_000);
    assert_eq!(
        out,
        [
            DaemonMsg::History {
                req_id: 7,
                answer: HistoryAnswer::ImportedReplay {
                    fight_id: id.clone(),
                    replaced: false,
                },
            },
            DaemonMsg::HistoryChanged {
                fight_id: id.clone()
            },
        ]
    );
    let out = mock.handle(ClientMsg::GetReplay {
        req_id: 8,
        fight_id: id.clone(),
    });
    let Some(DaemonMsg::Replay {
        bytes: Some(kept), ..
    }) = out.first()
    else {
        panic!("{out:?}");
    };
    assert_eq!(
        replay::decode(kept).unwrap().posts(),
        replay::decode(&bytes).unwrap().posts()
    );
    let out = mock.handle(ClientMsg::GetFight {
        req_id: 9,
        fight_id: id.clone(),
        view: View::Damage,
        drill: None,
        death: None,
        boss: None,
        range: None,
        spell: None,
        pair: None,
    });
    let Some(DaemonMsg::Fight {
        fight: Some(fight), ..
    }) = out.first()
    else {
        panic!("{out:?}");
    };
    assert_eq!(fight.tier, 1);
    assert!(fight.card.is_imported());
    let listed = |mock: &mut MockDaemon, origin| {
        let out = mock.handle(ClientMsg::GetHistory {
            req_id: 10,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                guid: None,
                since_utc_ms: None,
                kind: None,
                sort: FightSort::Newest,
                limit: 0,
                after_id: None,
                role: None,
                origin,
            },
        });
        match out.first() {
            Some(DaemonMsg::History {
                answer: HistoryAnswer::Fights { cards, .. },
                ..
            }) => cards.len(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(listed(&mut mock, OriginFilter::Local), before);
    assert_eq!(listed(&mut mock, OriginFilter::Imported), 1);
    assert_eq!(listed(&mut mock, OriginFilter::Any), before + 1);
    let out = mock.handle(ClientMsg::ImportReplay {
        req_id: 11,
        card: Box::new(imported(1_000_000, true, 15)),
        replay: b"WDRP".to_vec(),
    });
    assert!(
        matches!(
            out.as_slice(),
            [DaemonMsg::History {
                req_id: 11,
                answer: HistoryAnswer::Refused { .. },
            }]
        ),
        "{out:?}"
    );
}

/// A write that fails refuses the import and leaves the store as it was:
/// nothing indexed that is not on disk, and a fight imported before kept
/// whole.
#[test]
fn a_failed_write_leaves_the_store_as_it_was() {
    let bytes = replay_bytes();
    let mut backend = MemBackend::new();
    backend.fail_writes = true;
    let mut store = Store::open(backend, Retention::default());
    let err = store
        .import_replay(&imported(1_000_000, true, 15), &bytes)
        .unwrap_err();
    assert!(err.contains("write failed"), "{err}");
    assert!(store.cards().is_empty());
    assert!(store.backend().is_empty());

    let mut first = mem(Retention::default());
    let (id, _) = first
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    let mut backend = copied(first.backend());
    backend.fail_writes = true;
    let mut store = Store::open(backend, Retention::default());
    let err = store
        .import_replay(&imported(1_000_000, true, 15), &bytes)
        .unwrap_err();
    assert!(err.contains("write failed"), "{err}");
    assert_eq!(store.card(&id), first.card(&id), "the first import stands");
    assert!(store.has_replay(&id));
}

/// A backend over memory whose writes, reads or removals in one directory
/// can be made to fail on their own — the paths where a replay or an
/// annotation has already moved when the card's file does not.
struct Flaky {
    inner: MemBackend,
    fail_writes_in: std::cell::Cell<Option<&'static str>>,
    fail_reads_in: std::cell::Cell<Option<&'static str>>,
    fail_removes_in: std::cell::Cell<Option<&'static str>>,
}

impl Flaky {
    fn over(inner: MemBackend) -> Self {
        Self {
            inner,
            fail_writes_in: std::cell::Cell::new(None),
            fail_reads_in: std::cell::Cell::new(None),
            fail_removes_in: std::cell::Cell::new(None),
        }
    }
}

impl Backend for Flaky {
    fn list(&self, dir: &str) -> Vec<String> {
        self.inner.list(dir)
    }
    fn read(&self, dir: &str, name: &str) -> Option<Vec<u8>> {
        if self.fail_reads_in.get() == Some(dir) {
            return None;
        }
        self.inner.read(dir, name)
    }
    fn read_range(&self, dir: &str, name: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        self.inner.read_range(dir, name, offset, len)
    }
    fn size(&self, dir: &str, name: &str) -> Option<u64> {
        self.inner.size(dir, name)
    }
    fn exists(&self, dir: &str, name: &str) -> bool {
        self.inner.exists(dir, name)
    }
    fn write(&mut self, dir: &str, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        if self.fail_writes_in.get() == Some(dir) {
            return Err(std::io::Error::other(format!("{dir} writes fail")));
        }
        self.inner.write(dir, name, bytes)
    }
    fn remove(&mut self, dir: &str, name: &str) -> std::io::Result<()> {
        if self.fail_removes_in.get() == Some(dir) {
            return Err(std::io::Error::other(format!("{dir} removals fail")));
        }
        self.inner.remove(dir, name)
    }
}

/// A re-import writes its replay before its card: when only the card's
/// write fails, the fight imported before stands over its OWN replay —
/// the bytes and the index entry put back — not the new import's.
#[test]
fn a_failed_card_write_on_a_reimport_puts_the_old_replay_back() {
    let bytes = replay_bytes();
    let mut store = Store::open(Flaky::over(MemBackend::new()), Retention::default());
    let (id, _) = store
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    let card_before = store.card(&id).unwrap().clone();
    let replay_before = store.replay_file(&id).unwrap();
    let total_before = store.replay_total();
    // The new import's replay differs from the old: an event fewer.
    let mut cut = replay::decode(&bytes).unwrap();
    let events = cut.events.len();
    cut.events.pop();
    let other = replay::encode(&cut);

    store.backend().fail_writes_in.set(Some("fights"));
    let err = store
        .import_replay(&imported(1_000_000, true, 15), &other)
        .unwrap_err();
    assert!(err.contains("fights writes fail"), "{err}");
    assert_eq!(store.card(&id), Some(&card_before), "the old card stands");
    assert_eq!(
        store.replay_file(&id).as_deref(),
        Some(replay_before.as_slice()),
        "over its own replay"
    );
    let kept = replay::decode(&store.replay_file(&id).unwrap()).unwrap();
    assert_eq!(kept.events.len(), events, "not the new import's");
    assert_eq!(store.replay_total(), total_before);

    // And a store opened over the files agrees.
    store.backend().fail_writes_in.set(None);
    let reopened = Store::open(copied(&store.backend().inner), Retention::default());
    assert_eq!(reopened.card(&id), Some(&card_before));
    assert_eq!(
        reopened.replay_file(&id).as_deref(),
        Some(replay_before.as_slice())
    );
}

/// An import that names no owner stays ownerless in a `Fights` answer,
/// though the configured character is on its roster: the replay's "you"
/// and `GetFight` say nobody, and so does the list. A local card is still
/// owned at answer time.
#[test]
fn an_ownerless_import_is_nobodys_in_a_listing() {
    let tmp = Temp::new("ownerless");
    let path = log_of(&tmp, &[(30, false, 15)]);
    let facts = LogFacts::read(&path);
    let cfg = Retention {
        characters: vec!["Ana-Realm".to_string()],
        ..Retention::default()
    };
    let mut store = mem(cfg);
    let local = store.store(&closed_fights(&path)[0], facts).unwrap();
    let card = ImportedCard {
        owner: None,
        ..imported(1_000_000, false, 15)
    };
    let (id, _) = store.import_replay(&card, &replay_bytes()).unwrap();
    let listed = fights(&store, OriginFilter::Any, FightSort::Newest);
    let owner_of = |want: &str| {
        listed
            .iter()
            .find(|c| c.id == want)
            .map(|c| c.owner.clone())
            .unwrap()
    };
    assert_eq!(owner_of(&id), None, "the import names nobody");
    assert_eq!(owner_of(&local).as_deref(), Some(ANA), "ours is ours");
    let f = store.stored_fight(&id, View::Damage, None, None).unwrap();
    assert_eq!(f.card.owner, None);
}

/// An import whose start moved goes out under its old id: its annotations
/// carried to the new id (appended to any there), its old files gone.
#[test]
fn a_moved_import_carries_its_annotations() {
    let bytes = replay_bytes();
    let mut store = mem(Retention::default());
    let (old, _) = store
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    let mut backend = copied(store.backend());
    backend
        .write("annotations", &format!("{old}.ndjson"), b"{\"note\":1}\n")
        .unwrap();
    let mut store = Store::open(backend, Retention::default());
    let (new, replaced) = store
        .import_replay(&imported(1_005_000, false, 15), &bytes)
        .unwrap();
    assert!(replaced && new != old);
    let b = store.backend();
    assert_eq!(
        b.read("annotations", &format!("{new}.ndjson")).as_deref(),
        Some(&b"{\"note\":1}\n"[..])
    );
    assert!(!b.exists("annotations", &format!("{old}.ndjson")));
    assert!(!b.exists("fights", &format!("{old}.json")));
    assert!(store.card(&old).is_none());
}

/// A moved import's old card that cannot be removed stays whole — card
/// and replay, in the index as on disk, so a restart shows nothing it did
/// not — and the store says why; the next import of the source retires it.
#[test]
fn a_moved_import_whose_old_card_stays_says_so() {
    let bytes = replay_bytes();
    let mut store = Store::open(Flaky::over(MemBackend::new()), Retention::default());
    let (old, _) = store
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    store.backend().fail_removes_in.set(Some("fights"));
    let (new, _) = store
        .import_replay(&imported(1_005_000, false, 15), &bytes)
        .unwrap();
    assert!(store.card(&old).is_some() && store.card(&new).is_some());
    assert!(store.has_replay(&old), "kept whole, its replay too");
    let said = store.last_error.clone().unwrap_or_default();
    assert!(
        said.contains(&old) && said.contains("kept beside"),
        "{said}"
    );
    let reopened = Store::open(copied(&store.backend().inner), Retention::default());
    assert_eq!(
        reopened.cards().len(),
        2,
        "the files hold what the index did"
    );
    // The next import of the source retires both earlier ones.
    store.backend().fail_removes_in.set(None);
    let (last, _) = store
        .import_replay(&imported(1_010_000, false, 15), &bytes)
        .unwrap();
    assert_eq!(
        store
            .cards()
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        [last.as_str()]
    );
}

/// A moved import whose annotation file is there but cannot be read is
/// kept whole: its annotations stay reachable under it.
#[test]
fn a_moved_import_whose_annotations_cannot_be_read_keeps_its_card() {
    let bytes = replay_bytes();
    let mut first = mem(Retention::default());
    let (old, _) = first
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    let mut backend = copied(first.backend());
    backend
        .write("annotations", &format!("{old}.ndjson"), b"{\"note\":1}\n")
        .unwrap();
    let mut store = Store::open(Flaky::over(backend), Retention::default());
    store.backend().fail_reads_in.set(Some("annotations"));
    let (new, _) = store
        .import_replay(&imported(1_005_000, false, 15), &bytes)
        .unwrap();
    assert!(store.card(&old).is_some() && store.card(&new).is_some());
    assert!(
        store
            .backend()
            .inner
            .exists("annotations", &format!("{old}.ndjson"))
    );
    let said = store.last_error.clone().unwrap_or_default();
    assert!(
        said.contains("annotations") && said.contains(&old),
        "{said}"
    );
}

/// A re-import whose old replay is there but cannot be read could not put
/// it back if the card's write failed: refused before anything moves.
#[test]
fn a_reimport_over_an_unreadable_replay_is_refused() {
    let bytes = replay_bytes();
    let mut store = Store::open(Flaky::over(MemBackend::new()), Retention::default());
    let (id, _) = store
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    let card = store.card(&id).unwrap().clone();
    let file = store
        .backend()
        .inner
        .read("replay", &format!("{id}.bin"))
        .unwrap();
    store.backend().fail_reads_in.set(Some("replay"));
    let err = store
        .import_replay(&imported(1_000_000, true, 15), &bytes)
        .unwrap_err();
    assert!(err.contains("cannot be read"), "{err}");
    assert_eq!(store.card(&id), Some(&card));
    assert_eq!(
        store.backend().inner.read("replay", &format!("{id}.bin")),
        Some(file)
    );
}

/// An import's replay takes the size cap's room only once its card is in:
/// a failed card write leaves this machine's unprotected replay that the
/// trim would have taken.
#[test]
fn a_failed_import_trims_no_other_replay() {
    let tmp = Temp::new("trim-later");
    let path = log_of(&tmp, &[(30, false, 15)]);
    let facts = LogFacts::read(&path);
    let mut file = std::fs::File::open(&path).unwrap();
    let idx = index::scan(&mut file);
    let text = load_segment_text(&path, &idx.segments[0]).unwrap();
    let local_cut = cut_text(&text, None);
    // A cap that holds the local wipe's replay and nothing more.
    let cfg = Retention {
        keep_progression: false,
        replay_bytes: replay::encode(&local_cut).len() as u64,
        ..Retention::default()
    };
    let mut store = Store::open(Flaky::over(MemBackend::new()), cfg);
    let local = store.store(&closed_fights(&path)[0], facts).unwrap();
    assert!(store.store_replay(&local, local_cut));
    assert!(store.has_replay(&local), "the cap holds it");

    store.backend().fail_writes_in.set(Some("fights"));
    assert!(
        store
            .import_replay(&imported(1_000_000, false, 15), &replay_bytes())
            .is_err()
    );
    assert!(
        store.has_replay(&local),
        "nothing trimmed for an import that failed"
    );
    assert_eq!(store.status().replays, 1);

    // The same import landing does take the room: the import is protected,
    // the local wipe is not.
    store.backend().fail_writes_in.set(None);
    let (id, _) = store
        .import_replay(&imported(1_000_000, false, 15), &replay_bytes())
        .unwrap();
    assert!(store.has_replay(&id) && !store.has_replay(&local));
}

/// A moved import whose annotations cannot be carried is kept whole beside
/// the new one, reachable with them, and the store says so.
#[test]
fn a_moved_import_whose_annotations_stay_keeps_its_card() {
    let bytes = replay_bytes();
    let mut first = mem(Retention::default());
    let (old, _) = first
        .import_replay(&imported(1_000_000, false, 15), &bytes)
        .unwrap();
    let mut backend = copied(first.backend());
    backend
        .write("annotations", &format!("{old}.ndjson"), b"{\"note\":1}\n")
        .unwrap();
    let mut store = Store::open(Flaky::over(backend), Retention::default());
    store.backend().fail_writes_in.set(Some("annotations"));
    let (new, _) = store
        .import_replay(&imported(1_005_000, false, 15), &bytes)
        .unwrap();
    assert!(store.card(&old).is_some() && store.card(&new).is_some());
    assert!(store.has_replay(&old) && store.has_replay(&new));
    assert!(
        store
            .backend()
            .inner
            .exists("annotations", &format!("{old}.ndjson"))
    );
    let said = store.last_error.clone().unwrap_or_default();
    assert!(said.contains(&old) && said.contains("kept"), "{said}");
}
