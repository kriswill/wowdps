//! The history store (spec §12, "Store"): what the fixtures leave behind,
//! idempotency, retention, the protected set, aborted records, corrupt
//! files, unwritable directories, and the "no traffic before CaughtUp"
//! rule — mostly over the in-memory backend driven straight from the
//! engine, plus real daemons on temp sockets for the import path.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::tail::{SourceSpec, TailEvent};
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{
    Backend, ClosedFight, HistoryLink, HistoryOptions, HistoryReq, LogFacts, MemBackend, Retention,
    Store,
};
use wowdps_daemon::{DaemonOptions, run};
use wowdps_proto::history::{FightCard, FightKind};
use wowdps_proto::{ClientKind, ClientMsg, DaemonClient, DaemonMsg};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const INSTANCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/instance.txt");
const ARENA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/arena.txt");
const DEADLINE: Duration = Duration::from_secs(15);

// ---- scaffolding ------------------------------------------------------------

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("wowdps-hist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Replay a whole log through an engine the way the tail thread would
/// (no index: every segment is live), collecting every `Closed` fight.
fn closed_fights(path: &Path) -> Vec<ClosedFight> {
    closed_fights_from(path, &std::fs::read_to_string(path).unwrap())
}

fn closed_fights_from(path: &Path, text: &str) -> Vec<ClosedFight> {
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.to_path_buf()), &mut events);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    engine.on_tail(TailEvent::Lines(lines), &mut events);
    assert!(events.is_empty(), "no history traffic before CaughtUp");
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Closed(id) => engine.take_closed(*id),
            EngineEvent::Opened(_) => None,
        })
        .collect()
}

fn store_all(store: &mut Store<MemBackend>, path: &Path, fights: &[ClosedFight]) -> Vec<String> {
    let facts = LogFacts::read(path);
    fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect()
}

fn mem(cfg: Retention) -> Store<MemBackend> {
    Store::open(MemBackend::new(), cfg)
}

fn names(store: &Store<MemBackend>, dir: &str) -> Vec<String> {
    store.backend().list(dir)
}

// ---- the fixtures --------------------------------------------------------------

#[test]
fn sample_closes_two_encounters_and_the_raid_overall() {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let mut store = mem(Retention::default());
    let ids = store_all(&mut store, path, &fights);
    // The fixture's raid visit is still open at EOF, so there is no Σ yet.
    assert_eq!(ids.len(), 2, "2 bosses, no trash, no Σ: {ids:?}");
    assert_eq!(names(&store, "fights").len(), 2);
    assert_eq!(names(&store, "rows").len(), 2);
    assert_eq!(
        names(&store, "details").len(),
        1,
        "details for the kill only"
    );
    assert_eq!(
        names(&store, "loadouts").len(),
        3,
        "three COMBATANT_INFO builds, content-addressed"
    );

    let cards = store.cards();
    assert!(
        cards
            .windows(2)
            .all(|w| w[0].start_utc_ms <= w[1].start_utc_ms)
    );
    let kill = cards
        .iter()
        .find(|c| c.name == "The Ashen Warden")
        .expect("the kill is stored");
    assert_eq!(kill.kind, FightKind::Encounter);
    assert_eq!(kill.success, Some(true));
    assert!(!kill.aborted);
    let enc = kill.encounter.unwrap();
    assert_eq!((enc.id, enc.difficulty, enc.group_size), (3130, 15, 3));
    assert_eq!(kill.build, (12, 0, 0));
    assert_eq!(kill.project_id, 1);
    assert_eq!(kill.log_version, 22);
    assert_eq!(kill.tz_min, Some(-240), "the log is written at UTC-4");
    assert_eq!(kill.start_utc_ms, kill.start_local_ms + 240 * 60_000);
    assert_eq!(kill.duration_ms, 60_000);
    assert_eq!(kill.best_pct, Some(0), "R16: the kill took the boss to 0");
    assert_eq!(kill.players.len(), 3, "the pet folds into its owner");
    assert!(kill.players.iter().all(|p| p.logged && p.loadout.is_some()));
    assert!(kill.players.iter().all(|p| p.class.is_some()));
    assert!(kill.players[0].dps > 0.0);
    assert_eq!(kill.owner, None, "one log alone cannot name the logger");
    let facts = LogFacts::read(path);
    assert_eq!(
        kill.id,
        format!("{:016x}-{}", facts.id, kill.start_local_ms)
    );
    assert_eq!(kill.log, facts.id);
    assert!(store.has_details(&kill.id));

    let wipe = cards
        .iter()
        .find(|c| c.name == "Verkath the Hollow")
        .unwrap();
    assert_eq!(wipe.success, Some(false));
    assert_eq!(
        wipe.best_pct,
        Some(98),
        "R16: 8863800 / 9000000 rounds down"
    );
    assert!(!store.has_details(&wipe.id), "wipes get no details tier");
    let rows = store.rows(&wipe.id).expect("rows always");
    assert_eq!(rows.rows(wowdps_model::View::Damage).len(), 3);
    assert!(!rows.recaps.is_empty(), "somebody died in the wipe");

    assert!(
        !cards.iter().any(|c| c.kind == FightKind::Overall),
        "the raid visit never closed"
    );
    let k = kill.key.as_ref().expect("a raid boss knows its instance");
    assert_eq!((k.map_id, k.difficulty, k.level), (2769, 16, None));

    // Every card's rows file names the card, every loadout its hash.
    for c in cards {
        assert_eq!(store.rows(&c.id).unwrap().id, c.id);
        for p in &c.players {
            let l = store.loadout(p.loadout.unwrap()).unwrap();
            assert_eq!(l.hash, p.loadout.unwrap());
        }
    }
}

#[test]
fn trash_is_stored_only_under_the_switch() {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let mut with = mem(Retention {
        store_trash: true,
        ..Retention::default()
    });
    let ids = store_all(&mut with, path, &fights);
    assert_eq!(ids.len(), 4, "2 bosses + 2 trash: {ids:?}");
    assert_eq!(
        with.cards()
            .iter()
            .filter(|c| c.kind == FightKind::Trash)
            .count(),
        2
    );
}

#[test]
fn a_second_replay_writes_nothing() {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let mut store = mem(Retention::default());
    assert_eq!(store_all(&mut store, path, &fights).len(), 2);
    let before = store.backend().len();
    assert!(
        store_all(&mut store, path, &fights).is_empty(),
        "insert-if-absent on the fight id"
    );
    assert_eq!(store.backend().len(), before);
    assert_eq!(store.cards().len(), 2);

    // An aborted record is provisional: the real close replaces it.
    let boss = fights
        .iter()
        .position(|f| f.segment.kind == wowdps_core::meter::SegmentKind::Encounter)
        .unwrap();
    let mut aborted = fights[boss].clone();
    aborted.aborted = true;
    let mut store = mem(Retention::default());
    let facts = LogFacts::read(path);
    let id = store.store(&aborted, facts).unwrap();
    assert!(store.card(&id).unwrap().aborted);
    assert_eq!(store.store(&fights[boss], facts), Some(id.clone()));
    assert!(!store.card(&id).unwrap().aborted);
    assert_eq!(store.store(&aborted, facts), None, "never the other way");
    assert!(!store.card(&id).unwrap().aborted);
}

#[test]
fn a_crlf_copy_of_the_log_has_the_same_identity() {
    let tmp = Temp::new("crlf");
    let text = std::fs::read_to_string(SAMPLE).unwrap();
    let copy = tmp.join("WoWCombatLog-copy.txt");
    std::fs::write(&copy, text.replace('\n', "\r\n")).unwrap();
    let a = LogFacts::read(Path::new(SAMPLE));
    let b = LogFacts::read(&copy);
    assert_eq!(a.id, b.id);
    assert_eq!(a.tz_min, b.tz_min);
    assert_eq!(a.tz_min, Some(-240));

    // And the fights it holds resolve to the same ids.
    let fights = closed_fights_from(&copy, &std::fs::read_to_string(&copy).unwrap());
    let mut store = mem(Retention::default());
    let from_copy = store_all(&mut store, &copy, &fights);
    let original = closed_fights(Path::new(SAMPLE));
    assert!(
        store_all(&mut store, Path::new(SAMPLE), &original).is_empty(),
        "the original is already there under the copy's ids: {from_copy:?}"
    );
}

#[test]
fn a_half_written_first_line_is_not_an_identity_yet() {
    let tmp = Temp::new("halfline");
    let p = tmp.join("WoWCombatLog-x.txt");
    std::fs::write(&p, "7/27/2026 20:00:00.000-4  COMBAT_LOG_VER").unwrap();
    let facts = LogFacts::read(&p);
    assert_eq!(
        facts.id,
        wowdps_proto::history::fnv64(b"WoWCombatLog-x.txt"),
        "falls back to the file name"
    );
    assert_eq!(facts.tz_min, None);
    assert!(!facts.complete, "provisional: the header may still land");
    assert!(LogFacts::read(Path::new(SAMPLE)).complete);
}

#[test]
fn instance_fixture_stores_the_keyed_overall_not_its_members() {
    let path = Path::new(INSTANCE);
    let fights = closed_fights(path);
    let mut store = mem(Retention::default());
    let ids = store_all(&mut store, path, &fights);
    let kinds: Vec<FightKind> = store.cards().iter().map(|c| c.kind).collect();
    assert!(
        kinds.contains(&FightKind::Key),
        "the timed key's Σ is stored: {kinds:?} ({ids:?})"
    );
    let key = store
        .cards()
        .iter()
        .find(|c| c.kind == FightKind::Key)
        .unwrap();
    assert_eq!(key.name, "Algeth'ar Academy +12");
    assert_eq!(key.official_ms, Some(900_000));
    assert_eq!(key.success, Some(true), "timed");
    let k = key.key.as_ref().unwrap();
    assert_eq!(
        (k.map_id, k.level, k.completed),
        (2526, Some(12), Some(true))
    );
    assert!(key.pars_ms.is_some() || key.official_ms.is_some());
    // Vexamus was pulled inside the key: no card of his own.
    assert!(
        !store.cards().iter().any(|c| c.name == "Vexamus"),
        "a key's bosses are not stored on their own: {kinds:?}"
    );
    // Ranjit was pulled in an unkeyed Skyreach visit: a plain encounter.
    assert!(store.cards().iter().any(|c| c.name == "Ranjit"));

    let mut with_trash = mem(Retention {
        store_trash: true,
        ..Retention::default()
    });
    store_all(&mut with_trash, path, &fights);
    assert!(
        with_trash.cards().iter().any(|c| c.name == "Vexamus"),
        "the trash switch keeps a key's members too"
    );
}

#[test]
fn arena_fixture_stores_matches_with_their_verdicts_and_enemy_rows() {
    let path = Path::new(ARENA);
    let fights = closed_fights(path);
    let mut store = mem(Retention::default());
    store_all(&mut store, path, &fights);
    let matches: Vec<&FightCard> = store
        .cards()
        .iter()
        .filter(|c| c.kind == FightKind::Arena)
        .collect();
    assert!(matches.len() >= 2, "{:?}", store.cards());
    assert!(matches.iter().any(|m| m.success == Some(true)));
    assert!(matches.iter().any(|m| m.success == Some(false)));
    assert!(matches.iter().all(|m| m.encounter.is_none()));
    let decided = matches.iter().find(|m| m.success.is_some()).unwrap();
    assert!(decided.players.iter().any(|p| p.enemy), "enemy rows kept");
    assert!(decided.players.iter().any(|p| !p.enemy));
    let rows = store.rows(&decided.id).unwrap();
    assert!(
        rows.rows(wowdps_model::View::Damage)
            .iter()
            .any(|r| r.enemy)
    );
}

// ---- durability -------------------------------------------------------------------

#[test]
fn corrupt_cards_are_skipped_and_reported_and_the_rest_served() {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let mut store = mem(Retention::default());
    store_all(&mut store, path, &fights);
    // Reopen the same backend with two bad files dropped in.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "details", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    backend
        .write("fights", "torn.json", b"{\"schema\":1,\"id\":\"x")
        .unwrap();
    backend.write("fights", "foreign.json", b"[1,2,3]").unwrap();
    let reopened = Store::open(backend, Retention::default());
    assert_eq!(reopened.cards().len(), 2);
    assert_eq!(reopened.corrupt(), 2);
    assert!(
        reopened
            .status()
            .error
            .as_deref()
            .unwrap()
            .contains("2 unreadable"),
        "{:?}",
        reopened.status()
    );
}

#[test]
fn an_unwritable_store_fails_soft() {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let mut backend = MemBackend::new();
    backend.fail_writes = true;
    let mut store = Store::open(backend, Retention::default());
    assert!(store_all(&mut store, path, &fights).is_empty());
    assert!(
        store.cards().is_empty(),
        "nothing indexed that isn't on disk"
    );
    assert!(
        store
            .last_error
            .as_deref()
            .unwrap()
            .contains("write failed")
    );
    assert!(store.backend().is_empty());
}

#[test]
fn a_full_queue_drops_and_counts_instead_of_blocking() {
    let (link, _rx) = HistoryLink::bounded(1);
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let start = Instant::now();
    for f in fights.iter().take(3) {
        let _ = link.send(HistoryReq::Store(Box::new(f.clone())));
    }
    assert!(start.elapsed() < Duration::from_secs(1), "never blocks");
    assert_eq!(link.status().dropped, 2);
    assert!(link.enabled());

    let disabled = HistoryLink::disabled("off");
    let _ = disabled.send(HistoryReq::Sweep(PathBuf::from("/nowhere")));
    assert!(!disabled.enabled());
    assert_eq!(disabled.status().error.as_deref(), Some("off"));
    assert_eq!(
        disabled.status().dropped,
        0,
        "a disabled store drops nothing"
    );
}

// ---- retention ---------------------------------------------------------------------

/// A synthetic log: `n` pulls of one boss, each `dur_s` long, with one hit
/// per pull by a named player; `kill[i]` decides the outcome. Timestamps
/// advance 10 minutes per pull so ids and ordering are unambiguous.
fn boss_log(pulls: &[(u32, bool)]) -> String {
    let mut out = String::from(
        "7/27/2026 20:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1\n\
         7/27/2026 20:00:01.000-4  ZONE_CHANGE,2769,\"Nerub-ar Palace\",15\n",
    );
    for (i, (dur_s, kill)) in pulls.iter().enumerate() {
        let h = 20 + (i / 6) as u32;
        let m = ((i % 6) * 10) as u32;
        let ts = |sec: u32| format!("7/27/2026 {h:02}:{:02}:{:02}.000-4", m + sec / 60, sec % 60);
        out.push_str(&format!(
            "{}  ENCOUNTER_START,3130,\"The Ashen Warden\",15,20,2769\n",
            ts(0)
        ));
        out.push_str(&format!(
            "{}  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"The Ashen Warden\",0xa48,0x0,116,\"Frostbolt\",16,{},{},0,0,0,0,0,nil,nil\n",
            ts(1),
            1000 * (i as u64 + 1),
            1000 * (i as u64 + 1)
        ));
        out.push_str(&format!(
            "{}  ENCOUNTER_END,3130,\"The Ashen Warden\",15,20,{},{}\n",
            ts(*dur_s),
            u8::from(*kill),
            dur_s * 1000
        ));
    }
    out
}

fn boss_store(pulls: &[(u32, bool)], cfg: Retention) -> (Store<MemBackend>, Vec<String>) {
    let tmp = Temp::new("boss");
    let path = tmp.join("WoWCombatLog-boss.txt");
    std::fs::write(&path, boss_log(pulls)).unwrap();
    let fights = closed_fights(&path);
    let mut store = mem(cfg);
    let ids = store_all(&mut store, &path, &fights);
    (store, ids)
}

#[test]
fn retention_evicts_oldest_first_but_never_the_protected_set() {
    // Six pulls of one boss: the FASTEST kill is the oldest, the rest wipe
    // or kill slowly. Keep 3 → the fastest survives, two others go.
    let pulls = [
        (30, true),
        (90, false),
        (80, true),
        (70, false),
        (60, true),
        (50, false),
    ];
    let (store, ids) = boss_store(
        &pulls,
        Retention {
            keep_per_encounter: 3,
            keep_details_per_encounter: 10,
            ..Retention::default()
        },
    );
    assert_eq!(ids.len(), 6, "all six were written before eviction ran");
    let kept: Vec<i64> = store.cards().iter().map(|c| c.duration_ms).collect();
    assert_eq!(
        kept,
        vec![30_000, 60_000, 50_000],
        "fastest kill (oldest) protected; then the newest two"
    );
    assert_eq!(names(&store, "fights").len(), 3);
    assert_eq!(names(&store, "rows").len(), 3);
    assert_eq!(names(&store, "details").len(), 2, "kills 30s and 60s");
}

#[test]
fn a_pin_protects_a_fight_and_details_are_demoted_by_unlink() {
    let pulls = [(40, true), (35, true), (30, true), (25, true)];
    let tmp = Temp::new("pin");
    let path = tmp.join("WoWCombatLog-pin.txt");
    std::fs::write(&path, boss_log(&pulls)).unwrap();
    let fights = closed_fights(&path);
    let facts = LogFacts::read(&path);
    let mut store = mem(Retention {
        keep_per_encounter: 2,
        keep_details_per_encounter: 1,
        ..Retention::default()
    });
    // Store the first (slowest, oldest) and pin it, then the rest.
    let first = store.store(&fights[0], facts).unwrap();
    assert!(store.pin(&first, true));
    assert!(store.card(&first).unwrap().pinned);
    for f in &fights[1..] {
        store.store(f, facts);
    }
    let durs: Vec<i64> = store.cards().iter().map(|c| c.duration_ms).collect();
    assert!(
        durs.contains(&40_000),
        "the pinned slowest kill survives: {durs:?}"
    );
    assert!(
        durs.contains(&25_000),
        "the fastest kill survives: {durs:?}"
    );
    // Details: keep 1 beyond the protected set — the pin and the fastest
    // keep theirs, everything else was demoted by unlink.
    let with_details: Vec<i64> = store
        .cards()
        .iter()
        .filter(|c| store.has_details(&c.id))
        .map(|c| c.duration_ms)
        .collect();
    assert!(with_details.contains(&40_000) && with_details.contains(&25_000));
    assert!(names(&store, "details").len() <= 3);
    assert!(!store.pin("no-such-fight", true));
    assert!(store.pin(&first, false));
    assert!(!store.card(&first).unwrap().pinned);
}

#[test]
fn an_annotation_file_protects_a_fight() {
    let pulls = [(40, false), (30, false), (20, false)];
    let tmp = Temp::new("annot");
    let path = tmp.join("WoWCombatLog-a.txt");
    std::fs::write(&path, boss_log(&pulls)).unwrap();
    let fights = closed_fights(&path);
    let facts = LogFacts::read(&path);
    let mut store = mem(Retention {
        keep_per_encounter: 1,
        ..Retention::default()
    });
    let first = store.store(&fights[0], facts).unwrap();
    // Reserved for item 4: nothing writes these yet, but their presence
    // already counts.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    backend
        .write(
            "annotations",
            &format!("{first}.ndjson"),
            b"{\"kind\":\"note\"}\n",
        )
        .unwrap();
    let mut store = Store::open(
        backend,
        Retention {
            keep_per_encounter: 1,
            ..Retention::default()
        },
    );
    for f in &fights[1..] {
        store.store(f, facts);
    }
    let ids: Vec<&str> = store.cards().iter().map(|c| c.id.as_str()).collect();
    assert!(
        ids.contains(&first.as_str()),
        "annotated wipe kept: {ids:?}"
    );
    // The cap counts the protected fight too: nothing unprotected fits.
    assert_eq!(ids.len(), 1, "{ids:?}");
}

// ---- who is "me" -----------------------------------------------------------------------

fn card(log: u64, start: i64, players: &[(&str, &str, bool)]) -> FightCard {
    FightCard {
        id: wowdps_proto::history::fight_id(log, start, false),
        log,
        start_local_ms: start,
        start_utc_ms: start,
        players: players
            .iter()
            .map(|(guid, name, logged)| wowdps_proto::history::CardPlayer {
                guid: guid.to_string(),
                name: name.to_string(),
                logged: *logged,
                ..Default::default()
            })
            .collect(),
        ..FightCard::default()
    }
}

fn store_of(cards: &[FightCard], cfg: Retention) -> Store<MemBackend> {
    let mut backend = MemBackend::new();
    for c in cards {
        backend
            .write(
                "fights",
                &format!("{}.json", c.id),
                c.to_json().to_line().as_bytes(),
            )
            .unwrap();
    }
    Store::open(backend, cfg)
}

#[test]
fn the_owner_is_the_guid_every_log_logged_or_the_configured_character() {
    let night1 = [
        card(1, 100, &[("G-me", "Me-Realm", true), ("G-a", "A", true)]),
        card(1, 200, &[("G-me", "Me-Realm", true), ("G-b", "B", true)]),
    ];
    let night2 = [card(
        2,
        300,
        &[("G-me", "Me-Realm", true), ("G-c", "C", true)],
    )];
    let one_log = store_of(&night1, Retention::default());
    assert_eq!(
        one_log.owner(),
        None,
        "one log cannot tell the logger from a guildmate"
    );
    assert!(!one_log.status().owner_inferred);

    let all: Vec<FightCard> = night1.iter().chain(&night2).cloned().collect();
    let two_logs = store_of(&all, Retention::default());
    assert_eq!(two_logs.owner(), Some(("G-me".to_string(), true)));
    assert!(two_logs.status().owner_inferred);

    // A configured character wins, matched case-insensitively by name.
    let configured = store_of(
        &all,
        Retention {
            characters: vec!["a".to_string()],
            ..Retention::default()
        },
    );
    assert_eq!(configured.owner(), Some(("G-a".to_string(), false)));
    assert!(!configured.status().owner_inferred);
    // A bare name (no realm) matches the name half of "Name-Realm"; a
    // realm given must match whole.
    let bare = store_of(
        &all,
        Retention {
            characters: vec!["me".to_string()],
            ..Retention::default()
        },
    );
    assert_eq!(bare.owner(), Some(("G-me".to_string(), false)));
    let wrong_realm = store_of(
        &all,
        Retention {
            characters: vec!["Me-Other".to_string()],
            ..Retention::default()
        },
    );
    assert_eq!(wrong_realm.owner(), None);
    // Unknown character: nobody, not a guess.
    let unknown = store_of(
        &all,
        Retention {
            characters: vec!["Nobody-Realm".to_string()],
            ..Retention::default()
        },
    );
    assert_eq!(unknown.owner(), None);
}

#[test]
fn an_inferred_owner_survives_a_log_without_them() {
    // Two nights named the main; the cards carry the stamp the store
    // writes. A third log without them (an alt's dungeon, a friend's
    // import) empties the intersection but must not un-know the main:
    // every personal best would lose its retention protection.
    let stamp = |mut c: FightCard| {
        c.owner = Some("G-me".to_string());
        c
    };
    let known = [
        stamp(card(
            1,
            100,
            &[("G-me", "Me-Realm", true), ("G-a", "A", true)],
        )),
        stamp(card(
            2,
            200,
            &[("G-me", "Me-Realm", true), ("G-b", "B", true)],
        )),
    ];
    let alt = card(3, 300, &[("G-alt", "Alt-Realm", true), ("G-c", "C", true)]);
    let all: Vec<FightCard> = known.iter().cloned().chain([alt]).collect();
    let store = store_of(&all, Retention::default());
    assert_eq!(store.owner(), Some(("G-me".to_string(), true)));
    // Nothing ever stamped: still nobody, not a guess.
    let fresh = [
        card(1, 100, &[("G-me", "Me-Realm", true), ("G-a", "A", true)]),
        card(3, 300, &[("G-alt", "Alt-Realm", true), ("G-c", "C", true)]),
    ];
    assert_eq!(store_of(&fresh, Retention::default()).owner(), None);
}

#[test]
fn opening_a_store_moves_unmarked_sigma_cards_to_their_marked_ids() {
    // Schema-1 stores filed a visit's Σ under the pull spelling, where a
    // member starting on the visit's millisecond collides with it.
    let mut sigma = card(7, 500, &[("G-me", "Me", true)]);
    sigma.kind = FightKind::Overall;
    let pull = card(7, 500, &[("G-me", "Me", true)]);
    let old = sigma.id.clone();
    assert_eq!(old, pull.id, "the collision this migration exists for");
    let mut backend = MemBackend::new();
    backend
        .write(
            "fights",
            &format!("{old}.json"),
            sigma.to_json().to_line().as_bytes(),
        )
        .unwrap();
    backend
        .write("rows", &format!("{old}.json"), b"{}")
        .unwrap();
    backend
        .write("annotations", &format!("{old}.ndjson"), b"{}\n")
        .unwrap();
    let store = Store::open(backend, Retention::default());
    let marked = wowdps_proto::history::sigma_id(&old);
    assert_ne!(marked, old);
    assert_eq!(store.cards().len(), 1);
    assert_eq!(store.cards()[0].id, marked);
    assert!(store.card(&marked).is_some());
    assert!(store.card(&old).is_none());
    for (dir, ext) in [
        ("fights", "json"),
        ("rows", "json"),
        ("annotations", "ndjson"),
    ] {
        assert!(
            store.backend().exists(dir, &format!("{marked}.{ext}")),
            "{dir}"
        );
        assert!(
            !store.backend().exists(dir, &format!("{old}.{ext}")),
            "{dir}"
        );
    }
}

#[test]
fn a_regrade_that_downgrades_a_kill_drops_its_stale_details() {
    let path = Path::new(SAMPLE);
    let fights = closed_fights(path);
    let mut store = mem(Retention::default());
    store_all(&mut store, path, &fights);
    let facts = LogFacts::read(path);
    let kill = fights
        .iter()
        .find(|f| f.segment.success == Some(true))
        .expect("the fixture has a kill");
    let id = wowdps_proto::history::fight_id(facts.id, kill.segment.start_ms, false);
    assert!(store.has_details(&id), "a kill writes its details tier");
    let mut wiped = kill.clone();
    wiped.segment.success = Some(false);
    assert_eq!(store.regrade(&wiped, facts).as_deref(), Some(id.as_str()));
    assert!(
        !store.has_details(&id),
        "a regrade to a wipe must not serve the old parse's details as tier 3"
    );
    assert_eq!(store.card(&id).unwrap().success, Some(false));
    // And back: a kill again writes them fresh.
    assert!(store.regrade(kill, facts).is_some());
    assert!(store.has_details(&id));
}

// ---- real daemons: the import path --------------------------------------------------

fn options(tmp: &Temp, source: SourceSpec, history_dir: PathBuf) -> DaemonOptions {
    DaemonOptions {
        socket: tmp.join("test.sock"),
        lockfile: tmp.join("test.lock"),
        source,
        linger: true,
        idle_grace: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        version: "test".to_string(),
        cache_dir: None,
        game_pattern: None,
        loader_workers: 2,
        auto_overlay: false,
        overlay_exit_grace: Duration::ZERO,
        gui_bin: None,
        history: Some(HistoryOptions {
            dir: history_dir,
            store_trash: false,
            keep_per_encounter: 200,
            keep_details_per_encounter: 10,
            characters: Vec::new(),
            cache_dir: None,
        }),
    }
}

struct Daemon {
    socket: PathBuf,
    done: mpsc::Receiver<std::io::Result<()>>,
}

fn start(opts: DaemonOptions) -> Daemon {
    let socket = opts.socket.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(opts));
    });
    let deadline = Instant::now() + DEADLINE;
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon never bound {socket:?}");
        thread::sleep(Duration::from_millis(5));
    }
    Daemon { socket, done: rx }
}

fn stop(d: Daemon) {
    if let Ok(stream) = UnixStream::connect(&d.socket)
        && let Ok(mut c) = DaemonClient::over(stream, ClientKind::Mcp)
    {
        c.send(&ClientMsg::Shutdown);
    }
    let _ = d.done.recv_timeout(DEADLINE);
}

/// Poll `Status` until the store reports `fights` cards and nothing
/// importing, or the deadline passes.
fn wait_for_fights(socket: &Path, fights: u32) -> wowdps_proto::HistoryStatus {
    let stream = UnixStream::connect(socket).unwrap();
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).unwrap();
    let deadline = Instant::now() + DEADLINE;
    let mut last = None;
    let mut req_id = 1;
    while Instant::now() < deadline {
        client.send(&ClientMsg::GetStatus { req_id });
        req_id += 1;
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            for msg in client.poll() {
                if let DaemonMsg::Status { history, .. } = msg {
                    if history.fights == fights && history.importing == 0 {
                        return history;
                    }
                    last = Some(history);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    panic!("store never reached {fights} fights: {last:?}");
}

fn count(dir: &Path, sub: &str) -> usize {
    std::fs::read_dir(dir.join(sub))
        .map(|d| d.flatten().count())
        .unwrap_or(0)
}

#[test]
fn a_daemon_over_the_fixture_imports_its_history_and_a_restart_adds_nothing() {
    let tmp = Temp::new("import");
    let hist = tmp.join("history");
    let d = start(options(
        &tmp,
        SourceSpec::File(PathBuf::from(SAMPLE)),
        hist.clone(),
    ));
    let st = wait_for_fights(&d.socket, 2);
    assert!(st.enabled);
    assert_eq!(st.error, None);
    assert_eq!(st.dropped, 0);
    stop(d);
    assert_eq!(count(&hist, "fights"), 2);
    assert_eq!(count(&hist, "rows"), 2);
    assert_eq!(count(&hist, "details"), 1);
    assert_eq!(count(&hist, "loadouts"), 3);
    let mtimes = |sub: &str| -> Vec<std::time::SystemTime> {
        let mut v: Vec<_> = std::fs::read_dir(hist.join(sub))
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().modified().unwrap())
            .collect();
        v.sort();
        v
    };
    let before = (mtimes("fights"), mtimes("rows"));
    thread::sleep(Duration::from_millis(20));

    // Same directory, same fixture, a fresh daemon: zero new files, and the
    // index rebuilt from the cards alone.
    let tmp2 = Temp::new("import2");
    let d = start(options(
        &tmp2,
        SourceSpec::File(PathBuf::from(SAMPLE)),
        hist.clone(),
    ));
    let st = wait_for_fights(&d.socket, 2);
    assert_eq!(st.error, None);
    stop(d);
    assert_eq!(count(&hist, "fights"), 2);
    assert_eq!(
        (mtimes("fights"), mtimes("rows")),
        before,
        "nothing rewritten"
    );
    // The files are the truth: what a fresh Store reads is what was served.
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist.clone()),
        Retention::default(),
    );
    assert_eq!(reopened.cards().len(), 2);
    assert_eq!(reopened.corrupt(), 0);
}

#[test]
fn an_older_log_left_open_at_eof_imports_as_aborted() {
    // Two logs in one directory: the newest is the tailed fixture; the
    // older is a different session (its header line differs) cut before its
    // last ENCOUNTER_END, so its final pull never closed.
    let tmp = Temp::new("aborted");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let text = std::fs::read_to_string(SAMPLE).unwrap();
    let cut = text.rfind("ENCOUNTER_END").unwrap();
    let older = text[..cut].replace("7/27/2026 20:00:00.000-4", "7/20/2026 20:00:00.000-4");
    assert_ne!(older, text[..cut], "the header line must differ");
    let old = logs.join("WoWCombatLog-072026.txt");
    std::fs::write(&old, &older).unwrap();
    thread::sleep(Duration::from_millis(30));
    let new = logs.join("WoWCombatLog-072726.txt");
    std::fs::write(&new, &text).unwrap();

    let hist = tmp.join("history");
    let d = start(options(&tmp, SourceSpec::Dir(logs.clone()), hist.clone()));
    // Newest: 2 bosses (its raid visit is open at EOF and live, so no Σ
    // yet). Older: 1 closed boss + 1 aborted boss + its raid visit's Σ,
    // open at EOF but finished as far as that log is concerned.
    let st = wait_for_fights(&d.socket, 5);
    assert_eq!(st.error, None);
    stop(d);
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist),
        Retention::default(),
    );
    let aborted: Vec<&FightCard> = reopened.cards().iter().filter(|c| c.aborted).collect();
    assert_eq!(aborted.len(), 1, "{:?}", reopened.cards());
    assert_eq!(aborted[0].name, "Verkath the Hollow");
    assert_eq!(aborted[0].success, None);
    assert!(aborted[0].byte_range.is_some(), "provenance from the index");
    let old_facts = LogFacts::read(&old);
    assert_eq!(aborted[0].log, old_facts.id);
    assert_ne!(old_facts.id, LogFacts::read(&new).id);
    let overalls: Vec<&FightCard> = reopened
        .cards()
        .iter()
        .filter(|c| c.kind == FightKind::Overall)
        .collect();
    assert_eq!(
        overalls.len(),
        1,
        "only the older session's raid Σ: {overalls:?}"
    );
    assert_eq!(overalls[0].log, old_facts.id);
    assert!(
        !overalls[0].aborted,
        "a plain visit's Σ is complete as stored"
    );
    assert_eq!(
        reopened.cards().iter().filter(|c| !c.aborted).count(),
        4,
        "the older session's first boss and raid Σ plus the newest session's two"
    );
}

/// Restart mid-fight: the first daemon exits with a pull open; the second
/// starts on the grown file whose END has since arrived. The pull is
/// stored exactly once — by import, since its close predates CaughtUp.
#[test]
fn a_restart_mid_fight_stores_the_pull_once_via_import() {
    let tmp = Temp::new("midfight");
    let hist = tmp.join("history");
    let text = std::fs::read_to_string(SAMPLE).unwrap();
    let cut = text.rfind("ENCOUNTER_END").unwrap();
    let log = tmp.join("WoWCombatLog-live.txt");
    std::fs::write(&log, &text[..cut]).unwrap();

    let d = start(options(&tmp, SourceSpec::File(log.clone()), hist.clone()));
    // Only the first boss has closed; the open pull is the tailed log's own
    // live tail, never an aborted record.
    let st = wait_for_fights(&d.socket, 1);
    assert_eq!(st.error, None);
    stop(d);
    let first = Store::open(
        wowdps_daemon::history::DirBackend::new(hist.clone()),
        Retention::default(),
    );
    assert!(
        first.cards().iter().all(|c| !c.aborted),
        "{:?}",
        first.cards()
    );

    std::fs::write(&log, &text).unwrap();
    let tmp2 = Temp::new("midfight2");
    let d = start(options(&tmp2, SourceSpec::File(log.clone()), hist.clone()));
    let st = wait_for_fights(&d.socket, 2);
    assert_eq!(st.error, None);
    stop(d);
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist),
        Retention::default(),
    );
    let ids: Vec<&str> = reopened.cards().iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(reopened.cards().iter().all(|c| !c.aborted));
    assert!(reopened.cards().iter().all(|c| c.success.is_some()));
}

/// Perf gate over a real log: how many fights a night holds, how long the
/// import sweep takes, and the bytes per fight the lake costs.
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_import_reports_fights_wall_time_and_bytes() {
    let real = std::env::var("WOWDPS_REAL_LOG").expect("WOWDPS_REAL_LOG");
    let tmp = Temp::new("real");
    let hist = tmp.join("history");
    let started = Instant::now();
    let d = start(options(
        &tmp,
        SourceSpec::File(PathBuf::from(&real)),
        hist.clone(),
    ));
    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).unwrap();
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut settled: Option<wowdps_proto::HistoryStatus> = None;
    let mut req_id = 1;
    while Instant::now() < deadline {
        client.send(&ClientMsg::GetStatus { req_id });
        req_id += 1;
        thread::sleep(Duration::from_millis(200));
        let mut done = false;
        for msg in client.poll() {
            if let DaemonMsg::Status { history, .. } = msg {
                // Settled: nothing importing and the count stopped moving.
                done = history.importing == 0
                    && history.fights > 0
                    && settled.as_ref().is_some_and(|s| s.fights == history.fights);
                settled = Some(history);
            }
        }
        if done {
            break;
        }
    }
    let wall = started.elapsed();
    stop(d);
    let st = settled.expect("status");
    let bytes: u64 = ["fights", "rows", "details", "loadouts"]
        .iter()
        .flat_map(|sub| {
            std::fs::read_dir(hist.join(sub))
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum();
    eprintln!(
        "real log: {} fights imported in {:.1?} ({} dropped, error {:?}); lake {} KiB, {} B/fight",
        st.fights,
        wall,
        st.dropped,
        st.error,
        bytes / 1024,
        bytes / u64::from(st.fights.max(1))
    );
    assert!(st.fights > 0);
}

// ---- the mock's synchronous one-shots (v20) ----------------------------------------

#[test]
fn the_mock_answers_history_one_shots_from_its_in_memory_store() {
    use wowdps_daemon::mock::MockDaemon;
    use wowdps_proto::{ClientMsg, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery};

    let mut mock = MockDaemon::fixture().with_history();
    let out = mock.handle(ClientMsg::GetHistory {
        req_id: 1,
        query: HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Newest,
            limit: 0,
            after_id: None,
        },
    });
    let [
        DaemonMsg::History {
            req_id: 1,
            answer: HistoryAnswer::Fights { cards, .. },
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(cards.len(), 2, "both bosses of the fixture");
    // Paging: limit 1 answers the newest with total 2; after its id the
    // other one follows; a stale cursor starts over from the top.
    let mut page = |after_id: Option<&str>| {
        let out = mock.handle(ClientMsg::GetHistory {
            req_id: 9,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                guid: None,
                since_utc_ms: None,
                kind: None,
                sort: FightSort::Newest,
                limit: 1,
                after_id: after_id.map(str::to_string),
            },
        });
        match out.as_slice() {
            [
                DaemonMsg::History {
                    answer: HistoryAnswer::Fights { cards, total },
                    ..
                },
            ] => (cards.clone(), *total),
            other => panic!("{other:?}"),
        }
    };
    let (first, total) = page(None);
    assert_eq!((first.len(), total), (1, 2));
    assert_eq!(first[0].id, cards[0].id);
    let (second, total) = page(Some(&first[0].id));
    assert_eq!((second.len(), total), (1, 2));
    assert_eq!(second[0].id, cards[1].id);
    let (third, _) = page(Some(&second[0].id));
    assert!(third.is_empty(), "past the end: {third:?}");
    let (stale, _) = page(Some("0000000000000000-0"));
    assert_eq!(stale[0].id, cards[0].id, "an unknown cursor starts over");
    assert!(
        cards[0].start_utc_ms >= cards[1].start_utc_ms,
        "newest first"
    );

    // Best kill: fastest, limit 1 — the one kill in the fixture.
    let out = mock.handle(ClientMsg::GetHistory {
        req_id: 2,
        query: HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Fastest,
            limit: 1,
            after_id: None,
        },
    });
    let [
        DaemonMsg::History {
            answer: HistoryAnswer::Fights { cards: best, .. },
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(best.len(), 1);
    assert_eq!(best[0].name, "The Ashen Warden");
    let kill_id = best[0].id.clone();

    // Progression on the kill's boss: one pull, one kill, one night.
    let out = mock.handle(ClientMsg::GetHistory {
        req_id: 3,
        query: HistoryQuery::Progression {
            encounter: 3130,
            difficulty: 15,
            local_cutover_hour: None,
        },
    });
    let [
        DaemonMsg::History {
            answer:
                HistoryAnswer::Progression {
                    pulls,
                    kills,
                    first_kill,
                    nights,
                    median_kill_ms,
                },
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!((*pulls, *kills), (1, 1));
    assert_eq!(
        first_kill.as_ref().map(|c| c.id.as_str()),
        Some(kill_id.as_str())
    );
    assert_eq!(nights.len(), 1);
    assert!(nights[0].kill);
    assert_eq!(nights[0].best_pct, Some(0), "R16 per night");
    assert_eq!(*median_kill_ms, Some(60_000));

    // A stored fight, drilled: the kill keeps its details tier.
    let guid = best[0].players[0].guid.clone();
    let out = mock.handle(ClientMsg::GetFight {
        req_id: 4,
        fight_id: kill_id.clone(),
        view: wowdps_model::View::Damage,
        drill: Some(guid.clone()),
        boss: None,
    });
    let [
        DaemonMsg::Fight {
            req_id: 4,
            fight: Some(f),
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(f.card.id, kill_id);
    assert_eq!(f.rows.len(), 3);
    let b = f.breakdown.as_ref().expect("details for the kill");
    assert!(!b.by_spell.is_empty());
    assert!(b.timeline.is_some());

    // Trend for that player, per fight.
    let out = mock.handle(ClientMsg::GetHistory {
        req_id: 5,
        query: HistoryQuery::Trend {
            guid: guid.clone(),
            spec: None,
            encounter: None,
            difficulty: None,
            view: wowdps_model::View::Damage,
            bucket: wowdps_proto::TrendBucket::None,
            since_utc_ms: None,
            limit: 0,
            local_cutover_hour: None,
        },
    });
    let [
        DaemonMsg::History {
            answer: HistoryAnswer::Trend(points),
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(points.len(), 2);
    assert!(points.iter().all(|p| p.per_sec > 0.0 && p.n == 1));
    // Bucketed by day: the whole fixture is one evening.
    let out = mock.handle(ClientMsg::GetHistory {
        req_id: 6,
        query: HistoryQuery::Trend {
            guid,
            spec: None,
            encounter: None,
            difficulty: None,
            view: wowdps_model::View::Damage,
            bucket: wowdps_proto::TrendBucket::Day,
            since_utc_ms: None,
            limit: 0,
            local_cutover_hour: None,
        },
    });
    let [
        DaemonMsg::History {
            answer: HistoryAnswer::Trend(days),
            ..
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].n, 2);

    // Pin, then unknown fight and unknown pin.
    let out = mock.handle(ClientMsg::PinFight {
        req_id: 7,
        fight_id: kill_id.clone(),
        pinned: true,
    });
    assert!(matches!(
        out.as_slice(),
        [
            DaemonMsg::History {
                answer: HistoryAnswer::Pinned { pinned: true, .. },
                ..
            },
            DaemonMsg::HistoryChanged { .. }
        ]
    ));
    assert!(mock.history().card(&kill_id).unwrap().pinned);
    let out = mock.handle(ClientMsg::GetFight {
        req_id: 8,
        fight_id: "nope".to_string(),
        view: wowdps_model::View::Damage,
        drill: None,
        boss: None,
    });
    assert!(matches!(
        out.as_slice(),
        [DaemonMsg::Fight {
            req_id: 8,
            fight: None
        }]
    ));
    let out = mock.handle(ClientMsg::PinFight {
        req_id: 9,
        fight_id: "nope".to_string(),
        pinned: true,
    });
    assert!(matches!(
        out.as_slice(),
        [
            DaemonMsg::History {
                answer: HistoryAnswer::Pinned { pinned: false, .. },
                ..
            },
            DaemonMsg::HistoryChanged { .. }
        ]
    ));
}

/// The night's last key: the player zones out and logs off, which only
/// SUSPENDS the visit (R10), so the key is still open at the end of the
/// log and exists only as the index's `open_visit`. The sweep must store
/// it — a completed key is a finished run, not an aborted one.
#[test]
fn an_older_logs_last_key_left_open_at_eof_is_imported() {
    let tmp = Temp::new("openvisit");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    // instance.txt up to the ZONE_CHANGE out of the completed key.
    let text = std::fs::read_to_string(INSTANCE).unwrap();
    let cut = text.find("ZONE_CHANGE,0,\"Silvermoon City\"").unwrap();
    let cut = text[cut..].find('\n').unwrap() + cut + 1;
    let older = &text[..cut];
    assert!(older.contains("CHALLENGE_MODE_END,2526,1,12"));
    let old = logs.join("WoWCombatLog-080126.txt");
    std::fs::write(&old, older).unwrap();
    thread::sleep(Duration::from_millis(30));
    let new = logs.join("WoWCombatLog-072726.txt");
    std::fs::write(&new, std::fs::read_to_string(SAMPLE).unwrap()).unwrap();

    let hist = tmp.join("history");
    let d = start(options(&tmp, SourceSpec::Dir(logs.clone()), hist.clone()));
    // Newest: 2 bosses. Older: the plain visit closed by the reset END on
    // entry, plus the key's Σ (its boss is a member, not stored on its own).
    let st = wait_for_fights(&d.socket, 4);
    assert_eq!(st.error, None);
    stop(d);
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist),
        Retention::default(),
    );
    let key = reopened
        .cards()
        .iter()
        .find(|c| c.kind == FightKind::Key)
        .unwrap_or_else(|| panic!("the open key is stored: {:?}", reopened.cards()));
    assert_eq!(key.name, "Algeth'ar Academy +12");
    assert_eq!(key.success, Some(true));
    assert!(!key.aborted, "a key whose END fired is a finished run");
    assert_eq!(key.official_ms, Some(900_000));
    assert_eq!(key.log, LogFacts::read(&old).id);
    assert!(!reopened.cards().iter().any(|c| c.name == "Vexamus"));
}

/// `wowdps history import <file>` on an older session's log: the file is
/// not the daemon's tailed log, so its open visit is a finished night, not
/// live — the key must come in, its member boss must not.
#[test]
fn importing_an_older_file_by_hand_stores_its_open_key() {
    let tmp = Temp::new("importfile");
    let text = std::fs::read_to_string(INSTANCE).unwrap();
    let cut = text.find("ZONE_CHANGE,0,\"Silvermoon City\"").unwrap();
    let cut = text[cut..].find('\n').unwrap() + cut + 1;
    let old = tmp.join("WoWCombatLog-080126.txt");
    std::fs::write(&old, &text[..cut]).unwrap();
    let new = tmp.join("WoWCombatLog-072726.txt");
    std::fs::write(&new, std::fs::read_to_string(SAMPLE).unwrap()).unwrap();

    let hist = tmp.join("history");
    // The daemon tails the fixture alone; the older file is not its source.
    let d = start(options(&tmp, SourceSpec::File(new), hist.clone()));
    wait_for_fights(&d.socket, 2);
    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).unwrap();
    client.send(&ClientMsg::ImportLog {
        req_id: 7,
        path: old.display().to_string(),
    });
    let st = wait_for_fights(&d.socket, 4);
    assert_eq!(st.error, None);
    stop(d);
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist),
        Retention::default(),
    );
    let key = reopened
        .cards()
        .iter()
        .find(|c| c.kind == FightKind::Key)
        .unwrap_or_else(|| panic!("the open key is stored: {:?}", reopened.cards()));
    assert_eq!(key.name, "Algeth'ar Academy +12");
    assert!(!key.aborted);
    assert!(
        !reopened.cards().iter().any(|c| c.name == "Vexamus"),
        "the key's boss is a member, not a fight of its own"
    );
}

/// A key abandoned before its END (the log ends inside it) is stored
/// aborted — and still counts as a key, so its member bosses are not
/// promoted to pulls of their own.
#[test]
fn an_abandoned_key_is_aborted_and_still_owns_its_bosses() {
    let tmp = Temp::new("abandonedkey");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let text = std::fs::read_to_string(INSTANCE).unwrap();
    // Cut right after Vexamus dies: the key's END never comes.
    let cut = text.find("ENCOUNTER_END,2562").unwrap();
    let cut = text[cut..].find('\n').unwrap() + cut + 1;
    let old = logs.join("WoWCombatLog-080126.txt");
    std::fs::write(&old, &text[..cut]).unwrap();
    thread::sleep(Duration::from_millis(30));
    let new = logs.join("WoWCombatLog-072726.txt");
    std::fs::write(&new, std::fs::read_to_string(SAMPLE).unwrap()).unwrap();

    let hist = tmp.join("history");
    let d = start(options(&tmp, SourceSpec::Dir(logs), hist.clone()));
    // Newest: 2 bosses. Older: the pre-key plain Σ + the aborted key.
    let st = wait_for_fights(&d.socket, 4);
    assert_eq!(st.error, None);
    stop(d);
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist),
        Retention::default(),
    );
    let key = reopened
        .cards()
        .iter()
        .find(|c| c.kind == FightKind::Key)
        .unwrap_or_else(|| panic!("the abandoned key is stored: {:?}", reopened.cards()));
    assert!(key.aborted, "no END: aborted");
    assert_eq!(key.success, None);
    assert!(
        !reopened.cards().iter().any(|c| c.name == "Vexamus"),
        "an aborted key still owns its bosses: {:?}",
        reopened.cards()
    );
}

/// A boss pulled at Mythic Keystone difficulty is a key's member even when
/// the key's START predates the log (the daemon attached mid-run), so the
/// plain visit that results does not promote it to a pull of its own.
#[test]
fn a_keystone_difficulty_boss_without_its_key_start_is_still_a_member() {
    let text = std::fs::read_to_string(INSTANCE).unwrap();
    let headless: String = text
        .lines()
        .filter(|l| !l.contains("CHALLENGE_MODE_"))
        .map(|l| format!("{l}\n"))
        .collect();
    let path = Path::new("WoWCombatLog-headless.txt");
    let fights = closed_fights_from(path, &headless);
    assert!(
        fights.iter().any(|f| f.segment.name == "Vexamus"),
        "the pull closed on the live meter"
    );
    let mut store = mem(Retention::default());
    store_all(&mut store, path, &fights);
    assert!(
        !store.cards().iter().any(|c| c.name == "Vexamus"),
        "difficulty 8 is a key: {:?}",
        store.cards()
    );
    let mut with_trash = mem(Retention {
        store_trash: true,
        ..Retention::default()
    });
    store_all(&mut with_trash, path, &fights);
    assert!(with_trash.cards().iter().any(|c| c.name == "Vexamus"));
}

/// A pin is the user's decision: the rewrite that turns an aborted record
/// into the real fight (its END arriving after a restart) keeps it.
#[test]
fn a_pin_survives_the_aborted_to_real_rewrite() {
    let path = Path::new(SAMPLE);
    let text = std::fs::read_to_string(path).unwrap();
    let cut = text.rfind("ENCOUNTER_END").unwrap();
    let mut store = mem(Retention::default());
    // The last pull, cut before its END: stored aborted.
    let partial: Vec<ClosedFight> = closed_fights_from(path, &text[..cut])
        .into_iter()
        .map(|mut f| {
            f.aborted = true;
            f
        })
        .collect();
    let ids = store_all(&mut store, path, &partial);
    let id = ids.last().cloned().expect("the aborted pull");
    assert!(store.card(&id).unwrap().aborted);
    assert!(store.pin(&id, true));
    // The END arrives: the real fight replaces the aborted record.
    let whole = closed_fights(path);
    store_all(&mut store, path, &whole);
    let card = store.card(&id).unwrap();
    assert!(!card.aborted, "rewritten as the real fight");
    assert!(card.pinned, "the pin came along");
}

/// Item 10: a key entered and left without its CHALLENGE_MODE_END — the
/// visit closes at the next instance's zone-in — is a keystone record
/// that never finished: aborted, not a run with no verdict.
#[test]
fn a_key_left_without_its_end_is_stored_aborted_on_the_live_path() {
    let text = std::fs::read_to_string(INSTANCE).unwrap();
    let abandoned: String = text
        .lines()
        .filter(|l| !l.contains("CHALLENGE_MODE_END,2526,1,"))
        .map(|l| format!("{l}\n"))
        .collect();
    let path = Path::new("WoWCombatLog-abandoned.txt");
    let fights = closed_fights_from(path, &abandoned);
    let mut store = mem(Retention::default());
    store_all(&mut store, path, &fights);
    let key = store
        .cards()
        .iter()
        .find(|c| c.kind == FightKind::Key)
        .unwrap_or_else(|| panic!("the key's Σ is stored: {:?}", store.cards()));
    assert!(key.aborted, "{key:?}");
    assert_eq!(key.success, None);
    assert_eq!(key.key.as_ref().and_then(|k| k.completed), None);
}

/// `Regrade`: a stored card is rewritten from its log in place — same id,
/// pin kept — and the answer counts what was queued.
#[test]
fn a_regrade_rewrites_a_card_in_place_and_keeps_its_pin() {
    use wowdps_proto::HistoryAnswer;
    let tmp = Temp::new("regrade");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let log = logs.join("WoWCombatLog-072726.txt");
    std::fs::write(&log, std::fs::read_to_string(SAMPLE).unwrap()).unwrap();
    let hist = tmp.join("history");
    let d = start(options(&tmp, SourceSpec::Dir(logs), hist.clone()));
    wait_for_fights(&d.socket, 2);

    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).unwrap();
    // Find the kill, pin it, then tamper with its stored grade so the
    // rewrite is observable.
    client.send(&ClientMsg::GetHistory {
        req_id: 1,
        query: wowdps_proto::HistoryQuery::Fights {
            encounter: Some(3130),
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: wowdps_proto::FightSort::Newest,
            limit: 0,
            after_id: None,
        },
    });
    let deadline = Instant::now() + DEADLINE;
    let mut id = None;
    while id.is_none() && Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::History {
                answer: HistoryAnswer::Fights { cards, .. },
                ..
            } = msg
            {
                id = cards.first().map(|c| c.id.clone());
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    let id = id.expect("the kill's card");
    client.send(&ClientMsg::PinFight {
        req_id: 2,
        fight_id: id.clone(),
        pinned: true,
    });
    thread::sleep(Duration::from_millis(200));
    let card_path = hist.join("fights").join(format!("{id}.json"));
    let tampered = std::fs::read_to_string(&card_path)
        .unwrap()
        .replace("\"best_pct\":0", "\"best_pct\":77");
    assert!(tampered.contains("\"best_pct\":77"), "{tampered}");
    std::fs::write(&card_path, tampered).unwrap();

    client.send(&ClientMsg::Regrade {
        req_id: 3,
        fight_id: Some(id.clone()),
        encounter: None,
        difficulty: None,
        kind: None,
    });
    let deadline = Instant::now() + DEADLINE;
    let mut queued = None;
    while queued.is_none() && Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::History {
                answer: HistoryAnswer::Regraded { queued: n },
                ..
            } = msg
            {
                queued = Some(n);
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(queued, Some(1));
    wait_for_fights(&d.socket, 2);
    stop(d);
    let reopened = Store::open(
        wowdps_daemon::history::DirBackend::new(hist),
        Retention::default(),
    );
    let card = reopened.card(&id).expect("still there");
    assert_eq!(card.best_pct, Some(0), "re-derived from the log");
    assert!(card.pinned, "the pin survived the rewrite");
}

/// A key's card lists its member bosses, and `GetFight { boss }` parses one
/// from the log on demand — its own rows and breakdown, nothing stored.
#[test]
fn a_keys_member_boss_drills_from_the_log_on_demand() {
    use wowdps_model::View;
    use wowdps_proto::{FightSort, HistoryAnswer, HistoryQuery};
    let tmp = Temp::new("bossdrill");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    // The keyed fixture as an older log; the sample as the tailed one.
    let old = logs.join("WoWCombatLog-080126.txt");
    std::fs::write(&old, std::fs::read_to_string(INSTANCE).unwrap()).unwrap();
    thread::sleep(Duration::from_millis(30));
    let new = logs.join("WoWCombatLog-072726.txt");
    std::fs::write(&new, std::fs::read_to_string(SAMPLE).unwrap()).unwrap();
    let hist = tmp.join("history");
    let d = start(options(&tmp, SourceSpec::Dir(logs), hist.clone()));
    // Sample: 2 bosses. Instance: Ranjit, the Skyreach Σ, the key's Σ and
    // the pre-key plain Σ.
    let st = wait_for_fights(&d.socket, 6);
    assert_eq!(st.error, None);

    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).unwrap();
    client.send(&ClientMsg::GetHistory {
        req_id: 1,
        query: HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: Some(FightKind::Key),
            sort: FightSort::Newest,
            limit: 0,
            after_id: None,
        },
    });
    let deadline = Instant::now() + DEADLINE;
    let mut key = None;
    while key.is_none() && Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::History {
                answer: HistoryAnswer::Fights { cards, .. },
                ..
            } = msg
            {
                key = cards.into_iter().next();
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    let key = key.expect("the key's card");
    assert_eq!(key.name, "Algeth'ar Academy +12");
    assert_eq!(
        key.bosses
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        ["Vexamus"],
        "{:?}",
        key.bosses
    );
    assert_eq!(key.bosses[0].success, Some(true));

    client.send(&ClientMsg::GetFight {
        req_id: 2,
        fight_id: key.id.clone(),
        view: View::Damage,
        drill: None,
        boss: Some("vexamus".to_string()),
    });
    let deadline = Instant::now() + DEADLINE;
    let mut answer = None;
    while answer.is_none() && Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::Fight { req_id: 2, fight } = msg {
                answer = Some(fight);
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    let f = answer.expect("answered").expect("the boss parsed");
    assert_eq!(f.card.name, "Vexamus");
    assert_eq!(f.card.kind, FightKind::Encounter);
    assert_eq!(f.tier, 3);
    assert!(!f.rows.is_empty(), "the boss's own damage rows");
    // A boss the key does not have answers None.
    client.send(&ClientMsg::GetFight {
        req_id: 3,
        fight_id: key.id.clone(),
        view: View::Damage,
        drill: None,
        boss: Some("Nobody".to_string()),
    });
    let deadline = Instant::now() + DEADLINE;
    let mut answer = None;
    while answer.is_none() && Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::Fight { req_id: 3, fight } = msg {
                answer = Some(fight);
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(answer.expect("answered").is_none());
    stop(d);
    // Nothing was stored for the boss.
    assert_eq!(count(&hist, "fights"), 6);
}

/// Local nights: with a cutover hour the fixture's 20:05 UTC-4 evening is
/// the 07-27 night (its bucket starts 07-27 06:00 local = 10:00 UTC),
/// while the UTC day puts it on 07-28.
#[test]
fn a_local_cutover_keeps_an_evening_on_its_own_night() {
    use wowdps_daemon::mock::MockDaemon;
    use wowdps_proto::{ClientMsg, DaemonMsg, HistoryAnswer, HistoryQuery};
    let mut mock = MockDaemon::fixture().with_history();
    let ask = |mock: &mut MockDaemon, cutover: Option<u8>| {
        let out = mock.handle(ClientMsg::GetHistory {
            req_id: 1,
            query: HistoryQuery::Progression {
                encounter: 3130,
                difficulty: 15,
                local_cutover_hour: cutover,
            },
        });
        match out.as_slice() {
            [
                DaemonMsg::History {
                    answer: HistoryAnswer::Progression { nights, .. },
                    ..
                },
            ] => nights.clone(),
            other => panic!("{other:?}"),
        }
    };
    let utc = ask(&mut mock, None);
    let local = ask(&mut mock, Some(6));
    assert_eq!((utc.len(), local.len()), (1, 1));
    assert_eq!(utc[0].tz_min, Some(-240));
    // 07-28 00:00 UTC (the UTC day) vs 07-27 10:00 UTC (06:00 local).
    assert_eq!(local[0].day_utc_ms, utc[0].day_utc_ms - 14 * 3_600_000);
    assert_eq!(local[0].pulls, utc[0].pulls);
}
