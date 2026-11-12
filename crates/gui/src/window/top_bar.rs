//! The top bar every window screen wears (the prototype's `.top`, the iced
//! window's `top_bar.rs`): the wordmark, the two places — Home and Fights —
//! and, while the window knows more than one edition of the game, the
//! edition switch beside them: a chip per game, the one shown filled with
//! its color, or where they do not fit a dropdown; the jump box, the live
//! pill, the character picker, the gear and the help button, on the
//! panel's surface over a hairline the active place's underline covers. At
//! 820 px and under the wordmark goes, the jump box shrinks to its glyph,
//! the pill keeps its dot and clock, the switch is the dropdown, its name
//! truncating sooner, and the picker keeps its icon.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{IterationCount, Keyframe, Keyframes, Timing, animate_keyframes};
use gpui_kit::prelude::*;
use gpui_kit::{
    Bounds, Context, Div, ElementId, MouseButton, Pixels, SharedString, TestSupportExt as _,
    Window, canvas, div, img, px, relative,
};
use wowdps_gui_logic::editions::{
    Entry, Face, Form, chip_tip, chips, ink_on, richest_fit, switch_form,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_model::fmt::duration;
use wowdps_model::{Class, Screen, SegmentKind, Spec};
use wowdps_proto::ClientState;

use super::Gui;
use super::chrome::{PLACE_PAD, icon_button, kbd, place, tip};
use super::marks::{Seat, badge, with_pip};
use super::paint::{dot, glyph, glyph_ink, ring};
use super::w::{MEDIUM, REGULAR, SEMIBOLD, W};
use crate::images;
use crate::theme::hsla;

/// The bar's sides (`.top{padding-inline:8px 10px}`) and the gap between
/// its pieces (`gap:2px`).
const PAD_LEFT: f32 = 8.0;
const PAD_RIGHT: f32 = 10.0;
const GAP: f32 = 2.0;
/// How far the picker's right edge stands in from the window's: the bar's
/// right padding, then the gear and — unless the bar folded it away — the
/// help button, and the gaps before them. The character menu hangs from
/// there (`.menu{right:70px}`).
pub fn picker_end(w: &W, help: bool) -> f32 {
    let buttons = if help { 2.0 } else { 1.0 };
    PAD_RIGHT + buttons * (w.pitch.icon_button + GAP)
}
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
/// The edition switch: as tall as the picker, a hair from the places, and
/// the widest its name grows (an expansion's name can run long), less at
/// 820 px and under.
const SWITCH_GAP: f32 = 6.0;
const SWITCH_NAME_W: f32 = 150.0;
const SWITCH_NAME_NARROW: f32 = 76.0;
/// A game's chip: the live pill's height, corners and gap, a badge
/// concentric with its round end, a little more room after the name, and
/// a hair between two chips.
const CHIP_BADGE: f32 = 18.0;
const CHIP_PAD_RIGHT: f32 = 12.0;
const CHIP_GAP: f32 = 4.0;
/// The room before the picker.
const PICKER_GAP: f32 = 4.0;
/// The least room the whole jump box takes beside the chips, its margins
/// included: less, and the chips stand beside its glyph.
const JUMP_ROOM: f32 = 2.0 * JUMP_MARGIN + 200.0;
/// The wordmark's sides (`.mark{padding-inline:6px 12px}`), the switch's
/// dropdown face's and the gap inside it, the pill's and the picker's.
const MARK_PAD: (f32, f32) = (6.0, 12.0);
const SWITCH_PAD: (f32, f32) = (8.0, 6.0);
const SWITCH_INNER_GAP: f32 = 6.0;
const PILL_PAD: f32 = 10.0;
const PILL_INNER_GAP: f32 = 7.0;
const PICKER_PAD: (f32, f32) = (6.0, 8.0);
const PICKER_INNER_GAP: f32 = 8.0;
/// A chip's inner gap, as the pill's.
const CHIP_INNER_GAP: f32 = 7.0;
/// The share of the places' type size a caret is drawn at.
const CARET: f32 = 0.8;
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

/// The bar's pieces measured, zoom-1 pixels: what [`Parts::total`] adds up
/// for each form of the edition switch and each fold of the bar.
struct Parts {
    narrow: bool,
    /// The wordmark, the places, the gear and the pads: never folded.
    fixed: f32,
    /// The live pill's words, and its clock; `None` without a pill.
    pill: Option<(f32, f32)>,
    /// The picker, its spacer included; `None` without one.
    picker: Option<f32>,
    /// The switch in each form (`Form::ALL`'s order), its spacer
    /// included; `None` with one game known.
    switch: Option<[f32; 4]>,
}

/// How often the bar folds, at most, beyond the switch's own forms: the
/// pill keeps its dot alone, then the help button goes (`?` still opens
/// the sheet), then the jump glyph (Ctrl K still opens the palette).
pub(crate) const FOLDS: usize = 3;
/// The widest clock the switch's form reserves room for: a pull up to
/// an hour and a half long.
const CLOCK_WIDEST: &str = "88:88";
/// The room a form of the switch keeps spare beyond the bar's measured
/// width, for the layout's rounding.
const FIT_MARGIN: f32 = 1.0;
/// How far past the window one game's bar may measure before it folds:
/// the layout's rounding, so it folds only where it truly runs past.
const RUN_PAST: f32 = 0.5;

impl Parts {
    /// The bar at its widest for whichever game it shows: the pill and the
    /// picker there with their words at their caps (a clock up to an hour
    /// and a half). What the switch's
    /// form is chosen by, so that neither a pick nor a pull going live, but
    /// the window's width alone, changes it.
    fn widest(w: &W, window: &mut Window, narrow: bool, entries: &[Entry], face: &Face) -> Self {
        let mut parts = Self::measure(w, window, narrow, None, None, Some((entries, face)));
        let words = if narrow { 0.0 } else { PILL_NAME_W };
        let clock = words_width(w, window, CLOCK_WIDEST, PILL_PX, &w.ui, REGULAR);
        parts.pill = Some((words, clock));
        let name = if narrow {
            0.0
        } else {
            PICKER_NAME_W + PICKER_INNER_GAP
        };
        parts.picker = Some(
            PICKER_GAP
                + PICKER_PAD.0
                + w.size.place
                + PICKER_INNER_GAP
                + w.size.place * CARET
                + PICKER_PAD.1
                + name,
        );
        parts
    }

    /// Measure the bar: every piece as `bar` lays it out, its words in the
    /// window's own faces.
    fn measure(
        w: &W,
        window: &mut Window,
        narrow: bool,
        pill: Option<&Pill>,
        picker: Option<(&CharPick, bool)>,
        switch: Option<(&[Entry], &Face)>,
    ) -> Self {
        let text = |window: &mut Window, s: &str, size: f32, family: &SharedString, weight| {
            words_width(w, window, s, size, family, weight)
        };
        let place_px = w.size.place;
        let caret = place_px * CARET;
        let mut fixed = PAD_LEFT + PAD_RIGHT + w.pitch.icon_button;
        if !narrow {
            fixed +=
                MARK_PAD.0 + text(window, "wowdps", w.size.mark, &w.title, REGULAR) + MARK_PAD.1;
        }
        fixed += 4.0 * PLACE_PAD
            + text(window, "Home", place_px, &w.ui, MEDIUM)
            + text(window, "Fights", place_px, &w.ui, MEDIUM)
            + GAP;
        let pill = pill.map(|p| {
            let words = if narrow {
                0.0
            } else {
                let word = if p.live { "Live" } else { "Latest" };
                let words = format!("{word}, {}", p.name);
                text(window, &words, PILL_PX, &w.ui, REGULAR).min(PILL_NAME_W)
            };
            (
                words,
                text(window, &duration(p.ms), PILL_PX, &w.ui, REGULAR),
            )
        });
        let picker = picker.map(|(me, hide_realms)| {
            let mut width =
                PICKER_GAP + PICKER_PAD.0 + place_px + PICKER_INNER_GAP + caret + PICKER_PAD.1;
            if !narrow {
                let name = if hide_realms {
                    wowdps_gui_logic::labels::display_name(&me.name).to_string()
                } else {
                    me.name.clone()
                };
                width += text(window, &name, place_px, &w.ui, SEMIBOLD).min(PICKER_NAME_W)
                    + PICKER_INNER_GAP;
            }
            width
        });
        let switch = switch.map(|(entries, face)| {
            let chips = entries
                .iter()
                .map(|e| {
                    (PILL_H - CHIP_BADGE) / 2.
                        + CHIP_BADGE
                        + CHIP_INNER_GAP
                        + text(window, &e.name, PILL_PX, &w.ui, MEDIUM).min(SWITCH_NAME_W)
                        + CHIP_PAD_RIGHT
                })
                .sum::<f32>()
                + CHIP_GAP * entries.len().saturating_sub(1) as f32;
            let badge_only = SWITCH_PAD.0 + place_px + SWITCH_INNER_GAP + caret + SWITCH_PAD.1;
            let dropdown =
                badge_only + name_slot(w, window, entries, face, narrow) + SWITCH_INNER_GAP;
            [chips, chips, dropdown, badge_only].map(|width| SWITCH_GAP + width)
        });
        Parts {
            narrow,
            fixed,
            pill,
            picker,
            switch,
        }
    }

    /// The bar's whole width with the switch in `form` (none with one game
    /// known) and the bar folded `fold` times.
    fn total(&self, w: &W, form: Option<Form>, fold: usize) -> f32 {
        let mut width = self.fixed;
        // The line's children: the places, the jump box's room and the
        // gear always; the wordmark, the help button, the pill, the
        // picker and its spacer, the switch and its spacer as they are.
        let mut children = 3 + usize::from(!self.narrow);
        if let Some((words, clock)) = self.pill {
            width += 2.0 * PILL_PAD + w.size.dot;
            if fold == 0 {
                if words > 0.0 {
                    width += PILL_INNER_GAP + words;
                }
                width += PILL_INNER_GAP + clock;
            }
            children += 1;
        }
        if let Some(picker) = self.picker {
            width += picker;
            children += 2;
        }
        if fold < 2 {
            width += w.pitch.icon_button;
            children += 1;
        }
        if let (Some(switch), Some(form)) = (self.switch, form) {
            width += switch.get(form as usize).copied().unwrap_or_default();
            children += 2;
        }
        // The jump box's least room: none once it folds away; beside the
        // chips the whole box, or its glyph after a margin; else its glyph
        // narrow, its margins (it shrinks) wide.
        width += match (fold >= FOLDS, form) {
            (true, _) => 0.0,
            (false, Some(Form::Chips)) => JUMP_ROOM,
            (false, Some(Form::ChipsGlyph)) => JUMP_MARGIN + w.pitch.icon_button,
            (false, _) if self.narrow => w.pitch.icon_button,
            (false, _) => 2.0 * JUMP_MARGIN,
        };
        width + GAP * children.saturating_sub(1) as f32
    }
}

/// The dropdown's room for a game's name: the longest of the games'
/// names, at most the name's cap, so the dropdown keeps its width
/// whichever game it shows.
fn name_slot(w: &W, window: &mut Window, entries: &[Entry], face: &Face, narrow: bool) -> f32 {
    let cap = if narrow {
        SWITCH_NAME_NARROW
    } else {
        SWITCH_NAME_W
    };
    entries
        .iter()
        .map(|e| e.name.as_str())
        .chain(std::iter::once(face.name.as_str()))
        .map(|name| words_width(w, window, name, w.size.place, &w.ui, MEDIUM))
        .fold(0.0, f32::max)
        .min(cap)
}

/// One line of words' width at `size` in `family` and `weight`, as it is
/// painted, in zoom-1 pixels: the shaper's own, so a sum of them is the
/// laid-out line's.
fn words_width(
    w: &W,
    window: &mut Window,
    s: &str,
    size: f32,
    family: &SharedString,
    weight: gpui_kit::FontWeight,
) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let run = gpui_kit::TextRun {
        len: s.len(),
        font: gpui_kit::Font {
            weight,
            ..gpui_kit::font(family.clone())
        },
        color: gpui_kit::Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped =
        window
            .text_system()
            .shape_line(SharedString::from(s.to_string()), w.z(size), &[run], None);
    f32::from(shaped.width) / w.zoom
}

#[cfg(test)]
thread_local! {
    /// A test's switch: draw the bar unfolded on this thread, as main drew
    /// it, to measure where it would run past the window.
    static UNFOLDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Draw the bar unfolded on this thread (`false`: fold it as it must).
#[cfg(test)]
pub(crate) fn unfolded(on: bool) {
    UNFOLDED.with(|u| u.set(on));
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
    // The edition switch, only once more than one edition is known: a
    // window of one edition is today's. Its form is the richest whose bar
    // fits the window (`editions::switch_form`): the chips beside the
    // whole jump box, the chips beside its glyph, the dropdown, the
    // dropdown's badge alone. Where even that leaves the bar too wide, the
    // bar folds (`Parts::total`, `editions::richest_fit`): the pill keeps
    // its dot alone, the help button goes, then the jump glyph — each
    // folding back as the window widens, with room to spare.
    let me = gui.picked(cx);
    let switch = gui
        .edition_entries()
        .map(|entries| (entries, gui.edition_face()));
    let parts = Parts::measure(
        w,
        window,
        narrow,
        pill.as_ref(),
        me.as_ref().map(|me| (me, gui.cfg.hide_realms)),
        switch.as_ref().map(|(e, f)| (e.as_slice(), f)),
    );
    // The form by the bar at its widest for either game, so the width
    // alone moves it; the folds by the bar as it is.
    let form = switch.as_ref().map(|(entries, face)| {
        let widest = Parts::widest(w, window, narrow, entries, face);
        let needs = Form::ALL.map(|form| widest.total(w, Some(form), 0) + FIT_MARGIN);
        let chosen = switch_form(
            needs,
            w.width,
            gui.edition_form.get(),
            chips(entries.len(), narrow),
        );
        gui.edition_form.set(Some(chosen));
        chosen
    });
    let fold = if form.is_some() {
        let folds: Vec<f32> = (0..=FOLDS)
            .map(|fold| parts.total(w, form, fold) + FIT_MARGIN)
            .collect();
        richest_fit(&folds, w.width, gui.bar_fold.get())
    } else {
        // One game: main's bar wherever it fits, folded only where it would
        // run past the window, with no hysteresis to hold a fold where it
        // would fit.
        let folds: Vec<f32> = (0..=FOLDS)
            .map(|fold| parts.total(w, None, fold) - RUN_PAST)
            .collect();
        richest_fit(&folds, w.width, None)
    };
    #[cfg(test)]
    let fold = if UNFOLDED.with(std::cell::Cell::get) {
        0
    } else {
        fold
    };
    gui.bar_fold.set(Some(fold));
    if let (Some((entries, face)), Some(form)) = (switch, form) {
        line = line.child(div().w(w.z(SWITCH_GAP)).flex_none());
        line = if form.chips() {
            line.child(edition_chips(w, entries, cx))
        } else {
            let slot = name_slot(w, window, &entries, &face, narrow);
            line.child(edition_switch(
                w,
                face,
                (form == Form::Dropdown).then_some(slot),
                Rc::clone(&gui.edition_at),
                cx,
            ))
        };
    }
    let chipped = form.is_some_and(Form::chips);
    // The jump box stands centred in the room between the places and what
    // follows (`.jump{margin-inline:auto}`), keeping clear of both; narrow
    // it is a glyph at the end of that room. Beside the games' chips it
    // starts right after them instead, so it keeps its place whichever
    // game is shown (the pill and the picker after it are that game's, of
    // their own widths); where the chips take its room it is the glyph
    // there. At the bar's last fold the room is empty.
    let glyph_jump = narrow || form == Some(Form::ChipsGlyph);
    line = line.child(if fold >= FOLDS {
        div().flex_1()
    } else if glyph_jump {
        div()
            .flex_1()
            .flex()
            .when(chipped, |d| d.justify_start().pl(w.z(JUMP_MARGIN)))
            .when(!chipped, |d| d.justify_end())
            .child(tip(
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
            .when(chipped, |d| d.justify_start())
            .when(!chipped, |d| d.justify_center())
            .px(w.z(JUMP_MARGIN))
            .child(jump_box(w).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.jump(window, cx)),
            ))
    });
    if let Some(pill) = pill {
        line = line.child(live_pill(w, &pill, narrow, fold > 0, window, cx));
    }
    if let Some(me) = me {
        line = line
            .child(div().w(w.z(PICKER_GAP)).flex_none())
            .child(picker(w, me, narrow, gui.cfg.hide_realms, cx));
    }
    line = line.child(tip(
        icon_button(w, "top-gear", Glyph::Gear, true).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.toggle_menu(super::cards::Menu::Options, window, cx)
            }),
        ),
        "Options",
    ));
    if fold < 2 {
        line = line.child(tip(
            icon_button(w, "top-help", Glyph::Help, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.toggle_menu(super::cards::Menu::Sheet, window, cx)
                }),
            ),
            "Keyboard (?)",
        ));
    }
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

/// The games side by side (`editions::chips`): a chip each, in the order
/// the census knows them. The one shown is filled with its color, its
/// badge and name in the ink that reads there; the others are quiet — the
/// badge on the bar, the name in the muted ink, brightened under the
/// pointer.
/// The live dot is a pip on the badge of the game being played. A press
/// shows that game; the tooltip is its build, and says when it is the game
/// being played.
fn edition_chips(w: &W, entries: Vec<Entry>, cx: &mut Context<Gui>) -> impl IntoElement {
    let mut row = div()
        .id("top-edition")
        .test_support()
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(CHIP_GAP));
    for e in entries {
        row = row.child(chip(w, e, cx));
    }
    row
}

/// One game's chip in the row. Its geometry is the same in every state —
/// the same paddings filled or quiet, the playing pip over its badge's edge
/// rather than a slot of its own — so neither a pick nor the game played
/// moves anything in the bar.
fn chip(w: &W, e: Entry, cx: &mut Context<Gui>) -> impl IntoElement {
    let on = e.on;
    let fill = e.mark.tone.fill(&w.t);
    let ink = hsla(ink_on(fill, &w.t));
    let seat = if on {
        Seat::Fill { fill }
    } else {
        Seat::Bare {
            under: w.c(|t| t.surface),
        }
    };
    let side = w.z(CHIP_BADGE);
    let mark = with_pip(
        w,
        badge(w, e.edition, e.mark, side, seat),
        side,
        e.note.is_some(),
    );
    let edition = e.edition;
    let tip_words = chip_tip(&e);
    let chip = div()
        .id(ElementId::Name(SharedString::from(e.id.clone())))
        .aria_label(SharedString::from(e.name.clone()))
        .test_support()
        .aria_selected(on)
        .h(w.z(PILL_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(CHIP_INNER_GAP))
        .pl(w.z((PILL_H - CHIP_BADGE) / 2.))
        .pr(w.z(CHIP_PAD_RIGHT))
        .rounded(w.pill(PILL_H / 2.))
        .font_family(w.ui.clone())
        .font_weight(MEDIUM)
        .text_size(w.z(PILL_PX))
        .when(on, |d| d.bg(hsla(fill)).text_color(ink))
        // A quiet chip brightens its name under the pointer, as a place
        // does; the game shown's is no button to press again.
        .when(!on, |d| {
            d.cursor_pointer()
                .text_color(w.c(|t| t.ink_3_text))
                .hover(|s| s.text_color(w.c(|t| t.ink)))
        })
        .child(mark)
        .child(
            div()
                .max_w(w.z(SWITCH_NAME_W))
                .min_w_0()
                .truncate()
                .line_height(relative(1.3))
                .child(e.name),
        );
    tip(chip, tip_words).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            // The keys stay the window's, as after a pick from the menu.
            window.focus(&this.focus, cx);
            this.pick_edition(edition, cx);
        }),
    )
}

/// The edition switch as a dropdown, where the chips do not fit: the game
/// the window shows, its badge — the live pip on it while it is the game
/// being played — its name and the caret; the build in its tooltip. A
/// press opens the edition menu. Its bounds are kept for the menu to hang
/// from.
fn edition_switch(
    w: &W,
    face: Face,
    named: Option<f32>,
    at: Rc<Cell<Bounds<Pixels>>>,
    cx: &mut Context<Gui>,
) -> impl IntoElement {
    let side = w.z(w.size.place);
    let mark = with_pip(
        w,
        badge(
            w,
            face.edition,
            face.mark,
            side,
            Seat::Bare {
                under: w.c(|t| t.surface),
            },
        ),
        side,
        face.playing,
    );
    let seat = canvas(move |b, _, _| at.set(b), |_, (), _, _| {})
        .absolute()
        .inset_0();
    let switch = div()
        .id("top-edition")
        .test_support()
        .relative()
        .h(w.z(WHO_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(SWITCH_INNER_GAP))
        .pl(w.z(SWITCH_PAD.0))
        .pr(w.z(SWITCH_PAD.1))
        .rounded(w.r(6.))
        .cursor_pointer()
        .text_color(w.c(|t| t.ink_2))
        .hover(|s| s.bg(w.c(|t| t.hover)).text_color(w.c(|t| t.ink)))
        .child(seat)
        .child(mark)
        .when_some(named, |d, slot| {
            d.child(
                div()
                    .w(w.z(slot))
                    .min_w_0()
                    .font_family(w.ui.clone())
                    .font_weight(MEDIUM)
                    .text_size(w.z(w.size.place))
                    .truncate()
                    .line_height(relative(1.3))
                    .child(face.name),
            )
        })
        .child(glyph_ink(Glyph::ChevronDown, w.z(w.size.place * CARET)));
    tip(switch, face.tip).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, window, cx| {
            this.toggle_menu(super::cards::Menu::Edition, window, cx)
        }),
    )
}

/// The wordmark (`.mark`): "wowdps" in Marcellus and the game's gold,
/// whatever the chrome.
fn wordmark(w: &W) -> Div {
    div()
        .pl(w.z(MARK_PAD.0))
        .pr(w.z(MARK_PAD.1))
        .flex_none()
        .child(w.title_text("wowdps", w.size.mark, w.c(|t| t.accent)))
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
        .rounded(w.r(6.))
        .cursor_pointer()
        .hover(|s| s.border_color(w.c(|t| t.edge)))
        .child(glyph(Glyph::Search, w.z(w.size.icon), w.c(|t| t.ink_3)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(w.ui.clone())
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
/// stage. Narrow, the words go and the dot and clock stay; `dot_alone`
/// (the bar folded at its least widths), the clock goes too.
fn live_pill(
    w: &W,
    p: &Pill,
    narrow: bool,
    dot_alone: bool,
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
        .gap(w.z(PILL_INNER_GAP))
        // iced draws a container's border inside its padding; GPUI's is outside.
        .px(w.z(PILL_PAD - 1.))
        .rounded(w.pill(PILL_H / 2.))
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
    if !narrow && !dot_alone {
        let word = if p.live { "Live" } else { "Latest" };
        face = face.child(
            div()
                .max_w(w.z(PILL_NAME_W))
                .min_w_0()
                .font_family(w.ui.clone())
                .text_size(w.z(PILL_PX))
                .truncate()
                .child(format!("{word}, {}", p.name)),
        );
    }
    if !dot_alone {
        face = face.child(w.words(duration(p.ms), PILL_PX, REGULAR));
    }
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
    let px = w.device(side);
    let art = me
        .spec
        .and_then(|s| images::spec_icon(s.id(), px))
        .or_else(|| me.class.and_then(|c| images::class_icon(c, px)));
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
        .gap(w.z(PICKER_INNER_GAP))
        .pl(w.z(PICKER_PAD.0))
        .pr(w.z(PICKER_PAD.1))
        .rounded(w.r(6.))
        .cursor_pointer()
        .hover(|s| s.bg(w.c(|t| t.hover)))
        .child(icon)
        .when(!narrow, |d| {
            d.child(
                div()
                    .max_w(w.z(PICKER_NAME_W))
                    .min_w_0()
                    .font_family(w.ui.clone())
                    .font_weight(SEMIBOLD)
                    .text_size(w.z(w.size.place))
                    .text_color(w.you_text(me.class))
                    .truncate()
                    .line_height(relative(1.3))
                    .child(name),
            )
        })
        .text_color(w.c(|t| t.ink_3))
        .child(glyph_ink(Glyph::ChevronDown, w.z(w.size.place * CARET)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.toggle_menu(super::cards::Menu::Picker, window, cx)
            }),
        )
}
