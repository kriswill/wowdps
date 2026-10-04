//! The graph's states as data, for every crate that draws the graph: the
//! GUI's `inspector_plot_shots` renders them (as the iced window's plot
//! shots did, so the two pictures could be laid side by side and diffed
//! rather than eyeballed from two whole windows). Deterministic,
//! hand-shaped, five minutes long.

use wowdps_model::Class;

use super::{AXIS_GAP, AXIS_H, LANE_GAP, LANE_ROW, LANES_TOP, PLOT_H};
use crate::inspect::lanes::{Lane, Row, Span};
use crate::inspect::plot::{Curve, Dead, Ink};
use crate::theme::Color;
use crate::theme::{DataTokens, NAVY};

/// The width every sample is drawn at: the wide inspector's (520) less its
/// 16 px sides.
pub const WIDTH: f32 = 488.0;

/// One state of the graph, and where the pointer rests on it (canvas units
/// at zoom 1), if anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: &'static str,
    pub window: (u32, u32),
    pub peak: f64,
    pub curves: Vec<Curve>,
    pub dead: Vec<Dead>,
    pub lanes: Vec<Row>,
    pub total: bool,
    pub word: &'static str,
    pub pointer: Option<(f32, f32)>,
}

const FIGHT_MS: u32 = 300_000;
const BUCKET_MS: u32 = 10_000;

/// A rate that rises and falls like a pull: a high opener, a lull, a burst
/// under a cooldown, nothing while dead (145–190 s).
fn rate(scale: f64, phase: f64) -> Vec<f64> {
    (0..FIGHT_MS / BUCKET_MS)
        .map(|i| {
            let s = f64::from(i * BUCKET_MS) / 1000.0;
            if (145.0..190.0).contains(&s) {
                return 0.0;
            }
            let t = f64::from(i);
            let opener = if i < 3 { 60_000.0 } else { 0.0 };
            let burst = if (20..24).contains(&i) { 45_000.0 } else { 0.0 };
            scale * (110_000.0 + 35_000.0 * (t * 0.55 + phase).sin() + opener + burst)
        })
        .collect()
}

fn curve(name: &str, color: Color, points: Vec<f64>, ink: Ink) -> Curve {
    Curve {
        name: name.to_string(),
        color,
        points,
        bucket_ms: BUCKET_MS,
        ink,
    }
}

fn span(at_s: i64, dur_s: i64, label: &str, caster: Option<&str>, color: Color) -> Span {
    Span {
        at_ms: at_s * 1000,
        dur_ms: dur_s * 1000,
        label: label.to_string(),
        caster: caster.map(str::to_string),
        color,
        whose: None,
        second: false,
    }
}

fn lanes(own: Color) -> Vec<Row> {
    let shaman = Color::of_class(Class::Shaman);
    let priest = Color::of_class(Class::Priest);
    vec![
        Row {
            lane: Lane::Cooldowns,
            spans: vec![
                span(4, 20, "Summon Infernal", None, own),
                span(200, 20, "Summon Infernal", None, own),
            ],
        },
        Row {
            lane: Lane::Items,
            spans: vec![
                span(3, 0, "Tempered Potion", None, own),
                span(60, 15, "Signet of the Priory", None, own),
                span(240, 0, "Healthstone", None, own),
            ],
        },
        Row {
            lane: Lane::Externals,
            spans: vec![
                span(2, 40, "Heroism", Some("Vingsham"), shaman),
                span(10, 15, "Power Infusion", Some("Lumen"), priest),
                span(195, 8, "Pain Suppression", Some("Lumen"), priest),
            ],
        },
        Row {
            lane: Lane::Defensives,
            spans: vec![
                span(120, 8, "Unending Resolve", None, own),
                span(130, 0, "Dark Pact", None, own),
            ],
        },
    ]
}

fn dead() -> Vec<Dead> {
    vec![Dead {
        at_ms: 145_000,
        end_ms: 190_000,
        words: "died 2:25, rezzed by Lumen".to_string(),
    }]
}

fn peak(curves: &[Curve]) -> f64 {
    curves
        .iter()
        .flat_map(|c| c.points.iter().copied())
        .fold(0.0, f64::max)
}

/// The pair's lanes: each player's own, split — the first's over the
/// second's.
fn pair_lanes(a: Color, b: Color) -> Vec<Row> {
    let mark = |rows: Vec<Row>, who: &str, second: bool| -> Vec<Row> {
        rows.into_iter()
            .map(|r| Row {
                lane: r.lane,
                spans: r
                    .spans
                    .into_iter()
                    .map(|s| Span {
                        whose: Some(who.to_string()),
                        second,
                        ..s
                    })
                    .collect(),
            })
            .collect()
    };
    let first = mark(lanes(a), "Tranqlock", false);
    let mut second = mark(lanes(b), "Embersong", true);
    for r in &mut second {
        for s in &mut r.spans {
            s.at_ms += 12_000;
        }
    }
    first
        .into_iter()
        .zip(second)
        .map(|(mut x, y)| {
            x.spans.extend(y.spans);
            x
        })
        .collect()
}

/// Every state, by name: the curve alone; with its lanes; zoomed; the
/// pointer on the plot and on a span; a comparison (two classes, then one
/// class dashed); an ability over its ghost; the stack — its hues `navy`'s.
pub fn all() -> Vec<Sample> {
    all_in(&NAVY.data)
}

/// [`all`], the stack in `data`'s hues: a theme's own, for its shots.
pub fn all_in(data: &DataTokens) -> Vec<Sample> {
    let lock = Color::of_class(Class::Warlock);
    let mage = Color::of_class(Class::Mage);
    let own = vec![curve("", lock, rate(1.0, 0.0), Ink::Area)];
    let base = Sample {
        name: "alone",
        window: (0, FIGHT_MS),
        peak: peak(&own),
        curves: own.clone(),
        dead: dead(),
        lanes: Vec::new(),
        total: false,
        word: "dps",
        pointer: None,
    };
    let laned = Sample {
        name: "lanes",
        lanes: lanes(lock),
        ..base.clone()
    };
    let pair_curves = vec![
        curve("Tranqlock", lock, rate(1.0, 0.0), Ink::Line),
        curve("Embersong", mage, rate(0.85, 1.7), Ink::Line),
    ];
    let twin_curves = vec![
        curve("Tranqlock", lock, rate(1.0, 0.0), Ink::Line),
        curve("Grimveil", lock, rate(0.9, 2.4), Ink::Dashed),
    ];
    let ghosted = vec![
        curve("", lock, rate(1.0, 0.0), Ink::Ghost),
        curve("Chaos Bolt", lock, rate(0.35, 0.9), Ink::Area),
    ];
    let stacked: Vec<Curve> = ["Chaos Bolt", "Incinerate", "Wither", "Infernal"]
        .iter()
        .zip(data.stack())
        .enumerate()
        .map(|(i, (name, hue))| {
            curve(
                name,
                hue,
                rate(0.22 - 0.03 * i as f64, i as f64 * 0.8),
                Ink::Stack,
            )
        })
        .chain(std::iter::once(curve(
            "Other",
            data.stack_other,
            rate(0.08, 2.0),
            Ink::StackRest,
        )))
        .collect();
    let stack_peak = (0..FIGHT_MS / BUCKET_MS)
        .map(|i| {
            stacked
                .iter()
                .map(|c| c.points.get(i as usize).copied().unwrap_or(0.0))
                .sum::<f64>()
        })
        .fold(0.0, f64::max);
    vec![
        base.clone(),
        laned.clone(),
        Sample {
            name: "zoomed",
            window: (60_000, 150_000),
            ..laned.clone()
        },
        Sample {
            name: "hover-plot",
            pointer: Some((300.0, 50.0)),
            ..laned.clone()
        },
        Sample {
            name: "hover-span",
            // Power Infusion inside the Heroism, on the Externals lane (the
            // third): the later drawn of the two answers.
            pointer: Some((
                100.0,
                PLOT_H
                    + AXIS_GAP
                    + AXIS_H
                    + LANES_TOP
                    + 2.0 * (LANE_ROW + LANE_GAP)
                    + LANE_ROW / 2.0,
            )),
            ..laned.clone()
        },
        Sample {
            name: "pair",
            peak: peak(&pair_curves),
            curves: pair_curves,
            lanes: pair_lanes(lock, mage),
            ..base.clone()
        },
        Sample {
            name: "twins",
            peak: peak(&twin_curves),
            curves: twin_curves,
            lanes: Vec::new(),
            dead: Vec::new(),
            ..base.clone()
        },
        Sample {
            name: "ghost",
            peak: peak(&ghosted),
            curves: ghosted,
            ..laned.clone()
        },
        Sample {
            name: "stack",
            peak: stack_peak,
            curves: stacked,
            ..laned
        },
        Sample {
            name: "total",
            total: true,
            curves: vec![curve(
                "",
                lock,
                rate(1.0, 0.0)
                    .iter()
                    .scan(0.0, |sum, v| {
                        *sum += v * f64::from(BUCKET_MS) / 1000.0;
                        Some(*sum)
                    })
                    .collect(),
                Ink::Area,
            )],
            peak: rate(1.0, 0.0).iter().sum::<f64>() * f64::from(BUCKET_MS) / 1000.0,
            pointer: Some((200.0, 40.0)),
            ..base
        },
    ]
}
