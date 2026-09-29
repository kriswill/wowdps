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
//!
//! A label can carry a quieter second run after it — a pet's name after the
//! ability it cast ([`Ellipsis::tail`]) — and the two end in ONE mark, as
//! the prototype's `.an .x{text-overflow:ellipsis}` span ends: the tail
//! gives way first, and whole, once fewer than [`TAIL_MIN`] of its letters
//! would show, so a row never ends in a bare "…" after a name that fit.
//! Two other ways a tail gives way ([`Give`]): whole or not at all (a
//! source cut to four letters saves nothing), or down to its mark before
//! the label loses a letter (the label is what the column is about).

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text as core_text;
use iced::advanced::widget::text as text_widget;
use iced::advanced::widget::{self, Tree, Widget, tree};
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Color, Element, Font, Length, Pixels, Point, Rectangle, Size, mouse};

/// What stands in for the rest of a label that does not fit.
const MARK: &str = "…";
/// The fewest letters of a tail worth showing: under this it is dropped
/// whole, the name alone ending the line.
pub(crate) const TAIL_MIN: usize = 3;

/// A one-line label, ellipsised to its width.
pub(crate) struct Ellipsis {
    full: String,
    size: f32,
    font: Font,
    color: Option<Color>,
    line_height: LineHeight,
    /// What the line keeps after the label ([`Ellipsis::leaving`]).
    leave: Option<Leave>,
    /// A quieter run after the label ([`Ellipsis::tail`]).
    tail: Option<Tail>,
}

/// Room a label leaves after it: `texts`, each measured at layout in its
/// own size and font, and `px` more — fixed widths and the gaps between.
struct Leave {
    texts: Vec<(String, f32, Font)>,
    px: f32,
}

/// A second run after the label, `gap` after it, in its own size and ink.
struct Tail {
    text: String,
    size: f32,
    color: Color,
    gap: f32,
    give: Give,
}

/// How a tail gives way when the line cannot hold it and its label whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Give {
    /// Cut while [`TAIL_MIN`] of its letters show, then dropped whole and
    /// the label cut: a pet's name after the ability it cast.
    Cut,
    /// Whole or not at all, the label then alone: a name cut to a few
    /// letters ("Zul'j…") saves almost nothing and reads as another name.
    Whole,
    /// Cut down to its mark before the label gives a letter — the label is
    /// what the line is about (the Deaths table's killing blow, its source
    /// and rez after it: the prototype's `.src` span shrinking to nothing
    /// beside an unshrinkable blow). Dropped only when not even the mark
    /// fits; the label is cut only then.
    First,
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
        tail: None,
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

    /// A quieter run after the label, `gap` px after it, at `size` in
    /// `color` (a pet's name after its ability, "Fel Firebolt" + "Wild
    /// Imp"): the two share the line's one mark ([`fit_pair`]).
    pub(crate) fn tail(
        mut self,
        text: impl Into<String>,
        size: f32,
        color: Color,
        gap: f32,
    ) -> Self {
        self.tail = Some(Tail {
            text: text.into(),
            size,
            color,
            gap,
            give: Give::Cut,
        });
        self
    }

    /// How the tail gives way ([`Give`]; [`Give::Cut`] unless said).
    pub(crate) fn giving(mut self, give: Give) -> Self {
        if let Some(t) = self.tail.as_mut() {
            t.give = give;
        }
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

    /// The paragraph format for a run at `size` in `font`, `width` wide.
    fn format(&self, width: Length, size: f32, font: Font) -> text_widget::Format<Font> {
        text_widget::Format {
            width,
            height: Length::Shrink,
            size: Some(Pixels(size)),
            font: Some(font),
            line_height: self.line_height,
            align_x: core_text::Alignment::Default,
            align_y: iced::alignment::Vertical::Top,
            shaping: Shaping::Advanced,
            wrapping: Wrapping::None,
        }
    }
}

/// What a label and its tail were last fitted to: the label, the tail's
/// text, the width — and what was shown of each.
type Fitted = (String, String, f32, String, Option<String>);

/// The label as it was last fitted, so a relayout at the same width with the
/// same words shapes nothing new.
struct State<P: core_text::Paragraph> {
    fitted: Option<Fitted>,
    plain: text_widget::State<P>,
    tail: text_widget::State<P>,
}

/// `content`'s one-line width at `size` in `font`, as the renderer shapes it.
pub(crate) fn width_of<P: core_text::Paragraph<Font = Font>>(
    content: &str,
    size: f32,
    font: Font,
) -> f32 {
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
    // any space or comma before the cut dropped ("Zul'jan…", never
    // "Zul'jan,…"), the mark after it.
    let candidates: Vec<String> = full
        .char_indices()
        .skip(1)
        .map(|(i, _)| i)
        .chain([full.len()])
        .filter_map(|cut| full.get(..cut))
        .map(|prefix| {
            let kept = prefix.trim_end_matches(|c: char| c.is_whitespace() || c == ',');
            format!("{kept}{MARK}")
        })
        .collect();
    // The widths only grow with the prefix, so the ones that fit are a run
    // at the front, and the longest of them is where that run ends.
    let fits = candidates.partition_point(|c| measure(c) <= max);
    fits.checked_sub(1)
        .and_then(|last| candidates.get(last))
        .cloned()
        .unwrap_or_else(|| MARK.to_string())
}

/// A label and its tail in `max`, `gap` between them, ending in one mark:
/// both whole when they fit; else as `give` says ([`Give`]) — by default
/// the label whole and the tail cut when at least [`TAIL_MIN`] of its
/// letters still show, else the tail dropped and the label alone fitted
/// ([`fit`]). `name_w` and `tail_w` measure a candidate in each run's own
/// size.
pub(crate) fn fit_pair(
    name: &str,
    tail: &str,
    max: f32,
    gap: f32,
    give: Give,
    name_w: impl Fn(&str) -> f32,
    tail_w: impl Fn(&str) -> f32,
) -> (String, Option<String>) {
    let room = max - name_w(name) - gap;
    if tail_w(tail) <= room {
        return (name.to_string(), Some(tail.to_string()));
    }
    let cut = match give {
        Give::Whole => false,
        Give::First => tail_w(MARK) <= room,
        Give::Cut => {
            let least: String = tail.chars().take(TAIL_MIN).collect();
            tail.chars().count() > TAIL_MIN
                && tail_w(&format!("{}{MARK}", least.trim_end())) <= room
        }
    };
    if cut {
        return (name.to_string(), Some(fit(tail, room, tail_w)));
    }
    (fit(name, max, name_w), None)
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
            tail: text_widget::State::<Renderer::Paragraph>::default(),
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
        let tail_text = self.tail.as_ref().map_or("", |t| t.text.as_str());
        let (shown, tail_shown) = match &state.fitted {
            Some((full, tail, width, shown, tail_shown))
                if *full == self.full && tail == tail_text && *width == max =>
            {
                (shown.clone(), tail_shown.clone())
            }
            _ => {
                let (size, font) = (self.size, self.font);
                let name_w = |s: &str| width_of::<Renderer::Paragraph>(s, size, font);
                let (shown, tail_shown) = match &self.tail {
                    Some(t) => fit_pair(&self.full, &t.text, max, t.gap, t.give, name_w, |s| {
                        width_of::<Renderer::Paragraph>(s, t.size, crate::theme::UI)
                    }),
                    None => (fit(&self.full, max, name_w), None),
                };
                state.fitted = Some((
                    self.full.clone(),
                    tail_text.to_string(),
                    max,
                    shown.clone(),
                    tail_shown.clone(),
                ));
                (shown, tail_shown)
            }
        };
        let (Some(tail), Some(tail_shown)) = (&self.tail, tail_shown) else {
            let format = self.format(self.width(), self.size, self.font);
            return text_widget::layout(&mut state.plain, renderer, limits, &shown, format);
        };
        // Both runs at their own widths, the tail after the label, their
        // feet on one line (the label's the taller box).
        let loose = layout::Limits::new(Size::ZERO, limits.max());
        let name = text_widget::layout(
            &mut state.plain,
            renderer,
            &loose,
            &shown,
            self.format(Length::Shrink, self.size, self.font),
        );
        let after = text_widget::layout(
            &mut state.tail,
            renderer,
            &loose,
            &tail_shown,
            self.format(Length::Shrink, tail.size, crate::theme::UI),
        );
        let (n, a) = (name.size(), after.size());
        let height = n.height.max(a.height);
        let size = limits.resolve(
            self.width(),
            Length::Shrink,
            Size::new(n.width + tail.gap + a.width, height),
        );
        layout::Node::with_children(
            size,
            vec![
                name.move_to(Point::new(0.0, height - n.height)),
                after.move_to(Point::new(n.width + tail.gap, height - a.height)),
            ],
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
        let mut runs = layout.children();
        match (runs.next(), runs.next(), &self.tail) {
            (Some(name), Some(after), Some(tail)) => {
                text_widget::draw(
                    renderer,
                    defaults,
                    name.bounds(),
                    state.plain.raw(),
                    text_widget::Style { color: self.color },
                    viewport,
                );
                text_widget::draw(
                    renderer,
                    defaults,
                    after.bounds(),
                    state.tail.raw(),
                    text_widget::Style {
                        color: Some(tail.color),
                    },
                    viewport,
                );
            }
            _ => text_widget::draw(
                renderer,
                defaults,
                layout.bounds(),
                state.plain.raw(),
                text_widget::Style { color: self.color },
                viewport,
            ),
        }
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        // Found by what it IS, not by what fitted: the label where it is
        // drawn, and a tail — while it is shown — by its own words.
        let mut runs = layout.children();
        match (runs.next(), runs.next(), &self.tail) {
            (Some(name), Some(after), Some(tail)) => {
                operation.text(None, name.bounds(), &self.full);
                operation.text(None, after.bounds(), &tail.text);
            }
            _ => operation.text(None, layout.bounds(), &self.full),
        }
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
        // Nor a comma.
        assert_eq!(fit("Zul'jan, rezzed 2:03", 9.0, chars), "Zul'jan…");
        // Characters, not bytes: an accented name is cut between letters.
        assert_eq!(fit("Akanôs-Nebula", 6.0, chars), "Akanô…");
        assert_eq!(fit("Akanôs", 1.0, chars), "…");
        assert_eq!(fit("Akanôs", 0.0, chars), "…");
    }

    /// A label and its tail end in one mark: both whole when they fit, the
    /// tail cut while at least three of its letters show, and — with less
    /// room than that — the tail gone and the label whole or cut, never a
    /// bare mark after a name that fit.
    #[test]
    fn a_tail_gives_way_first_and_whole() {
        let pair = |max| {
            fit_pair(
                "Fel Firebolt",
                "Wild Imp",
                max,
                1.0,
                Give::Cut,
                chars,
                chars,
            )
        };
        assert_eq!(pair(21.0), ("Fel Firebolt".into(), Some("Wild Imp".into())));
        assert_eq!(pair(20.0), ("Fel Firebolt".into(), Some("Wild I…".into())));
        assert_eq!(pair(17.0), ("Fel Firebolt".into(), Some("Wil…".into())));
        assert_eq!(pair(16.0), ("Fel Firebolt".into(), None), "the name fits");
        assert_eq!(pair(12.0), ("Fel Firebolt".into(), None));
        assert_eq!(pair(9.0), ("Fel Fire…".into(), None), "cut at a letter");
        // A tail no longer than the least worth showing is whole or gone.
        assert_eq!(
            fit_pair("Bite", "Pet", 7.0, 1.0, Give::Cut, chars, chars),
            ("Bite".into(), None)
        );
    }

    /// A source that gives way WHOLE is shown whole or not at all — never
    /// cut to a few letters of a name.
    #[test]
    fn a_whole_tail_is_whole_or_gone() {
        let pair = |max| {
            fit_pair(
                "Gravebound",
                "Hex Lord Malakro",
                max,
                1.0,
                Give::Whole,
                chars,
                chars,
            )
        };
        assert_eq!(
            pair(27.0),
            ("Gravebound".into(), Some("Hex Lord Malakro".into()))
        );
        assert_eq!(pair(26.0), ("Gravebound".into(), None), "never cut");
        assert_eq!(pair(14.0), ("Gravebound".into(), None));
        assert_eq!(pair(6.0), ("Grave…".into(), None));
    }

    /// A tail that gives way FIRST is cut down to its mark before its label
    /// loses a letter: the killing blow stays whole while its source and
    /// rez have any room to give.
    #[test]
    fn a_first_tail_gives_every_letter_before_the_label_one() {
        let pair = |max| {
            fit_pair(
                "Venom Rupture",
                "Zul'jan, rezzed 2:03",
                max,
                1.0,
                Give::First,
                chars,
                chars,
            )
        };
        assert_eq!(
            pair(34.0),
            ("Venom Rupture".into(), Some("Zul'jan, rezzed 2:03".into()))
        );
        assert_eq!(
            pair(26.0),
            ("Venom Rupture".into(), Some("Zul'jan, re…".into()))
        );
        assert_eq!(pair(17.0), ("Venom Rupture".into(), Some("Zu…".into())));
        // Down to the mark alone, and only then the label.
        assert_eq!(pair(15.0), ("Venom Rupture".into(), Some("…".into())));
        assert_eq!(pair(14.0), ("Venom Rupture".into(), None));
        assert_eq!(pair(9.0), ("Venom Ru…".into(), None));
    }

    /// Drawn, a pet's name follows its ability on one line, both found by
    /// their own words; squeezed, the pet goes and the ability keeps its
    /// letters whole.
    #[test]
    fn a_tail_is_drawn_after_its_label() {
        let line = |w: f32| -> Element<'static, ()> {
            iced::widget::container(ellipsis("Burning Cleave").size(14.0).tail(
                "Demonic Tyrant",
                13.0,
                Color::WHITE,
                5.0,
            ))
            .width(w)
            .into()
        };
        let at = |w: f32| {
            crate::window::testkit::simulator_as(
                crate::window::settings(),
                iced::Size::new(w, 20.0),
                line(w),
            )
        };
        let mut ui = at(300.0);
        let name = ui.find("Burning Cleave").expect("the ability").bounds();
        let pet = ui.find("Demonic Tyrant").expect("the pet").bounds();
        assert!(pet.x >= name.x + name.width, "{name:?} then {pet:?}");
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
        let mut ui = at(100.0);
        assert!(ui.find("Demonic Tyrant").is_err(), "no room: the pet goes");
        assert!(ui.find("Burning Cleave").is_ok());
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
