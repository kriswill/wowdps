//! R19 + the R2 amendment over a real log (`docs/plan-role-pivots-step3.md`
//! §4): on EVERY segment of the log — every support `src` folds to a
//! player (orphans = 0), Σ effective = Σ damage, the healing identity
//! (exactly as `tests/support.rs` pins it over the fixtures), a census by
//! `_SUPPORT` family (the six parse as `Support`, anything else is `Other`),
//! per hit the support shares paired with it (nearest same-(src, dst) hit
//! within a few ms — group sums, since two buffs share one hit) never
//! exceed it, and the wall time of the parse.
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

/// How far a share may be stamped from its hit and still pair with it.
const PAIR_TOL_MS: i64 = 10;

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
    max_damage_ratio: f64,
    max_heal_ratio: f64,
    over_groups: usize,
    unpaired: usize,
    groups: usize,
    unpaired_samples: Vec<String>,
    over_samples: Vec<String>,
    /// Clusters that open on a share, the hit following (the swing order).
    hit_after_share: u64,
    /// Clusters whose only hit is a SWING_DAMAGE_LANDED twin (a guardian
    /// outside the log filter), and the shares they carry.
    landed_only: usize,
    landed_only_shares: u64,
    /// Σ share amount the passive gate dropped (pre-pull shares).
    dropped_by_gate: u64,
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

    // Σ effective = Σ damage over the Damage rows plus every guid a support
    // line names on either side (a supporter with no row; a buffed pet's
    // owner with none).
    let rows = seg.rows(View::Damage);
    let damage: u64 = rows.iter().map(|x| x.amount).sum();
    let mut keys: HashSet<&str> = rows.iter().map(|x| x.key.as_str()).collect();
    keys.extend(supporters.iter().copied());
    keys.extend(srcs.iter().copied());
    keys.extend(
        guids
            .iter()
            .map(String::as_str)
            .filter(|g| g.starts_with("Player-")),
    );
    let effective: u64 = keys.iter().map(|k| seg.effective(k)).sum();
    assert_eq!(effective, damage, "{}: Σ effective vs Σ damage", seg.name);
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

    // Group sums per (src, dst) CLUSTER: every same-(src, dst) line — a hit
    // (R1 amount, `amount + absorbed`; for the heal side, a heal's
    // effective amount), a share, or a SWING_DAMAGE_LANDED twin — within
    // `PAIR_TOL_MS` of the cluster's previous line joins it, and per
    // cluster Σ shares ≤ Σ hits. Nothing finer is falsifiable: the game
    // stamps a spell's share up to 1 ms AFTER its hit (`.136` hit, `.137`
    // share), a swing's LANDED twin + share 1 ms BEFORE the SWING_DAMAGE
    // line (`.489` share, `.490` hit), two buffs share one hit, and one
    // (src, dst, ms) can carry two hits (a Frostbolt and the Fate Mirror
    // proc it triggered — both `src` = the buffed player). A cluster with
    // shares and NO hit is unpaired — unless its only hit is a LANDED twin:
    // a guardian outside the log filter (Army of the Dead ghouls) has its
    // swings logged from the target's view only, so R1 counts nothing for
    // them while the share is still received by the owner. Reported apart;
    // the ruling ("R1 does not move") accepts it.
    struct Cluster<'a> {
        hits: u64,
        shares: u64,
        landed: bool,
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
        let stale = open
            .get(&key)
            .is_some_and(|c| l.ts_ms - c.last_ts > PAIR_TOL_MS);
        if stale && let Some(c) = open.remove(&key) {
            closed.push(c);
        }
        let c = open.entry(key).or_insert_with(|| Cluster {
            hits: 0,
            shares: 0,
            landed: false,
            first_is_share: matches!(kind, Kind::Share(_)),
            last_ts: l.ts_ms,
            first: l,
        });
        c.last_ts = l.ts_ms;
        match kind {
            Kind::Hit(a) => c.hits += a,
            Kind::Share(a) => c.shares += a,
            Kind::Landed => c.landed = true,
        }
    }
    closed.extend(open.into_values());
    for c in &closed {
        if c.shares == 0 {
            continue;
        }
        r.groups += 1;
        let healing = matches!(
            c.first.event,
            Event::Heal { .. } | Event::Support { healing: true, .. }
        );
        if c.hits == 0 {
            if c.landed {
                r.landed_only += 1;
                r.landed_only_shares += c.shares;
            } else {
                r.unpaired += 1;
                if r.unpaired_samples.len() < 5 {
                    r.unpaired_samples.push(format!("{:?}", c.first.event));
                }
            }
            continue;
        }
        if c.first_is_share {
            r.hit_after_share += 1;
        }
        let ratio = c.shares as f64 / c.hits as f64;
        if healing {
            r.max_heal_ratio = r.max_heal_ratio.max(ratio);
        } else {
            r.max_damage_ratio = r.max_damage_ratio.max(ratio);
        }
        if c.shares > c.hits {
            r.over_groups += 1;
            if r.over_samples.len() < 5 {
                r.over_samples.push(format!(
                    "Σ shares {} > Σ hits {}: {:?}",
                    c.shares, c.hits, c.first.event
                ));
            }
        }
    }
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

    println!(
        "{} segments checked ({} encounters), Σ damage {} = Σ effective {}; \
         Σ given damage {} / healing {}; {} support lines, {} orphan lines from {} srcs {:?}; \
         Σ healed received {} (from NPCs {}), Σ absorb credit {}; \
         {} share groups: {} unpaired, {} over their hit, max share/hit {:.4} (damage) {:.4} (heal); \
         {} clusters open on a share (swing order), {} LANDED-only clusters carrying {} of shares, \
         {} share amount dropped by the passive gate; \
         parse+meter {parse_ms} ms",
        r.segments,
        metas
            .iter()
            .filter(|m| m.kind == wowdps_core::meter::SegmentKind::Encounter)
            .count(),
        r.damage,
        r.effective,
        r.given.0,
        r.given.1,
        r.support_lines,
        r.orphan_lines,
        r.orphan_srcs.len(),
        r.orphan_srcs,
        r.healed,
        r.healed_from_npcs,
        r.credited,
        r.groups,
        r.unpaired,
        r.over_groups,
        r.max_damage_ratio,
        r.max_heal_ratio,
        r.hit_after_share,
        r.landed_only,
        r.landed_only_shares,
        r.dropped_by_gate,
    );
    for (family, (parsed, other)) in &census {
        println!("  {family}: {parsed} Support, {other} Other");
    }
    for s in &r.unpaired_samples {
        println!("  unpaired: {s}");
    }
    for s in &r.over_samples {
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
        r.unpaired, 0,
        "every share cluster has a hit (or a LANDED twin) within {PAIR_TOL_MS} ms: {:?}",
        r.unpaired_samples
    );
    assert_eq!(
        r.over_groups, 0,
        "a share cluster never exceeds its hits: {:?}",
        r.over_samples
    );
    assert!(r.max_damage_ratio <= 1.0 && r.max_heal_ratio <= 1.0);
}
