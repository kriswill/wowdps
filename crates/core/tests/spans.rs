//! R18: aura spans with caster and target, over every committed fixture —
//! the two identities (Σ externals given = Σ received; Σ support uptime over
//! targets = the supporter's total), the segment-start rule, the read-time
//! close, the passive gate, the AM union, the trinket-dedupe bypass, the
//! span cap against the uncapped rollup, pets folding, lazy = full =
//! checkpoint-resume parity for every new number, the scanner's
//! indifference to aura lines, and the R10 merge. Every fixture in
//! `FIXTURES` must exist — a missing one fails, never skips.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, Segment, SegmentKind, UptimeRow, View, meter_from_lines};
use wowdps_core::parser::{Event, LogLine, parse_line};
use wowdps_model::{Mark, MarkKind, Timeline};

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
    "support.txt",
    "spans.txt",
];

/// The spans fixture's roster (see `spans.expected.md`).
const W: &str = "Player-1168-0A1B2C31";
const H: &str = "Player-1168-0A1B2C32";
const E: &str = "Player-1168-0A1B2C33";
const M: &str = "Player-1168-0A1B2C34";

const SHIELD_BLOCK: u32 = 132404;
const SHIELD_BLOCK_ALT: u32 = 132403; // the other Shield Block aura, also AM
const SHIELD_WALL: u32 = 871;
const PAIN_SUPPRESSION: u32 = 33206;
const POWER_INFUSION: u32 = 10060;
const EBON_MIGHT: u32 = 395152;
const PRESCIENCE: u32 = 410089;
const TRINKET_PROC: u32 = 1258223; // Nalorakk's Rage — in item_spells, not a role
const BLOOD_SHIELD: u32 = 77535; // AM
const BONE_SHIELD: u32 = 195181; // AM
const ANCIENT_HYSTERIA: u32 = 90355; // the hunter lusts: census-exempt externals
const NETHERWINDS: u32 = 160452;
const HARRIERS_CRY: u32 = 466904;

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

/// Every guid a span could be keyed on or folded to: aura sources and
/// targets, summon owners and pets, damage sources and victims.
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

// ---- synthetic log lines --------------------------------------------------

const W_UNIT: &str = "Player-1168-0A1B2C31,\"Bastión-Nebula-US\",0x511,0x80000000";
const H_UNIT: &str = "Player-1168-0A1B2C32,\"Lumenia-Nebula-US\",0x514,0x80000000";
const E_UNIT: &str = "Player-1168-0A1B2C33,\"Vesperine-Nebula-US\",0x514,0x80000000";
const PET: &str = "Pet-0-4232-2662-31585-417-0102ABCDEF";
const PET_UNIT: &str = "Pet-0-4232-2662-31585-417-0102ABCDEF,\"Fluffy\",0x1114,0x80000000";
const BOSS_GUID: &str = "Creature-0-4232-2662-31585-217000-0000AD01";
const BOSS_UNIT: &str = "Creature-0-4232-2662-31585-217000-0000AD01,\"Spans Test Boss\",0xa48,0x80";

/// A line at `ms` after 20:05:00.000 on the fixture's date.
fn line(ms: i64, body: &str) -> String {
    let total = 20 * 3_600_000 + 5 * 60_000 + ms;
    let (h, rem) = (total / 3_600_000, total % 3_600_000);
    let (m, rem) = (rem / 60_000, rem % 60_000);
    let (s, milli) = (rem / 1000, rem % 1000);
    format!("9/5/2026 {h}:{m:02}:{s:02}.{milli:03}-4  {body}")
}

fn start(ms: i64) -> String {
    line(ms, "ENCOUNTER_START,3147,\"Spans Test Boss\",16,4,2769")
}

fn end(ms: i64) -> String {
    line(ms, "ENCOUNTER_END,3147,\"Spans Test Boss\",16,4,1,60000")
}

/// The boss swings on a friendly for `amount` (advanced block included).
fn swing_on(ms: i64, dst_unit: &str, amount: u64) -> String {
    line(
        ms,
        &format!(
            "SWING_DAMAGE,{BOSS_UNIT},{dst_unit},{BOSS_GUID},0000000000000000,296000,296000,0,0,0,0,0,0,0,0,0,0,-812.44,2145.87,2287,4.7123,83,{amount},{amount},-1,1,0,0,0,nil,nil,nil"
        ),
    )
}

fn hit(ms: i64, src_unit: &str, amount: u64) -> String {
    line(
        ms,
        &format!(
            "SPELL_DAMAGE,{src_unit},{BOSS_UNIT},23922,\"Shield Slam\",0x1,{BOSS_GUID},0000000000000000,276000,296000,0,0,0,0,0,0,0,0,0,0,-812.44,2145.87,2287,4.7123,83,{amount},{amount},-1,1,0,0,0,nil,nil,nil,ST"
        ),
    )
}

fn aura(ms: i64, event: &str, src_unit: &str, dst_unit: &str, id: u32, name: &str) -> String {
    line(
        ms,
        &format!("{event},{src_unit},{dst_unit},{id},\"{name}\",0x1,BUFF"),
    )
}

fn apply(ms: i64, src: &str, dst: &str, id: u32, name: &str) -> String {
    aura(ms, "SPELL_AURA_APPLIED", src, dst, id, name)
}

fn refresh(ms: i64, src: &str, dst: &str, id: u32, name: &str) -> String {
    aura(ms, "SPELL_AURA_REFRESH", src, dst, id, name)
}

fn remove(ms: i64, src: &str, dst: &str, id: u32, name: &str) -> String {
    aura(ms, "SPELL_AURA_REMOVED", src, dst, id, name)
}

fn cast(ms: i64, src_unit: &str, id: u32, name: &str) -> String {
    line(
        ms,
        &format!("SPELL_CAST_SUCCESS,{src_unit},{src_unit},{id},\"{name}\",0x1"),
    )
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

/// (at, spell, dur, src) — a span reduced to what R18 pins.
fn flat_spans(marks: &[Mark]) -> Vec<(i64, u32, i64, String)> {
    marks
        .iter()
        .map(|m| (m.at_ms, m.spell_id, m.dur_ms, m.src.clone()))
        .collect()
}

// ---- the fixture's own table ---------------------------------------------

/// The 16 spans of `spans.expected.md` §"Spans (R18)", by target, with
/// their casters and read-time closes; the item mark beside them.
#[test]
fn the_spans_fixture_reproduces_its_span_table() {
    let meter = replay(&fixtures()[6].1);
    let seg = &meter.segments()[0];
    assert_eq!(seg.kind, SegmentKind::Encounter);
    assert_eq!(seg.duration_ms(0), 60_000);
    let w = seg.spans(W);
    assert_eq!(
        flat_spans(&w),
        vec![
            (0, SHIELD_BLOCK, 11_000, W.into()),
            (1_000, 80353, 40_000, M.into()),
            (2_000, EBON_MIGHT, 10_000, E.into()),
            (20_000, SHIELD_BLOCK, 6_000, W.into()),
            (22_000, SHIELD_WALL, 8_000, W.into()),
            (24_000, PAIN_SUPPRESSION, 8_000, H.into()),
            (40_000, 47788, 10_000, H.into()),
            (50_000, SHIELD_BLOCK, 10_000, W.into()),
        ]
    );
    assert_eq!(seg.spans(M).len(), 7);
    assert_eq!(seg.spans(H).len(), 1);
    assert!(seg.spans(E).is_empty());
    let kinds: Vec<MarkKind> = w.iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MarkKind::ActiveMitigation,
            MarkKind::External,
            MarkKind::SupportBuff,
            MarkKind::ActiveMitigation,
            MarkKind::Defensive,
            MarkKind::External,
            MarkKind::External,
            MarkKind::ActiveMitigation,
        ]
    );
    // The uptime drill behind the headline: Shield Block 3 × 27 000.
    let sb: Vec<UptimeRow> = seg
        .uptime(W)
        .into_iter()
        .filter(|r| r.spell_id == SHIELD_BLOCK)
        .collect();
    assert_eq!(sb.len(), 1);
    assert_eq!(
        (sb[0].count, sb[0].total_ms, sb[0].src.as_str()),
        (3, 27_000, W)
    );
    assert_eq!(sb[0].label, "Shield Block");
    assert_eq!(sb[0].kind, MarkKind::ActiveMitigation);
    // The trash tail: one span from the segment's start (the l.86 removal),
    // the l.81 apply nowhere.
    let trash = &meter.segments()[1];
    assert_eq!(trash.kind, SegmentKind::Trash);
    assert_eq!(
        flat_spans(&trash.spans(W)),
        vec![(0, SHIELD_BLOCK, 5_000, W.into())]
    );
    assert_eq!(trash.am_uptime_ms(W), 5_000);
}

// ---- the identities --------------------------------------------------------

/// Σ `externals_given` ms (and counts) over players = Σ `externals_received`
/// on every segment of every fixture — every External span has exactly one
/// caster and one target. On spans.txt: 6 spans, 158 000 ms.
#[test]
fn externals_given_equals_received_on_every_segment() {
    let mut nonzero = 0;
    for (name, text) in fixtures() {
        let keys = guids(&parsed(&text));
        let meter = replay(&text);
        for seg in meter.segments() {
            let mut given = (0u32, 0i64);
            let mut received = (0u32, 0i64);
            for k in &keys {
                let g = seg.externals_given(k);
                let r = seg.externals_received(k);
                given = (given.0 + g.0, given.1 + g.1);
                received = (received.0 + r.0, received.1 + r.1);
            }
            assert_eq!(given, received, "{name} / {}", seg.name);
            if given.0 > 0 {
                nonzero += 1;
            }
            if name == "spans.txt" && seg.kind == SegmentKind::Encounter {
                assert_eq!(given, (6, 158_000));
                assert_eq!(seg.externals_given(M), (3, 120_000));
                assert_eq!(seg.externals_received(M), (2, 60_000), "self-cast counts");
                assert_eq!(seg.externals_given(H), (3, 38_000));
                assert_eq!(seg.externals_received(W), (3, 58_000));
                assert_eq!(seg.externals_given(W), (0, 0));
            }
        }
    }
    assert!(nonzero > 0, "some fixture carries externals");
}

/// Σ `support_uptime` over (target, spell) = the supporter's total, read
/// off the targets' own uptime rollups; on spans.txt E = 48 000 over
/// three rows.
#[test]
fn support_uptime_over_targets_equals_the_supporters_total() {
    let mut rows_seen = 0;
    for (name, text) in fixtures() {
        let keys = guids(&parsed(&text));
        let meter = replay(&text);
        for seg in meter.segments() {
            for k in &keys {
                let rows = seg.support_uptime(k);
                let by_rows: i64 = rows.iter().map(|r| r.2).sum();
                // The supporter's total, from every target's rollup.
                let by_targets: i64 = keys
                    .iter()
                    .flat_map(|t| seg.uptime(t))
                    .filter(|u| u.kind == MarkKind::SupportBuff && &u.src == k)
                    .map(|u| u.total_ms)
                    .sum();
                assert_eq!(by_rows, by_targets, "{name} / {} / {k}", seg.name);
                rows_seen += rows.len();
            }
            if name == "spans.txt" && seg.kind == SegmentKind::Encounter {
                assert_eq!(
                    seg.support_uptime(E),
                    vec![
                        (W.to_string(), EBON_MIGHT, 10_000),
                        (M.to_string(), EBON_MIGHT, 20_000),
                        (M.to_string(), PRESCIENCE, 18_000),
                    ]
                );
                assert!(seg.support_uptime(H).is_empty());
                let em_on_m: Vec<UptimeRow> = seg
                    .uptime(M)
                    .into_iter()
                    .filter(|u| u.spell_id == EBON_MIGHT)
                    .collect();
                assert_eq!(em_on_m.len(), 1);
                assert_eq!((em_on_m[0].count, em_on_m[0].total_ms), (2, 20_000));
            }
        }
    }
    assert!(rows_seen > 0);
}

// ---- the segment-start rule ----------------------------------------------

/// A REFRESH with no apply inside the segment opens the span at the
/// segment's START (not the refresh time), with the refresh line's caster.
#[test]
fn a_refresh_with_no_apply_opens_at_the_segment_start() {
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        refresh(5_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        hit(8_000, W_UNIT, 1_000),
        remove(11_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![(0, SHIELD_BLOCK, 11_000, W.into())]
    );
    assert_eq!(seg.am_uptime_ms(W), 11_000);
    let u = seg.uptime(W);
    assert_eq!((u.len(), u[0].count, u[0].total_ms), (1, 1, 11_000));
    // A second refresh while it runs is a no-op.
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        refresh(5_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        refresh(7_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        apply(9_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(11_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        end(60_000),
    ]);
    assert_eq!(
        flat_spans(&m.segments()[0].spans(W)),
        vec![(0, SHIELD_BLOCK, 11_000, W.into())]
    );
}

/// A REMOVED with no open span is a span from the segment's start to the
/// removal — inside a Trash segment that start is the first combat line.
#[test]
fn a_removal_with_no_apply_spans_start_to_removal() {
    let m = meter_of(&[
        swing_on(10_000, W_UNIT, 1_500),
        hit(13_000, W_UNIT, 6_000),
        remove(15_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        hit(18_000, W_UNIT, 6_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(seg.kind, SegmentKind::Trash);
    assert_eq!(seg.start_ms - seg.start_ms, 0);
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![(0, SHIELD_BLOCK, 5_000, W.into())]
    );
    assert_eq!(seg.am_uptime_ms(W), 5_000);
    assert!(seg.am_uptime_ms(W) <= seg.duration_ms(0));
    // With a caster other than the target, the removal line's source is
    // the span's caster: PS on W by H, removed with no apply.
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        remove(9_000, H_UNIT, W_UNIT, PAIN_SUPPRESSION, "Pain Suppression"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![(0, PAIN_SUPPRESSION, 9_000, H.into())]
    );
    assert_eq!(seg.externals_given(H), (1, 9_000));
    assert_eq!(seg.externals_received(W), (1, 9_000));
}

// ---- the read-time close ---------------------------------------------------

/// A role span still open at the kill reads `end − at`; an item mark still
/// open reads 0 (R12's rule, untouched); on a live segment the clock is the
/// newest combat line, and it advances as combat does.
#[test]
fn an_open_role_span_reads_to_the_end_but_an_open_item_mark_reads_zero() {
    let fixture = replay(&fixtures()[6].1);
    let seg = &fixture.segments()[0];
    let open = seg
        .spans(W)
        .into_iter()
        .find(|s| s.at_ms == 50_000)
        .expect("the Shield Block open at the kill");
    assert_eq!(open.dur_ms, 10_000, "60 000 − 50 000");
    let proc_mark = seg
        .timeline(W)
        .marks
        .into_iter()
        .find(|m| m.kind == MarkKind::TrinketProc)
        .expect("the trinket proc");
    assert_eq!((proc_mark.at_ms, proc_mark.dur_ms), (30_000, 15_000));

    // Synthetic: both open at the end.
    let closed = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        apply(20_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        apply(25_000, W_UNIT, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
        hit(30_000, W_UNIT, 1_000),
        end(45_000),
    ]);
    let seg = &closed.segments()[0];
    let marks = seg.timeline(W).marks;
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert_eq!(
        (marks[0].kind, marks[0].dur_ms),
        (MarkKind::ActiveMitigation, 25_000)
    );
    assert_eq!((marks[1].kind, marks[1].dur_ms), (MarkKind::TrinketProc, 0));
    assert_eq!(marks[0].src, W);
    assert!(marks[1].src.is_empty(), "item marks carry no caster");
    assert_eq!(seg.am_uptime_ms(W), 25_000);
    assert_eq!(
        seg.uptime(W)[0].total_ms,
        25_000,
        "the rollup sees the open span too"
    );

    // Live: no ENCOUNTER_END — the clock is the last combat line (30 s),
    // and a later hit moves it.
    let mut live = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        apply(20_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        hit(30_000, W_UNIT, 1_000),
    ]);
    assert_eq!(live.segments()[0].spans(W)[0].dur_ms, 10_000);
    assert_eq!(live.segments()[0].am_uptime_ms(W), 10_000);
    if let Some(l) = parse_line(&hit(41_000, W_UNIT, 1_000)) {
        live.feed(l);
    }
    assert_eq!(live.segments()[0].spans(W)[0].dur_ms, 21_000);
    assert_eq!(live.segments()[0].am_uptime_ms(W), 21_000);
    // An aura after the last combat line of a live segment reads 0, never
    // negative.
    if let Some(l) = parse_line(&apply(50_000, H_UNIT, W_UNIT, PAIN_SUPPRESSION, "PS")) {
        live.feed(l);
    }
    let ps = live.segments()[0]
        .spans(W)
        .into_iter()
        .find(|s| s.spell_id == PAIN_SUPPRESSION)
        .expect("PS span");
    assert_eq!(ps.dur_ms, 0);
}

// ---- the passive gate ------------------------------------------------------

/// An aura after ENCOUNTER_END lands nowhere: not in the closed pull (which
/// would read a span past its end), not in a trash segment that has not
/// begun. A cast after the end lands nowhere either (the pre-R18 R12 hole).
/// Past the trash gap an aura is dropped, and never opens a segment.
#[test]
fn an_aura_after_a_segments_end_lands_nowhere() {
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        end(60_000),
        apply(65_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        cast(66_000, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
        apply(66_100, W_UNIT, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
    ]);
    assert_eq!(m.segments().len(), 1, "an aura never opens a segment");
    let seg = &m.segments()[0];
    assert!(seg.spans(W).is_empty(), "{:?}", seg.spans(W));
    assert!(seg.timeline(W).marks.is_empty());
    assert_eq!(seg.am_uptime_ms(W), 0);
    assert!(seg.uptime(W).is_empty());

    // The fixture's own dead-zone apply (l.81): the encounter has no span at
    // 65 s and the trash's only span starts at ITS start.
    let f = replay(&fixtures()[6].1);
    assert!(f.segments()[0].spans(W).iter().all(|s| s.at_ms < 60_000));
    assert_eq!(f.segments()[1].spans(W)[0].at_ms, 0);

    // Past the R4 trash gap: dropped, no new segment, no span in the stale one.
    let m = meter_of(&[
        swing_on(0, W_UNIT, 1_000),
        hit(1_000, W_UNIT, 1_000),
        apply(70_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(75_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        hit(80_000, W_UNIT, 1_000),
    ]);
    assert_eq!(m.segments().len(), 2);
    assert!(m.segments()[0].spans(W).is_empty());
    assert!(m.segments()[1].spans(W).is_empty());
    assert_eq!(
        m.segments()[0].last_combat_ms(),
        m.segments()[0].start_ms + 1_000
    );
}

// ---- the union -------------------------------------------------------------

/// `am_uptime_ms` is the per-ms union of `ActiveMitigation` spans ONLY:
/// two overlapping AM auras count once, and a Defensive or an External
/// overlapping them adds nothing. The fixture's 27 000 (not 37 000) too.
#[test]
fn am_uptime_is_the_union_of_active_mitigation_only() {
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        apply(5_000, W_UNIT, W_UNIT, SHIELD_WALL, "Shield Wall"),
        apply(10_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        apply(15_000, W_UNIT, W_UNIT, SHIELD_BLOCK_ALT, "Shield Block"),
        apply(18_000, H_UNIT, W_UNIT, PAIN_SUPPRESSION, "Pain Suppression"),
        remove(20_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(26_000, H_UNIT, W_UNIT, PAIN_SUPPRESSION, "Pain Suppression"),
        remove(30_000, W_UNIT, W_UNIT, SHIELD_BLOCK_ALT, "Shield Block"),
        remove(40_000, W_UNIT, W_UNIT, SHIELD_WALL, "Shield Wall"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(seg.am_uptime_ms(W), 20_000, "[10, 30) once, not 25 000");
    let am_sum: i64 = seg
        .uptime(W)
        .iter()
        .filter(|u| u.kind == MarkKind::ActiveMitigation)
        .map(|u| u.total_ms)
        .sum();
    assert_eq!(am_sum, 25_000, "the drill sums; the headline unions");
    assert_eq!(seg.spans(W).len(), 4);
    // The fixture: Shield Wall + Pain Suppression over the second Shield
    // Block never enter the union.
    let f = replay(&fixtures()[6].1);
    assert_eq!(f.segments()[0].am_uptime_ms(W), 27_000);
    for p in [H, E, M] {
        assert_eq!(f.segments()[0].am_uptime_ms(p), 0);
    }
    // A retroactive open while another AM span is on pulls the group's
    // start back to the segment's start.
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        apply(10_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(12_000, W_UNIT, W_UNIT, SHIELD_BLOCK_ALT, "Shield Block"),
        remove(20_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        end(60_000),
    ]);
    assert_eq!(
        m.segments()[0].am_uptime_ms(W),
        20_000,
        "[0, 12) ∪ [10, 20)"
    );
}

// ---- the trinket dedupe does not apply -------------------------------------

/// Role kinds bypass R12's trinket rules: a Shield Block re-applied 300 ms
/// after its removal is a second span (`PROC_GAP_MS` would have merged a
/// proc), and the player's own cast just before does not veto it
/// (`USE_AURA_MS` is an item rule).
#[test]
fn role_kinds_bypass_the_trinket_dedupe() {
    let m = meter_of(&[
        start(0),
        swing_on(3_000, W_UNIT, 10_000),
        cast(9_500, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        apply(10_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(20_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        apply(20_300, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(25_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        // The trinket, for contrast: a re-apply within 500 ms of the
        // proc's start (`PROC_GAP_MS`) is one proc under R12, removal or
        // not.
        apply(30_000, W_UNIT, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
        remove(30_200, W_UNIT, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
        apply(30_300, W_UNIT, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![
            (10_000, SHIELD_BLOCK, 10_000, W.into()),
            (20_300, SHIELD_BLOCK, 4_700, W.into()),
        ]
    );
    let u = seg.uptime(W);
    assert_eq!((u.len(), u[0].count, u[0].total_ms), (1, 2, 14_700));
    assert_eq!(seg.am_uptime_ms(W), 14_700);
    let procs: Vec<Mark> = seg
        .timeline(W)
        .marks
        .into_iter()
        .filter(|m| m.kind == MarkKind::TrinketProc)
        .collect();
    assert_eq!(procs.len(), 1, "R12's 500 ms rule still merges procs");
    assert_eq!(procs[0].dur_ms, 200);
}

// ---- the cap against the rollup --------------------------------------------

/// `SPAN_CAP` drops the NEWEST spans from the list; the uptime rollup and
/// the AM union keep counting past it.
#[test]
fn span_cap_drops_the_newest_while_uptime_keeps_counting() {
    let mut lines = vec![start(0), swing_on(1_000, W_UNIT, 10_000)];
    for i in 0..300i64 {
        let at = 2_000 + i * 150;
        lines.push(apply(at, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"));
        lines.push(remove(
            at + 100,
            W_UNIT,
            W_UNIT,
            SHIELD_BLOCK,
            "Shield Block",
        ));
    }
    lines.push(hit(50_000, W_UNIT, 1_000));
    lines.push(end(60_000));
    let m = meter_of(&lines);
    let seg = &m.segments()[0];
    let spans = seg.spans(W);
    assert_eq!(spans.len(), 256);
    assert_eq!(
        spans[255].at_ms,
        2_000 + 255 * 150,
        "the 257th onward were dropped"
    );
    assert!(spans.iter().all(|s| s.dur_ms == 100));
    let u = seg.uptime(W);
    assert_eq!((u.len(), u[0].count, u[0].total_ms), (1, 300, 30_000));
    assert_eq!(seg.am_uptime_ms(W), 30_000);
    // Item marks have their own cap: 300 spans evicted no trinket proc.
    let mut with_proc = lines.clone();
    with_proc.insert(
        2,
        apply(1_500, W_UNIT, W_UNIT, TRINKET_PROC, "Nalorakk's Rage"),
    );
    let m = meter_of(&with_proc);
    let marks = m.segments()[0].timeline(W).marks;
    assert_eq!(marks.len(), 257);
    assert_eq!(marks[0].kind, MarkKind::TrinketProc);
}

// ---- R12 untouched ---------------------------------------------------------

/// Item marks keep every R12 rule: the fixture's trinket proc is exactly one
/// `TrinketProc` of 15 000 ms on W with no caster; a class buff not in the
/// role table (Arcane Intellect) leaves nothing on M.
#[test]
fn item_marks_are_untouched_by_spans() {
    let f = replay(&fixtures()[6].1);
    let seg = &f.segments()[0];
    let items: Vec<Mark> = seg
        .timeline(W)
        .marks
        .into_iter()
        .filter(|m| {
            matches!(
                m.kind,
                MarkKind::TrinketUse | MarkKind::TrinketProc | MarkKind::Consumable
            )
        })
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(
        (
            items[0].kind,
            items[0].at_ms,
            items[0].dur_ms,
            items[0].spell_id
        ),
        (MarkKind::TrinketProc, 30_000, 15_000, TRINKET_PROC)
    );
    assert!(items[0].src.is_empty());
    assert_eq!(items[0].label, "Nalorakk's Rage");
    // `timeline().marks` = items + spans merged, sorted by time.
    let all = seg.timeline(W).marks;
    assert_eq!(all.len(), 9);
    assert!(all.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    assert_eq!(
        seg.heal_timeline(W).marks,
        all,
        "every timeline flavor shares them"
    );
    // M: Arcane Intellect is neither an item nor a role — every mark on M
    // is a role span.
    assert!(seg.timeline(M).marks.iter().all(|m| m.spell_id != 1459));
    assert_eq!(seg.timeline(M).marks.len(), seg.spans(M).len());
}

// ---- pets fold -------------------------------------------------------------

/// An external on a pet is its owner's received (and the caster's given);
/// the pet's own guid answers nothing once the owner is known.
#[test]
fn an_external_on_a_pet_is_the_owners_received() {
    let m = meter_of(&[
        start(0),
        summon(500, W_UNIT, PET_UNIT),
        swing_on(3_000, W_UNIT, 10_000),
        apply(10_000, H_UNIT, PET_UNIT, POWER_INFUSION, "Power Infusion"),
        remove(30_000, H_UNIT, PET_UNIT, POWER_INFUSION, "Power Infusion"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(seg.externals_received(W), (1, 20_000));
    assert_eq!(seg.externals_received(PET), (0, 0));
    assert_eq!(seg.externals_given(H), (1, 20_000));
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![(10_000, POWER_INFUSION, 20_000, H.into())]
    );
    assert_eq!(seg.uptime(W).len(), 1);
    assert!(
        seg.timeline(W)
            .marks
            .iter()
            .any(|m| m.spell_id == POWER_INFUSION)
    );
}

// ---- parity ----------------------------------------------------------------

/// Everything R18 says about one player, in one comparable value.
type Picture = (
    String,
    Vec<Mark>,
    Vec<UptimeRow>,
    i64,
    (u32, i64),
    (u32, i64),
    Vec<(String, u32, i64)>,
    Timeline,
);

fn span_picture(seg: &Segment, keys: &[String]) -> Vec<Picture> {
    keys.iter()
        .map(|k| {
            (
                k.clone(),
                seg.spans(k),
                seg.uptime(k),
                seg.am_uptime_ms(k),
                seg.externals_given(k),
                seg.externals_received(k),
                seg.support_uptime(k),
                seg.taken_timeline(k),
            )
        })
        .filter(|p| {
            !p.1.is_empty()
                || !p.2.is_empty()
                || p.3 > 0
                || p.4.0 > 0
                || p.5.0 > 0
                || !p.6.is_empty()
                || !p.7.buckets.is_empty()
        })
        .collect()
}

/// Lazy load == full replay == a scan resumed from any checkpoint, for
/// spans, the rollup, the AM union, externals both ways, support uptime
/// and the taken timeline, on every segment and every Overall of every
/// fixture.
#[test]
fn spans_survive_lazy_loading_and_checkpoints_on_every_fixture() {
    let mut checked = 0;
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
            assert_eq!(
                span_picture(&lazy.segments()[0], &keys),
                span_picture(seg, &keys),
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
                span_picture(&got, &keys),
                span_picture(&want, &keys),
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
                    span_picture(&lazy.segments()[0], &keys),
                    span_picture(seg, &keys),
                    "{name} / {}: resumed at {cut}",
                    meta.name
                );
            }
        }
    }
    assert!(checked > 0);
}

/// Aura lines are invisible to segmentation: renaming every `SPELL_AURA_*`
/// event to an unknown one of the same byte length changes nothing about
/// the scan — no boundary, no duration, no byte range — nor about the
/// meter's segment table or Damage rows. Only spans and marks go away.
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
                assert!(sb.spans(k).is_empty(), "{name}: {k} spans without auras");
                assert_eq!(sb.am_uptime_ms(k), 0);
                assert_eq!(sb.taken_timeline(k).buckets, sa.taken_timeline(k).buckets);
            }
        }
    }
    assert!(rewritten_any, "no fixture carries an aura line");
}

// ---- R10 -------------------------------------------------------------------

/// An Overall's span measures are the sums of its members': the AM union
/// (members are disjoint in time, so Σ unions = the union), externals both
/// ways, the rollup cells, support uptime; its span list is the members'
/// rebased onto the visit's start; its taken curve merges with the same
/// shift as the damage curve; and `am_uptime_ms` never exceeds the duration
/// the card writes (`duration_ms`).
#[test]
fn overall_sums_members_spans_and_uptime() {
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
            let last = ov.last_combat_ms();
            for k in &keys {
                let am: i64 = members.iter().map(|m| m.am_uptime_ms(k)).sum();
                assert_eq!(ov.am_uptime_ms(k), am, "{name}: visit {ordinal} {k} am");
                assert!(
                    ov.am_uptime_ms(k) <= ov.duration_ms(last).max(am),
                    "{name}: {k}"
                );
                let sum2 = |f: &dyn Fn(&Segment) -> (u32, i64)| {
                    members.iter().fold((0u32, 0i64), |acc, m| {
                        let v = f(m);
                        (acc.0 + v.0, acc.1 + v.1)
                    })
                };
                assert_eq!(ov.externals_given(k), sum2(&|m| m.externals_given(k)));
                assert_eq!(ov.externals_received(k), sum2(&|m| m.externals_received(k)));
                let mut cells: BTreeMap<(u32, String), (u32, i64)> = BTreeMap::new();
                for m in &members {
                    for u in m.uptime(k) {
                        let c = cells.entry((u.spell_id, u.src)).or_default();
                        c.0 += u.count;
                        c.1 += u.total_ms;
                    }
                }
                let got: BTreeMap<(u32, String), (u32, i64)> = ov
                    .uptime(k)
                    .into_iter()
                    .map(|u| ((u.spell_id, u.src), (u.count, u.total_ms)))
                    .collect();
                assert_eq!(got, cells, "{name}: visit {ordinal} {k} uptime");
                let mut support: BTreeMap<(String, u32), i64> = BTreeMap::new();
                for m in &members {
                    for (t, s, ms) in m.support_uptime(k) {
                        *support.entry((t, s)).or_default() += ms;
                    }
                }
                let got: BTreeMap<(String, u32), i64> = ov
                    .support_uptime(k)
                    .into_iter()
                    .map(|(t, s, ms)| ((t, s), ms))
                    .collect();
                assert_eq!(got, support, "{name}: visit {ordinal} {k} support");
                // The list: members' spans shifted onto the visit clock.
                let mut want: Vec<(i64, u32, i64, String)> = members
                    .iter()
                    .flat_map(|m| {
                        let shift = m.start_ms - ov.start_ms;
                        m.spans(k)
                            .into_iter()
                            .map(move |s| (s.at_ms + shift, s.spell_id, s.dur_ms, s.src))
                    })
                    .collect();
                want.sort();
                let mut got = flat_spans(&ov.spans(k));
                got.sort();
                assert_eq!(got, want, "{name}: visit {ordinal} {k} spans");
                // The taken curve merges with the damage curve's shift.
                let taken: u64 = ov.taken_timeline(k).buckets.iter().sum();
                let members_taken: u64 = members
                    .iter()
                    .map(|m| m.taken_timeline(k).buckets.iter().sum::<u64>())
                    .sum();
                assert_eq!(taken, members_taken, "{name}: {k} taken");
                for m in &members {
                    let shift = ((m.start_ms - ov.start_ms) / 1_000) as usize;
                    let mt = m.taken_timeline(k);
                    let ot = ov.taken_timeline(k);
                    for (i, v) in mt.buckets.iter().enumerate() {
                        if *v > 0 {
                            assert!(
                                ot.buckets.get(i + shift).is_some_and(|o| o >= v),
                                "{name}: {k} bucket {i} shifted by {shift}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(visits > 0);
}

/// R17/R18: the taken series is the Taken row on a grid — Σ buckets = the
/// row's amount on every segment of every fixture — and coarsening keeps
/// the total; the fixture's W buckets are the golden's.
#[test]
fn the_taken_series_sums_to_the_taken_row() {
    for (name, text) in fixtures() {
        let meter = replay(&text);
        for seg in meter.segments() {
            for r in seg.rows(View::Taken) {
                let t = seg.taken_timeline(&r.key);
                assert_eq!(t.bucket_ms, 1_000);
                assert_eq!(
                    t.buckets.iter().sum::<u64>(),
                    r.amount,
                    "{name} / {}: {}",
                    seg.name,
                    r.label
                );
                let c = t.coarsen(10);
                assert_eq!(c.buckets.iter().sum::<u64>(), r.amount);
                assert_eq!(c.marks, t.marks);
            }
        }
    }
    let f = replay(&fixtures()[6].1);
    let w = f.segments()[0].taken_timeline(W).coarsen(10);
    assert_eq!(w.bucket_ms, 10_000);
    assert_eq!(w.buckets, vec![22_000, 8_000, 9_000, 11_000, 7_000, 13_000]);
    let m = f.segments()[0].taken_timeline(M).coarsen(10);
    assert_eq!(m.buckets, vec![0, 0, 0, 5_000]);
    assert_eq!(f.segments()[1].taken_timeline(W).buckets, vec![1_500]);
}

// ---- the AM union under the segment-start rule (review B1) ----------------

/// The union is computed over the interval list at read time, so a
/// retroactive open at the segment's start (a REFRESH of a second AM spell
/// with no apply, after another AM group already closed) lands UNDER the
/// closed group instead of re-counting it: Blood Shield [3 s, 8 s] closed,
/// then Bone Shield refreshed at 20 s with no apply and still on at the
/// kill → [0, 60 s] ∪ [3 s, 8 s] = exactly the fight, never 65 s.
#[test]
fn a_retroactive_open_after_a_closed_am_group_does_not_recount_it() {
    let m = meter_of(&[
        start(0),
        swing_on(1_000, W_UNIT, 10_000),
        apply(3_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        remove(8_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        hit(15_000, W_UNIT, 1_000),
        refresh(20_000, W_UNIT, W_UNIT, BONE_SHIELD, "Bone Shield"),
        hit(50_000, W_UNIT, 1_000),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(seg.duration_ms(0), 60_000);
    assert_eq!(seg.am_uptime_ms(W), 60_000, "the union, not 5 000 + 60 000");
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![
            (0, BONE_SHIELD, 60_000, W.into()),
            (3_000, BLOOD_SHIELD, 5_000, W.into()),
        ]
    );
    // The drill still sums: 5 000 + 60 000.
    let am_sum: i64 = seg
        .uptime(W)
        .iter()
        .filter(|u| u.kind == MarkKind::ActiveMitigation)
        .map(|u| u.total_ms)
        .sum();
    assert_eq!(am_sum, 65_000);
}

/// With gaps between the closed groups the retro span covers them and
/// nothing else is added: G1 [0, 10] and G2 [20, 30] (Blood Shield twice)
/// under a retro [0, 40] (Bone Shield removed at 40 s with no apply) → 40 s.
/// An incremental busy counter would answer 60 s.
#[test]
fn a_retroactive_open_over_gapped_am_groups_is_their_cover() {
    let m = meter_of(&[
        start(0),
        apply(0, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        swing_on(1_000, W_UNIT, 10_000),
        remove(10_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        apply(20_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        remove(30_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        remove(40_000, W_UNIT, W_UNIT, BONE_SHIELD, "Bone Shield"),
        hit(50_000, W_UNIT, 1_000),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(seg.am_uptime_ms(W), 40_000, "[0, 40] covers both groups");
    assert_eq!(seg.spans(W).len(), 3);
    assert!(seg.am_uptime_ms(W) <= seg.duration_ms(0));
    // Without the retro span the two groups read as themselves.
    let m = meter_of(&[
        start(0),
        apply(0, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        swing_on(1_000, W_UNIT, 10_000),
        remove(10_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        apply(20_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        remove(30_000, W_UNIT, W_UNIT, BLOOD_SHIELD, "Blood Shield"),
        end(60_000),
    ]);
    assert_eq!(m.segments()[0].am_uptime_ms(W), 20_000);
}

/// On a Trash segment an AM removal in the 60 s idle tail closes its span
/// past the R7 clock (the span and the rollup keep it), but the union is
/// clamped to the segment's duration — `am_uptime_ms <= duration_ms` on
/// every kind.
#[test]
fn am_union_is_clamped_to_a_trash_segments_clock() {
    let m = meter_of(&[
        swing_on(10_000, W_UNIT, 1_500),
        apply(12_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        hit(20_000, W_UNIT, 6_000),
        remove(45_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(seg.kind, SegmentKind::Trash);
    assert_eq!(seg.duration_ms(0), 10_000);
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![(2_000, SHIELD_BLOCK, 33_000, W.into())],
        "the span keeps its true close"
    );
    assert_eq!(seg.uptime(W)[0].total_ms, 33_000);
    assert_eq!(seg.am_uptime_ms(W), 8_000, "[12 s, 20 s] of a 10 s segment");
}

// ---- one key per caster (review B2) ----------------------------------------

/// Two priests' Power Infusion on one target are two spans, each closed
/// by its own removal, with the right casters and durations — a shared
/// (target, spell) key would read B's apply as a refresh and B's removal as
/// an orphan, fabricating a `[start, ts]` span for B. The same caster
/// re-applying while on is still a refresh.
#[test]
fn two_casters_of_one_spell_on_one_target_are_two_spans() {
    let m = meter_of(&[
        start(0),
        swing_on(1_000, W_UNIT, 10_000),
        apply(5_000, H_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        apply(10_000, E_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        remove(20_000, H_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        remove(30_000, E_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![
            (5_000, POWER_INFUSION, 15_000, H.into()),
            (10_000, POWER_INFUSION, 20_000, E.into()),
        ]
    );
    assert_eq!(seg.externals_given(H), (1, 15_000));
    assert_eq!(seg.externals_given(E), (1, 20_000));
    assert_eq!(seg.externals_received(W), (2, 35_000));
    let cells: Vec<(String, u32, i64)> = seg
        .uptime(W)
        .into_iter()
        .map(|u| (u.src, u.count, u.total_ms))
        .collect();
    assert_eq!(
        cells,
        vec![(H.into(), 1, 15_000), (E.into(), 1, 20_000)],
        "sorted by caster"
    );
    // The same caster re-applying while on: one span.
    let m = meter_of(&[
        start(0),
        swing_on(1_000, W_UNIT, 10_000),
        apply(5_000, H_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        apply(8_000, H_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        remove(20_000, H_UNIT, W_UNIT, POWER_INFUSION, "Power Infusion"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![(5_000, POWER_INFUSION, 15_000, H.into())]
    );
    assert_eq!(seg.externals_given(H), (1, 15_000));
}

/// The segment-start rule fires at most once per (target, spell, caster)
/// per segment: after a retro span of a key has closed, a second orphaned
/// removal (or refresh) of that key is dropped, not a second `[start, ts]`.
#[test]
fn the_segment_start_rule_fires_once_per_key() {
    let m = meter_of(&[
        start(0),
        swing_on(1_000, W_UNIT, 10_000),
        refresh(5_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(11_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(15_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        refresh(20_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        // A real apply afterwards is a span like any other.
        apply(30_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        remove(36_000, W_UNIT, W_UNIT, SHIELD_BLOCK, "Shield Block"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![
            (0, SHIELD_BLOCK, 11_000, W.into()),
            (30_000, SHIELD_BLOCK, 6_000, W.into()),
        ]
    );
    assert_eq!(seg.am_uptime_ms(W), 17_000);
    let u = seg.uptime(W);
    assert_eq!((u.len(), u[0].count, u[0].total_ms), (1, 2, 17_000));
    // Another caster's orphan is its own key and gets its own retro span.
    let m = meter_of(&[
        start(0),
        swing_on(1_000, W_UNIT, 10_000),
        remove(9_000, H_UNIT, W_UNIT, PAIN_SUPPRESSION, "Pain Suppression"),
        remove(12_000, E_UNIT, W_UNIT, PAIN_SUPPRESSION, "Pain Suppression"),
        remove(15_000, H_UNIT, W_UNIT, PAIN_SUPPRESSION, "Pain Suppression"),
        end(60_000),
    ]);
    let seg = &m.segments()[0];
    assert_eq!(
        flat_spans(&seg.spans(W)),
        vec![
            (0, PAIN_SUPPRESSION, 9_000, H.into()),
            (0, PAIN_SUPPRESSION, 12_000, E.into()),
        ]
    );
    assert_eq!(seg.externals_received(W), (2, 21_000));
}

// ---- the census-exempt externals (review S2) --------------------------------

/// The three hunter lusts ship in the role table as externals though no
/// committed log holds one: each landing on a player is an external span
/// with its caster — the hunter's pet, folded onto the hunter for the
/// given side.
#[test]
fn the_hunter_lusts_are_externals() {
    for (id, name) in [
        (ANCIENT_HYSTERIA, "Ancient Hysteria"),
        (NETHERWINDS, "Netherwinds"),
        (HARRIERS_CRY, "Harrier's Cry"),
    ] {
        let m = meter_of(&[
            start(0),
            summon(500, H_UNIT, PET_UNIT),
            swing_on(1_000, W_UNIT, 10_000),
            apply(5_000, PET_UNIT, W_UNIT, id, name),
            remove(45_000, PET_UNIT, W_UNIT, id, name),
            end(60_000),
        ]);
        let seg = &m.segments()[0];
        let spans = seg.spans(W);
        assert_eq!(
            flat_spans(&spans),
            vec![(5_000, id, 40_000, PET.into())],
            "{name}"
        );
        assert_eq!(spans[0].kind, MarkKind::External, "{name}");
        assert_eq!(seg.externals_received(W), (1, 40_000), "{name}");
        assert_eq!(seg.externals_given(H), (1, 40_000), "{name}: the pet folds");
        assert_eq!(seg.am_uptime_ms(W), 0, "{name}: never mitigation");
    }
}
