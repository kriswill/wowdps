//! R19 (support attribution) and the R2 amendment (the healing split and
//! healing received), over every committed fixture — the partition
//! identities through the public surface, the healing identity, lazy-load
//! and checkpoint-resume parity for every new number, the scanner's
//! indifference to `*_SUPPORT` lines, and the two fixtures that carry
//! support lines (`sample.txt`'s one pair, `support.txt`'s full set). Every
//! fixture in `FIXTURES` must exist — a missing one fails, never skips.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, Row, Segment, View, meter_from_lines};
use wowdps_core::parser::{Event, LogLine, parse_line};
use wowdps_model::{Healed, Support};

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
    "support.txt",
    "spans.txt",
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

fn friendly(guid: &str) -> bool {
    guid.starts_with("Player-") || guid.starts_with("Pet-")
}

/// What the parsed lines say, independently of the meter: every guid that
/// could key a fold (sources, victims, absorbers, supporters, pet owners),
/// the friendly names, and the raw tallies the identities need.
#[derive(Default)]
struct Census {
    /// Every guid seen anywhere — the fold keys. Summing a folding accessor
    /// over this set counts every raw entry exactly once: a raw entry is
    /// either its own owner (in here as a unit) or resolves to a SUMMON /
    /// advanced-block owner (in here too).
    guids: HashSet<String>,
    /// Names of friendly (Player-/Pet-) units that were healed or shielded.
    friendly_names: HashSet<String>,
    /// Guids a `*_SUPPORT` line trails with.
    supporters: HashSet<String>,
    /// Guids a `*_SUPPORT` line names as the buffed source.
    support_srcs: HashSet<String>,
    /// Σ effective heal amount on friendly victims, from ANY source, the
    /// R2 exclusions applied.
    healed_any_source: u64,
    /// The subset from non-friendly (NPC / nil) sources.
    healed_from_npcs: u64,
    /// Σ support share, damage and healing.
    shares: (u64, u64),
}

const NON_HEALING_ABSORBS: [u32; 4] = [114556, 31850, 31230, 115069];

fn census(lines: &[LogLine]) -> Census {
    let mut c = Census::default();
    for l in lines {
        if let Some(h) = &l.owner_hint {
            c.guids.insert(h.owner_guid.clone());
            c.guids.insert(h.unit_guid.clone());
        }
        match &l.event {
            Event::Damage { src, dst, .. } => {
                c.guids.insert(src.guid.clone());
                c.guids.insert(dst.guid.clone());
            }
            Event::Heal {
                src,
                dst,
                spell,
                amount,
                overheal,
                ..
            } => {
                c.guids.insert(src.guid.clone());
                c.guids.insert(dst.guid.clone());
                if NON_HEALING_ABSORBS.contains(&spell.id) {
                    continue;
                }
                if friendly(&dst.guid) {
                    c.friendly_names.insert(dst.name.clone());
                    let eff = amount.saturating_sub(*overheal);
                    c.healed_any_source += eff;
                    if !friendly(&src.guid) {
                        c.healed_from_npcs += eff;
                    }
                }
            }
            Event::Absorbed {
                dst,
                absorber,
                absorb_spell,
                ..
            } => {
                c.guids.insert(absorber.guid.clone());
                c.guids.insert(dst.guid.clone());
                if NON_HEALING_ABSORBS.contains(&absorb_spell.id) {
                    continue;
                }
                if friendly(&dst.guid) {
                    c.friendly_names.insert(dst.name.clone());
                }
            }
            Event::Support {
                src,
                dst,
                supporter,
                amount,
                healing,
                ..
            } => {
                c.guids.insert(src.guid.clone());
                c.guids.insert(dst.guid.clone());
                c.guids.insert(supporter.clone());
                c.supporters.insert(supporter.clone());
                c.support_srcs.insert(src.guid.clone());
                if *healing {
                    c.shares.1 += amount;
                } else {
                    c.shares.0 += amount;
                }
            }
            Event::Summon { owner, pet, .. } => {
                c.guids.insert(owner.guid.clone());
                c.guids.insert(pet.guid.clone());
            }
            _ => {}
        }
    }
    c
}

/// The keys `effective` partitions over: everyone with a Damage row plus
/// everyone a support line names on either side (a supporter may have no
/// row at all; a buffed pet folds to an owner that may have none either).
fn effective_keys(seg: &Segment, c: &Census) -> HashSet<String> {
    let mut keys: HashSet<String> = seg.rows(View::Damage).into_iter().map(|r| r.key).collect();
    keys.extend(c.supporters.iter().cloned());
    keys.extend(c.support_srcs.iter().cloned());
    // A buffed pet's owner, learned from a summon or an advanced block.
    keys.extend(c.guids.iter().filter(|g| g.starts_with("Player-")).cloned());
    keys
}

/// Σ SPELL_ABSORBED amount on NON-friendly victims (an NPC's own shield)
/// logged inside the segment's clock: credited healing the friendly-name
/// filter leaves out. Segments never overlap in time, and the absorb line
/// that opens a Trash segment is its `start_ms`, so the window is exact.
fn hostile_absorbs(lines: &[LogLine], seg: &Segment) -> u64 {
    let end = seg.end_ms.unwrap_or(seg.last_combat_ms());
    lines
        .iter()
        .filter(|l| l.ts_ms >= seg.start_ms && l.ts_ms <= end)
        .filter_map(|l| match &l.event {
            Event::Absorbed {
                dst,
                absorb_spell,
                amount,
                ..
            } if !friendly(&dst.guid) && !NON_HEALING_ABSORBS.contains(&absorb_spell.id) => {
                Some(*amount)
            }
            _ => None,
        })
        .sum()
}

/// The three identities of the slice, on one segment; `absorbs_on_hostiles`
/// is `hostile_absorbs` over the segment (summed over the members for an
/// Overall). Returns the segment's Σ Damage so callers can also check that
/// something was summed.
fn check_identities(name: &str, seg: &Segment, c: &Census, absorbs_on_hostiles: u64) -> u64 {
    let damage: u64 = seg.rows(View::Damage).iter().map(|r| r.amount).sum();
    let keys = effective_keys(seg, c);

    // R19: Σ effective = Σ damage — a true partition.
    let effective: u64 = keys.iter().map(|k| seg.effective(k)).sum();
    assert_eq!(
        effective, damage,
        "{name} / {}: Σ effective vs Σ damage",
        seg.name
    );

    // R19: Σ given = Σ received, damage and healing apart, over the fold.
    let mut given = (0u64, 0u64);
    let mut received = (0u64, 0u64);
    for k in &c.guids {
        let Some(s) = seg.support(k) else {
            continue;
        };
        given.0 += s.given_damage;
        given.1 += s.given_healing;
        received.0 += s.received_damage;
        received.1 += s.received_healing;
    }
    assert_eq!(
        given, received,
        "{name} / {}: Σ given vs Σ received",
        seg.name
    );

    // R2 amendment, THE HEALING IDENTITY, exact form: over the fold keys G
    // (every guid the slice mentions) and the friendly names F,
    //   Σ_{g∈G} Σ_{r ∈ breakdown(g, Healing).by_target, r.label ∈ F} r.amount
    //     = Σ_{g∈G} healed(g).received + Σ_{g∈G} absorbed_healing(g)
    //       − Σ SPELL_ABSORBED amount on non-friendly victims (parsed lines).
    // The left side is what the Healing rows say landed on friendlies —
    // heals AND absorb credit, from every actor (NPC healers have a drill
    // even without a row). The right side is the two new counters; the one
    // parsed-line term removes the credits whose victim is not a friendly
    // (an NPC's own shield), which the name filter also leaves out.
    let by_target: u64 = c
        .guids
        .iter()
        .flat_map(|g| seg.breakdown(g, View::Healing).1)
        .filter(|r| c.friendly_names.contains(&r.label))
        .map(|r| r.amount)
        .sum();
    let healed: u64 = c
        .guids
        .iter()
        .filter_map(|g| seg.healed(g))
        .map(|h| h.received)
        .sum();
    let credited: u64 = c.guids.iter().map(|g| seg.absorbed_healing(g)).sum();
    assert_eq!(
        by_target,
        healed + credited - absorbs_on_hostiles,
        "{name} / {}: Healing by_target(friendly) vs healed received + absorb credit",
        seg.name
    );
    // And the split never exceeds the row it is a half of.
    for r in seg.rows(View::Healing) {
        assert!(
            seg.absorbed_healing(&r.key) <= r.amount,
            "{name} / {}: {} absorbed {} > healing {}",
            seg.name,
            r.label,
            seg.absorbed_healing(&r.key),
            r.amount
        );
    }
    damage
}

/// The R19 / R2-amendment partitions, on every segment and every Overall
/// of every fixture, through nothing but the public surface.
#[test]
fn effective_and_healing_identities_hold_on_every_segment() {
    let mut checked = 0;
    let mut with_support = 0;
    for (name, text) in fixtures() {
        let lines = parsed(&text);
        let c = census(&lines);
        let meter = replay(&text);
        for seg in meter.segments() {
            check_identities(name, seg, &c, hostile_absorbs(&lines, seg));
            checked += 1;
        }
        for (ordinal, _) in meter.visits().iter().enumerate() {
            let ordinal = ordinal as u32;
            if let Some(ov) = meter.overall(ordinal) {
                let members: u64 = meter
                    .segments()
                    .iter()
                    .filter(|s| s.visit == Some(ordinal))
                    .map(|s| hostile_absorbs(&lines, s))
                    .sum();
                check_identities(name, &ov, &c, members);
            }
        }
        // Whole-log: every heal on a friendly reached exactly one
        // segment's record, NPC heals included.
        let healed_all: u64 = meter
            .segments()
            .iter()
            .flat_map(|s| c.guids.iter().filter_map(|g| s.healed(g)))
            .map(|h| h.received)
            .sum();
        assert_eq!(
            healed_all, c.healed_any_source,
            "{name}: Σ healed received over the log vs the parsed heals"
        );
        if c.shares != (0, 0) {
            with_support += 1;
            // And every share the log carries inside a pull reached the
            // ledger (no fixture logs a share outside one).
            let given_all: (u64, u64) = meter
                .segments()
                .iter()
                .flat_map(|s| c.guids.iter().filter_map(|g| s.support(g)))
                .fold((0, 0), |acc, s| {
                    (acc.0 + s.given_damage, acc.1 + s.given_healing)
                });
            assert_eq!(given_all, c.shares, "{name}: Σ given over the log");
        }
    }
    assert!(checked > 0);
    assert!(
        with_support >= 2,
        "sample.txt and support.txt carry support lines"
    );
}

/// One row reduced to what R19 pins: key, label, amount, extra, count.
type Flat = (String, String, u64, u64, u64);

fn flat(rows: &[Row]) -> Vec<Flat> {
    rows.iter()
        .map(|r| (r.key.clone(), r.label.clone(), r.amount, r.extra, r.count))
        .collect()
}

/// Everything R19 and the R2 amendment say about one player.
type Picture = (
    String,
    Option<Support>,
    Vec<Flat>,
    Option<Healed>,
    u64,
    u64,
    Vec<(f64, f64)>,
);

/// The new numbers for every fold key, in one comparable value — sorted by
/// key so two replays compare regardless of map order.
fn support_picture(seg: &Segment, c: &Census) -> Vec<Picture> {
    let mut keys: Vec<&String> = c.guids.iter().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let targets = seg.support_targets(k);
            (
                k.clone(),
                seg.support(k),
                flat(&targets),
                seg.healed(k),
                seg.absorbed_healing(k),
                seg.effective(k),
                targets.iter().map(|r| (r.per_sec, r.pct)).collect(),
            )
        })
        .filter(|p| p.1.is_some() || p.3.is_some() || p.4 > 0 || p.5 > 0)
        .collect()
}

/// Lazy load == full replay for every new number, on every segment and
/// every Overall of every fixture; and a scan resumed from any checkpoint
/// hands out slices that load to the same numbers.
#[test]
fn support_survives_lazy_loading_and_checkpoints_on_every_fixture() {
    let mut checked = 0;
    for (name, text) in fixtures() {
        let path = fixture_path(name);
        let bytes = text.as_bytes();
        let c = census(&parsed(&text));
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
                support_picture(ls, &c),
                support_picture(seg, &c),
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
                support_picture(&got, &c),
                support_picture(&want, &c),
                "{name} / {}",
                meta.name
            );
        }
        // Checkpoint resume: the metas a resumed scan produces load to the
        // same numbers as the full scan's. Cut at every line boundary.
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
                    support_picture(&lazy.segments()[0], &c),
                    support_picture(seg, &c),
                    "{name} / {}: resumed at {cut}",
                    meta.name
                );
            }
        }
    }
    assert!(checked > 0);
}

/// `*_SUPPORT` lines are invisible to segmentation: renaming every one to
/// an unknown event of the same byte length changes nothing about the scan
/// — no boundary, no duration, no byte range — and nothing about the
/// meter's segment table, Damage or Healing rows. Only the R19 ledger
/// goes away.
#[test]
fn support_lines_never_move_a_segment_boundary() {
    let mut rewritten_any = false;
    for (name, text) in fixtures() {
        let blind = text.replace("_SUPPORT,", "_SUPPORX,");
        if blind != text {
            rewritten_any = true;
        }
        assert_eq!(blind.len(), text.len(), "{name}: same-length rewrite");
        let real = scan(&mut text.as_bytes());
        let scan_blind = scan(&mut blind.as_bytes());
        assert_eq!(real, scan_blind, "{name}: the scanner must not see support");

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
        let c = census(&parsed(&text));
        let rows = |s: &Segment, v: View| {
            s.rows(v)
                .iter()
                .map(|r| (r.key.clone(), r.amount, r.extra, r.count))
                .collect::<Vec<_>>()
        };
        for (sa, sb) in a.segments().iter().zip(b.segments()) {
            for v in [View::Damage, View::Healing, View::Taken] {
                assert_eq!(rows(sa, v), rows(sb, v), "{name}: {v:?} rows");
            }
            for k in &c.guids {
                assert_eq!(sb.support(k), None, "{name}: no ledger without the lines");
                assert!(sb.support_targets(k).is_empty());
                // The R2 amendment does not depend on support lines at all.
                assert_eq!(sa.healed(k), sb.healed(k), "{name}: healed");
                assert_eq!(
                    sa.absorbed_healing(k),
                    sb.absorbed_healing(k),
                    "{name}: absorbed"
                );
            }
            // With no shares, effective is exactly the Damage row.
            let rows = sa.rows(View::Damage);
            for k in effective_keys(sa, &c) {
                let row = rows.iter().find(|r| r.key == k).map_or(0, |r| r.amount);
                assert_eq!(
                    sb.effective(&k),
                    row,
                    "{name}: {k} effective without shares"
                );
            }
        }
    }
    assert!(
        rewritten_any,
        "no fixture carries a *_SUPPORT line — sample.txt and support.txt must"
    );
}

/// `sample.txt`'s one RANGE_DAMAGE_SUPPORT line: the Marksmanship hunter's
/// Aimed Shot, 29 400 attributed to a supporter the log names nowhere else
/// — a guid with no rows, which `support` still answers for.
#[test]
fn the_sample_fixture_support_pair_is_attributed_both_ways() {
    const HUNTER: &str = "Player-1168-0A1B2C03";
    const SUPPORTER: &str = "Player-1168-0A1B2C04";
    let text = std::fs::read_to_string(fixture_path("sample.txt"));
    assert!(text.is_ok(), "fixtures/sample.txt must exist");
    let meter = replay(&text.unwrap_or_default());
    let seg = meter
        .segments()
        .iter()
        .find(|s| s.support(SUPPORTER).is_some())
        .expect("the segment with the support line");
    assert_eq!(
        seg.support(HUNTER),
        Some(Support {
            received_damage: 29_400,
            ..Support::default()
        })
    );
    assert_eq!(
        seg.support(SUPPORTER),
        Some(Support {
            given_damage: 29_400,
            ..Support::default()
        })
    );
    assert!(
        seg.rows(View::Damage).iter().all(|r| r.key != SUPPORTER),
        "the supporter never dealt damage"
    );
    assert_eq!(seg.effective(SUPPORTER), 29_400, "given, nothing else");
    let hunter_damage = seg
        .rows(View::Damage)
        .iter()
        .find(|r| r.key == HUNTER)
        .map_or(0, |r| r.amount);
    assert_eq!(seg.effective(HUNTER), hunter_damage - 29_400);
    let targets = seg.support_targets(SUPPORTER);
    assert_eq!(
        flat(&targets),
        vec![(
            HUNTER.to_string(),
            "Kael'thar-Nebula-US".to_string(),
            29_400,
            0,
            1
        )]
    );
    assert!(seg.support_targets(HUNTER).is_empty());
    assert!(targets[0].per_sec > 0.0 && (targets[0].pct - 100.0).abs() < 1e-9);
    // Exactly one segment of the fixture carries it.
    assert_eq!(
        meter
            .segments()
            .iter()
            .filter(|s| s.support(SUPPORTER).is_some())
            .count(),
        1
    );
}

/// R10: an Overall's ledgers are the sums of its members'.
#[test]
fn overall_sums_members_support_and_healing() {
    let mut visits = 0;
    for (name, text) in fixtures() {
        let c = census(&parsed(&text));
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
            for k in &c.guids {
                let mut sup: Option<Support> = None;
                let mut healed: Option<Healed> = None;
                let mut credited = 0;
                let mut targets: HashMap<String, (u64, u64, u64)> = HashMap::new();
                for m in &members {
                    if let Some(s) = m.support(k) {
                        sup.get_or_insert_with(Support::default).merge(&s);
                    }
                    if let Some(h) = m.healed(k) {
                        healed.get_or_insert_with(Healed::default).merge(&h);
                    }
                    credited += m.absorbed_healing(k);
                    for r in m.support_targets(k) {
                        let t = targets.entry(r.label).or_default();
                        t.0 += r.amount;
                        t.1 += r.extra;
                        t.2 += r.count;
                    }
                }
                assert_eq!(ov.support(k), sup, "{name}: visit {ordinal} {k} support");
                assert_eq!(ov.healed(k), healed, "{name}: visit {ordinal} {k} healed");
                assert_eq!(ov.absorbed_healing(k), credited, "{name}: {k} absorbed");
                let got: HashMap<String, (u64, u64, u64)> = ov
                    .support_targets(k)
                    .into_iter()
                    .map(|r| (r.label, (r.amount, r.extra, r.count)))
                    .collect();
                assert_eq!(got, targets, "{name}: visit {ordinal} {k} targets");
            }
        }
    }
    assert!(visits > 0);
}

/// The R19-only fixture: every ruling branch reaches the ledger, checked
/// against a test-side tally of the parsed lines that folds pets through
/// the log's own SPELL_SUMMON lines. Loose relational assertions — the
/// hand-computed numbers live in `support.expected.tsv`, gated by
/// `fixture_totals`.
#[test]
fn the_support_fixture_exercises_every_ruling_branch() {
    let text = std::fs::read_to_string(fixture_path("support.txt"));
    assert!(
        text.is_ok(),
        "fixtures/support.txt must exist: {:?}",
        text.as_ref().err()
    );
    let text = text.unwrap_or_default();
    let lines = parsed(&text);
    let meter = replay(&text);
    let seg = meter
        .segments()
        .iter()
        .find(|s| s.encounter.is_some())
        .expect("the fixture's one kill");

    // Test-side: owners from the summons, then per-owner received and
    // per-supporter given, damage and healing apart.
    let owners: HashMap<String, String> = lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Summon { owner, pet, .. } => Some((pet.guid.clone(), owner.guid.clone())),
            _ => None,
        })
        .collect();
    let owner_of = |g: &str| owners.get(g).cloned().unwrap_or_else(|| g.to_string());
    let mut received: HashMap<String, (u64, u64)> = HashMap::new();
    let mut given: HashMap<String, (u64, u64)> = HashMap::new();
    let mut pet_share = 0;
    let mut self_share = 0;
    let mut families: HashSet<String> = HashSet::new();
    for (l, raw) in lines
        .iter()
        .zip(text.lines().filter(|l| parse_line(l).is_some()))
    {
        let Event::Support {
            src,
            supporter,
            amount,
            healing,
            ..
        } = &l.event
        else {
            continue;
        };
        if l.ts_ms < seg.start_ms || seg.end_ms.is_some_and(|e| l.ts_ms > e) {
            continue;
        }
        families.insert(
            raw.split("  ")
                .nth(1)
                .and_then(|r| r.split(',').next())
                .unwrap_or_default()
                .to_string(),
        );
        let r = received.entry(owner_of(&src.guid)).or_default();
        let g = given.entry(supporter.clone()).or_default();
        if *healing {
            r.1 += amount;
            g.1 += amount;
        } else {
            r.0 += amount;
            g.0 += amount;
        }
        if src.guid.starts_with("Pet-") {
            pet_share += amount;
        }
        if &src.guid == supporter {
            self_share += amount;
        }
    }
    for fam in [
        "SPELL_DAMAGE_SUPPORT",
        "SPELL_PERIODIC_DAMAGE_SUPPORT",
        "SWING_DAMAGE_LANDED_SUPPORT",
        "SPELL_HEAL_SUPPORT",
        "SPELL_PERIODIC_HEAL_SUPPORT",
    ] {
        assert!(
            families.contains(fam),
            "{fam} inside the pull: {families:?}"
        );
    }
    assert!(pet_share > 0, "a buffed pet");
    assert!(self_share > 0, "a self-supported proc");
    assert_eq!(given.len(), 1, "one Augmentation");
    let (evoker, &(gd, gh)) = given.iter().next().expect("the supporter");
    assert!(gd > 0 && gh > 0, "damage and healing shares given");

    // The ledger agrees with the tally, pets folded onto owners.
    for (owner, &(rd, rh)) in &received {
        let s = seg.support(owner).expect("received folds onto the owner");
        assert_eq!((s.received_damage, s.received_healing), (rd, rh), "{owner}");
    }
    let s = seg.support(evoker).expect("the supporter's ledger");
    assert_eq!((s.given_damage, s.given_healing), (gd, gh));
    // The self-supported proc: given AND received by the Evoker, so
    // `effective` cancels it and counts it once — through the R1 row.
    assert!(s.received_damage >= self_share);
    let evoker_row = seg
        .rows(View::Damage)
        .iter()
        .find(|r| &r.key == evoker)
        .map_or(0, |r| r.amount);
    assert_eq!(
        seg.effective(evoker),
        evoker_row - s.received_damage + s.given_damage
    );
    // The pet never answers for itself, and the supporter's targets name
    // the pet's OWNER, never the pet.
    for pet in owners.keys() {
        assert_eq!(seg.support(pet), None, "{pet} folds away");
        assert!(seg.support_targets(pet).is_empty());
    }
    let targets = seg.support_targets(evoker);
    let pet_names: HashSet<&str> = lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Summon { pet, .. } => Some(pet.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        targets
            .iter()
            .all(|r| !pet_names.contains(r.label.as_str()))
    );
    assert_eq!(targets.len(), received.len(), "one row per buffed owner");
    for r in &targets {
        let want = received.get(&r.key).copied().unwrap_or_default();
        assert_eq!((r.amount, r.extra), want, "{}", r.label);
    }
    assert!(
        targets.windows(2).all(|w| w[0].amount >= w[1].amount),
        "sorted by damage share desc"
    );

    // R2 amendment: an NPC-sourced heal counts as received; a self-heal
    // is the self subset; a heal on the pet is the owner's; the shield is
    // the absorber's `absorbed`, the Stagger absorb nobody's.
    let mut npc_heal = (String::new(), 0u64);
    let mut self_heal: HashMap<String, u64> = HashMap::new();
    let mut pet_heal = (String::new(), 0u64);
    let mut credits: HashMap<String, u64> = HashMap::new();
    let mut stagger = 0u64;
    for l in &lines {
        match &l.event {
            Event::Heal {
                src,
                dst,
                amount,
                overheal,
                ..
            } if friendly(&dst.guid) => {
                let eff = amount.saturating_sub(*overheal);
                if !friendly(&src.guid) {
                    npc_heal = (dst.guid.clone(), npc_heal.1 + eff);
                }
                if src.guid == dst.guid {
                    *self_heal.entry(dst.guid.clone()).or_default() += eff;
                }
                if dst.guid.starts_with("Pet-") {
                    pet_heal = (owner_of(&dst.guid), pet_heal.1 + eff);
                }
            }
            Event::Absorbed {
                absorber,
                absorb_spell,
                amount,
                ..
            } => {
                if NON_HEALING_ABSORBS.contains(&absorb_spell.id) {
                    stagger += amount;
                } else {
                    *credits.entry(absorber.guid.clone()).or_default() += amount;
                }
            }
            _ => {}
        }
    }
    assert!(npc_heal.1 > 0, "an NPC heals a player");
    assert!(pet_heal.1 > 0, "the pet is healed");
    assert!(stagger > 0 && !credits.is_empty());
    let victim = seg.healed(&npc_heal.0).expect("healed");
    assert!(victim.received >= npc_heal.1 + victim.self_healed);
    for (who, amt) in &self_heal {
        let h = seg.healed(who).expect("self-healed");
        assert_eq!(h.self_healed, *amt, "{who}");
        assert!(h.received >= *amt);
    }
    let owner = seg
        .healed(&pet_heal.0)
        .expect("the pet's heal is its owner's");
    assert!(owner.received >= pet_heal.1);
    for (absorber, amt) in &credits {
        assert_eq!(seg.absorbed_healing(absorber), *amt, "{absorber}");
        let row = seg
            .rows(View::Healing)
            .iter()
            .find(|r| &r.key == absorber)
            .map_or(0, |r| r.amount);
        assert!(*amt <= row, "absorbed ≤ healing");
    }
    // Absorbs are not received healing: the shielded warrior's received is
    // exactly his heals.
    let warrior_heals: u64 = lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Heal {
                dst,
                amount,
                overheal,
                ..
            } if dst.guid == npc_heal.0 => Some(amount.saturating_sub(*overheal)),
            _ => None,
        })
        .sum();
    assert_eq!(
        seg.healed(&npc_heal.0).map(|h| h.received),
        Some(warrior_heals)
    );

    // The Trash segment after the zone change carries its own share.
    let trash = meter
        .segments()
        .iter()
        .find(|s| s.encounter.is_none())
        .expect("the city pull");
    assert!(
        trash.support(evoker).is_some_and(|s| s.given_damage > 0),
        "a support line inside open trash records"
    );
}
