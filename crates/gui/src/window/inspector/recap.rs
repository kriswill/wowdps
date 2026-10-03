//! A death's recap (R9, the prototype's `recapPanel`): the last events
//! oldest first to the tinted killing blow — when (before the death), the
//! signed change, the event with its source quiet after it ("yours" for
//! the owner's own), and the health after it as a 6 px bar, amber under
//! 15 %, red under 3 %, a dash where none was reported — then the insight
//! when the player's own hit mattered. Its time and change columns are
//! as wide as their widest figure, measured.

use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Div, FontWeight, SharedString, TestSupportExt as _, TextRun, Window, div, font,
    relative,
};
use wowdps_gui_logic::deaths::before;
use wowdps_gui_logic::inspect::recap::{Recap, change_words, insight};
use wowdps_gui_logic::labels::display_name;
use wowdps_gui_logic::table::split_pet;
use wowdps_gui_logic::theme as gl;
use wowdps_model::Row;

use crate::theme::hsla;
use crate::window::Gui;
use crate::window::w::{Fit, MEDIUM, REGULAR, SEMIBOLD, W};

const PAD: (f32, f32) = (4.0, 12.0);
const HEAD_PAD: (f32, f32, f32) = (8.0, 16.0, 4.0);
const ROW_H: f32 = 30.0;
const SIDE: f32 = 16.0;
const GAP: f32 = 10.0;
const CHANGE_W: f32 = 78.0;
const HP_W: f32 = 88.0;
const HP_W_TILE: f32 = 56.0;
const HP_H: f32 = 6.0;
const HP_RADIUS: f32 = 3.0;
const PX: f32 = 14.0;
const SRC_PX: f32 = 12.0;
const SRC_GAP: f32 = 4.0;
const TOP_PX: f32 = 13.0;
const TIME_W: f32 = 44.0;
const TIME_PX: f32 = 13.0;
const LOW: f32 = 0.15;
const CRIT: f32 = 0.03;
const HP_MIN: f32 = 0.015;
const KILL_ALPHA: f32 = 0.09;
// The health strip's track is the theme's `health_track`.
const HP_DIM: f32 = 0.5;
const INSIGHT_MARGIN: (f32, f32) = (10.0, 16.0);
const INSIGHT_PAD: (f32, f32) = (8.0, 10.0);
const INSIGHT_RADIUS: f32 = 6.0;
const INSIGHT_PX: f32 = 14.0;
const INSIGHT_WASH: f32 = 0.10;

/// The recap's measured columns.
struct Cols {
    time: Option<f32>,
    change: f32,
    hp: f32,
    hp_head: f32,
}

/// `words`' one-line width at `size` in the window's face and `weight`.
fn text_w(window: &Window, w: &W, words: &str, size: f32, weight: FontWeight) -> f32 {
    let mut f = font(w.ui);
    f.weight = weight;
    let run = TextRun {
        len: words.len(),
        font: f,
        color: gpui_kit::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(
                SharedString::from(words.to_string()),
                w.z(size),
                &[run],
                None,
            )
            .width,
    ) / w.zoom
}

impl Cols {
    fn of(r: &Recap, fit: Fit, w: &W, window: &Window) -> Self {
        let timed = r.rows.iter().any(|e| e.offset_ms.is_some());
        let widest = |words: Vec<String>, size: f32, weight: FontWeight| {
            words
                .iter()
                .map(|s| text_w(window, w, s, size, weight))
                .fold(0.0_f32, f32::max)
                .ceil()
        };
        let time = timed.then(|| {
            widest(
                r.rows
                    .iter()
                    .filter_map(|e| e.offset_ms.map(before))
                    .collect(),
                TIME_PX,
                REGULAR,
            )
            .max(text_w(window, w, "Time", TOP_PX, REGULAR).ceil())
            .min(TIME_W)
        });
        let change = if timed {
            widest(r.rows.iter().map(change_words).collect(), PX, MEDIUM)
                .max(text_w(window, w, "Change", TOP_PX, REGULAR).ceil())
                .min(CHANGE_W)
        } else {
            CHANGE_W
        };
        Cols {
            time,
            change,
            hp: if fit == Fit::Tile { HP_W_TILE } else { HP_W },
            hp_head: text_w(window, w, "Health after", TOP_PX, REGULAR).ceil(),
        }
    }
}

/// The recap, oldest first.
pub fn view(r: &Recap, fit: Fit, w: &W, window: &mut Window, _cx: &Context<Gui>) -> Div {
    let cols = Cols::of(r, fit, w, window);
    let head = |words: &'static str, width: Option<f32>| {
        let t = w.text(words, TOP_PX, w.c(|t| t.label_ink), REGULAR);
        match width {
            Some(width) => div()
                .w(w.z(width))
                .flex_none()
                .flex()
                .justify_end()
                .child(t),
            None => div().flex_1().min_w_0().child(t),
        }
    };
    let mut heads = div().flex().items_center().gap(w.z(GAP));
    if let Some(time) = cols.time {
        heads = heads.child(head("Time", Some(time)));
    }
    heads = heads
        .child(head("Change", Some(cols.change)))
        .child(head("Last events, oldest first", None))
        .child(head("Health after", Some(cols.hp.max(cols.hp_head))));
    let mut list = div().flex().flex_col().pt(w.z(PAD.0)).pb(w.z(PAD.1)).child(
        div()
            .pt(w.z(HEAD_PAD.0))
            .px(w.z(HEAD_PAD.1))
            .pb(w.z(HEAD_PAD.2))
            .child(heads),
    );
    let oldest: Vec<&Row> = r.rows.iter().rev().collect();
    let blow = oldest.iter().rposition(|e| !e.gain);
    for (i, e) in oldest.iter().enumerate() {
        list = list.child(line(r, e, blow == Some(i), &cols, w));
    }
    if let Some(words) = insight(r) {
        let bold = w.you_text(r.class);
        let wash = r
            .class
            .map_or(w.t.ink_3, gl::Color::of_class)
            .alpha(INSIGHT_WASH);
        list = list.child(
            div()
                .pt(w.z(INSIGHT_MARGIN.0))
                .px(w.z(INSIGHT_MARGIN.1))
                .child(
                    div()
                        .id("recap-insight")
                        .test_support()
                        .py(w.z(INSIGHT_PAD.0))
                        .px(w.z(INSIGHT_PAD.1))
                        .rounded(w.r(INSIGHT_RADIUS))
                        .bg(hsla(wash))
                        .flex()
                        .flex_wrap()
                        .font_family(w.ui)
                        .text_size(w.z(INSIGHT_PX))
                        .children(words.into_iter().map(|(s, b)| {
                            div()
                                .font_weight(if b { SEMIBOLD } else { REGULAR })
                                .text_color(if b { bold } else { w.c(|t| t.ink) })
                                .child(s)
                        })),
                ),
        );
    }
    list
}

/// One event.
fn line(r: &Recap, e: &Row, kill: bool, cols: &Cols, w: &W) -> Div {
    let ink = if e.gain {
        w.c(|t| t.good)
    } else {
        w.c(|t| t.bad)
    };
    let (what, from) = split_pet(&e.label);
    let own = from.is_some_and(|s| display_name(s) == display_name(&r.who));
    let mut label = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_baseline()
        .gap(w.z(SRC_GAP))
        .overflow_hidden()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(w.text(what.to_string(), PX, w.c(|t| t.ink), REGULAR)),
        );
    // The source gives way whole: a name cut to "Zul'j…" reads as another.
    match from {
        Some(_) if own => {
            label = label.child(div().flex_shrink_0().child(w.text(
                if r.yours { "yours" } else { "self" },
                SRC_PX,
                w.you_text(r.class),
                REGULAR,
            )));
        }
        Some(src) => {
            label = label.child(div().flex_shrink_0().child(w.text(
                src.to_string(),
                SRC_PX,
                w.c(|t| t.ink_3_text),
                REGULAR,
            )));
        }
        None => {}
    }
    // A heal reporting 0 is the killing blow's report filled in: unknown.
    let hp = e.hp.filter(|(cur, _)| !(e.gain && *cur == 0));
    let track = w.t.health_track;
    let health = match hp {
        Some((cur, max)) => {
            let p = (cur as f32 / max.max(1) as f32).clamp(0.0, 1.0);
            let shown = if cur > 0 { p.max(HP_MIN) } else { 0.0 };
            let fill = if p < CRIT {
                w.c(|t| t.bad)
            } else if p < LOW {
                w.c(|t| t.amber)
            } else {
                w.c(|t| t.good)
            };
            div()
                .w(w.z(cols.hp))
                .h(w.z(HP_H))
                .flex_none()
                .rounded(w.r(HP_RADIUS))
                .bg(hsla(track))
                .child(
                    div()
                        .h_full()
                        .w(relative(shown))
                        .rounded(w.r(HP_RADIUS))
                        .when(shown > 0.0, |d| d.bg(fill)),
                )
        }
        // Unknown: a dash and the track dimmed — never a bright empty track,
        // which reads as dead.
        None => div()
            .w(w.z(cols.hp))
            .flex_none()
            .flex()
            .items_center()
            .gap(w.z(SRC_GAP))
            .child(w.text("\u{2014}", SRC_PX, w.c(|t| t.ink_3_text), REGULAR))
            .child(
                div()
                    .flex_1()
                    .h(w.z(HP_H))
                    .rounded(w.r(HP_RADIUS))
                    .bg(hsla(track.alpha(track.a * HP_DIM))),
            ),
    };
    let mut row = div()
        .h(w.z(ROW_H))
        .px(w.z(SIDE))
        .flex()
        .items_center()
        .gap(w.z(GAP))
        .when(kill, |d| d.bg(w.c(|t| t.bad.alpha(KILL_ALPHA))));
    if let Some(width) = cols.time {
        row = row.child(
            div()
                .w(w.z(width))
                .flex_none()
                .flex()
                .justify_end()
                .child(w.text(
                    e.offset_ms.map(before).unwrap_or_default(),
                    TIME_PX,
                    w.c(|t| t.ink_3_text),
                    REGULAR,
                )),
        );
    }
    row.child(
        div()
            .w(w.z(cols.change))
            .flex_none()
            .flex()
            .justify_end()
            .child(w.text(change_words(e), PX, ink, MEDIUM)),
    )
    .child(label)
    .child(health)
}
