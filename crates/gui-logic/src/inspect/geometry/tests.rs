use super::*;
use crate::inspect::lanes::{Lane, Span};
use crate::theme::Color;

/// A stand-in for a renderer's shaped width: 0.55 em a character,
/// semibold a touch wider — enough for every placement rule to bite.
fn measure(s: &str, px: f32, face: Face) -> f32 {
    let em = match face {
        Face::Regular => 0.55,
        Face::Semibold => 0.6,
    };
    s.chars().count() as f32 * px * em
}

struct Owned {
    window: (u32, u32),
    curves: Vec<Curve>,
    dead: Vec<Dead>,
    lanes: Vec<LaneRow>,
}

impl Owned {
    fn plot(&self) -> Plot<'_> {
        Plot {
            window: self.window,
            peak: 100.0,
            curves: &self.curves,
            dead: &self.dead,
            lanes: &self.lanes,
            total: false,
            word: "dps",
            plot_h: PLOT_H,
        }
    }
}

fn owned(window: (u32, u32), lanes: Vec<LaneRow>) -> Owned {
    Owned {
        window,
        curves: vec![Curve {
            name: String::new(),
            color: Color::WHITE,
            points: vec![10.0, 50.0, 100.0, 40.0],
            bucket_ms: 1000,
            ink: Ink::Area,
        }],
        dead: Vec::new(),
        lanes,
    }
}

fn span(at_ms: i64, dur_ms: i64) -> Span {
    Span {
        at_ms,
        dur_ms,
        label: "Heroism".into(),
        caster: Some("Vingsham".into()),
        color: Color::WHITE,
        whose: None,
        second: false,
    }
}

fn one_lane(spans: Vec<Span>) -> Vec<LaneRow> {
    vec![LaneRow {
        lane: Lane::Externals,
        spans,
    }]
}

/// Minutes while they fit; wider steps when they would crowd, finer ones
/// in a short zoom.
#[test]
fn the_axis_ticks_in_minutes_while_they_fit() {
    assert_eq!(
        ticks((0, 422_040), 438.0),
        (0..=7).map(|m| m * 60_000).collect::<Vec<_>>()
    );
    let long = ticks((0, 30 * 60_000), 300.0);
    assert_eq!(long.get(1), Some(&300_000), "every 5 minutes");
    let zoomed = ticks((61_000, 92_000), 300.0);
    assert_eq!(zoomed, vec![65_000, 70_000, 75_000, 80_000, 85_000, 90_000]);
}

/// The plot, the axis and every lane share one x: with lanes the plot
/// starts at the lanes' track, without them at the canvas's edge, and a
/// zoom maps its window across all of them.
#[test]
fn the_plot_and_the_lanes_share_one_x() {
    let o = owned(
        (0, 4_000),
        one_lane(vec![span(2_000, 1_000), span(3_000, 0)]),
    );
    let p = o.plot();
    let w = LEFT + 400.0;
    assert_eq!(p.x_of(0.0, w), LEFT, "the plot starts at the track");
    assert_eq!(p.x_of(2_000.0, w), LEFT + 200.0);
    let r = p.span_rect(0, &p.lanes[0].spans[0], w).unwrap();
    assert_eq!((r.x, r.w), (LEFT + 200.0, 100.0), "a span is its time");
    let tick = p.span_rect(0, &p.lanes[0].spans[1], w).unwrap();
    assert_eq!(tick.w, SPAN_MIN, "a moment is a 3 px tick");
    assert_eq!(p.ms_at(LEFT + 100.0, w), 1_000);
    let bare_o = owned((1_000, 3_000), Vec::new());
    let bare = bare_o.plot();
    assert_eq!(bare.x_of(1_000.0, 400.0), 0.0, "no lanes: the whole width");
    assert_eq!(bare.x_of(2_000.0, 400.0), 200.0, "a zoom fills the plot");
    assert_eq!(
        p.height(),
        PLOT_H + AXIS_GAP + AXIS_H + LANES_TOP + LANE_ROW,
        "one lane"
    );
    assert_eq!(bare.height(), PLOT_H + AXIS_GAP + AXIS_H, "no lanes");
}

/// Zoomed, a span that began before the window starts at the track's edge
/// — never over the lane labels — one that runs past it stops at the
/// canvas's, and one wholly outside is not drawn or hovered.
#[test]
fn a_zoom_holds_every_span_to_the_track() {
    let spans = vec![
        span(0, 2_000),
        span(2_500, 5_000),
        span(0, 500),
        span(500, 0),
        span(4_000, 1_000),
    ];
    let o = owned((1_000, 3_000), one_lane(spans));
    let p = o.plot();
    let w = LEFT + 400.0;
    let rect = |i: usize| p.span_rect(0, &p.lanes[0].spans[i], w);
    let early = rect(0).expect("it overlaps the window");
    assert_eq!((early.x, early.w), (LEFT, 200.0), "cut at the start");
    let late = rect(1).expect("it overlaps the window");
    assert_eq!(late.x + late.w, w, "cut at the end");
    assert_eq!(rect(2), None, "over before the window");
    assert_eq!(rect(3), None, "a moment before it");
    assert_eq!(rect(4), None, "after it");
    assert_eq!(
        p.hover_at(LABEL_W / 2.0, p.lane_y(0) + 6.0, w),
        None,
        "the label is no span"
    );
}

/// The pointer on a span names it (its caster, its time and length); on
/// the plot, the instant and the curve's value there.
#[test]
fn hovering_names_a_span_or_reads_the_curve() {
    let o = owned((0, 4_000), one_lane(vec![span(2_000, 1_000)]));
    let p = o.plot();
    let w = LEFT + 400.0;
    assert_eq!(
        p.hover_at(LEFT + 250.0, p.lane_y(0) + 6.0, w),
        Some(Hover::Span(0, 0))
    );
    let words = p.tip_words(Hover::Span(0, 0), w);
    assert_eq!(words[0].0, "Heroism");
    assert_eq!(words[1].0, "0:02, 1s, from Vingsham");
    let Some(Hover::Plot(x)) = p.hover_at(LEFT + 210.0, 40.0, w) else {
        panic!("the plot answers the pointer");
    };
    assert_eq!(x, LEFT + 250.0, "snapped to the bucket's middle");
    let words = p.tip_words(Hover::Plot(x), w);
    assert_eq!(words[0].0, "0:02");
    assert_eq!(words[1].0, "dps  100");
    assert_eq!(p.hover_at(10.0, 40.0, w), None, "the lane labels' column");
}

/// With lanes the gutter beside the plot is its scale — the peak level
/// with the peak, 0 on the baseline, both ending before the plot — in the
/// readable faint ink; without lanes there is no gutter and no scale.
#[test]
fn the_gutter_is_the_plot_s_scale() {
    let o = owned((0, 4_000), one_lane(vec![span(2_000, 1_000)]));
    let p = o.plot();
    let labels = p.labels(None, LEFT + 400.0, &measure);
    assert!(labels.iter().any(|l| l.words == "Externals"));
    assert!(labels.iter().any(|l| l.words == "0:00"));
    let scale = |words: &str| labels.iter().find(|l| l.words == words).cloned();
    let peak = scale("100").expect("the peak on the scale");
    let zero = scale("0").expect("0 on the scale");
    for l in [&peak, &zero] {
        assert!(
            (l.x + l.width - LABEL_W).abs() < 0.01,
            "right-aligned, a gap before the plot: {l:?}"
        );
    }
    let at_peak = PEAK_INSET;
    assert!(
        peak.rect().y <= at_peak && at_peak <= peak.rect().y + peak.rect().h,
        "level with the peak: {peak:?}"
    );
    assert!(
        (zero.rect().y + zero.rect().h - PLOT_H).abs() < 0.01,
        "on the baseline: {zero:?}"
    );
    assert!(labels.iter().all(|l| l.ink == Ink3::Quiet));
    let bare_o = owned((0, 4_000), Vec::new());
    assert!(
        bare_o
            .plot()
            .labels(None, 400.0, &measure)
            .iter()
            .all(|l| l.words != "100"),
        "no lanes, no gutter, no scale"
    );
}

/// A lane's row is as tall as its label's line, the 13 px track centred in
/// it and the label on the same line: rows 19.6 px apart.
#[test]
fn a_lane_row_is_its_label_s_line() {
    let o = owned(
        (0, 4_000),
        vec![
            LaneRow {
                lane: Lane::Cooldowns,
                spans: vec![span(0, 1_000)],
            },
            LaneRow {
                lane: Lane::Items,
                spans: vec![span(0, 0)],
            },
        ],
    );
    let p = o.plot();
    assert!((LANE_ROW - 15.6).abs() < 0.01);
    assert!((p.lane_y(1) - p.lane_y(0) - 19.6).abs() < 0.01);
    let inset = p.track_y(0) - p.lane_y(0);
    assert!((inset - 1.3).abs() < 0.01, "centred: {inset}");
    let r = p.span_rect(0, &p.lanes[0].spans[0], LEFT + 400.0).unwrap();
    assert_eq!(
        r.y,
        p.track_y(0) + SPAN_INSET,
        "the span inset in its track"
    );
    let label = p
        .labels(None, LEFT + 400.0, &measure)
        .into_iter()
        .find(|l| l.words == "Cooldowns")
        .unwrap();
    let (mid_label, mid_track) = (label.rect().center_y(), p.track_y(0) + LANE_H / 2.0);
    assert!((mid_label - mid_track).abs() < 0.01, "on the track's line");
}

/// A comparison's track is split: the first player's span over the
/// second's, each found by the pointer's half.
#[test]
fn a_pair_s_lanes_split_the_track_between_the_two() {
    let half = |second: bool, at_ms| Span {
        whose: Some(if second { "B" } else { "A" }.into()),
        second,
        ..span(at_ms, 1_000)
    };
    let o = owned(
        (0, 4_000),
        one_lane(vec![half(false, 1_000), half(true, 1_000)]),
    );
    let p = o.plot();
    assert!(p.split());
    let w = LEFT + 400.0;
    let (a, b) = (
        p.span_rect(0, &p.lanes[0].spans[0], w).unwrap(),
        p.span_rect(0, &p.lanes[0].spans[1], w).unwrap(),
    );
    assert_eq!(a.x, b.x, "one clock");
    assert!(a.y + a.h <= b.y, "A over B: {a:?} {b:?}");
    assert!(b.y + b.h <= p.track_y(0) + LANE_H, "inside the track");
    let x = a.x + a.w / 2.0;
    assert_eq!(p.hover_at(x, a.center_y(), w), Some(Hover::Span(0, 0)));
    assert_eq!(p.hover_at(x, b.center_y(), w), Some(Hover::Span(0, 1)));
    assert_eq!(
        p.tip_words(Hover::Span(0, 1), w)[1].0,
        "B, 0:01, 1s, from Vingsham"
    );
    assert!(!owned((0, 4_000), one_lane(vec![span(0, 1)])).plot().split());
}

/// A death's words stand in a band over the curves: the curves peak under
/// it, and a second death's words that would overlap the first's drop to a
/// second row — never printed over them.
#[test]
fn hatch_words_keep_a_band_over_the_curves() {
    let mut o = owned((0, 4_000), Vec::new());
    assert_eq!(
        Plot::top_of(&o.plot().hatch_labels(400.0, &measure)),
        PEAK_INSET
    );
    o.dead.push(Dead {
        at_ms: 1_000,
        end_ms: 4_000,
        words: "Tranqlock died 0:01".into(),
    });
    o.dead.push(Dead {
        at_ms: 1_200,
        end_ms: 4_000,
        words: "Swampert died 0:01".into(),
    });
    let p = o.plot();
    let words = p.hatch_labels(400.0, &measure);
    assert_eq!(words.len(), 2);
    assert!(!words[0].rect().overlaps(&words[1].rect()), "{words:?}");
    let top = Plot::top_of(&words);
    assert!(
        words.iter().all(|l| l.rect().y + l.rect().h < top),
        "the curves peak under every word: {top}"
    );
    assert_eq!(p.y_of(p.peak, top), top);
    assert_eq!(p.y_of(0.0, top), PLOT_H);
}

/// A drag of 3 px or more is a window; less is a click; backwards still
/// reads low to high.
#[test]
fn a_drag_is_a_window_and_a_wander_a_click() {
    let o = owned((0, 4_000), Vec::new());
    let p = o.plot();
    assert_eq!(p.drag_range(100.0, 200.0, 400.0), Some((1_000, 2_000)));
    assert_eq!(p.drag_range(200.0, 100.0, 400.0), Some((1_000, 2_000)));
    assert_eq!(p.drag_range(100.0, 101.0, 400.0), None);
    assert!(p.in_plot(10.0, PLOT_H) && !p.in_plot(10.0, PLOT_H + 1.0));
}

/// A tooltip over a lane-0 span lands on the axis's ticks; every tick it
/// covers is left out, never printed through it.
#[test]
fn a_tooltip_hides_the_labels_under_it() {
    let spans = (0..7).map(|m| span(m * 60_000, 20_000)).collect();
    let o = owned((0, 420_000), one_lane(spans));
    let p = o.plot();
    let w = LEFT + 438.0;
    let everything = p.labels(None, w, &measure);
    for i in 0..7 {
        let hover = Hover::Span(0, i);
        let tip = p.tip(hover, w, &measure).expect("a tooltip").rect;
        let shown = p.labels(Some(hover), w, &measure);
        assert!(
            shown.iter().all(|l| !l.rect().overlaps(&tip)),
            "span {i}: {shown:?} under {tip:?}"
        );
        let covered = everything
            .iter()
            .filter(|l| l.rect().overlaps(&tip))
            .count();
        assert_eq!(shown.len() + covered, everything.len(), "only those");
    }
    let tip = p.tip(Hover::Span(0, 3), w, &measure).unwrap().rect;
    assert!(
        everything
            .iter()
            .any(|l| l.words.contains(':') && l.rect().overlaps(&tip)),
        "the case is real: a lane-0 tooltip covers the ticks"
    );
}

/// The spline stays inside the plot and ends on every point; the stripes
/// fill a death's stretch and nothing outside it; the stack's bands sum.
#[test]
fn curves_bands_and_stripes_hold_their_shape() {
    let pts = [(0.0, 90.0), (10.0, 0.0), (20.0, 96.0), (30.0, 10.0)];
    let segs = smooth(&pts, PLOT_H);
    assert_eq!(segs.len(), 3);
    for (seg, p) in segs.iter().zip(pts.iter().skip(1)) {
        assert_eq!(seg[2], *p, "each segment ends on its point");
        for c in &seg[..2] {
            assert!((0.0..=PLOT_H).contains(&c.1), "held: {c:?}");
        }
    }
    assert!(smooth(&pts[..1], PLOT_H).is_empty());
    let s = stripes(100.0, 160.0, PLOT_H);
    assert!(!s.is_empty());
    for (a, b) in &s {
        assert!(a.0 >= 100.0 - 0.01 && b.0 <= 160.0 + 0.01, "{a:?} {b:?}");
    }
    assert!(stripes(100.0, 100.0, PLOT_H).is_empty());

    let mut o = owned((0, 3_000), Vec::new());
    o.curves = vec![
        Curve {
            name: "A".into(),
            color: Color::WHITE,
            points: vec![1.0, 2.0, 3.0],
            bucket_ms: 1000,
            ink: Ink::Stack,
        },
        Curve {
            name: "B".into(),
            color: Color::WHITE,
            points: vec![4.0, 4.0, 4.0],
            bucket_ms: 1000,
            ink: Ink::Stack,
        },
        Curve {
            name: String::new(),
            color: Color::WHITE,
            points: vec![9.0, 9.0, 9.0],
            bucket_ms: 1000,
            ink: Ink::Ghost,
        },
    ];
    let p = o.plot();
    let bands = p.bands(400.0, PEAK_INSET);
    assert_eq!(bands.len(), 2);
    assert!(
        bands[0].lower.is_empty(),
        "the first band stands on the floor"
    );
    assert_eq!(bands[1].lower, bands[0].upper, "each on the one under it");
    assert_eq!(p.lines().len(), 1, "a ghost is a line; bands are not");
}

/// The shared samples rest the pointer where their names say: on the
/// plot, or on Power Infusion inside the Heroism on the Externals lane.
#[test]
fn the_samples_point_where_they_say() {
    for s in samples::all() {
        let g = Plot {
            window: s.window,
            peak: s.peak,
            curves: &s.curves,
            dead: &s.dead,
            lanes: &s.lanes,
            total: s.total,
            word: s.word,
            plot_h: PLOT_H,
        };
        let hover = s
            .pointer
            .and_then(|(x, y)| g.hover_at(x, y, samples::WIDTH));
        match s.name {
            "hover-plot" | "total" => {
                assert!(matches!(hover, Some(Hover::Plot(_))), "{}", s.name)
            }
            "hover-span" => {
                let Some(Hover::Span(lane, i)) = hover else {
                    panic!("hover-span hovers {hover:?}");
                };
                assert_eq!(s.lanes[lane].spans[i].label, "Power Infusion");
            }
            _ => assert_eq!(hover, None, "{}", s.name),
        }
        assert!(s.peak > 0.0 && g.height() >= PLOT_H, "{}", s.name);
    }
}
