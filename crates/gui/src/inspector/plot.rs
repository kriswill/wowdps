//! The inspector's graph (the prototype's `.igraph`): one plot with the
//! curve to itself — an area in the player's class colour, or two lines on
//! one scale for a comparison — a time axis in minutes shared with the
//! fight, a death hatched from the moment it happened to the rez, and the
//! lanes of spans under it — a comparison's lanes split in two, the first
//! player's spans over the second's. The curves are drawn through a
//! Catmull-Rom spline (the prototype's `smooth()`), so a rate reads as
//! hills rather than a saw.
//!
//! Where everything stands — every instant, value, span, death and word —
//! is gui-logic's `inspect::geometry` (moved from here, so both window
//! GUIs lay the graph out the same); this canvas measures its words and
//! paints. iced draws a canvas's text above all of its shapes, whatever
//! order they were made in: every label a tooltip covers is left out
//! rather than printed through it (`geometry::Plot::labels`).
//!
//! Hover is the canvas's own — a crosshair and the values under it, a
//! span's name, time, length and caster — and never a message; a drag
//! selects a zoom window and a right-click resets it.
//!
//! Window-only. The overlay's graph is `compare::drill_graph`, untouched.

use iced::widget::canvas::{self, Canvas, LineDash, Path, Stroke};
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use wowdps_gui_logic::inspect::geometry::{
    self as geo, AREA_ALPHA, CURVE_W, DASH, DRAG_MIN_PX, Face, GHOST_ALPHA, GHOST_W, HAIRLINE,
    HATCH_ALPHA, HATCH_DASH, HATCH_EDGE, HATCH_PATCH_ALPHA, HATCH_PATCH_PAD, HATCH_PATCH_RADIUS,
    HATCH_WORDS_Y, Hover, Ink3, LANE_H, LINE, PLOT_H, SPAN_ALPHA, SPAN_EDGE, SPAN_RADIUS,
    STACK_GAP, STRIPE_W, SWATCH, SWATCH_RADIUS, TICK_RADIUS, TIP_LINE, TIP_PAD, TIP_PX, TIP_RADIUS,
    TRACK_RADIUS, XHAIR_ALPHA,
};

use super::lanes::Row as LaneRow;
use crate::theme;
// The graph's data is gui-logic's (`inspect::plot`); this canvas draws it.
pub(crate) use wowdps_gui_logic::inspect::plot::{Curve, Dead, Ink};

/// The track under a lane (`.lane{background:rgba(255,255,255,.028)}`), a
/// drag's window and a hovered span's outline: the window tokens.
const TRACK: Color = theme::c(wowdps_gui_logic::theme::GOLD.window.lane_track);
const DRAG_FILL: Color = theme::c(wowdps_gui_logic::theme::GOLD.window.drag_fill);
const SPAN_LIT: Color = theme::c(wowdps_gui_logic::theme::GOLD.window.span_lit);

/// Everything the graph draws, owned: the inspector builds it from the
/// snapshot, the canvas draws it.
pub(crate) struct Plot<M> {
    /// The stretch of the fight on show, ms: the whole of it, or a zoom.
    pub window: (u32, u32),
    /// The one scale every curve is drawn on.
    pub peak: f64,
    pub curves: Vec<Curve>,
    pub dead: Vec<Dead>,
    pub lanes: Vec<LaneRow>,
    /// The hover reads the curve as a running total ("63.0M") rather than
    /// a rate ("149,258").
    pub total: bool,
    /// What the hover calls a lone curve's value ("dps").
    pub word: &'static str,
    /// What a drag-selected window (or a right-click's "the whole fight
    /// back") becomes; `None` leaves the graph unzoomable.
    pub on_range: Option<crate::compare::OnRange<M>>,
}

/// The graph, as tall as its lanes make it.
pub(crate) fn view<M: 'static>(plot: Plot<M>) -> Element<'static, M> {
    let height = plot.geo().height();
    Canvas::new(plot)
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .into()
}

/// The canvas's own memory: a drag in progress, and what the pointer was
/// last over (so a move that changes nothing redraws nothing).
#[derive(Default)]
pub(crate) struct State {
    drag: Option<(f32, f32)>,
    hover: Option<Hover>,
}

impl<M> Plot<M> {
    /// The graph's geometry, over its own data.
    fn geo(&self) -> geo::Plot<'_> {
        geo::Plot {
            window: self.window,
            peak: self.peak,
            curves: &self.curves,
            dead: &self.dead,
            lanes: &self.lanes,
            total: self.total,
            word: self.word,
        }
    }
}

/// The window's face for a word.
fn font(face: Face) -> Font {
    match face {
        Face::Regular => theme::UI,
        Face::Semibold => theme::UI_SEMIBOLD,
    }
}

/// The window's ink for a word's role.
fn ink(role: Ink3) -> Color {
    match role {
        Ink3::Quiet => theme::INK_3_TEXT,
        Ink3::Bad => theme::BAD,
        Ink3::Ink => theme::INK,
        Ink3::Ink2 => theme::INK_2,
    }
}

/// `content`'s one-line width at `px` in `face`, as the renderer shapes it.
fn text_w(content: &str, px: f32, face: Face) -> f32 {
    crate::ellipsis::width_of::<<Renderer as iced::advanced::text::Renderer>::Paragraph>(
        content,
        px,
        font(face),
    )
}

fn pt((x, y): (f32, f32)) -> Point {
    Point::new(x, y)
}

/// Trace `pts` into `path` through gui-logic's spline (`geometry::smooth`).
fn smooth(path: &mut canvas::path::Builder, pts: &[(f32, f32)]) {
    for [c1, c2, to] in geo::smooth(pts) {
        path.bezier_curve_to(pt(c1), pt(c2), pt(to));
    }
}

/// One line of canvas text.
fn words(frame: &mut canvas::Frame, s: &str, at: Point, px: f32, color: Color, font: Font) {
    frame.fill_text(canvas::Text {
        content: s.to_string(),
        position: at,
        color,
        size: px.into(),
        font,
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top,
        ..canvas::Text::default()
    });
}

impl<M> canvas::Program<M> for Plot<M> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        use iced::mouse::{Button, Event as Mouse};
        let iced::Event::Mouse(mouse) = event else {
            return None;
        };
        let g = self.geo();
        let w = bounds.width;
        let left = g.left();
        let pos = cursor.position_in(bounds);
        match mouse {
            Mouse::ButtonPressed(Button::Left) if self.on_range.is_some() => {
                let p = pos.filter(|p| g.in_plot(p.x, p.y))?;
                state.drag = Some((p.x, p.x));
                Some(canvas::Action::request_redraw().and_capture())
            }
            Mouse::ButtonReleased(Button::Left) => {
                let (a, b) = state.drag.take()?;
                let on_range = self.on_range.as_ref()?;
                let Some(range) = g.drag_range(a, b, w) else {
                    return Some(canvas::Action::request_redraw());
                };
                Some(canvas::Action::publish(on_range(Some(range))).and_capture())
            }
            Mouse::ButtonPressed(Button::Right) => {
                let on_range = self.on_range.as_ref()?;
                pos.filter(|p| g.in_plot(p.x, p.y))?;
                Some(canvas::Action::publish(on_range(None)).and_capture())
            }
            Mouse::CursorMoved { .. } | Mouse::CursorLeft => {
                if let Some((_, cur)) = state.drag.as_mut() {
                    let x = cursor.position().map(|p| (p.x - bounds.x).clamp(left, w))?;
                    *cur = x;
                    return Some(canvas::Action::request_redraw());
                }
                let hover = pos.and_then(|p| g.hover_at(p.x, p.y, w));
                if hover == state.hover {
                    return None;
                }
                state.hover = hover;
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::ResizingHorizontally;
        }
        match cursor
            .position_in(bounds)
            .and_then(|p| self.geo().hover_at(p.x, p.y, bounds.width))
        {
            Some(Hover::Plot(_)) if self.on_range.is_some() => mouse::Interaction::Crosshair,
            _ => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let g = self.geo();
        let w = bounds.width;
        let left = g.left();
        let plot_w = (w - left).max(1.0);
        // The hatches' words, placed once: their band is what the curves
        // peak under.
        let hatches = g.hatch_labels(w, &text_w);
        let top = geo::Plot::top_of(&hatches);

        // The baseline, a hairline.
        frame.stroke(
            &Path::line(Point::new(left, PLOT_H - 0.5), Point::new(w, PLOT_H - 0.5)),
            Stroke::default()
                .with_width(HAIRLINE)
                .with_color(theme::LINE),
        );

        // A death, hatched from the moment to the rez (or the end), its
        // edge a dashed red rule.
        for d in &self.dead {
            let x1 = g.x_of(d.at_ms as f64, w).clamp(left, w);
            let x2 = g.x_of(d.end_ms as f64, w).clamp(left, w);
            if x2 <= x1 {
                continue;
            }
            let ink = Color {
                a: HATCH_ALPHA,
                ..theme::BAD
            };
            for (a, b) in geo::stripes(x1, x2) {
                frame.stroke(
                    &Path::line(pt(a), pt(b)),
                    Stroke::default().with_width(STRIPE_W).with_color(ink),
                );
            }
            frame.stroke(
                &Path::line(Point::new(x1, 0.0), Point::new(x1, PLOT_H)),
                Stroke {
                    line_dash: LineDash {
                        segments: &HATCH_DASH,
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_width(HATCH_EDGE)
                        .with_color(theme::BAD)
                },
            );
        }

        // R26: the stack — each band from the running sum of those before it
        // up to its own top, in list order (bottom first), its top edge a
        // 2 px line of the panel so two bands read as two.
        for band in g.bands(w, top) {
            let (Some(first), Some(last)) = (band.upper.first(), band.upper.last()) else {
                continue;
            };
            let mut path = canvas::path::Builder::new();
            path.move_to(pt(*first));
            smooth(&mut path, &band.upper);
            if band.lower.is_empty() {
                path.line_to(Point::new(last.0, PLOT_H));
                path.line_to(Point::new(first.0, PLOT_H));
            } else {
                let back: Vec<(f32, f32)> = band.lower.iter().rev().copied().collect();
                if let Some(b0) = back.first() {
                    path.line_to(pt(*b0));
                }
                smooth(&mut path, &back);
            }
            path.close();
            frame.fill(&path.build(), theme::c(band.color));
            let mut edge = canvas::path::Builder::new();
            edge.move_to(pt(*first));
            smooth(&mut edge, &band.upper);
            frame.stroke(
                &edge.build(),
                Stroke::default()
                    .with_width(STACK_GAP)
                    .with_color(theme::SURFACE)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        // The curves: ghosts first, so a focus reads on top of its context.
        for c in g.lines() {
            let pts = g.points(c, w, top);
            let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
                continue;
            };
            let mut line = canvas::path::Builder::new();
            line.move_to(pt(*first));
            smooth(&mut line, &pts);
            let line = line.build();
            if c.ink == Ink::Area {
                let mut area = canvas::path::Builder::new();
                area.move_to(Point::new(first.0, PLOT_H));
                area.line_to(pt(*first));
                smooth(&mut area, &pts);
                area.line_to(Point::new(last.0, PLOT_H));
                area.close();
                frame.fill(
                    &area.build(),
                    Color {
                        a: AREA_ALPHA,
                        ..theme::c(c.color)
                    },
                );
            }
            let stroke = match c.ink {
                Ink::Ghost => Stroke::default().with_width(GHOST_W).with_color(Color {
                    a: GHOST_ALPHA,
                    ..theme::c(c.color)
                }),
                Ink::Dashed => Stroke {
                    line_dash: LineDash {
                        segments: &DASH,
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_width(CURVE_W)
                        .with_color(theme::c(c.color))
                },
                Ink::Area | Ink::Line | Ink::Stack => Stroke::default()
                    .with_width(CURVE_W)
                    .with_color(theme::c(c.color)),
            };
            frame.stroke(&line, stroke.with_line_join(canvas::LineJoin::Round));
        }

        // The hatches' words sit on a patch of the panel, over the hatch's
        // stripes and its dashed edge.
        for l in &hatches {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(l.x - HATCH_PATCH_PAD, l.y - HATCH_WORDS_Y),
                    Size::new(
                        l.width + 2.0 * HATCH_PATCH_PAD,
                        l.px * LINE + 2.0 * HATCH_WORDS_Y,
                    ),
                    HATCH_PATCH_RADIUS.into(),
                ),
                Color {
                    a: HATCH_PATCH_ALPHA,
                    ..theme::SURFACE
                },
            );
        }

        // The lanes: the faint track, the spans in their casters' colours,
        // each edged in the panel's surface so two that overlap read as
        // two — the hovered one lit and outlined in white.
        let hovered_span = match state.hover {
            Some(Hover::Span(l, i)) => Some((l, i)),
            _ => None,
        };
        for (li, row) in self.lanes.iter().enumerate() {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(left, g.track_y(li)),
                    Size::new(plot_w, LANE_H),
                    TRACK_RADIUS.into(),
                ),
                TRACK,
            );
            for (si, s) in row.spans.iter().enumerate() {
                let Some(r) = g.span_rect(li, s, w) else {
                    continue;
                };
                let lit = hovered_span == Some((li, si));
                let radius = if s.dur_ms <= 0 {
                    TICK_RADIUS
                } else {
                    SPAN_RADIUS
                };
                let shape = Path::rounded_rectangle(
                    Point::new(r.x, r.y),
                    Size::new(r.w, r.h),
                    radius.into(),
                );
                frame.fill(
                    &shape,
                    Color {
                        a: if lit { 1.0 } else { SPAN_ALPHA },
                        ..theme::c(s.color)
                    },
                );
                frame.stroke(
                    &shape,
                    Stroke::default().with_width(SPAN_EDGE).with_color(if lit {
                        SPAN_LIT
                    } else {
                        theme::SURFACE
                    }),
                );
            }
        }

        // A drag in progress, over everything but the tooltip.
        if let Some((a, b)) = state.drag
            && (b - a).abs() >= DRAG_MIN_PX
        {
            let (lo, hi) = (a.min(b), a.max(b));
            frame.fill(
                &Path::rectangle(Point::new(lo, 0.0), Size::new(hi - lo, PLOT_H)),
                DRAG_FILL,
            );
        }

        // The crosshair, in gold (`.xhair`).
        if let Some(Hover::Plot(x)) = state.hover {
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, PLOT_H)),
                Stroke::default().with_width(HAIRLINE).with_color(Color {
                    a: XHAIR_ALPHA,
                    ..theme::GOLD
                }),
            );
        }

        // Every label the tooltip leaves clear, then the tooltip over all.
        for l in g.labels(state.hover, w, &text_w) {
            words(
                &mut frame,
                &l.words,
                Point::new(l.x, l.y),
                l.px,
                ink(l.ink),
                font(l.face),
            );
        }
        if let Some(tip) = state.hover.and_then(|h| g.tip(h, w, &text_w)) {
            let r = tip.rect;
            let shape = Path::rounded_rectangle(
                Point::new(r.x, r.y),
                Size::new(r.w, r.h),
                TIP_RADIUS.into(),
            );
            frame.fill(&shape, theme::SURFACE);
            frame.stroke(
                &shape,
                Stroke::default()
                    .with_width(HAIRLINE)
                    .with_color(theme::EDGE),
            );
            let inset = tip.inset();
            for (i, (s, role, face)) in tip.lines.iter().enumerate() {
                let y = r.y + TIP_PAD.1 + i as f32 * TIP_LINE;
                if let Some(Some(swatch)) = tip.swatches.get(i) {
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(r.x + TIP_PAD.0, y + (TIP_LINE - SWATCH) / 2.0),
                            Size::new(SWATCH, SWATCH),
                            SWATCH_RADIUS.into(),
                        ),
                        theme::c(*swatch),
                    );
                }
                words(
                    &mut frame,
                    s,
                    Point::new(r.x + TIP_PAD.0 + inset, y),
                    TIP_PX,
                    ink(*role),
                    font(*face),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::lanes::{Lane, Span};
    use wowdps_gui_logic::inspect::geometry::LEFT;

    fn plot(window: (u32, u32), lanes: Vec<LaneRow>) -> Plot<()> {
        Plot {
            window,
            peak: 100.0,
            curves: vec![Curve {
                name: String::new(),
                color: theme::g(Color::WHITE),
                points: vec![10.0, 50.0, 100.0, 40.0],
                bucket_ms: 1000,
                ink: Ink::Area,
            }],
            dead: Vec::new(),
            lanes,
            total: false,
            word: "dps",
            on_range: None,
        }
    }

    fn span(at_ms: i64, dur_ms: i64) -> Span {
        Span {
            at_ms,
            dur_ms,
            label: "Heroism".into(),
            caster: Some("Vingsham".into()),
            color: theme::g(Color::WHITE),
            whose: None,
            second: false,
        }
    }

    /// A drag over the plot selects a window and publishes it; a click
    /// that barely moves publishes nothing; a right-click asks for the
    /// whole fight back.
    #[test]
    fn a_drag_zooms_a_click_does_not_and_a_right_click_resets() {
        use iced::mouse::{Button, Event as Mouse};
        use iced::widget::canvas::Program;
        let mut p = plot((0, 4_000), Vec::new());
        let on_range: crate::compare::OnRange<Option<(u32, u32)>> = std::rc::Rc::new(|r| r);
        let p: Plot<Option<(u32, u32)>> = Plot {
            window: p.window,
            peak: p.peak,
            curves: std::mem::take(&mut p.curves),
            dead: Vec::new(),
            lanes: Vec::new(),
            total: false,
            word: "dps",
            on_range: Some(on_range),
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(400.0, p.geo().height()));
        let at = |x: f32| mouse::Cursor::Available(Point::new(x, 40.0));
        let send = |state: &mut State, e: Mouse, x: f32| {
            p.update(state, &iced::Event::Mouse(e), bounds, at(x))
                .map(|a| a.into_inner().0)
        };
        let mut state = State::default();
        send(&mut state, Mouse::ButtonPressed(Button::Left), 100.0);
        send(
            &mut state,
            Mouse::CursorMoved {
                position: Point::new(200.0, 40.0),
            },
            200.0,
        );
        let zoom = send(&mut state, Mouse::ButtonReleased(Button::Left), 200.0);
        assert_eq!(
            zoom,
            Some(Some(Some((1_000, 2_000)))),
            "a quarter to a half"
        );
        send(&mut state, Mouse::ButtonPressed(Button::Left), 100.0);
        let click = send(&mut state, Mouse::ButtonReleased(Button::Left), 101.0);
        assert_eq!(click, Some(None), "a click: a redraw, nothing published");
        let reset = send(&mut state, Mouse::ButtonPressed(Button::Right), 150.0);
        assert_eq!(reset, Some(Some(None)), "the whole fight back");
    }

    /// Every state draws — hatched, dashed, ghosted, hovered and dragging —
    /// through the software renderer without a panic.
    #[test]
    fn the_graph_draws_in_every_state() {
        use iced::widget::canvas::Program;
        let lanes = vec![LaneRow {
            lane: Lane::Cooldowns,
            spans: vec![span(500, 1_500), span(3_000, 0)],
        }];
        let mut p = plot((0, 4_000), lanes);
        p.dead.push(Dead {
            at_ms: 2_500,
            end_ms: 4_000,
            words: "died 0:02".into(),
        });
        for ink in [Ink::Line, Ink::Dashed, Ink::Ghost] {
            p.curves.push(Curve {
                ink,
                ..p.curves[0].clone()
            });
        }
        let renderer = crate::window::testkit::renderer();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(LEFT + 400.0, p.geo().height()));
        let w = bounds.width;
        for hover in [
            None,
            Some(Hover::Span(0, 0)),
            p.geo().hover_at(LEFT + 100.0, 50.0, w),
        ] {
            let state = State {
                drag: Some((LEFT + 10.0, LEFT + 90.0)),
                hover,
            };
            let geometry = p.draw(
                &state,
                &renderer,
                &Theme::Dark,
                bounds,
                mouse::Cursor::Unavailable,
            );
            assert_eq!(geometry.len(), 1);
        }
    }

    /// The graph's shots: every state of gui-logic's shared samples drawn
    /// on the inspector's surface, 16 px around, the pointer where the
    /// sample rests it — the same pictures gui-new's
    /// `inspector_plot_shots` takes, so the two sets diff pixel for pixel.
    ///
    /// `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui plot_shots -- --ignored`
    #[test]
    #[ignore = "writes PNGs; run by hand beside gui-new's"]
    fn plot_shots() {
        use crate::window::testkit::simulator_as;
        use wowdps_gui_logic::inspect::geometry::samples;
        const PAD: f32 = 16.0;
        let Some(dir) = std::env::var_os("WOWDPS_SHOTS_DIR").map(std::path::PathBuf::from) else {
            return;
        };
        for s in samples::all() {
            let p: Plot<()> = Plot {
                window: s.window,
                peak: s.peak,
                curves: s.curves,
                dead: s.dead,
                lanes: s.lanes,
                total: s.total,
                word: s.word,
                on_range: None,
            };
            let frame = Size::new(samples::WIDTH + 2.0 * PAD, p.geo().height() + 2.0 * PAD);
            let page = iced::widget::container(view(p))
                .padding(PAD)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| iced::widget::container::Style {
                    background: Some(theme::SURFACE.into()),
                    ..Default::default()
                });
            let mut ui = simulator_as(crate::window::settings(), frame, page.into());
            if let Some((x, y)) = s.pointer {
                let at = Point::new(PAD + x, PAD + y);
                ui.point_at(at);
                let _ = ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved {
                    position: at,
                })]);
            }
            let snap = ui.snapshot(&Theme::TokyoNight).unwrap();
            let scratch = dir.join(".render");
            let _ = std::fs::remove_dir_all(&scratch);
            assert!(snap.matches_image(scratch.join(s.name)).unwrap());
            let made = std::fs::read_dir(&scratch)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            std::fs::rename(made, dir.join(format!("plot-{}.png", s.name))).unwrap();
            let _ = std::fs::remove_dir(&scratch);
        }
    }
}
