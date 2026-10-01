//! Home as the prototype lays it out (`.home`; the iced window's
//! `home/panels.rs`): the title and its scope chips (`.home-top`), the
//! night (`.lastnight`), and the week's panels in a grid (`.hgrid`,
//! `.panel`) — keys against their timers (`.krow`, `.par`), a raid's bosses
//! with a dot per pull (`.brow`, `.pdot`), and key throughput across
//! characters. Every tile, row and dot is a jump point into its pull. The
//! words and measures are gui-logic's `home`; this module lays them out.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, ElementId, Hsla, MouseButton, SharedString, TestSupportExt as _, div,
};
use wowdps_gui_logic::fight_head::ordinal;
use wowdps_gui_logic::fold::fold;
use wowdps_gui_logic::home::chart::SLOPE_MAX_W;
use wowdps_gui_logic::home::{
    ALL_CHARACTERS, BossLine, Char, CharLine, EMPTY_CAP, Home, KEY_DOT, KEY_GAP, KEY_RESULT_W,
    KeyRun, MAX_PAGES, MAX_TILES, NightPanel, NightPull, PAGE, Panels, PullDot, RaidPanel,
    boss_words, columns_for, empty_night, key_widths, long_date, night_cap, none_of, panel_count,
    state_line,
};
use wowdps_gui_logic::labels::{display_name, plural, shown_name};
use wowdps_gui_logic::rail::Pull;
use wowdps_gui_logic::theme::{AA_CONTRAST, Color, accent_wash, class_text_on};
use wowdps_model::fmt::{duration, human};

use super::super::rail::{RAIL_W, mark};
use super::super::w::{Fit, MEDIUM, REGULAR, SEMIBOLD, W};
use super::super::{Gui, chrome};
use super::charts::{par_bar, rank_slope, trend};
use crate::theme::hsla;

/// The page's insets (`.home{padding:18px 22px 30px}`, and `14px 12px
/// 24px` at 820 px and under), as top, right, bottom, left, and the air
/// between its parts (`gap:18px`).
const PAD: [f32; 4] = [18.0, 22.0, 30.0, 22.0];
const PAD_NARROW: [f32; 4] = [14.0, 12.0, 24.0, 12.0];
const GAP: f32 = 18.0;
/// The title (`.home-top h2{font-family:Marcellus;font-size:28px}`) and
/// what stands beside it (`.home-top{gap:16px}`).
const TITLE_PX: f32 = 28.0;
const TOP_GAP: f32 = 16.0;
/// A scope chip (`.chip{height:26px;padding-inline:10px;font-size:14px;
/// gap:6px}`, `.chip .cd{8px}`) and the air between two (`.chips{gap:6px}`).
const CHIP_H: f32 = 26.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_PX: f32 = 14.0;
const CHIP_DOT: f32 = 8.0;
const CHIP_GAP: f32 = 6.0;
/// The night's card (`.lastnight{padding:16px 18px;gap:18px 26px;
/// border-radius:10px}`, its columns `1.2fr` and `1fr`), its place in
/// Marcellus (`h3{22px}`), its caption (`.cap{13px 600}`) and its line of
/// words (`.sub{margin:2px 0 10px}`, the frame's 14 px).
const CARD_PAD: [f32; 2] = [16.0, 18.0];
const CARD_GAP_X: f32 = 26.0;
const CARD_GAP_Y: f32 = 18.0;
const CARD_RADIUS: f32 = 10.0;
const LEFT_SHARE: f32 = 12.0;
const RIGHT_SHARE: f32 = 10.0;
const PLACE_PX: f32 = 22.0;
const CAP_PX: f32 = 13.0;
const SUB_PX: f32 = 14.0;
const SUB_ABOVE: f32 = 2.0;
const SUB_BELOW: f32 = 10.0;
/// A pull's tile (`.ptile{grid-template-columns:18px minmax(0,1fr) auto;
/// gap:10px;padding:7px 8px;border-radius:6px;font-size:15px}`, `.r{14px}`)
/// and a wipe's best % a space's width after its name.
const TILE_MARK: f32 = 18.0;
const TILE_GAP: f32 = 10.0;
const TILE_PAD: [f32; 2] = [7.0, 8.0];
const TILE_RADIUS: f32 = 6.0;
const TILE_PX: f32 = 15.0;
const TILE_RIGHT_PX: f32 = 14.0;
const TILE_TAIL_GAP: f32 = 4.0;
/// A panel (`.panel{padding:14px 16px 16px;border-radius:10px}`, `h3{margin:
/// 0 0 10px;font-size:15px;font-weight:600}`, `h3 small{13px}`), and the
/// least air between its title and its small words.
const PANEL_PAD: [f32; 4] = [14.0, 16.0, 16.0, 16.0];
const PANEL_RADIUS: f32 = 10.0;
const PANEL_TITLE_PX: f32 = 15.0;
const PANEL_SMALL_PX: f32 = 13.0;
const PANEL_HEAD_BELOW: f32 = 10.0;
const PANEL_HEAD_GAP: f32 = 8.0;
/// A key's or a boss's row, washed under the pointer: a keycap's corners.
const ROW_RADIUS: f32 = 4.0;
/// A key's row (`.krow{height:30px;font-size:14.5px}`).
const KEY_H: f32 = 30.0;
const KEY_PX: f32 = 14.5;
/// A boss's row (`.brow{gap:4px 12px;padding:7px 0}`, `.bn{15px}`, `.bs{
/// 14px}`) and its dots (`.pdot{10px;border:2px}`, `.pdots{gap:4px}`).
const BOSS_PAD_Y: f32 = 7.0;
const BOSS_GAP: f32 = 4.0;
const BOSS_GAP_X: f32 = 12.0;
const BOSS_PX: f32 = 15.0;
const BOSS_RIGHT_PX: f32 = 14.0;
const PDOT: f32 = 10.0;
const PDOT_EDGE: f32 = 2.0;
const PDOT_GAP: f32 = 4.0;
/// A legend (`.legend{gap:14px;font-size:13px;margin-top:8px}`, `i{9px}`,
/// `span{gap:6px}`) and a ring's edge in it (`inset 0 0 0 2px`).
const LEGEND_GAP: f32 = 14.0;
const LEGEND_PX: f32 = 13.0;
const LEGEND_DOT: f32 = 9.0;
const LEGEND_ABOVE: f32 = 8.0;
const LEGEND_INNER: f32 = 6.0;
const LEGEND_RING: f32 = 2.0;
/// The note (`.note{padding:8px 10px;border-left:2px solid var(--edge);
/// font-size:13px}`): faint words behind an edge.
const NOTE_PAD: [f32; 2] = [8.0, 10.0];
const NOTE_EDGE: f32 = 2.0;
const NOTE_PX: f32 = 13.0;

/// The facts the page needs besides the panels.
struct Meta {
    /// The store is on and has answered: there is a week to show. Off, or
    /// not heard from yet, the page says which instead of drawing panels
    /// that say "none" of what nobody read.
    settled: bool,
    stalled: bool,
    /// Whose week: a guid, or `None` for every character.
    scope: Option<String>,
    /// The line that tells a disabled store from a cold one from a
    /// degraded one.
    state_line: Option<String>,
    hide_realms: bool,
    /// The night it is now, which words how long ago the lead night was.
    tonight: i64,
}

/// The whole page: the title and its scope chips, the night, and the
/// week's panels in as many columns as the width holds.
pub fn view(gui: &mut Gui, w: &W, cx: &mut Context<Gui>) -> AnyElement {
    let blank = Home::new();
    let home = gui.hist.store.home.as_ref().unwrap_or(&blank);
    let meta = Meta {
        settled: home.answered && home.disabled_reason.is_none(),
        stalled: home.stalled(),
        scope: home.scope.clone(),
        state_line: state_line(home),
        hide_realms: gui.cfg.hide_realms,
        tonight: gui.tonight(),
    };
    let panels = gui.hist.store.panels.clone();
    // The chips stand still: by name, not by how much each was played.
    let mut chars: Vec<CharLine> = gui
        .hist
        .known
        .iter()
        .filter(|c| !c.guid.is_empty())
        .cloned()
        .collect();
    chars.sort_by_key(|c| fold(&c.name));

    // `@container app (max-width: 820px)`: Home is the whole window's width
    // there (the rail is a drawer), so the window's is the test.
    let narrow = w.narrow();
    let pad = if narrow { PAD_NARROW } else { PAD };
    let width = w.width
        - if w.fit() == Fit::Wide {
            RAIL_W + 1.0
        } else {
            0.0
        };
    // Narrow, the page's bar takes a lane of its own while the page
    // overflows (12 px of gutter is too little for a bar to float in
    // beside the panels), as the last frame laid it out; wide, it floats
    // in the 22 px gutter, clear of them.
    let lane = if narrow && gui.hist.home_scroll.max_offset().y > gpui_kit::px(0.) {
        w.pitch.scroll_lane
    } else {
        0.0
    };
    let inner = (width - lane - pad[1] - pad[3]).max(0.0);
    let mut page = div()
        .flex()
        .flex_col()
        .gap(w.z(GAP))
        .pt(w.z(pad[0]))
        .pr(w.z(pad[1]))
        .pb(w.z(pad[2]))
        .pl(w.z(pad[3]))
        .child(top(&meta, &chars, w, cx));
    // What the reader is looking at, before any number: an empty screen for
    // three different reasons must not look like one screen. A store that
    // is off, or not heard from yet, has no week to show — the night card
    // says why. A store that answered but lost something says so over the
    // week it has.
    if meta.settled
        && let Some(line) = meta.state_line.clone()
    {
        page = page.child(w.text(line, w.size.micro, w.c(|t| t.ink_2), REGULAR));
    }
    page = page.child(last_night(&meta, &panels, &chars, inner, narrow, w, cx));
    if meta.settled {
        // The prototype's three, always and in its order — keys, the raid,
        // throughput — each saying so when its week is empty, so the
        // dashboard keeps its shape; a second raid of the week after them.
        let cols = if narrow {
            1
        } else {
            columns_for(inner, GAP).min(panel_count(&panels)).max(1)
        };
        let col_w = (inner - GAP * cols.saturating_sub(1) as f32) / cols as f32;
        let who = scope_name(&meta, &chars);
        let hide = meta.hide_realms;
        let mut raids = panels.raids.iter();
        let mut cards = vec![
            keys_panel(&panels, who.as_deref(), hide, col_w, w, cx),
            raid_panel(raids.next(), who.as_deref(), hide, 0, w, cx),
            trend_panel(&panels, who.as_deref(), hide, col_w, w, cx),
        ];
        for (i, r) in raids.enumerate() {
            cards.push(raid_panel(Some(r), who.as_deref(), hide, i + 1, w, cx));
        }
        page = page.child(grid(cards, cols, w));
        // The one honest word about a stop the reader would otherwise read
        // as the whole week.
        if meta.stalled {
            page = page.child(w.text(
                format!(
                    "Only the newest {} pulls of the week were read.",
                    PAGE * MAX_PAGES
                ),
                w.size.micro,
                w.c(|t| t.ink_3_text),
                REGULAR,
            ));
        }
    }
    div()
        .id("home")
        .test_support()
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(
            div()
                .id("home-page")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&gui.hist.home_scroll)
                .pr(w.z(lane))
                .child(page),
        )
        .child(crate::scrollbar::bar(w.scrollbar(), &gui.hist.home_scroll))
        .into_any_element()
}

/// The scoped character's name, as drawn; `None` scoped to all of them.
fn scope_name(meta: &Meta, chars: &[CharLine]) -> Option<String> {
    let guid = meta.scope.as_deref()?;
    Some(chars.iter().find(|c| c.guid == guid).map_or_else(
        || "this character".to_string(),
        |c| shown_name(&c.name, meta.hide_realms),
    ))
}

/// Words that wrap, in the window's face.
fn wrapping(w: &W, words: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    div()
        .font_family(w.ui)
        .text_size(w.z(size))
        .text_color(color)
        .child(words.into())
}

/// A character's ink: their class's colour, or the third ink for one whose
/// class nobody saw.
fn char_ink(c: &Char, w: &W) -> Hsla {
    hsla(c.ink(w.t.ink_3))
}

/// The title — "You, this week", or the scoped character's — and the scope
/// chips: all characters, then each one, a dot in their class colour. A
/// chip scopes Home and is remembered as the scope it opens on.
fn top(meta: &Meta, chars: &[CharLine], w: &W, cx: &mut Context<Gui>) -> impl IntoElement {
    // The title names the character without their realm whatever the
    // option says — the chip beside it carries the realm when it is shown —
    // and wraps rather than runs off a narrow window's edge.
    let title = match scope_name(meta, chars) {
        Some(name) => format!("{}, this week", display_name(&name)),
        None => "You, this week".to_string(),
    };
    let mut chips = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(w.z(CHIP_GAP))
        .child(scope_chip(
            ALL_CHARACTERS.to_string(),
            None,
            meta.scope.is_none(),
            None,
            w,
            cx,
        ));
    for c in chars {
        let on = meta.scope.as_deref() == Some(c.guid.as_str());
        let dot = hsla(c.class.map_or(w.t.ink_3, Color::of_class));
        chips = chips.child(scope_chip(
            shown_name(&c.name, meta.hide_realms),
            Some(dot),
            on,
            Some(c.guid.clone()),
            w,
            cx,
        ));
    }
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(w.z(TOP_GAP))
        .gap_y(w.z(CHIP_GAP))
        .child(
            div()
                .font_family(w.title)
                .text_size(w.z(TITLE_PX))
                .text_color(w.c(|t| t.ink))
                .child(title),
        )
        .child(chips)
}

/// One scope chip (`.chip`): the character's dot and name, pressed — the
/// accent's edge over a wash of it — when it is the scope; under the
/// pointer its edge and its words brighten, a pressed chip's edge staying
/// the accent's.
fn scope_chip(
    label: String,
    dot: Option<Hsla>,
    on: bool,
    scope: Option<String>,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let id = ElementId::Name(SharedString::from(match &scope {
        Some(guid) => format!("scope-{guid}"),
        None => "scope-all".to_string(),
    }));
    let accent = w.accent();
    div()
        .id(id)
        .test_support()
        .aria_selected(on)
        .flex_none()
        .flex()
        .items_center()
        .gap(w.z(CHIP_GAP))
        .h(w.z(CHIP_H))
        .px(w.z(CHIP_PAD_X))
        .rounded(w.z(CHIP_H / 2.0))
        .border(w.z(1.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(hsla(accent_wash(w.accent)))
                .border_color(accent)
                .text_color(w.c(|t| t.ink))
        })
        .when(!on, |d| {
            d.border_color(w.c(|t| t.line))
                .text_color(w.c(|t| t.ink_2))
                .hover(|s| s.border_color(w.c(|t| t.edge)).text_color(w.c(|t| t.ink)))
        })
        .children(dot.map(|c| super::super::paint::dot(w.z(CHIP_DOT), c)))
        .child(w.words(label, CHIP_PX, REGULAR))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.scope_home(scope.clone(), cx)),
        )
        .into_any_element()
}

/// A surface: the night card's and every panel's (`background:var(--
/// surface);border:1px solid var(--line)`).
fn surface(radius: f32, w: &W) -> Div {
    div()
        .bg(w.c(|t| t.surface))
        .border(w.z(1.))
        .border_color(w.c(|t| t.line))
        .rounded(w.z(radius))
}

fn cap(words: &'static str, w: &W) -> Div {
    w.text(words, CAP_PX, w.c(|t| t.gold_dim), SEMIBOLD)
}

/// The note: `words` behind a 2 px edge rule, as tall as they are.
fn note(words: String, w: &W) -> Div {
    div()
        .flex()
        .child(div().w(w.z(NOTE_EDGE)).flex_none().bg(w.c(|t| t.edge)))
        .child(
            wrapping(w, words, NOTE_PX, w.c(|t| t.ink_3_text))
                .flex_1()
                .min_w_0()
                .py(w.z(NOTE_PAD[0]))
                .px(w.z(NOTE_PAD[1])),
        )
}

/// The night (`.lastnight`): where and when, on whom, each pull as a tile
/// with its place in the role — and beside it (under it, narrow) how that
/// place moved across the night.
fn last_night(
    meta: &Meta,
    panels: &Panels,
    chars: &[CharLine],
    inner: f32,
    narrow: bool,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let card = surface(CARD_RADIUS, w)
        .id("home-night")
        .test_support()
        .w_full()
        .py(w.z(CARD_PAD[0]))
        .px(w.z(CARD_PAD[1]));
    let Some(n) = &panels.night else {
        // Why the card has no night: words for each reason — a store off or
        // not heard from yet in its own words, before any "none".
        let scope = meta.settled.then(|| scope_name(meta, chars)).flatten();
        let words = empty_night(
            meta.settled,
            meta.state_line.as_deref(),
            panels,
            scope.as_deref(),
        );
        return card
            .flex()
            .flex_col()
            .gap(w.z(SUB_BELOW))
            .child(cap(EMPTY_CAP, w))
            .child(note(words, w))
            .into_any_element();
    };
    // The chart is as wide as its column lets it be, up to its own cap.
    let content = inner - 2.0 * CARD_PAD[1] - 2.0;
    let (left_w, right_w) = if narrow {
        (content, content)
    } else {
        let share = (content - CARD_GAP_X) / (LEFT_SHARE + RIGHT_SHARE);
        (share * LEFT_SHARE, share * RIGHT_SHARE)
    };
    let who = char_ink(&n.who, w);
    let right = div()
        .flex()
        .flex_col()
        .child(cap("Your rank, pull by pull", w))
        .child(rank_slope(
            &n.pulls,
            who,
            right_w.clamp(1.0, SLOPE_MAX_W),
            w,
            cx,
        ))
        .child(legend(
            vec![
                (legend_dot(who, w), "kill or timed".to_string()),
                (
                    legend_ring(who, w.c(|t| t.surface), w),
                    "wipe or over time".to_string(),
                ),
            ],
            w,
        ));
    let left = night_words(meta, n, w, cx);
    let body = if narrow {
        div()
            .flex()
            .flex_col()
            .gap(w.z(CARD_GAP_Y))
            .child(left)
            .child(right)
    } else {
        div()
            .flex()
            .gap(w.z(CARD_GAP_X))
            .child(left.w(w.z(left_w)).flex_none())
            .child(right.w(w.z(right_w)).flex_none())
    };
    card.child(body).into_any_element()
}

/// The night card's left column: its caption, its place, the date and the
/// character, and the tiles.
fn night_words(meta: &Meta, n: &NightPanel, w: &W, cx: &mut Context<Gui>) -> Div {
    let who = n.who.class.map_or(w.c(|t| t.ink), |c| {
        hsla(class_text_on(c, w.t.surface, AA_CONTRAST))
    });
    let quiet = |s: String| w.text(s, SUB_PX, w.c(|t| t.ink_2), REGULAR);
    // The line wraps between its words, never inside the name: a row of
    // runs, each whole.
    let sub = div()
        .flex()
        .flex_wrap()
        .pt(w.z(SUB_ABOVE))
        .pb(w.z(SUB_BELOW))
        .child(quiet(format!("{}, on ", long_date(n.day, meta.tonight))))
        .child(w.text(
            shown_name(&n.who.name, meta.hide_realms),
            SUB_PX,
            who,
            SEMIBOLD,
        ))
        .child(quiet(". ".to_string()))
        .child(quiet(format!("Rank is among your role, by {}.", n.measure)));
    let mut col = div()
        .flex()
        .flex_col()
        .items_start()
        .min_w_0()
        .child(cap(night_cap(n.day, meta.tonight), w))
        .child(w.title_text(n.place.clone(), PLACE_PX, w.c(|t| t.ink)))
        .child(sub);
    // The newest of a long night: the chart beside holds every pull.
    let skip = n.pulls.len().saturating_sub(MAX_TILES);
    if skip > 0 {
        col = col.child(w.text(
            format!(
                "{} earlier that night, on the chart and the rail",
                plural(skip, "pull")
            ),
            NOTE_PX,
            w.c(|t| t.ink_3_text),
            REGULAR,
        ));
    }
    for (i, p) in n.pulls.iter().enumerate().skip(skip) {
        col = col.child(tile(p, i, w, cx));
    }
    col
}

/// One pull of the night (`.ptile`): its outcome, its name — "at 56%" after
/// a wipe's — and its place in the role with the figure it was by: "17th
/// of 19, 149.3k". As wide as it says; a name too long for the column gives
/// way before the place does. A press opens it.
fn tile(p: &NightPull, i: usize, w: &W, cx: &mut Context<Gui>) -> AnyElement {
    let of = format!(
        " of {}, {}",
        p.standing.of,
        human(p.standing.value.round().max(0.0) as u64)
    );
    let mut name = div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(w.z(TILE_TAIL_GAP))
        .child(div().min_w_0().truncate().child(w.text(
            p.name.clone(),
            TILE_PX,
            w.c(|t| t.ink),
            REGULAR,
        )));
    if let Some(pct) = p.wipe_pct {
        name = name.child(
            w.text(
                format!("at {pct}%"),
                TILE_PX,
                w.c(|t| t.ink_3_text),
                REGULAR,
            )
            .flex_none(),
        );
    }
    let right = div()
        .flex_none()
        .flex()
        .child(w.text(
            ordinal(p.standing.place),
            TILE_RIGHT_PX,
            w.c(|t| t.ink),
            MEDIUM,
        ))
        .child(w.text(of, TILE_RIGHT_PX, w.c(|t| t.ink_2), REGULAR));
    let face = div()
        .flex()
        .items_center()
        .gap(w.z(TILE_GAP))
        .child(
            div()
                .w(w.z(TILE_MARK))
                .flex_none()
                .flex()
                .justify_center()
                .child(mark(p.mark, w)),
        )
        .child(name)
        .child(right);
    jump(("tile", i), p.fight_id.clone(), TILE_RADIUS, w, cx)
        .test_support()
        .max_w_full()
        .py(w.z(TILE_PAD[0]))
        .px(w.z(TILE_PAD[1]))
        .child(face)
        .into_any_element()
}

/// A press anywhere on it opens the stored pull `fight_id`, washed under
/// the pointer as every row that answers it is.
fn jump(
    id: impl Into<ElementId>,
    fight_id: String,
    radius: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> gpui_kit::Stateful<Div> {
    div()
        .id(id.into())
        .rounded(w.z(radius))
        .cursor_pointer()
        .hover(|s| s.bg(w.c(|t| t.hover)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.go_pull(Pull::Stored(fight_id.clone()), cx);
                cx.stop_propagation();
            }),
        )
}

/// A panel (`.panel`): its heading and the small words beside it, over its
/// body, in the panel's insets, on its surface — as tall as its row, as
/// the prototype's grid stretches its cards.
fn panel(
    id: impl Into<ElementId>,
    title: String,
    small: &'static str,
    body: impl IntoElement,
    w: &W,
) -> AnyElement {
    let head = div()
        .flex()
        .items_center()
        .gap(w.z(PANEL_HEAD_GAP))
        .pb(w.z(PANEL_HEAD_BELOW))
        .child(div().flex_1().min_w_0().truncate().child(w.text(
            title,
            PANEL_TITLE_PX,
            w.c(|t| t.ink),
            SEMIBOLD,
        )))
        .child(
            w.text(small, PANEL_SMALL_PX, w.c(|t| t.ink_3_text), REGULAR)
                .flex_none(),
        );
    surface(PANEL_RADIUS, w)
        .id(id.into())
        .test_support()
        .flex_1()
        .flex()
        .flex_col()
        .min_w_0()
        .pt(w.z(PANEL_PAD[0]))
        .pr(w.z(PANEL_PAD[1]))
        .pb(w.z(PANEL_PAD[2]))
        .pl(w.z(PANEL_PAD[3]))
        .child(head)
        .child(body)
        .into_any_element()
}

/// "Keys this week": each key's time against its timers.
fn keys_panel(
    panels: &Panels,
    who: Option<&str>,
    hide_realms: bool,
    col_w: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let inner = (col_w - PANEL_PAD[1] - PANEL_PAD[3] - 2.0).max(0.0);
    let body = if panels.keys.is_empty() {
        note(none_of("keys", who), w)
    } else {
        let mut list = div().flex().flex_col();
        for (i, k) in panels.keys.iter().enumerate() {
            list = list.child(key_row(k, i, inner, hide_realms, w, cx));
        }
        div().flex().flex_col().child(list).child(legend_words(
            "Ticks mark +3, +2 and the timer, left to right",
            w,
        ))
    };
    panel(
        "home-keys",
        "Keys this week".to_string(),
        "run time against the timer",
        body,
        w,
    )
}

/// One key (`.krow`): whose it was, its name, its run against its timers
/// and what it earned — "+1 31:39", "over 36:12". A press opens it.
fn key_row(
    k: &KeyRun,
    i: usize,
    inner: f32,
    hide_realms: bool,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let (name_w, bar_w) = key_widths(inner);
    let result = div()
        .w(w.z(KEY_RESULT_W))
        .flex_none()
        .flex()
        .justify_end()
        .child(w.text(
            k.result(),
            KEY_PX,
            if k.timed {
                w.c(|t| t.good)
            } else {
                w.c(|t| t.bad)
            },
            SEMIBOLD,
        ))
        .child(w.text(
            format!(" {}", duration(k.clock_ms)),
            KEY_PX,
            w.c(|t| t.ink),
            REGULAR,
        ));
    let whose = SharedString::from(shown_name(&k.who.name, hide_realms));
    let face = div()
        .h(w.z(KEY_H))
        .flex()
        .items_center()
        .gap(w.z(KEY_GAP))
        .child(
            div()
                .id(("key-who", i))
                .flex_none()
                .child(super::super::paint::dot(w.z(KEY_DOT), char_ink(&k.who, w)))
                .tooltip(move |window, cx| Tooltip::new(whose.clone()).build(window, cx)),
        )
        .child(
            div()
                .w(w.z(name_w))
                .flex_none()
                .min_w_0()
                .truncate()
                .child(w.text(k.name.clone(), KEY_PX, w.c(|t| t.ink), REGULAR)),
        )
        .child(par_bar(k.clock_ms, k.pars, k.timed, bar_w, w))
        .child(result);
    jump(("key", i), k.fight_id.clone(), ROW_RADIUS, w, cx)
        .test_support()
        .w_full()
        .child(face)
        .into_any_element()
}

/// A raid at its difficulty: each boss with how the week went on it —
/// "Killed in 7:02" or "Best 2%", the pull count — and one dot per pull.
/// `None` is the panel with nothing in it, which says so.
fn raid_panel(
    r: Option<&RaidPanel>,
    who: Option<&str>,
    hide_realms: bool,
    n: usize,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let id = ("home-raid", n);
    let Some(r) = r else {
        return panel(
            id,
            "Raid".to_string(),
            "one dot per pull",
            note(none_of("raid pulls", who), w),
            w,
        );
    };
    let mut list = div().flex().flex_col();
    for (i, b) in r.bosses.iter().enumerate() {
        // `.brow{border-top:1px solid var(--line)}`, the first's none.
        if i > 0 {
            list = list.child(chrome::hairline(w));
        }
        list = list.child(boss_row(b, n * 100 + i, hide_realms, w, cx));
    }
    let body = div().flex().flex_col().child(list).child(legend(
        vec![
            (legend_dot(w.c(|t| t.good), w), "kill".to_string()),
            (
                legend_ring(w.c(|t| t.ink_3), gpui_kit::transparent_black(), w),
                "wipe, ringed in the character's colour".to_string(),
            ),
        ],
        w,
    ));
    panel(id, r.title.clone(), "one dot per pull", body, w)
}

/// One boss (`.brow`): its name and outcome — a press opens its fastest
/// kill, else its newest pull — and under them a dot per pull, each a
/// press to that pull.
fn boss_row(b: &BossLine, i: usize, hide_realms: bool, w: &W, cx: &mut Context<Gui>) -> AnyElement {
    let (outcome, killed) = boss_words(b);
    let ink = if killed {
        w.c(|t| t.good)
    } else {
        w.c(|t| t.ink)
    };
    let head = div()
        .flex()
        .items_center()
        .gap(w.z(BOSS_GAP_X))
        .child(div().flex_1().min_w_0().truncate().child(w.text(
            b.name.clone(),
            BOSS_PX,
            w.c(|t| t.ink),
            REGULAR,
        )))
        .child(
            div()
                .flex_none()
                .flex()
                .child(w.text(outcome, BOSS_RIGHT_PX, ink, MEDIUM))
                .child(w.text(
                    format!(", {}", plural(b.pulls.len(), "pull")),
                    BOSS_RIGHT_PX,
                    w.c(|t| t.ink_2),
                    REGULAR,
                )),
        );
    let mut dots = div().flex().flex_wrap().items_center().gap(w.z(PDOT_GAP));
    for (j, d) in b.pulls.iter().enumerate() {
        dots = dots.child(pull_dot(d, i * 1000 + j, hide_realms, w, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap(w.z(BOSS_GAP))
        .py(w.z(BOSS_PAD_Y))
        .child(
            jump(("boss", i), b.fight_id.clone(), ROW_RADIUS, w, cx)
                .test_support()
                .child(head),
        )
        .child(dots)
        .into_any_element()
}

/// A pull's dot (`.pdot`): filled green on a kill, a ring in whose colour
/// on a wipe. A press opens the pull.
fn pull_dot(d: &PullDot, i: usize, hide_realms: bool, w: &W, cx: &mut Context<Gui>) -> AnyElement {
    let (fill, edge) = if d.kill {
        (w.c(|t| t.good), w.c(|t| t.good))
    } else {
        (gpui_kit::transparent_black(), char_ink(&d.who, w))
    };
    let words = SharedString::from(format!(
        "{}, {}",
        shown_name(&d.who.name, hide_realms),
        if d.kill { "kill" } else { "wipe" }
    ));
    let fight = d.fight_id.clone();
    div()
        .id(("boss-dot", i))
        .test_support()
        .flex_none()
        .size(w.z(PDOT))
        .rounded_full()
        .border(w.z(PDOT_EDGE))
        .border_color(edge)
        .bg(fill)
        .cursor_pointer()
        .tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.go_pull(Pull::Stored(fight.clone()), cx);
                cx.stop_propagation();
            }),
        )
        .into_any_element()
}

/// "Effective dps on keys": a dot per run in its character's colour, each
/// character's best ringed; a legend of whose dots are whose.
fn trend_panel(
    panels: &Panels,
    who: Option<&str>,
    hide_realms: bool,
    col_w: f32,
    w: &W,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let inner = (col_w - PANEL_PAD[1] - PANEL_PAD[3] - 2.0).max(1.0);
    let body = if panels.trend.is_empty() {
        note(
            if panels.keys.is_empty() {
                none_of("keys", who)
            } else {
                none_of("keys played as dps", who)
            },
            w,
        )
    } else {
        let mut seen: Vec<&Char> = Vec::new();
        for p in &panels.trend {
            if !seen.iter().any(|c| c.guid == p.who.guid) {
                seen.push(&p.who);
            }
        }
        let mut items: Vec<(AnyElement, String)> = seen
            .iter()
            .map(|c| {
                (
                    legend_dot(char_ink(c, w), w),
                    shown_name(&c.name, hide_realms),
                )
            })
            .collect();
        items.push((
            legend_ring(w.c(|t| t.legendary), gpui_kit::transparent_black(), w),
            "personal best".to_string(),
        ));
        div()
            .flex()
            .flex_col()
            .child(trend(
                &panels.trend,
                w.c(|t| t.ink_3),
                hide_realms,
                inner,
                w,
                cx,
            ))
            .child(legend(items, w))
    };
    panel(
        "home-trend",
        "Effective dps on keys".to_string(),
        "each dot a run",
        body,
        w,
    )
}

/// A legend (`.legend`): marks and their words, wrapping.
fn legend(items: Vec<(AnyElement, String)>, w: &W) -> Div {
    let mut line = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(w.z(LEGEND_GAP))
        .gap_y(w.z(LEGEND_INNER))
        .pt(w.z(LEGEND_ABOVE));
    for (mark, words) in items {
        line = line.child(
            div()
                .flex()
                .items_center()
                .gap(w.z(LEGEND_INNER))
                .child(mark)
                .child(w.text(words, LEGEND_PX, w.c(|t| t.ink_2), REGULAR)),
        );
    }
    line
}

/// A legend of words alone (`.legend span` with no mark).
fn legend_words(words: &'static str, w: &W) -> Div {
    div()
        .pt(w.z(LEGEND_ABOVE))
        .child(wrapping(w, words, LEGEND_PX, w.c(|t| t.ink_2)))
}

/// A legend's filled mark.
fn legend_dot(color: Hsla, w: &W) -> AnyElement {
    div()
        .flex_none()
        .size(w.z(LEGEND_DOT))
        .rounded_full()
        .bg(color)
        .into_any_element()
}

/// A legend's ring: `edge` round a `fill`.
fn legend_ring(edge: Hsla, fill: Hsla, w: &W) -> AnyElement {
    div()
        .flex_none()
        .size(w.z(LEGEND_DOT))
        .rounded_full()
        .border(w.z(LEGEND_RING))
        .border_color(edge)
        .bg(fill)
        .into_any_element()
}

/// Lay panels out in `cols` columns, padding the last row so a lone panel
/// keeps its column's width instead of stretching across the window. Every
/// panel of a row is as tall as the tallest: a flex row stretches its
/// children, as the prototype's CSS grid stretches its cards.
fn grid(panels: Vec<AnyElement>, cols: usize, w: &W) -> Div {
    let mut grid = div().flex().flex_col().gap(w.z(GAP));
    let mut panels = panels.into_iter().peekable();
    while panels.peek().is_some() {
        let mut line = div().flex().gap(w.z(GAP));
        for _ in 0..cols {
            line = line.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(panels.next()),
            );
        }
        grid = grid.child(line);
    }
    grid
}
