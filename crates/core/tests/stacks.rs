//! R21: stacked-debuff conditioning, over every committed fixture — the
//! stacks fixture's own per-cell table through `Segment::stack_cells` and
//! `stacking_debuffs`, the bound every fixture must respect (Σ a cell
//! group's hits / sum ≤ the unconditioned Taken by-ability row, so the
//! derived level 0 is never negative; a debuff's `hits` = Σ its cells'),
//! the ledger's transitions in isolation (a dose up and down, a refresh
//! keeping the level, an orphan dose opening at its level, a player-
//! sourced debuff and a Buff dose never conditioning, a miss recording
//! nothing, a pet's hit folding to its owner, auras after the end and in
//! the dead zone landing nowhere), the cap, lazy = full = checkpoint-resume
//! parity, and the R10 merge. Every fixture in `FIXTURES` must exist — a
//! missing one fails, never skips.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, STACK_CELL_CAP, Segment, View, meter_from_lines};
use wowdps_core::parser::{Event, LogLine, parse_line};
use wowdps_model::{StackCell, StackingDebuff};

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
    "support.txt",
    "spans.txt",
    "shields.txt",
    "stacks.txt",
];

/// The stacks fixture's roster (see `stacks.expected.md`).
const T: &str = "Player-1168-0A1B2C51"; // Protection Paladin — the Tectonic ladder
const H: &str = "Player-1168-0A1B2C52"; // Holy Priest — the orphan dose at 4
const M: &str = "Player-1168-0A1B2C53"; // Frost Mage — the buff, the Slow, the pet
const PET: &str = "Pet-0-4232-2662-31585-78116-0301A1B2E1";

const TECTONIC: u32 = 1305225;
const SMASH: u32 = 1305230;
const CLAWS: u32 = 1305240;
const SLOW: u32 = 31589; // the mage's own debuff on the tank
const HELLBENT: u32 = 1281559; // a stacking BUFF

fn fixture_path(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn fixtures() -> Vec<(&'static str, String)> {
    FIXTURES
        .iter()
        .map(|name| {
            let text = std::fs::read_to_string(fixture_path(name));
            assert!(
                text.is_ok(),
                "{name}: unreadable fixture: {:?}",
                text.as_ref().err()
            );
            (*name, text.unwrap_or_default())
        })
        .collect()
}

fn parsed(text: &str) -> Vec<LogLine> {
    text.lines().filter_map(parse_line).collect()
}

fn replay(text: &str) -> Meter {
    meter_from_lines(text.lines())
}

/// Every guid a cell could be keyed on or folded to.
fn guids(lines: &[LogLine]) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for l in lines {
        if let Some(h) = &l.owner_hint {
            set.insert(h.owner_guid.clone());
            set.insert(h.unit_guid.clone());
        }
        match &l.event {
            Event::AuraApplied { src, dst, .. }
            | Event::AuraRefresh { src, dst, .. }
            | Event::AuraRemoved { src, dst, .. }
            | Event::AuraDose { src, dst, .. }
            | Event::Damage { src, dst, .. } => {
                set.insert(src.guid.clone());
                set.insert(dst.guid.clone());
            }
            Event::Summon { owner, pet } => {
                set.insert(owner.guid.clone());
                set.insert(pet.guid.clone());
            }
            _ => {}
        }
    }
    let mut out: Vec<String> = set
        .into_iter()
        .filter(|g| g.starts_with("Player-") || g.starts_with("Pet-"))
        .collect();
    out.sort();
    out
}

/// A cell reduced to what R21 pins: (aura, damage id, level) → (hits, sum, max).
type Flat = BTreeMap<(u32, u32, u16), (u32, u64, u64)>;

fn flat(cells: &[StackCell]) -> Flat {
    cells
        .iter()
        .map(|c| {
            (
                (c.aura_spell_id, c.damage_spell_id, c.level),
                (c.hits, c.sum, c.max),
            )
        })
        .collect()
}

fn debuffs(list: &[StackingDebuff]) -> Vec<(u32, u16, u32)> {
    list.iter()
        .map(|d| (d.spell_id, d.max_level, d.hits))
        .collect()
}

/// Everything R21 exposes for a set of keys, comparable across replays.
#[allow(clippy::type_complexity)]
fn picture(seg: &Segment, keys: &[String]) -> Vec<(String, Vec<(u32, u16, u32)>, Flat, u32)> {
    keys.iter()
        .map(|k| {
            (
                k.clone(),
                debuffs(&seg.stacking_debuffs(k)),
                flat(&seg.stack_cells(k)),
                seg.stacks_dropped(k),
            )
        })
        .filter(|(_, d, c, dropped)| !d.is_empty() || !c.is_empty() || *dropped > 0)
        .collect()
}

// ---- synthetic log lines --------------------------------------------------

const T_UNIT: &str = "Player-1168-0A1B2C51,\"Brannoc-Nebula-US\",0x511,0x80000000";
const BOSS_UNIT: &str =
    "Creature-0-4232-2662-31585-219000-0000AF01,\"Stacks Test Boss\",0xa48,0x80";

fn line(ms: i64, body: &str) -> String {
    let s = ms / 1000;
    format!(
        "9/7/2026 20:{:02}:{:02}.{:03}-4  {body}",
        s / 60,
        s % 60,
        ms % 1000
    )
}

fn smash(ms: i64, amount: u64) -> String {
    line(
        ms,
        &format!(
            "SPELL_DAMAGE,{BOSS_UNIT},{T_UNIT},{SMASH},\"Crushing Smash\",0x1,{T},0000000000000000,1128000,1200000,21000,0,14000,0,0,0,1,60,100,0,-810.12,2148.30,2287,3.1416,650,{amount},{amount},-1,0x1,0,0,0,nil,nil,nil,ST"
        ),
    )
}

fn tect(ms: i64, ev: &str, tail: &str) -> String {
    line(
        ms,
        &format!("{ev},{BOSS_UNIT},{T_UNIT},{TECTONIC},\"Tectonic Strike\",0x1,DEBUFF{tail}"),
    )
}

fn encounter(body: &[String]) -> Meter {
    let mut lines = vec![
        line(
            0,
            "COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.7,PROJECT_ID,1",
        ),
        line(100, "ZONE_CHANGE,2769,\"Sepulcher of the Ashen Vow\",16"),
        line(1000, "ENCOUNTER_START,3150,\"Stacks Test Boss\",16,5,2769"),
    ];
    lines.extend_from_slice(body);
    lines.push(line(
        60_000,
        "ENCOUNTER_END,3150,\"Stacks Test Boss\",16,5,1,59000",
    ));
    meter_from_lines(lines.iter().map(String::as_str))
}

// ---- the fixture's table ---------------------------------------------------

/// The hand table in `stacks.expected.md`, cell by cell.
#[test]
fn the_stacks_fixture_reproduces_its_cell_table() {
    let text = std::fs::read_to_string(fixture_path("stacks.txt")).unwrap();
    let meter = replay(&text);
    let segs = meter.segments();
    assert_eq!(
        segs.len(),
        3,
        "encounter + trash + the segment after the dead zone"
    );
    let enc = &segs[0];

    let want_t: Flat = [
        ((TECTONIC, SMASH, 1), (1, 230_000, 230_000)),
        ((TECTONIC, SMASH, 2), (3, 1_450_000, 700_000)),
        ((TECTONIC, SMASH, 3), (4, 2_010_000, 620_000)),
        ((TECTONIC, TECTONIC, 1), (1, 90_000, 90_000)),
        ((TECTONIC, TECTONIC, 2), (1, 110_000, 110_000)),
        ((TECTONIC, TECTONIC, 3), (1, 130_000, 130_000)),
        ((CLAWS, SMASH, 2), (1, 500_000, 500_000)),
    ]
    .into_iter()
    .collect();
    assert_eq!(flat(&enc.stack_cells(T)), want_t, "the tank's cells");
    assert_eq!(
        debuffs(&enc.stacking_debuffs(T)),
        vec![(TECTONIC, 3, 11), (CLAWS, 2, 1)],
        "the tank's debuffs: highest level first"
    );
    let tect = &enc.stacking_debuffs(T)[0];
    assert_eq!(tect.label, "Tectonic Strike");
    assert_eq!(
        tect.src, "Stacks Test Boss",
        "the applier's name, the R17 way"
    );
    // The level-0 row is DERIVED: the unconditioned Crushing Smash row minus
    // the levels — 3 820 000 − 2 990 000 exactly; the row's COUNT is R17's
    // events (12 hits AND the dodge at 3 stacks), so the derived level-0
    // count is 13 − 8 = 5, an upper bound on its hits that includes every
    // miss at any level — stated in R21, never silently a hit count.
    let (by_spell, _) = enc.breakdown(T, View::Taken);
    let smash = by_spell
        .iter()
        .find(|r| r.label == "Crushing Smash")
        .unwrap();
    assert_eq!((smash.count, smash.amount), (13, 4_520_000));
    let conditioned: (u32, u64) = enc
        .stack_cells(T)
        .iter()
        .filter(|c| c.aura_spell_id == TECTONIC && c.damage_spell_id == SMASH)
        .fold((0, 0), |(h, s), c| (h + c.hits, s + c.sum));
    assert_eq!(
        (
            smash.count - u64::from(conditioned.0),
            smash.amount - conditioned.1
        ),
        (5, 830_000)
    );
    // The death rule (case 11): the REMOVED at 20:05:47.000 precedes the
    // 700 000 killing blow at the same millisecond — it landed at level 2.
    let l2 = enc
        .stack_cells(T)
        .into_iter()
        .find(|c| c.aura_spell_id == TECTONIC && c.damage_spell_id == SMASH && c.level == 2)
        .unwrap();
    assert_eq!(l2.max, 700_000, "the killing blow is conditioned");
    // Never: the mage's Slow on the tank (a controlled source).
    assert!(enc.stack_cells(T).iter().all(|c| c.aura_spell_id != SLOW));
    assert!(enc.stacking_debuffs(T).iter().all(|d| d.spell_id != SLOW));

    // The healer: an orphan dose opened at 4.
    assert_eq!(
        flat(&enc.stack_cells(H)),
        [((TECTONIC, SMASH, 4), (1, 150_000, 150_000))]
            .into_iter()
            .collect()
    );
    assert_eq!(debuffs(&enc.stacking_debuffs(H)), vec![(TECTONIC, 4, 1)]);

    // The mage: her pet's hit folds onto her; her Hellbent buff is nothing;
    // the hit on her under no debuff is nothing.
    assert_eq!(
        flat(&enc.stack_cells(M)),
        [((TECTONIC, SMASH, 1), (1, 60_000, 60_000))]
            .into_iter()
            .collect()
    );
    assert!(
        enc.stacking_debuffs(M)
            .iter()
            .all(|d| d.spell_id != HELLBENT)
    );
    assert!(
        enc.stack_cells(PET).is_empty(),
        "the pet has no row of its own"
    );

    // Trash after the kill: the apply before any segment opened landed
    // nowhere, the orphan dose opened at 2, one hit at 2.
    let trash = &segs[1];
    assert_eq!(
        flat(&trash.stack_cells(T)),
        [((TECTONIC, SMASH, 2), (1, 100_000, 100_000))]
            .into_iter()
            .collect()
    );
    assert_eq!(debuffs(&trash.stacking_debuffs(T)), vec![(TECTONIC, 2, 1)]);
    // Past the dead zone: the dose at 3 landed nowhere — a new segment, an
    // empty ledger, and nothing on the old one either.
    let after = &segs[2];
    assert!(after.stack_cells(T).is_empty());
    assert!(after.stacking_debuffs(T).is_empty());
    for s in segs {
        assert_eq!(s.stacks_dropped(T), 0);
    }
}

// ---- invariants over every fixture ----------------------------------------

/// Σ a (aura, damage) group's hits and sum never exceed the unconditioned
/// Taken row — so the derived level 0 is never negative — and a debuff's
/// `hits` is Σ its cells'.
#[test]
fn cells_are_bounded_by_the_taken_row_everywhere() {
    let mut checked = 0;
    for (name, text) in fixtures() {
        let keys = guids(&parsed(&text));
        let meter = replay(&text);
        for (i, seg) in meter.segments().iter().enumerate() {
            for k in &keys {
                let cells = seg.stack_cells(k);
                if cells.is_empty() {
                    continue;
                }
                let (by_spell, _) = seg.breakdown(k, View::Taken);
                let mut groups: BTreeMap<(u32, String), (u32, u64)> = BTreeMap::new();
                for c in &cells {
                    assert!(c.level >= 1, "{name} seg {i} {k}: a level-0 cell");
                    assert!(c.hits >= 1 && c.max <= c.sum, "{name} seg {i} {k}: {c:?}");
                    let g = groups
                        .entry((c.aura_spell_id, c.damage_label.clone()))
                        .or_default();
                    g.0 += c.hits;
                    g.1 += c.sum;
                }
                for ((aura, label), (hits, sum)) in &groups {
                    let row = by_spell
                        .iter()
                        .find(|r| r.label == *label)
                        .unwrap_or_else(|| panic!("{name} seg {i} {k}: no Taken row {label}"));
                    assert!(
                        u64::from(*hits) <= row.count && *sum <= row.amount,
                        "{name} seg {i} {k} aura {aura} {label}: {hits}/{sum} over {}/{}",
                        row.count,
                        row.amount
                    );
                    checked += 1;
                }
                for d in seg.stacking_debuffs(k) {
                    let want: u32 = cells
                        .iter()
                        .filter(|c| c.aura_spell_id == d.spell_id)
                        .map(|c| c.hits)
                        .sum();
                    assert_eq!(d.hits, want, "{name} seg {i} {k} debuff {}", d.spell_id);
                    assert!(
                        cells
                            .iter()
                            .filter(|c| c.aura_spell_id == d.spell_id)
                            .all(|c| c.level <= d.max_level),
                        "{name} seg {i} {k}: a cell above the debuff's max level"
                    );
                }
            }
        }
    }
    assert!(checked > 0);
}

// ---- transitions in isolation ---------------------------------------------

#[test]
fn a_dose_sets_the_level_up_and_down_and_a_refresh_keeps_it() {
    let m = encounter(&[
        tect(2_000, "SPELL_AURA_APPLIED", ""),
        smash(3_000, 100),
        tect(4_000, "SPELL_AURA_APPLIED_DOSE", ",3"),
        smash(5_000, 300),
        tect(6_000, "SPELL_AURA_REFRESH", ""),
        smash(7_000, 301),
        tect(8_000, "SPELL_AURA_REMOVED_DOSE", ",1"),
        smash(9_000, 101),
        tect(10_000, "SPELL_AURA_REMOVED", ""),
        smash(11_000, 999),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.stack_cells(T)),
        [
            ((TECTONIC, SMASH, 1), (2, 201, 101)),
            ((TECTONIC, SMASH, 3), (2, 601, 301)),
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(debuffs(&seg.stacking_debuffs(T)), vec![(TECTONIC, 3, 4)]);
}

#[test]
fn an_orphan_dose_or_refresh_opens_at_its_level_and_an_orphan_removal_nothing() {
    let m = encounter(&[
        tect(2_000, "SPELL_AURA_REMOVED", ""),
        smash(3_000, 5),
        tect(4_000, "SPELL_AURA_APPLIED_DOSE", ",5"),
        smash(5_000, 50),
        tect(6_000, "SPELL_AURA_REMOVED", ""),
        tect(7_000, "SPELL_AURA_REFRESH", ""),
        smash(8_000, 10),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.stack_cells(T)),
        [
            ((TECTONIC, SMASH, 1), (1, 10, 10)),
            ((TECTONIC, SMASH, 5), (1, 50, 50)),
        ]
        .into_iter()
        .collect()
    );
}

/// The death rule in isolation: a removal closes the entry AT its
/// millisecond — a hit at that millisecond still lands at the level, a
/// hit one millisecond later does not, and a re-apply reopens cleanly.
#[test]
fn a_hit_at_the_removal_millisecond_still_lands_at_the_level() {
    let m = encounter(&[
        tect(2_000, "SPELL_AURA_APPLIED_DOSE", ",3"),
        tect(3_000, "SPELL_AURA_REMOVED", ""),
        smash(3_000, 300),
        smash(3_001, 1),
        tect(4_000, "SPELL_AURA_REFRESH", ""), // closed: reopens at 1
        smash(5_000, 100),
        tect(6_000, "SPELL_AURA_REMOVED", ""),
        tect(6_000, "SPELL_AURA_APPLIED", ""), // re-apply at the same ms: open at 1
        smash(6_000, 101),
        smash(7_000, 102),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.stack_cells(T)),
        [
            ((TECTONIC, SMASH, 1), (3, 303, 102)),
            ((TECTONIC, SMASH, 3), (1, 300, 300)),
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn a_non_numeric_dose_is_other_and_a_miss_records_nothing() {
    let bad = tect(2_000, "SPELL_AURA_APPLIED_DOSE", ",lots");
    let l = parse_line(&bad).unwrap();
    assert!(matches!(l.event, Event::Other), "{:?}", l.event);
    let m = encounter(&[
        tect(2_000, "SPELL_AURA_APPLIED_DOSE", ",2"),
        line(
            3_000,
            &format!(
                "SPELL_MISSED,{BOSS_UNIT},{T_UNIT},{SMASH},\"Crushing Smash\",0x1,DODGE,nil,ST"
            ),
        ),
    ]);
    let seg = &m.segments()[0];
    assert!(seg.stack_cells(T).is_empty());
    assert_eq!(debuffs(&seg.stacking_debuffs(T)), vec![(TECTONIC, 2, 0)]);
    assert_eq!(
        seg.mitigation(T).map(|mi| mi.misses()),
        Some(1),
        "R17 still counts the dodge"
    );
}

#[test]
fn the_cap_counts_what_it_drops() {
    // STACK_CELL_CAP distinct damage spells at level 1, then one more.
    let mut body = vec![tect(2_000, "SPELL_AURA_APPLIED", "")];
    for i in 0..=STACK_CELL_CAP as u32 {
        body.push(line(3_000 + i64::from(i), &format!(
            "SPELL_DAMAGE,{BOSS_UNIT},{T_UNIT},{},\"Spell {i}\",0x1,{T},0000000000000000,1128000,1200000,21000,0,14000,0,0,0,1,60,100,0,-810.12,2148.30,2287,3.1416,650,7,7,-1,0x1,0,0,0,nil,nil,nil,ST",
            2_000_000 + i
        )));
    }
    let m = encounter(&body);
    let seg = &m.segments()[0];
    assert_eq!(seg.stack_cells(T).len(), STACK_CELL_CAP);
    assert_eq!(seg.stacks_dropped(T), 1);
    assert_eq!(
        seg.stacking_debuffs(T)[0].hits,
        STACK_CELL_CAP as u32 + 1,
        "seen counts every hit"
    );
}

// ---- parity and merge -------------------------------------------------------

#[test]
fn stacks_survive_lazy_loading_and_checkpoints_on_every_fixture() {
    let mut pictured = 0;
    for (name, text) in fixtures() {
        let path = fixture_path(name);
        let bytes = text.as_bytes();
        let keys = guids(&parsed(&text));
        let idx = scan(&mut &bytes[..]);
        let full = replay(&text);
        let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
        assert_eq!(metas.len(), full.segments().len(), "{name}: segment count");
        for (meta, seg) in metas.iter().zip(full.segments()) {
            let lines = load_segment(Path::new(&path), meta).expect("slice loads");
            let lazy = meter_from_lines(lines.iter().map(String::as_str));
            let want = picture(seg, &keys);
            pictured += want.len();
            assert_eq!(
                picture(&lazy.segments()[0], &keys),
                want,
                "{name} / {}",
                meta.name
            );
        }
        for meta in &idx.overalls {
            let ordinal = meta.visit.expect("an Overall meta names its visit");
            let lines = load_segment(Path::new(&path), meta).expect("visit loads");
            let lazy = meter_from_lines(lines.iter().map(String::as_str));
            let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
            let want = full.overall(ordinal).expect("full replay has the visit");
            assert_eq!(
                picture(&got, &keys),
                picture(&want, &keys),
                "{name} / {}",
                meta.name
            );
        }
        let cuts: Vec<usize> = bytes
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .map(|(i, _)| i + 1)
            .collect();
        for cut in cuts {
            let prefix = scan(&mut &bytes[..cut]);
            let state = prefix.checkpoint.clone();
            let off = state.offset as usize;
            let resumed = scan_from(&mut &bytes[off..], state);
            assert_eq!(resumed.segments, idx.segments, "{name}: cut at {cut}");
            assert_eq!(resumed.open, idx.open, "{name}: cut at {cut}");
        }
    }
    assert!(pictured > 0, "some segment carries a stack picture");
}

#[test]
fn overall_sums_members_cells() {
    let mut visits = 0;
    for (name, text) in fixtures() {
        let keys = guids(&parsed(&text));
        let meter = replay(&text);
        for (ordinal, _) in meter.visits().iter().enumerate() {
            let ordinal = ordinal as u32;
            let Some(ov) = meter.overall(ordinal) else {
                continue;
            };
            visits += 1;
            let members: Vec<&Segment> = meter
                .segments()
                .iter()
                .filter(|s| s.visit == Some(ordinal))
                .collect();
            for k in &keys {
                let mut want: Flat = BTreeMap::new();
                let mut want_d: BTreeMap<u32, (u16, u32)> = BTreeMap::new();
                for m in &members {
                    for (key, (h, s, x)) in flat(&m.stack_cells(k)) {
                        let c = want.entry(key).or_default();
                        c.0 += h;
                        c.1 += s;
                        c.2 = c.2.max(x);
                    }
                    for d in m.stacking_debuffs(k) {
                        let e = want_d.entry(d.spell_id).or_default();
                        e.0 = e.0.max(d.max_level);
                        e.1 += d.hits;
                    }
                }
                assert_eq!(
                    flat(&ov.stack_cells(k)),
                    want,
                    "{name}: visit {ordinal} {k}"
                );
                let got_d: BTreeMap<u32, (u16, u32)> = ov
                    .stacking_debuffs(k)
                    .into_iter()
                    .map(|d| (d.spell_id, (d.max_level, d.hits)))
                    .collect();
                assert_eq!(got_d, want_d, "{name}: visit {ordinal} {k} debuffs");
            }
        }
    }
    assert!(visits > 0);
}
