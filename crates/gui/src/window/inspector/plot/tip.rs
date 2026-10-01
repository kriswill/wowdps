//! The graph's words: one shaper that measures them as they will be
//! painted (shaped at the zoomed size, measured back at zoom 1, so the
//! geometry places what is drawn), the labels, and the tooltip (`.tip`):
//! the surface under a hairline of the edge, a swatch before each stacked
//! band's line, its lines one under the other.

use std::sync::Arc;

use gpui_kit::{
    App, BorderStyle, Font, FontWeight, Hsla, ShapedLine, SharedString, TextAlign, TextRun, Window,
    WindowTextSystem, font, quad,
};
use wowdps_gui_logic::inspect::geometry::{
    Face, HAIRLINE, Ink3, LINE, Label, SWATCH, SWATCH_RADIUS, TIP_LINE, TIP_PAD, TIP_PX,
    TIP_RADIUS, Tip,
};

use super::Pen;
use crate::theme::hsla;
use crate::window::w::{REGULAR, SEMIBOLD};

/// The window's weight for a word's face.
pub fn weight(face: Face) -> FontWeight {
    match face {
        Face::Regular => REGULAR,
        Face::Semibold => SEMIBOLD,
    }
}

pub(super) struct Shaper {
    text: Arc<WindowTextSystem>,
    pen: Pen,
}

impl Shaper {
    pub fn new(window: &Window, pen: Pen) -> Self {
        Self {
            text: window.text_system().clone(),
            pen,
        }
    }

    /// The window's ink for a word's role.
    pub fn ink(&self, role: Ink3) -> Hsla {
        let t = &self.pen.t;
        hsla(match role {
            Ink3::Quiet => t.ink_3_text,
            Ink3::Bad => t.bad,
            Ink3::Ink => t.ink,
            Ink3::Ink2 => t.ink_2,
        })
    }

    /// `s` shaped at `size` (zoom 1) in `face`, as it is painted.
    pub fn line(&self, s: &str, size: f32, face: Face, color: Hsla) -> ShapedLine {
        let run = TextRun {
            len: s.len(),
            font: Font {
                weight: weight(face),
                ..font(self.pen.ui)
            },
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.text.shape_line(
            SharedString::from(s.to_string()),
            self.pen.px(size),
            &[run],
            None,
        )
    }

    /// `s`'s one-line width at `size` in `face`, at zoom 1.
    pub fn width(&self, s: &str, size: f32, face: Face) -> f32 {
        f32::from(self.line(s, size, face, Hsla::default()).width) / self.pen.z
    }

    /// One line of words with its box's top-left at `(x, y)`.
    #[allow(clippy::too_many_arguments)]
    pub fn words(
        &self,
        s: &str,
        (x, y): (f32, f32),
        size: f32,
        face: Face,
        color: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) {
        let line = self.line(s, size, face, color);
        let _ = line.paint(
            self.pen.at(x, y),
            self.pen.px(size * LINE),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    pub fn paint_label(&self, l: &Label, window: &mut Window, cx: &mut App) {
        self.words(
            &l.words,
            (l.x, l.y),
            l.px,
            l.face,
            self.ink(l.ink),
            window,
            cx,
        );
    }

    /// The tooltip, its box first, then each line (a swatch before a
    /// stacked band's).
    pub fn paint_tip(&self, tip: &Tip, window: &mut Window, cx: &mut App) {
        let pen = self.pen;
        let r = tip.rect;
        // iced strokes the frame centred on the box's edge: half out.
        let half = HAIRLINE / 2.0;
        window.paint_quad(quad(
            pen.rect(r.x - half, r.y - half, r.w + HAIRLINE, r.h + HAIRLINE),
            pen.px(TIP_RADIUS + half),
            hsla(pen.t.surface),
            pen.px(HAIRLINE),
            hsla(pen.t.edge),
            BorderStyle::Solid,
        ));
        let inset = tip.inset();
        for (i, (s, role, face)) in tip.lines.iter().enumerate() {
            let y = r.y + TIP_PAD.1 + i as f32 * TIP_LINE;
            if let Some(Some(swatch)) = tip.swatches.get(i) {
                window.paint_quad(
                    gpui_kit::fill(
                        pen.rect(
                            r.x + TIP_PAD.0,
                            y + (TIP_LINE - SWATCH) / 2.0,
                            SWATCH,
                            SWATCH,
                        ),
                        hsla(*swatch),
                    )
                    .corner_radii(pen.px(SWATCH_RADIUS)),
                );
            }
            self.words(
                s,
                (r.x + TIP_PAD.0 + inset, y),
                TIP_PX,
                *face,
                self.ink(*role),
                window,
                cx,
            );
        }
    }
}
