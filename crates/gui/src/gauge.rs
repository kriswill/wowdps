//! Bespoke vector footer indicators. Font glyphs (⦿ ◷ ◴…) come from
//! whatever symbol font the fallback chain finds, with its own metrics and
//! pixel-grid rounding; these are drawn, so they are crisp at any zoom and
//! can animate smoothly.

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Theme};

/// A crisp filled circle, `d` px across (pass a zoom-scaled size).
pub fn dot<M: 'static>(color: Color, d: f32) -> Element<'static, M> {
    Canvas::new(Dot { color })
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .into()
}

/// A radar sweep in `color`: a dim ring, a glowing hand at `angle`
/// (radians, 0 at 12 o'clock, clockwise), and a gradient trail fading out
/// behind it.
pub fn radar<M: 'static>(angle: f32, d: f32, color: Color) -> Element<'static, M> {
    Canvas::new(Radar { angle, color })
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .into()
}

struct Dot {
    color: Color,
}

impl<M> canvas::Program<M> for Dot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let c = frame.center();
        let r = bounds.width.min(bounds.height) / 2.0;
        frame.fill(&Path::circle(c, r), self.color);
        vec![frame.into_geometry()]
    }
}

/// Trail length behind the hand, its band count, and the falloff curve:
/// exponential in the distance behind the hand, so the head is bright, the
/// drop is quick, and a long faint tail remains.
const TRAIL: f32 = std::f32::consts::PI * 1.4;
const TRAIL_BANDS: usize = 14;
const TRAIL_PEAK_ALPHA: f32 = 0.95;
const TRAIL_FALLOFF: f32 = 2.2;

struct Radar {
    angle: f32,
    color: Color,
}

impl<M> canvas::Program<M> for Radar {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let c = frame.center();
        let r = bounds.width.min(bounds.height) / 2.0 - 1.0;
        // Screen angle: 0 at 12 o'clock, clockwise. With y down, that is
        // math angle `a - 90°` and increasing values sweep clockwise.
        let screen = |a: f32| a - std::f32::consts::FRAC_PI_2;
        let tip =
            |a: f32, len: f32| Point::new(c.x + len * screen(a).cos(), c.y + len * screen(a).sin());
        let tint = |alpha: f32| Color {
            a: alpha,
            ..self.color
        };

        // The gradient trail: STACKED sectors, every one a big shape ending
        // at the hand — thin per-band slivers get washed out by antialiasing
        // at this size. Each layer's alpha is solved so the layers'
        // accumulation reproduces the exponential falloff profile.
        let profile = |f: f32| TRAIL_PEAK_ALPHA * (-TRAIL_FALLOFF * f.clamp(0.0, 1.0)).exp();
        let step = TRAIL / TRAIL_BANDS as f32;
        for i in 0..TRAIL_BANDS {
            let here = profile((i as f32 + 0.5) / TRAIL_BANDS as f32);
            let next = if i + 1 < TRAIL_BANDS {
                profile((i as f32 + 1.5) / TRAIL_BANDS as f32)
            } else {
                0.0
            };
            let alpha = ((here - next) / (1.0 - next)).clamp(0.0, 1.0);
            let from = self.angle - step * (i as f32 + 1.0);
            let mut b = canvas::path::Builder::new();
            b.move_to(c);
            b.line_to(tip(from, r));
            b.arc(canvas::path::Arc {
                center: c,
                radius: r,
                start_angle: Radians(screen(from)),
                end_angle: Radians(screen(self.angle)),
            });
            b.close();
            frame.fill(&b.build(), tint(alpha));
        }

        // The ring, over the trail so its edge stays clean.
        frame.stroke(
            &Path::circle(c, r),
            Stroke::default().with_width(1.0).with_color(tint(0.6)),
        );

        // The hand: wide soft strokes underneath for the glow, a bright
        // whitened core on top.
        let hand = Path::line(c, tip(self.angle, r));
        for (width, alpha) in [(3.6, 0.18), (2.2, 0.38)] {
            frame.stroke(
                &hand,
                Stroke::default()
                    .with_width(width)
                    .with_color(tint(alpha))
                    .with_line_cap(canvas::LineCap::Round),
            );
        }
        let core = Color {
            r: self.color.r + (1.0 - self.color.r) * 0.55,
            g: self.color.g + (1.0 - self.color.g) * 0.55,
            b: self.color.b + (1.0 - self.color.b) * 0.55,
            a: 0.95,
        };
        frame.stroke(
            &hand,
            Stroke::default()
                .with_width(1.2)
                .with_color(core)
                .with_line_cap(canvas::LineCap::Round),
        );
        vec![frame.into_geometry()]
    }
}
