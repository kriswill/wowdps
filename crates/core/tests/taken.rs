//! R17: damage taken and mitigation, over every committed fixture — the
//! dealt = taken identity through the public surface, lazy-load and
//! checkpoint-resume parity for the Taken view (rows, drill, mitigation),
//! and the scanner's indifference to `*_MISSED` lines. Every fixture in
//! `FIXTURES` must exist — a missing one fails, never skips.

use std::collections::HashSet;
use std::path::Path;

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, Row, Segment, View, meter_from_lines};
use wowdps_core::parser::{Event, LogLine, parse_line};

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
];

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

/// One row reduced to what R17 pins: key, label, amount, extra, count,
/// crits, spell id.
type Flat = (String, String, u64, u64, u64, u64, u32);

/// A row list or drill pane, the mitigation record beside it, and a rate.
type Picture = (Vec<Flat>, Option<wowdps_model::Mitigation>, f64);

fn flat(rows: &[Row]) -> Vec<Flat> {
    rows.iter()
        .map(|r| {
            (
                r.key.clone(),
                r.label.clone(),
                r.amount,
                r.extra,
                r.count,
                r.crits,
                r.spell_id,
            )
        })
        .collect()
}

/// Everything R17 says about one segment, in one comparable value.
fn taken_picture(seg: &Segment) -> Vec<Picture> {
    let rows = seg.rows(View::Taken);
    let mut out = vec![(flat(&rows), None, 0.0)];
    for r in &rows {
        let (by_spell, by_attacker) = seg.breakdown(&r.key, View::Taken);
        out.push((flat(&by_spell), seg.mitigation(&r.key), r.per_sec));
        out.push((flat(&by_attacker), None, r.pct));
    }
    out
}

/// THE IDENTITY, through nothing but the public surface: per segment, Σ over
/// every actor's Damage by_target for friendly NAMES = Σ Taken row amounts +
/// Σ stagger ticks (dealt under R1, excluded from Taken under R17).
#[test]
fn dealt_to_friendlies_equals_taken_on_every_segment() {
    let mut checked = 0;
    for (name, text) in fixtures() {
        let lines = parsed(&text);
        // Friendly victims by name, and every guid a Damage drill could be
        // keyed on: damage sources, pet owners (summons and the advanced
        // block's ownerGUID) — `breakdown` folds pets onto owners, so an
        // owner that never swung still answers for its pet.
        let mut friendly: HashSet<String> = HashSet::new();
        let mut guids: HashSet<String> = HashSet::new();
        for l in &lines {
            if let Some(h) = &l.owner_hint {
                guids.insert(h.owner_guid.clone());
            }
            match &l.event {
                Event::Damage { src, dst, .. } => {
                    guids.insert(src.guid.clone());
                    if dst.guid.starts_with("Player-") || dst.guid.starts_with("Pet-") {
                        friendly.insert(dst.name.clone());
                    }
                }
                Event::Summon { owner, .. } => {
                    guids.insert(owner.guid.clone());
                }
                _ => {}
            }
        }
        let meter = replay(&text);
        for seg in meter.segments() {
            let dealt: u64 = guids
                .iter()
                .flat_map(|g| seg.breakdown(g, View::Damage).1)
                .filter(|r| friendly.contains(&r.label))
                .map(|r| r.amount)
                .sum();
            let rows = seg.rows(View::Taken);
            let taken: u64 = rows.iter().map(|r| r.amount).sum();
            let ticked: u64 = rows
                .iter()
                .filter_map(|r| seg.mitigation(&r.key))
                .map(|m| m.stagger_ticked)
                .sum();
            assert_eq!(
                dealt,
                taken + ticked,
                "{name} / {}: dealt to friendlies vs taken (+ticked {ticked})",
                seg.name
            );
            // The drill panes total the row: by ability and by attacker.
            for r in &rows {
                let (by_spell, by_attacker) = seg.breakdown(&r.key, View::Taken);
                let spells: u64 = by_spell.iter().map(|s| s.amount).sum();
                let attackers: u64 = by_attacker.iter().map(|s| s.amount).sum();
                assert_eq!(
                    (spells, attackers),
                    (r.amount, r.amount),
                    "{name}: {}",
                    r.label
                );
                let counts: u64 = by_attacker.iter().map(|s| s.count).sum();
                assert_eq!(
                    counts, r.count,
                    "{name}: {} misses count on the drill",
                    r.label
                );
                assert!(r.count > 0, "{name}: a Taken row lists on count > 0");
                let m = seg
                    .mitigation(&r.key)
                    .expect("every listed victim has a record");
                assert_eq!(m.absorbed, r.extra, "{name}: {} extra = absorbed", r.label);
                assert!(m.mitigated_pct(r.amount) <= 100.0);
            }
            checked += 1;
        }
    }
    assert!(checked > 0);
}

/// Lazy load == full replay for the Taken view — rows, both drill panes,
/// `per_sec`/`pct`, and the mitigation record — on every segment of every
/// fixture, closed and open alike.
#[test]
fn taken_survives_lazy_loading_on_every_fixture() {
    let mut checked = 0;
    for (name, text) in fixtures() {
        let path = fixture_path(name);
        let bytes = text.as_bytes();
        let idx = scan(&mut &bytes[..]);
        let full = replay(&text);
        let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
        assert_eq!(metas.len(), full.segments().len(), "{name}: segment count");
        for (meta, seg) in metas.iter().zip(full.segments()) {
            let lines = load_segment(Path::new(&path), meta).expect("slice loads");
            let lazy = meter_from_lines(lines.iter().map(String::as_str));
            assert_eq!(lazy.segments().len(), 1, "{name}: one segment per slice");
            let ls = &lazy.segments()[0];
            assert_eq!(
                taken_picture(ls),
                taken_picture(seg),
                "{name} / {}",
                meta.name
            );
            checked += 1;
        }
        // R10: a lazily loaded Overall folds exactly like the full one.
        for meta in &idx.overalls {
            let ordinal = meta.visit.expect("an Overall meta names its visit");
            let lines = load_segment(Path::new(&path), meta).expect("visit loads");
            let lazy = meter_from_lines(lines.iter().map(String::as_str));
            let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
            let want = full.overall(ordinal).expect("full replay has the visit");
            assert_eq!(
                taken_picture(&got),
                taken_picture(&want),
                "{name} / {}",
                meta.name
            );
        }
    }
    assert!(checked > 0);
}

/// Resuming a scan from any cut's checkpoint reproduces the full scan on
/// every fixture — misses and Stagger lines in the tail included, since the
/// scanner must keep ignoring them.
#[test]
fn a_resumed_scan_matches_a_full_scan_on_every_fixture() {
    for (name, text) in fixtures() {
        let bytes = text.as_bytes();
        let full = scan(&mut &bytes[..]);
        let cuts: Vec<usize> = bytes
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .map(|(i, _)| i + 1)
            .chain([bytes.len() / 2, bytes.len()])
            .collect();
        for cut in cuts {
            let prefix = scan(&mut &bytes[..cut]);
            let state = prefix.checkpoint.clone();
            let off = state.offset as usize;
            let resumed = scan_from(&mut &bytes[off..], state);
            assert_eq!(resumed.segments, full.segments, "{name}: cut at {cut}");
            assert_eq!(resumed.overalls, full.overalls, "{name}: cut at {cut}");
            assert_eq!(resumed.open, full.open, "{name}: cut at {cut}");
            assert_eq!(resumed.checkpoint, full.checkpoint, "{name}: cut at {cut}");
        }
    }
}

/// `*_MISSED` lines are invisible to segmentation: renaming every one to an
/// unknown event of the same byte length changes nothing about the scan —
/// no boundary, no duration, no byte range — and nothing about the meter's
/// segment table either. Only the Taken bookkeeping goes away.
#[test]
fn missed_lines_never_move_a_segment_boundary() {
    let mut rewritten_any = false;
    for (name, text) in fixtures() {
        let blind = text.replace("_MISSED,", "_XISSED,");
        if blind != text {
            rewritten_any = true;
        }
        let real = scan(&mut text.as_bytes());
        let scan_blind = scan(&mut blind.as_bytes());
        assert_eq!(real, scan_blind, "{name}: the scanner must not see misses");

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
        // The Damage side is untouched by misses; only Taken counts drop.
        for (sa, sb) in a.segments().iter().zip(b.segments()) {
            assert_eq!(flat(&sa.rows(View::Damage)), flat(&sb.rows(View::Damage)));
            let hits = |s: &Segment| s.rows(View::Taken).iter().map(|r| r.amount).sum::<u64>();
            assert_eq!(hits(sa), hits(sb), "{name}: a miss adds no amount");
            for r in sb.rows(View::Taken) {
                assert_eq!(sb.mitigation(&r.key).map(|m| m.misses()), Some(0));
            }
        }
    }
    assert!(
        rewritten_any,
        "no fixture carries a *_MISSED line — taken.txt must"
    );
}

/// The R17-only fixture: every `MissKind` against a friendly target exactly
/// once, a pet hit before its summon folding onto its owner, Stagger taken
/// once. Loose shape assertions — the hand-computed numbers live in
/// `taken.expected.tsv` and are gated by `fixture_totals`.
#[test]
fn the_taken_fixture_exercises_every_ruling_branch() {
    let text = std::fs::read_to_string(fixture_path("taken.txt"));
    assert!(
        text.is_ok(),
        "fixtures/taken.txt must exist: {:?}",
        text.as_ref().err()
    );
    let text = text.unwrap_or_default();
    let meter = replay(&text);
    let lines = parsed(&text);
    let kinds_in_log: HashSet<wowdps_model::MissKind> = lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Missed { dst, kind, .. }
                if dst.guid.starts_with("Player-") || dst.guid.starts_with("Pet-") =>
            {
                Some(*kind)
            }
            _ => None,
        })
        .collect();
    assert!(
        !kinds_in_log.is_empty(),
        "the fixture carries misses on friendlies"
    );
    let mut merged = wowdps_model::Mitigation::default();
    let mut taken_rows = 0;
    for seg in meter.segments() {
        for r in seg.rows(View::Taken) {
            taken_rows += 1;
            if let Some(m) = seg.mitigation(&r.key) {
                merged.merge(&m);
            }
        }
    }
    assert!(taken_rows > 0);
    for kind in kinds_in_log {
        assert!(merged.misses_of(kind) > 0, "{kind:?} reached a Taken row");
    }
    let stagger_lines = lines
        .iter()
        .filter(|l| matches!(&l.event, Event::Absorbed { absorb_spell, .. } if absorb_spell.id == 115069))
        .count();
    if stagger_lines > 0 {
        assert!(merged.stagger > 0, "Stagger shield lines feed `stagger`");
        assert!(
            merged.stagger_ticked > 0,
            "and the ticks feed `stagger_ticked`"
        );
        assert!(
            merged.stagger <= merged.absorbed,
            "stagger is a subset of absorbed"
        );
    }
}
