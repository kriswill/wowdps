//! v45: the history store's replay tier (R29) and the progression rule,
//! over the in-memory backend driven straight from the engine — which
//! fights keep a replay, what retention and the size cap take, what
//! `GetReplay` answers — and every wipe on a boss the store has not seen
//! killed at its difficulty kept whole until the first kill there.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use wowdps_core::index::{self, load_segment_text};
use wowdps_core::model::replay as replay_kind;
use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{Backend, ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_daemon::mock::MockDaemon;
use wowdps_daemon::replay::cut_text;
use wowdps_proto::history::FightKind;
use wowdps_proto::{ClientMsg, DaemonMsg, replay};

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("wowdps-replay-{tag}-{}-{n}", std::process::id()));
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

/// One boss's pulls — (seconds, kill, difficulty) — one log, ten minutes
/// apart, each with a hit and a hostile cast.
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

/// A log of `pulls` and its fights, closed.
fn log_of(tmp: &Temp, pulls: &[(u32, bool, u32)]) -> (PathBuf, Vec<ClosedFight>) {
    let path = tmp.0.join("WoWCombatLog-boss.txt");
    std::fs::write(&path, boss_log(pulls)).unwrap();
    let fights = closed_fights(&path);
    (path, fights)
}

/// Cut the fight `id` from its log, as the rewrite queue does, and offer it
/// to the store: whether it kept it.
fn cut_into(store: &mut Store<MemBackend>, path: &Path, id: &str) -> bool {
    let start = store.card(id).unwrap().start_local_ms;
    let mut file = std::fs::File::open(path).unwrap();
    let idx = index::scan(&mut file);
    let meta = idx.segments.iter().find(|m| m.start_ms == start).unwrap();
    let text = load_segment_text(path, meta).unwrap();
    store.store_replay(id, cut_text(&text, None))
}

fn cfg(f: impl FnOnce(&mut Retention)) -> Retention {
    let mut r = Retention::default();
    f(&mut r);
    r
}

// ---- progression -------------------------------------------------------------

/// Three short wipes on a boss never killed at Heroic: each is progression
/// — protected, with details and series however short — until the first
/// kill lands, when they answer to the caps again (keep 1, details 0).
#[test]
fn wipes_before_the_first_kill_are_kept_whole_until_it_lands() {
    let tmp = Temp::new("prog");
    let (path, fights) = log_of(
        &tmp,
        &[
            (30, false, 15),
            (40, false, 15),
            (50, false, 15),
            (90, true, 15),
        ],
    );
    let facts = LogFacts::read(&path);
    let mut store = Store::open(
        MemBackend::new(),
        cfg(|r| {
            r.keep_per_encounter = 1;
            r.keep_details_per_encounter = 0;
        }),
    );
    let wipes: Vec<String> = fights[..3]
        .iter()
        .map(|f| store.store(f, facts).unwrap())
        .collect();
    for id in &wipes {
        let card = store.card(id).unwrap();
        assert!(store.is_progression(card), "{id}");
        assert!(
            store.has_details(id),
            "a progression wipe keeps its details"
        );
        assert!(store.has_series(id), "and its series");
    }
    assert_eq!(store.cards().len(), 3, "the caps never touch progression");
    let st = store.status();
    assert_eq!(
        (
            st.kept_kills,
            st.kept_keys,
            st.kept_progression,
            st.kept_pins
        ),
        (0, 0, 3, 0)
    );
    for id in &wipes {
        assert!(
            cut_into(&mut store, &path, id),
            "a progression wipe keeps its replay"
        );
    }
    assert_eq!(store.status().replays, 3);

    // The kill: the boss is no longer progression at Heroic, and the next
    // retention pass (this very write) takes the wipes back to the caps.
    let kill = store.store(&fights[3], facts).unwrap();
    assert!(!store.is_progression(store.card(&wipes[2]).unwrap()));
    let ids: Vec<&str> = store.cards().iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [wipes[2].as_str(), kill.as_str()],
        "keep 1 wipe beside the kill"
    );
    assert!(!store.has_details(&wipes[2]), "its details demoted (cap 0)");
    assert!(!store.has_series(&wipes[2]), "and its series");
    assert!(
        !store.has_replay(&wipes[2]),
        "and its replay (the details cap)"
    );
    assert!(!store.has_replay(&wipes[0]) && !store.has_replay(&wipes[1]));
    let st = store.status();
    assert_eq!(
        (
            st.kept_kills,
            st.kept_keys,
            st.kept_progression,
            st.kept_pins
        ),
        (1, 0, 0, 0)
    );
}

/// A pin keeps a progression wipe through the kill; a wipe AFTER the kill
/// is an ordinary wipe.
#[test]
fn a_pinned_progression_wipe_stays_and_a_later_wipe_is_ordinary() {
    let tmp = Temp::new("prog-pin");
    let (path, fights) = log_of(
        &tmp,
        &[
            (30, false, 15),
            (40, false, 15),
            (90, true, 15),
            (45, false, 15),
        ],
    );
    let facts = LogFacts::read(&path);
    let mut store = Store::open(
        MemBackend::new(),
        cfg(|r| {
            r.keep_per_encounter = 1;
            r.keep_details_per_encounter = 0;
        }),
    );
    let first = store.store(&fights[0], facts).unwrap();
    assert!(store.pin(&first, true));
    let second = store.store(&fights[1], facts).unwrap();
    let kill = store.store(&fights[2], facts).unwrap();
    let after = store.store(&fights[3], facts).unwrap();
    assert!(store.card(&first).is_some(), "the pin holds");
    assert!(store.card(&second).is_none(), "the cap took the other");
    assert!(!store.is_progression(store.card(&after).unwrap()));
    assert!(
        !store.has_details(&after),
        "45 s, after the kill: an ordinary wipe"
    );
    let st = store.status();
    assert_eq!(
        (st.kept_kills, st.kept_progression, st.kept_pins),
        (1, 0, 1)
    );
    assert!(store.card(&kill).is_some());
}

/// A Heroic kill says nothing of Mythic: a Mythic wipe stays progression.
#[test]
fn progression_is_per_difficulty() {
    let tmp = Temp::new("prog-diff");
    let (path, fights) = log_of(&tmp, &[(90, true, 15), (30, false, 16), (30, false, 15)]);
    let facts = LogFacts::read(&path);
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids: Vec<String> = fights
        .iter()
        .map(|f| store.store(f, facts).unwrap())
        .collect();
    assert!(
        store.is_progression(store.card(&ids[1]).unwrap()),
        "Mythic, unkilled"
    );
    assert!(store.has_details(&ids[1]));
    assert!(
        !store.is_progression(store.card(&ids[2]).unwrap()),
        "Heroic, killed"
    );
    assert!(!store.has_details(&ids[2]), "a 30 s ordinary wipe");

    // Off, a wipe is a wipe.
    let mut off = Store::open(MemBackend::new(), cfg(|r| r.keep_progression = false));
    let id = off.store(&fights[1], facts).unwrap();
    assert!(!off.is_progression(off.card(&id).unwrap()));
    assert!(!off.has_details(&id));
}

// ---- the replay tier ------------------------------------------------------------

/// Every boss pull is cut, and retention keeps what the details caps keep:
/// the protected set (a kill here) and the newest of the rest.
#[test]
fn a_replay_is_kept_in_the_details_slots() {
    let tmp = Temp::new("slots");
    let (path, fights) = log_of(
        &tmp,
        &[
            (70, false, 15),
            (70, false, 15),
            (70, false, 15),
            (90, true, 15),
        ],
    );
    let facts = LogFacts::read(&path);
    let mut store = Store::open(
        MemBackend::new(),
        cfg(|r| {
            r.keep_details_per_encounter = 1;
            r.keep_progression = false;
        }),
    );
    let ids: Vec<String> = fights
        .iter()
        .map(|f| store.store(f, facts).unwrap())
        .collect();
    let kept: Vec<bool> = ids
        .iter()
        .map(|id| cut_into(&mut store, &path, id))
        .collect();
    assert_eq!(
        kept,
        [false, false, true, true],
        "the newest wipe and the kill"
    );
    assert_eq!(store.status().replays, 2);
    assert!(store.status().replay_bytes > 0);
    // What the store keeps reads back as the cut.
    let bytes = store.replay_file(&ids[3]).unwrap();
    let cut = replay::decode(&bytes).unwrap();
    assert_eq!(cut.head.success, Some(true));
    assert_eq!(
        cut.events.len(),
        3,
        "the boss's one cast between its two boss rows"
    );
    assert!(!store.wants_recut(&ids[3]), "a kept replay wants nothing");
    assert!(
        !store.wants_recut(&ids[0]),
        "a wipe past the slots wants none"
    );
    // A pin earns the oldest wipe a slot, and so a cut.
    assert!(store.pin(&ids[0], true));
    assert!(store.wants_recut(&ids[0]));
    // Its series is wanted too (a pinned long wipe): a whole rewrite, which
    // cuts the replay beside it, not a cut alone.
    assert_eq!(store.rewrites(), [ids[0].clone()]);
    assert!(store.recuts().is_empty());
    assert!(cut_into(&mut store, &path, &ids[0]));
    // Letting go drops it again.
    assert!(store.pin(&ids[0], false));
    assert!(!store.has_replay(&ids[0]));
}

/// Past `history_replay_mb` the oldest unprotected replays go; a protected
/// one never does, and a full tier backfills nothing more but protected.
#[test]
fn the_size_cap_takes_the_oldest_unprotected_replay() {
    let tmp = Temp::new("cap");
    let (path, fights) = log_of(
        &tmp,
        &[
            (70, false, 15),
            (70, false, 15),
            (70, false, 15),
            (90, true, 15),
        ],
    );
    let facts = LogFacts::read(&path);
    // One replay's size: every cut here is the same shape.
    let one = {
        let mut probe = Store::open(MemBackend::new(), Retention::default());
        let id = probe.store(&fights[0], facts).unwrap();
        assert!(cut_into(&mut probe, &path, &id));
        probe.status().replay_bytes
    };
    let mut store = Store::open(
        MemBackend::new(),
        cfg(|r| {
            r.keep_progression = false;
            r.replay_bytes = one * 2;
        }),
    );
    let ids: Vec<String> = fights
        .iter()
        .map(|f| store.store(f, facts).unwrap())
        .collect();
    for id in &ids {
        cut_into(&mut store, &path, id);
    }
    let has: Vec<bool> = ids.iter().map(|id| store.has_replay(id)).collect();
    assert_eq!(
        has,
        [false, false, true, true],
        "two fit: the kill and the newest wipe"
    );
    assert!(store.status().replay_bytes <= one * 2);
    // At the cap a wipe older than every unprotected replay held is not
    // backfilled: the trim would only take it again.
    assert!(!store.wants_recut(&ids[0]));
    assert!(store.recuts().is_empty());
}

/// A store full to the byte still takes its newest wipe: the backfill cuts
/// it and the trim takes the oldest unprotected replay in its place, as
/// retention counts the cap (`>`), never stuck at it.
#[test]
fn a_store_at_the_cap_still_takes_its_newest_wipe() {
    let tmp = Temp::new("cap-newest");
    let (path, fights) = log_of(&tmp, &[(70, false, 15), (70, false, 15), (70, false, 15)]);
    let facts = LogFacts::read(&path);
    let one = {
        let mut probe = Store::open(MemBackend::new(), Retention::default());
        let id = probe.store(&fights[0], facts).unwrap();
        assert!(cut_into(&mut probe, &path, &id));
        probe.status().replay_bytes
    };
    let mut store = Store::open(
        MemBackend::new(),
        cfg(|r| {
            r.keep_progression = false;
            r.replay_bytes = one * 2;
        }),
    );
    let ids: Vec<String> = fights
        .iter()
        .map(|f| store.store(f, facts).unwrap())
        .collect();
    // The two older wipes fill the tier exactly; the newest never got its
    // cut (a live close whose recut has not run yet).
    assert!(cut_into(&mut store, &path, &ids[0]));
    assert!(cut_into(&mut store, &path, &ids[1]));
    assert_eq!(store.status().replay_bytes, one * 2, "exactly at the cap");
    assert!(
        store.wants_recut(&ids[2]),
        "the newest wipe is still wanted"
    );
    assert_eq!(store.recuts(), [ids[2].clone()]);
    assert!(cut_into(&mut store, &path, &ids[2]));
    let has: Vec<bool> = ids.iter().map(|id| store.has_replay(id)).collect();
    assert_eq!(has, [false, true, true], "the oldest made room");
    assert!(!store.wants_recut(&ids[0]), "and is not cut again");
}

/// An aborted fight (a pull the log abandoned) keeps no replay: it would
/// read as a wipe at 0:00. No slot, no cut, nothing backfilled.
#[test]
fn an_aborted_pull_keeps_no_replay() {
    let tmp = Temp::new("aborted");
    let (path, fights) = log_of(&tmp, &[(70, false, 15)]);
    let facts = LogFacts::read(&path);
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let mut abandoned = fights[0].clone();
    abandoned.aborted = true;
    let id = store.store(&abandoned, facts).unwrap();
    assert!(store.card(&id).unwrap().aborted);
    assert!(!store.replay_slots().contains(&id));
    assert!(!store.wants_recut(&id) && store.recuts().is_empty());
    assert!(!cut_into(&mut store, &path, &id), "offered, refused");
    assert_eq!(store.status().replays, 0);
}

/// A read-through store learns each replay's size and head without reading
/// the files whole: `MemBackend` answers both from what it holds, a seeded
/// file by its metadata and one range.
#[test]
fn a_read_through_store_sizes_replays_without_reading_them() {
    let tmp = Temp::new("over-dir");
    std::fs::create_dir_all(tmp.0.join("replay")).unwrap();
    std::fs::write(tmp.0.join("replay").join("x.bin"), b"WDRP\x01abcdefgh").unwrap();
    let mut b = MemBackend::over_dir(&tmp.0);
    assert_eq!(b.size("replay", "x.bin"), Some(13));
    assert_eq!(
        b.read_range("replay", "x.bin", 0, 5).as_deref(),
        Some(&b"WDRP\x01"[..])
    );
    assert_eq!(b.read_range("replay", "x.bin", 10, 9), None, "past the end");
    b.write("replay", "y.bin", b"WDRP").unwrap();
    assert_eq!(b.size("replay", "y.bin"), Some(4));
    assert_eq!(
        b.read_range("replay", "y.bin", 1, 2).as_deref(),
        Some(&b"DR"[..])
    );
    assert_eq!(b.size("replay", "nope.bin"), None);
}

/// `GetReplay` answers the tier's bytes — the mock cuts the fight asked for
/// as the rewrite queue would, lazily, on the first ask — and `None` for a
/// fight it does not keep.
#[test]
fn get_replay_answers_the_tier() {
    let mut mock = MockDaemon::fixture().with_history();
    assert_eq!(
        mock.history().status().replays,
        0,
        "a mock with history cuts nothing until asked"
    );
    let kill = mock
        .history()
        .cards()
        .iter()
        .find(|c| c.kind == FightKind::Encounter && c.success == Some(true))
        .unwrap()
        .id
        .clone();
    let out = mock.handle(ClientMsg::GetReplay {
        req_id: 4,
        fight_id: kill.clone(),
    });
    let Some(DaemonMsg::Replay {
        req_id: 4,
        fight_id,
        bytes: Some(bytes),
    }) = out.first()
    else {
        panic!("{out:?}");
    };
    assert_eq!(fight_id, &kill);
    let cut = replay::decode(bytes).unwrap();
    assert_eq!(
        cut.head.encounter.as_ref().map(|e| e.name.as_str()),
        Some("The Ashen Warden")
    );
    assert!(!cut.units.is_empty() && cut.posts() > 0);
    let out = mock.handle(ClientMsg::GetReplay {
        req_id: 5,
        fight_id: "nope".to_string(),
    });
    assert!(
        matches!(out.first(), Some(DaemonMsg::Replay { bytes: None, .. })),
        "{out:?}"
    );
    assert_eq!(
        mock.history().status().replays,
        1,
        "the one asked for, cut on the ask"
    );
}

// ---- the tier's formats ------------------------------------------------------

/// A format-1 replay file — a small kill as a v45 build wrote it, before
/// every floor (`proto::replay`'s own format-1 golden).
const FORMAT1: &str = "57445250011d0000000801004c024c1b036713047a2c05a6011506bb010d07c8010908d1010a07125468652056656e6f6d6f757320416279737307556c612774656b0d54616e6b2d5265616c6d2d55530a506c617965722d312d41000556656e6f6d0f44656d6f6e696320476174657761798213bc17000201b0ea01c0f7a797a368ea0f0a0701b817011014000200020306fa01010002010104000000f40301ffe80792a812bb05b0ea01880e010664c8013fe807960164d00fc701ff8827b0b71200f0ea03880e0103643202d00f0000000ba8a912d7040178a09c010a00010001d00f00030002a09c01c0b80201000101d08c01cf0f03d8360500049be90606";

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The store's files, read whole: a store opened over them is the same
/// store (its replay may then be swapped for another build's).
fn copied(from: &MemBackend) -> MemBackend {
    let mut to = MemBackend::new();
    for dir in [
        "fights",
        "rows",
        "details",
        "series",
        "replay",
        "loadouts",
        "annotations",
        "affiliations",
    ] {
        for name in from.list(dir) {
            to.write(dir, &name, &from.read(dir, &name).unwrap())
                .unwrap();
        }
    }
    to
}

/// A format-1 replay still answers, and is OLDER than this build's: the
/// rewrite queue recuts it from its log as format 2 (every floor, the boss
/// rows), as the series tier's were at v42. A later build's format is left
/// alone and answers nothing.
#[test]
fn a_format_1_replay_is_recut_as_format_2() {
    let tmp = Temp::new("format1");
    let (path, fights) = log_of(&tmp, &[(90, true, 15)]);
    let facts = LogFacts::read(&path);
    let mut first = Store::open(MemBackend::new(), Retention::default());
    let id = first.store(&fights[0], facts).unwrap();
    assert!(cut_into(&mut first, &path, &id));
    assert!(first.recuts().is_empty(), "a current replay wants nothing");
    // The same store, its replay as a format-1 build wrote it.
    let mut old = copied(first.backend());
    old.write("replay", &format!("{id}.bin"), &unhex(FORMAT1))
        .unwrap();
    let mut store = Store::open(old, Retention::default());
    assert!(store.has_replay(&id), "format 1 still answers");
    let bytes = store.replay_file(&id).unwrap();
    assert_eq!(replay::format_of(&bytes), Some(1));
    assert!(replay::decode(&bytes).is_some());
    assert!(store.rewrites().is_empty(), "its details and series stand");
    assert_eq!(
        store.recuts(),
        std::slice::from_ref(&id),
        "older: queued for its recut"
    );
    assert!(cut_into(&mut store, &path, &id));
    let bytes = store.replay_file(&id).unwrap();
    assert_eq!(replay::format_of(&bytes), Some(replay::FORMAT));
    let cut = replay::decode(&bytes).unwrap();
    let boss: Vec<replay_kind::EventKind> = cut
        .events
        .iter()
        .filter(|e| e.kind.is_boss())
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        boss,
        [
            replay_kind::EventKind::BossEngaged,
            replay_kind::EventKind::BossKilled
        ],
        "recut: the boss rows a format-1 file never held"
    );
    assert!(store.recuts().is_empty(), "and nothing more to do");
    // A later build's format: never rewritten, never answered.
    let mut newer = copied(store.backend());
    newer
        .write("replay", &format!("{id}.bin"), b"WDRP\x03\0\0\0\0")
        .unwrap();
    let store = Store::open(newer, Retention::default());
    assert!(!store.has_replay(&id));
    assert!(store.recuts().is_empty(), "a newer format is left alone");
}
