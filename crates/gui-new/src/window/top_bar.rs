//! The top bar every window screen wears (the prototype's `.top`, the iced
//! window's `top_bar.rs`): the wordmark, the two places — Home and Fights —
//! the jump box, the live pill, the character picker, the gear and the help
//! button, on the panel's surface over a hairline the active place's
//! underline covers. At 820 px and under the wordmark goes, the jump box
//! shrinks to its glyph, the pill keeps its dot and clock, and the picker
//! its icon.

use std::time::Duration;

use gpui_kit::base::{IterationCount, Keyframe, Keyframes, Timing, animate_keyframes};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Div, MouseButton, TestSupportExt as _, Window, div, img, px, relative};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_model::fmt::duration;
use wowdps_model::{Class, Screen, SegmentKind, Spec};
use wowdps_proto::ClientState;

use super::Gui;
use super::chrome::{icon_button, kbd, place, tip};
use super::paint::{dot, glyph, glyph_ink, ring};
use super::w::{REGULAR, SEMIBOLD, W};
use crate::images;
use crate::theme::hsla;

/// The bar's sides (`.top{padding-inline:8px 10px}`) and the gap between
/// its pieces (`gap:2px`).
const PAD_LEFT: f32 = 8.0;
const PAD_RIGHT: f32 = 10.0;
const GAP: f32 = 2.0;
/// How far the picker's right edge stands in from the window's: the bar's
/// right padding, then the gear and the help button and the gaps before
/// them. The character menu hangs from there (`.menu{right:70px}`).
pub const PICKER_END: f32 = PAD_RIGHT + 2.0 * (wowdps_gui_logic::theme::PITCHES.icon_button + GAP);
/// The jump box (`.jump{flex:0 1 360px;height:28px;gap:8px;padding-inline:
/// 10px;border:1px solid;border-radius:6px;font-size:14px}`): content-box,
/// so 382 px edge to edge, and the room it keeps either side.
const JUMP_OUTER: f32 = 382.0;
const JUMP_H: f32 = 28.0;
const JUMP_MARGIN: f32 = 14.0;
const JUMP_PX: f32 = 14.0;
/// The live pill (`.live{height:28px;padding-inline:10px;border-radius:
/// 14px;gap:7px;font-size:14px}`) and the widest its words grow.
const PILL_H: f32 = 28.0;
const PILL_PX: f32 = 14.0;
const PILL_NAME_W: f32 = 200.0;
/// The picker (`.who{height:30px;padding-inline:6px 8px;gap:8px;
/// margin-left:4px}`), and the widest its name grows.
const WHO_H: f32 = 30.0;
const PICKER_NAME_W: f32 = 160.0;
/// The live dot's pulse (`.pulse` in the prototype): a ring that swells
/// off the dot and fades, once every 1.6 s — a few times when a pull goes
/// live, then the dot at rest. Every frame of it redraws the whole window,
/// so an endless pulse cost an idle window 4 % of a core for as long as a
/// log stayed open.
const PULSES: u64 = 3;
const PULSE: Duration = Duration::from_millis(1600);

/// The jump box's placeholder: what the palette it opens searches.
pub const JUMP_WORDS: &str = "Jump to a pull, player or view";
/// The key the jump box names on its cap.
pub const JUMP_KEY: &str = "Ctrl K";
/// What the pill does, by what it says.
pub const LIVE_TIP: &str = "Go to the live pull (m)";
pub const LATEST_TIP: &str = "Go to the latest pull (m)";

/// The live pill (`.live`): the log's newest pull, which `m` pins.
#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    /// Still going: the red dot and "Live"; else a ring and "Latest".
    pub live: bool,
    pub name: String,
    pub ms: i64,
    /// The newest pull's id: a new live pull pulses afresh.
    pub key: u64,
    /// It is on the stage now (`.live[aria-current]`).
    pub current: bool,
}

impl Pill {
    /// The pill for the log's newest pull: the snapshot's words while the
    /// stage follows it (fresher than the list's), else the list's row.
    pub fn of(app: &ClientState, home: bool) -> Option<Self> {
        let following =
            app.screen != Screen::List && app.following_live() && app.segment_name().is_some();
        app.entries().last().map(|e| {
            let (name, ms, live) = if following {
                (
                    app.segment_name().unwrap_or_default(),
                    app.duration_ms(),
                    app.is_live(),
                )
            } else {
                (e.row.name.clone(), e.row.duration_ms, e.row.live)
            };
            // Trash is trash, whatever the engine last named it.
            let name = if e.row.kind == SegmentKind::Trash {
                "Trash".to_string()
            } else {
                name
            };
            Pill {
                live,
                key: e.id.0,
                name,
                ms,
                current: following && !home,
            }
        })
    }
}

/// One of your characters, as the picker wears it: spec icon and class
/// colour. (The rail and Home name every one the store has seen; until
/// they do, the window knows the owner of the pull on the stage.)
#[derive(Debug, Clone, PartialEq)]
pub struct CharPick {
    pub guid: String,
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
}

/// The bar, laid out by the window's width.
pub fn bar(gui: &Gui, w: &W, window: &mut Window, cx: &mut Context<Gui>) -> impl IntoElement {
    let narrow = w.narrow();
    let app = gui.session.read(cx).state();
    let home = gui.place == super::Place::Home;
    // A stored pull on the stage is no more the live one than Home is.
    let pill = Pill::of(app, home || gui.hist.store.stored.is_some());
    let mut line = div()
        .size_full()
        .flex()
        .items_center()
        .gap(w.z(GAP))
        .pl(w.z(PAD_LEFT))
        .pr(w.z(PAD_RIGHT));
    if !narrow {
        line = line.child(wordmark(w));
    }
    // The places sit on the bar's foot, so the active one's underline lands
    // on its hairline (`.place{margin-top:2px}`).
    line = line.child(
        div()
            .h_full()
            .flex()
            .items_end()
            .gap(w.z(GAP))
            .child(place(w, "place-home", "Home", home).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.goto_home(window, cx)),
            ))
            .child(place(w, "place-fights", "Fights", !home).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.goto_fights(window, cx)),
            )),
    );
    // The jump box stands centred in the room between the places and what
    // follows (`.jump{margin-inline:auto}`), keeping clear of both; narrow,
    // it is a glyph at the end of that room.
    line = line.child(if narrow {
        div().flex_1().flex().justify_end().child(tip(
            icon_button(w, "top-jump", Glyph::Search, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.jump(window, cx)),
            ),
            "Jump to a pull, player or view (Ctrl K)",
        ))
    } else {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .px(w.z(JUMP_MARGIN))
            .child(jump_box(w).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.jump(window, cx)),
            ))
    });
    if let Some(pill) = pill {
        line = line.child(live_pill(w, &pill, narrow, window, cx));
    }
    if let Some(me) = gui.picked(cx) {
        line = line.child(div().w(w.z(4.)).flex_none()).child(picker(
            w,
            me,
            narrow,
            gui.cfg.hide_realms,
            cx,
        ));
    }
    line = line
        .child(tip(
            icon_button(w, "top-gear", Glyph::Gear, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.toggle_menu(super::cards::Menu::Options, window, cx)
                }),
            ),
            "Options",
        ))
        .child(tip(
            icon_button(w, "top-help", Glyph::Help, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.toggle_menu(super::cards::Menu::Sheet, window, cx)
                }),
            ),
            "Keyboard (?)",
        ));
    // `.top`: the panel's surface edge to edge, a hairline along its foot
    // that the active place's underline covers.
    div()
        .id("top-bar")
        .w_full()
        .h(w.z(w.pitch.top_bar))
        .flex_none()
        .relative()
        .bg(w.c(|t| t.surface))
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(w.z(1.))
                .bg(w.c(|t| t.line)),
        )
        .child(div().absolute().inset_0().child(line))
}

/// The wordmark (`.mark`): "wowdps" in Marcellus and the game's gold,
/// whatever the chrome.
fn wordmark(w: &W) -> Div {
    div()
        .pl(w.z(6.))
        .pr(w.z(12.))
        .flex_none()
        .child(w.title_text("wowdps", w.size.mark, w.c(|t| t.gold)))
}

/// The jump box (`.jump`): the search glyph, the placeholder and its key,
/// on the ground in a hairline frame the pointer brightens. A press opens
/// the command palette, as Ctrl K does.
fn jump_box(w: &W) -> gpui_kit::base::ObservedElement<gpui_kit::Stateful<Div>> {
    div()
        .id("top-jump")
        .test_support()
        .w_full()
        .max_w(w.z(JUMP_OUTER))
        .h(w.z(JUMP_H))
        .flex()
        .items_center()
        .gap(w.z(8.))
        .px(w.z(10.))
        .bg(w.c(|t| t.ground))
        .border(w.z(1.))
        .border_color(w.c(|t| t.line))
        .rounded(w.z(6.))
        .cursor_pointer()
        .hover(|s| s.border_color(w.c(|t| t.edge)))
        .child(glyph(Glyph::Search, w.z(w.size.icon), w.c(|t| t.ink_3)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(w.ui)
                .text_size(w.z(JUMP_PX))
                .text_color(w.c(|t| t.ink_3_text))
                .truncate()
                .child(JUMP_WORDS),
        )
        .child(kbd(w, JUMP_KEY))
}

/// The live pill (`.live`): a red dot while the newest pull is going —
/// pulsing, as the prototype's `.pulse` does — a ring once it is over,
/// "Live, Trash" or "Latest, …" and its clock; raised while it is on the
/// stage. Narrow, the words go and the dot and clock stay.
fn live_pill(
    w: &W,
    p: &Pill,
    narrow: bool,
    window: &mut Window,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let current = p.current;
    let mark = if p.live {
        pulsing_dot(w, p.key, window, cx).into_any_element()
    } else {
        ring(w.z(w.size.dot), w.c(|t| t.ink_3)).into_any_element()
    };
    let mut face = div()
        .id("top-live")
        .test_support()
        .h(w.z(PILL_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(7.))
        // iced draws a container's border inside its padding; GPUI's is outside.
        .px(w.z(10. - 1.))
        .rounded(w.z(PILL_H / 2.))
        .border(w.z(1.))
        .cursor_pointer()
        .text_color(if current {
            w.c(|t| t.ink)
        } else {
            w.c(|t| t.ink_2)
        })
        .when(current, |d| {
            d.bg(w.c(|t| t.raise)).border_color(w.c(|t| t.edge))
        })
        .when(!current, |d| {
            d.border_color(gpui_kit::transparent_black())
                .hover(|s| s.bg(w.c(|t| t.hover)).text_color(w.c(|t| t.ink)))
        })
        .child(mark);
    if !narrow {
        let word = if p.live { "Live" } else { "Latest" };
        face = face.child(
            div()
                .max_w(w.z(PILL_NAME_W))
                .min_w_0()
                .font_family(w.ui)
                .text_size(w.z(PILL_PX))
                .truncate()
                .child(format!("{word}, {}", p.name)),
        );
    }
    face = face.child(w.words(duration(p.ms), PILL_PX, REGULAR));
    tip(face, if p.live { LIVE_TIP } else { LATEST_TIP }).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, window, cx| this.go_live(window, cx)),
    )
}

/// The live dot with its pulse: a ring of the dot's colour swelling to
/// twice its size and fading, [`PULSES`] times for each new live pull —
/// and at rest, or under reduced motion, the dot alone, as the iced window
/// draws it.
fn pulsing_dot(w: &W, key: u64, window: &mut Window, cx: &mut Context<Gui>) -> impl IntoElement {
    let bad = w.c(|t| t.bad);
    let d = w.z(w.size.dot);
    let pulse = Keyframes::try_new([Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 1.0)])
        .ok()
        .map(|frames| {
            animate_keyframes(
                gpui_kit::ElementId::NamedInteger("live-pulse".into(), key),
                &frames,
                Timing::new(PULSE).iterations(IterationCount::Finite(PULSES)),
                window,
                cx,
            )
            .value
        })
        .filter(|_| !cx.reduce_motion())
        .unwrap_or(0.0);
    div()
        .relative()
        .size(d)
        .flex_none()
        // At rest the pulse sits at its last frame, 1.0: the dot alone.
        .when(pulse > 0.0 && pulse < 1.0, |el| {
            let grow = 1.0 + pulse;
            el.child(
                div()
                    .absolute()
                    .left(d * (1.0 - grow) / 2.)
                    .top(d * (1.0 - grow) / 2.)
                    .size(d * grow)
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(bad.opacity((1.0 - pulse) * 0.6)),
            )
        })
        .child(div().absolute().inset_0().child(dot(d, bad)))
}

/// The picker (`.who`): the character played last's spec icon in a 1 px
/// ring of their class colour, their name in the owner's text colour, and
/// the caret; narrow, the icon and caret. A press opens the character menu.
fn picker(
    w: &W,
    me: CharPick,
    narrow: bool,
    hide_realms: bool,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let side = w.z(w.size.place);
    let ring_color = me.class.map_or(w.c(|t| t.ink_3), |c| {
        hsla(wowdps_gui_logic::theme::Color::of_class(c))
    });
    let art = me
        .spec
        .and_then(|s| images::spec_icon(s.id()))
        .or_else(|| me.class.and_then(images::class_icon));
    let icon = div()
        .size(side)
        .flex_none()
        .rounded_full()
        .border(w.z(1.))
        .border_color(ring_color)
        .p(w.z(1.))
        .overflow_hidden()
        .child(match art {
            Some(tile) => img(tile).size_full().rounded_full().into_any_element(),
            None => div()
                .size_full()
                .rounded_full()
                .bg(hsla(w.class_rgb(me.class)))
                .into_any_element(),
        });
    let name = if hide_realms {
        wowdps_gui_logic::labels::display_name(&me.name).to_string()
    } else {
        me.name.clone()
    };
    div()
        .id("top-picker")
        .test_support()
        .h(w.z(WHO_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(8.))
        .pl(w.z(6.))
        .pr(w.z(8.))
        .rounded(w.z(6.))
        .cursor_pointer()
        .hover(|s| s.bg(w.c(|t| t.hover)))
        .child(icon)
        .when(!narrow, |d| {
            d.child(
                div()
                    .max_w(w.z(PICKER_NAME_W))
                    .min_w_0()
                    .font_family(w.ui)
                    .font_weight(SEMIBOLD)
                    .text_size(w.z(w.size.place))
                    .text_color(w.you_text(me.class))
                    .truncate()
                    .line_height(relative(1.3))
                    .child(name),
            )
        })
        .text_color(w.c(|t| t.ink_3))
        .child(glyph_ink(Glyph::ChevronDown, w.z(w.size.place * 0.8)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.toggle_menu(super::cards::Menu::Picker, window, cx)
            }),
        )
}
