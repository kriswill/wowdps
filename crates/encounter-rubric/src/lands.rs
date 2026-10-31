//! A spell's hits as they land, told from its periodic ticks under the
//! same id: a hit that leaves a damage-over-time aura (an orb's touch and
//! its dot, say) is written once as it lands and again at each tick, every row
//! the same spell. A `hit` trigger or ending with `lands = true` reads the
//! landings alone, and so do a volley's `hits` (`landings`): the one
//! reading for the replay and the editors.
//!
//! The log writes no flag for a periodic row, so a landing is told by what
//! it does write:
//!
//! 1. A hit within `LAND_SAME_MS` of its aura going on or gaining a stack
//!    on its victim lands.
//! 2. Else, where the spell's period is known (the game's tables, the
//!    draft's `[spell.<id>] period`), a hit one period after an earlier
//!    hit of it on the same victim (within `TICK_SLACK_MS`) is a tick, so
//!    long as the aura is still on from the victim's last landing (within
//!    the spell's `duration` of it, where the tables give one); any other
//!    lands. Any earlier hit, not only the last: a second landing on a
//!    victim the aura already ticks on keeps the ticks' cadence, so the
//!    tick after it lies a period after the last tick, not
//!    after the landing.
//! 3. Else, a spell whose aura the log writes on some victim is a tick but
//!    for rule 1, and one whose aura it never writes lands every time.
//!
//! The aura rows alone (rule 3) find too few where the log leaves them out:
//! a kill can write several times as many hits of a spell as rows of its
//! aura going on or stacking, and the cadence (rule 2) tells its landings
//! from its ticks. The ticks keep their cadence closely: nearly every hit
//! within 50 ms of one period after an earlier hit lies within half that
//! of it, and past 50 ms the hits spread evenly (the landings'). A landing that falls within
//! `TICK_SLACK_MS` of the cadence by chance is still read as a tick: the
//! log writes no row as a touch refreshes an aura already on (the cut
//! keeps only its going on, gaining a stack and coming off), so rule 1
//! cannot catch it. A periodic flag in the cut would be the honest source
//! (a replay format bump).

/// How near a hit and its aura going on or gaining a stack are one
/// landing (ms).
pub const LAND_SAME_MS: u32 = 100;

/// How far from one period after an earlier hit a tick may fall (ms): its
/// cadence keeps within a few hundredths of a second.
pub const TICK_SLACK_MS: u32 = 50;

/// What the game's tables say of a spell's aura, for its ticks: how often
/// it ticks and how long one application lasts (ms; none, until taken
/// away).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dot {
    pub period: u32,
    pub lasts: Option<u32>,
}

/// A hit of a spell on a victim, or its aura going on or gaining a stack
/// there: when (ms), on whom, which spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub t: u32,
    pub unit: usize,
    pub spell: u32,
}

/// One victim's hits of one spell as `landings` reads them: its unit and
/// spell, its hits so far and its last landing.
type Chain = ((usize, u32), Vec<u32>, Option<u32>);

/// Whether each of `hits` lands, in their order (any order): `ons` are
/// the auras going on or gaining a stack, and `dot` the spell's ticks
/// where the game's tables give a period (rules in the module's words).
/// A spell with no period and no aura row lands every time.
pub fn landings(hits: &[Row], ons: &[Row], dot: impl Fn(u32) -> Option<Dot>) -> Vec<bool> {
    use std::collections::{HashMap, HashSet};
    // Each victim's aura rows of each spell, in time order.
    let mut on: HashMap<(usize, u32), Vec<u32>> = HashMap::new();
    for r in ons {
        on.entry((r.unit, r.spell)).or_default().push(r.t);
    }
    let auras: HashSet<u32> = on.keys().map(|k| k.1).collect();
    for v in on.values_mut() {
        v.sort_unstable();
    }
    // Whether a sorted list holds a time within `slack` of `t`.
    let near = |ts: &[u32], t: u32, slack: u32| {
        let i = ts.partition_point(|&x| x < t.saturating_sub(slack));
        ts.get(i).is_some_and(|&x| x <= t.saturating_add(slack))
    };
    // The hits by victim and spell, then in time order: each read after
    // the ones before it, the last landing known.
    let mut order: Vec<usize> = (0..hits.len()).collect();
    order.sort_by_key(|&i| hits.get(i).map(|h| (h.unit, h.spell, h.t)));
    let mut out = vec![false; hits.len()];
    // The victim and spell read, its hits so far and its last landing.
    let mut chain: Option<Chain> = None;
    for i in order {
        let Some(h) = hits.get(i) else {
            continue;
        };
        let key = (h.unit, h.spell);
        if chain.as_ref().is_none_or(|(k, _, _)| *k != key) {
            chain = Some((key, Vec::new(), None));
        }
        let Some((_, struck, last)) = chain.as_mut() else {
            continue;
        };
        let lands = if on.get(&key).is_some_and(|ts| near(ts, h.t, LAND_SAME_MS)) {
            true
        } else {
            match dot(h.spell).filter(|d| d.period > TICK_SLACK_MS) {
                // A tick lies a period after an earlier hit on its victim,
                // while the aura from its last landing is on.
                Some(d) => {
                    let cadence = (h.t.checked_sub(d.period))
                        .is_some_and(|back| near(struck, back, TICK_SLACK_MS));
                    let on = d.lasts.is_none_or(|l| {
                        last.is_some_and(|at| h.t.saturating_sub(at) <= l + TICK_SLACK_MS)
                    });
                    !(cadence && on)
                }
                None => !auras.contains(&h.spell),
            }
        };
        if lands {
            *last = Some(h.t);
        }
        struck.push(h.t);
        if let Some(o) = out.get_mut(i) {
            *o = lands;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(t: u32, unit: usize) -> Row {
        Row { t, unit, spell: 7 }
    }

    /// With a period, a hit one period after an earlier one on its victim
    /// is a tick, any other a landing: a second landing mid-dot keeps the
    /// ticks' cadence, so the tick after it is a period from the last
    /// tick, not from it. An aura row at a hit makes it a landing even on
    /// the cadence; another victim's hits are its own.
    #[test]
    fn a_hit_off_its_victims_tick_cadence_lands() {
        let hits = [
            row(10_000, 0),
            row(12_000, 0),
            row(14_030, 0),
            // A second orb touched mid-dot, then the tick on the cadence.
            row(15_100, 0),
            row(16_020, 0),
            // On the cadence, but its aura's dose row says it landed.
            row(18_000, 0),
            // Another victim, 2 s after the first's landing.
            row(12_000, 1),
        ];
        let ons = [row(10_000, 0), row(18_040, 0)];
        let dot = |_| {
            Some(Dot {
                period: 2_000,
                lasts: None,
            })
        };
        let lands = landings(&hits, &ons, dot);
        assert_eq!(lands, [true, false, false, true, false, true, true]);
    }

    /// A tick keeps within TICK_SLACK_MS of its cadence; a hit further off
    /// lands. With the aura's duration known, a hit on the cadence after
    /// the aura from the victim's last landing has run out is a fresh
    /// landing, not a tick.
    #[test]
    fn a_tick_keeps_its_cadence_while_its_aura_is_on() {
        let dot = |lasts| {
            move |_| {
                Some(Dot {
                    period: 2_000,
                    lasts,
                })
            }
        };
        // 40 ms off the cadence a tick, 60 ms off a landing; then ticks
        // on from that landing's.
        let hits = [
            row(10_000, 0),
            row(12_040, 0),
            row(14_100, 0),
            row(16_100, 0),
            row(18_100, 0),
        ];
        assert_eq!(
            landings(&hits, &[], dot(None)),
            [true, false, true, false, false]
        );
        // Its aura 3 s long: the hit 4 s after the last landing is one.
        assert_eq!(
            landings(&hits, &[], dot(Some(3_000))),
            [true, false, true, false, true]
        );
    }
}
