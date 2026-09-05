//! R20 over a real log (`docs/plan-role-pivots-step5.md` §0, the real-log
//! gate): over every segment — pulls and trash alike — Σ rows.consumed =
//! `absorbed_healing` for every player, `applied >= consumed + wasted` on
//! every row whose shields all closed with a known size — and `=` on every
//! spell the census never saw shrink (raise-only: a removal trailer below
//! the balance closes the shield as unknown, so a known row is never short),
//! no negatives (the unknown count never exceeds the count), `absorb_wasted`
//! `Some` only where a row has a known waste; a census of over-absorbs and refresh-downs by spell, with
//! the healer set — Power Word: Shield, Divine Aegis, Chi Cocoon, Life
//! Cocoon, Void Shield — asserted at 0 over-absorbs; and the wall time.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_shields -- --ignored --nocapture`

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, meter_from_lines};
use wowdps_core::parser::{AuraType, Event, parse_line};

/// The healer shields whose log sizes are trusted: an absorb never exceeds
/// the running balance. Every id the client carries under those names.
const HEALER_SET: &[(u32, &str)] = &[
    (17, "Power Word: Shield"),
    (47753, "Divine Aegis"),
    (116849, "Life Cocoon"),
    (165128, "Life Cocoon"),
    (406139, "Chi Cocoon"),
    (406220, "Chi Cocoon"),
    (432772, "Chi Cocoon"),
    (451299, "Chi Cocoon"),
    (177268, "Void Shield"),
    (302322, "Void Shield"),
];

/// One spell's ledger totals: (shields, applied, consumed, wasted, unknown).
type SpellTotals = (u32, u64, u64, u64, u32);

/// The census's own reading of one key's balance — independent of the
/// meter: the trailer semantics only (APPLIED = size, REFRESH = new total,
/// an absorb drains), so an over-absorb or a refresh-down is a fact about
/// the LOG, not the ledger.
#[derive(Default)]
struct Balance {
    remaining: Option<u64>,
}

#[derive(Default, Debug)]
struct Census {
    over_absorbs: u32,
    excess: u64,
    refresh_downs: u32,
    overwritten: u64,
    absorbs: u32,
    /// Removal trailers above / below the balance, and by how much.
    grew: u32,
    grown: u64,
    shrank: u32,
    shrunk: u64,
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn shields_balance_on_every_real_segment() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
    assert!(!metas.is_empty(), "a real log has segments");

    let mut parse_ms = 0u128;
    let mut segments = 0usize;
    let mut pulls = 0usize;
    let mut rows_total = 0usize;
    let mut known_rows = 0usize;
    let mut shields_total = 0u32;
    let mut unknown_total = 0u32;
    let mut consumed_total = 0u64;
    let mut wasted_total = 0u64;
    let mut applied_total = 0u64;
    let mut wasters = 0usize;
    let mut shielders = 0usize;
    let mut by_spell: BTreeMap<(u32, String), SpellTotals> = BTreeMap::new();
    let mut census: BTreeMap<(u32, String), Census> = BTreeMap::new();

    for meta in &metas {
        let text = load_segment_text(Path::new(&path), meta).expect("load the segment");
        let t = Instant::now();
        let lines: Vec<_> = text.lines().filter_map(parse_line).collect();
        let meter = meter_from_lines(text.lines());
        parse_ms += t.elapsed().as_millis();

        // Every guid a shield can be keyed on or folded to.
        let mut players: HashSet<String> = HashSet::new();
        // The census: per (target, spell, absorber) key, the balance the
        // trailers report.
        let mut balances: HashMap<(String, u32, String), Balance> = HashMap::new();
        for l in &lines {
            if let Some(h) = &l.owner_hint {
                players.insert(h.owner_guid.clone());
            }
            match &l.event {
                Event::AuraApplied {
                    src,
                    dst,
                    spell,
                    aura_type,
                    absorb,
                } => {
                    players.insert(src.guid.clone());
                    players.insert(dst.guid.clone());
                    if *aura_type == AuraType::Buff && absorb.is_some() {
                        balances.insert(
                            (dst.guid.clone(), spell.id, src.guid.clone()),
                            Balance { remaining: *absorb },
                        );
                    }
                }
                Event::AuraRefresh {
                    src,
                    dst,
                    spell,
                    aura_type,
                    absorb,
                } => {
                    players.insert(src.guid.clone());
                    players.insert(dst.guid.clone());
                    if *aura_type == AuraType::Buff
                        && let Some(r) = absorb
                        && let Some(b) =
                            balances.get_mut(&(dst.guid.clone(), spell.id, src.guid.clone()))
                    {
                        if let Some(rem) = b.remaining
                            && *r < rem
                        {
                            let c = census.entry((spell.id, spell.name.clone())).or_default();
                            c.refresh_downs += 1;
                            c.overwritten += rem - r;
                        }
                        b.remaining = Some(*r);
                    }
                }
                Event::AuraRemoved {
                    src,
                    dst,
                    spell,
                    absorb,
                    ..
                } => {
                    players.insert(src.guid.clone());
                    players.insert(dst.guid.clone());
                    if let Some(b) =
                        balances.remove(&(dst.guid.clone(), spell.id, src.guid.clone()))
                        && let (Some(w), Some(rem)) = (absorb, b.remaining)
                        && *w != rem
                    {
                        // The trailer disagrees with the balance the
                        // trailers built: the shield grew or shrank with no
                        // line saying so (stacking shields).
                        let c = census.entry((spell.id, spell.name.clone())).or_default();
                        if *w > rem {
                            c.grew += 1;
                            c.grown += w - rem;
                        } else {
                            c.shrank += 1;
                            c.shrunk += rem - w;
                        }
                    }
                }
                Event::Absorbed {
                    dst,
                    absorber,
                    absorb_spell,
                    amount,
                    ..
                } => {
                    players.insert(dst.guid.clone());
                    players.insert(absorber.guid.clone());
                    let c = census
                        .entry((absorb_spell.id, absorb_spell.name.clone()))
                        .or_default();
                    c.absorbs += 1;
                    if let Some(b) = balances.get_mut(&(
                        dst.guid.clone(),
                        absorb_spell.id,
                        absorber.guid.clone(),
                    )) && let Some(rem) = b.remaining
                    {
                        if *amount > rem {
                            c.over_absorbs += 1;
                            c.excess += amount - rem;
                            b.remaining = Some(0);
                        } else {
                            b.remaining = Some(rem - amount);
                        }
                    }
                }
                Event::Damage { src, dst, .. } | Event::Heal { src, dst, .. } => {
                    players.insert(src.guid.clone());
                    players.insert(dst.guid.clone());
                }
                Event::Summon { owner, pet } => {
                    players.insert(owner.guid.clone());
                    players.insert(pet.guid.clone());
                }
                _ => {}
            }
        }
        let mut keys: Vec<&String> = players
            .iter()
            .filter(|g| g.starts_with("Player-") || g.starts_with("Pet-"))
            .collect();
        keys.sort();

        for seg in meter.segments() {
            segments += 1;
            if seg.kind == SegmentKind::Encounter {
                pulls += 1;
            }
            for k in &keys {
                let rows = seg.shields(k);
                let sum: u64 = rows.iter().map(|r| r.consumed).sum();
                assert_eq!(
                    sum,
                    seg.absorbed_healing(k),
                    "{}: {k} Σ rows.consumed vs absorbed_healing",
                    seg.name
                );
                let waste = seg.absorb_wasted(k);
                let unknown = seg.shields_unknown(k);
                assert_eq!(
                    unknown,
                    rows.iter().map(|r| r.unknown).sum::<u32>(),
                    "{}: {k} shields_unknown vs Σ rows.unknown",
                    seg.name
                );
                if rows.is_empty() {
                    assert_eq!(waste, None, "{}: {k} waste without rows", seg.name);
                    continue;
                }
                shielders += 1;
                if waste.is_some() {
                    wasters += 1;
                }
                let mut sorted = true;
                for (i, r) in rows.iter().enumerate() {
                    rows_total += 1;
                    shields_total += r.count;
                    unknown_total += r.unknown;
                    consumed_total += r.consumed;
                    wasted_total += r.wasted;
                    applied_total += r.applied;
                    assert!(r.count > 0, "{}: {k} {r:?}", seg.name);
                    assert!(r.unknown <= r.count, "{}: {k} {r:?}", seg.name);
                    assert!(!r.label.is_empty(), "{}: {k} {r:?}", seg.name);
                    if r.unknown == 0 {
                        known_rows += 1;
                        // Raise-only: no transition ever lowers `applied`,
                        // so a fully known row is never short of its
                        // consumed + wasted; and a shrink (a removal
                        // trailer below the balance) closes as unknown, so
                        // on a spell the census never saw shrink the
                        // identity is exact.
                        assert!(
                            r.applied >= r.consumed + r.wasted,
                            "{}: {k} {r:?}",
                            seg.name
                        );
                        let shrank = census
                            .get(&(r.spell_id, r.label.clone()))
                            .is_some_and(|c| c.shrank > 0);
                        if !shrank {
                            assert_eq!(r.applied, r.consumed + r.wasted, "{}: {k} {r:?}", seg.name);
                        }
                    }
                    if i > 0 && rows[i - 1].consumed < r.consumed {
                        sorted = false;
                    }
                    let cell = by_spell.entry((r.spell_id, r.label.clone())).or_default();
                    cell.0 += r.count;
                    cell.1 += r.applied;
                    cell.2 += r.consumed;
                    cell.3 += r.wasted;
                    cell.4 += r.unknown;
                }
                assert!(sorted, "{}: {k} rows not consumed-desc: {rows:?}", seg.name);
                // A known waste implies a row that could have fixed one:
                // some closed shield, so not every shield is open (unknown
                // ≤ count is the bound; a row of only open shields has
                // unknown == count and no waste).
                if let Some(w) = waste {
                    assert!(
                        rows.iter()
                            .any(|r| r.unknown < r.count || r.wasted > 0 || w == 0),
                        "{}: {k} waste {w} from rows {rows:?}",
                        seg.name
                    );
                }
            }
        }
    }

    println!(
        "{segments} segments ({pulls} pulls); {shielders} shielder-segments ({wasters} with a \
         known waste); {rows_total} rows ({known_rows} fully known), {shields_total} shields \
         ({unknown_total} unknown-applied or open); Σ applied {applied_total}, Σ consumed \
         {consumed_total}, Σ wasted {wasted_total}; parse+meter {parse_ms} ms"
    );
    println!("ledger by spell (id, name → shields, applied, consumed, wasted, unknown):");
    for ((id, name), (n, a, c, w, u)) in &by_spell {
        println!("  {id:>7} {name:<28} {n:>6} {a:>12} {c:>12} {w:>12} {u:>5}");
    }
    println!(
        "census by spell (id, name → absorbs, over-absorbs / excess, refresh-downs / overwritten, \
         removals above the balance / by, below / by):"
    );
    let mut healer_over = Vec::new();
    for ((id, name), c) in &census {
        if c.over_absorbs > 0 || c.refresh_downs > 0 || c.grew > 0 || c.shrank > 0 {
            println!(
                "  {id:>7} {name:<28} {:>6} {:>5} / {:>10} {:>5} / {:>10} {:>5} / {:>10} {:>5} / {:>10}",
                c.absorbs,
                c.over_absorbs,
                c.excess,
                c.refresh_downs,
                c.overwritten,
                c.grew,
                c.grown,
                c.shrank,
                c.shrunk
            );
        }
        if HEALER_SET.iter().any(|(h, _)| h == id) && c.over_absorbs > 0 {
            healer_over.push((*id, name.clone(), c.over_absorbs, c.excess));
        }
    }
    let seen: Vec<_> = HEALER_SET
        .iter()
        .filter(|(id, _)| census.keys().any(|(k, _)| k == id))
        .collect();
    println!("healer set seen: {seen:?}");
    assert!(
        healer_over.is_empty(),
        "the healer set over-absorbed: {healer_over:?}"
    );
    assert!(rows_total > 0, "a real raid log carries shields");
}
