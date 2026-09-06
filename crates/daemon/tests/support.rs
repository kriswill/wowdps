//! R19 and the R2 amendment through the history store (step 3b, v23): the
//! card carries the healing split (`overheal` / `absorbed`), the damage
//! halves of the support ledger and healing received; a supporter the log
//! only ever trails with joins the roster so Σ `effective` over a card is Σ
//! `damage`; the rows tier carries one `PlayerSupport` block per player
//! with support and `stored_fight` / `derived_fight` hand the drilled
//! player's block back on every view; `effective_dps` is `dps` bit for bit
//! wherever there is no support; `Trend { EffectiveDps }` reads the
//! derived rate; and a regrade back-fills a PR #19-shaped card + rows,
//! pin kept. Numbers are `crates/core/fixtures/support.expected.md`'s
//! (and `sample.expected.md`'s addendum for the unnamed supporter).

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
use wowdps_model::View;
use wowdps_proto::history::{CardPlayer, FightCard, PlayerSupport};
use wowdps_proto::{
    HistoryAnswer, HistoryQuery, StoredFight, TrendBucket, TrendMeasure, TrendPoint,
};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const TAKEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");
const SUPPORT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/support.txt");

/// support.txt's roster.
const EVOKER: &str = "Player-1168-0A1B2C21";
const MAGE: &str = "Player-1168-0A1B2C22";
const WARRIOR: &str = "Player-1168-0A1B2C23";
const PRIEST: &str = "Player-1168-0A1B2C24";

/// sample.txt: the hunter whose Aimed Shot carries a support twin, and the
/// guid that twin trails with — named nowhere, on no view's rows.
const KAELTHAR: &str = "Player-1168-0A1B2C03";
const UNNAMED: &str = "Player-1168-0A1B2C04";

const BOSS: &str = "Support Test Boss";

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

/// Trash too: the fixture's tail is the supporter-only-row case.
fn with_trash() -> Retention {
    Retention {
        store_trash: true,
        ..Retention::default()
    }
}

/// Every fight of `path` through a fresh in-memory store; the ids in
/// store order.
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

/// support.txt with one more line: the boar's swing again ten minutes on,
/// past the R7 gap, so the trash tail CLOSES (a finished log's last trash
/// is live at EOF, never a `Closed` fight) — the supporter-only-row case
/// then reaches the store. Written under the temp dir; the copy is what
/// the store reads.
fn support_log() -> (Temp, PathBuf) {
    let tmp = Temp::new("log");
    let text = std::fs::read_to_string(SUPPORT).unwrap();
    let boar = text
        .lines()
        .find(|l| l.contains("22:10:02.000-4  SWING_DAMAGE,Creature"))
        .expect("the boar's swing");
    let later = boar.replace("22:10:02.000-4", "22:20:00.000-4");
    let path = tmp.0.join("WoWCombatLog-090426.txt");
    std::fs::write(
        &path,
        format!(
            "{text}{later}
"
        ),
    )
    .unwrap();
    (tmp, path)
}

/// support.txt: the kill and the trash tail, both stored.
fn support_store() -> (
    Store<MemBackend>,
    Vec<ClosedFight>,
    LogFacts,
    String,
    String,
) {
    let (_tmp, path) = support_log();
    let (store, fights, facts, ids) = stored(&path, with_trash());
    assert_eq!(ids.len(), 2, "the kill and the trash: {ids:?}");
    let kill = ids
        .iter()
        .find(|id| store.card(id).unwrap().name == BOSS)
        .expect("the kill")
        .clone();
    let trash = ids
        .iter()
        .find(|id| store.card(id).unwrap().kind == wowdps_proto::history::FightKind::Trash)
        .expect("the trash")
        .clone();
    (store, fights, facts, kill, trash)
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

/// (given_damage, given_healing, received_damage, received_healing).
fn scalars(s: &PlayerSupport) -> (u64, u64, u64, u64) {
    (
        s.given_damage,
        s.given_healing,
        s.received_damage,
        s.received_healing,
    )
}

#[test]
fn the_card_carries_the_healing_split_and_the_support_scalars() {
    let (store, _, _, kill, _) = support_store();
    let card = store.card(&kill).expect("the kill's card");
    assert_eq!(card.duration_ms, 60_000);
    assert_eq!(card.players.len(), 4, "{:?}", card.players);

    // support.expected.md, segment 1: (damage, support_given,
    // support_received, effective, healed_received, self_healed,
    // overheal, absorbed).
    for (guid, damage, given, received, effective, healed, selfh, overheal, absorbed) in [
        (
            EVOKER, 69_500u64, 23_900u64, 7_500u64, 85_900u64, 10_000u64, 0u64, 0u64, 0u64,
        ),
        (MAGE, 271_000, 0, 1_650, 269_350, 5_000, 0, 0, 0),
        (WARRIOR, 242_000, 0, 14_750, 227_250, 50_000, 0, 0, 0),
        (PRIEST, 0, 0, 0, 0, 13_000, 13_000, 16_000, 15_000),
    ] {
        let p = player(card, guid);
        assert_eq!(p.damage, damage, "{guid} damage");
        assert_eq!(
            (p.support_given, p.support_received),
            (given, received),
            "{guid} support"
        );
        assert_eq!(p.effective(), effective, "{guid} effective");
        assert!(
            close(p.effective_dps(card.duration_ms), effective as f64 / 60.0),
            "{guid} effective_dps {}",
            p.effective_dps(card.duration_ms)
        );
        assert_eq!(
            (p.healed_received, p.self_healed),
            (healed, selfh),
            "{guid} healed"
        );
        assert_eq!(
            (p.overheal, p.absorbed),
            (overheal, absorbed),
            "{guid} split"
        );
    }
    // The healer's split against her row: absorbed ≤ healing, and the
    // effective part is the row minus the absorb credit.
    let priest = player(card, PRIEST);
    assert_eq!(priest.healing, 88_000);
    assert!(priest.absorbed <= priest.healing);
    assert!(
        close(priest.effective_dps(card.duration_ms), 0.0),
        "a healer with no damage has no effective rate"
    );
    // The Evoker's rate is its contribution, above its raw dps.
    let evoker = player(card, EVOKER);
    assert!(close(evoker.dps, 69_500.0 / 60.0));
    assert!(evoker.effective_dps(card.duration_ms) > evoker.dps);
    // The Mage's is below its raw dps — the shares are the Evoker's.
    let mage = player(card, MAGE);
    assert!(mage.effective_dps(card.duration_ms) < mage.dps);

    // The identity the roster gap would break: Σ effective = Σ damage.
    let effective: u64 = card.players.iter().map(CardPlayer::effective).sum();
    let damage: u64 = card.players.iter().map(|p| p.damage).sum();
    assert_eq!((effective, damage), (582_500, 582_500));
    // And Σ given = Σ received (damage shares), every share on a player.
    assert_eq!(
        card.players.iter().map(|p| p.support_given).sum::<u64>(),
        card.players.iter().map(|p| p.support_received).sum::<u64>()
    );
}

#[test]
fn the_rows_tier_carries_one_block_per_player_with_support() {
    let (store, fights, _, kill, _) = support_store();
    let seg = &fight(&fights, BOSS).segment;
    let rows = store.rows(&kill).expect("the rows tier");
    let block = |guid: &str| {
        rows.support
            .iter()
            .find(|s| s.guid == guid)
            .unwrap_or_else(|| panic!("{guid} has a block: {:?}", rows.support))
    };

    // Four blocks: the Evoker gave, the Mage and the Warrior received
    // damage shares, the Priest received the heal shares — both ride HER
    // heals (l.39 Fate Mirror 2 000 + l.41 Shifting Sands 100: `received`
    // is keyed by the line's SOURCE, the healer, as the TSV has it; the
    // .md's prose table puts the 2 000 on the Warrior and is stale).
    assert_eq!(rows.support.len(), 4, "{:?}", rows.support);
    let evoker = block(EVOKER);
    assert_eq!(scalars(evoker), (23_900, 2_100, 7_500, 0));
    assert_eq!(scalars(block(MAGE)), (0, 0, 1_650, 0));
    assert_eq!(scalars(block(WARRIOR)), (0, 0, 14_750, 0));
    assert_eq!(scalars(block(PRIEST)), (0, 0, 0, 2_100));

    // The Evoker's targets: the Mage (with the Water Elemental's 90 folded
    // onto her), the Warrior, the Priest (heal share only) and the Evoker
    // herself (the twice-logged Bombardments), sorted by damage share desc.
    assert_eq!(evoker.targets, seg.support_targets(EVOKER), "verbatim");
    let target = |guid: &str| {
        evoker
            .targets
            .iter()
            .find(|r| r.key == guid)
            .unwrap_or_else(|| panic!("{guid} is a target: {:?}", evoker.targets))
    };
    assert_eq!((target(MAGE).amount, target(MAGE).extra), (1_650, 0));
    assert_eq!(target(MAGE).count, 5, "five shares, one the pet's");
    assert_eq!((target(WARRIOR).amount, target(WARRIOR).extra), (14_750, 0));
    assert_eq!((target(EVOKER).amount, target(EVOKER).extra), (7_500, 0));
    assert_eq!((target(PRIEST).amount, target(PRIEST).extra), (0, 2_100));
    assert!(
        !evoker.targets.iter().any(|r| r.label == "Water Elemental"),
        "the pet folds onto the Mage"
    );
    assert_eq!(evoker.targets[0].key, WARRIOR, "sorted by damage share");
    assert_eq!(
        evoker.targets.iter().map(|r| r.amount).sum::<u64>(),
        evoker.given_damage,
        "the targets partition the given damage"
    );
    assert_eq!(
        evoker.targets.iter().map(|r| r.extra).sum::<u64>(),
        evoker.given_healing
    );
    // Received-only players have no targets.
    for guid in [MAGE, WARRIOR, PRIEST] {
        assert!(block(guid).targets.is_empty(), "{guid} supported nobody");
    }
}

#[test]
fn a_stored_fight_carries_the_drilled_players_block_on_every_view() {
    let (store, fights, facts, kill, trash) = support_store();
    let kill_fight = fight(&fights, BOSS);
    let rows = store.rows(&kill).unwrap();
    let evoker_block = rows
        .support
        .iter()
        .find(|s| s.guid == EVOKER)
        .cloned()
        .expect("the Evoker's block");
    let priest_block = rows
        .support
        .iter()
        .find(|s| s.guid == PRIEST)
        .cloned()
        .expect("the Priest's block");

    // Whatever the view, the drill hands the block back; no drill, none.
    for view in [View::Damage, View::Healing, View::Taken, View::Deaths] {
        let sf = store.stored_fight(&kill, view, Some(EVOKER)).unwrap();
        assert_eq!(sf.tier, 3);
        assert_eq!(sf.support.as_ref(), Some(&evoker_block), "{view:?}");
        assert_eq!(
            store
                .stored_fight(&kill, view, Some(PRIEST))
                .unwrap()
                .support
                .as_ref(),
            Some(&priest_block),
            "{view:?}: the Priest received a heal share"
        );
        assert!(
            store
                .stored_fight(&kill, view, None)
                .unwrap()
                .support
                .is_none(),
            "{view:?}: no drill, no block"
        );
    }
    // A player the ledger never names: the Warrior in the trash tail (only
    // the Mage's Fireball carried a share there).
    let tail = store
        .stored_fight(&trash, View::Damage, Some(WARRIOR))
        .unwrap();
    assert!(tail.support.is_none(), "{:?}", tail.support);
    assert_eq!(
        store
            .stored_fight(&trash, View::Damage, Some(EVOKER))
            .unwrap()
            .support
            .map(|s| scalars(&s)),
        Some((80, 0, 0, 0))
    );

    // `derived_fight` answers from the same extract: identical block.
    let derived = store.derived_fight(kill_fight, facts, View::Healing, Some(EVOKER));
    assert_eq!(derived.support.as_ref(), Some(&evoker_block));
    assert!(
        store
            .derived_fight(kill_fight, facts, View::Damage, None)
            .support
            .is_none()
    );

    // Demote the details tier: the block lives on the rows tier, so tier
    // 2 answers identically.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let demoted = Store::open(backend, with_trash());
    assert!(!demoted.has_details(&kill));
    let sf = demoted
        .stored_fight(&kill, View::Damage, Some(EVOKER))
        .unwrap();
    assert_eq!(sf.tier, 2);
    assert_eq!(sf.support.as_ref(), Some(&evoker_block));

    // Card only (rows gone): `None`, the tier says why.
    let mut backend = MemBackend::new();
    for name in store.backend().list("fights") {
        backend
            .write(
                "fights",
                &name,
                &store.backend().read("fights", &name).unwrap(),
            )
            .unwrap();
    }
    let card_only = Store::open(backend, with_trash());
    let sf: StoredFight = card_only
        .stored_fight(&kill, View::Damage, Some(EVOKER))
        .unwrap();
    assert_eq!(sf.tier, 1);
    assert!(sf.support.is_none());
}

#[test]
fn effective_dps_is_dps_bit_for_bit_wherever_there_is_no_support() {
    // taken.txt has no support line at all; sample.txt has exactly one
    // pair (the Ashen Warden), so every other player of every other fight
    // is the no-support case — and that pair is the proof the equality is
    // not vacuous.
    let mut checked = 0;
    let mut differing = Vec::new();
    for path in [TAKEN, SAMPLE] {
        let (store, _, _, ids) = stored(Path::new(path), with_trash());
        for id in &ids {
            let c = store.card(id).unwrap();
            for p in &c.players {
                if p.support_given == 0 && p.support_received == 0 {
                    assert_eq!(p.effective(), p.damage, "{path} {id} {}", p.guid);
                    assert_eq!(
                        p.effective_dps(c.duration_ms).to_bits(),
                        p.dps.to_bits(),
                        "{path} {id} {}: {} vs {}",
                        p.guid,
                        p.effective_dps(c.duration_ms),
                        p.dps
                    );
                    checked += 1;
                } else {
                    differing.push((c.name.clone(), p.guid.clone()));
                }
            }
            // The roster-gap guard on every card, support or not.
            assert_eq!(
                c.players.iter().map(CardPlayer::effective).sum::<u64>(),
                c.players.iter().map(|p| p.damage).sum::<u64>(),
                "{path} {id}: Σ effective = Σ damage"
            );
        }
    }
    assert!(checked >= 12, "{checked} players checked");
    differing.sort();
    assert_eq!(
        differing,
        vec![
            ("The Ashen Warden".to_string(), KAELTHAR.to_string()),
            ("The Ashen Warden".to_string(), UNNAMED.to_string()),
        ],
        "the one support pair in sample.txt"
    );
}

#[test]
fn a_supporter_with_no_meter_row_is_on_the_card() {
    // sample.expected.md's addendum: the RANGE_DAMAGE_SUPPORT twin trails
    // a guid that appears nowhere else — no name, no row on any view.
    let (store, fights, _, ids) = stored(Path::new(SAMPLE), Retention::default());
    let id = ids
        .iter()
        .find(|id| store.card(id).unwrap().name == "The Ashen Warden")
        .unwrap();
    let card = store.card(id).unwrap();
    let seg = &fight(&fights, "The Ashen Warden").segment;
    for view in [View::Damage, View::Healing, View::Taken, View::Deaths] {
        assert!(
            !seg.rows(view).iter().any(|r| r.key == UNNAMED),
            "{view:?}: the supporter has no row"
        );
    }
    let p = player(card, UNNAMED);
    assert_eq!(
        (p.damage, p.support_given, p.support_received),
        (0, 29_400, 0)
    );
    assert_eq!(p.effective(), 29_400);
    assert!(close(p.effective_dps(card.duration_ms), 29_400.0 / 60.0));
    assert!(close(p.dps, 0.0), "no Damage row, no dps");
    assert_eq!(p.name, UNNAMED, "unnamed in the log: the guid is the label");
    assert!(!p.enemy);
    assert_eq!((p.class, p.spec), (None, None));
    assert_eq!(p.healing, 0);
    assert!(p.loadout.is_none() && !p.logged);
    assert_eq!(
        card.players.last().map(|p| p.guid.as_str()),
        Some(UNNAMED),
        "joins after the four views' union"
    );
    let hunter = player(card, KAELTHAR);
    assert_eq!((hunter.damage, hunter.support_received), (167_200, 29_400));
    assert_eq!(hunter.effective(), 137_800);
    assert_eq!(
        card.players.iter().map(CardPlayer::effective).sum::<u64>(),
        364_670
    );
    assert_eq!(card.players.iter().map(|p| p.damage).sum::<u64>(), 364_670);
    // The rows tier: the supporter's block with the hunter as its one
    // target; the hunter's received-only block.
    let rows = store.rows(id).unwrap();
    let sup = rows.support.iter().find(|s| s.guid == UNNAMED).unwrap();
    assert_eq!(scalars(sup), (29_400, 0, 0, 0));
    assert_eq!(sup.targets.len(), 1);
    assert_eq!(
        (sup.targets[0].key.as_str(), sup.targets[0].amount),
        (KAELTHAR, 29_400)
    );
    assert_eq!(
        rows.support
            .iter()
            .find(|s| s.guid == KAELTHAR)
            .map(scalars),
        Some((0, 0, 29_400, 0))
    );
    // The friendly set grew, so `content` differs from a set without the
    // supporter — while the id is the log + start.
    let without: Vec<&str> = card
        .players
        .iter()
        .filter(|p| !p.enemy && p.guid != UNNAMED)
        .map(|p| p.guid.as_str())
        .collect();
    assert_ne!(
        card.content,
        wowdps_proto::history::content_id(card.encounter, card.start_utc_ms, without),
    );
    // The other three fights carry no support at all: no blocks written.
    for other in ids.iter().filter(|i| *i != id) {
        assert!(store.rows(other).unwrap().support.is_empty(), "{other}");
    }

    // support.txt's trash tail is the same shape with a NAMED supporter:
    // damage 0, given 80, effective 80.
    let (store, _, _, _, trash) = support_store();
    let tail = store.card(&trash).unwrap();
    let e = player(tail, EVOKER);
    assert_eq!((e.damage, e.support_given, e.effective()), (0, 80, 80));
    assert_eq!(e.name, "Vessyra-Nebula-US");
    assert_eq!(e.spec.map(|s| s.id()), Some(1473), "COMBATANT_INFO's spec");
    assert_eq!(
        tail.players.iter().map(CardPlayer::effective).sum::<u64>(),
        tail.players.iter().map(|p| p.damage).sum::<u64>()
    );
}

fn trend_of(store: &Store<MemBackend>, guid: &str, measure: TrendMeasure) -> Vec<TrendPoint> {
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

#[test]
fn a_trend_by_effective_dps_reads_the_derived_rate() {
    let (store, _, _, kill, trash) = support_store();

    // The Evoker: two stored fights (the kill and the trash tail), newest
    // first; amount = effective, per_sec = effective over the duration.
    let points = trend_of(&store, EVOKER, TrendMeasure::EffectiveDps);
    assert_eq!(points.len(), 2, "{points:?}");
    let by_id = |id: &str| points.iter().find(|p| p.fight_id == id).unwrap();
    let k = by_id(&kill);
    assert_eq!((k.amount, k.duration_ms), (85_900, 60_000));
    assert!(close(k.per_sec, 85_900.0 / 60.0), "{}", k.per_sec);
    assert_eq!(k.spec, Some(1473));
    let t = by_id(&trash);
    assert_eq!((t.amount, t.duration_ms), (80, 2_000));
    assert!(close(t.per_sec, 40.0), "{}", t.per_sec);
    assert_eq!(points[0].fight_id, trash, "newest first");

    // Raw dps stays raw and reachable beside it.
    let raw = trend_of(&store, EVOKER, TrendMeasure::Dps);
    assert_eq!(
        raw.iter().find(|p| p.fight_id == kill).unwrap().amount,
        69_500
    );

    // The Mage: below its raw rate by the shares.
    let mage = trend_of(&store, MAGE, TrendMeasure::EffectiveDps);
    let k = mage.iter().find(|p| p.fight_id == kill).unwrap();
    assert_eq!(k.amount, 269_350);
    assert!(close(k.per_sec, 269_350.0 / 60.0));
    let raw = trend_of(&store, MAGE, TrendMeasure::Dps);
    let r = raw.iter().find(|p| p.fight_id == kill).unwrap();
    assert!(r.per_sec > k.per_sec);
    assert_eq!(r.amount, 271_000);

    // A Day bucket folds `per_sec` as a running MEAN, `amount` as a sum.
    let day = match store.answer(&HistoryQuery::Trend {
        guid: EVOKER.to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure: TrendMeasure::EffectiveDps,
        bucket: TrendBucket::Day,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::Trend(points) => points,
        other => panic!("{other:?}"),
    };
    assert_eq!(day.len(), 1);
    assert_eq!((day[0].amount, day[0].n), (85_980, 2));
    assert!(close(day[0].per_sec, (85_900.0 / 60.0 + 40.0) / 2.0));
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

const CARD_KEYS: [&str; 7] = [
    "overheal",
    "absorbed",
    "support_given",
    "support_received",
    "healed_received",
    "self_healed",
    "effective_dps",
];

#[test]
fn a_regrade_back_fills_a_pre_3b_record_and_keeps_its_pin() {
    let (store, fights, facts, kill, _) = support_store();
    let kill_fight = fight(&fights, BOSS);
    let file = format!("{kill}.json");
    let fresh_card = String::from_utf8(store.backend().read("fights", &file).unwrap()).unwrap();
    let fresh_rows = String::from_utf8(store.backend().read("rows", &file).unwrap()).unwrap();
    for key in CARD_KEYS {
        assert!(fresh_card.contains(&format!("\"{key}\":")), "{key} written");
    }
    assert!(fresh_rows.contains(",\"support\":["));

    // Copy the store into a new backend with the kill written the way PR
    // #19 wrote it: the seven card keys and the whole `support` array
    // surgically removed, nothing else touched.
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
        stripped.contains("\"mitigated_pct\":"),
        "v22 keys untouched"
    );
    assert!(stripped.len() < fresh_card.len());
    backend.write("fights", &file, stripped.as_bytes()).unwrap();
    let at = fresh_rows
        .find(",\"support\":[")
        .expect("the support array");
    let end = fresh_rows.rfind('}').expect("the object closes");
    let rows_stripped = format!("{}{}", &fresh_rows[..at], &fresh_rows[end..]);
    assert!(!rows_stripped.contains("\"support\""), "{rows_stripped}");
    assert!(rows_stripped.contains("\"mitigation\":["));
    backend
        .write("rows", &file, rows_stripped.as_bytes())
        .unwrap();

    let mut reopened = Store::open(backend, with_trash());
    let old = reopened.card(&kill).expect("the pre-3b card still reads");
    assert_eq!(old.id, kill, "the id is the log + start, never the content");
    assert_eq!(
        old.players.len(),
        4,
        "the Evoker has rows here: no roster gap"
    );
    for guid in [EVOKER, MAGE, WARRIOR, PRIEST] {
        let p = player(old, guid);
        assert_eq!(
            (
                p.overheal,
                p.absorbed,
                p.support_given,
                p.support_received,
                p.healed_received,
                p.self_healed
            ),
            (0, 0, 0, 0, 0, 0),
            "{guid}: six zeros"
        );
        // A PR #19 card: `effective` is `damage`, the rate its raw dps.
        assert_eq!(p.effective(), p.damage, "{guid}");
        assert_eq!(
            p.effective_dps(old.duration_ms).to_bits(),
            p.dps.to_bits(),
            "{guid}"
        );
    }
    assert!(
        reopened
            .stored_fight(&kill, View::Damage, Some(EVOKER))
            .unwrap()
            .support
            .is_none(),
        "a pre-3b rows file has no block to hand back"
    );
    // The rows themselves still serve: this is a back-fill, not a repair.
    assert_eq!(
        reopened
            .stored_fight(&kill, View::Damage, None)
            .unwrap()
            .rows,
        kill_fight.segment.rows(View::Damage)
    );
    assert_eq!(
        trend_of(&reopened, EVOKER, TrendMeasure::EffectiveDps)
            .iter()
            .find(|p| p.fight_id == kill)
            .map(|p| p.amount),
        Some(69_500),
        "a pre-3b card trends its raw damage"
    );

    assert!(reopened.pin(&kill, true));
    assert_eq!(
        reopened.regrade(kill_fight, facts).as_deref(),
        Some(kill.as_str())
    );
    let card = reopened.card(&kill).unwrap();
    assert!(card.pinned, "the pin survived the rewrite");
    assert_eq!(card.id, kill);
    let e = player(card, EVOKER);
    assert_eq!((e.support_given, e.support_received), (23_900, 7_500));
    assert_eq!(e.effective(), 85_900);
    let h = player(card, PRIEST);
    assert_eq!(
        (h.overheal, h.absorbed, h.self_healed),
        (16_000, 15_000, 13_000)
    );
    assert_eq!(player(card, WARRIOR).healed_received, 50_000);
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
        .stored_fight(&kill, View::Damage, Some(EVOKER))
        .unwrap();
    let block = sf.support.expect("the back-filled block");
    assert_eq!(scalars(&block), (23_900, 2_100, 7_500, 0));
    assert_eq!(block.targets, kill_fight.segment.support_targets(EVOKER));
    assert_eq!(
        trend_of(&reopened, EVOKER, TrendMeasure::EffectiveDps)
            .iter()
            .find(|p| p.fight_id == kill)
            .map(|p| p.amount),
        Some(85_900)
    );
}

/// A scratch directory under the system temp dir, removed on drop — one
/// per call (tests run in parallel and each wants its own).
struct Temp(PathBuf);

static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "wowdps-support-{tag}-{}-{}",
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
fn the_real_store_round_trips_the_card_and_the_block_through_its_files() {
    // Write support.txt through a directory-backed store, reopen it from
    // the files alone, and compare with the in-memory answers: what the
    // JSON carries is what the daemon serves after a restart.
    let tmp = Temp::new("roundtrip");
    let (_log, path) = support_log();
    let path = path.as_path();
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    let mut disk = Store::open(DirBackend::new(tmp.0.clone()), with_trash());
    let ids: Vec<String> = fights.iter().filter_map(|f| disk.store(f, facts)).collect();
    assert_eq!(ids.len(), 2);
    drop(disk);

    let reopened = Store::open(DirBackend::new(tmp.0.clone()), with_trash());
    assert_eq!(reopened.corrupt(), 0);
    let (mem, _, _, kill, trash) = support_store();
    for id in [&kill, &trash] {
        let a = reopened.card(id).expect("read back");
        let b = mem.card(id).unwrap();
        assert_eq!(a.players, b.players, "{id}: every scalar round-trips");
        assert_eq!(a.duration_ms, b.duration_ms);
        assert_eq!(
            reopened.rows(id).unwrap().support,
            mem.rows(id).unwrap().support,
            "{id}: the blocks round-trip"
        );
        for guid in [EVOKER, MAGE, WARRIOR, PRIEST] {
            assert_eq!(
                reopened
                    .stored_fight(id, View::Damage, Some(guid))
                    .unwrap()
                    .support,
                mem.stored_fight(id, View::Damage, Some(guid))
                    .unwrap()
                    .support,
                "{id} {guid}"
            );
        }
    }
    // The stored `effective_dps` is on the file (SQL reads it) and is the
    // derived value, never read back into the card.
    let file = std::fs::read_to_string(tmp.0.join("fights").join(format!("{kill}.json"))).unwrap();
    let e = player(reopened.card(&kill).unwrap(), EVOKER);
    let written = format!("\"effective_dps\":{}", 85_900.0 / 60.0);
    assert!(file.contains(&written), "{written} in the card: {file}");
    assert!(close(e.effective_dps(60_000), 85_900.0 / 60.0));
}

#[test]
fn the_mock_daemons_store_writes_the_same_card() {
    // The in-process fake daemon feeds every Closed into its own store —
    // the seam the GUI and TUI tests build on — so what it holds must be
    // what the engine path above holds.
    let mock = MockDaemon::fixture_at(Path::new(SUPPORT)).with_history();
    let store = mock.history();
    let card = store
        .cards()
        .iter()
        .find(|c| c.name == BOSS && c.kind == wowdps_proto::history::FightKind::Encounter)
        .expect("the mock stored the kill");
    let e = player(card, EVOKER);
    assert_eq!((e.support_given, e.support_received), (23_900, 7_500));
    assert_eq!(e.effective(), 85_900);
    assert_eq!(player(card, PRIEST).overheal, 16_000);
}
