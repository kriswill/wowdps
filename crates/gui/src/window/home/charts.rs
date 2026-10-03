//! Home's three drawings (the iced window's `home/charts.rs`), each a
//! canvas scaled from the prototype's own SVG `viewBox` so its proportions
//! hold at any width — its type with them, but never smaller than its own
//! size (gui-logic's `chart::type_scale`): the rank across the night
//! (`300 × 140`), the key throughput dots (`420 × 170`), and a key's run
//! against its timers (`.par`). The geometry is gui-logic's `home::chart`.
//!
//! A dot on either chart is a jump point, as the tile or row it stands for
//! is: over each stands an element of its own the size of the press
//! radius, which takes the press, wears the pointer and says under it
//! which pull it is — so a press is GPUI's, and a test clicks a dot by id.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, Context, Font, FontWeight, Hsla, MouseButton, PathBuilder, Pixels,
    Point, SharedString, TestSupportExt as _, TextAlign, TextRun, Window, canvas, div, fill, font,
    point, px, size,
};
use wowdps_gui_logic::fight_head::ordinal;
use wowdps_gui_logic::home::chart::{
    AXIS_DROP, AXIS_PX, BEST_RING, BEST_RISE, CAP_OF_TYPE, DAY_FOOT, DAY_PX, Dot, GRID_END,
    GRID_LABEL_X, GRID_X, GUIDE_END, GUIDE_FOOT, GUIDE_FOOT_WORDS, GUIDE_PX, GUIDE_TOP,
    GUIDE_TOP_WORDS, GUIDE_X, HIT, MID_OF_TYPE, RANK_PX, RANK_RISE, SLOPE_H, SLOPE_LINE, SLOPE_R,
    SLOPE_W, TREND_H, TREND_R, TREND_W, TrendAxis, rank_under, slope_dots, slope_labelled,
    type_scale,
};
use wowdps_gui_logic::home::{
    NightPull, TrendPoint, axis_label, best_label, day_labels, hollow, par_x,
};
use wowdps_gui_logic::labels::shown_name;
use wowdps_gui_logic::rail::{Pull, weekday};
use wowdps_gui_logic::theme::WindowTokens;
use wowdps_model::fmt::human;

use super::super::Gui;
use super::super::paint::{fill_disc, stroke_ring};
use super::super::w::{REGULAR, SEMIBOLD, W};
use crate::theme::hsla;

/// A painted word's line box, of its size: the middle of a lower-case
/// line stands at its middle.
const LINE: f32 = 1.3;

/// Where a painted word stands on its baseline point.
#[derive(Debug, Clone, Copy)]
enum Anchor {
    Center,
    Right,
}

/// A word on a chart, its baseline at (`x`, `y`) in the chart's own
/// pixels at zoom 1, as the SVG sets it.
struct Word {
    s: String,
    x: f32,
    y: f32,
    px: f32,
    color: Hsla,
    weight: FontWeight,
    anchor: Anchor,
}

impl Word {
    fn paint(&self, o: Point<Pixels>, z: f32, ui: &'static str, window: &mut Window, cx: &mut App) {
        let run = TextRun {
            len: self.s.len(),
            font: Font {
                weight: self.weight,
                ..font(ui)
            },
            color: self.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            SharedString::from(self.s.clone()),
            px(self.px * z),
            &[run],
            None,
        );
        let width = f32::from(line.width);
        let x = self.x * z
            - match self.anchor {
                Anchor::Center => width / 2.0,
                Anchor::Right => width,
            };
        let mid = (self.y - self.px * MID_OF_TYPE) * z;
        let top = mid - self.px * LINE * z / 2.0;
        let _ = line.paint(
            point(o.x + px(x), o.y + px(top)),
            px(self.px * LINE * z),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

/// A straight stroke from `a` to `b`.
fn rule(window: &mut Window, a: Point<Pixels>, b: Point<Pixels>, width: Pixels, color: Hsla) {
    let mut p = PathBuilder::stroke(width);
    p.move_to(a);
    p.line_to(b);
    if let Ok(p) = p.build() {
        window.paint_path(p, color);
    }
}

/// The press targets over a chart's dots: each `HIT` round its dot, the
/// pointer's, a press opening its pull, `tip` under the pointer.
fn targets(
    dots: &[Dot],
    tips: Vec<String>,
    id: &'static str,
    w: &W,
    cx: &mut Context<Gui>,
) -> Vec<AnyElement> {
    dots.iter()
        .zip(tips)
        .enumerate()
        .map(|(i, (((x, y), fight), tip))| {
            let fight = fight.clone();
            let tip = SharedString::from(tip);
            div()
                .id((id, i))
                .test_support()
                .absolute()
                .left(w.z(x - HIT))
                .top(w.z(y - HIT))
                .size(w.z(2.0 * HIT))
                .rounded_full()
                .cursor_pointer()
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.go_pull(Pull::Stored(fight.clone()), cx);
                        cx.stop_propagation();
                    }),
                )
                .into_any_element()
        })
        .collect()
}

// ---- the night's rank --------------------------------------------------------

/// "Your rank, pull by pull" `cw` wide: each pull's place in the role as a
/// height — the top of the role at the top, the bottom at the foot, so
/// 17th of 19 and 3rd of 5 share one scale — joined in the character's
/// colour. A kill or a timed key is a filled dot, a wipe or a key over
/// time a ring; each carries its rank above it (under it where above would
/// print over the guide's words), all of them while there is room.
pub fn rank_slope(
    pulls: &[NightPull],
    color: Hsla,
    cw: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let ch = cw * SLOPE_H / SLOPE_W;
    let dots = slope_dots(pulls, cw);
    let labelled = slope_labelled(pulls, cw);
    let tips = pulls
        .iter()
        .map(|p| {
            format!(
                "{}, {} of {}",
                p.name,
                ordinal(p.standing.place),
                p.standing.of
            )
        })
        .collect();
    let hits = targets(&dots, tips, "slope-dot", w, cx);
    let (t, z, ui) = (w.t, w.zoom, w.ui);
    let marks: Vec<(bool, String)> = pulls
        .iter()
        .map(|p| (hollow(p.mark), ordinal(p.standing.place)))
        .collect();
    let drawn = dots.clone();
    let chart = canvas(
        |_, _, _| {},
        move |b, (), window, cx| {
            paint_slope(
                &drawn, &marks, &labelled, color, cw, t, z, ui, b, window, cx,
            );
        },
    )
    .absolute()
    .inset_0();
    div()
        .id("rank-slope")
        .relative()
        .flex_none()
        .w(w.z(cw))
        .h(w.z(ch))
        .child(chart)
        .children(hits)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn paint_slope(
    dots: &[Dot],
    marks: &[(bool, String)],
    labelled: &[bool],
    color: Hsla,
    cw: f32,
    t: WindowTokens,
    z: f32,
    ui: &'static str,
    b: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let o = b.origin;
    let at = |x: f32, y: f32| point(o.x + px(x * z), o.y + px(y * z));
    let s = cw / SLOPE_W;
    let ts = type_scale(s);
    let (x1, x2) = (GUIDE_X * s, (SLOPE_W - GUIDE_END) * s);
    for y in [GUIDE_TOP, SLOPE_H - GUIDE_FOOT] {
        let y = ((y * s * z).round() + 0.5) / z;
        rule(window, at(x1, y), at(x2, y), px(z), hsla(t.line));
    }
    let guide = |s_: &str, y: f32| Word {
        s: s_.to_string(),
        x: x2,
        y,
        px: GUIDE_PX * ts,
        color: hsla(t.ink_3_text),
        weight: REGULAR,
        anchor: Anchor::Right,
    };
    guide("top of the role", GUIDE_TOP_WORDS * s).paint(o, z, ui, window, cx);
    guide("bottom", (SLOPE_H - GUIDE_FOOT_WORDS) * s).paint(o, z, ui, window, cx);
    // The line under the dots: its joins stand under them, so it needs no
    // round joins of its own.
    for (((ax, ay), _), ((bx, by), _)) in dots.iter().zip(dots.iter().skip(1)) {
        rule(
            window,
            at(*ax, *ay),
            at(*bx, *by),
            px(SLOPE_LINE * s * z),
            color,
        );
    }
    for (((xy, _), (hollow, rank)), label) in dots.iter().zip(marks).zip(labelled) {
        let c = at(xy.0, xy.1);
        let r = px(SLOPE_R * s * z);
        fill_disc(window, c, r, if *hollow { hsla(t.surface) } else { color });
        stroke_ring(window, c, r, px(SLOPE_LINE * s * z), color);
        if *label {
            // Above its dot, on its baseline; under it, hanging from its
            // cap, where above would print over the guide's words.
            let y = if rank_under(*xy, cw) {
                xy.1 + RANK_RISE * s + RANK_PX * ts * CAP_OF_TYPE
            } else {
                xy.1 - RANK_RISE * s
            };
            Word {
                s: rank.clone(),
                x: xy.0,
                y,
                px: RANK_PX * ts,
                color: hsla(t.ink),
                weight: REGULAR,
                anchor: Anchor::Center,
            }
            .paint(o, z, ui, window, cx);
        }
    }
}

// ---- the week's key throughput ----------------------------------------------

/// "Effective dps on keys" `cw` wide: a dot per run in its character's
/// colour over a round value axis, each character's best ringed in
/// legendary orange with its figure, the day under the first run of each.
pub fn trend(
    points: &[TrendPoint],
    unknown: Hsla,
    hide_realms: bool,
    cw: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let axis = TrendAxis::of(points);
    let ch = cw * TREND_H / TREND_W;
    let dots = axis.dots(points, cw);
    let tips = points
        .iter()
        .map(|p| {
            format!(
                "{}, {} on {}",
                shown_name(&p.who.name, hide_realms),
                human(p.value.round().max(0.0) as u64),
                weekday(p.day)
            )
        })
        .collect();
    let hits = targets(&dots, tips, "trend-dot", w, cx);
    let (t, z, ui) = (w.t, w.zoom, w.ui);
    let fills: Vec<(Hsla, bool, f64, i64)> = points
        .iter()
        .map(|p| {
            (
                p.who.class.map_or(unknown, |c| {
                    hsla(wowdps_gui_logic::theme::Color::of_class(c))
                }),
                p.best,
                p.value,
                p.day,
            )
        })
        .collect();
    let days = day_labels(points);
    let drawn = dots.clone();
    let chart = canvas(
        |_, _, _| {},
        move |b, (), window, cx| {
            let o = b.origin;
            let at = |x: f32, y: f32| point(o.x + px(x * z), o.y + px(y * z));
            let s = cw / TREND_W;
            let ts = type_scale(s);
            for v in &axis.ticks {
                let y = ((axis.y_of(*v, s) * z).round() + 0.5) / z;
                rule(
                    window,
                    at(GRID_X * s, y),
                    at((TREND_W - GRID_END) * s, y),
                    px(z),
                    hsla(t.line),
                );
                Word {
                    s: axis_label(*v),
                    x: GRID_LABEL_X * s,
                    y: y + AXIS_DROP * s,
                    px: AXIS_PX * ts,
                    color: hsla(t.ink_3_text),
                    weight: REGULAR,
                    anchor: Anchor::Right,
                }
                .paint(o, z, ui, window, cx);
            }
            for (((xy, _), (color, best, value, day)), first) in drawn.iter().zip(&fills).zip(&days)
            {
                let c = at(xy.0, xy.1);
                let r = px(TREND_R * s * z);
                fill_disc(window, c, r, *color);
                if *best {
                    stroke_ring(window, c, r, px(BEST_RING * s * z), hsla(t.legendary));
                    Word {
                        s: best_label(*value),
                        x: xy.0,
                        y: xy.1 - BEST_RISE * s,
                        px: AXIS_PX * ts,
                        color: hsla(t.legendary),
                        weight: SEMIBOLD,
                        anchor: Anchor::Center,
                    }
                    .paint(o, z, ui, window, cx);
                }
                if *first {
                    Word {
                        s: weekday(*day).to_string(),
                        x: xy.0,
                        y: (TREND_H - DAY_FOOT) * s,
                        px: DAY_PX * ts,
                        color: hsla(t.ink_3_text),
                        weight: REGULAR,
                        anchor: Anchor::Center,
                    }
                    .paint(o, z, ui, window, cx);
                }
            }
        },
    )
    .absolute()
    .inset_0();
    div()
        .id("trend")
        .relative()
        .flex_none()
        .w(w.z(cw))
        .h(w.z(ch))
        .child(chart)
        .children(hits)
        .into_any_element()
}

// ---- a key against its timers -------------------------------------------------

/// The par bar's box: 8 px of track with its round ends (`.par{height:8px;
/// border-radius:4px}`) and the ticks 3 px proud of it either side (`.tk{
/// top:-3px;bottom:-3px}`); the run's fill over its track at `opacity:.8`.
const PAR_TRACK_H: f32 = 8.0;
const PAR_PROUD: f32 = 3.0;
const PAR_H: f32 = PAR_TRACK_H + 2.0 * PAR_PROUD;
const PAR_FILL_ALPHA: f32 = 0.8;

/// A key's run against its timers (`.par`), `bw` wide: the track a quarter
/// longer than the timer, the run's time filled in green when it was timed
/// and in red when it went over, and a tick at +3, +2 and the timer.
pub fn par_bar(
    clock_ms: i64,
    pars: (i64, i64, i64),
    timed: bool,
    bw: f32,
    w: &W,
) -> impl IntoElement {
    let (t, z, shape) = (w.t, w.zoom, w.shape);
    canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let o = b.origin;
            let r = shape.pill(PAR_TRACK_H / 2.0);
            let x_of = |ms: i64| par_x(pars.0, ms, bw);
            let track = |width: f32| {
                Bounds::new(
                    point(o.x, o.y + px(PAR_PROUD * z)),
                    size(px(width * z), px(PAR_TRACK_H * z)),
                )
            };
            window.paint_quad(fill(track(bw), hsla(t.par_track)).corner_radii(px(r * z)));
            let run = x_of(clock_ms);
            if run > 0.0 {
                let color = if timed { t.par_timed } else { t.par_over };
                window.paint_quad(
                    fill(track(run), hsla(color.alpha(PAR_FILL_ALPHA)))
                        .corner_radii(px(r.min(run / 2.0) * z)),
                );
            }
            for ms in [pars.2, pars.1, pars.0] {
                let x = ((x_of(ms) * z).round() + 0.5) / z;
                rule(
                    window,
                    point(o.x + px(x * z), o.y),
                    point(o.x + px(x * z), o.y + px(PAR_H * z)),
                    px(z),
                    hsla(t.ink_3),
                );
            }
        },
    )
    .flex_none()
    .w(w.z(bw))
    .h(w.z(PAR_H))
}
