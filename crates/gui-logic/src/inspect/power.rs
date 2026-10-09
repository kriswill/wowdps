//! R28 (v44): a healer's mana under the inspector's graph — a thin line of
//! the pool as a fraction of its max, second by second, broken wherever no
//! line of the log reported it, on the graph's own time axis. The data is
//! the drilled player's `Breakdown.power` (live for a `Window` session, and
//! a stored pull's details tier); which line a player gets, and where its
//! points stand, is decided here so the window only strokes them.

use wowdps_model::{PowerSeries, Role, Spec, power_name};

use crate::labels::sentence;

/// The strip's height under the plot, at zoom 1.
pub const STRIP_H: f32 = 14.0;
/// Its gap under the plot's axis.
pub const STRIP_GAP: f32 = 4.0;
/// The line's width — thinner than a curve's 1.6.
pub const LINE_W: f32 = 1.2;
/// The words beside it (the lanes' size).
pub const WORDS_PX: f32 = 11.0;

/// The game's power type for mana.
pub const MANA: u32 = 0;

/// What the strip draws: the pool's name, its grid, and each second's pool
/// as a fraction of its max (`None` where no line reported it).
#[derive(Debug, Clone, PartialEq)]
pub struct PowerLine {
    pub label: String,
    pub bucket_ms: u32,
    pub fractions: Vec<Option<f64>>,
}

impl PowerLine {
    /// The strip's words: the pool and its lowest point ("Mana, low 12%"),
    /// or the pool alone when nothing reported.
    pub fn words(&self) -> String {
        let low = self.fractions.iter().flatten().copied().reduce(f64::min);
        match low {
            Some(f) => format!("{}, low {:.0}%", self.label, f * 100.0),
            None => self.label.clone(),
        }
    }
}

/// A power type's name as the window says it: "Mana", "Soul shards",
/// "Power 10" for one the game's list does not name.
pub fn power_label(power_type: u32) -> String {
    match power_name(power_type) {
        "" => format!("Power {power_type}"),
        name => sentence(&name.replace('_', " ")),
    }
}

/// The line a drilled player gets by default: a HEALER's mana, when their
/// pools carry it with a max to measure against. Anyone else — and a
/// healer whose mana went unreported — gets none.
pub fn healer_mana(spec: Option<Spec>, series: &[PowerSeries]) -> Option<PowerLine> {
    if spec.map(Spec::role) != Some(Role::Healer) {
        return None;
    }
    let mana = series
        .iter()
        .find(|s| s.power_type == MANA && s.max > 0 && s.reported() > 0)?;
    Some(PowerLine {
        label: power_label(MANA),
        bucket_ms: mana.bucket_ms.max(1),
        fractions: mana.fractions(),
    })
}

/// The line's runs in canvas units, on a strip `w` wide and `h` tall whose
/// time axis starts at `left` and shows `window` (ms from the fight's
/// start) — the plot's own mapping (`geometry::Plot::x_of`), so a second
/// stands under the same second of the curve. One polyline per run of
/// reported seconds, each point at its second's middle, a full pool at the
/// strip's top; a second outside the window is left out, and a run of one
/// second is a short level stroke across it.
pub fn runs(
    line: &PowerLine,
    window: (u32, u32),
    left: f32,
    w: f32,
    h: f32,
) -> Vec<Vec<(f32, f32)>> {
    let span = f64::from(window.1.saturating_sub(window.0).max(1));
    let width = (w - left).max(1.0);
    let x_of = |ms: f64| left + ((ms - f64::from(window.0)) / span) as f32 * width;
    let y_of = |f: f64| h - (f.clamp(0.0, 1.0) as f32) * h;
    let bucket = f64::from(line.bucket_ms);
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut run: Vec<(f32, f32)> = Vec::new();
    for (i, f) in line.fractions.iter().enumerate() {
        let from = i as f64 * bucket;
        let inside = from + bucket > f64::from(window.0) && from < f64::from(window.1);
        match (f, inside) {
            (Some(f), true) => run.push((x_of(from + bucket / 2.0), y_of(*f))),
            _ => {
                if !run.is_empty() {
                    out.push(std::mem::take(&mut run));
                }
            }
        }
    }
    if !run.is_empty() {
        out.push(run);
    }
    // A lone second has no length to stroke: give it its own second's width.
    let half = (bucket / span) as f32 * width / 2.0;
    for r in &mut out {
        if let [(x, y)] = r.as_slice() {
            *r = vec![(x - half, *y), (x + half, *y)];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::geometry::Plot;

    fn mana(per_sec: Vec<Option<u32>>) -> PowerSeries {
        PowerSeries {
            power_type: MANA,
            max: 1_000,
            bucket_ms: 1_000,
            per_sec,
        }
    }

    #[test]
    fn a_healer_gets_their_mana_and_nobody_else_any_line() {
        let series = vec![
            mana(vec![Some(1_000), None, Some(250)]),
            PowerSeries {
                power_type: 3,
                max: 100,
                bucket_ms: 1_000,
                per_sec: vec![Some(50)],
            },
        ];
        let line = healer_mana(Some(Spec::Discipline), &series).expect("a healer's mana");
        assert_eq!(line.label, "Mana");
        assert_eq!(line.fractions, vec![Some(1.0), None, Some(0.25)]);
        assert_eq!(line.words(), "Mana, low 25%");
        assert_eq!(healer_mana(Some(Spec::FrostMage), &series), None, "a dps");
        assert_eq!(healer_mana(None, &series), None, "no spec, no role");
        assert_eq!(
            healer_mana(Some(Spec::Discipline), &series[1..]),
            None,
            "no mana reported"
        );
    }

    #[test]
    fn power_types_read_as_words() {
        assert_eq!(power_label(0), "Mana");
        assert_eq!(power_label(7), "Soul shards");
        assert_eq!(power_label(19), "Essence");
        assert_eq!(power_label(10), "Power 10");
    }

    /// The strip keeps the plot's clock: a second stands at the x the plot
    /// gives its middle, a gap breaks the line, and a zoom keeps only the
    /// seconds inside it.
    #[test]
    fn runs_break_at_gaps_and_keep_the_plots_clock() {
        let line = PowerLine {
            label: "Mana".into(),
            bucket_ms: 1_000,
            fractions: vec![Some(1.0), Some(0.5), None, Some(0.0), Some(0.25)],
        };
        let (left, w, h) = (82.0, 482.0, 14.0);
        let r = runs(&line, (0, 5_000), left, w, h);
        assert_eq!(r.len(), 2, "{r:?}");
        let plot = Plot {
            window: (0, 5_000),
            peak: 1.0,
            curves: &[],
            dead: &[],
            lanes: &[crate::inspect::lanes::Row {
                lane: crate::inspect::lanes::Lane::Cooldowns,
                spans: Vec::new(),
            }],
            total: false,
            word: "hps",
            plot_h: 96.0,
        };
        assert_eq!(plot.left(), left, "lanes put the plot past the gutter");
        assert_eq!(r[0][0], (plot.x_of(500.0, w), 0.0), "full at the top");
        assert_eq!(r[0][1], (plot.x_of(1_500.0, w), 7.0));
        assert_eq!(r[1][0].1, h, "empty at the bottom");
        // A zoom on the last two seconds keeps that run alone.
        let z = runs(&line, (3_000, 5_000), 0.0, 200.0, h);
        assert_eq!(z.len(), 1);
        assert_eq!(z[0].len(), 2);
        assert!((z[0][0].0 - 50.0).abs() < 1e-4, "{z:?}");
        // A lone second strokes across its own width.
        let one = PowerLine {
            fractions: vec![None, Some(0.5), None],
            ..line
        };
        let r = runs(&one, (0, 3_000), 0.0, 300.0, h);
        assert_eq!(r, vec![vec![(100.0, 7.0), (200.0, 7.0)]]);
    }
}
