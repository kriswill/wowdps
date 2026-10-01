//! The pull rail (plan step 3.4; the iced window's `rail.rs`, the
//! prototype's `.rail`): ONE list of pulls, tonight's log and every stored
//! night, beside the stage above 1180 px and, at 1180 and under, a 280 px
//! drawer over a scrim. The model — nights, visits, pulls, the steps — is
//! gui-logic's `rail`; this module feeds it what the window holds, opens
//! what it lists, and draws it.
//!
//! The drawer closes on the scrim, Esc, Enter or a pick. While it is open
//! j, k and the arrows walk a highlight over its rows for Enter to open,
//! and `[` `]` step the stage leaving it open on the row they reach.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, Context, Div, MouseButton, PathBuilder, SharedString,
    TestSupportExt as _, canvas, div, point, px,
};
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::home::dungeon_name;
use wowdps_gui_logic::labels::shown_name;
use wowdps_gui_logic::rail::{
    self as rl, EMPTY, KeyWord, Line, Mark, More, Pull, Rail, Sources, Visit, Who, clock,
};
use wowdps_model::{Action, ListRow, Screen, SegmentId, SegmentKind};
use wowdps_proto::history::fight_id;

use super::paint::{dot, glyph};
use super::w::{MEDIUM, REGULAR, SEMIBOLD, W};
use super::{Gui, Place};

#[cfg(test)]
pub(crate) mod tests;
use crate::theme::hsla;

/// The rail beside the stage (`.body{grid-template-columns:236px …}`) and
/// the drawer it becomes (`.rail{width:280px}` under 1180 px).
pub const RAIL_W: f32 = 236.0;
pub const DRAWER_W: f32 = 280.0;
/// A pull's row (`.pull{padding:4px 12px 4px 14px;line-height:20px}`), and
/// its lead glyph's box and the glyph in it (`.oc`, `.oc svg`).
const PULL_H: f32 = 28.0;
const PULL_PAD: (f32, f32, f32, f32) = (4.0, 12.0, 4.0, 14.0);
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
/// border-radius:4px}`).
const HEAD_PAD: (f32, f32, f32, f32) = (10.0, 8.0, 8.0, 14.0);
const HEAD_GAP: f32 = 6.0;
const HEAD_PX: f32 = 15.0;
const TOGGLE_PX: f32 = 13.0;
const TOGGLE_PAD: (f32, f32) = (2.0, 6.0);
const TOGGLE_RADIUS: f32 = 4.0;
/// A night (`.night{padding-top:12px}`, `h3{13px 600;padding:0 14px 2px}`)
/// and a visit (`.visit{padding:6px 14px 3px;gap:7px;font-size:13px}`).
const NIGHT_PAD: (f32, f32, f32, f32) = (12.0, 14.0, 2.0, 14.0);
const LABEL_PX: f32 = 13.0;
const VISIT_PAD: (f32, f32, f32, f32) = (6.0, 14.0, 3.0, 14.0);
const VISIT_GAP: f32 = 7.0;
/// The trash dash (`.oc .dash{width:6px;height:1.5px;border-radius:1px}`).
const DASH_W: f32 = 6.0;
const DASH_H: f32 = 1.5;
/// "Show older nights" (`.rail-more{margin:14px 14px 0;padding:6px 10px;
/// border:1px dashed;border-radius:6px}`) and the list's foot.
const MORE_MARGIN: f32 = 14.0;
const MORE_PAD: (f32, f32) = (6.0, 10.0);
const MORE_RADIUS: f32 = 6.0;
const MORE_DASH: f32 = 4.0;
const MORE_GAP: f32 = 3.0;
const FOOT: f32 = 14.0;
/// The pin's mark in a row's left gutter, and what it says under the
/// pointer.
pub const PIN: &str = "★";
const PIN_PX: f32 = 11.0;
const PIN_INSET: f32 = 4.0;
pub const PIN_TIP: &str = "Pinned: retention keeps it (p)";
/// The drawer's shadow (`box-shadow:20px 0 50px rgba(0,0,0,.5)`).
const DRAWER_SHADOW: (f32, f32, f32) = (20.0, 50.0, 0.5);

// ---- the window's rail -----------------------------------------------------------

impl Gui {
    /// The rail as the window stands: tonight's log, and the store's pages
    /// under it.
    pub(crate) fn rail(&self, cx: &App) -> Rail {
        let state = self.session.read(cx).state();
        Rail::build(&Sources {
            entries: state.entries(),
            log_id: state.log_id(),
            watched: self.watched_row(cx),
            cards: &self.hist.store.earlier.cards,
            owner: self.picked(cx).and_then(|p| Some((Some(p.name), p.class?))),
            tonight: self.tonight(),
        })
    }

    /// The log segment the tailed log's state watches, as its snapshot has
    /// it now — the list's row with the snapshot's verdict, clock and
    /// liveness, which move before the list does.
    fn watched_row(&self, cx: &App) -> Option<(SegmentId, ListRow)> {
        let state = self.session.read(cx).state();
        if state.screen == Screen::List || state.segment_name().is_none() {
            return None;
        }
        let e = state.entries().get(state.segment_index())?;
        Some((
            e.id,
            ListRow {
                success: state.segment_success(),
                duration_ms: state.duration_ms(),
                live: state.is_live(),
                ..e.row.clone()
            },
        ))
    }

    /// The night "Tonight" is: the clock's, in the timezone the store's
    /// newest card was logged in (UTC with none) — or the one a test or a
    /// shot pinned.
    pub(crate) fn tonight(&self) -> i64 {
        if let Some(night) = self.hist.tonight_pin {
            return night;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        rl::tonight(
            now,
            self.hist.store.earlier.cards.first().and_then(|c| c.tz_min),
        )
    }

    /// The pull the stage stands on: the stored one, else the log segment
    /// the tailed log's state watches — none before it watches any.
    pub(crate) fn current_pull(&self, cx: &App) -> Option<Pull> {
        if let Some(s) = &self.hist.store.stored {
            return Some(Pull::Stored(s.fight_id.clone()));
        }
        let state = self.session.read(cx).state();
        if state.screen == Screen::List {
            return None;
        }
        state
            .entries()
            .get(state.segment_index())
            .map(|e| Pull::Log(e.id))
    }

    /// The tailed log's own segment for stored fight `id`, when the store's
    /// card is one of the log's pulls: it opens as the log's, which answers
    /// what the store cannot.
    fn log_segment_of(&self, id: &str, cx: &App) -> Option<SegmentId> {
        let state = self.session.read(cx).state();
        let log = state.log_id()?;
        state
            .entries()
            .iter()
            .find(|e| fight_id(log, e.row.start_ms, e.row.kind == SegmentKind::Overall) == id)
            .map(|e| e.id)
    }

    /// Put `pull` on the stage — a log segment through the tailed log's
    /// state, a stored pull through one of its own — and leave Home for
    /// it. The view and the player being inspected go with the reader.
    /// The drawer is the caller's: a pick closes it, a step of the keys
    /// leaves it open on the row it stepped to.
    pub(crate) fn go_pull(&mut self, pull: Pull, cx: &mut Context<Self>) {
        self.leave_home();
        let pull = match pull {
            Pull::Stored(id) => self
                .log_segment_of(&id, cx)
                .map_or(Pull::Stored(id), Pull::Log),
            log => log,
        };
        self.hist.rail.cursor = Some(pull.clone());
        self.hist.rail.reveal = Some(pull.clone());
        if self.current_pull(cx).as_ref() == Some(&pull) {
            cx.notify();
            return;
        }
        let (view, drill) = {
            let f = self.fight(cx);
            (f.view, f.drill.clone())
        };
        match pull {
            Pull::Log(id) => {
                // Back from the store onto the log: the view the log was on,
                // where a stored pull only stood in for it.
                let view = match self.hist.store.leave_stored() {
                    true => self.hist.log_view.take().unwrap_or(view),
                    false => view,
                };
                self.hist.log_view = None;
                self.act(
                    move |s| {
                        let Some(pos) = s.entries().iter().position(|e| e.id == id) else {
                            return Vec::new();
                        };
                        let here = s.screen != Screen::List
                            && s.entries().get(s.segment_index()).map(|e| e.id) == Some(id);
                        if here {
                            return if s.view != view {
                                s.apply(Action::SetView(view))
                            } else {
                                Vec::new()
                            };
                        }
                        s.view = view;
                        if drill.is_some() {
                            s.drill = drill;
                        }
                        s.goto_list_pos(pos)
                    },
                    cx,
                );
            }
            Pull::Stored(id) => {
                // Off the log onto a view the store does not keep: the stored
                // pull shows Damage, and the log's view waits for the step back.
                if self.hist.store.stored.is_none() && !view.is_stored() {
                    self.hist.log_view = Some(view);
                }
                let sent = self.hist.store.open_stored(id, view, drill);
                self.send_history(sent, cx);
            }
        }
        cx.notify();
    }

    /// Is there a newer pull up the rail, and an older one down it — or
    /// more of the store to ask for past its end? The header's ‹ ›.
    pub(crate) fn pull_steps(&self, cx: &App) -> (bool, bool) {
        let rail = self.rail(cx);
        let at = self.current_pull(cx);
        let newer = at.is_some()
            && rail
                .step(at.as_ref(), false, self.hist.rail.hide_trash)
                .is_some();
        let older = rail
            .step(at.as_ref(), true, self.hist.rail.hide_trash)
            .is_some()
            || (self.hist.store.earlier.answered && self.hist.store.earlier.more());
        (newer, older)
    }

    /// `[` (older) or `]`: the next pull along the rail, stored nights
    /// included. Past the store's last card in hand, `[` asks for the next
    /// page. From Home, where the rail lights no row, either starts at its
    /// top.
    pub(crate) fn rail_step(&mut self, older: bool, cx: &mut Context<Self>) {
        let at = (self.place == Place::Fights)
            .then(|| self.current_pull(cx))
            .flatten();
        match self
            .rail(cx)
            .step(at.as_ref(), older, self.hist.rail.hide_trash)
        {
            Some(pull) => self.go_pull(pull, cx),
            None if older => self.older_nights(cx),
            None => {}
        }
    }

    /// "Show older nights": the page after the oldest card in hand.
    pub(crate) fn older_nights(&mut self, cx: &mut Context<Self>) {
        self.hist.store.earlier.want_older();
        let sent = self.hist.store.requests(std::time::Instant::now());
        self.send_history(sent, cx);
        cx.notify();
    }

    /// The "Hide trash" toggle.
    pub(crate) fn toggle_trash(&mut self, cx: &mut Context<Self>) {
        self.hist.rail.hide_trash = !self.hist.rail.hide_trash;
        cx.notify();
    }

    /// Open the drawer on the pull the stage stands on.
    pub(crate) fn open_drawer(&mut self, cx: &mut Context<Self>) {
        self.cards.rail = true;
        self.hist.rail.cursor = self.current_pull(cx);
        self.hist.rail.reveal = self.hist.rail.cursor.clone();
        cx.notify();
    }

    pub(crate) fn close_drawer(&mut self, cx: &mut Context<Self>) {
        self.cards.rail = false;
        cx.notify();
    }

    /// `H`: the rail at the earlier nights — the drawer where the rail is
    /// one — asking for them when none is in hand.
    pub(crate) fn open_earlier(&mut self, cx: &mut Context<Self>) {
        if !self.hist.store.earlier.answered {
            if !self.hist.store.earlier.asking() {
                self.hist.store.earlier.want_newest();
            }
        } else if self.rail(cx).earlier().is_none() {
            self.hist.store.earlier.want_older();
        }
        let sent = self.hist.store.requests(std::time::Instant::now());
        self.send_history(sent, cx);
        self.cards.rail = true;
        self.hist.rail.cursor = self.current_pull(cx);
        self.hist.rail.earlier = true;
        cx.notify();
    }

    /// A key of the meter's keymap, as the rail hears it. While the drawer
    /// is open: j, k and the arrows walk its highlight, Enter opens the row
    /// and closes it, Esc closes it, `[` `]` step the stage leaving it open
    /// on the row they reach. Otherwise `[` `]` walk the rail's drawn
    /// order, stored nights included — unless the keys are in the
    /// inspector, whose ← → are its own. `true` when taken.
    pub(crate) fn rail_key(&mut self, action: Action, cx: &mut Context<Self>) -> bool {
        let step = matches!(action, Action::OlderSegment | Action::NewerSegment);
        if !self.cards.rail || super::w::Fit::of(self.width) == super::w::Fit::Wide {
            if step && !self.fight(cx).inspecting() {
                self.rail_step(action == Action::OlderSegment, cx);
                return true;
            }
            return false;
        }
        match action {
            Action::Down | Action::Up => {
                let from = self
                    .hist
                    .rail
                    .cursor
                    .clone()
                    .or_else(|| self.current_pull(cx));
                if let Some(to) = self.rail(cx).step(
                    from.as_ref(),
                    action == Action::Down,
                    self.hist.rail.hide_trash,
                ) {
                    self.hist.rail.reveal = Some(to.clone());
                    self.hist.rail.cursor = Some(to);
                    cx.notify();
                }
                true
            }
            Action::Open => {
                if let Some(pull) = self.hist.rail.cursor.clone() {
                    self.go_pull(pull, cx);
                }
                self.close_drawer(cx);
                true
            }
            Action::Back => {
                self.close_drawer(cx);
                true
            }
            Action::OlderSegment | Action::NewerSegment => {
                self.rail_step(action == Action::OlderSegment, cx);
                true
            }
            _ => false,
        }
    }
}

// ---- drawing ---------------------------------------------------------------------

/// The rail `width` wide (`.rail`): its head — "Pulls" and the trash
/// toggle — over the nights, on the panel's surface. The list's rows are
/// its scroll's own children, so a row asked into sight (a step of the
/// keys, the drawer opening, `H`'s earlier nights) is found by its place
/// among them and scrolled to once the layout has it.
pub fn panel(gui: &mut Gui, w: &W, width: f32, drawer: bool, cx: &mut Context<Gui>) -> AnyElement {
    let rail = gui.rail(cx);
    let at = (gui.place == Place::Fights)
        .then(|| gui.current_pull(cx))
        .flatten();
    // The keys' highlight, where the drawer has them.
    let cursor = drawer.then(|| gui.hist.rail.cursor.clone()).flatten();
    let hide_trash = gui.hist.rail.hide_trash;
    let shows = |l: &Line| l.shown(hide_trash, at.as_ref(), cursor.as_ref());
    let more = if gui.hist.store.earlier.asking() {
        More::Asking
    } else if gui.hist.store.earlier.more() {
        More::Offer
    } else {
        More::None
    };
    let toggle = div()
        .id("hide-trash")
        .test_support()
        .aria_selected(hide_trash)
        .cursor_pointer()
        .py(w.z(TOGGLE_PAD.0))
        .px(w.z(TOGGLE_PAD.1))
        .rounded(w.z(TOGGLE_RADIUS))
        .when(hide_trash, |d| d.bg(w.c(|t| t.raise)))
        .text_color(if hide_trash {
            w.c(|t| t.ink_2)
        } else {
            w.c(|t| t.ink_3_text)
        })
        .child(w.words("Hide trash", TOGGLE_PX, REGULAR))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.toggle_trash(cx);
                cx.stop_propagation();
            }),
        );
    let head = div()
        .flex()
        .items_center()
        .gap(w.z(HEAD_GAP))
        .pt(w.z(HEAD_PAD.0))
        .pr(w.z(HEAD_PAD.1))
        .pb(w.z(HEAD_PAD.2))
        .pl(w.z(HEAD_PAD.3))
        .child(w.text("Pulls", HEAD_PX, w.c(|t| t.ink), SEMIBOLD))
        .child(div().flex_1())
        .child(toggle);

    // `H`: the earlier nights' heading at the top of the list.
    let earlier = std::mem::take(&mut gui.hist.rail.earlier)
        .then(|| rail.earlier())
        .flatten();
    let reveal = gui.hist.rail.reveal.take();
    let mut list: Vec<AnyElement> = Vec::new();
    if rail.nights.is_empty() {
        list.push(
            night_pad(w)
                .child(
                    div()
                        .text_size(w.z(w.size.small))
                        .text_color(w.c(|t| t.ink_2))
                        .child(EMPTY),
                )
                .into_any_element(),
        );
    }
    let mut row = 0usize;
    for (i, n) in rail.nights.iter().enumerate() {
        // The visits the toggle leaves anything of: a night whose every
        // pull it hides is no heading over nothing.
        let visits: Vec<(&Visit, Vec<&Line>)> = n
            .visits
            .iter()
            .map(|v| (v, v.lines.iter().filter(|l| shows(l)).collect()))
            .filter(|(_, lines): &(&Visit, Vec<&Line>)| !lines.is_empty())
            .collect();
        if visits.is_empty() {
            continue;
        }
        if earlier == Some(i) {
            gui.hist.rail.scroll.scroll_to_top_of_item(list.len());
        }
        list.push(
            night_pad(w)
                .id(("night", i))
                .test_support()
                .child(w.text(n.label.clone(), LABEL_PX, w.c(|t| t.gold_dim), SEMIBOLD))
                .into_any_element(),
        );
        for (v, lines) in visits {
            list.push(visit_line(v, w, gui.cfg.hide_realms).into_any_element());
            for l in lines {
                if reveal.as_ref() == Some(&l.pull) {
                    gui.hist.rail.scroll.scroll_to_item(list.len());
                }
                let current = at.as_ref() == Some(&l.pull);
                let keyed = cursor.as_ref() == Some(&l.pull);
                list.push(pull_line(
                    l,
                    row,
                    current,
                    keyed,
                    w,
                    gui.cfg.hide_realms,
                    cx,
                ));
                row += 1;
            }
        }
    }
    match more {
        More::None => {}
        More::Offer => list.push(more_button(w, true, cx)),
        More::Asking => list.push(more_button(w, false, cx)),
    }
    list.push(div().h(w.z(FOOT)).flex_none().into_any_element());
    div()
        .id(if drawer { "rail-drawer" } else { "rail" })
        .test_support()
        .w(w.z(width))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .bg(w.c(|t| t.surface))
        .child(head)
        .child(super::chrome::hairline(w))
        .child(
            div()
                .id("rail-scroll")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .track_scroll(&gui.hist.rail.scroll)
                .children(list),
        )
        .into_any_element()
}

/// The rail over the stage as a drawer (`.app.rail-open .rail`): a scrim
/// that closes it, and the rail at the left with its shadow. The rail
/// takes its own presses, so none reaches the scrim under it.
pub fn drawer(gui: &mut Gui, w: &W, cx: &mut Context<Gui>) -> AnyElement {
    let rail = panel(gui, w, DRAWER_W, true, cx);
    let (x, blur, alpha) = DRAWER_SHADOW;
    let shadow = BoxShadow {
        color: hsla(wowdps_gui_logic::theme::Color::rgba(0.0, 0.0, 0.0, alpha)),
        offset: point(w.z(x), px(0.)),
        blur_radius: w.z(blur),
        spread_radius: px(0.),
        inset: false,
    };
    div()
        .absolute()
        .inset_0()
        .child(
            div()
                .id("rail-scrim")
                .test_support()
                .absolute()
                .inset_0()
                .bg(w.c(|t| t.rail_scrim))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_drawer(cx)),
                ),
        )
        .child(
            div()
                .id("rail-drawer-frame")
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .shadow(vec![shadow])
                .occlude()
                .child(rail),
        )
        .into_any_element()
}

/// A night's padding (`.night h3{padding:0 14px 2px}` after its 12 px).
fn night_pad(w: &W) -> Div {
    div()
        .w_full()
        .pt(w.z(NIGHT_PAD.0))
        .pr(w.z(NIGHT_PAD.1))
        .pb(w.z(NIGHT_PAD.2))
        .pl(w.z(NIGHT_PAD.3))
}

/// A character's dot, saying under the pointer whose it is.
fn who_dot(who: &Who, id: impl Into<gpui_kit::ElementId>, w: &W, hide_realms: bool) -> AnyElement {
    let name = SharedString::from(shown_name(&who.name, hide_realms));
    div()
        .id(id.into())
        .flex_none()
        .child(dot(w.z(DOT), hsla(who.color)))
        .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
        .into_any_element()
}

/// A visit's line (`.visit`): the character's dot and what it was.
fn visit_line(v: &Visit, w: &W, hide_realms: bool) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(w.z(VISIT_GAP))
        .pt(w.z(VISIT_PAD.0))
        .pr(w.z(VISIT_PAD.1))
        .pb(w.z(VISIT_PAD.2))
        .pl(w.z(VISIT_PAD.3))
        .children(v.dot.as_ref().map(|who| {
            who_dot(
                who,
                SharedString::from(format!("visit-dot-{}", v.title)),
                w,
                hide_realms,
            )
        }))
        .child(div().flex_1().min_w_0().truncate().child(w.text(
            v.title.clone(),
            LABEL_PX,
            w.c(|t| t.ink_3_text),
            REGULAR,
        )))
}

/// A pull's row (`.pull`): its glyph, its name, and at its right what it
/// came to — lit and edged in the accent when it is on the stage, ringed
/// in it when the drawer's keys are on another row (`keyed`). A press
/// opens it.
fn pull_line(
    l: &Line,
    row: usize,
    current: bool,
    keyed: bool,
    w: &W,
    hide_realms: bool,
    cx: &mut Context<Gui>,
) -> AnyElement {
    let (px_, ink) = match (l.trash, l.mark) {
        (true, _) => (TRASH_PX, w.c(|t| t.ink_3_text)),
        (_, Mark::Sum) => (NAME_PX, w.c(|t| t.ink_2)),
        _ => (NAME_PX, w.c(|t| t.ink)),
    };
    let quiet = |s: String| w.text(s, RIGHT_PX, w.c(|t| t.ink_2), REGULAR).flex_none();
    let mut right = div().flex().items_center().gap(w.z(RIGHT_GAP)).flex_none();
    if let Some(pct) = l.best_pct {
        right = right.child(quiet(format!("{pct}%")));
    }
    match l.key {
        Some(KeyWord::Plus(n)) => {
            right = right.child(w.text(format!("+{n}"), RIGHT_PX, w.c(|t| t.good), SEMIBOLD));
        }
        Some(KeyWord::Over) => {
            right = right.child(w.text("over", RIGHT_PX, w.c(|t| t.bad), SEMIBOLD));
        }
        None => {}
    }
    if let Some(who) = &l.dot {
        right = right.child(who_dot(who, ("pull-dot", row), w, hide_realms));
    }
    right = right.child(quiet(clock(l.duration_ms)));
    let body = div()
        .w_full()
        .h(w.z(PULL_H))
        .flex()
        .items_center()
        .gap(w.z(PULL_GAP))
        .pt(w.z(PULL_PAD.0))
        .pr(w.z(PULL_PAD.1))
        .pb(w.z(PULL_PAD.2))
        .pl(w.z(PULL_PAD.3))
        .child(mark(l.mark, w))
        .child(pull_name(&l.name, px_, ink, w))
        .child(right);
    let accent = w.accent();
    let pull = l.pull.clone();
    div()
        .id(("pull", row))
        .test_support()
        .aria_selected(current)
        .relative()
        .w_full()
        .cursor_pointer()
        .when(current, |d| d.bg(w.c(|t| t.raise)))
        .when(!current && keyed, |d| {
            d.bg(w.c(|t| t.hover))
                .border(w.z(CURSOR_RING))
                .border_color(accent)
                .rounded(w.z(CURSOR_RADIUS))
        })
        .when(!current && !keyed, |d| d.hover(|s| s.bg(w.c(|t| t.hover))))
        .child(body)
        .when(current, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(w.z(EDGE_W))
                    .bg(accent),
            )
        })
        // A pinned card's star stands in the row's left gutter, in the
        // quiet ink: the one gold at the rail's edge is the current row's.
        .when(l.pinned, |d| {
            d.child(
                div()
                    .id(("pin", row))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(w.z(PIN_INSET))
                    .flex()
                    .items_center()
                    .child(w.text(PIN, PIN_PX, w.c(|t| t.ink_3), REGULAR))
                    .tooltip(|window, cx| Tooltip::new(PIN_TIP).build(window, cx)),
            )
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.go_pull(pull.clone(), cx);
                this.cards.rail = false;
                cx.stop_propagation();
            }),
        )
        .into_any_element()
}

/// A pull's name, where the line lets it be: a key's level ("+14", the one
/// thing between two runs of a dungeon) is kept whole after its dungeon's
/// name, which is what gives way to "…".
fn pull_name(name: &str, size: f32, ink: gpui_kit::Hsla, w: &W) -> Div {
    let dungeon = dungeon_name(name);
    let level = name.get(dungeon.len()..).unwrap_or_default();
    let words = |s: &str| w.text(s.to_string(), size, ink, REGULAR);
    let name_box = div().flex_1().min_w_0().flex().items_center();
    if level.is_empty() {
        return name_box.child(div().min_w_0().truncate().child(words(name)));
    }
    name_box
        .child(div().min_w_0().truncate().child(words(dungeon)))
        .child(words(level).flex_none())
}

/// A row's lead glyph in its 16 px box.
pub(crate) fn mark(m: Mark, w: &W) -> AnyElement {
    let glyph_el = match m {
        Mark::Good => glyph(Glyph::Check, w.z(MARK_ICON), w.c(|t| t.good)).into_any_element(),
        Mark::Bad => glyph(Glyph::Close, w.z(MARK_ICON), w.c(|t| t.bad)).into_any_element(),
        Mark::Live => dot(w.z(w.size.dot), w.c(|t| t.bad)).into_any_element(),
        Mark::Sum => w
            .text("Σ", NAME_PX, w.c(|t| t.ink_3_text), REGULAR)
            .into_any_element(),
        Mark::Dash => div()
            .w(w.z(DASH_W))
            .h(w.z(DASH_H))
            .rounded(w.z(1.))
            .bg(w.c(|t| t.ink_3))
            .into_any_element(),
    };
    div()
        .size(w.z(MARK_BOX))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(glyph_el)
        .into_any_element()
}

/// "Show older nights" (`.rail-more`), or the word that a page is coming:
/// framed in a dashed edge, which is what says "more to load" rather than
/// "a button like the rest".
fn more_button(w: &W, offer: bool, cx: &mut Context<Gui>) -> AnyElement {
    let words = if offer {
        "Show older nights"
    } else {
        "Reading the history store…"
    };
    let edge = w.c(|t| t.edge);
    let (dash, gap, radius, zoom) = (MORE_DASH, MORE_GAP, MORE_RADIUS, w.zoom);
    let frame = canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let half = px(0.5);
            let inner = gpui_kit::Bounds::new(
                point(b.origin.x + half, b.origin.y + half),
                gpui_kit::size(b.size.width - px(1.), b.size.height - px(1.)),
            );
            let mut path =
                PathBuilder::stroke(px(1.)).dash_array(&[px(dash * zoom), px(gap * zoom)]);
            super::paint::rounded_rect(&mut path, inner, px(radius * zoom));
            if let Ok(p) = path.build() {
                window.paint_path(p, edge);
            }
        },
    )
    .absolute()
    .inset_0();
    let face = div()
        .id(if offer {
            "older-nights"
        } else {
            "older-nights-wait"
        })
        .test_support()
        .relative()
        .w_full()
        .py(w.z(MORE_PAD.0))
        .px(w.z(MORE_PAD.1))
        .flex()
        .justify_center()
        .text_color(w.c(|t| t.ink_3_text))
        .when(offer, |d| {
            d.cursor_pointer()
                .hover(|s| s.text_color(w.c(|t| t.ink_2)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.older_nights(cx);
                        cx.stop_propagation();
                    }),
                )
        })
        .child(frame)
        .child(w.words(words, RIGHT_PX, MEDIUM));
    div()
        .w_full()
        .pt(w.z(MORE_MARGIN))
        .px(w.z(MORE_MARGIN))
        .child(face)
        .into_any_element()
}
