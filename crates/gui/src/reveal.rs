//! A strip of tabs too wide for its row, shown through a window onto it
//! that keeps the ACTIVE tab whole in sight (`.vscroll`, whose browser
//! scrolls the selected tab into view).
//!
//! A `scrollable` can only be told where to stand from `update`, by an
//! operation that runs after the frame it would fix — and a first frame, a
//! resize or a filter widening beside the strip goes through no `update`
//! at all. Anchoring the strip at one end by the active tab's INDEX, what
//! the window did before, is right only while the strip has most of the
//! row: beside the meter's filter, a 460 px window shows about three tabs,
//! and a tab in the middle (Deaths, Interrupts) was in sight from neither
//! end. So the layout decides, where the widths are known: whenever the
//! active tab, the window onto the strip or the strip itself changed since
//! the last layout, the strip moves the least that brings the active tab
//! whole into sight. A wheel over the strip moves it along in between, as
//! far as its ends allow, and a relayout that changed none of the three
//! leaves it where the wheel put it. An edge with more of the strip past
//! it fades into what the strip sits on, so the reader sees there is more
//! and no tab is cut to a stray glyph.
//!
//! Window-only: the overlay draws no tab strip.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, overlay};
use iced::gradient::Linear;
use iced::{
    Background, Color, Element, Event, Gradient, Length, Point, Radians, Rectangle, Size, Vector,
    mouse,
};

/// A wheel notch, in pixels along the strip: the old scrollable's line.
const LINE: f32 = 60.0;
/// How far in from an edge the strip fades into what it sits on, where
/// more of it lies past that edge: a tab cut there reads as going on
/// rather than as a stray glyph (a skull and a sliver of "D" read "[").
pub(crate) const FADE: f32 = 24.0;

/// `content` — a row whose children are the tabs — seen through a window
/// as wide as the layout gives it, with the `active`th child kept whole in
/// sight.
pub(crate) struct Reveal<'a, M, Theme, Renderer> {
    content: Element<'a, M, Theme, Renderer>,
    active: Option<usize>,
    /// What the strip sits on, which an edge with more beyond it fades
    /// into ([`FADE`]); `None` draws hard edges.
    fade: Option<Color>,
}

pub(crate) fn reveal<'a, M, Theme, Renderer>(
    content: impl Into<Element<'a, M, Theme, Renderer>>,
    active: Option<usize>,
) -> Reveal<'a, M, Theme, Renderer> {
    Reveal {
        content: content.into(),
        active,
        fade: None,
    }
}

impl<M, Theme, Renderer> Reveal<'_, M, Theme, Renderer> {
    /// Fade an edge with more of the strip beyond it into `ground`.
    pub(crate) fn fade(mut self, ground: Color) -> Self {
        self.fade = Some(ground);
        self
    }
}

/// Which edges have more of the strip past them, for a strip `long` wide
/// seen `offset` along through a window `width` wide: (left, right).
pub(crate) fn cut_edges(offset: f32, width: f32, long: f32) -> (bool, bool) {
    const EPS: f32 = 0.5;
    let far = (long - width).max(0.0);
    (offset > EPS, offset < far - EPS)
}

/// Where the strip stands, and what it was laid out for last.
#[derive(Debug, Default)]
struct State {
    /// How far along the strip the window's left edge is.
    offset: f32,
    /// The active tab, the window's width and the strip's at the last
    /// layout: a change in any of them brings the active tab into sight.
    seen: Option<(Option<usize>, f32, f32)>,
}

/// The offset nearest `offset` at which `[start, end]` is whole inside a
/// window `width` wide: unchanged when it already is; its start at the
/// window's left edge when it is off to the left (or wider than the
/// window); its end at the right edge when it is off to the right.
pub(crate) fn nearest(offset: f32, width: f32, start: f32, end: f32) -> f32 {
    if end - start >= width || start < offset {
        start
    } else if end > offset + width {
        end - width
    } else {
        offset
    }
}

impl<M, Theme, Renderer> Widget<M, Theme, Renderer> for Reveal<'_, M, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        let Some(child) = tree.children.first_mut() else {
            return layout::Node::new(Size::ZERO);
        };
        // The strip at its own width, however wide that is.
        let strip = self.content.as_widget_mut().layout(
            child,
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, max.height)),
        );
        let long = strip.size().width;
        let width = if max.width.is_finite() {
            max.width
        } else {
            long
        };
        let height = strip.size().height;
        let span = self
            .active
            .and_then(|i| strip.children().get(i))
            .map(|tab| {
                let b = tab.bounds();
                (b.x, b.x + b.width)
            });
        let state = tree.state.downcast_mut::<State>();
        let now = (self.active, width, long);
        if state.seen != Some(now) {
            state.seen = Some(now);
            if let Some((start, end)) = span {
                state.offset = nearest(state.offset, width, start, end);
            }
        }
        state.offset = state.offset.clamp(0.0, (long - width).max(0.0));
        let strip = strip.move_to(Point::new(-state.offset, 0.0));
        layout::Node::with_children(Size::new(width, height), vec![strip])
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let (Some(strip), Some(child)) = (layout.children().next(), tree.children.first()) else {
            return;
        };
        renderer.with_layer(clip, |renderer| {
            self.content.as_widget().draw(
                child,
                renderer,
                theme,
                style,
                strip,
                inside(cursor, bounds),
                &clip,
            );
        });
        let Some(ground) = self.fade else {
            return;
        };
        // A layer of its own over the strip's: within one layer iced draws
        // text above every quad, so a fade drawn with the tabs would sit
        // under their words.
        renderer.with_layer(clip, |renderer| {
            let offset = tree.state.downcast_ref::<State>().offset;
            let (left, right) = cut_edges(offset, bounds.width, strip.bounds().width);
            let w = FADE.min(bounds.width / 2.0);
            let clear = Color { a: 0.0, ..ground };
            // A quad washing from `from` to `to`, left to right.
            let mut wash = |x: f32, from: Color, to: Color| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x,
                            y: bounds.y,
                            width: w,
                            height: bounds.height,
                        },
                        ..renderer::Quad::default()
                    },
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(std::f32::consts::FRAC_PI_2))
                            .add_stop(0.0, from)
                            .add_stop(1.0, to),
                    )),
                );
            };
            if left {
                wash(bounds.x, ground, clear);
            }
            if right {
                wash(bounds.x + bounds.width - w, clear, ground);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let (Some(strip), Some(child)) = (layout.children().next(), tree.children.first_mut())
        else {
            return;
        };
        // A tab moved out of the window is out of reach too: the pointer
        // only reaches the strip where the window shows it.
        self.content.as_widget_mut().update(
            child,
            event,
            strip,
            inside(cursor, bounds),
            renderer,
            clipboard,
            shell,
            &bounds.intersection(viewport).unwrap_or(bounds),
        );
        if shell.is_event_captured() || !cursor.is_over(bounds) {
            return;
        }
        let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event else {
            return;
        };
        // Every wheel is pixels along the strip — a mouse's vertical one as
        // much as a trackpad's sideways one — a wheel turned toward the
        // reader, like a swipe to the left, reading as further along.
        let (x, y) = match *delta {
            mouse::ScrollDelta::Lines { x, y } => (x * LINE, y * LINE),
            mouse::ScrollDelta::Pixels { x, y } => (x, y),
        };
        let state = tree.state.downcast_mut::<State>();
        let far = (strip.bounds().width - bounds.width).max(0.0);
        let offset = (state.offset - (x + y)).clamp(0.0, far);
        if offset != state.offset {
            state.offset = offset;
            shell.invalidate_layout();
            shell.request_redraw();
        }
        shell.capture_event();
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let (Some(strip), Some(child)) = (layout.children().next(), tree.children.first()) else {
            return mouse::Interaction::None;
        };
        self.content.as_widget().mouse_interaction(
            child,
            strip,
            inside(cursor, bounds),
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.container(None, layout.bounds());
        let (Some(strip), Some(child)) = (layout.children().next(), tree.children.first_mut())
        else {
            return;
        };
        operation.traverse(&mut |operation| {
            self.content
                .as_widget_mut()
                .operate(child, strip, renderer, operation);
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let strip = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            tree.children.first_mut()?,
            strip,
            renderer,
            viewport,
            translation,
        )
    }
}

/// The pointer as the strip sees it: where it is over the window, and
/// nowhere otherwise.
fn inside(cursor: mouse::Cursor, bounds: Rectangle) -> mouse::Cursor {
    if cursor.is_over(bounds) {
        cursor
    } else {
        mouse::Cursor::Unavailable
    }
}

impl<'a, M, Theme, Renderer> From<Reveal<'a, M, Theme, Renderer>>
    for Element<'a, M, Theme, Renderer>
where
    M: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(r: Reveal<'a, M, Theme, Renderer>) -> Self {
        Element::new(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fade is drawn OVER the tabs' words — a layer of its own, since
    /// iced draws a layer's text above its quads — so a word cut by the
    /// window's edge washes out into the ground instead of standing there
    /// as a stray glyph.
    #[test]
    fn a_cut_edge_fades_over_the_words() {
        use crate::window::testkit::pixels;
        use iced::widget::{Row, text};
        let strip = || {
            Row::with_children((0..8).map(|i| {
                text(format!("Tab number {i}"))
                    .size(15)
                    .color(Color::WHITE)
                    .into()
            }))
            .spacing(10)
        };
        let size = Size::new(300.0, 30.0);
        // Bright pixels in the window's last 5 px (physical, at scale 2).
        let lit = |fade: bool| {
            let r = reveal(strip(), Some(0));
            let r = if fade { r.fade(Color::BLACK) } else { r };
            pixels::<()>(Element::from(r), size, &iced::Theme::Dark).count_in(
                (590, 0, 600, 60),
                Color::WHITE,
                100,
            )
        };
        assert!(lit(false) > 0, "the words run to the edge");
        assert_eq!(lit(true), 0, "and fade out before it");
    }

    /// An edge fades only where more of the strip lies past it.
    #[test]
    fn only_an_edge_with_more_beyond_it_fades() {
        assert_eq!(cut_edges(0.0, 300.0, 300.0), (false, false), "all in sight");
        assert_eq!(
            cut_edges(0.0, 300.0, 500.0),
            (false, true),
            "more to the right"
        );
        assert_eq!(
            cut_edges(200.0, 300.0, 500.0),
            (true, false),
            "at the far end"
        );
        assert_eq!(cut_edges(100.0, 300.0, 500.0), (true, true));
    }

    #[test]
    fn a_tab_in_sight_stays_where_it_is() {
        assert_eq!(nearest(0.0, 300.0, 100.0, 200.0), 0.0);
        assert_eq!(nearest(50.0, 300.0, 100.0, 200.0), 50.0);
    }

    #[test]
    fn a_tab_out_of_sight_is_brought_to_the_nearer_edge() {
        // Off to the right: its end at the window's right edge.
        assert_eq!(nearest(0.0, 300.0, 350.0, 450.0), 150.0);
        // Off to the left: its start at the left edge.
        assert_eq!(nearest(400.0, 300.0, 100.0, 200.0), 100.0);
        // Cut by the right edge.
        assert_eq!(nearest(0.0, 300.0, 250.0, 340.0), 40.0);
        // Wider than the window: its start, its words' beginning.
        assert_eq!(nearest(0.0, 80.0, 100.0, 200.0), 100.0);
    }
}
