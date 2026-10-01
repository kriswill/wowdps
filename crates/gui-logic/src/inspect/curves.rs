//! The graph's curves, as the inspector cuts them from a timeline: the
//! rate in the prototype's 10 s buckets (finer only for a stretch too
//! short to hold forty), or the running total; the stretch on show and
//! its one scale; the R23 dead spans and their words; and the R26 series
//! a graph can stack. Moved from the iced window's inspector.

use wowdps_model::{AbilitySeries, GraphMode, Mark, MarkKind, Timeline};
use wowdps_proto::ClientState;

use super::{Roster, plot, stack};
use crate::graph::mmss;
use crate::labels::display_name;

/// The rate curve's buckets: the prototype's 10 s ("per second, 10 s"),
/// drawn through its spline — finer only when the stretch on show is too
/// short to hold [`RATE_POINTS`] of them (a zoom, a short pull), down to
/// the timeline's own second.
pub const RATE_BUCKET_MS: u32 = 10_000;

pub const RATE_POINTS: u32 = 40;

/// "died 5:45", "died 5:45, rezzed by Gennar" — an R23 death span as the
/// hatch and the head word it. `names` resolves the rezzer.
pub fn death_words(m: &Mark, roster: &Roster) -> String {
    let mut words = format!("died {}", mmss(m.at_ms.max(0) as u32));
    let spell = m
        .label
        .strip_prefix("Death (")
        .and_then(|s| s.strip_suffix(')'));
    if !m.src.is_empty() {
        // A rezzer no meter named (a pet's guid, someone never on screen)
        // goes unnamed: a guid's tail is no name a reader knows.
        match roster.get(&m.src) {
            Some((n, _)) => words.push_str(&format!(", rezzed by {}", display_name(n))),
            None => words.push_str(", rezzed"),
        }
    } else if let Some(spell) = spell {
        // A self-rez (an Ankh, a Soulstone they clicked) names the spell.
        words.push_str(&format!(", {spell}"));
    }
    words
}

/// The deaths on a timeline (R23), as the plot hatches them: from the
/// death to the rez that ended it, or — with none — to the fight's end.
/// R23 also closes a span at the first thing only the living do, and a
/// dead warlock's DoTs still tick in their name: without a rez the span's
/// own end is a guess, and the hatch does not draw one.
pub fn dead_spans(
    t: &Timeline,
    end_ms: u32,
    who: Option<&str>,
    roster: &Roster,
) -> Vec<plot::Dead> {
    t.marks
        .iter()
        .filter(|m| m.kind == MarkKind::Death)
        .map(|m| {
            let words = death_words(m, roster);
            let rezzed = !m.src.is_empty() || m.label.starts_with("Death (");
            let end = if rezzed {
                m.at_ms + m.dur_ms.max(0)
            } else {
                i64::from(end_ms)
            };
            plot::Dead {
                at_ms: m.at_ms,
                end_ms: end.min(i64::from(end_ms)),
                words: match who {
                    Some(who) => format!("{who} {words}"),
                    None => words,
                },
            }
        })
        .collect()
}

/// The rate curve's bucket for a window `window_ms` long over a timeline
/// of `base_ms` buckets: [`RATE_BUCKET_MS`], or as fine as keeps
/// [`RATE_POINTS`] in the window — a whole number of the timeline's own,
/// and never finer than one.
pub fn rate_bucket(window_ms: u32, base_ms: u32) -> u32 {
    let base = base_ms.max(1);
    let want = (window_ms / RATE_POINTS).clamp(base, RATE_BUCKET_MS.max(base));
    (want / base).max(1) * base
}

/// The curve a mode draws, as (values, their bucket's ms): the rate in
/// `bucket_ms` buckets ([`bucket_rate`]), or the running total on the
/// timeline's own grid.
pub fn curve(t: &Timeline, mode: GraphMode, bucket_ms: u32) -> (Vec<f64>, u32) {
    match mode {
        GraphMode::Dps => bucket_rate(t, bucket_ms),
        GraphMode::Total => (
            t.cumulative().into_iter().map(|v| v as f64).collect(),
            t.bucket_ms,
        ),
    }
}

/// The rate per second in buckets of `bucket_ms`, as the prototype's curve
/// reads (10 s buckets, which the plot draws through its spline): each
/// bucket's sum over the seconds it spans — the last one over only the
/// seconds the fight gave it, so the curve ends at a real rate.
pub fn bucket_rate(t: &Timeline, bucket_ms: u32) -> (Vec<f64>, u32) {
    if t.bucket_ms == 0 || t.buckets.is_empty() {
        return (Vec::new(), bucket_ms.max(1));
    }
    let per = (bucket_ms / t.bucket_ms).max(1) as usize;
    let secs = f64::from(t.bucket_ms) / 1000.0;
    let points = t
        .buckets
        .chunks(per)
        .map(|c| c.iter().sum::<u64>() as f64 / (c.len() as f64 * secs))
        .collect();
    (points, per as u32 * t.bucket_ms)
}

/// The stretch of the fight a graph shows: a zoom, or the whole of it.
pub fn window_of(shown: Option<(u32, u32)>, span: u32) -> (u32, u32) {
    shown.filter(|(lo, hi)| hi > lo).unwrap_or((0, span))
}

/// The fight's span on the graph's axis: its duration, or the timeline's
/// own length where that is longer (a visit's Σ runs the visit's clock).
pub fn span_of(app: &ClientState, t: &Timeline) -> u32 {
    let clock = app.duration_ms().max(0) as u64;
    let grid = t.buckets.len() as u64 * u64::from(t.bucket_ms);
    clock.max(grid).clamp(1, u64::from(u32::MAX)) as u32
}

/// The highest point of `curves` inside `window`.
pub fn peak_in(curves: &[plot::Curve], window: (u32, u32)) -> f64 {
    let inside = |b: u64, i: usize| {
        let at = i as u64 * b;
        at + b > u64::from(window.0) && at <= u64::from(window.1)
    };
    let lone = curves
        .iter()
        .filter(|c| c.ink != plot::Ink::Stack)
        .flat_map(|c| {
            let b = u64::from(c.bucket_ms.max(1));
            c.points
                .iter()
                .enumerate()
                .filter_map(move |(i, v)| inside(b, i).then_some(*v))
        })
        .fold(0.0, f64::max);
    // R26: a stack peaks where its bands' SUM does — every band is cut on
    // one grid.
    let mut sum: Vec<f64> = Vec::new();
    let mut b = 1;
    for c in curves.iter().filter(|c| c.ink == plot::Ink::Stack) {
        b = u64::from(c.bucket_ms.max(1));
        if sum.len() < c.points.len() {
            sum.resize(c.points.len(), 0.0);
        }
        for (s, v) in sum.iter_mut().zip(&c.points) {
            *s += v.max(0.0);
        }
    }
    let stacked = sum
        .iter()
        .enumerate()
        .filter(|(i, _)| inside(b, *i))
        .map(|(_, v)| *v)
        .fold(0.0, f64::max);
    lone.max(stacked)
}

/// R26 (step 2): the curves the drill's graph can stack, with the context
/// their hues are seated in and whether they are an open ability's
/// targets — `None` when the snapshot carries none (a stored pull, a count
/// view, a session the daemon builds none for).
pub fn stack_series(app: &ClientState) -> Option<(String, Vec<AbilitySeries>, bool)> {
    let d = app.drill.as_ref()?;
    let (abilities, targets) = app.drill_series();
    let spell = app.drill_spell().map(|(k, _)| k.as_str());
    let (series, on_targets) = match spell {
        Some(_) => (targets, true),
        None => (abilities, false),
    };
    (!series.is_empty()).then(|| {
        (
            stack::context(&d.key, app.view, spell),
            series.to_vec(),
            on_targets,
        )
    })
}

/// R26 (step 2): the context and keys the window seats hues for, once per
/// snapshot.
pub fn stack_keys(app: &ClientState) -> Option<(String, Vec<String>)> {
    let (context, series, _) = stack_series(app)?;
    Some((context, series.into_iter().map(|s| s.key).collect()))
}
