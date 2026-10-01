//! The fight header over the window's meter (the prototype's `.fhead`, the
//! iced window's `fight_head.rs`): ONE title line — the encounter in
//! Marcellus, then in secondary ink its difficulty and size, the outcome
//! badge and the duration, and at its end the older / newer pull buttons —
//! and ONE stat line of the view's raid figures, gold-dim labels over ink
//! values with their commas, ending in the owner's "you" chip. What it says
//! is gui-logic's (`fight_head`); this module lays it out.

use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Div, MouseButton, TestSupportExt as _, div, relative};
use wowdps_gui_logic::fight_head::{
    GONE, NEWER_TIP, OLDER_TIP, RAIL_TIP, READING, Seen, Stats, Verdict, WAITING, You, meta,
    outcome, pairs, pending, you,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::home::wipe_pct;
use wowdps_gui_logic::labels::{Tone, display_name};
use wowdps_gui_logic::rail::{night_of, night_short};
use wowdps_gui_logic::theme::{YOU_EDGE, YOU_WASH, YOU_WASH_HOVER};
use wowdps_model::fmt::duration;

use super::Gui;
use super::chrome::{badge, class_icon, icon_button, quiet, tip};
use super::w::{Fit, MEDIUM, REGULAR, SEMIBOLD, W};
use crate::theme::hsla;

/// A live pull says how far behind the log is once it has been silent
/// this long (the game buffers its log writes).
const STALE_AFTER: Duration = Duration::from_secs(5);
/// The you chip: its spec disc, and the chip itself — the disc and the
/// 3 px above and below it.
const DISC: f32 = 20.0;
pub const CHIP_H: f32 = DISC + 2.0 * 3.0;
/// Gaps: the title line's pieces (`.ftitle{gap:12px}`), the meta's
/// (`.fmeta{gap:10px}`), the stat pairs' (`.stats{gap:4px 22px}`, `2px
/// 14px` narrow), a pair's own (`.stat{gap:7px}`), and the two lines'
/// (`.fhead{gap:7px}`).
const TITLE_GAP: f32 = 12.0;
const META_GAP: f32 = 10.0;
const STATS_GAP: f32 = 22.0;
const STATS_GAP_NARROW: f32 = 14.0;
const STATS_ROW_GAP: f32 = 4.0;
const STATS_ROW_GAP_NARROW: f32 = 2.0;
const PAIR_GAP: f32 = 7.0;
const LINE_GAP: f32 = 7.0;
/// The title's leading (`.ftitle h2{line-height:1.1}`).
const TITLE_LEADING: f32 = 1.1;

/// Everything the header says, gathered.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    /// The fight's name; `None` before there is a fight, when the line says
    /// it is waiting, in the quiet ink of a status.
    pub title: Option<String>,
    /// "Heroic, 25 players"; empty off a boss pull.
    pub meta: String,
    pub outcome: Option<(String, Tone)>,
    /// Empty before there is a fight to time.
    pub duration: String,
    pub stale: Option<String>,
    pub newer: bool,
    pub older: bool,
    pub stats: Option<Stats>,
    pub waiting: &'static str,
    /// The rail is a drawer, and the line leads with its button.
    pub rail_button: bool,
    /// The pull's card is pinned: a star after the title, as on the rail.
    pub pinned: bool,
    /// The night a stored pull of an earlier night is from ("Mon, Sep
    /// 21"): what says where on the rail the stage is once the drawer is
    /// shut. `None` for tonight's.
    pub night: Option<String>,
}

impl Head {
    pub fn of(gui: &Gui, w: &W, cx: &App) -> Self {
        let session = gui.session.read(cx);
        let app = gui.fight(cx);
        let name = app.segment_name();
        // ‹ › walk the rail, stored nights included.
        let (newer, older) = gui.pull_steps(cx);
        // A stored card knows how close a wipe came, which a live snapshot
        // does not carry yet — a stored pull's own, or the card the store
        // wrote for a pull of the log.
        let stored = gui.hist.store.stored.as_ref();
        let card = gui.stage_card(cx);
        let tonight = gui.tonight();
        let night = stored
            .and(card)
            .map(|c| night_of(c.start_local_ms))
            .filter(|day| *day != tonight)
            .map(|day| night_short(day, tonight));
        let stale = (app.is_live())
            .then(|| session.last_snapshot())
            .flatten()
            .map(|at| at.elapsed())
            .filter(|e| *e >= STALE_AFTER)
            .map(|e| format!("no events for {}s", e.as_secs()));

        Head {
            duration: name
                .as_ref()
                .map_or_else(String::new, |_| duration(app.duration_ms())),
            stats: name.is_some().then(|| stats_of(gui, cx)),
            title: name,
            meta: meta(app.segment_encounter()),
            outcome: outcome(Verdict {
                wipe_pct: card.and_then(wipe_pct),
                ..Verdict::of(app)
            }),
            stale,
            older,
            newer,
            waiting: match stored {
                Some(s) if s.missing => GONE,
                Some(_) => READING,
                None => WAITING,
            },
            rail_button: w.fit() != Fit::Wide,
            pinned: card.is_some_and(|c| c.pinned),
            night,
        }
    }
}

/// The stat line for the view on screen — or, while its answer is on its
/// way, nothing: the rows in hand are another view's or a placeholder's.
fn stats_of(gui: &Gui, cx: &App) -> Stats {
    let app = gui.fight(cx);
    if pending(app) {
        return Stats::default();
    }
    let rows = app.rows();
    let seen = gui.seen.of(app).and_then(Seen::owner);
    let owner = gui.owner_in(&rows, app.view);
    let raid = app.raid();
    // R25: when the owner died, as the raid timeline marks it.
    let died_at = owner
        .and_then(|i| rows.get(i))
        .zip(raid)
        .and_then(|(me, r)| r.deaths.iter().find(|d| d.guid == me.key))
        .map(|d| d.at_ms);
    let you = you(app.view, &rows, owner, seen, app.is_live(), died_at).map(|mut you| {
        if gui.cfg.hide_realms {
            you.name = display_name(&you.name).to_string();
        }
        you
    });
    Stats {
        pairs: pairs(app.view, &rows, raid),
        you,
    }
}

/// The header, laid out at the window's breakpoint.
pub fn view(head: Head, w: &W, cx: &mut Context<Gui>) -> impl IntoElement {
    let fit = w.fit();
    let (pt, px_, pb) = if fit == Fit::Narrow {
        (10.0, 12.0, 8.0)
    } else {
        (13.0, 18.0, 9.0)
    };
    let stats = head.stats.clone();
    div()
        .id("fight-head")
        .test_support()
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap(w.z(LINE_GAP))
        .pt(w.z(pt))
        .pb(w.z(pb))
        .px(w.z(px_))
        .child(title_line(&head, w, cx))
        .children(stats.map(|s| stat_line(&s, w, cx)))
}

/// The title, what it was, and the steps to its neighbours. The title is
/// what gives way: it ends in "…" before the meta or a step is pushed off.
fn title_line(head: &Head, w: &W, cx: &mut Context<Gui>) -> Div {
    let narrow = w.narrow();
    let (title_px, meta_px) = if narrow {
        (w.size.encounter_narrow, w.size.meta_narrow)
    } else {
        (w.size.encounter, w.size.meta)
    };
    let mut line = div().w_full().flex().items_center().gap(w.z(TITLE_GAP));
    if head.rail_button {
        line = line.child(tip(
            icon_button(w, "rail-button", Glyph::List, true).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.open_rail(cx)),
            ),
            RAIL_TIP,
        ));
    }
    line = line.child(match &head.title {
        Some(title) => div()
            .flex_shrink(1.)
            .min_w_0()
            .font_family(w.title)
            .text_size(w.z(title_px))
            .line_height(relative(TITLE_LEADING))
            .text_color(w.c(|t| t.ink))
            .truncate()
            .child(title.clone()),
        None => div()
            .h(w.z(w.pitch.icon_button))
            .flex()
            .items_center()
            .child(quiet(w, head.waiting, meta_px)),
    });
    let mut meta = div().flex_none().flex().items_center().gap(w.z(META_GAP));
    // A pinned pull says so first, a shape as the rail's row does, in its
    // quiet ink; an earlier night's pull says which night. Narrow, the
    // title needs the room more — the rail's row says both.
    if !w.narrow() {
        if head.pinned {
            meta = meta.child(tip(
                div().id("head-pin").child(w.text(
                    super::rail::PIN,
                    meta_px,
                    w.c(|t| t.ink_3),
                    REGULAR,
                )),
                super::rail::PIN_TIP,
            ));
        }
        if let Some(night) = &head.night {
            meta = meta.child(quiet(w, night.clone(), meta_px));
        }
    }
    if !head.meta.is_empty() {
        meta = meta.child(quiet(w, head.meta.clone(), meta_px));
    }
    if let Some((word, tone)) = &head.outcome {
        meta = meta.child(badge(w, word, *tone));
    }
    if !head.duration.is_empty() {
        meta = meta.child(quiet(w, head.duration.clone(), meta_px));
    }
    if let Some(stale) = &head.stale {
        meta = meta.child(quiet(w, stale.clone(), w.size.micro));
    }
    // ‹ is older and › newer, as ← → and `[` `]` are.
    let steps = div()
        .flex_none()
        .flex()
        .gap(w.z(2.))
        .child(tip(
            icon_button(w, "older-pull", Glyph::ChevronLeft, head.older).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.step_pull(true, window, cx)),
            ),
            OLDER_TIP,
        ))
        .child(tip(
            icon_button(w, "newer-pull", Glyph::ChevronRight, head.newer).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.step_pull(false, window, cx)),
            ),
            NEWER_TIP,
        ));
    line.child(meta).child(div().flex_1()).child(steps)
}

/// The stat line: the pairs, then the chip — at the line's far end in a
/// wide window, right after the pairs in a tile, and in a narrow one after
/// them with the whole line wrapping as it must. Never shorter than the
/// chip, so a view without one lays its tabs and rows out where every
/// other view does.
fn stat_line(stats: &Stats, w: &W, cx: &mut Context<Gui>) -> gpui_kit::AnyElement {
    let fit = w.fit();
    let value_px = if fit == Fit::Narrow {
        w.size.stat_narrow
    } else {
        w.size.stat
    };
    let pairs = stats.pairs.iter().map(|p| {
        div()
            .flex_none()
            .flex()
            .items_end()
            .gap(w.z(PAIR_GAP))
            .child(w.text(p.label.clone(), w.size.label, w.c(|t| t.gold_dim), REGULAR))
            .child(w.text(p.value.clone(), value_px, w.c(|t| t.ink), MEDIUM))
    });
    let chip = stats.you.as_ref().map(|you| you_chip(you, w, cx));
    let (gap, row_gap) = if fit == Fit::Narrow {
        (STATS_GAP_NARROW, STATS_ROW_GAP_NARROW)
    } else {
        (STATS_GAP, STATS_ROW_GAP)
    };
    let line = div()
        .id("stat-line")
        .test_support()
        .w_full()
        .min_h(w.z(CHIP_H))
        .flex()
        .items_center()
        .gap_x(w.z(gap))
        .gap_y(w.z(row_gap));
    match fit {
        Fit::Wide => line
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_end()
                    .gap(w.z(STATS_GAP))
                    .children(pairs),
            )
            .children(chip)
            .into_any_element(),
        Fit::Tile | Fit::Narrow => line
            .flex_wrap()
            .children(pairs)
            .children(chip)
            .into_any_element(),
    }
}

/// The chip (`.youchip`): a pill washed in the owner's class colour with
/// its edge a step stronger, their spec disc, their name as the owner's
/// text, and what the view says of them. A press selects their row — when
/// they have one here.
fn you_chip(you: &You, w: &W, cx: &mut Context<Gui>) -> impl IntoElement {
    let raw = hsla(w.class_rgb(you.class));
    let mut chip = div()
        .id("you-chip")
        .test_support()
        .h(w.z(CHIP_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(8.))
        .pt(w.z(3.))
        .pb(w.z(3.))
        .pl(w.z(3.))
        .pr(w.z(10.))
        .rounded(w.z(CHIP_H / 2.))
        .border(w.z(1.))
        .border_color(raw.opacity(YOU_EDGE))
        .bg(raw.opacity(YOU_WASH))
        .child(class_icon(w, you.class, you.spec, w.z(DISC), false))
        .child(w.text(
            you.name.clone(),
            w.size.chip,
            w.you_text(you.class),
            SEMIBOLD,
        ))
        .child(w.text(you.words.clone(), w.size.chip, w.c(|t| t.ink), REGULAR))
        .children(
            you.figure
                .clone()
                .map(|f| w.text(f, w.size.chip, w.c(|t| t.ink_2), REGULAR)),
        );
    if you.selectable {
        chip = chip
            .cursor_pointer()
            .hover(|s| s.bg(raw.opacity(YOU_WASH_HOVER)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.select_owner(window, cx)),
            );
        return tip(chip, "Select your row").into_any_element();
    }
    chip.into_any_element()
}
