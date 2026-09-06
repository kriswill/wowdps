//! R20 through the history store (step 5, v26): the card carries
//! `absorb_wasted` (`None` when no closed shield had a known waste) and
//! `shields_unknown` per friendly player, `absorb_efficiency` derived on
//! write; the rows tier carries `shields[]` — one block per friendly
//! player with any ledger row; `stored_fight` hands the drilled player's
//! rows back and equals `derived_fight` on every view × drill; `Trend {
//! AbsorbEfficiency }` reads the percentage and SKIPS a card whose waste is
//! unknown; `RoleNight` folds the fixture's one night by role; a regrade
//! back-fills a PR #23-shaped record, pin kept; and the mock daemon's store
//! writes the same card. Numbers are
//! `crates/core/fixtures/shields.expected.md`'s.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::path::{Path, PathBuf};

use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{
    Backend, ClosedFight, DirBackend, LogFacts, MemBackend, Retention, Store,
};
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{Role, RoleNightRow, ShieldRow, View};
use wowdps_proto::history::{CardPlayer, FightCard, FightKind};
use wowdps_proto::{
    HistoryAnswer, HistoryQuery, Night, StoredFight, TrendBucket, TrendMeasure, TrendPoint,
};

const SHIELDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/shields.txt");

/// shields.txt's roster.
const PRIEST: &str = "Player-1168-0A1B2C41";
const WARRIOR: &str = "Player-1168-0A1B2C42";
const MAGE: &str = "Player-1168-0A1B2C43";
const MONK: &str = "Player-1168-0A1B2C44";
const DK: &str = "Player-1168-0A1B2C45";
const ROSTER: [&str; 5] = [PRIEST, WARRIOR, MAGE, MONK, DK];

const BOSS: &str = "Shields Test Boss";
const ENCOUNTER: u32 = 3148;
const DIFFICULTY: u32 = 16;
const KILL_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

const PWS: u32 = 17;
const ICE_BARRIER: u32 = 11_426;
const BLOOD_SHIELD: u32 = 77_535;

/// The Priest's efficiency: 65 000 absorbed / (65 000 + 19 000 wasted).
const PRIEST_EFF: f64 = 65_000.0 / 84_000.0;

/// Replay a whole log through an engine the way the tail thread would,
/// collecting every `Closed` fight.
fn closed_fights(path: &Path) -> Vec<ClosedFight> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.to_path_buf()), &mut events);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    engine.on_tail(TailEvent::Lines(lines), &mut events);
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Closed(id) => engine.take_closed(*id),
            EngineEvent::Opened(_) => None,
        })
        .collect()
}

fn with_trash() -> Retention {
    Retention {
        store_trash: true,
        ..Retention::default()
    }
}

/// shields.txt with one more line: the boar's swing again ten minutes on,
/// past the R7 gap, so the trash tail CLOSES (a finished log's last trash
/// is live at EOF, never a `Closed` fight) and the store sees both.
fn shields_log() -> (Temp, PathBuf) {
    let tmp = Temp::new("log");
    let text = std::fs::read_to_string(SHIELDS).unwrap();
    let boar = text
        .lines()
        .find(|l| l.contains("20:06:10.000-4  SWING_DAMAGE,Creature"))
        .expect("the boar's swing");
    let later = boar.replace("20:06:10.000-4", "20:16:10.000-4");
    let path = tmp.0.join("WoWCombatLog-090626.txt");
    std::fs::write(&path, format!("{text}{later}\n")).unwrap();
    (tmp, path)
}

fn stored(
    path: &Path,
    cfg: Retention,
) -> (Store<MemBackend>, Vec<ClosedFight>, LogFacts, Vec<String>) {
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    let mut store = Store::open(MemBackend::new(), cfg);
    let ids: Vec<String> = fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    assert!(!ids.is_empty(), "{} stores something", path.display());
    (store, fights, facts, ids)
}

/// shields.txt: the kill and the trash tail, both stored.
fn shields_store() -> (
    Temp,
    Store<MemBackend>,
    Vec<ClosedFight>,
    LogFacts,
    String,
    String,
) {
    let (tmp, path) = shields_log();
    let (store, fights, facts, ids) = stored(&path, with_trash());
    assert_eq!(ids.len(), 2, "the kill and the trash: {ids:?}");
    let kill = ids
        .iter()
        .find(|id| store.card(id).unwrap().name == BOSS)
        .expect("the kill")
        .clone();
    let trash = ids
        .iter()
        .find(|id| store.card(id).unwrap().kind == FightKind::Trash)
        .expect("the trash")
        .clone();
    (tmp, store, fights, facts, kill, trash)
}

fn fight<'a>(fights: &'a [ClosedFight], name: &str) -> &'a ClosedFight {
    fights
        .iter()
        .find(|f| f.segment.name == name)
        .unwrap_or_else(|| panic!("{name} closed"))
}

fn player<'a>(card: &'a FightCard, guid: &str) -> &'a CardPlayer {
    card.players
        .iter()
        .find(|p| p.guid == guid)
        .unwrap_or_else(|| panic!("{guid} on the card: {:?}", card.players))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// (absorb_wasted, shields_unknown, absorbed).
fn scalars(p: &CardPlayer) -> (Option<u64>, u32, u64) {
    (p.absorb_wasted, p.shields_unknown, p.absorbed)
}

/// (spell_id, count, applied, consumed, wasted, unknown).
fn row(r: &ShieldRow) -> (u32, u32, u64, u64, u64, u32) {
    (
        r.spell_id, r.count, r.applied, r.consumed, r.wasted, r.unknown,
    )
}

fn trend_of<B: Backend>(store: &Store<B>, guid: &str, measure: TrendMeasure) -> Vec<TrendPoint> {
    match store.answer(&HistoryQuery::Trend {
        guid: guid.to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure,
        bucket: TrendBucket::None,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::Trend(points) => points,
        other => panic!("{other:?}"),
    }
}

/// The fixture's one night, as `progression` hands it back.
fn the_night<B: Backend>(store: &Store<B>) -> Night {
    match store.answer(&HistoryQuery::Progression {
        encounter: ENCOUNTER,
        difficulty: DIFFICULTY,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::Progression { nights, .. } => {
            assert_eq!(nights.len(), 1, "{nights:?}");
            nights[0].clone()
        }
        other => panic!("{other:?}"),
    }
}

fn role_night<B: Backend>(store: &Store<B>, night: i64) -> (Night, Vec<RoleNightRow>) {
    match store.answer(&HistoryQuery::RoleNight {
        encounter: ENCOUNTER,
        difficulty: DIFFICULTY,
        night,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::RoleNight { night, rows } => (night, rows),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_card_carries_the_waste_and_the_unknown_count() {
    let (_tmp, store, _, _, kill, trash) = shields_store();
    let card = store.card(&kill).unwrap();
    assert_eq!(card.duration_ms, KILL_MS);
    // Absorbers: the waste is KNOWN, the Priest's two unknown shields
    // (the pre-pull one and the one open at the kill) counted.
    assert_eq!(scalars(player(card, PRIEST)), (Some(19_000), 2, 65_000));
    assert_eq!(scalars(player(card, MAGE)), (Some(5_000), 0, 11_000));
    assert_eq!(scalars(player(card, DK)), (Some(5_000), 0, 9_000));
    // Shield TARGETS only: no closed shield of theirs, so the waste is
    // unknown — `None`, never a 0 that would claim a perfect efficiency.
    assert_eq!(scalars(player(card, WARRIOR)), (None, 0, 0));
    assert_eq!(scalars(player(card, MONK)), (None, 0, 0));
    // The derived efficiency.
    assert!(close(
        player(card, PRIEST).absorb_efficiency().unwrap(),
        PRIEST_EFF
    ));
    assert!(close(
        player(card, MAGE).absorb_efficiency().unwrap(),
        11_000.0 / 16_000.0
    ));
    assert!(close(
        player(card, DK).absorb_efficiency().unwrap(),
        9_000.0 / 14_000.0
    ));
    assert_eq!(player(card, WARRIOR).absorb_efficiency(), None);
    assert_eq!(player(card, MONK).absorb_efficiency(), None);
    // The trash tail: nothing was shielded in it (the dead-zone shield
    // lands nowhere), so every scalar is the unknown default.
    let t = store.card(&trash).unwrap();
    for p in &t.players {
        assert_eq!(scalars(p), (None, 0, 0), "{}", p.guid);
    }
}

#[test]
fn the_rows_tier_carries_one_ledger_block_per_absorber() {
    let (_tmp, store, _, _, kill, trash) = shields_store();
    let rows = store.rows(&kill).unwrap();
    let mut guids: Vec<&str> = rows.shields.iter().map(|s| s.guid.as_str()).collect();
    guids.sort_unstable();
    let mut expect = [PRIEST, MAGE, DK];
    expect.sort_unstable();
    assert_eq!(guids, expect, "a block per absorber, none for a target");
    let of = |guid: &str| -> Vec<(u32, u32, u64, u64, u64, u32)> {
        rows.shields
            .iter()
            .find(|s| s.guid == guid)
            .unwrap()
            .rows
            .iter()
            .map(row)
            .collect()
    };
    assert_eq!(of(PRIEST), vec![(PWS, 7, 75_000, 65_000, 19_000, 2)]);
    assert_eq!(of(MAGE), vec![(ICE_BARRIER, 2, 16_000, 11_000, 5_000, 0)]);
    assert_eq!(of(DK), vec![(BLOOD_SHIELD, 1, 14_000, 9_000, 5_000, 0)]);
    // Σ rows.consumed = the card's absorbed, per player.
    let card = store.card(&kill).unwrap();
    for b in &rows.shields {
        let consumed: u64 = b.rows.iter().map(|r| r.consumed).sum();
        assert_eq!(consumed, player(card, &b.guid).absorbed, "{}", b.guid);
    }
    assert!(store.rows(&trash).unwrap().shields.is_empty());
}

#[test]
fn a_stored_fight_hands_the_drilled_players_shields_back_and_equals_derived() {
    let (_tmp, store, fights, facts, kill, trash) = shields_store();
    let sf = store
        .stored_fight(&kill, View::Healing, Some(PRIEST))
        .unwrap();
    assert_eq!(
        sf.shields.iter().map(row).collect::<Vec<_>>(),
        vec![(PWS, 7, 75_000, 65_000, 19_000, 2)]
    );
    assert_eq!(sf.shields[0].label, "Power Word: Shield");
    // Whatever the view; empty without a drill or for a target.
    let sf = store
        .stored_fight(&kill, View::Damage, Some(PRIEST))
        .unwrap();
    assert_eq!(sf.shields.len(), 1);
    assert!(
        store
            .stored_fight(&kill, View::Healing, None)
            .unwrap()
            .shields
            .is_empty()
    );
    assert!(
        store
            .stored_fight(&kill, View::Taken, Some(WARRIOR))
            .unwrap()
            .shields
            .is_empty()
    );
    // stored == derived over every view × drill on the kill (all three
    // tiers stored); the trash card stores no details tier, so only its
    // ledger half is compared (its drill differs by design since 4b).
    let trash_fight = fights
        .iter()
        .find(|f| f.segment.kind == wowdps_core::meter::SegmentKind::Trash)
        .expect("the trash closed");
    let drills: Vec<Option<&str>> = std::iter::once(None)
        .chain(ROSTER.iter().map(|g| Some(*g)))
        .collect();
    for view in [View::Damage, View::Healing, View::Taken] {
        for drill in &drills {
            let a: StoredFight = store.stored_fight(&kill, view, *drill).unwrap();
            let b = store.derived_fight(fight(&fights, BOSS), facts, view, *drill);
            assert_eq!(a, b, "{kill} {view:?} {drill:?}");
            let a = store.stored_fight(&trash, view, *drill).unwrap();
            let b = store.derived_fight(trash_fight, facts, view, *drill);
            assert_eq!(a.shields, b.shields, "{trash} {view:?} {drill:?}");
            assert_eq!(a.card, b.card);
        }
    }
}

#[test]
fn a_trend_by_absorb_efficiency_reads_the_percentage_and_skips_the_unknown() {
    let (_tmp, store, _, _, kill, trash) = shields_store();
    let points = trend_of(&store, PRIEST, TrendMeasure::AbsorbEfficiency);
    let k = points
        .iter()
        .find(|p| p.fight_id == kill)
        .expect("the kill");
    assert_eq!(k.amount, 65_000);
    assert!(close(k.per_sec, PRIEST_EFF * 100.0), "{}", k.per_sec);
    assert_eq!(k.duration_ms, KILL_MS);
    // The Priest is not on the trash card at all; the Warrior is on both
    // with an unknown waste and yields NO point on either.
    assert!(points.iter().all(|p| p.fight_id != trash));
    assert!(
        trend_of(&store, WARRIOR, TrendMeasure::AbsorbEfficiency).is_empty(),
        "an unknown waste is skipped, never 0"
    );
    assert_eq!(
        trend_of(&store, WARRIOR, TrendMeasure::Dtps).len(),
        2,
        "the Warrior does trend on another measure"
    );
    // A day bucket folds the known points only: the Mage's kill point
    // stands alone (her trash card has no waste).
    let folded = match store.answer(&HistoryQuery::Trend {
        guid: MAGE.to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure: TrendMeasure::AbsorbEfficiency,
        bucket: TrendBucket::Day,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::Trend(points) => points,
        other => panic!("{other:?}"),
    };
    assert_eq!(folded.len(), 1, "{folded:?}");
    assert_eq!(folded[0].n, 1);
    assert_eq!(folded[0].amount, 11_000);
    assert!(close(folded[0].per_sec, 11_000.0 / 16_000.0 * 100.0));
}

#[test]
fn a_role_night_folds_the_night_by_role() {
    let (_tmp, store, _, _, kill, _) = shields_store();
    let card = store.card(&kill).unwrap();
    let night = the_night(&store);
    assert_eq!(night.pulls, 1);
    assert!(night.kill);
    let (summary, rows) = role_night(&store, night.day_utc_ms);
    assert_eq!(summary, night, "the Night is progression's bucket");
    assert_eq!(rows.len(), 5, "{rows:?}");
    // Three tanks (Protection, Brewmaster, Blood), the healer, the dps —
    // in that order, tanks by measure desc then guid.
    let roles: Vec<Option<Role>> = rows.iter().map(|r| r.role).collect();
    assert_eq!(
        roles,
        vec![
            Some(Role::Tank),
            Some(Role::Tank),
            Some(Role::Tank),
            Some(Role::Healer),
            Some(Role::Dps)
        ]
    );
    for w in rows[..3].windows(2) {
        assert!(
            w[0].measure > w[1].measure || (w[0].measure == w[1].measure && w[0].guid < w[1].guid),
            "{:?}",
            w
        );
    }
    for r in &rows {
        let p = player(card, &r.guid);
        assert_eq!(r.pulls, 1);
        assert_eq!(r.name, p.name);
        assert_eq!(r.spec, p.spec.map(|s| u16::try_from(s.id()).unwrap()));
        assert_eq!(r.taken, p.taken);
        assert!(close(r.dtps, p.dtps));
        assert!(close(r.am_uptime_pct, p.am_uptime_pct(card.duration_ms)));
        assert_eq!(r.externals_given, p.externals_given);
        assert!(close(r.best, r.measure), "one pull: best = mean");
        let expect = match r.role {
            Some(Role::Tank) => p.mitigated_pct(),
            Some(Role::Healer) => p.hps,
            Some(Role::Dps) => p.effective_dps(card.duration_ms),
            None => 0.0,
        };
        assert!(
            close(r.measure, expect),
            "{}: {} vs {expect}",
            r.guid,
            r.measure
        );
    }
    let priest = rows.iter().find(|r| r.guid == PRIEST).unwrap();
    assert_eq!(priest.spec, Some(256));
    assert!(close(priest.measure, 89_000.0 / 60.0));
    assert!(close(priest.overheal_pct, 6_000.0 * 100.0 / 95_000.0));
    assert!(close(priest.absorb_efficiency.unwrap(), PRIEST_EFF));
    assert_eq!(priest.taken, 23_000);
    let mage = rows.iter().find(|r| r.guid == MAGE).unwrap();
    assert_eq!(mage.spec, Some(63));
    assert!(close(mage.measure, 186_000.0 / 60.0));
    assert!(close(mage.absorb_efficiency.unwrap(), 11_000.0 / 16_000.0));
    assert_eq!(mage.overheal_pct, 0.0);
    let dk = rows.iter().find(|r| r.guid == DK).unwrap();
    assert!(close(dk.absorb_efficiency.unwrap(), 9_000.0 / 14_000.0));
    for guid in [WARRIOR, MONK] {
        let r = rows.iter().find(|r| r.guid == guid).unwrap();
        assert_eq!(r.absorb_efficiency, None, "{guid}: an unknown waste");
        assert_eq!(r.overheal_pct, 0.0);
    }
    // Another night, or another boss: the Night with 0 pulls and no rows.
    let (empty, rows) = role_night(&store, night.day_utc_ms + DAY_MS);
    assert!(rows.is_empty());
    assert_eq!(
        empty,
        Night {
            day_utc_ms: night.day_utc_ms + DAY_MS,
            pulls: 0,
            kill: false,
            kills: 0,
            best_pct: None,
            tz_min: None,
        }
    );
    match store.answer(&HistoryQuery::RoleNight {
        encounter: ENCOUNTER + 1,
        difficulty: DIFFICULTY,
        night: night.day_utc_ms,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::RoleNight { night: n, rows } => {
            assert_eq!(n.pulls, 0);
            assert!(rows.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

/// Cut every `"key":<scalar>` (with its comma) out of a one-line card.
fn strip_keys(doc: &str, keys: &[&str]) -> String {
    let mut stripped = doc.to_string();
    for key in keys {
        while let Some(at) = stripped.find(&format!("\"{key}\":")) {
            let end = stripped[at..]
                .find(['}', ','])
                .map(|i| at + i)
                .expect("a value ends");
            let cut = if stripped.as_bytes()[end] == b',' {
                end + 1
            } else {
                end
            };
            stripped.replace_range(at..cut, "");
        }
    }
    stripped.replace(",}", "}")
}

/// Cut a top-level `,"key":[…]` array out of a one-line rows document.
fn strip_array(doc: &str, key: &str) -> String {
    let needle = format!(",\"{key}\":[");
    let at = doc
        .find(&needle)
        .unwrap_or_else(|| panic!("{key} in {doc}"));
    let bytes = doc.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut end = None;
    for (i, &b) in bytes.iter().enumerate().skip(at + needle.len() - 1) {
        match b {
            b'"' if !in_str => in_str = true,
            b'"' if in_str && bytes[i - 1] != b'\\' => in_str = false,
            b'[' if !in_str => depth += 1,
            b']' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.expect("the array closes");
    format!("{}{}", &doc[..at], &doc[end..])
}

const CARD_KEYS: [&str; 3] = ["absorb_wasted", "shields_unknown", "absorb_efficiency"];

#[test]
fn a_regrade_back_fills_a_pre_5_record_and_keeps_its_pin() {
    let (_tmp, store, fights, facts, kill, _) = shields_store();
    let kill_fight = fight(&fights, BOSS);
    let file = format!("{kill}.json");
    let fresh_card = String::from_utf8(store.backend().read("fights", &file).unwrap()).unwrap();
    let fresh_rows = String::from_utf8(store.backend().read("rows", &file).unwrap()).unwrap();
    for key in CARD_KEYS {
        assert!(fresh_card.contains(&format!("\"{key}\":")), "{key} written");
    }
    assert!(fresh_card.contains("\"absorb_wasted\":19000"));
    assert!(fresh_card.contains("\"absorb_wasted\":null"), "a target's");
    assert!(fresh_card.contains("\"shields_unknown\":2"));
    assert!(fresh_rows.contains(",\"shields\":["));

    // The kill written the way step 4b wrote it: the three card keys and
    // the rows array surgically removed, nothing else touched.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "details", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let stripped = strip_keys(&fresh_card, &CARD_KEYS);
    for key in CARD_KEYS {
        assert!(
            !stripped.contains(&format!("\"{key}\"")),
            "{key}: {stripped}"
        );
    }
    assert!(
        stripped.contains("\"am_uptime_pct\":"),
        "v25 keys untouched"
    );
    backend.write("fights", &file, stripped.as_bytes()).unwrap();
    let rows_stripped = strip_array(&fresh_rows, "shields");
    assert!(!rows_stripped.contains("\"shields\""), "{rows_stripped}");
    assert!(rows_stripped.contains("\"uptime\":["));
    assert!(rows_stripped.contains("\"coarse\":["));
    backend
        .write("rows", &file, rows_stripped.as_bytes())
        .unwrap();

    let mut reopened = Store::open(backend, with_trash());
    assert_eq!(reopened.corrupt(), 0);
    let old = reopened.card(&kill).expect("the pre-5 card still reads");
    for guid in ROSTER {
        assert_eq!(scalars(player(old, guid)).0, None, "{guid}: unknown");
        assert_eq!(scalars(player(old, guid)).1, 0);
        assert_eq!(player(old, guid).absorb_efficiency(), None);
    }
    assert_eq!(player(old, PRIEST).absorbed, 65_000, "v23 keys untouched");
    let sf = reopened
        .stored_fight(&kill, View::Healing, Some(PRIEST))
        .unwrap();
    assert!(sf.shields.is_empty(), "a pre-5 rows file has no ledger");
    assert!(
        trend_of(&reopened, PRIEST, TrendMeasure::AbsorbEfficiency).is_empty(),
        "a pre-5 card trends no point"
    );
    let (_, rows) = role_night(&reopened, the_night(&reopened).day_utc_ms);
    assert!(rows.iter().all(|r| r.absorb_efficiency.is_none()));

    assert!(reopened.pin(&kill, true));
    assert_eq!(
        reopened.regrade(kill_fight, facts).as_deref(),
        Some(kill.as_str())
    );
    let card = reopened.card(&kill).unwrap();
    assert!(card.pinned, "the pin survived the rewrite");
    assert_eq!(scalars(player(card, PRIEST)), (Some(19_000), 2, 65_000));
    assert_eq!(scalars(player(card, WARRIOR)), (None, 0, 0));
    let rewritten = String::from_utf8(reopened.backend().read("fights", &file).unwrap()).unwrap();
    assert_eq!(
        rewritten,
        fresh_card.replace("\"pinned\":false", "\"pinned\":true"),
        "byte-for-byte the live write, pin aside"
    );
    assert_eq!(
        String::from_utf8(reopened.backend().read("rows", &file).unwrap()).unwrap(),
        fresh_rows,
        "the rows tier is back to its full shape"
    );
    let sf = reopened
        .stored_fight(&kill, View::Healing, Some(PRIEST))
        .unwrap();
    assert_eq!(sf.shields.len(), 1);
    assert!(close(
        trend_of(&reopened, PRIEST, TrendMeasure::AbsorbEfficiency)
            .iter()
            .find(|p| p.fight_id == kill)
            .unwrap()
            .per_sec,
        PRIEST_EFF * 100.0
    ));
}

/// A scratch directory under the system temp dir, removed on drop — one
/// per call (tests run in parallel and each wants its own).
struct Temp(PathBuf);

static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "wowdps-shields-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
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

#[test]
fn the_real_store_round_trips_the_scalars_and_the_ledger() {
    // Write shields.txt through a directory-backed store, reopen it from
    // the files alone, and compare with the in-memory answers: what the
    // JSON carries is what the daemon serves after a restart.
    let tmp = Temp::new("roundtrip");
    let (_log, path) = shields_log();
    let path = path.as_path();
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    let mut disk = Store::open(DirBackend::new(tmp.0.clone()), with_trash());
    let ids: Vec<String> = fights.iter().filter_map(|f| disk.store(f, facts)).collect();
    assert_eq!(ids.len(), 2);
    drop(disk);

    let reopened = Store::open(DirBackend::new(tmp.0.clone()), with_trash());
    assert_eq!(reopened.corrupt(), 0);
    let (_tmp2, mem, _, _, kill, trash) = shields_store();
    for id in [&kill, &trash] {
        let a = reopened.card(id).expect("read back");
        let b = mem.card(id).unwrap();
        assert_eq!(a.players, b.players, "{id}: every scalar round-trips");
        assert_eq!(
            reopened.rows(id).unwrap().shields,
            mem.rows(id).unwrap().shields,
            "{id}: the ledger round-trips"
        );
        for guid in ROSTER {
            for view in [View::Taken, View::Healing, View::Damage] {
                let a: StoredFight = reopened.stored_fight(id, view, Some(guid)).unwrap();
                let b = mem.stored_fight(id, view, Some(guid)).unwrap();
                assert_eq!(a, b, "{id} {guid} {view:?}");
            }
        }
    }
    let night = the_night(&mem).day_utc_ms;
    assert_eq!(
        role_night(&reopened, night),
        role_night(&mem, night),
        "the night folds the same off the files"
    );
    // The derived efficiency is on the file (SQL reads it), never read back.
    let file = std::fs::read_to_string(tmp.0.join("fights").join(format!("{kill}.json"))).unwrap();
    assert!(file.contains("\"absorb_efficiency\":0.77"), "{file}");
    assert!(file.contains("\"absorb_efficiency\":null"), "{file}");
}

#[test]
fn the_mock_daemons_store_writes_the_same_card() {
    // The in-process fake daemon feeds every Closed into its own store —
    // the seam the GUI and TUI tests build on — so what it holds must be
    // what the engine path above holds.
    let mock = MockDaemon::fixture_at(Path::new(SHIELDS)).with_history();
    let store = mock.history();
    let card = store
        .cards()
        .iter()
        .find(|c| c.name == BOSS && c.kind == FightKind::Encounter)
        .expect("the mock stored the kill");
    assert_eq!(scalars(player(card, PRIEST)), (Some(19_000), 2, 65_000));
    assert_eq!(scalars(player(card, MAGE)), (Some(5_000), 0, 11_000));
    assert_eq!(scalars(player(card, MONK)), (None, 0, 0));
    let rows = store.rows(&card.id).unwrap();
    assert_eq!(rows.shields.len(), 3);
    let (_, night_rows) = role_night(store, the_night(store).day_utc_ms);
    assert_eq!(night_rows.len(), 5);
    assert!(close(
        night_rows
            .iter()
            .find(|r| r.guid == PRIEST)
            .unwrap()
            .absorb_efficiency
            .unwrap(),
        PRIEST_EFF
    ));
}
