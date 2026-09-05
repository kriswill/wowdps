//! R20: the shield ledger, over every committed fixture — the fixture's own
//! per-shield table through `Segment::shields`, the two identities
//! (Σ rows.consumed = `absorbed_healing` for every player on every segment;
//! `applied = consumed + wasted` on every row whose shields all closed with
//! a known size), each transition in isolation (a refresh up and down, an
//! over-absorb on a known and on an unknown size, a re-apply while open, the
//! pre-pull shield, the shield open at the kill and in a Trash segment, the
//! orphan refresh / removal, a refresh making an unknown balance known,
//! Stagger, a non-shield buff with a trailer, an aura after `ENCOUNTER_END`
//! and in the trash dead zone, a pet absorber, an NPC absorber, a shield
//! outside the table), lazy = full = checkpoint-resume parity, the R10
//! merge, and the scanner's indifference to aura lines. Every fixture in
//! `FIXTURES` must exist — a missing one fails, never skips.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, Segment, SegmentKind, View, meter_from_lines};
use wowdps_core::parser::{Event, LogLine, parse_line};
use wowdps_model::ShieldRow;

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
    "support.txt",
    "spans.txt",
    "shields.txt",
];

/// The shields fixture's roster (see `shields.expected.md`).
const P: &str = "Player-1168-0A1B2C41"; // Discipline Priest — every PW:S
const W: &str = "Player-1168-0A1B2C42"; // Protection Warrior
const M: &str = "Player-1168-0A1B2C43"; // Fire Mage — her own Ice Barrier
const K: &str = "Player-1168-0A1B2C44"; // Brewmaster Monk — Stagger
const D: &str = "Player-1168-0A1B2C45"; // Blood DK — his own Blood Shield

const PWS: u32 = 17;
const ICE_BARRIER: u32 = 11426;
const BLOOD_SHIELD: u32 = 77535;
const STAGGER: u32 = 115069; // NON_HEALING_ABSORBS
const BONE_SHIELD: u32 = 195181; // an AM span, never an absorb spell
const SECOND_WIND: u32 = 29838; // the `BUFF,0,0` shape
const GUARDIAN_SPIRIT: u32 = 47788; // outside the absorb-spell table

fn fixture_path(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Every fixture, as (name, text). A missing or unreadable one fails.
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

/// Every guid a shield could be keyed on or folded to: aura sources and
/// targets, absorbers and victims, summon owners and pets, damage and heal
/// sources and destinations.
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
            | Event::Damage { src, dst, .. }
            | Event::Heal { src, dst, .. } => {
                set.insert(src.guid.clone());
                set.insert(dst.guid.clone());
            }
            Event::Absorbed { dst, absorber, .. } => {
                set.insert(dst.guid.clone());
                set.insert(absorber.guid.clone());
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

/// A row reduced to what R20 pins: (spell, applied, consumed, wasted, count, unknown).
fn flat(rows: &[ShieldRow]) -> Vec<(u32, u64, u64, u64, u32, u32)> {
    rows.iter()
        .map(|r| {
            (
                r.spell_id, r.applied, r.consumed, r.wasted, r.count, r.unknown,
            )
        })
        .collect()
}

fn consumed(seg: &Segment, guid: &str) -> u64 {
    seg.shields(guid).iter().map(|r| r.consumed).sum()
}

// ---- synthetic log lines --------------------------------------------------

const P_UNIT: &str = "Player-1168-0A1B2C41,\"Serenya-Nebula-US\",0x514,0x80000000";
const W_UNIT: &str = "Player-1168-0A1B2C42,\"Bulwark-Nebula-US\",0x511,0x80000000";
const M_UNIT: &str = "Player-1168-0A1B2C43,\"Pyrelle-Nebula-US\",0x514,0x80000000";
const PET: &str = "Pet-0-4232-2662-31585-417-0102ABCDEF";
const PET_UNIT: &str = "Pet-0-4232-2662-31585-417-0102ABCDEF,\"Fluffy\",0x1114,0x80000000";
const BOSS_GUID: &str = "Creature-0-4232-2662-31585-218000-0000AE01";
const BOSS_UNIT: &str =
    "Creature-0-4232-2662-31585-218000-0000AE01,\"Shields Test Boss\",0xa48,0x80";

/// A line at `ms` after 20:05:00.000 on the fixture's date.
fn line(ms: i64, body: &str) -> String {
    let total = 20 * 3_600_000 + 5 * 60_000 + ms;
    let (h, rem) = (total / 3_600_000, total % 3_600_000);
    let (m, rem) = (rem / 60_000, rem % 60_000);
    let (s, milli) = (rem / 1000, rem % 1000);
    format!("9/6/2026 {h}:{m:02}:{s:02}.{milli:03}-4  {body}")
}

fn start(ms: i64) -> String {
    line(ms, "ENCOUNTER_START,3148,\"Shields Test Boss\",16,5,2769")
}

fn end(ms: i64) -> String {
    line(ms, "ENCOUNTER_END,3148,\"Shields Test Boss\",16,5,1,60000")
}

fn guid_of(unit: &str) -> &str {
    unit.split(',').next().unwrap_or(unit)
}

/// The boss's Cinder Lash on a friendly for `amount`, with `absorbed` soaked
/// (the advanced block's `absorbed` field) — R17's taken twin of an absorb.
fn lash(ms: i64, dst_unit: &str, amount: u64, absorbed: u64) -> String {
    let dst = guid_of(dst_unit);
    let total = amount + absorbed;
    line(
        ms,
        &format!(
            "SPELL_DAMAGE,{BOSS_UNIT},{dst_unit},380001,\"Cinder Lash\",0x4,{dst},0000000000000000,1247000,1250000,24000,0,16000,0,0,0,1,60,100,0,-810.12,2148.30,2287,3.1416,650,{amount},{total},-1,0x4,0,0,{absorbed},nil,nil,nil,ST"
        ),
    )
}

/// The 22-field SPELL_ABSORBED: the boss's Cinder Lash on `dst_unit`, soaked
/// for `amount` by `absorber_unit`'s shield `(id, name)`.
fn absorbed(
    ms: i64,
    dst_unit: &str,
    absorber_unit: &str,
    id: u32,
    name: &str,
    amount: u64,
) -> String {
    line(
        ms,
        &format!(
            "SPELL_ABSORBED,{BOSS_UNIT},{dst_unit},380001,\"Cinder Lash\",0x4,{absorber_unit},{id},\"{name}\",0x2,{amount},{amount},nil"
        ),
    )
}

/// A lash and its absorb together: `amount` landed, `soaked` absorbed.
fn hit_and_absorb(
    ms: i64,
    dst_unit: &str,
    absorber_unit: &str,
    id: u32,
    name: &str,
    amount: u64,
    soaked: u64,
) -> Vec<String> {
    vec![
        lash(ms, dst_unit, amount, soaked),
        absorbed(ms, dst_unit, absorber_unit, id, name, soaked),
    ]
}

/// A friendly's hit on the boss — plain combat.
fn hit(ms: i64, src_unit: &str, amount: u64) -> String {
    line(
        ms,
        &format!(
            "SPELL_DAMAGE,{src_unit},{BOSS_UNIT},23922,\"Shield Slam\",0x1,{BOSS_GUID},0000000000000000,276000,296000,0,0,0,0,0,0,0,0,0,0,-812.44,2145.87,2287,4.7123,83,{amount},{amount},-1,1,0,0,0,nil,nil,nil,ST"
        ),
    )
}

fn aura(
    ms: i64,
    event: &str,
    src: &str,
    dst: &str,
    id: u32,
    name: &str,
    trailer: Option<u64>,
) -> String {
    let tail = trailer.map(|t| format!(",{t}")).unwrap_or_default();
    line(
        ms,
        &format!("{event},{src},{dst},{id},\"{name}\",0x2,BUFF{tail}"),
    )
}

fn apply(ms: i64, src: &str, dst: &str, id: u32, name: &str, a: Option<u64>) -> String {
    aura(ms, "SPELL_AURA_APPLIED", src, dst, id, name, a)
}

fn refresh(ms: i64, src: &str, dst: &str, id: u32, name: &str, r: Option<u64>) -> String {
    aura(ms, "SPELL_AURA_REFRESH", src, dst, id, name, r)
}

fn remove(ms: i64, src: &str, dst: &str, id: u32, name: &str, w: Option<u64>) -> String {
    aura(ms, "SPELL_AURA_REMOVED", src, dst, id, name, w)
}

fn summon(ms: i64, owner_unit: &str, pet_unit: &str) -> String {
    line(
        ms,
        &format!("SPELL_SUMMON,{owner_unit},{pet_unit},883,\"Call Pet 1\",0x1"),
    )
}

fn meter_of(lines: &[String]) -> Meter {
    meter_from_lines(lines.iter().map(String::as_str))
}

/// The meter of a kill with `body` between the pull's first hit and
/// the kill.
fn kill_of(body: Vec<String>) -> Meter {
    meter_of(&pull(body))
}

/// One kill with `body` between the pull's first hit and the kill.
fn pull(body: Vec<String>) -> Vec<String> {
    let mut lines = vec![start(0), hit(500, W_UNIT, 1000)];
    lines.extend(body);
    lines.push(hit(59_000, W_UNIT, 1000));
    lines.push(end(60_000));
    lines
}

// ---- the fixture's own table ---------------------------------------------

/// The rows of `shields.expected.md` §"Rows": P's seven Power Word: Shields
/// (#1 unknown-applied, #10 open at the kill), M's two Ice Barriers, D's
/// one Blood Shield; W and K absorb nothing. The derived waste and the
/// unknown count beside them; the trash tail empty for everyone.
#[test]
fn the_shields_fixture_reproduces_its_ledger_table() {
    let meter = replay(&fixtures()[7].1);
    let segs = meter.segments();
    assert_eq!(segs.len(), 2);
    let kill = &segs[0];
    assert_eq!(kill.kind, SegmentKind::Encounter);
    assert_eq!(
        flat(&kill.shields(P)),
        vec![(PWS, 75_000, 65_000, 19_000, 7, 2)],
        "P: #1 unknown-applied, #10 open at the kill"
    );
    assert_eq!(kill.shields(P)[0].label, "Power Word: Shield");
    assert_eq!(
        flat(&kill.shields(M)),
        vec![(ICE_BARRIER, 16_000, 11_000, 5_000, 2, 0)]
    );
    assert_eq!(
        flat(&kill.shields(D)),
        vec![(BLOOD_SHIELD, 14_000, 9_000, 5_000, 1, 0)]
    );
    assert!(kill.shields(W).is_empty());
    assert!(kill.shields(K).is_empty());

    assert_eq!(kill.absorb_wasted(P), Some(19_000));
    assert_eq!(kill.absorb_wasted(M), Some(5_000));
    assert_eq!(kill.absorb_wasted(D), Some(5_000));
    assert_eq!(kill.absorb_wasted(W), None, "a shield TARGET has no waste");
    assert_eq!(kill.absorb_wasted(K), None);
    assert_eq!(kill.shields_unknown(P), 2);
    for k in [W, M, K, D] {
        assert_eq!(kill.shields_unknown(k), 0, "{k}");
    }

    // The per-player identity, spelled out as the golden does: P's known
    // applied = (consumed − #1 − #10) + (wasted − #1).
    assert_eq!(75_000, (65_000 - 4_000 - 3_000) + (19_000 - 2_000));

    let trash = &segs[1];
    assert_eq!(trash.kind, SegmentKind::Trash);
    for k in [P, W, M, K, D] {
        assert!(trash.shields(k).is_empty(), "{k}: the trash tail");
        assert_eq!(trash.absorb_wasted(k), None);
        assert_eq!(trash.shields_unknown(k), 0);
    }
}

// ---- the identities -------------------------------------------------------

/// Σ rows.consumed = `absorbed_healing` for every player on every segment
/// and every Overall of every fixture — exact, because an open shield folds
/// its consumed at read time and an absorb outside the table still opens a
/// key.
#[test]
fn sum_consumed_equals_absorbed_healing_everywhere() {
    let mut nonzero = 0;
    for (name, text) in fixtures() {
        let keys = guids(&parsed(&text));
        let meter = replay(&text);
        let mut segs: Vec<Segment> = meter.segments().to_vec();
        for (ordinal, _) in meter.visits().iter().enumerate() {
            if let Some(ov) = meter.overall(ordinal as u32) {
                segs.push(ov);
            }
        }
        for seg in &segs {
            for k in &keys {
                let sum = consumed(seg, k);
                assert_eq!(
                    sum,
                    seg.absorbed_healing(k),
                    "{name} / {}: {k} Σ consumed vs absorbed_healing",
                    seg.name
                );
                if sum > 0 {
                    nonzero += 1;
                }
            }
        }
    }
    assert!(nonzero >= 5, "the fixtures carry absorbs: {nonzero}");
}

/// `applied = consumed + wasted` on every row whose shields all closed with
/// a known size (`unknown == 0`): a known-applied shield always knows its
/// balance, so its waste is known at the close and an over-absorb raised
/// `applied` by construction. Rows with an unknown shield are checked on
/// the fixture's own arithmetic in the table test.
#[test]
fn applied_equals_consumed_plus_wasted_on_every_known_row() {
    let mut checked = 0;
    for (name, text) in fixtures() {
        let keys = guids(&parsed(&text));
        let meter = replay(&text);
        for seg in meter.segments() {
            for k in &keys {
                for r in seg.shields(k) {
                    if r.unknown == 0 {
                        assert_eq!(
                            r.applied,
                            r.consumed + r.wasted,
                            "{name} / {}: {k} {r:?}",
                            seg.name
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked >= 2, "M's and D's rows at least: {checked}");
}

// ---- the transitions, one at a time ----------------------------------------

/// A refresh's trailer is the NEW RUNNING TOTAL: above the balance it is
/// more shield (applied grows by the delta), below it the difference was
/// overwritten — waste.
#[test]
fn a_refresh_up_is_applied_and_a_refresh_down_is_waste() {
    // Up: 12 000, soak 5 000 (balance 7 000), refresh to 18 000 (+11 000),
    // soak 15 000 (balance 3 000), removed 3 000.
    let mut body = vec![apply(
        1_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(12_000),
    )];
    body.extend(hit_and_absorb(
        2_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        5_000,
    ));
    body.push(refresh(
        3_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(18_000),
    ));
    body.extend(hit_and_absorb(
        4_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        15_000,
    ));
    body.push(remove(
        5_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(3_000),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.shields(P)),
        vec![(PWS, 23_000, 20_000, 3_000, 1, 0)]
    );
    assert_eq!(seg.absorb_wasted(P), Some(3_000));

    // Down: 10 000, refresh to 6 000 (4 000 overwritten), soak 6 000, removed 0.
    let mut body = vec![apply(
        1_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(10_000),
    )];
    body.push(refresh(
        2_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(6_000),
    ));
    body.extend(hit_and_absorb(
        3_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        6_000,
    ));
    body.push(remove(
        4_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(0),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.shields(P)),
        vec![(PWS, 10_000, 6_000, 4_000, 1, 0)]
    );
    assert_eq!(seg.absorb_wasted(P), Some(4_000));

    // A refresh without a trailer is a no-op on the balance.
    let mut body = vec![apply(
        1_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(10_000),
    )];
    body.push(refresh(
        2_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        None,
    ));
    body.push(remove(
        4_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        None,
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 10_000, 0, 10_000, 1, 0)]);
}

/// An absorb larger than the balance raises `applied` by the excess on a
/// known size (the identity holds by construction) and raises nothing on
/// an unknown one; the balance is 0 either way.
#[test]
fn an_over_absorb_raises_applied_only_when_the_size_was_known() {
    let mut body = vec![apply(
        1_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(5_000),
    )];
    body.extend(hit_and_absorb(
        2_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        7_000,
    ));
    body.push(remove(
        3_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(0),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 7_000, 7_000, 0, 1, 0)]);

    // Unknown size: first seen by an absorb (4 000), a refresh makes the
    // balance known (3 000), the next absorb (5 000) exceeds it — applied
    // stays unknown, the balance is 0, the removal's 0 is the waste.
    let mut body = hit_and_absorb(1_000, W_UNIT, P_UNIT, PWS, "Power Word: Shield", 0, 4_000);
    body.push(refresh(
        2_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(3_000),
    ));
    body.extend(hit_and_absorb(
        3_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        5_000,
    ));
    body.push(remove(
        4_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(0),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 9_000, 0, 1, 1)]);
    assert_eq!(seg.absorb_wasted(P), Some(0), "the trailer fixed the waste");
    assert_eq!(seg.shields_unknown(P), 1);
}

/// An apply while the key is open closes the old shield first — with
/// `wasted = remaining` when the balance was known, an unknown waste
/// otherwise — and the new one is its own count.
#[test]
fn a_re_apply_while_open_closes_the_old_shield() {
    let mut body = vec![apply(
        1_000,
        M_UNIT,
        M_UNIT,
        ICE_BARRIER,
        "Ice Barrier",
        Some(8_000),
    )];
    body.extend(hit_and_absorb(
        2_000,
        M_UNIT,
        M_UNIT,
        ICE_BARRIER,
        "Ice Barrier",
        0,
        3_000,
    ));
    body.push(apply(
        3_000,
        M_UNIT,
        M_UNIT,
        ICE_BARRIER,
        "Ice Barrier",
        Some(8_000),
    ));
    body.extend(hit_and_absorb(
        4_000,
        M_UNIT,
        M_UNIT,
        ICE_BARRIER,
        "Ice Barrier",
        0,
        8_000,
    ));
    body.push(remove(
        5_000,
        M_UNIT,
        M_UNIT,
        ICE_BARRIER,
        "Ice Barrier",
        Some(0),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.shields(M)),
        vec![(ICE_BARRIER, 16_000, 11_000, 5_000, 2, 0)]
    );

    // The old one unknown-sized with an unknown balance: it closes with
    // its consumed and `unknown`, no waste; the new one is then open at
    // the kill, so nothing known ever fixed a waste — `None`.
    let mut body = hit_and_absorb(1_000, M_UNIT, M_UNIT, ICE_BARRIER, "Ice Barrier", 0, 3_000);
    body.push(apply(
        3_000,
        M_UNIT,
        M_UNIT,
        ICE_BARRIER,
        "Ice Barrier",
        Some(8_000),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.shields(M)),
        vec![(ICE_BARRIER, 0, 3_000, 0, 2, 2)]
    );
    assert_eq!(seg.absorb_wasted(M), None);
    assert_eq!(seg.shields_unknown(M), 2);
}

/// The pre-pull shield: applied before ENCOUNTER_START (no segment — lands
/// nowhere), first seen by its absorb (unknown-applied), removed with a
/// trailer that is authoritative for the waste even so.
#[test]
fn a_pre_pull_shield_is_unknown_applied_with_a_known_waste() {
    let mut lines = vec![apply(
        -5_000,
        P_UNIT,
        D_UNIT,
        PWS,
        "Power Word: Shield",
        Some(6_000),
    )];
    let mut body = hit_and_absorb(
        1_000,
        D_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        3_000,
        4_000,
    );
    body.push(remove(
        3_000,
        P_UNIT,
        D_UNIT,
        PWS,
        "Power Word: Shield",
        Some(2_000),
    ));
    lines.extend(pull(body));
    let meter = meter_of(&lines);
    assert_eq!(
        meter.segments().len(),
        1,
        "an aura opens no pre-pull segment"
    );
    let seg = &meter.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 4_000, 2_000, 1, 1)]);
    assert_eq!(seg.absorb_wasted(P), Some(2_000));
    assert_eq!(seg.shields_unknown(P), 1);
    assert_eq!(seg.absorbed_healing(P), 4_000);

    // Without the trailer, and the balance never known: the waste stays
    // unknown — `None`, never a 0.
    let mut body = hit_and_absorb(
        1_000,
        D_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        3_000,
        4_000,
    );
    body.push(remove(
        3_000,
        P_UNIT,
        D_UNIT,
        PWS,
        "Power Word: Shield",
        None,
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 4_000, 0, 1, 1)]);
    assert_eq!(seg.absorb_wasted(P), None);
}

const D_UNIT: &str = "Player-1168-0A1B2C45,\"Morthane-Nebula-US\",0x514,0x80000000";

/// A removal trailer that disagrees with the running balance of a KNOWN
/// shield: ABOVE it raises `applied` by the difference (the shield grew with
/// no line saying so — a stacking shield), so `applied = consumed + wasted`
/// holds by construction; BELOW it the trailer is the waste, `applied` stays
/// where the log put it and the shield closes as `unknown` — raise-only, the
/// row visibly inconsistent rather than quietly corrected; on an
/// unknown-sized shield it fixes the waste and touches nothing else.
#[test]
fn a_removal_trailer_off_the_balance_corrects_applied() {
    // Grew: 843 applied, nothing soaked, 3 171 remained at the removal.
    let body = vec![
        apply(1_000, P_UNIT, W_UNIT, PWS, "Power Word: Shield", Some(843)),
        remove(
            2_000,
            P_UNIT,
            W_UNIT,
            PWS,
            "Power Word: Shield",
            Some(3_171),
        ),
    ];
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 3_171, 0, 3_171, 1, 0)]);

    // Shrank: 10 000 applied, 2 000 soaked (balance 8 000), 5 000 remained:
    // consumed 2 000, wasted 5 000, applied stays 10 000, unknown 1.
    let mut body = vec![apply(
        1_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(10_000),
    )];
    body.extend(hit_and_absorb(
        2_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        2_000,
    ));
    body.push(remove(
        3_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(5_000),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.shields(P)),
        vec![(PWS, 10_000, 2_000, 5_000, 1, 1)]
    );
    assert_eq!(seg.absorb_wasted(P), Some(5_000));
    assert_eq!(seg.shields_unknown(P), 1, "a shrink counts as unknown");

    // Unknown size with a known balance (from a refresh): the trailer is
    // the waste, applied stays unknown.
    let mut body = hit_and_absorb(1_000, W_UNIT, P_UNIT, PWS, "Power Word: Shield", 0, 4_000);
    body.push(refresh(
        2_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(6_000),
    ));
    body.push(remove(
        3_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(9_000),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 4_000, 9_000, 1, 1)]);
}

/// A refresh on an unknown-applied shield makes the balance known without
/// touching `applied`; a later removal without a trailer then knows its
/// waste from that balance.
#[test]
fn a_refresh_makes_an_unknown_balance_known() {
    let mut body = hit_and_absorb(1_000, W_UNIT, P_UNIT, PWS, "Power Word: Shield", 0, 4_000);
    body.push(refresh(
        2_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(6_000),
    ));
    body.extend(hit_and_absorb(
        3_000,
        W_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        0,
        2_000,
    ));
    body.push(remove(
        4_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        None,
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 6_000, 4_000, 1, 1)]);
    assert_eq!(seg.absorb_wasted(P), Some(4_000));
}

/// A refresh or a removal with no open key is a no-op — never the spans'
/// segment-start rule: a removal is not evidence of a shield, and the
/// absorb that would prove one opens its own key.
#[test]
fn an_orphan_refresh_or_removal_opens_nothing() {
    let body = vec![
        refresh(
            1_000,
            P_UNIT,
            W_UNIT,
            PWS,
            "Power Word: Shield",
            Some(5_000),
        ),
        remove(
            2_000,
            P_UNIT,
            W_UNIT,
            PWS,
            "Power Word: Shield",
            Some(5_000),
        ),
    ];
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert!(seg.shields(P).is_empty());
    assert_eq!(seg.absorb_wasted(P), None);
    assert_eq!(seg.shields_unknown(P), 0);
}

/// A shield still open at the kill folds with its consumed and count only
/// — `unknown` += 1, no applied, no wasted, and it fixes no waste — at READ
/// time; a Trash segment (which has no close event) reads the same way.
#[test]
fn an_open_shield_folds_consumed_and_count_at_read_time() {
    let mut body = vec![apply(
        50_000,
        P_UNIT,
        K_UNIT,
        PWS,
        "Power Word: Shield",
        Some(9_000),
    )];
    body.extend(hit_and_absorb(
        52_000,
        K_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        2_000,
        3_000,
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 3_000, 0, 1, 1)]);
    assert_eq!(seg.absorb_wasted(P), None);
    assert_eq!(seg.shields_unknown(P), 1);

    // Trash: the pull's first hit opens it, the shield is on at the last
    // line, and the removal 70 s later is in the dead zone — nowhere.
    let mut lines = vec![hit(0, W_UNIT, 1_000)];
    lines.push(apply(
        1_000,
        P_UNIT,
        K_UNIT,
        PWS,
        "Power Word: Shield",
        Some(9_000),
    ));
    lines.extend(hit_and_absorb(
        2_000,
        K_UNIT,
        P_UNIT,
        PWS,
        "Power Word: Shield",
        2_000,
        3_000,
    ));
    lines.push(remove(
        72_000,
        P_UNIT,
        K_UNIT,
        PWS,
        "Power Word: Shield",
        Some(6_000),
    ));
    let meter = meter_of(&lines);
    assert_eq!(meter.segments().len(), 1);
    let seg = &meter.segments()[0];
    assert_eq!(seg.kind, SegmentKind::Trash);
    assert_eq!(flat(&seg.shields(P)), vec![(PWS, 0, 3_000, 0, 1, 1)]);
    assert_eq!(seg.absorb_wasted(P), None);
}

const K_UNIT: &str = "Player-1168-0A1B2C44,\"Brewmoon-Nebula-US\",0x514,0x80000000";

/// Stagger is `NON_HEALING_ABSORBS`: R17's `stagger`, never a row.
#[test]
fn stagger_never_enters_the_ledger() {
    let body = vec![
        line(
            2_000,
            &format!(
                "SWING_DAMAGE,{BOSS_UNIT},{K_UNIT},{BOSS_GUID},0000000000000000,370000,400000,0,0,0,0,0,0,0,0,0,0,-812.44,2145.87,2287,4.7123,83,5000,8000,-1,1,0,0,3000,nil,nil,nil"
            ),
        ),
        line(
            2_000,
            &format!(
                "SPELL_ABSORBED,{BOSS_UNIT},{K_UNIT},{K_UNIT},{STAGGER},\"Stagger\",0x1,3000,8000,nil"
            ),
        ),
    ];
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert!(seg.shields(K).is_empty());
    assert_eq!(seg.absorbed_healing(K), 0);
    assert_eq!(seg.mitigation(K).map(|m| m.stagger), Some(3_000));
}

/// A buff outside the absorb-spell table never rows, whatever its trailer:
/// Bone Shield with 7 500 on both ends, Second Wind's `BUFF,0,0`.
#[test]
fn a_non_shield_buff_with_a_trailer_is_never_a_row() {
    let body = vec![
        apply(
            1_000,
            D_UNIT,
            D_UNIT,
            BONE_SHIELD,
            "Bone Shield",
            Some(7_500),
        ),
        remove(
            2_000,
            D_UNIT,
            D_UNIT,
            BONE_SHIELD,
            "Bone Shield",
            Some(7_500),
        ),
        line(
            3_000,
            &format!(
                "SPELL_AURA_APPLIED,{W_UNIT},{W_UNIT},{SECOND_WIND},\"Second Wind\",0x1,BUFF,0,0"
            ),
        ),
    ];
    let m = kill_of(body);
    let seg = &m.segments()[0];
    for k in [D, W] {
        assert!(seg.shields(k).is_empty(), "{k}");
        assert_eq!(seg.absorb_wasted(k), None);
    }
    // Bone Shield is still its R18 span.
    assert_eq!(seg.am_uptime_ms(D), 1_000);
}

/// An absorb naming a spell OUTSIDE the table (Guardian Spirit) still
/// ledgers — unknown-applied, its consumed counted — so an un-generated
/// build never loses healing; its auras stay gated, so it never closes.
#[test]
fn an_absorb_outside_the_table_still_ledgers_unknown_applied() {
    let mut body = vec![apply(
        1_000,
        P_UNIT,
        W_UNIT,
        GUARDIAN_SPIRIT,
        "Guardian Spirit",
        None,
    )];
    body.extend(hit_and_absorb(
        2_000,
        W_UNIT,
        P_UNIT,
        GUARDIAN_SPIRIT,
        "Guardian Spirit",
        0,
        5_000,
    ));
    body.push(remove(
        3_000,
        P_UNIT,
        W_UNIT,
        GUARDIAN_SPIRIT,
        "Guardian Spirit",
        None,
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(
        flat(&seg.shields(P)),
        vec![(GUARDIAN_SPIRIT, 0, 5_000, 0, 1, 1)]
    );
    assert_eq!(seg.shields(P)[0].label, "Guardian Spirit");
    assert_eq!(seg.absorbed_healing(P), 5_000);
    assert_eq!(seg.absorb_wasted(P), None);
}

/// An aura after ENCOUNTER_END lands nowhere; so does one in a Trash
/// segment's dead zone (past the 60 s gap after its last combat line).
#[test]
fn an_aura_after_the_end_or_in_the_dead_zone_lands_nowhere() {
    let mut lines = pull(Vec::new());
    lines.push(apply(
        61_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(11_000),
    ));
    lines.push(remove(
        62_000,
        P_UNIT,
        W_UNIT,
        PWS,
        "Power Word: Shield",
        Some(11_000),
    ));
    let meter = meter_of(&lines);
    assert_eq!(meter.segments().len(), 1);
    assert!(meter.segments()[0].shields(P).is_empty());

    let lines = vec![
        hit(0, W_UNIT, 1_000),
        hit(8_000, W_UNIT, 1_000),
        apply(
            80_000,
            P_UNIT,
            P_UNIT,
            PWS,
            "Power Word: Shield",
            Some(5_000),
        ),
        remove(
            95_000,
            P_UNIT,
            P_UNIT,
            PWS,
            "Power Word: Shield",
            Some(5_000),
        ),
    ];
    let meter = meter_of(&lines);
    assert_eq!(meter.segments().len(), 1, "an aura never opens a segment");
    let seg = &meter.segments()[0];
    assert_eq!(seg.duration_ms(seg.last_combat_ms()), 8_000);
    assert!(seg.shields(P).is_empty());
    assert_eq!(seg.absorb_wasted(P), None);
}

/// A pet's shield is its owner's row (folded at read time, so a summon
/// seen after the absorb still resolves); a `Creature-` guardian's shield
/// is its owner's too once a summon named the owner — the aura is admitted
/// from then on, so its size and waste are known; an NPC absorber reaches
/// no player's rows, and answers for its own guid exactly as
/// `absorbed_healing` does (the identity holds for any guid asked).
#[test]
fn a_pet_absorber_folds_to_its_owner_and_an_npc_to_nobody() {
    let mut body = hit_and_absorb(1_000, W_UNIT, PET_UNIT, 108366, "Soul Leech", 0, 2_000);
    body.push(summon(2_000, M_UNIT, PET_UNIT));
    body.extend(hit_and_absorb(
        3_000,
        W_UNIT,
        BOSS_UNIT,
        999_999,
        "Boss Ward",
        0,
        700,
    ));
    // The Priest's Celestial: a Creature- guardian whose Celestial Infusion
    // shield is applied, soaked and removed after its summon.
    body.push(summon(4_000, P_UNIT, CELESTIAL_UNIT));
    body.push(apply(
        5_000,
        CELESTIAL_UNIT,
        W_UNIT,
        1241059,
        "Celestial Infusion",
        Some(9_000),
    ));
    body.extend(hit_and_absorb(
        6_000,
        W_UNIT,
        CELESTIAL_UNIT,
        1241059,
        "Celestial Infusion",
        0,
        4_000,
    ));
    body.push(remove(
        7_000,
        CELESTIAL_UNIT,
        W_UNIT,
        1241059,
        "Celestial Infusion",
        Some(5_000),
    ));
    let m = kill_of(body);
    let seg = &m.segments()[0];
    assert_eq!(flat(&seg.shields(M)), vec![(108366, 0, 2_000, 0, 1, 1)]);
    assert!(
        seg.shields(PET).is_empty(),
        "the pet's rows are its owner's"
    );
    assert_eq!(seg.absorbed_healing(M), 2_000);
    assert_eq!(
        flat(&seg.shields(P)),
        vec![(1241059, 9_000, 4_000, 5_000, 1, 0)]
    );
    assert_eq!(seg.absorb_wasted(P), Some(5_000));
    for k in [P, M, W] {
        assert!(seg.shields(k).iter().all(|r| r.spell_id != 999_999), "{k}");
    }
    assert_eq!(
        flat(&seg.shields(BOSS_GUID)),
        vec![(999_999, 0, 700, 0, 1, 1)]
    );
    assert_eq!(seg.absorbed_healing(BOSS_GUID), 700);
    assert_eq!(seg.absorb_wasted(BOSS_GUID), None);
}

const CELESTIAL_UNIT: &str =
    "Creature-0-4232-2662-31585-100868-0000ABCD,\"Chi-Ji\",0x2111,0x80000000";

// ---- parity ----------------------------------------------------------------

/// Everything R20 says about one player, in one comparable value.
type Picture = (String, Vec<ShieldRow>, Option<u64>, u32);

fn shield_picture(seg: &Segment, keys: &[String]) -> Vec<Picture> {
    keys.iter()
        .map(|k| {
            (
                k.clone(),
                seg.shields(k),
                seg.absorb_wasted(k),
                seg.shields_unknown(k),
            )
        })
        .filter(|p| !p.1.is_empty() || p.2.is_some() || p.3 > 0)
        .collect()
}

/// Lazy load == full replay == a scan resumed from any checkpoint, for the
/// rows, the waste and the unknown count, on every segment and every
/// Overall of every fixture.
#[test]
fn shields_survive_lazy_loading_and_checkpoints_on_every_fixture() {
    let mut checked = 0;
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
            assert_eq!(lazy.segments().len(), 1, "{name}: one segment per slice");
            let want = shield_picture(seg, &keys);
            pictured += want.len();
            assert_eq!(
                shield_picture(&lazy.segments()[0], &keys),
                want,
                "{name} / {}",
                meta.name
            );
            checked += 1;
        }
        for meta in &idx.overalls {
            let ordinal = meta.visit.expect("an Overall meta names its visit");
            let lines = load_segment(Path::new(&path), meta).expect("visit loads");
            let lazy = meter_from_lines(lines.iter().map(String::as_str));
            let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
            let want = full.overall(ordinal).expect("full replay has the visit");
            assert_eq!(
                shield_picture(&got, &keys),
                shield_picture(&want, &keys),
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
            let rmetas: Vec<_> = resumed
                .segments
                .iter()
                .chain(resumed.open.as_ref())
                .collect();
            for (meta, seg) in rmetas.iter().zip(full.segments()) {
                let lines = load_segment(Path::new(&path), meta).expect("slice loads");
                let lazy = meter_from_lines(lines.iter().map(String::as_str));
                assert_eq!(
                    shield_picture(&lazy.segments()[0], &keys),
                    shield_picture(seg, &keys),
                    "{name} / {}: resumed at {cut}",
                    meta.name
                );
            }
        }
    }
    assert!(checked > 0);
    assert!(pictured > 0, "some segment carries a shield picture");
}

/// Aura lines are invisible to segmentation: renaming every `SPELL_AURA_*`
/// event to an unknown one of the same byte length changes nothing about
/// the scan, the meter's segment table or Damage rows. Only the ledger's
/// sizes go away: every shield is then first seen by its absorb and never
/// closes, so Σ consumed still equals `absorbed_healing`, every row is
/// unknown-applied and no waste is known.
#[test]
fn aura_lines_never_move_a_segment_boundary() {
    let mut rewritten_any = false;
    for (name, text) in fixtures() {
        let blind = text.replace("SPELL_AURA_", "SPELL_XURA_");
        if blind != text {
            rewritten_any = true;
        }
        let real = scan(&mut text.as_bytes());
        let scan_blind = scan(&mut blind.as_bytes());
        assert_eq!(real, scan_blind, "{name}: the scanner must not see auras");
        let a = replay(&text);
        let b = replay(&blind);
        let table = |m: &Meter| {
            m.segments()
                .iter()
                .map(|s| {
                    (
                        s.kind,
                        s.name.clone(),
                        s.start_ms,
                        s.end_ms,
                        s.last_combat_ms(),
                        s.visit,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(table(&a), table(&b), "{name}: the meter's segment table");
        let keys = guids(&parsed(&text));
        for (sa, sb) in a.segments().iter().zip(b.segments()) {
            let rows = |s: &Segment| {
                s.rows(View::Damage)
                    .iter()
                    .map(|r| (r.key.clone(), r.amount))
                    .collect::<Vec<_>>()
            };
            assert_eq!(rows(sa), rows(sb));
            for k in &keys {
                assert_eq!(
                    sb.absorbed_healing(k),
                    sa.absorbed_healing(k),
                    "{name}: {k}"
                );
                assert_eq!(consumed(sb, k), sb.absorbed_healing(k), "{name}: {k}");
                assert_eq!(sb.absorb_wasted(k), None, "{name}: {k} waste without auras");
                for r in sb.shields(k) {
                    assert_eq!(r.applied, 0, "{name}: {k} {r:?}");
                    assert_eq!(r.unknown, r.count, "{name}: {k} {r:?}");
                }
            }
        }
    }
    assert!(rewritten_any, "no fixture carries an aura line");
}

// ---- R10 -------------------------------------------------------------------

/// An Overall's ledger is the sum of its members': rows merged per spell
/// (each member's open shields folded as its own read folds them),
/// `absorb_wasted` `Some` iff any member's is and the sum of those,
/// `shields_unknown` the sum.
#[test]
fn overall_sums_members_shields() {
    let mut visits = 0;
    let mut nonempty = 0;
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
                let mut want: BTreeMap<u32, (u64, u64, u64, u32, u32)> = BTreeMap::new();
                for m in &members {
                    for r in m.shields(k) {
                        let c = want.entry(r.spell_id).or_default();
                        c.0 += r.applied;
                        c.1 += r.consumed;
                        c.2 += r.wasted;
                        c.3 += r.count;
                        c.4 += r.unknown;
                    }
                }
                let got: BTreeMap<u32, (u64, u64, u64, u32, u32)> = ov
                    .shields(k)
                    .into_iter()
                    .map(|r| {
                        (
                            r.spell_id,
                            (r.applied, r.consumed, r.wasted, r.count, r.unknown),
                        )
                    })
                    .collect();
                assert_eq!(got, want, "{name}: visit {ordinal} {k} rows");
                if !got.is_empty() {
                    nonempty += 1;
                }
                let wastes: Vec<u64> = members.iter().filter_map(|m| m.absorb_wasted(k)).collect();
                let want_waste = (!wastes.is_empty()).then(|| wastes.iter().sum::<u64>());
                assert_eq!(
                    ov.absorb_wasted(k),
                    want_waste,
                    "{name}: visit {ordinal} {k} waste"
                );
                let unknown: u32 = members.iter().map(|m| m.shields_unknown(k)).sum();
                assert_eq!(
                    ov.shields_unknown(k),
                    unknown,
                    "{name}: visit {ordinal} {k} unknown"
                );
                assert_eq!(consumed(&ov, k), ov.absorbed_healing(k));
                // The rows stay sorted by consumed desc.
                let rows = ov.shields(k);
                assert!(rows.windows(2).all(|w| w[0].consumed >= w[1].consumed));
            }
        }
    }
    assert!(visits > 0, "some fixture has an instance visit");
    assert!(nonempty > 0, "some Overall carries a shield row");
}
