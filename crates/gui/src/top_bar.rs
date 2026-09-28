//! The top bar every window screen wears (the prototype's `.top`): the
//! wordmark, the two places — Home and Fights — the jump box, the live
//! pill, the character picker, the gear and the help button. It replaces
//! the icon tab strip (home, fights, live, history): the fights and the
//! history are one pull rail now, and the live pull is the pill, a press
//! away from wherever the reader is.
//!
//! At 820 px and under the wordmark goes, the jump box shrinks to its
//! glyph, the pill keeps its dot and time, and the picker its icon.
//! Window-only.

use iced::widget::{Space, button, container, row, stack, text};
use iced::{Color, Element, Length, Theme};

use wowdps_model::Screen;
use wowdps_model::fmt::duration;

use crate::ellipsis::ellipsis;
use crate::line_icons::{LineIcon, line_icon};
use crate::nav;
use crate::theme::{self, pitch, size};
use crate::window::{Gui, Message};

/// The bar's sides (`.top{padding-inline:8px 10px}`) and the gap between
/// its pieces (`gap:2px`).
const PAD_LEFT: f32 = 8.0;
const PAD_RIGHT: f32 = 10.0;
const GAP: f32 = 2.0;
/// The jump box (`.jump{all:unset;flex:0 1 360px;height:28px;gap:8px;
/// padding-inline:10px;border:1px solid;border-radius:6px;font-size:14px}`).
/// `all:unset` makes it content-box: the 360 px are its content's, and its
/// padding and border come on top — 382 px from edge to edge.
const JUMP_W: f32 = 360.0;
const JUMP_H: f32 = 28.0;
const JUMP_GAP: f32 = 8.0;
const JUMP_PAD_X: f32 = 10.0;
const JUMP_BORDER: f32 = 1.0;
const JUMP_OUTER: f32 = JUMP_W + 2.0 * (JUMP_PAD_X + JUMP_BORDER);
const JUMP_RADIUS: f32 = 6.0;
const JUMP_PX: f32 = 14.0;
/// The least room kept either side of the jump box: it gives way before
/// it runs up against the places or the pill.
const JUMP_MARGIN: f32 = 14.0;
/// The live pill (`.live{height:28px;padding-inline:10px;border-radius:
/// 14px;gap:7px;font-size:14px}`).
const PILL_H: f32 = 28.0;
const PILL_PAD_X: f32 = 10.0;
const PILL_GAP: f32 = 7.0;
const PILL_PX: f32 = 14.0;
/// The pill's hairline while it is on the stage.
const PILL_EDGE: f32 = 1.0;
/// The widest the pill's words grow ("Live, Priory of the Sacred Flame
/// +14"): a longer name ends in "…" before it squeezes the jump box, and
/// the clock after it stays whole.
const PILL_NAME_W: f32 = 200.0;
/// Before the picker (`.who{margin-left:4px}`), and between its icon and
/// its caret (`.who{gap:8px}`).
const WHO_GAP: f32 = 4.0;
const WHO_INNER_GAP: f32 = 8.0;
/// The widest the picker's name grows on the bar: a long one — a realm
/// shown — ends in "…" before it squeezes the jump box.
const PICKER_NAME_W: f32 = 160.0;

/// How far the picker's right edge stands in from the window's: the bar's
/// right padding, then the gear and the help button and the gaps before
/// them. The picker's menu hangs from there (`.menu{right:70px}`).
pub(crate) const PICKER_END: f32 = PAD_RIGHT + 2.0 * (pitch::ICON_BUTTON + GAP);

/// The jump box's placeholder: what it will do once the palette is there.
pub(crate) const JUMP_WORDS: &str = "Jump to a pull, player or view";
/// The key the jump box names on its cap.
pub(crate) const JUMP_KEY: &str = "Ctrl K";

/// The margin the jump box keeps either side in a room `room` wide: whole
/// while the box can still be half its width inside it, then giving way —
/// the box shrinks first, and never below its key for want of a margin.
pub(crate) fn jump_margin(room: f32) -> f32 {
    ((room - JUMP_OUTER / 2.0) / 2.0).clamp(0.0, JUMP_MARGIN)
}

/// The jump box and the live pill, so a test can press them.
pub(crate) fn jump_id() -> iced::widget::Id {
    iced::widget::Id::new("top-jump")
}
pub(crate) fn live_id() -> iced::widget::Id {
    iced::widget::Id::new("top-live")
}

/// The live pill (`.live`): the log's newest pull, which `m` pins.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pill {
    /// Still going: the red dot and "Live"; else a ring and "Latest".
    pub live: bool,
    pub name: String,
    pub ms: i64,
    /// It is on the stage now (`.live[aria-current]`).
    pub current: bool,
}

/// Everything the bar says, gathered: it is laid out by its width.
#[derive(Debug, Clone)]
pub(crate) struct Bar {
    /// Home is the place on show; else the fights are.
    pub home: bool,
    pub pill: Option<Pill>,
    /// The picker's characters, the locked one and the realm option —
    /// none on Home, whose title is the picker.
    pub picks: Option<(Vec<nav::CharPick>, Option<String>, bool)>,
    pub accent: theme::Accent,
}

impl Bar {
    pub(crate) fn of(state: &Gui) -> Self {
        let app = &state.state;
        let home = state.home.is_some();
        // On the stage, following the log's newest: the snapshot's words
        // are fresher than the list's, which moves only when its shape does.
        let following = state.stored.is_none()
            && app.screen != Screen::List
            && app.following_live()
            && app.segment_name().is_some();
        let pill = app.entries().last().map(|e| {
            let (name, ms, live) = if following {
                (
                    app.segment_name().unwrap_or_default(),
                    app.duration_ms(),
                    app.is_live(),
                )
            } else {
                (e.row.name.clone(), e.row.duration_ms, e.row.live)
            };
            // Trash is trash, whatever the engine last named it (the rail
            // says so too).
            let name = if e.row.kind == wowdps_model::SegmentKind::Trash {
                "Trash".to_string()
            } else {
                name
            };
            Pill {
                live,
                name,
                ms,
                current: following && !home,
            }
        });
        let picks = (!home && !state.known_characters.is_empty()).then(|| {
            (
                state
                    .known_characters
                    .iter()
                    .map(crate::home::char_pick)
                    .collect(),
                state.owner_guid.clone(),
                state.cfg.hide_realms,
            )
        });
        Bar {
            home,
            pill,
            picks,
            accent: state.accent,
        }
    }

    /// The bar, laid out by the window's width.
    pub(crate) fn element(self) -> Element<'static, Message> {
        let bar = iced::widget::responsive(move |bounds| self.layout(bounds.width))
            .height(Length::Fixed(pitch::TOP_BAR));
        // `.top`: the panel's surface edge to edge, a hairline along its
        // foot that the active place's underline covers.
        stack![
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(pitch::TOP_BAR))
                .style(|_: &Theme| container::Style {
                    background: Some(theme::SURFACE.into()),
                    ..container::Style::default()
                }),
            container(nav::hairline::<Message>())
                .width(Length::Fill)
                .height(Length::Fixed(pitch::TOP_BAR))
                .align_y(iced::Alignment::End),
            bar,
        ]
        .width(Length::Fill)
        .height(Length::Fixed(pitch::TOP_BAR))
        .into()
    }

    fn layout(&self, width: f32) -> Element<'static, Message> {
        // `@container app (max-width: 820px)`.
        let narrow = width <= theme::NARROW_WINDOW;
        let mut line = row![].spacing(GAP).align_y(iced::Alignment::Center);
        if !narrow {
            line = line.push(nav::wordmark::<Message>());
        }
        let places = vec![
            nav::Tab {
                lead: nav::Lead::None,
                label: "Home",
                active: self.home,
                on_press: Some(Message::GotoHome),
                tip: None,
            },
            nav::Tab {
                lead: nav::Lead::None,
                label: "Fights",
                active: !self.home,
                on_press: Some(Message::GotoFights),
                tip: None,
            },
        ];
        // The places sit on the bar's foot, so the active one's underline
        // lands on its hairline (`.place{margin-top:2px}`).
        line = line.push(
            container(nav::tab_bar(places, self.accent, nav::Strip::Places))
                .height(Length::Fill)
                .align_y(iced::Alignment::End),
        );
        // The jump box stands centred in the room between the places and
        // what follows (`.jump{margin-inline:auto}`): its container is the
        // row's one stretch, the places and all after it as wide as they
        // are — and it keeps clear of both by its margin, which gives way
        // only once the box is down to half its width. Narrow, it is a
        // glyph at the end of that room.
        line = line.push(if narrow {
            container(nav::tip(
                nav::icon_button(LineIcon::Search, Some(Message::Jump), Some(jump_id())),
                format!("{JUMP_WORDS} ({JUMP_KEY})"),
            ))
            .width(Length::Fill)
            .align_x(iced::Alignment::End)
        } else {
            container(
                iced::widget::responsive(|room| {
                    container(jump_box())
                        .padding([0.0, jump_margin(room.width)])
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::Alignment::Center)
                        .align_y(iced::Alignment::Center)
                        .into()
                })
                .height(Length::Fixed(pitch::TOP_BAR)),
            )
            .width(Length::Fill)
        });
        if let Some(pill) = &self.pill {
            line = line.push(live_pill(pill, narrow));
        }
        if let Some((picks, owner, hide_realms)) = &self.picks {
            line = line.push(Space::new().width(Length::Fixed(WHO_GAP)));
            // Narrow, the name goes and the icon and caret stay (`.who
            // .nmx{display:none}`): the menu names every character anyway.
            line = line.push(if narrow {
                compact_picker(picks, owner.as_deref())
            } else {
                nav::character_picker(
                    picks,
                    owner.as_deref(),
                    *hide_realms,
                    Message::TogglePicker,
                    size::PLACE,
                    PICKER_NAME_W,
                )
            });
        }
        // The gear's options (ranks, realm names, the chrome) are every
        // screen's, so the switch is never a screen away.
        container(
            line.push(nav::gear(Message::ToggleOptions))
                .push(nav::help_glyph(Message::ToggleShortcuts))
                .height(Length::Fill),
        )
        .padding(iced::Padding {
            top: 0.0,
            right: PAD_RIGHT,
            bottom: 0.0,
            left: PAD_LEFT,
        })
        .width(Length::Fill)
        .height(Length::Fixed(pitch::TOP_BAR))
        .into()
    }
}

/// The picker at 820 px and under: the locked character's spec icon and
/// the caret — the name is the menu's, which also holds the follow switch,
/// so it opens with one character as with several.
fn compact_picker(picks: &[nav::CharPick], owner: Option<&str>) -> Element<'static, Message> {
    let current = owner
        .and_then(|guid| picks.iter().find(|c| c.guid == guid))
        .or_else(|| picks.first());
    let Some(c) = current else {
        return Space::new().into();
    };
    let face = row![
        nav::ringed_icon::<Message>(c.class, c.spec, size::PLACE),
        line_icon(LineIcon::ChevronDown, size::PLACE * 0.8, theme::INK_3),
    ]
    .spacing(WHO_INNER_GAP)
    .align_y(iced::Alignment::Center);
    nav::picker_button(face, Message::TogglePicker)
}

/// The jump box (`.jump`): the search glyph, the placeholder and its key,
/// on the ground in a hairline frame the pointer brightens. The command
/// palette is a later step's: until it is here, the box opens the `?`
/// sheet, the keys being the one index the window has.
fn jump_box() -> Element<'static, Message> {
    // The placeholder gives way before its key does (`.jump .lbl{overflow:
    // hidden;text-overflow:ellipsis}`): it leaves the keycap its width, or
    // a squeezed box would draw "Ctrl K" through the cap's frame.
    let face = row![
        line_icon(LineIcon::Search, size::ICON, theme::INK_3),
        ellipsis(JUMP_WORDS)
            .size(JUMP_PX)
            .color(theme::INK_3_TEXT)
            .leaving(
                vec![(JUMP_KEY.to_string(), size::KBD, theme::UI_MEDIUM)],
                nav::KBD_CHROME_X,
            ),
        Space::new().width(Length::Fill),
        nav::kbd::<Message>(JUMP_KEY),
    ]
    .spacing(JUMP_GAP)
    .align_y(iced::Alignment::Center);
    let frame = container(face)
        .id(jump_id())
        .padding([0.0, JUMP_PAD_X])
        .height(Length::Fixed(JUMP_H))
        .width(Length::Fill)
        .align_y(iced::Alignment::Center);
    container(
        button(frame)
            .padding(0)
            .width(Length::Fill)
            .on_press(Message::Jump)
            .style(|_: &Theme, status| button::Style {
                background: Some(theme::GROUND.into()),
                text_color: theme::INK_3,
                border: iced::Border {
                    color: match status {
                        button::Status::Hovered | button::Status::Pressed => theme::EDGE,
                        _ => theme::LINE,
                    },
                    width: 1.0,
                    radius: JUMP_RADIUS.into(),
                },
                ..button::Style::default()
            }),
    )
    .max_width(JUMP_OUTER)
    .into()
}

/// The live pill (`.live`): a red dot while the newest pull is going (a
/// ring once it is over), "Live, Trash" — or "Latest, …" — and its clock;
/// raised while it is on the stage. Narrow, the words go and the dot and
/// clock stay. A press pins it, as `m` does.
fn live_pill(p: &Pill, narrow: bool) -> Element<'static, Message> {
    let current = p.current;
    let mut face = row![if p.live {
        nav::dot::<Message>(theme::BAD, size::DOT)
    } else {
        nav::ring::<Message>(theme::INK_3, size::DOT)
    }]
    .spacing(PILL_GAP)
    .align_y(iced::Alignment::Center);
    if !narrow {
        let word = if p.live { "Live" } else { "Latest" };
        // As wide as it says, up to its cap: a long name ends in "…".
        face = face.push(
            container(
                ellipsis(format!("{word}, {}", p.name))
                    .size(PILL_PX)
                    .leaving(Vec::new(), 0.0),
            )
            .max_width(PILL_NAME_W),
        );
    }
    face = face.push(
        text(duration(p.ms))
            .size(PILL_PX)
            .wrapping(text::Wrapping::None),
    );
    let face = container(face)
        .id(live_id())
        .padding([0.0, PILL_PAD_X])
        .height(Length::Fixed(PILL_H))
        .align_y(iced::Alignment::Center);
    nav::tip(
        button(face)
            .padding(0)
            .on_press(Message::GotoLive)
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: if current {
                        Some(theme::RAISE.into())
                    } else {
                        hovered.then(|| theme::HOVER.into())
                    },
                    text_color: if current || hovered {
                        theme::INK
                    } else {
                        theme::INK_2
                    },
                    // On the stage, a hairline says so as a shape: the
                    // raise alone is a faint step on the bar.
                    border: iced::Border {
                        color: if current {
                            theme::EDGE
                        } else {
                            Color::TRANSPARENT
                        },
                        width: PILL_EDGE,
                        radius: (PILL_H / 2.0).into(),
                    },
                    ..button::Style::default()
                }
            }),
        if p.live { LIVE_TIP } else { LATEST_TIP },
    )
}

/// What the pill does, by what it says: the live pull while one is going,
/// else the log's latest.
pub(crate) const LIVE_TIP: &str = "Go to the live pull (m)";
pub(crate) const LATEST_TIP: &str = "Go to the latest pull (m)";
