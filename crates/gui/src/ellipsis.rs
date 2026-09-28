//! One line of text that ends in "…" when it does not fit, instead of being
//! cut through a glyph: a clipped "Fel Firebolt (Wild Im" reads as a
//! different word, "Fel Firebolt (Wild…" as the start of one. Window-only —
//! the overlay's rows clip as they always have.
//!
//! It is as wide as its parent gives it (`Length::Fill`), measures its text
//! with the renderer's own shaping, and keeps the longest prefix that fits
//! with the ellipsis after it, cut at a character. A test or an operation
//! finds it by its WHOLE text: what is drawn is a picture of the label, the
//! label is still what the row is.
//!
//! A label that must leave room for what follows it on its line — a role
//! glyph after a name, the meta after a fight's title — says how much
//! ([`Ellipsis::leaving`]): it is then as wide as what it shows, so what
//! follows sits right after it, and it gives way before any of that does.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text as core_text;
use iced::advanced::widget::text as text_widget;
use iced::advanced::widget::{self, Tree, Widget, tree};
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Color, Element, Font, Length, Pixels, Rectangle, Size, mouse};

/// What stands in for the rest of a label that does not fit.
const MARK: &str = "…";

/// A one-line label, ellipsised to its width.
pub(crate) struct Ellipsis {
    full: String,
    size: f32,
    font: Font,
    color: Option<Color>,
    line_height: LineHeight,
    /// What the line keeps after the label ([`Ellipsis::leaving`]).
    leave: Option<Leave>,
}

/// Room a label leaves after it: `texts`, each measured at layout in its
/// own size and font, and `px` more — fixed widths and the gaps between.
struct Leave {
    texts: Vec<(String, f32, Font)>,
    px: f32,
}

/// A one-line label that ends in "…" where it would be cut.
pub(crate) fn ellipsis(full: impl Into<String>) -> Ellipsis {
    Ellipsis {
        full: full.into(),
        size: crate::theme::size::BODY,
        font: crate::theme::UI,
        color: None,
        line_height: LineHeight::default(),
        leave: None,
    }
}

impl Ellipsis {
    pub(crate) fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub(crate) fn font(mut self, font: Font) -> Self {
        self.font = font;
        self
    }

    pub(crate) fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The label's line height: iced's 1.3 by default; a title that sets
    /// its own (`.ftitle h2{line-height:1.1}`) says so.
    pub(crate) fn line_height(mut self, line_height: LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    /// Leave room after the label for `texts` (measured at layout, each in
    /// its own size and font — words whose width only the renderer knows)
    /// and `px` more. The label is then as wide as what it shows rather
    /// than its parent: a Shrink child of its row, fitted to the row's
    /// width less the room, so whatever follows it keeps its place.
    pub(crate) fn leaving(mut self, texts: Vec<(String, f32, Font)>, px: f32) -> Self {
        self.leave = Some(Leave { texts, px });
        self
    }

    /// Its parent's width, or — leaving room — its own.
    fn width(&self) -> Length {
        if self.leave.is_some() {
            Length::Shrink
        } else {
            Length::Fill
        }
    }
}

/// The label as it was last fitted, so a relayout at the same width with the
/// same words shapes nothing new.
struct State<P: core_text::Paragraph> {
    fitted: Option<(String, f32, String)>,
    plain: text_widget::State<P>,
}

/// `content`'s one-line width at `size` in `font`, as the renderer shapes it.
fn width_of<P: core_text::Paragraph<Font = Font>>(content: &str, size: f32, font: Font) -> f32 {
    P::with_text(core_text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: LineHeight::default(),
        font,
        align_x: core_text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
    })
    .min_width()
}

/// The longest prefix of `full` that fits `max` with the mark after it —
/// `full` itself when the whole of it fits, the mark alone when nothing
/// does. `measure` is the renderer's width of a candidate.
pub(crate) fn fit(full: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(full) <= max {
        return full.to_string();
    }
    // Every candidate, shortest first: the label cut after each character,
    // any space before the cut dropped, the mark after it.
    let candidates: Vec<String> = full
        .char_indices()
        .skip(1)
        .map(|(i, _)| i)
        .chain([full.len()])
        .filter_map(|cut| full.get(..cut))
        .map(|prefix| format!("{}{MARK}", prefix.trim_end()))
        .collect();
    // The widths only grow with the prefix, so the ones that fit are a run
    // at the front, and the longest of them is where that run ends.
    let fits = candidates.partition_point(|c| measure(c) <= max);
    fits.checked_sub(1)
        .and_then(|last| candidates.get(last))
        .cloned()
        .unwrap_or_else(|| MARK.to_string())
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Ellipsis
where
    Renderer: core_text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph> {
            fitted: None,
            plain: text_widget::State::<Renderer::Paragraph>::default(),
        })
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width(), Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        let room = self.leave.as_ref().map_or(0.0, |l| {
            l.texts
                .iter()
                .map(|(s, size, font)| width_of::<Renderer::Paragraph>(s, *size, *font))
                .sum::<f32>()
                + l.px
        });
        let max = (limits.max().width - room).max(0.0);
        let shown = match &state.fitted {
            Some((full, width, shown)) if *full == self.full && *width == max => shown.clone(),
            _ => {
                let (size, font) = (self.size, self.font);
                let shown = fit(&self.full, max, |s| {
                    width_of::<Renderer::Paragraph>(s, size, font)
                });
                state.fitted = Some((self.full.clone(), max, shown.clone()));
                shown
            }
        };
        text_widget::layout(
            &mut state.plain,
            renderer,
            limits,
            &shown,
            text_widget::Format {
                width: self.width(),
                height: Length::Shrink,
                size: Some(Pixels(self.size)),
                font: Some(self.font),
                line_height: self.line_height,
                align_x: core_text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
            },
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        text_widget::draw(
            renderer,
            defaults,
            layout.bounds(),
            state.plain.raw(),
            text_widget::Style { color: self.color },
            viewport,
        );
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        // Found by what it IS, not by what fitted.
        operation.text(None, layout.bounds(), &self.full);
    }
}

impl<'a, Message, Theme, Renderer> From<Ellipsis> for Element<'a, Message, Theme, Renderer>
where
    Renderer: core_text::Renderer<Font = Font> + 'a,
{
    fn from(e: Ellipsis) -> Self {
        Element::new(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One unit per character: the search on its own, without a renderer.
    fn chars(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn a_label_that_fits_is_left_whole() {
        assert_eq!(fit("Melee", 5.0, chars), "Melee");
        assert_eq!(fit("", 0.0, chars), "");
    }

    #[test]
    fn a_label_that_does_not_fit_keeps_the_longest_prefix_and_the_mark() {
        assert_eq!(fit("Fel Firebolt (Wild Imp)", 10.0, chars), "Fel Fireb…");
        // A cut after a space does not leave the space before the mark.
        assert_eq!(fit("Fel Firebolt", 5.0, chars), "Fel…");
        // Characters, not bytes: an accented name is cut between letters.
        assert_eq!(fit("Akanôs-Nebula", 6.0, chars), "Akanô…");
        assert_eq!(fit("Akanôs", 1.0, chars), "…");
        assert_eq!(fit("Akanôs", 0.0, chars), "…");
    }

    /// Drawn, the label ends in the mark at a narrow width, and is found by
    /// its whole text either way.
    #[test]
    fn it_is_found_by_its_whole_label_and_drawn_short() {
        let label = "Coalesced Venom (Zul'jan the Unending)";
        let el: Element<'static, ()> = iced::widget::container(ellipsis(label).size(14.0))
            .width(80.0)
            .into();
        let mut ui = crate::window::testkit::simulator_as(
            crate::window::settings(),
            iced::Size::new(80.0, 20.0),
            el,
        );
        let found = ui.find(label).expect("found by its whole label");
        assert!(found.bounds().width <= 80.5);
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
    }

    /// A label that leaves room is as wide as what it shows, so what
    /// follows sits right after a short one — and a long one gives way
    /// before what follows is pushed off the line.
    #[test]
    fn a_label_leaving_room_keeps_what_follows_on_its_line() {
        let line = |label: &str| -> Element<'static, ()> {
            iced::widget::row![
                ellipsis(label).size(14.0).leaving(Vec::new(), 30.0),
                iced::widget::text("TAG").size(12.0).width(30.0),
                iced::widget::Space::new().width(iced::Length::Fill),
            ]
            .width(160.0)
            .into()
        };
        let at = |label: &str| {
            let mut ui = crate::window::testkit::simulator_as(
                crate::window::settings(),
                iced::Size::new(160.0, 20.0),
                line(label),
            );
            let name = ui.find(label).expect("the label").bounds();
            let tag = ui.find("TAG").expect("what follows").bounds();
            (name, tag)
        };
        let (name, tag) = at("Cid");
        assert!(name.width < 60.0, "as wide as it reads: {name:?}");
        assert!(
            (tag.x - (name.x + name.width)).abs() < 1.0,
            "right after it"
        );
        let (name, tag) = at("Coalesced Venom (Zul'jan the Unending)");
        assert!(name.width <= 130.5, "{name:?}");
        assert!(tag.x + tag.width <= 160.5, "still on the line: {tag:?}");
    }
}
