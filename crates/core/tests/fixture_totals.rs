//! End-to-end verification: the real parser + meter, run over the canonical fixture,
//! asserted against expected values computed independently of this code.
//!
//! `fixtures/sample.expected.tsv` is produced by `fixtures/check.awk`, the validator's
//! own reading of the log grammar. Nothing in this test consults the implementation to
//! decide what the right answer is — if these two disagree, one of them is wrong and
//! the disagreement is the finding.
//!
use std::collections::BTreeMap;
use wowdps_core::meter::{Meter, SegmentKind, View};
use wowdps_core::parser::parse_line;

/// (segment index 0-based, player guid, metric) -> value
type Totals = BTreeMap<(usize, String, String), f64>;

/// One segment as both sides describe it: kind, name, duration, and the
/// ENCOUNTER_START identity as "id/difficulty" ("" off encounters).
type Seg = (String, String, i64, String);

const VIEWS: &[(View, &str, &str)] = &[
    (View::Damage, "damage", "overkill"),
    (View::Healing, "heal", "overheal"),
    (View::Interrupts, "interrupts", ""),
    (View::CrowdControl, "cc", ""),
    (View::Dispels, "dispels", ""),
    (View::Deaths, "deaths", ""),
    // R17: the destination side. `absorbed` is the Taken row's `extra`; the
    // rest of the split (`blocked`, `prevented`, `misses`, `stagger`,
    // `stagger_ticked`) is read off `Segment::mitigation` below.
    (View::Taken, "taken", "absorbed"),
];

/// Feed a log through the real parser + meter and flatten it into the same shape as
/// the expected TSV.
/// Read a fixture, failing the test loudly (but panic-macro-free) if it is
/// missing — the panic bans in `Cargo.toml` reach helpers outside `#[test]` fns.
fn read_fixture(path: &str) -> String {
    let text = std::fs::read_to_string(path);
    assert!(text.is_ok(), "{path}: unreadable fixture");
    text.unwrap_or_default()
}

fn actual_totals(path: &str) -> (Totals, Vec<Seg>) {
    let text = read_fixture(path);
    let mut meter = Meter::new();
    let mut last_ms = 0i64;
    for line in text.lines() {
        if let Some(parsed) = parse_line(line) {
            last_ms = last_ms.max(parsed.ts_ms);
            meter.feed(parsed);
        }
    }

    let mut out: Totals = BTreeMap::new();
    let mut segs = Vec::new();
    for (i, seg) in meter.segments().iter().enumerate() {
        // The meter's segment stream never contains Overall (R10):
        // overalls are synthesized by merging, not recorded.
        assert!(
            !matches!(seg.kind, SegmentKind::Overall),
            "Overall never appears in the segment stream"
        );
        let kind = match seg.kind {
            SegmentKind::Encounter => "Encounter",
            SegmentKind::Trash | SegmentKind::Overall => "Trash",
        };
        let result = match seg.success {
            Some(true) => "kill",
            Some(false) => "wipe",
            None => "",
        };
        // The golden TSV predates display naming: trash rows carry the
        // literal "Trash". The Details-style pull names are presentation,
        // covered by the meter unit tests and the index parity test.
        let name = match seg.kind {
            SegmentKind::Trash => "Trash".to_string(),
            SegmentKind::Encounter | SegmentKind::Overall => seg.name.clone(),
        };
        let enc = seg
            .encounter
            .map(|e| format!("{}/{}", e.id, e.difficulty))
            .unwrap_or_default();
        segs.push((kind.to_string(), name, seg.duration_ms(last_ms), enc));

        let mut players = std::collections::BTreeSet::new();
        for (view, amount_metric, extra_metric) in VIEWS {
            let rows = seg.rows(*view);
            for r in &rows {
                players.insert(r.key.clone());
                out.insert(
                    (i, r.key.clone(), amount_metric.to_string()),
                    r.amount as f64,
                );
                if !extra_metric.is_empty() {
                    out.insert((i, r.key.clone(), extra_metric.to_string()), r.extra as f64);
                }
                if *amount_metric == "damage" {
                    out.insert((i, r.key.clone(), "dps".into()), r.per_sec);
                    out.insert((i, r.key.clone(), "pct".into()), r.pct);
                }
            }
        }
        // R17: the mitigation split for every player the segment lists in
        // ANY view — a Stagger shield line with no damage twin in the segment
        // leaves a record (and a golden `stagger`) with no Taken row behind it.
        for key in players {
            let Some(m) = seg.mitigation(&key) else {
                continue;
            };
            let mut put = |metric: &str, v: u64| {
                out.insert((i, key.clone(), metric.to_string()), v as f64);
            };
            put("blocked", m.blocked);
            put("prevented", m.absorbed_full + m.blocked_full);
            put("misses", u64::from(m.misses()));
            put("stagger", m.stagger);
            put("stagger_ticked", m.stagger_ticked);
        }
        let _ = result;
    }
    (out, segs)
}

/// Parse the golden TSV. Columns: segment kind name result dur_ms enc_id
/// difficulty player metric value
fn expected_totals(path: &str) -> (Totals, Vec<Seg>) {
    let text = read_fixture(path);
    let mut out: Totals = BTreeMap::new();
    let mut segs: BTreeMap<usize, Seg> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 10 {
            continue;
        }
        let col = |i: usize| f.get(i).copied().unwrap_or_default();
        // TSV is 1-based; a malformed row would fail the comparison anyway.
        let Some(seg) = col(0).parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else {
            continue;
        };
        segs.insert(
            seg,
            (
                col(1).into(),
                col(2).into(),
                col(4).parse().unwrap_or_default(),
                match (col(5), col(6)) {
                    ("", _) => String::new(),
                    (id, diff) => format!("{id}/{diff}"),
                },
            ),
        );
        out.insert(
            (seg, col(7).to_string(), col(8).to_string()),
            col(9).parse().unwrap_or_default(),
        );
    }
    let segs = segs.into_values().collect();
    (out, segs)
}

/// Metrics the meter API does not expose as separate rows. `petdamage` and
/// `absorbheal` are the validator's internal cross-check columns: pet damage is
/// already inside the owner's `damage`, and absorb-as-healing is already inside
/// `heal`. Both are therefore validated implicitly by the totals we do compare.
fn is_comparable(metric: &str) -> bool {
    !matches!(metric, "petdamage" | "absorbheal")
}

/// Returns (gated mismatches, advisory notes).
///
/// Everything the contract pins is now gated, including R7 trash duration
/// (first..last combat event) and CC spell 117526. `notes` is retained for future
/// adopted-but-unlanded rules and is expected to be empty.
fn diff(log: &str, golden: &str) -> (Vec<String>, Vec<String>) {
    let (actual, actual_segs) = actual_totals(log);
    let (expected, expected_segs) = expected_totals(golden);
    let mut problems = Vec::new();
    let notes: Vec<String> = Vec::new();

    if actual_segs.len() != expected_segs.len() {
        problems.push(format!(
            "segment count: expected {} {:?}, got {} {:?}",
            expected_segs.len(),
            expected_segs.iter().map(|s| &s.1).collect::<Vec<_>>(),
            actual_segs.len(),
            actual_segs.iter().map(|s| &s.1).collect::<Vec<_>>(),
        ));
    }
    for (i, (exp, act)) in expected_segs.iter().zip(actual_segs.iter()).enumerate() {
        if exp.0 != act.0 || exp.1 != act.1 {
            problems.push(format!(
                "segment {i}: expected {} \"{}\", got {} \"{}\"",
                exp.0, exp.1, act.0, act.1
            ));
        }
        if exp.2 != act.2 {
            let msg = format!(
                "segment {i} \"{}\": duration expected {} ms, got {} ms",
                exp.1, exp.2, act.2
            );
            problems.push(msg);
        }
        if exp.3 != act.3 {
            problems.push(format!(
                "segment {i} \"{}\": encounter id/difficulty expected \"{}\", got \"{}\"",
                exp.1, exp.3, act.3
            ));
        }
    }

    for ((seg, player, metric), want) in &expected {
        if !is_comparable(metric) {
            continue;
        }
        let got = actual
            .get(&(*seg, player.clone(), metric.clone()))
            .copied()
            .unwrap_or(0.0);
        let ok = if matches!(metric.as_str(), "dps" | "pct") {
            (got - want).abs() < 0.01
        } else {
            (got - want).abs() < f64::EPSILON
        };
        if ok {
            continue;
        }
        let msg = format!(
            "segment {} {} {}: expected {}, got {}",
            seg + 1,
            short(player),
            metric,
            want,
            got
        );
        problems.push(msg);
    }

    // Nothing may appear in the meter that the fixture does not account for.
    for ((seg, player, metric), got) in &actual {
        if metric != "damage" || *got == 0.0 {
            continue;
        }
        if !expected.contains_key(&(*seg, player.clone(), metric.clone())) {
            problems.push(format!(
                "segment {} {}: unexpected meter row with {} damage",
                seg + 1,
                short(player),
                got
            ));
        }
    }
    (problems, notes)
}

fn short(guid: &str) -> String {
    guid.rsplit('-').next().unwrap_or(guid).to_string()
}

/// POSITIVE CONTROL — the real code must reproduce the independently-computed totals.
#[test]
fn fixture_totals_match_expected() {
    let (problems, notes) = diff("fixtures/sample.txt", "fixtures/sample.expected.tsv");
    for n in &notes {
        println!("ADVISORY (not gated): {n}");
    }
    assert!(
        problems.is_empty(),
        "meter disagrees with independently-computed expected values:\n  {}",
        problems.join("\n  ")
    );
}

/// R17 — the taken/mitigation fixture against its hand-computed goldens.
/// Skips (loudly) until `taken.expected.tsv` lands beside `taken.txt`.
#[test]
fn taken_fixture_totals_match_expected() {
    if !std::path::Path::new("fixtures/taken.expected.tsv").exists() {
        println!("SKIPPED: fixtures/taken.expected.tsv is not there yet");
        return;
    }
    let (problems, notes) = diff("fixtures/taken.txt", "fixtures/taken.expected.tsv");
    for n in &notes {
        println!("ADVISORY (not gated): {n}");
    }
    assert!(
        problems.is_empty(),
        "meter disagrees with independently-computed expected values:\n  {}",
        problems.join("\n  ")
    );
}

/// NEGATIVE CONTROL — proves this test can actually fail. `corrupt.txt` is
/// `sample.txt` with three silently altered amounts; checked against sample's
/// expected values it MUST produce mismatches. A suite that cannot fail proves
/// nothing.
#[test]
fn corrupt_fixture_is_detected() {
    let (problems, _notes) = diff("fixtures/corrupt.txt", "fixtures/sample.expected.tsv");
    assert!(
        !problems.is_empty(),
        "corrupt.txt was accepted as matching sample.txt's expected values — the \
         comparison is not actually checking anything"
    );
}

/// Diagnostic: print exactly what the negative control detects, so the failure mode
/// is on the record rather than merely asserted.
#[test]
fn corrupt_fixture_detection_is_specific() {
    let (problems, _) = diff("fixtures/corrupt.txt", "fixtures/sample.expected.tsv");
    for p in &problems {
        println!("NEGATIVE CONTROL caught: {p}");
    }
    assert!(
        problems.len() >= 3,
        "expected >=3 gated mismatches, got {}",
        problems.len()
    );
}

/// R6 — a mid-log COMBAT_LOG_VERSION is a hard boundary: it closes the open segment
/// and resets the pet-owner map. `relog.txt` has two such boundaries. The pet's 7000
/// Bite sits in the middle epoch, which contains no SPELL_SUMMON and no swing carrying
/// an ownerGUID, so after the reset it is unattributable and must get NO meter row.
/// Its owner is re-established in the third epoch by a swing's advanced block.
#[test]
fn relog_boundary_resets_pet_ownership() {
    let (problems, notes) = diff("fixtures/relog.txt", "fixtures/relog.expected.tsv");
    for n in &notes {
        println!("ADVISORY (not gated): {n}");
    }
    assert!(
        problems.is_empty(),
        "R6 mid-log COMBAT_LOG_VERSION handling disagrees with expected values:\n  {}",
        problems.join("\n  ")
    );
}
