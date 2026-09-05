//! R18 over a real log (`docs/plan-role-pivots-step4.md` §0, the real-log
//! gate): over every segment — pulls and trash alike — no negative span
//! duration, at most one span per (target, spell) still open at the end,
//! every player's AM union within the segment's duration and within the
//! sum of its AM spans, Σ externals given = Σ received, a census of the
//! role-table ids actually seen, and the wall time of the parse.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_spans -- --ignored --nocapture`

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::Instant;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};
use wowdps_model::MarkKind;

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn spans_balance_on_every_real_segment() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
    assert!(!metas.is_empty(), "a real log has segments");

    let mut parse_ms = 0u128;
    let mut segments = 0usize;
    let mut pulls = 0usize;
    let mut spans_total = 0usize;
    let mut open_at_end = 0usize;
    let mut census: BTreeMap<(u32, String, u8), u32> = BTreeMap::new();
    let mut am_players = 0usize;
    let mut am_max_pct = 0.0f64;
    let mut externals = (0u32, 0i64);
    let mut supporters = 0usize;
    let mut trash_tail_spans = 0usize;
    let mut trash_tail_am = 0usize;
    let mut casters: BTreeMap<String, u32> = BTreeMap::new();

    for meta in &metas {
        let text = load_segment_text(Path::new(&path), meta).expect("load the segment");
        let t = Instant::now();
        let lines: Vec<_> = text.lines().filter_map(parse_line).collect();
        let meter = meter_from_lines(text.lines());
        parse_ms += t.elapsed().as_millis();

        // Every guid a span can be keyed on or folded to — casters included
        // whatever they are (an NPC-sourced external is still given by
        // SOMEONE, and the identity is per caster, not per player).
        let mut players: HashSet<String> = HashSet::new();
        for l in &lines {
            if let Some(h) = &l.owner_hint {
                players.insert(h.owner_guid.clone());
            }
            match &l.event {
                Event::AuraApplied { src, dst, .. }
                | Event::AuraRefresh { src, dst, .. }
                | Event::AuraRemoved { src, dst, .. }
                | Event::Damage { src, dst, .. }
                | Event::Heal { src, dst, .. } => {
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
        let mut keys: Vec<&String> = players.iter().collect();
        keys.sort();

        for seg in meter.segments() {
            segments += 1;
            if seg.kind == SegmentKind::Encounter {
                pulls += 1;
            }
            let duration = seg.duration_ms(seg.last_combat_ms());
            // An Encounter's clock is its END, and an aura after it lands
            // nowhere, so no span can pass it. A Trash segment's R7 clock
            // is its LAST COMBAT LINE, while the passive gate admits an
            // aura up to the 60 s trash gap after that — a removal in that
            // idle tail closes a span past the R7 duration (the truth: the
            // buff was on that long), so the bound is the gate's.
            let bound = match seg.kind {
                SegmentKind::Encounter => duration,
                SegmentKind::Trash | SegmentKind::Overall => duration + 60_000,
            };
            let mut given = (0u32, 0i64);
            let mut received = (0u32, 0i64);
            for k in &keys {
                let spans = seg.spans(k);
                spans_total += spans.len();
                let mut open_per_spell: BTreeMap<u32, usize> = BTreeMap::new();
                for s in &spans {
                    assert!(s.dur_ms >= 0, "{}: {k} {s:?}", seg.name);
                    assert!(s.at_ms >= 0, "{}: {k} {s:?}", seg.name);
                    assert!(
                        s.at_ms + s.dur_ms <= bound.max(s.at_ms),
                        "{}: {k} span past the end: {s:?} (duration {duration})",
                        seg.name
                    );
                    if s.at_ms + s.dur_ms > duration {
                        trash_tail_spans += 1;
                    }
                    // "Open at end" is observable as a span closed exactly on
                    // the segment's clock.
                    if s.at_ms + s.dur_ms == duration {
                        *open_per_spell.entry(s.spell_id).or_default() += 1;
                    }
                }
                for (spell, n) in open_per_spell {
                    open_at_end += n;
                    assert!(
                        n <= 1,
                        "{}: {k} spell {spell} has {n} spans open at the end",
                        seg.name
                    );
                }
                for u in seg.uptime(k) {
                    *census
                        .entry((u.spell_id, u.label.clone(), u.kind.code()))
                        .or_default() += u.count;
                }
                let am = seg.am_uptime_ms(k);
                let am_sum: i64 = seg
                    .uptime(k)
                    .iter()
                    .filter(|u| u.kind == MarkKind::ActiveMitigation)
                    .map(|u| u.total_ms)
                    .sum();
                assert!(
                    am >= 0 && am <= am_sum,
                    "{}: {k} am {am} > Σ {am_sum}",
                    seg.name
                );
                assert!(
                    am <= bound,
                    "{}: {k} am {am} > duration {duration}",
                    seg.name
                );
                if am > duration {
                    trash_tail_am += 1;
                }
                if am > 0 {
                    am_players += 1;
                    am_max_pct = am_max_pct.max(am as f64 * 100.0 / duration.max(1) as f64);
                }
                let g = seg.externals_given(k);
                if g.0 > 0 {
                    let prefix = k.split('-').next().unwrap_or("?").to_string();
                    *casters.entry(prefix).or_default() += g.0;
                }
                let r = seg.externals_received(k);
                given = (given.0 + g.0, given.1 + g.1);
                received = (received.0 + r.0, received.1 + r.1);
                if !seg.support_uptime(k).is_empty() {
                    supporters += 1;
                }
            }
            assert_eq!(given, received, "{}: externals given vs received", seg.name);
            externals = (externals.0 + given.0, externals.1 + given.1);
        }
    }

    println!(
        "{segments} segments ({pulls} pulls), {spans_total} spans listed, {open_at_end} open at \
         an end; AM on {am_players} player-segments (max {am_max_pct:.1} %); externals \
         {} spans / {} ms balanced; {supporters} supporter-segments; {trash_tail_spans} spans \
         closed in a trash segment's idle tail ({trash_tail_am} AM unions past the R7 clock); \
         parse+meter {parse_ms} ms",
        externals.0, externals.1
    );
    println!("external casters by guid prefix: {casters:?}");
    println!("role-table census (id, name, kind → closed+open spans):");
    for ((id, name, kind), n) in &census {
        let kind = MarkKind::from_code(*kind);
        println!("  {id:>7} {name:<28} {kind:?} {n}");
    }
    assert!(spans_total > 0, "a real raid log carries role spans");
}
