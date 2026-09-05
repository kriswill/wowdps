//! R19 + the R2 amendment over a real log (`docs/plan-role-pivots-step3.md`
//! §4): on EVERY segment of the log — every support `src` folds to a
//! player (orphans = 0), Σ effective = Σ damage (with the players whose
//! received exceeds their damage — the `model::effective` clamp cases —
//! counted and printed), the healing identity (exactly as
//! `tests/support.rs` pins it over the fixtures), a census by `_SUPPORT`
//! family (the six parse as `Support`, anything else is `Other`), the
//! whole-hit procs (Bombardments, Fate Mirror) paired with their own hit
//! and EQUAL to it, the (src, dst) share clusters never exceeding their
//! hits — at a 10 ms window and again at 1 ms, with identical verdicts, so
//! the window is shown not to be load-bearing — and the wall time of the
//! parse.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_support -- --ignored --nocapture`

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{Segment, View, meter_from_lines};
use wowdps_core::parser::{Event, LogLine, parse_line};

const FAMILIES: [&str; 6] = [
    "SPELL_DAMAGE_SUPPORT",
    "SPELL_PERIODIC_DAMAGE_SUPPORT",
    "RANGE_DAMAGE_SUPPORT",
    "SWING_DAMAGE_LANDED_SUPPORT",
    "SPELL_HEAL_SUPPORT",
    "SPELL_PERIODIC_HEAL_SUPPORT",
];

const NON_HEALING_ABSORBS: [u32; 4] = [114556, 31850, 31230, 115069];

/// The buffs whose share IS the hit: procs the Evoker owns outright, logged
/// twice (a hit with the proc as its spell, and a support line whose buff
/// is the same proc), same amount. Bombardments, Fate Mirror.
const PROC_BUFFS: [u32; 2] = [434481, 413786];

/// How far a share may be stamped from its hit and still pair with it —
/// the working window, and the tight one it is checked against.
const PAIR_TOL_MS: i64 = 10;
const TIGHT_TOL_MS: i64 = 1;

fn friendly(guid: &str) -> bool {
    guid.starts_with("Player-") || guid.starts_with("Pet-")
}

/// Every guid the slice mentions — the fold keys (see `tests/support.rs`).
fn guids_of(lines: &[LogLine]) -> HashSet<String> {
    let mut g = HashSet::new();
    for l in lines {
        if let Some(h) = &l.owner_hint {
            g.insert(h.owner_guid.clone());
            g.insert(h.unit_guid.clone());
        }
        match &l.event {
            Event::Damage { src, dst, .. } | Event::Heal { src, dst, .. } => {
                g.insert(src.guid.clone());
                g.insert(dst.guid.clone());
            }
            Event::Absorbed { dst, absorber, .. } => {
                g.insert(absorber.guid.clone());
                g.insert(dst.guid.clone());
            }
            Event::Support {
                src,
                dst,
                supporter,
                ..
            } => {
                g.insert(src.guid.clone());
                g.insert(dst.guid.clone());
                g.insert(supporter.clone());
            }
            Event::Summon { owner, pet } => {
                g.insert(owner.guid.clone());
                g.insert(pet.guid.clone());
            }
            _ => {}
        }
    }
    g
}

/// One clustering pass's verdicts (see `cluster`).
#[derive(Default, Clone)]
struct ClusterStats {
    groups: usize,
    unpaired: usize,
    over_groups: usize,
    /// Clusters whose only hit is a SWING_DAMAGE_LANDED twin (a guardian
    /// outside the log filter), and the shares they carry.
    landed_only: usize,
    landed_only_shares: u64,
    max_damage_ratio: f64,
    max_heal_ratio: f64,
    /// Swing clusters where the LANDED twin is logged BEFORE the
    /// SWING_DAMAGE line — the order that makes a swing's share precede
    /// the hit R1 counts (and a pre-pull share real).
    landed_precedes_swing: usize,
    /// Clusters that open on a share with no LANDED twin ahead of it — a
    /// SPELL share stamped before its own hit. Only the spell side can
    /// ever count here: a swing's LANDED twin opens its cluster first.
    share_opens_cluster: usize,
    unpaired_samples: Vec<String>,
    over_samples: Vec<String>,
}

/// Group sums per (src, dst) CLUSTER: every same-(src, dst) line — a hit
/// (R1 amount, `amount + absorbed`; for the heal side, a heal's effective
/// amount), a share, or a SWING_DAMAGE_LANDED twin — within `tol_ms` of
/// the cluster's previous line joins it, and per cluster Σ shares ≤ Σ
/// hits. Nothing finer is falsifiable for a SHARE: the game stamps a
/// spell's share up to 1 ms AFTER its hit (`.136` hit, `.137` share), a
/// swing's LANDED twin + share 1 ms BEFORE the SWING_DAMAGE line (`.489`
/// share, `.490` hit), two buffs share one hit, and one (src, dst, ms) can
/// carry two hits (a Frostbolt and the Fate Mirror proc it triggered —
/// both `src` = the buffed player). A cluster with shares and NO hit is
/// unpaired — unless its only hit is a LANDED twin: a guardian outside
/// the log filter (Army of the Dead ghouls) has its swings logged from the
/// target's view only, so R1 counts nothing for them while the share is
/// still received by the owner. Reported apart; the ruling ("R1 does not
/// move") accepts it. The whole-hit procs get the exact check instead
/// (`check_procs`).
fn cluster(raws: &[&str], lines: &[LogLine], tol_ms: i64) -> ClusterStats {
    struct Cluster<'a> {
        hits: u64,
        shares: u64,
        landed: bool,
        landed_before_hit: bool,
        first_is_share: bool,
        last_ts: i64,
        first: &'a LogLine,
    }
    enum Kind {
        Hit(u64),
        Share(u64),
        Landed,
    }
    let mut open: HashMap<(String, String, bool), Cluster> = HashMap::new();
    let mut closed: Vec<Cluster> = Vec::new();
    for (raw, l) in raws.iter().zip(lines) {
        let (key, kind) = match &l.event {
            Event::Damage {
                src,
                dst,
                amount,
                absorbed,
                ..
            } => (
                (src.guid.clone(), dst.guid.clone(), false),
                Kind::Hit(amount + absorbed),
            ),
            Event::Heal {
                src,
                dst,
                amount,
                overheal,
                ..
            } => (
                (src.guid.clone(), dst.guid.clone(), true),
                Kind::Hit(amount.saturating_sub(*overheal)),
            ),
            Event::Support {
                src,
                dst,
                amount,
                healing,
                ..
            } => (
                (src.guid.clone(), dst.guid.clone(), *healing),
                Kind::Share(*amount),
            ),
            Event::Other if raw.contains("  SWING_DAMAGE_LANDED,") => {
                let mut f = raw.split(',');
                let src = f.nth(1).unwrap_or_default().to_string();
                let dst = f.nth(3).unwrap_or_default().to_string();
                ((src, dst, false), Kind::Landed)
            }
            _ => continue,
        };
        let stale = open.get(&key).is_some_and(|c| l.ts_ms - c.last_ts > tol_ms);
        if stale && let Some(c) = open.remove(&key) {
            closed.push(c);
        }
        let c = open.entry(key).or_insert_with(|| Cluster {
            hits: 0,
            shares: 0,
            landed: false,
            landed_before_hit: false,
            first_is_share: matches!(kind, Kind::Share(_)),
            last_ts: l.ts_ms,
            first: l,
        });
        c.last_ts = l.ts_ms;
        match kind {
            Kind::Hit(a) => {
                if c.hits == 0 && c.landed {
                    c.landed_before_hit = true;
                }
                c.hits += a;
            }
            Kind::Share(a) => c.shares += a,
            Kind::Landed => c.landed = true,
        }
    }
    closed.extend(open.into_values());

    let mut s = ClusterStats::default();
    for c in &closed {
        if c.landed_before_hit {
            s.landed_precedes_swing += 1;
        }
        if c.shares == 0 {
            continue;
        }
        s.groups += 1;
        let healing = matches!(
            c.first.event,
            Event::Heal { .. } | Event::Support { healing: true, .. }
        );
        if c.hits == 0 {
            if c.landed {
                s.landed_only += 1;
                s.landed_only_shares += c.shares;
            } else {
                s.unpaired += 1;
                if s.unpaired_samples.len() < 5 {
                    s.unpaired_samples.push(format!("{:?}", c.first.event));
                }
            }
            continue;
        }
        if c.first_is_share {
            s.share_opens_cluster += 1;
        }
        let ratio = c.shares as f64 / c.hits as f64;
        if healing {
            s.max_heal_ratio = s.max_heal_ratio.max(ratio);
        } else {
            s.max_damage_ratio = s.max_damage_ratio.max(ratio);
        }
        if c.shares > c.hits {
            s.over_groups += 1;
            if s.over_samples.len() < 5 {
                s.over_samples.push(format!(
                    "Σ shares {} > Σ hits {}: {:?}",
                    c.shares, c.hits, c.first.event
                ));
            }
        }
    }
    s
}

/// The whole-hit procs, exactly: every support line whose buff is one of
/// `PROC_BUFFS` has a hit with the same (src, dst, spell, side) within
/// `PAIR_TOL_MS` carrying the SAME amount (R1 / R2 form). Per proc spell:
/// (shares seen, shares matched exactly). A share with no equal twin is a
/// mismatch, sampled.
#[derive(Default)]
struct ProcStats {
    per_spell: BTreeMap<u32, (u64, u64)>,
    mismatches: Vec<String>,
}

/// Proc hits by (src, dst, spell, heal side): their (ts, amount)s.
type ProcHits<'a> = HashMap<(&'a str, &'a str, u32, bool), Vec<(i64, u64)>>;

fn check_procs(lines: &[LogLine], p: &mut ProcStats) {
    let mut hits: ProcHits = HashMap::new();
    for l in lines {
        match &l.event {
            Event::Damage {
                src,
                dst,
                spell: Some(sp),
                amount,
                absorbed,
                ..
            } if PROC_BUFFS.contains(&sp.id) => hits
                .entry((&src.guid, &dst.guid, sp.id, false))
                .or_default()
                .push((l.ts_ms, amount + absorbed)),
            Event::Heal {
                src,
                dst,
                spell,
                amount,
                overheal,
                ..
            } if PROC_BUFFS.contains(&spell.id) => hits
                .entry((&src.guid, &dst.guid, spell.id, true))
                .or_default()
                .push((l.ts_ms, amount.saturating_sub(*overheal))),
            _ => {}
        }
    }
    for l in lines {
        let Event::Support {
            src,
            dst,
            spell,
            amount,
            healing,
            ..
        } = &l.event
        else {
            continue;
        };
        if !PROC_BUFFS.contains(&spell.id) {
            continue;
        }
        let slot = p.per_spell.entry(spell.id).or_default();
        slot.0 += 1;
        let exact = hits
            .get(&(src.guid.as_str(), dst.guid.as_str(), spell.id, *healing))
            .is_some_and(|v| {
                v.iter()
                    .any(|(ts, a)| (ts - l.ts_ms).abs() <= PAIR_TOL_MS && a == amount)
            });
        if exact {
            slot.1 += 1;
        } else if p.mismatches.len() < 5 {
            p.mismatches.push(format!("{:?}", l.event));
        }
    }
}

#[derive(Default)]
struct Report {
    segments: usize,
    damage: u64,
    effective: u64,
    given: (u64, u64),
    support_lines: u64,
    orphan_lines: u64,
    orphan_srcs: HashSet<String>,
    healed: u64,
    healed_from_npcs: u64,
    credited: u64,
    /// Players whose received damage share exceeds their R1 damage — the
    /// cases `model::effective` clamps at 0 (a LANDED-only guardian's
    /// share is received by its owner with no counted hit to net it
    /// against). Counted; the partition is asserted exact only when none.
    clamped: usize,
    clamped_samples: Vec<String>,
    /// The working window's clustering, summed over the segments.
    at_tol: ClusterStats,
    /// The tight window's, for the sensitivity check.
    at_tight: ClusterStats,
    procs: ProcStats,
    /// Σ share amount the passive gate dropped (pre-pull shares).
    dropped_by_gate: u64,
}

fn fold(into: &mut ClusterStats, s: &ClusterStats) {
    into.groups += s.groups;
    into.unpaired += s.unpaired;
    into.over_groups += s.over_groups;
    into.landed_only += s.landed_only;
    into.landed_only_shares += s.landed_only_shares;
    into.max_damage_ratio = into.max_damage_ratio.max(s.max_damage_ratio);
    into.max_heal_ratio = into.max_heal_ratio.max(s.max_heal_ratio);
    into.landed_precedes_swing += s.landed_precedes_swing;
    into.share_opens_cluster += s.share_opens_cluster;
    for x in &s.unpaired_samples {
        if into.unpaired_samples.len() < 5 {
            into.unpaired_samples.push(x.clone());
        }
    }
    for x in &s.over_samples {
        if into.over_samples.len() < 5 {
            into.over_samples.push(x.clone());
        }
    }
}

/// The identities of one segment; folds the results into `r`.
fn check_segment(seg: &Segment, raws: &[&str], lines: &[LogLine], r: &mut Report) {
    let guids = guids_of(lines);
    let friendly_names: HashSet<String> = lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Heal { dst, spell, .. }
                if friendly(&dst.guid) && !NON_HEALING_ABSORBS.contains(&spell.id) =>
            {
                Some(dst.name.clone())
            }
            Event::Absorbed {
                dst, absorb_spell, ..
            } if friendly(&dst.guid) && !NON_HEALING_ABSORBS.contains(&absorb_spell.id) => {
                Some(dst.name.clone())
            }
            _ => None,
        })
        .collect();

    // Orphans: a support src that does not fold to a Player. A pet with a
    // known owner answers `None` for itself (its raw entry resolves to the
    // owner); a src that still answers for itself and is not a Player- is
    // an orphan, and its shares sit outside the partition.
    let mut supporters: HashSet<&str> = HashSet::new();
    let mut srcs: HashSet<&str> = HashSet::new();
    for l in lines {
        if let Event::Support { src, supporter, .. } = &l.event {
            supporters.insert(supporter);
            srcs.insert(&src.guid);
        }
    }
    for src in &srcs {
        if !src.starts_with("Player-") && seg.support(src).is_some() {
            r.orphan_srcs.insert((*src).to_string());
            r.orphan_lines += lines
                .iter()
                .filter(|l| matches!(&l.event, Event::Support { src: s, .. } if s.guid == *src))
                .count() as u64;
        }
    }

    // Σ effective = Σ damage over every key that can carry a term: Damage
    // rows (owner-folded), every supporter, every support src, every
    // Player- guid — a guid with no row and no support answers 0. The
    // partition is exact iff no player's received exceeds their damage:
    // `model::effective` clamps those at 0, and each clamp leaves the
    // shortfall unaccounted. Counted here, asserted to be absent.
    let rows = seg.rows(View::Damage);
    let damage: u64 = rows.iter().map(|x| x.amount).sum();
    let damage_of: HashMap<&str, u64> = rows.iter().map(|x| (x.key.as_str(), x.amount)).collect();
    let mut keys: HashSet<&str> = damage_of.keys().copied().collect();
    keys.extend(supporters.iter().copied());
    keys.extend(srcs.iter().copied());
    keys.extend(
        guids
            .iter()
            .map(String::as_str)
            .filter(|g| g.starts_with("Player-")),
    );
    let mut clamped_here = 0usize;
    for k in &keys {
        let received = seg.support(k).map_or(0, |s| s.received_damage);
        let dmg = damage_of.get(k).copied().unwrap_or(0);
        if received > dmg {
            clamped_here += 1;
            if r.clamped_samples.len() < 5 {
                r.clamped_samples.push(format!(
                    "{}: {} received {} > damage {}",
                    seg.name,
                    seg.rows(View::Damage)
                        .iter()
                        .find(|x| x.key == *k)
                        .map_or((*k).to_string(), |x| x.label.clone()),
                    received,
                    dmg
                ));
            }
        }
    }
    r.clamped += clamped_here;
    let effective: u64 = keys.iter().map(|k| seg.effective(k)).sum();
    assert_eq!(
        effective, damage,
        "{}: Σ effective vs Σ damage — exact when no player's received exceeds their damage \
         ({clamped_here} clamped here; the partition only holds when none do)",
        seg.name
    );
    r.damage += damage;
    r.effective += effective;
    for k in &guids {
        if let Some(s) = seg.support(k) {
            r.given.0 += s.given_damage;
            r.given.1 += s.given_healing;
        }
    }

    // The healing identity, exactly as tests/support.rs states it.
    let by_target: u64 = guids
        .iter()
        .flat_map(|g| seg.breakdown(g, View::Healing).1)
        .filter(|x| friendly_names.contains(&x.label))
        .map(|x| x.amount)
        .sum();
    let healed: u64 = guids
        .iter()
        .filter_map(|g| seg.healed(g))
        .map(|h| h.received)
        .sum();
    let credited: u64 = guids.iter().map(|g| seg.absorbed_healing(g)).sum();
    let hostile_absorbs: u64 = lines
        .iter()
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
        .sum();
    assert_eq!(
        by_target,
        healed + credited - hostile_absorbs,
        "{}: Healing by_target(friendly) vs healed received + absorb credit − NPC-shield credit",
        seg.name
    );
    r.healed += healed;
    r.credited += credited;
    r.healed_from_npcs += lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Heal {
                src,
                dst,
                spell,
                amount,
                overheal,
                ..
            } if friendly(&dst.guid)
                && !friendly(&src.guid)
                && !NON_HEALING_ABSORBS.contains(&spell.id) =>
            {
                Some(amount.saturating_sub(*overheal))
            }
            _ => None,
        })
        .sum::<u64>();
    for x in seg.rows(View::Healing) {
        assert!(
            seg.absorbed_healing(&x.key) <= x.amount,
            "{}: {} absorbed {} > healing {}",
            seg.name,
            x.label,
            seg.absorbed_healing(&x.key),
            x.amount
        );
    }

    // The share clusters at the working window and at the tight one: the
    // verdicts (unpaired, over) must not depend on the window, or the
    // window would be doing the pairing's work.
    let at_tol = cluster(raws, lines, PAIR_TOL_MS);
    let at_tight = cluster(raws, lines, TIGHT_TOL_MS);
    assert_eq!(
        (at_tol.unpaired, at_tol.over_groups),
        (at_tight.unpaired, at_tight.over_groups),
        "{}: (unpaired, over) at {PAIR_TOL_MS} ms vs {TIGHT_TOL_MS} ms — the window is not load-bearing",
        seg.name
    );
    fold(&mut r.at_tol, &at_tol);
    fold(&mut r.at_tight, &at_tight);
    check_procs(lines, &mut r.procs);

    // The passive gate: shares the slice carries that the ledger did not
    // keep — a share logged before the pull's first hit (a swing's
    // LANDED-before-SWING_DAMAGE order makes that real). Reported, never
    // asserted: it is R19's stated behaviour, and full = lazy on it.
    let parsed_shares: u64 = lines
        .iter()
        .filter_map(|l| match &l.event {
            Event::Support { amount, .. } => Some(*amount),
            _ => None,
        })
        .sum();
    let kept: u64 = guids
        .iter()
        .filter_map(|g| seg.support(g))
        .map(|s| s.given_damage + s.given_healing)
        .sum();
    r.dropped_by_gate += parsed_shares - kept;
    r.segments += 1;
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn support_partitions_damage_on_every_real_segment() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
    assert!(!metas.is_empty(), "a real log has segments");

    let mut r = Report::default();
    let mut census: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut parse_ms = 0u128;

    for meta in &metas {
        let text = load_segment_text(Path::new(&path), meta).expect("load the segment");
        let t = Instant::now();
        let lines: Vec<LogLine> = text.lines().filter_map(parse_line).collect();
        let meter = meter_from_lines(text.lines());
        parse_ms += t.elapsed().as_millis();
        let raws: Vec<&str> = text.lines().filter(|l| parse_line(l).is_some()).collect();

        // Census: every `_SUPPORT` line by family — the six parse as
        // Support, every other name (SPELL_ABSORBED_SUPPORT above all) as
        // Other. Counted per (family: parsed, other).
        for raw in text.lines() {
            let Some(ev) = raw.split("  ").nth(1).and_then(|x| x.split(',').next()) else {
                continue;
            };
            if !ev.ends_with("_SUPPORT") {
                continue;
            }
            let slot = census.entry(ev.to_string()).or_default();
            match parse_line(raw).map(|l| l.event) {
                Some(Event::Support { .. }) => slot.0 += 1,
                _ => slot.1 += 1,
            }
            r.support_lines += 1;
        }

        // The seeded slice replays to exactly one segment — the meta's.
        assert_eq!(
            meter.segments().len(),
            1,
            "{}: one segment per slice",
            meta.name
        );
        check_segment(&meter.segments()[0], &raws, &lines, &mut r);
    }

    let c = &r.at_tol;
    println!(
        "{} segments checked ({} encounters), Σ damage {} = Σ effective {} ({} players clamped: received > damage); \
         Σ given damage {} / healing {}; {} support lines, {} orphan lines from {} srcs {:?}; \
         Σ healed received {} (from NPCs {}), Σ absorb credit {}; \
         {} share groups at {PAIR_TOL_MS} ms: {} unpaired, {} over their hit, max share/hit {:.4} (damage) {:.4} (heal); \
         at {TIGHT_TOL_MS} ms: {} groups, {} unpaired, {} over; \
         {} swing clusters with LANDED before SWING_DAMAGE, {} spell clusters opening on a share, \
         {} LANDED-only clusters carrying {} of shares, \
         {} share amount dropped by the passive gate; \
         parse+meter {parse_ms} ms",
        r.segments,
        metas
            .iter()
            .filter(|m| m.kind == wowdps_core::meter::SegmentKind::Encounter)
            .count(),
        r.damage,
        r.effective,
        r.clamped,
        r.given.0,
        r.given.1,
        r.support_lines,
        r.orphan_lines,
        r.orphan_srcs.len(),
        r.orphan_srcs,
        r.healed,
        r.healed_from_npcs,
        r.credited,
        c.groups,
        c.unpaired,
        c.over_groups,
        c.max_damage_ratio,
        c.max_heal_ratio,
        r.at_tight.groups,
        r.at_tight.unpaired,
        r.at_tight.over_groups,
        c.landed_precedes_swing,
        c.share_opens_cluster,
        c.landed_only,
        c.landed_only_shares,
        r.dropped_by_gate,
    );
    for (family, (parsed, other)) in &census {
        println!("  {family}: {parsed} Support, {other} Other");
    }
    for (spell, (seen, exact)) in &r.procs.per_spell {
        println!("  proc {spell}: {seen} shares, {exact} equal to their own hit");
    }
    for s in &r.procs.mismatches {
        println!("  proc mismatch: {s}");
    }
    for s in &r.clamped_samples {
        println!("  clamped: {s}");
    }
    for s in &c.unpaired_samples {
        println!("  unpaired: {s}");
    }
    for s in &c.over_samples {
        println!("  over: {s}");
    }
    for fam in FAMILIES {
        let (parsed, other) = census.get(fam).copied().unwrap_or_default();
        assert_eq!(
            other, 0,
            "{fam}: every line of a modeled family parses as Support"
        );
        if parsed == 0 {
            println!("  ({fam}: absent from this log)");
        }
    }
    for (family, (parsed, _)) in &census {
        if !FAMILIES.contains(&family.as_str()) {
            assert_eq!(*parsed, 0, "{family}: an unmodeled family must stay Other");
        }
    }
    assert!(
        r.support_lines > 0,
        "an Augmentation log carries support lines"
    );
    assert_eq!(
        r.orphan_srcs.len(),
        0,
        "every support src folds to a player: {:?}",
        r.orphan_srcs
    );
    assert_eq!(
        c.unpaired, 0,
        "every share cluster has a hit (or a LANDED twin) within {PAIR_TOL_MS} ms: {:?}",
        c.unpaired_samples
    );
    assert_eq!(
        c.over_groups, 0,
        "a share cluster never exceeds its hits: {:?}",
        c.over_samples
    );
    assert!(c.max_damage_ratio <= 1.0 && c.max_heal_ratio <= 1.0);
    // The whole-hit procs: every share equals its own hit, exactly.
    for (spell, (seen, exact)) in &r.procs.per_spell {
        assert_eq!(
            seen, exact,
            "proc {spell}: every share equals its own hit: {:?}",
            r.procs.mismatches
        );
    }
    assert!(
        r.procs.per_spell.values().any(|(seen, _)| *seen > 0),
        "an Augmentation log carries at least one whole-hit proc share"
    );
    // The sensitivity check, once more over the totals.
    assert_eq!(
        (c.unpaired, c.over_groups),
        (r.at_tight.unpaired, r.at_tight.over_groups),
        "the pairing window ({PAIR_TOL_MS} ms vs {TIGHT_TOL_MS} ms) is not load-bearing"
    );
}
