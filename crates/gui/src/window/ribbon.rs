//! The ribbon (R25; the prototype's `.ribbon`, the iced window's
//! `ribbon.rs`): the pull's signature between the stat line and the view
//! tabs. One canvas: the whole group's rate for the view on screen as an
//! area of the ink fading from 26 % to 2 % under a line of it, its peak
//! named at the top-left, the minute ticks under it, the lust windows as a
//! faint wash, and a skull at every death in the dead player's class colour
//! on a faint hairline (the theme's `death_line`), the owner's labelled
//! "you". A skull is a press away from its recap; anywhere else on the plot
//! the pointer reads the time and the rate under the accent's crosshair —
//! and, inside a lust window, its name: a word on the ribbon either hid the
//! curve's crest or was struck out by it.
//!
//! GPUI paints a canvas's text and shapes in the order written (spike S8),
//! so the words sit over the curve with no tricks; the plates under the
//! peak's words and "you" are kept, as the iced ribbon's are, so no
//! hairline runs through a glyph. What the ribbon says — its span, its
//! rates, its words — is gui-logic's (`ribbon`, `axis`).
//!
//! The delight: the crosshair is a soft gold glow, not a hairline alone,
//! and it rides the curve with a dot where the rate is read.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, Font, FontWeight, Hsla, MouseButton, MouseMoveEvent, PathBuilder, Pixels,
    Point, SharedString, TestSupportExt as _, TextAlign, TextRun, Window, canvas, div, fill, font,
    linear_color_stop, linear_gradient, point, px, size,
};
use wowdps_gui_logic::axis::minute_ticks;
use wowdps_gui_logic::deaths::{Pick, is_mine, selected};
use wowdps_gui_logic::labels::display_name;
use wowdps_gui_logic::ribbon::{peak_words, rates, skull_words, span_of, word};
use wowdps_model::fmt::{commas, duration};
use wowdps_model::{Class, LustWindow, View};

use super::Gui;
use super::paint::fill_disc;
use super::w::{REGULAR, SEMIBOLD, W};
use crate::theme::hsla;

/// The ribbon's height (`.ribbon{height:86px}`, 74 at 820 px and under),
/// its 1 px top rule included, and its side margins.
const H: f32 = 86.0;
const H_NARROW: f32 = 74.0;
const MARGIN: f32 = 18.0;
const MARGIN_NARROW: f32 = 12.0;
/// The plot (`.plot{top:8px;height:58px}`, 46 narrow), under the rule.
const PLOT_TOP: f32 = 1.0 + 8.0;
const PLOT_H: f32 = 58.0;
const PLOT_H_NARROW: f32 = 46.0;
/// The axis (`.axis{bottom:2px;height:16px}`).
const AXIS_BOTTOM: f32 = 2.0;
const AXIS_H: f32 = 16.0;
/// The peak stands this far under the plot's top, and its words this far
/// in.
const PEAK_INSET: f32 = 4.0;
const PEAK_X: f32 = 4.0;
/// The curve's area, top and foot alphas, and its line.
const AREA_TOP: f32 = 0.26;
const AREA_FOOT: f32 = 0.02;
const LINE_ALPHA: f32 = 0.8;
const LINE_W: f32 = 1.4;
/// A lust window's wash and edge (its name is the tooltip's, under the
/// pointer).
const BAND_ALPHA: f32 = 0.05;
const BAND_EDGE_ALPHA: f32 = 0.18;
/// A death: its box, the reach a press has, the skull, the outline an
/// enemy's wears, how far past the plot's foot it stands, and its hairline.
const SKULL_BOX: f32 = 16.0;
const SKULL_REACH: f32 = 12.0;
const SKULL: f32 = 13.0;
const SKULL_OUTLINE: f32 = 1.2;
const SKULL_DROP: f32 = 3.0;
const MARK_LINE_GAP: f32 = 14.0;
const MARK_LINE_H: f32 = 48.0;
const MARK_LINE_H_NARROW: f32 = 36.0;
const MARK_LINE_ALPHA: f32 = 0.28;
const MARK_LINE_ON_ALPHA: f32 = 0.8;
const YOU_PX: f32 = 11.0;
const YOU_ABOVE: f32 = 50.0;
const YOU_ABOVE_NARROW: f32 = 38.0;
/// A hovered or open skull's glow: its outline stroked wide and faint,
/// then narrower and stronger, under the solid skull.
const GLOW: [(f32, f32); 3] = [(6.0, 0.06), (4.0, 0.12), (2.0, 0.25)];
/// A word's plate, and the crosshair.
const PLATE_PAD: f32 = 3.0;
const PLATE_RADIUS: f32 = 3.0;
const XHAIR_ALPHA: f32 = 0.55;
/// The crosshair's glow: wider, fainter strokes of the gold under it.
const XHAIR_GLOW: [(f32, f32); 2] = [(7.0, 0.07), (3.0, 0.16)];
/// The tooltip (`.tip{font-size:13px;padding:5px 8px;border-radius:6px}`).
const TIP_PX: f32 = 13.0;
const TIP_PAD: (f32, f32) = (8.0, 5.0);
const TIP_LINE: f32 = 17.0;
const TIP_RADIUS: f32 = 6.0;
const TIP_OFF_X: f32 = 12.0;
const TIP_OFF_Y: f32 = 4.0;
/// A one-line text box's height for its size.
const LINE: f32 = 1.3;
const TICK_FIRST: f32 = 0.001;
const TICK_LAST: f32 = 0.96;

/// Everything the ribbon draws, owned.
#[derive(Debug, Clone, PartialEq)]
pub struct Ribbon {
    /// What the curve is: "Raid dps".
    pub word: String,
    /// The raid's rate, one value per `step_ms` from the fight's start.
    pub rate: Vec<f64>,
    pub step_ms: u32,
    pub span_ms: u32,
    pub skulls: Vec<Skull>,
    pub lust: Vec<LustWindow>,
}

/// One death on the ribbon.
#[derive(Debug, Clone, PartialEq)]
pub struct Skull {
    pub at_ms: i64,
    pub pick: Pick,
    pub name: String,
    pub class: Option<Class>,
    /// "died 1:10, Venom Rupture".
    pub words: String,
    pub mine: bool,
    pub enemy: bool,
    /// The death the Deaths view has open: it glows.
    pub on: bool,
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hover {
    /// The plot, at this x (canvas px at zoom).
    Plot(f32),
    /// A skull, by its place.
    Skull(usize),
}

impl Ribbon {
    /// The ribbon for the pull on the stage — `None` without a raid
    /// timeline.
    pub fn of(gui: &Gui, cx: &App) -> Option<Self> {
        let app = gui.fight(cx);
        let raid = app.raid()?;
        let hide = gui.cfg.hide_realms;
        let open = (app.view == View::Deaths)
            .then(|| selected(app, raid))
            .flatten();
        let owner = gui.death_owner(raid);
        let span = span_of(raid, app.duration_ms());
        let (step_ms, rate) = rates(&raid.series, raid.bucket_ms.max(1), span);
        let skulls = raid
            .deaths
            .iter()
            .enumerate()
            .map(|(i, d)| Skull {
                at_ms: d.at_ms,
                pick: Pick::of(d),
                name: if hide {
                    display_name(&d.name).to_string()
                } else {
                    d.name.clone()
                },
                class: d.class,
                words: skull_words(d, hide),
                mine: is_mine(d, owner.as_deref()),
                enemy: d.enemy,
                on: open == Some(i),
            })
            .collect();
        Some(Ribbon {
            word: word(raid),
            rate,
            step_ms,
            span_ms: span as u32,
            skulls,
            lust: raid.lust.clone(),
        })
    }

    pub fn peak(&self) -> f64 {
        self.rate.iter().copied().fold(0.0, f64::max)
    }
}

/// The geometry at one width: everything in canvas px at the window's
/// zoom, `z` its scale.
#[derive(Clone, Copy)]
struct Geo<'a> {
    r: &'a Ribbon,
    z: f32,
    w: f32,
    narrow: bool,
}

impl Geo<'_> {
    fn plot_h(&self) -> f32 {
        self.z * if self.narrow { PLOT_H_NARROW } else { PLOT_H }
    }

    fn top(&self) -> f32 {
        self.z * PLOT_TOP
    }

    fn foot(&self) -> f32 {
        self.top() + self.plot_h()
    }

    fn height(&self) -> f32 {
        self.z * if self.narrow { H_NARROW } else { H }
    }

    fn x_of(&self, ms: f64) -> f32 {
        (ms / f64::from(self.r.span_ms.max(1))) as f32 * self.w
    }

    fn ms_at(&self, x: f32) -> u32 {
        let frac = (x / self.w.max(1.0)).clamp(0.0, 1.0);
        (f64::from(frac) * f64::from(self.r.span_ms)) as u32
    }

    fn y_of(&self, v: f64) -> f32 {
        let peak = self.r.peak();
        if peak <= 0.0 {
            return self.foot();
        }
        self.foot() - (v / peak).clamp(0.0, 1.0) as f32 * (self.plot_h() - self.z * PEAK_INSET)
    }

    /// A skull's box: centred on its moment, standing on the plot's foot.
    fn skull_box(&self, s: &Skull) -> (f32, f32, f32) {
        let side = self.z * SKULL_BOX;
        let x = self.x_of(s.at_ms as f64);
        let bottom = self.foot() + self.z * SKULL_DROP;
        (x - side / 2.0, bottom - side, side)
    }

    /// What the pointer at `(x, y)` is over: the nearest skull whose 24 px
    /// target holds it, the latest drawn of equals, else the plot.
    fn hover_at(&self, x: f32, y: f32) -> Option<Hover> {
        let reach = self.z * SKULL_REACH;
        let near = self
            .r
            .skulls
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let (bx, by, side) = self.skull_box(s);
                let (cx, cy) = (bx + side / 2.0, by + side / 2.0);
                let (dx, dy) = ((x - cx).abs(), (y - cy).abs());
                (dx <= reach && dy <= reach).then_some((i, dx + dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        if let Some((i, _)) = near {
            return Some(Hover::Skull(i));
        }
        let inside = y >= self.top() && y <= self.foot() && x >= 0.0 && x <= self.w;
        inside.then_some(Hover::Plot(x))
    }

    /// The curve's points, as the prototype's `curve()` lays them.
    fn points(&self) -> Vec<(f32, f32)> {
        let step = f64::from(self.r.step_ms.max(1));
        let span = f64::from(self.r.span_ms.max(1));
        let mut pts: Vec<(f32, f32)> = self
            .r
            .rate
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let ms = ((i as f64 + 0.5) * step).min(span);
                (self.x_of(ms), self.y_of(*v))
            })
            .collect();
        if let Some(first) = pts.first().copied() {
            pts.insert(0, (0.0, first.1));
        }
        let reach = self.r.rate.len() as f64 * step;
        if let Some(last) = pts.last().copied()
            && reach >= span - 5_000.0
        {
            pts.push((self.w, last.1));
        }
        pts
    }

    /// The rate under the crosshair at `x`.
    fn rate_at(&self, x: f32) -> f64 {
        let ms = self.ms_at(x);
        self.r
            .rate
            .get((ms / self.r.step_ms.max(1)) as usize)
            .copied()
            .unwrap_or(0.0)
    }
}

/// One line of canvas text, placed and measured.
struct Label {
    words: SharedString,
    x: f32,
    y: f32,
    px: f32,
    color: Hsla,
    weight: FontWeight,
    width: f32,
    /// Its plate is drawn, so no line runs through it.
    cover: bool,
}

impl Label {
    fn rect(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.width, self.px * LINE)
    }

    fn plate(&self, z: f32) -> (f32, f32, f32, f32) {
        let (x, y, w, h) = self.rect();
        (x - z * PLATE_PAD, y, w + 2.0 * z * PLATE_PAD, h)
    }
}

fn overlaps(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// The ribbon, laid out at the window's breakpoint. The pointer's place is
/// the window's (`Gui::ribbon_hover`); the canvas's bounds at the last
/// paint (`seen`) turn a move into a place on it.
pub fn view(
    ribbon: Ribbon,
    hover: Option<Hover>,
    seen: Rc<Cell<Bounds<Pixels>>>,
    w: &W,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let narrow = w.narrow();
    let (h, margin) = if narrow {
        (H_NARROW, MARGIN_NARROW)
    } else {
        (H, MARGIN)
    };
    let z = w.zoom;
    let look = w.clone();
    let ribbon = Rc::new(ribbon);
    let painted = Rc::clone(&seen);
    let drawn = Rc::clone(&ribbon);
    let plot = canvas(
        move |b, _, _| {
            painted.set(b);
        },
        move |b, (), window, cx| {
            let geo = Geo {
                r: &drawn,
                z,
                w: f32::from(b.size.width),
                narrow,
            };
            paint(&geo, hover, b, &look, window, cx);
        },
    )
    .size_full();
    let on_move = {
        let seen = Rc::clone(&seen);
        let r = Rc::clone(&ribbon);
        cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
            let b = seen.get();
            let geo = Geo {
                r: &r,
                z,
                w: f32::from(b.size.width),
                narrow,
            };
            let x = f32::from(e.position.x - b.origin.x);
            let y = f32::from(e.position.y - b.origin.y);
            let now = geo.hover_at(x, y);
            if this.ribbon_hover != now {
                this.ribbon_hover = now;
                cx.notify();
            }
        })
    };
    let on_press = {
        let seen = Rc::clone(&seen);
        let r = Rc::clone(&ribbon);
        cx.listener(move |this, e: &gpui_kit::MouseDownEvent, window, cx| {
            let b = seen.get();
            let geo = Geo {
                r: &r,
                z,
                w: f32::from(b.size.width),
                narrow,
            };
            let x = f32::from(e.position.x - b.origin.x);
            let y = f32::from(e.position.y - b.origin.y);
            if let Some(Hover::Skull(i)) = geo.hover_at(x, y)
                && let Some(s) = r.skulls.get(i)
            {
                this.open_death(s.pick.clone(), window, cx);
            }
        })
    };
    div()
        .id("ribbon")
        .flex_none()
        .w_full()
        .px(w.z(margin))
        .child(
            div()
                .id("ribbon-plot")
                .test_support()
                .w_full()
                .h(w.z(h))
                .cursor(match hover {
                    Some(Hover::Skull(_)) => gpui_kit::CursorStyle::PointingHand,
                    _ => gpui_kit::CursorStyle::Arrow,
                })
                .on_mouse_move(on_move)
                .on_mouse_down(MouseButton::Left, on_press)
                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                    if !*over && this.ribbon_hover.is_some() {
                        this.ribbon_hover = None;
                        cx.notify();
                    }
                }))
                .child(plot),
        )
}

#[allow(clippy::too_many_arguments)]
fn paint(
    geo: &Geo<'_>,
    hover: Option<Hover>,
    b: Bounds<Pixels>,
    look: &W,
    window: &mut Window,
    cx: &mut App,
) {
    let (t, ui) = (look.t, &look.ui);
    let z = geo.z;
    let w = geo.w;
    let at = |x: f32, y: f32| point(b.origin.x + px(x), b.origin.y + px(y));
    let (top, foot) = (geo.top(), geo.foot());
    let ink = hsla(t.ink);
    let stroke =
        |window: &mut Window, from: (f32, f32), to: (f32, f32), width: f32, color: Hsla| {
            let mut p = PathBuilder::stroke(px(width));
            p.move_to(at(from.0, from.1));
            p.line_to(at(to.0, to.1));
            if let Ok(path) = p.build() {
                window.paint_path(path, color);
            }
        };

    // The rule over the ribbon.
    stroke(window, (0.0, 0.5), (w, 0.5), z, hsla(t.line));

    // The chapter ring, where the theme draws one: a fine tick up from the
    // foot every ten seconds (the buckets' grid), a longer one each
    // minute — thinned to every 30 s or minute when they would crowd.
    if look.fx.fine_ticks {
        chapter_ring(geo, foot, &stroke, hsla(t.dial), window);
    }

    // The lust windows: a faint wash of the ink, its left edge a rule.
    for l in &geo.r.lust {
        let x1 = geo.x_of(l.at_ms as f64);
        let x2 = geo.x_of((l.at_ms + l.dur_ms) as f64).min(w);
        if x2 <= x1 {
            continue;
        }
        window.paint_quad(fill(
            Bounds::new(at(x1, top), size(px(x2 - x1), px(geo.plot_h()))),
            ink.opacity(BAND_ALPHA),
        ));
        stroke(
            window,
            (x1 + 0.5, top),
            (x1 + 0.5, foot),
            z,
            ink.opacity(BAND_EDGE_ALPHA),
        );
    }

    // The curve: an area fading down its height, under the line.
    let pts = geo.points();
    if let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) {
        let mut area = PathBuilder::fill();
        area.move_to(at(first.0, foot));
        area.line_to(at(first.0, first.1));
        smooth(&mut area, &pts, top, foot, &at);
        area.line_to(at(last.0, foot));
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(
                path,
                linear_gradient(
                    180.,
                    linear_color_stop(ink.opacity(AREA_TOP), 0.),
                    linear_color_stop(ink.opacity(AREA_FOOT), 1.),
                ),
            );
        }
        let mut line = PathBuilder::stroke(px(LINE_W * z));
        line.move_to(at(first.0, first.1));
        smooth(&mut line, &pts, top, foot, &at);
        if let Ok(path) = line.build() {
            window.paint_path(path, ink.opacity(LINE_ALPHA));
        }
    }

    // The deaths: a faint red hairline up the plot, the skull on the foot
    // in the dead player's colour — glowing when hovered or open; an
    // enemy's in outline.
    let death_line = hsla(t.death_line);
    let line_h = z * if geo.narrow {
        MARK_LINE_H_NARROW
    } else {
        MARK_LINE_H
    };
    let hovered = match hover {
        Some(Hover::Skull(i)) => Some(i),
        _ => None,
    };
    let scale = z * SKULL / 12.0;
    for (i, s) in geo.r.skulls.iter().enumerate() {
        let (bx, by, side) = geo.skull_box(s);
        let cx_ = (bx + side / 2.0).round() + 0.5;
        let low = by + side - z * MARK_LINE_GAP;
        let alpha = if s.on {
            MARK_LINE_ON_ALPHA
        } else {
            MARK_LINE_ALPHA
        };
        stroke(
            window,
            (cx_, (low - line_h).max(top)),
            (cx_, low),
            z,
            death_line.opacity(alpha),
        );
        let color = hsla(
            s.class
                .map_or(t.classless, wowdps_gui_logic::theme::Color::of_class),
        );
        let origin = (
            bx + side / 2.0 - z * SKULL / 2.0,
            by + side / 2.0 - z * SKULL / 2.0,
        );
        if s.on || hovered == Some(i) {
            for (width, a) in GLOW {
                let mut p = PathBuilder::stroke(px(width * z));
                skull(&mut p, origin, scale, false, &at);
                if let Ok(path) = p.build() {
                    window.paint_path(path, color.opacity(a));
                }
            }
        }
        if s.enemy {
            let mut p = PathBuilder::stroke(px(SKULL_OUTLINE * z));
            skull(&mut p, origin, scale, true, &at);
            if let Ok(path) = p.build() {
                window.paint_path(path, color);
            }
        } else {
            let mut p = PathBuilder::fill();
            skull(&mut p, origin, scale, false, &at);
            if let Ok(path) = p.build() {
                window.paint_path(path, color);
            }
            // The eyes, as holes: the ground showing through.
            for (ex, ey) in [(4.3, 5.5), (7.7, 5.5)] {
                fill_disc(
                    window,
                    at(origin.0 + ex * scale, origin.1 + ey * scale),
                    px(1.1 * scale),
                    hsla(t.ground),
                );
            }
        }
    }

    // The words: placed, then their plates, the crosshair, the words and
    // the tooltip over all.
    let measure = |window: &mut Window, s: &str, size: f32, weight: FontWeight| -> f32 {
        let line = shape(window, s, size, weight, ink, ui);
        f32::from(line.width)
    };
    let tip = hover.and_then(|h| tip_of(geo, h, t, ui, window));
    let labels = labels(geo, look, window, &measure, tip.as_ref().map(|t| t.rect));
    for l in labels.iter().filter(|l| l.cover) {
        let (x, y, w_, h) = l.plate(z);
        let ground = ground_at(geo, x + z * PLATE_PAD, t);
        window.paint_quad(
            fill(Bounds::new(at(x, y), size(px(w_), px(h))), ground)
                .corner_radii(px(look.shape.radius(PLATE_RADIUS) * z)),
        );
    }
    if let Some(Hover::Plot(x)) = hover {
        let gold = hsla(t.accent);
        for (width, a) in XHAIR_GLOW {
            stroke(window, (x, top), (x, foot), width * z, gold.opacity(a));
        }
        stroke(window, (x, top), (x, foot), z, gold.opacity(XHAIR_ALPHA));
        let y = geo.y_of(geo.rate_at(x));
        fill_disc(window, at(x, y), px(2.5 * z), gold);
    }
    for l in &labels {
        let line = shape(window, &l.words, l.px, l.weight, l.color, ui);
        let _ = line.paint(
            at(l.x, l.y),
            px(l.px * LINE),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
    // The reticle, where the theme frames its instruments: an L at each
    // corner of the plot.
    if look.fx.brackets {
        crate::window::instruments::paint_brackets(
            window,
            Bounds::new(at(0.0, top), size(px(w), px(foot - top))),
            z,
            hsla(t.bracket),
        );
    }
    if let Some(tip) = tip {
        let (x, y, w_, h) = tip.rect;
        super::paint::paint_float(
            window,
            Bounds::new(at(x, y), size(px(w_), px(h))),
            px(look.shape.radius(TIP_RADIUS) * z),
            px(z),
            hsla(t.surface),
            hsla(t.edge),
            &t,
            look.fx.glass,
        );
        let mut tx = x + z * TIP_PAD.0;
        for (s, color, weight) in tip.pieces {
            let line = shape(window, &s, z * TIP_PX, weight, color, ui);
            let width = f32::from(line.width);
            let _ = line.paint(
                at(tx, y + z * TIP_PAD.1),
                px(z * TIP_LINE),
                TextAlign::Left,
                None,
                window,
                cx,
            );
            tx += width;
        }
    }
}

/// The ground a plate at `x` stands on: the ribbon's own, or a lust band's
/// wash where the word starts inside one.
fn ground_at(geo: &Geo<'_>, x: f32, t: wowdps_gui_logic::theme::WindowTokens) -> Hsla {
    let banded = geo.r.lust.iter().any(|band| {
        let x1 = geo.x_of(band.at_ms as f64);
        let x2 = geo.x_of((band.at_ms + band.dur_ms) as f64);
        x >= x1 && x < x2
    });
    if banded {
        hsla(t.ink.alpha(BAND_ALPHA).over(t.ground))
    } else {
        hsla(t.ground)
    }
}

/// One line of the window's face, shaped.
fn shape(
    window: &mut Window,
    s: &str,
    size: f32,
    weight: FontWeight,
    color: Hsla,
    ui: &SharedString,
) -> gpui_kit::ShapedLine {
    let run = TextRun {
        len: s.len(),
        font: Font {
            weight,
            ..font(ui.clone())
        },
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(s.to_string()), px(size), &[run], None)
}

/// The tooltip, placed: its box and its pieces.
struct Tip {
    rect: (f32, f32, f32, f32),
    pieces: Vec<(String, Hsla, FontWeight)>,
}

fn tip_of(
    geo: &Geo<'_>,
    hover: Hover,
    t: wowdps_gui_logic::theme::WindowTokens,
    ui: &SharedString,
    window: &mut Window,
) -> Option<Tip> {
    let z = geo.z;
    let ink = hsla(t.ink);
    let (pieces, anchor) = match hover {
        Hover::Plot(x) => {
            let ms = geo.ms_at(x);
            let v = geo.rate_at(x);
            let mut pieces = vec![
                (duration(i64::from(ms)), ink, SEMIBOLD),
                (
                    format!(
                        "  {} {}",
                        geo.r.word.to_lowercase(),
                        commas(v.max(0.0).round() as u64)
                    ),
                    ink,
                    REGULAR,
                ),
            ];
            // The lust names itself only here, under the pointer: a word on
            // the ribbon either hid the curve's crest or was struck out by it.
            let at = i64::from(ms);
            if let Some(band) = geo
                .r
                .lust
                .iter()
                .find(|l| l.at_ms <= at && at < l.at_ms + l.dur_ms)
            {
                pieces.push((format!(", under {}", band.label), hsla(t.ink_2), REGULAR));
            }
            (pieces, None)
        }
        Hover::Skull(i) => {
            let s = geo.r.skulls.get(i)?;
            let you = s.class.map_or(ink, |c| {
                hsla(wowdps_gui_logic::theme::class_text_on(
                    c,
                    t.surface,
                    wowdps_gui_logic::theme::YOU_CONTRAST,
                ))
            });
            (
                vec![
                    (s.name.clone(), you, SEMIBOLD),
                    (format!(" {}", s.words), ink, REGULAR),
                ],
                Some(geo.skull_box(s)),
            )
        }
    };
    let tw = pieces
        .iter()
        .map(|(s, c, wt)| f32::from(shape(window, s, z * TIP_PX, *wt, *c, ui).width))
        .sum::<f32>()
        + 2.0 * z * TIP_PAD.0;
    let th = z * (TIP_LINE + 2.0 * TIP_PAD.1);
    let (mut x, mut y) = match (hover, anchor) {
        (_, Some((bx, by, side))) => (bx + side / 2.0 - tw / 2.0, by - th - z * TIP_OFF_Y),
        (Hover::Plot(x), None) if x + z * TIP_OFF_X + tw > geo.w => {
            (x - z * TIP_OFF_X - tw, geo.top() + z * TIP_OFF_Y)
        }
        (Hover::Plot(x), None) => (x + z * TIP_OFF_X, geo.top() + z * TIP_OFF_Y),
        (Hover::Skull(_), None) => return None,
    };
    x = x.clamp(0.0, (geo.w - tw).max(0.0));
    y = y.clamp(0.0, (geo.height() - th).max(0.0));
    Some(Tip {
        rect: (x, y, tw, th),
        pieces,
    })
}

/// Everything the ribbon says besides a tooltip — "you", the peak, each
/// lust's name, the ticks — placed so no two meet, less what the tooltip
/// covers (the iced ribbon's placement pass).
fn labels(
    geo: &Geo<'_>,
    look: &W,
    window: &mut Window,
    measure: &dyn Fn(&mut Window, &str, f32, FontWeight) -> f32,
    tip: Option<(f32, f32, f32, f32)>,
) -> Vec<Label> {
    let (z, w, t) = (geo.z, geo.w, look.t);
    let quiet = hsla(t.ink_3_text);
    let mut out: Vec<Label> = Vec::new();
    let label = |window: &mut Window, words: String, px_: f32, color: Hsla, weight: FontWeight| {
        let width = measure(window, &words, px_, weight);
        Label {
            words: words.into(),
            x: 0.0,
            y: 0.0,
            px: px_,
            color,
            weight,
            width,
            cover: true,
        }
    };
    let place = |out: &mut Vec<Label>, mut l: Label, spots: &[(f32, f32)]| {
        for (sx, sy) in spots {
            let pad = z * PLATE_PAD;
            l.x = sx.clamp(pad, (w - l.width - pad).max(pad));
            l.y = *sy;
            let r = l.plate(z);
            if !out.iter().any(|o| overlaps(o.plate(z), r)) {
                out.push(l);
                return;
            }
        }
    };
    let tick = z * look.size.tick;
    let line = tick * LINE;
    let above = z * if geo.narrow {
        YOU_ABOVE_NARROW
    } else {
        YOU_ABOVE
    };
    for s in geo.r.skulls.iter().filter(|s| s.mine) {
        let (bx, by, side) = geo.skull_box(s);
        let you = s.class.map_or(hsla(t.ink), |c| {
            hsla(wowdps_gui_logic::theme::class_text_on(
                c,
                t.surface,
                wowdps_gui_logic::theme::YOU_CONTRAST,
            ))
        });
        let l = label(window, "you".into(), z * YOU_PX, you, SEMIBOLD);
        let spot = (bx + side / 2.0 - l.width / 2.0, by - above);
        place(&mut out, l, &[spot]);
    }
    if !geo.r.rate.is_empty() {
        let l = label(
            window,
            peak_words(&geo.r.word, geo.r.peak()),
            tick,
            quiet,
            REGULAR,
        );
        let top = (z * PEAK_X, geo.top());
        place(&mut out, l, &[top, (top.0, top.1 + line)]);
    }
    let axis_y = geo.height() - z * (AXIS_BOTTOM + AXIS_H);
    for t_ in minute_ticks(geo.r.span_ms, w / z) {
        let words = duration(i64::from(t_));
        let mut l = Label {
            cover: false,
            ..label(window, words, tick, quiet, REGULAR)
        };
        let x = geo.x_of(f64::from(t_));
        let frac = x / w.max(1.0);
        l.x = if frac <= TICK_FIRST {
            x
        } else if frac > TICK_LAST {
            x - l.width
        } else {
            x - l.width / 2.0
        };
        l.y = axis_y;
        out.push(l);
    }
    match tip {
        Some(rect) => out
            .into_iter()
            .filter(|l| !overlaps(l.rect(), rect))
            .collect(),
        None => out,
    }
}

/// The skull (a 12-unit box): a rounded dome over a jaw; `eyes` adds the
/// two eye rings (an enemy's outline wears them).
fn skull(
    p: &mut PathBuilder,
    o: (f32, f32),
    k: f32,
    eyes: bool,
    at: &dyn Fn(f32, f32) -> Point<Pixels>,
) {
    let q = |x: f32, y: f32| at(o.0 + x * k, o.1 + y * k);
    p.move_to(q(6.0, 0.9));
    p.cubic_bezier_to(q(1.3, 5.2), q(3.1, 0.9), q(1.3, 2.8));
    p.cubic_bezier_to(q(3.0, 8.4), q(1.3, 6.7), q(2.0, 7.8));
    p.line_to(q(3.0, 10.8));
    p.line_to(q(9.0, 10.8));
    p.line_to(q(9.0, 8.4));
    p.cubic_bezier_to(q(10.7, 5.2), q(10.0, 7.8), q(10.7, 6.7));
    p.cubic_bezier_to(q(6.0, 0.9), q(10.7, 2.8), q(8.9, 0.9));
    p.close();
    if eyes {
        for (ex, ey) in [(4.3, 5.5), (7.7, 5.5)] {
            super::paint::circle(p, q(ex, ey), px(1.1 * k));
        }
    }
}

/// Trace `pts` into `path` as the prototype's `smooth()` does: a
/// Catmull-Rom spline as cubic Béziers, held inside the plot.
fn smooth(
    path: &mut PathBuilder,
    pts: &[(f32, f32)],
    top: f32,
    foot: f32,
    at: &dyn Fn(f32, f32) -> Point<Pixels>,
) {
    let hold = |(x, y): (f32, f32)| (x, y.clamp(top, foot));
    let get = |j: usize| {
        pts.get(j)
            .or_else(|| pts.last())
            .copied()
            .unwrap_or((0.0, 0.0))
    };
    for i in 0..pts.len().saturating_sub(1) {
        let (p0, p1, p2, p3) = (get(i.saturating_sub(1)), get(i), get(i + 1), get(i + 2));
        let c1 = hold((p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0));
        let c2 = hold((p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0));
        path.cubic_bezier_to(at(p2.0, p2.1), at(c1.0, c1.1), at(c2.0, c2.1));
    }
}

/// A stroke from one point to another, `width` wide, in a colour.
type Stroke<'a> = dyn Fn(&mut Window, (f32, f32), (f32, f32), f32, Hsla) + 'a;

/// The ribbon's chapter ring (`effects.fine_ticks`): a fine tick up from the
/// foot every ten seconds — the buckets the curve is cut in — and one twice
/// as tall each minute, thinned to every 30 s, then every minute, while
/// ten-second ones would stand closer than `CHAPTER_MIN` apart.
fn chapter_ring(geo: &Geo<'_>, foot: f32, stroke: &Stroke<'_>, color: Hsla, window: &mut Window) {
    let (z, span) = (geo.z, geo.r.span_ms.max(1) as f64);
    let gap = |step: i64| geo.x_of(step as f64) - geo.x_of(0.0);
    let Some(step) = [10_000_i64, 30_000, 60_000]
        .into_iter()
        .find(|s| gap(*s) >= CHAPTER_MIN * z)
    else {
        return;
    };
    let mut at = 0_i64;
    while (at as f64) <= span {
        let x = geo.x_of(at as f64).round() + 0.5;
        let (h, a) = if at % 60_000 == 0 {
            (CHAPTER_MINUTE, 1.0)
        } else {
            (CHAPTER_TICK, 0.7)
        };
        stroke(window, (x, foot), (x, foot - h * z), z, color.opacity(a));
        at += step;
    }
}

/// The chapter ring's ticks: the least gap between two, a ten-second
/// tick's height and a minute's.
const CHAPTER_MIN: f32 = 4.0;
const CHAPTER_TICK: f32 = 2.5;
const CHAPTER_MINUTE: f32 = 5.0;

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, point, px};
    use wowdps_gui_logic::raid::raided;
    use wowdps_model::View;

    use super::{Geo, Hover, Ribbon};
    use crate::window::tests::rig_over;

    /// A press on a skull opens that death — the Deaths view drilled into
    /// its window — and a press on the curve beside it opens nothing.
    #[gpui_kit::test]
    fn a_skull_s_press_opens_its_death(cx: &mut TestAppContext) {
        let rig = rig_over(cx, 1440., 900., raided(25));
        let (on_skull, off) = cx
            .update_window(rig.window, |_, window, cx| {
                window.render_frame(cx);
                let plot = window.find("ribbon-plot").bounds();
                let ribbon = Ribbon::of(rig.gui.read(cx), cx).expect("the raid's ribbon");
                let geo = Geo {
                    r: &ribbon,
                    z: 1.0,
                    w: f32::from(plot.size.width),
                    narrow: false,
                };
                let skull = ribbon
                    .skulls
                    .iter()
                    .find(|s| s.pick.key == "Player-1-5")
                    .expect("Raider5's skull");
                let (x, y, side) = geo.skull_box(skull);
                let at = (x + side / 2.0, y + side / 2.0);
                assert!(matches!(geo.hover_at(at.0, at.1), Some(Hover::Skull(_))));
                (at, (at.0 + 120.0, geo.top() + 4.0))
            })
            .unwrap();
        cx.update_window(rig.window, |_, window, cx| {
            window.click_at("ribbon-plot", point(px(off.0), px(off.1)), cx);
        })
        .unwrap();
        rig.session.read_with(cx, |s, _| {
            assert_eq!(s.state().view, View::Damage, "the curve opens nothing")
        });
        cx.update_window(rig.window, |_, window, cx| {
            window.click_at("ribbon-plot", point(px(on_skull.0), px(on_skull.1)), cx);
        })
        .unwrap();
        rig.session.read_with(cx, |s, _| {
            let state = s.state();
            assert_eq!(state.view, View::Deaths);
            assert_eq!(
                state.drill.as_ref().map(|d| d.key.as_str()),
                Some("Player-1-5")
            );
            assert_eq!(state.death_request(), Some(0));
        });
    }
}
