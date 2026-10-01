//! The pull rail (the prototype's `.rail`): ONE list of pulls, tonight's
//! log and every stored night, where the window used to keep a fight list
//! for the log and a History screen for the store — two lists with two
//! shapes, and four controls that moved between pulls, each over a
//! different scope.
//!
//! Nights by their local date ("Tonight", "Saturday, Sep 26"), the visits
//! in each (an instance and its difficulty, your keys, a delve) wearing a
//! dot in the colour of the character who played them, and the pulls newest
//! first: an outcome glyph (✓ kill or timed, ✕ wipe or over time, a dash for
//! trash, Σ for a whole visit, a red dot while live), the name, and at the
//! right a wipe's best %, a key's +N or "over", a dot where one visit holds
//! several characters, and the duration. Tonight comes from the daemon's
//! segment list; the earlier nights from the history store's pages
//! (`history::Earlier`). A stored card that is ALSO a segment of the tailed
//! log — its id is the log's id and the row's start — is listed once, as
//! the log's, and lends that row what only the store knows (a wipe's best
//! health, whose pull it was).
//!
//! Above 1180 px the rail stands at the window's left, 236 px wide; at 1180
//! and under it is a 280 px drawer over a scrim, opened from the fight
//! header's list button (or `H`) and closed by the scrim, Esc, Enter or a
//! pick. `[` and `]` walk it, stored nights included — the drawer left open
//! on the row they reach — and while it is open j, k and the arrows walk a
//! highlight over its rows for Enter to open. Window-only.

// The model is gui-logic's (`rail`); this module draws it.
pub(crate) use wowdps_gui_logic::rail::*;

use iced::advanced::widget::operation::{self, Operation, Outcome};
use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{
    Space, button, column, container, mouse_area, opaque, row, scrollable, stack, text,
};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::ellipsis::ellipsis;
use crate::line_icons::{LineIcon, line_icon};
use crate::nav;
use crate::theme::{self, size};
use crate::window::Message;
// A name as the window draws it: gui-logic's.
pub(crate) use wowdps_gui_logic::labels::shown_name;

/// The rail beside the stage (`.body{grid-template-columns:236px …}`) and
/// the drawer it becomes (`.rail{width:280px}` under 1180 px).
pub(crate) const RAIL_W: f32 = 236.0;
pub(crate) const DRAWER_W: f32 = 280.0;
/// A pull's row (`.pull{padding:4px 12px 4px 14px;line-height:20px}`), and
/// its lead glyph's box and the glyph in it (`.oc`, `.oc svg`).
const PULL_H: f32 = 28.0;
const PULL_PAD: iced::Padding = iced::Padding {
    top: 4.0,
    right: 12.0,
    bottom: 4.0,
    left: 14.0,
};
const PULL_GAP: f32 = 8.0;
const MARK_BOX: f32 = 16.0;
const MARK_ICON: f32 = 14.0;
/// A pull's name (14.5 px; trash 13.5 in the faint ink), what sits at its
/// right (`.rt{gap:7px;font-size:13.5px}`), a character's dot (7 px).
const NAME_PX: f32 = 14.5;
const TRASH_PX: f32 = 13.5;
const RIGHT_PX: f32 = 13.5;
const RIGHT_GAP: f32 = 7.0;
const DOT: f32 = 7.0;
/// The current pull's edge (`box-shadow:inset 2px 0 0 var(--accent)`),
/// and the ring on the row the drawer's keys are on.
const EDGE_W: f32 = 2.0;
const CURSOR_RING: f32 = 1.0;
const CURSOR_RADIUS: f32 = 3.0;
/// The head (`.rail-head{gap:6px;padding:10px 8px 8px 14px}`, `h2{15px
/// 600}`) and the trash toggle (`.toggle{13px;padding:2px 6px;
/// border-radius:4px}`), pressed in the raise and the second ink
/// (`.toggle[aria-pressed=true]`) — its words and its width as they were.
const HEAD_PAD: iced::Padding = iced::Padding {
    top: 10.0,
    right: 8.0,
    bottom: 8.0,
    left: 14.0,
};
const HEAD_GAP: f32 = 6.0;
const HEAD_PX: f32 = 15.0;
const TOGGLE_PX: f32 = 13.0;
const TOGGLE_PAD: [f32; 2] = [2.0, 6.0];
const TOGGLE_RADIUS: f32 = 4.0;
/// A night (`.night{padding-top:12px}`, `h3{13px 600;padding:0 14px 2px}`)
/// and a visit (`.visit{padding:6px 14px 3px;gap:7px;font-size:13px}`).
const NIGHT_PAD: iced::Padding = iced::Padding {
    top: 12.0,
    right: 14.0,
    bottom: 2.0,
    left: 14.0,
};
const LABEL_PX: f32 = 13.0;
const VISIT_PAD: iced::Padding = iced::Padding {
    top: 6.0,
    right: 14.0,
    bottom: 3.0,
    left: 14.0,
};
const VISIT_GAP: f32 = 7.0;
/// The trash dash (`.oc .dash{width:6px;height:1.5px;border-radius:1px}`).
const DASH_W: f32 = 6.0;
const DASH_H: f32 = 1.5;
const DASH_RADIUS: f32 = 1.0;
/// "Show older nights" (`.rail-more{margin:14px 14px 0;padding:6px 10px;
/// border:1px dashed;border-radius:6px;font-size:13.5px}`) — its dash and
/// gap, near a browser's for a 1 px dashed edge — and the list's foot
/// (`.rail-scroll{padding-bottom:14px}`).
const MORE_MARGIN: f32 = 14.0;
const MORE_PAD: [f32; 2] = [6.0, 10.0];
const MORE_RADIUS: f32 = 6.0;
const MORE_EDGE: f32 = 1.0;
const MORE_DASH: [f32; 2] = [4.0, 3.0];
const FOOT: f32 = 14.0;
/// The list's scrollbar (`::-webkit-scrollbar{width:10px}`, its thumb
/// `border:2px solid transparent;background-clip:padding-box`): a 10 px
/// lane with a 6 px thumb down its middle.
const SCROLL_LANE: f32 = 10.0;
const SCROLL_THUMB: f32 = 6.0;
/// The pin's mark on a row — in the row's 14 px left gutter, 2 px clear of
/// the current row's 2 px edge, in the quiet ink: the one gold at the
/// rail's edge is the current row's — and what it says under the pointer.
pub(crate) const PIN: &str = "★";
const PIN_PX: f32 = 11.0;
const PIN_INSET: f32 = 4.0;
pub(crate) const PIN_TIP: &str = "Pinned: retention keeps it (p)";
/// The drawer's shadow (`box-shadow:20px 0 50px rgba(0,0,0,.5)`).
const DRAWER_SHADOW: iced::Shadow = iced::Shadow {
    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
    offset: iced::Vector::new(20.0, 0.0),
    blur_radius: 50.0,
};

// ---- drawing -----------------------------------------------------------------------

/// The rail's scrollable, the current pull's row, the first earlier
/// night's heading and the drawer's scrim, for the window's operations and
/// its tests.
pub(crate) fn scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("rail")
}
pub(crate) fn current_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-current")
}
pub(crate) fn earlier_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-earlier")
}
pub(crate) fn scrim_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-scrim")
}
/// The drawer's keyboard highlight, and the heading of the night the pull
/// on the stage is listed under.
pub(crate) fn cursor_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-cursor")
}
pub(crate) fn current_night_id() -> iced::widget::Id {
    iced::widget::Id::new("rail-current-night")
}

/// What the rail draws beside its model.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shown {
    /// The pull on the stage.
    pub at: Option<Pull>,
    /// The row the drawer's keys are on (j, k, the arrows; Enter opens it).
    pub cursor: Option<Pull>,
    pub hide_trash: bool,
    pub more: More,
    pub accent: theme::Accent,
    /// Names in the dots' tips go without their realm, as every name does.
    pub hide_realms: bool,
}

impl Shown {
    /// Does the rail draw `l`? Trash goes when the toggle says so — but
    /// never the pull on the stage, nor the row the drawer's keys are on,
    /// which keep their places.
    fn shows(&self, l: &Line) -> bool {
        !(self.hide_trash && l.trash)
            || self.at.as_ref() == Some(&l.pull)
            || self.cursor.as_ref() == Some(&l.pull)
    }
}

/// What the rail says with nothing to list.
pub(crate) const EMPTY: &str =
    "No pulls yet. Pulls appear here as you fight, and earlier nights load from your history.";

/// The rail, `width` wide (`.rail`): its head — "Pulls" and the trash
/// toggle — over the nights, on the panel's surface.
pub(crate) fn panel(rail: &Rail, shown: &Shown, width: f32) -> Element<'static, Message> {
    let on = shown.hide_trash;
    // Pressed, the toggle is raised and its words in the second ink
    // (`.toggle[aria-pressed=true]`): the same words at the same width, so
    // nothing on the head moves when it is pressed.
    let toggle = button(text("Hide trash").size(TOGGLE_PX))
        .padding(TOGGLE_PAD)
        .on_press(Message::HideTrash)
        .style(move |_: &Theme, _| button::Style {
            background: on.then(|| theme::RAISE.into()),
            text_color: if on { theme::INK_2 } else { theme::INK_3_TEXT },
            border: iced::border::rounded(TOGGLE_RADIUS),
            ..button::Style::default()
        });
    let head = container(
        row![
            text("Pulls")
                .size(HEAD_PX)
                .font(theme::UI_SEMIBOLD)
                .color(theme::INK),
            Space::new().width(Length::Fill),
            toggle,
        ]
        .spacing(HEAD_GAP)
        .align_y(iced::Alignment::Center),
    )
    .padding(HEAD_PAD)
    .width(Length::Fill);
    let earlier = rail.earlier();
    let mut list = column![];
    if rail.nights.is_empty() {
        list = list
            .push(container(text(EMPTY).size(size::SMALL).color(theme::INK_2)).padding(NIGHT_PAD));
    }
    for (i, n) in rail.nights.iter().enumerate() {
        // The visits the toggle leaves anything of: a night whose every
        // pull it hides is no heading over nothing.
        let visits: Vec<(&Visit, Vec<&Line>)> = n
            .visits
            .iter()
            .map(|v| (v, v.lines.iter().filter(|l| shown.shows(l)).collect()))
            .filter(|(_, lines): &(&Visit, Vec<&Line>)| !lines.is_empty())
            .collect();
        if visits.is_empty() {
            continue;
        }
        let heading = container(
            text(n.label.clone())
                .size(LABEL_PX)
                .font(theme::UI_SEMIBOLD)
                .color(theme::GOLD_DIM),
        )
        .padding(NIGHT_PAD)
        .width(Length::Fill);
        let heading = if earlier == Some(i) {
            heading.id(earlier_id())
        } else {
            heading
        };
        // The night the pull on the stage is under: where the drawer
        // opening on it may stand its heading at the top.
        let holds_current = shown.at.as_ref().is_some_and(|at| {
            visits
                .iter()
                .any(|(_, ls)| ls.iter().any(|l| l.pull == *at))
        });
        list = list.push(if holds_current {
            container(heading)
                .id(current_night_id())
                .width(Length::Fill)
        } else {
            heading
        });
        for (v, lines) in visits {
            list = list.push(visit_line(v, shown.hide_realms));
            for l in lines {
                list = list.push(pull_line(
                    l,
                    shown.at.as_ref() == Some(&l.pull),
                    shown.cursor.as_ref() == Some(&l.pull),
                    shown.accent,
                    shown.hide_realms,
                ));
            }
        }
    }
    match shown.more {
        More::None => {}
        More::Offer => list = list.push(more_button(true)),
        More::Asking => list = list.push(more_button(false)),
    }
    let list = list.push(Space::new().height(Length::Fixed(FOOT)));
    container(
        column![
            head,
            nav::hairline::<Message>(),
            scrollable(crate::view::scroll_clear(list.width(Length::Fill)))
                .id(scroll_id())
                // The rail's own thumb: 6 px down the middle of its 10 px
                // lane (the prototype's padding-box thumb), rail-local so
                // every other list keeps its own.
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(SCROLL_LANE)
                        .scroller_width(SCROLL_THUMB),
                ))
                .height(Length::Fill)
                .width(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(|_: &Theme| container::Style {
        background: Some(theme::SURFACE.into()),
        ..container::Style::default()
    })
    .into()
}

/// The rail over `stage` as a drawer (`.app.rail-open .rail`): the stage
/// under a scrim that closes it, the rail at the left with its shadow.
/// The scrim holds the pointer (the arrow, `Idle`, is an interaction a
/// stack stops at), so the rows under it light for no one; the rail is
/// opaque to it, so a press on a heading, a visit's line or the empty foot
/// of a short list is the rail's, and never reaches the scrim under it.
pub(crate) fn drawer(
    stage: Element<'static, Message>,
    rail: Element<'static, Message>,
) -> Element<'static, Message> {
    let scrim = mouse_area(
        container(Space::new())
            .id(scrim_id())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::RAIL_SCRIM.into()),
                ..container::Style::default()
            }),
    )
    .interaction(iced::mouse::Interaction::Idle)
    .on_press(Message::CloseRail);
    let rail = opaque(
        container(rail)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::SURFACE.into()),
                shadow: DRAWER_SHADOW,
                ..container::Style::default()
            }),
    );
    stack![stage, scrim, rail]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A character's dot, saying under the pointer whose it is.
fn dot(who: &Who, hide_realms: bool) -> Element<'static, Message> {
    nav::tip(
        nav::dot::<Message>(
            Color::from_rgba(who.color.r, who.color.g, who.color.b, who.color.a),
            DOT,
        ),
        shown_name(&who.name, hide_realms),
    )
}

/// A visit's line (`.visit`): the character's dot and what it was.
fn visit_line(v: &Visit, hide_realms: bool) -> Element<'static, Message> {
    let mut line = row![].spacing(VISIT_GAP).align_y(iced::Alignment::Center);
    if let Some(who) = &v.dot {
        line = line.push(dot(who, hide_realms));
    }
    line = line.push(
        ellipsis(v.title.clone())
            .size(LABEL_PX)
            .color(theme::INK_3_TEXT),
    );
    container(line)
        .padding(VISIT_PAD)
        .width(Length::Fill)
        .into()
}

/// A pull's name, where the line lets it be: a key's level ("+14", the one
/// thing between two runs of a dungeon) is kept whole after its dungeon's
/// name, which is what gives way to "…".
fn pull_name(name: &str, px: f32, ink: Color) -> Element<'static, Message> {
    let dungeon = crate::home::dungeon_name(name);
    let level = name.get(dungeon.len()..).unwrap_or_default();
    if level.is_empty() {
        return ellipsis(name.to_string()).size(px).color(ink).into();
    }
    container(
        row![
            ellipsis(dungeon.to_string())
                .size(px)
                .color(ink)
                .leaving(vec![(level.to_string(), px, theme::UI)], 0.0),
            text(level.to_string())
                .size(px)
                .color(ink)
                .wrapping(text::Wrapping::None),
        ]
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .into()
}

/// A pull's row (`.pull`): its glyph, its name, and at its right what it
/// came to — lit and edged in the accent when it is on the stage, ringed
/// in it when the drawer's keys are on another row (`keyed`).
fn pull_line(
    l: &Line,
    current: bool,
    keyed: bool,
    accent: theme::Accent,
    hide_realms: bool,
) -> Element<'static, Message> {
    let (px, ink) = match (l.trash, l.mark) {
        (true, _) => (TRASH_PX, theme::INK_3_TEXT),
        (_, Mark::Sum) => (NAME_PX, theme::INK_2),
        _ => (NAME_PX, theme::INK),
    };
    let quiet = |s: String| {
        text(s)
            .size(RIGHT_PX)
            .color(theme::INK_2)
            .wrapping(text::Wrapping::None)
    };
    let mut right = row![].spacing(RIGHT_GAP).align_y(iced::Alignment::Center);
    if let Some(pct) = l.best_pct {
        right = right.push(quiet(format!("{pct}%")));
    }
    match l.key {
        Some(KeyWord::Plus(n)) => {
            right = right.push(
                text(format!("+{n}"))
                    .size(RIGHT_PX)
                    .font(theme::UI_SEMIBOLD)
                    .color(theme::GOOD),
            );
        }
        Some(KeyWord::Over) => {
            right = right.push(
                text("over")
                    .size(RIGHT_PX)
                    .font(theme::UI_SEMIBOLD)
                    .color(theme::BAD),
            );
        }
        None => {}
    }
    if let Some(who) = &l.dot {
        right = right.push(dot(who, hide_realms));
    }
    right = right.push(quiet(clock(l.duration_ms)));
    let body = container(
        row![mark(l.mark), pull_name(&l.name, px, ink), right]
            .spacing(PULL_GAP)
            .align_y(iced::Alignment::Center),
    )
    .padding(PULL_PAD)
    .height(Length::Fixed(PULL_H))
    .width(Length::Fill)
    .align_y(iced::Alignment::Center);
    let edge = container(Space::new())
        .width(Length::Fixed(EDGE_W))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: current.then(|| accent.base.into()),
            ..container::Style::default()
        });
    // A pinned card's star stands in the row's left gutter, before its
    // glyph: a shape that takes nothing from the name, where the name and
    // its right cluster already share 236 px (the store behind the shots
    // pins 117 of its 802 cards) — in the quiet ink, so the one gold at the
    // rail's edge is the current row's.
    let mut layers = stack![body, edge];
    if l.pinned {
        layers = layers.push(
            container(nav::tip(
                text(PIN)
                    .size(PIN_PX)
                    .color(theme::INK_3)
                    .wrapping(text::Wrapping::None),
                PIN_TIP,
            ))
            .padding(iced::Padding {
                left: PIN_INSET,
                ..iced::Padding::ZERO
            })
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
        );
    }
    let face = container(layers).width(Length::Fill);
    let face = if current { face.id(current_id()) } else { face };
    let face = if keyed {
        container(face).id(cursor_id()).width(Length::Fill)
    } else {
        face
    };
    button(face)
        .padding(0)
        .width(Length::Fill)
        .on_press(Message::Pull(l.pull.clone()))
        .style(move |_: &Theme, status| button::Style {
            background: if current {
                Some(theme::RAISE.into())
            } else {
                (keyed || matches!(status, button::Status::Hovered | button::Status::Pressed))
                    .then(|| theme::HOVER.into())
            },
            text_color: theme::INK,
            // The keys' row, once they leave the pull on the stage (whose
            // row is lit already): a focus ring, the accent's other use.
            border: if keyed && !current {
                iced::Border {
                    color: accent.base,
                    width: CURSOR_RING,
                    radius: CURSOR_RADIUS.into(),
                }
            } else {
                iced::Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

/// A row's lead glyph in its 16 px box.
pub(crate) fn mark(m: Mark) -> Element<'static, Message> {
    let glyph: Element<'static, Message> = match m {
        Mark::Good => line_icon(LineIcon::Check, MARK_ICON, theme::GOOD),
        Mark::Bad => line_icon(LineIcon::Close, MARK_ICON, theme::BAD),
        Mark::Live => nav::dot(theme::BAD, size::DOT),
        Mark::Sum => text("Σ").size(NAME_PX).color(theme::INK_3_TEXT).into(),
        Mark::Dash => container(Space::new())
            .width(Length::Fixed(DASH_W))
            .height(Length::Fixed(DASH_H))
            .style(|_: &Theme| container::Style {
                background: Some(theme::INK_3.into()),
                border: iced::border::rounded(DASH_RADIUS),
                ..container::Style::default()
            })
            .into(),
    };
    container(glyph).center(Length::Fixed(MARK_BOX)).into()
}

/// "Show older nights" (`.rail-more`), or the word that a page is coming:
/// framed in a dashed edge, which is what says "more to load" rather than
/// "a button like the rest". iced's borders are solid, so the dashes are a
/// canvas under the button, as large as it is.
fn more_button(offer: bool) -> Element<'static, Message> {
    let words = if offer {
        "Show older nights"
    } else {
        "Reading the history store…"
    };
    let face = button(
        text(words)
            .size(RIGHT_PX)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center),
    )
    .padding(MORE_PAD)
    .width(Length::Fill)
    .on_press_maybe(offer.then_some(Message::OlderNights))
    .style(|_: &Theme, status| button::Style {
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => theme::INK_2,
            _ => theme::INK_3_TEXT,
        },
        ..button::Style::default()
    });
    let edge = Canvas::new(DashedEdge)
        .width(Length::Fill)
        .height(Length::Fill);
    container(stack![face].push_under(edge))
        .padding(iced::Padding {
            top: MORE_MARGIN,
            right: MORE_MARGIN,
            bottom: 0.0,
            left: MORE_MARGIN,
        })
        .width(Length::Fill)
        .into()
}

/// The dashed edge of "Show older nights" (`border:1px dashed var(--edge);
/// border-radius:6px`), drawn inside its bounds.
struct DashedEdge;

impl<M> canvas::Program<M> for DashedEdge {
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
        // The stroke centred on a line half its width in, so all of it is
        // inside the bounds, as a CSS border is.
        let half = MORE_EDGE / 2.0;
        let frame_path = Path::rounded_rectangle(
            Point::new(half, half),
            Size::new(bounds.width - MORE_EDGE, bounds.height - MORE_EDGE),
            MORE_RADIUS.into(),
        );
        frame.stroke(
            &frame_path,
            Stroke {
                line_dash: canvas::LineDash {
                    segments: &MORE_DASH,
                    offset: 0,
                },
                ..Stroke::default()
                    .with_color(theme::EDGE)
                    .with_width(MORE_EDGE)
            },
        );
        vec![frame.into_geometry()]
    }
}

// ---- operations ------------------------------------------------------------------

/// Scrolls the rail so the earlier nights' heading stands at its top — `H`.
/// Nothing when the rail holds no earlier night, or is not drawn.
#[derive(Default)]
pub(crate) struct ToEarlier {
    content: Option<Rectangle>,
    found: Option<Rectangle>,
}

impl Operation for ToEarlier {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        if id == Some(&earlier_id()) {
            self.found = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        _bounds: Rectangle,
        content: Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&scroll_id()) {
            self.content = Some(content);
        }
    }

    fn finish(&self) -> Outcome<()> {
        match (self.content, self.found) {
            (Some(content), Some(found)) => Outcome::Chain(Box::new(ScrollTo {
                y: found.y - content.y,
            })),
            _ => Outcome::None,
        }
    }
}

/// Scrolls the rail to a row: [`near_offset`] after a step, or
/// [`open_offset`] as the drawer opens on it. Nothing when the row is not
/// drawn.
pub(crate) struct Reveal {
    row: iced::widget::Id,
    open: bool,
    viewport: Option<(Rectangle, Rectangle, iced::Vector)>,
    found: Option<Rectangle>,
    heading: Option<Rectangle>,
}

impl Reveal {
    /// The least scroll that shows the row `row` with the room under it.
    pub(crate) fn near(row: iced::widget::Id) -> Self {
        Reveal {
            row,
            open: false,
            viewport: None,
            found: None,
            heading: None,
        }
    }

    /// The drawer opening on the pull on the stage.
    pub(crate) fn open() -> Self {
        Reveal {
            open: true,
            ..Reveal::near(current_id())
        }
    }
}

impl Operation for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.found = Some(bounds);
        } else if self.open && id == Some(&current_night_id()) {
            self.heading = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: iced::Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&scroll_id()) {
            self.viewport = Some((bounds, content, translation));
        }
    }

    fn finish(&self) -> Outcome<()> {
        let (Some((bounds, content, at)), Some(row)) = (self.viewport, self.found) else {
            return Outcome::None;
        };
        let (top, bottom) = (row.y - content.y, row.y + row.height - content.y);
        let to = if self.open {
            open_offset(
                at.y,
                bounds.height,
                content.height,
                (top, bottom),
                self.heading.map(|h| h.y - content.y),
            )
        } else {
            near_offset(at.y, bounds.height, content.height, top, bottom)
        };
        if (to - at.y).abs() < 0.5 {
            return Outcome::None;
        }
        Outcome::Chain(Box::new(ScrollTo { y: to }))
    }
}

/// Puts the rail's content `y` at the top of its viewport.
struct ScrollTo {
    y: f32,
}

impl Operation for ScrollTo {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        _bounds: Rectangle,
        _content: Rectangle,
        _translation: iced::Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&scroll_id()) {
            state.scroll_to(iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(self.y.max(0.0)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::simulator;
    use wowdps_gui_logic::rail::samples::*;
    use wowdps_model::SegmentId;
    use wowdps_proto::history::FightKind;

    /// The rail draws its nights, its visits and their pulls: the current
    /// pull lit, trash left out when the toggle says so (but the pull on
    /// the stage kept), what a row says at its right, and the button for
    /// older nights — or the word that they are coming.
    #[test]
    fn the_panel_draws_what_the_rail_holds() {
        let start = day("2026-09-27") + 19 * H;
        let mut wipe = card(
            "w",
            FightKind::Encounter,
            "The Lost Explorers",
            start - 24 * H,
            454_000,
        );
        wipe.success = Some(false);
        wipe.best_pct = Some(97);
        let rail = build(&raid_night(start), &[wipe], night("2026-09-27"));
        let shown = |at: Option<Pull>, hide_trash, more| Shown {
            at,
            cursor: None,
            hide_trash,
            more,
            accent: theme::GOLD_ACCENT,
            hide_realms: false,
        };
        let mut ui = simulator(panel(&rail, &shown(None, false, More::Offer), RAIL_W));
        for words in [
            "Pulls",
            "Hide trash",
            "Tonight",
            "Saturday, Sep 26",
            "The Venomous Abyss, Heroic",
            "The Coiled Altar",
            "Whole visit",
            "Trash",
            "97%",
            "7:34",
            "Show older nights",
        ] {
            assert!(ui.find(words).is_ok(), "{words}");
        }
        ui.click("The Coiled Altar").unwrap();
        ui.click("Hide trash").unwrap();
        ui.click("Show older nights").unwrap();
        let sent: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::Pull(Pull::Log(SegmentId(2))),
                Message::HideTrash,
                Message::OlderNights
            ]
        ));
        // Trash hidden: gone, but for the pull on the stage.
        let mut ui = simulator(panel(
            &rail,
            &shown(Some(Pull::Log(SegmentId(3))), true, More::Asking),
            RAIL_W,
        ));
        assert!(ui.find(current_id()).is_ok(), "the pull on the stage");
        assert!(ui.find("Reading the history store…").is_ok());
        assert!(ui.find("Show older nights").is_err());
        let mut ui = simulator(panel(&rail, &shown(None, true, More::None), RAIL_W));
        assert!(ui.find("Trash").is_err(), "hidden");
        assert!(ui.find(current_id()).is_err());
        assert!(ui.find("Reading the history store…").is_err());
        // An empty rail says why it is empty.
        let mut ui = simulator(panel(
            &Rail::default(),
            &shown(None, false, More::None),
            RAIL_W,
        ));
        assert!(ui.find(EMPTY).is_ok());
        // The drawer's keys ring their row, and a pull under a second says
        // so rather than a clock that reads as missing.
        let mut ui = simulator(panel(
            &rail,
            &Shown {
                cursor: Some(Pull::Log(SegmentId(2))),
                ..shown(None, false, More::None)
            },
            RAIL_W,
        ));
        assert!(ui.find(cursor_id()).is_ok(), "the keys' row");
        assert_eq!(clock(0), "<0:01");
        assert_eq!(clock(999), "<0:01");
        assert_eq!(clock(1_000), "0:01");
    }
}
