//! The command palette (the prototype's `.pal`, the iced window's
//! `palette`): Ctrl K, or a press on the top bar's jump box, puts a search
//! over everything the window can go to — the pulls on the rail, the
//! players of the pull on the stage, the views and the window's screens —
//! grouped and narrowed as the reader types (`gui_logic::palette`).
//!
//! The arrows or Ctrl N / Ctrl P move the selection, Enter runs it, Esc,
//! Ctrl K again or a press off the card closes it. While the card is up
//! every key is its own: its field holds the focus, so the meter's keymap
//! (`Meter && !Input`) is silent under it. The selection is the one line
//! lit — the pointer lights none, as the prototype's `.pal-it` has no
//! hover. Window-local: `ClientState` never learns it exists.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, Context, Div, ElementId, Entity, KeyBinding, MouseButton,
    ScrollHandle, Subscription, TestSupportExt as _, Window, div, point, px,
};
use wowdps_gui_logic::fight_head::player_chart;
use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::palette::{Item, Palette, Run, items, listed};
use wowdps_gui_logic::theme::SHADOW_SHEET;
use wowdps_model::{Action, View};

use super::chrome::{class_icon, hairline, kbd};
use super::field::Field;
use super::inspector::list::Keep;
use super::paint::glyph;
use super::top_bar::JUMP_WORDS;
use super::w::{REGULAR, SEMIBOLD, W};
use super::{Gui, Place, Reveal};
use crate::theme::hsla;

/// The card (`.pal{width:min(580px,calc(100% - 24px));border-radius:10px}`),
/// hung 64 px down over the scrim (`.overlay{padding-top:64px}`).
const CARD_W: f32 = 580.0;
const CARD_RADIUS: f32 = 10.0;
const CARD_TOP: f32 = 64.0;
const CARD_SIDE: f32 = 12.0;
/// The field's row (`.pal-in{gap:10px;padding:0 14px}`, `input{height:46px;
/// font-size:17px}`).
const IN_GAP: f32 = 10.0;
const IN_PAD_X: f32 = 14.0;
const IN_H: f32 = 46.0;
const IN_PX: f32 = 17.0;
/// The list (`.pal-list{max-height:430px;padding:4px 0 8px}`), a group's
/// heading (`.pal-g{font-size:12.5px;padding:10px 14px 3px;font-weight:
/// 600}`) and an item (`.pal-it{gap:10px;padding:6px 14px;font-size:15px}`,
/// `.h{gap:6px;font-size:13px}`), a player's disc (`.disc{20px}`).
const LIST_H: f32 = 430.0;
const LIST_PAD: (f32, f32) = (4.0, 8.0);
const GROUP_PX: f32 = 12.5;
const GROUP_PAD: (f32, f32, f32) = (10.0, 14.0, 3.0);
const ITEM_GAP: f32 = 10.0;
const ITEM_PAD: (f32, f32) = (6.0, 14.0);
const ITEM_PX: f32 = 15.0;
const SUB_PX: f32 = 13.0;
const SUB_GAP: f32 = 6.0;
const DISC: f32 = 20.0;
/// The selected line's edge (`box-shadow:inset 2px 0 0 var(--accent)`).
const SEL_EDGE: f32 = 2.0;
/// The list's scrollbar: thin, floating in the lines' 14 px right inset.
const LIST_BAR: f32 = 4.0;
/// The note when nothing matches (`.note{margin:10px 16px}`), its edge and
/// its words (`nav::note`).
const EMPTY_MARGIN: (f32, f32) = (10.0, 16.0);
const NOTE_EDGE: f32 = 2.0;
const NOTE_PAD: (f32, f32) = (8.0, 10.0);
const NOTE_PX: f32 = 13.0;

/// The card's words when nothing matches.
pub const NOTHING: &str = "Nothing matches. Try a boss, a player or a view.";

/// The palette's keys while its field has them.
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct PalStep(pub bool);

#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct PalClose;

/// The arrows and Ctrl N / Ctrl P move, Esc and Ctrl K close — the field's
/// own up and down given way to the palette's.
pub fn bindings() -> Vec<KeyBinding> {
    const IN: Option<&str> = Some("Palette > Input");
    vec![
        KeyBinding::new("down", PalStep(true), IN),
        KeyBinding::new("up", PalStep(false), IN),
        KeyBinding::new("ctrl-n", PalStep(true), IN),
        KeyBinding::new("ctrl-p", PalStep(false), IN),
        KeyBinding::new("escape", PalClose, IN),
        KeyBinding::new("ctrl-k", PalClose, IN),
    ]
}

/// The palette while it is up: what is typed and the selection, its field,
/// and its list's scroll.
pub struct PalState {
    pub p: Palette,
    pub input: Entity<InputState>,
    pub scroll: crate::scrollbar::Scroll,
    /// A step moved the selection: bring it into sight once laid out.
    pub reveal: Rc<Cell<bool>>,
    _typed: Subscription,
}

impl Gui {
    /// The palette's items for what is typed, as its card lists them: the
    /// rail's pulls, the players of the pull on the stage (none without
    /// one), the views and the screens. Nothing while it is shut.
    pub(crate) fn palette_items(&self, cx: &App) -> Vec<Item> {
        let Some(pal) = &self.pal else {
            return Vec::new();
        };
        let players = match self.current_pull(cx) {
            Some(_) => self.seen.players(self.fight(cx)),
            None => Vec::new(),
        };
        listed(
            items(
                &self.rail(cx),
                &players,
                &self.hist.known,
                self.cfg.hide_realms,
            ),
            &pal.p.query,
        )
    }

    /// Ctrl K, or the jump box: the palette, empty, its field focused —
    /// over whatever card was up, which it replaces. Ctrl K again (from
    /// the window: the field's own Ctrl K is [`PalClose`]) closes it.
    pub(crate) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pal.is_some() {
            self.close_palette(window, cx);
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(JUMP_WORDS));
        let typed = cx.subscribe_in(
            &input,
            window,
            |this, state, event, window, cx| match event {
                InputEvent::Change => {
                    let query = state.read(cx).value().to_string();
                    if let Some(pal) = this.pal.as_mut() {
                        pal.p.typed(query);
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.palette_submit(window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        input.update(cx, |i, cx| i.focus(window, cx));
        self.pal = Some(PalState {
            p: Palette::default(),
            input,
            scroll: crate::scrollbar::Scroll::new(),
            reveal: Rc::default(),
            _typed: typed,
        });
        self.cards.palette = true;
        self.cards.options = false;
        self.cards.sheet = false;
        self.cards.picker = false;
        cx.notify();
    }

    /// The palette shut, the keys back on the window.
    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pal = None;
        self.cards.palette = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The selection one step down or up, kept in sight.
    fn palette_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let n = self.palette_items(cx).len();
        if let Some(pal) = self.pal.as_mut() {
            pal.p.step(down, n);
            pal.reveal.set(true);
        }
        cx.notify();
    }

    /// Enter: run the selection.
    fn palette_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.palette_items(cx);
        let Some(pal) = self.pal.as_mut() else {
            return;
        };
        pal.p.clamp(items.len());
        if let Some(run) = items.get(pal.p.sel).map(|i| i.run.clone()) {
            self.run_palette(run, window, cx);
        }
    }

    /// Run an item: the palette closes, and the window goes where the item
    /// says — as the key or the press it stands for would take it.
    pub(crate) fn run_palette(&mut self, run: Run, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
        match run {
            Run::Pull(pull) => {
                self.cards.rail = false;
                self.go_pull(pull, cx);
            }
            // The player on the pull's meter, on a chart they have a row
            // on — a count's or the enemies' view gives way to Damage —
            // the filter cleared so their row is drawn, and in sight.
            Run::Player { key, label } => {
                self.leave_home();
                self.place = Place::Fights;
                self.cards.rail = false;
                self.clear_filter(window, cx);
                let to_damage = !player_chart(self.fight(cx).view);
                self.act(
                    move |s| {
                        let mut reqs = Vec::new();
                        if to_damage {
                            reqs.extend(s.apply(Action::SetView(View::Damage)));
                        }
                        reqs.extend(s.select_player(&key, &label));
                        reqs
                    },
                    cx,
                );
                let key = self.fight(cx).drill.as_ref().map(|d| d.key.clone());
                self.reveal.set(key.map(Reveal::Row));
            }
            Run::View(view) => {
                self.leave_home();
                self.cards.rail = false;
                self.pick_view(view, cx);
            }
            Run::Home => {
                if self.place != Place::Home {
                    self.open_home(cx);
                }
            }
            Run::Live => self.go_live(window, cx),
            Run::Earlier => self.open_earlier(cx),
            Run::HomeScope(guid) => self.scope_home(guid, cx),
            Run::Sheet => {
                self.cards.sheet = true;
                cx.notify();
            }
            Run::Talents => self.open_talents(window, cx),
        }
    }
}

/// What lifts the card off the window (`.pal`'s shadow).
fn sheet_shadow(w: &W) -> BoxShadow {
    let s = SHADOW_SHEET;
    BoxShadow {
        color: hsla(s.color),
        offset: point(w.z(s.offset.0), w.z(s.offset.1)),
        blur_radius: w.z(s.blur),
        spread_radius: px(0.),
        inset: false,
    }
}

/// The palette over the window: the scrim, which a press closes it from,
/// and the card — the field over the grouped list, the selection raised
/// with the accent's edge. Nothing under them hears the pointer.
pub fn view(gui: &Gui, w: &W, cx: &mut Context<Gui>) -> Option<AnyElement> {
    let pal = gui.pal.as_ref()?;
    let items = gui.palette_items(cx);
    let sel = pal.p.sel.min(items.len().saturating_sub(1));
    let input = div()
        .h(w.z(IN_H))
        .px(w.z(IN_PAD_X))
        .flex()
        .items_center()
        .gap(w.z(IN_GAP))
        .child(glyph(Glyph::Search, w.z(w.size.icon), w.c(|t| t.ink_3)))
        .child(
            Field::new("palette-input", &pal.input, w)
                .size(IN_PX)
                .key_context("Palette")
                .flex_1()
                .h_full()
                .on_action(
                    cx.listener(|this, PalStep(down): &PalStep, _, cx| {
                        this.palette_step(*down, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &PalClose, window, cx| this.close_palette(window, cx)),
                ),
        );
    let body: AnyElement = if items.is_empty() {
        div()
            .pt(w.z(LIST_PAD.0 + EMPTY_MARGIN.0))
            .pb(w.z(LIST_PAD.1 + EMPTY_MARGIN.0))
            .px(w.z(EMPTY_MARGIN.1))
            .child(note(NOTHING, w))
            .into_any_element()
    } else {
        let keep = Keep {
            scroll: ScrollHandle::clone(&pal.scroll),
            pending: pal.reveal.clone(),
        };
        let mut list = div()
            .id("palette-list")
            .test_support()
            .flex()
            .flex_col()
            .pt(w.z(LIST_PAD.0))
            .pb(w.z(LIST_PAD.1))
            .max_h(w.z(LIST_H))
            .overflow_y_scroll()
            .track_scroll(&pal.scroll);
        let mut group = None;
        for (i, item) in items.iter().enumerate() {
            if group != Some(item.group) {
                group = Some(item.group);
                list = list.child(
                    div()
                        .pt(w.z(GROUP_PAD.0))
                        .px(w.z(GROUP_PAD.1))
                        .pb(w.z(GROUP_PAD.2))
                        .child(w.text(item.group.name(), GROUP_PX, w.c(|t| t.gold_dim), SEMIBOLD)),
                );
            }
            list = list.child(line(i, item, i == sel, &keep, w, cx));
        }
        let style = crate::scrollbar::Style {
            width: w.z(LIST_BAR),
            ..w.scrollbar()
        };
        div()
            .relative()
            .child(list)
            .child(crate::scrollbar::bar(style, &pal.scroll))
            .into_any_element()
    };
    let card = div()
        .id("palette")
        .test_support()
        .w_full()
        .max_w(w.z(CARD_W))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(w.z(CARD_RADIUS))
        .border_1()
        .border_color(w.c(|t| t.edge))
        .bg(w.c(|t| t.surface))
        .shadow(vec![sheet_shadow(w)])
        .occlude()
        // A press on the card — a heading, its edge — is the card's own.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(input)
        .child(hairline(w))
        .child(body);
    Some(
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .id("palette-scrim")
                    .test_support()
                    .absolute()
                    .inset_0()
                    .bg(w.c(|t| t.scrim))
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .pt(w.z(CARD_TOP))
                    .px(w.z(CARD_SIDE))
                    .pb(w.z(CARD_SIDE))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(card),
            )
            .into_any_element(),
    )
}

/// One item (`.pal-it`): a player's disc, the title, and at the right its
/// words and its key; raised with the accent's edge when selected. A
/// press runs it.
fn line(i: usize, item: &Item, selected: bool, keep: &Keep, w: &W, cx: &Context<Gui>) -> Div {
    let mut face = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(w.z(ITEM_GAP))
        .py(w.z(ITEM_PAD.0))
        .px(w.z(ITEM_PAD.1));
    if let Some((class, spec)) = item.disc {
        face = face.child(class_icon(w, class, spec, w.z(DISC), false));
    }
    face = face.child(
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .child(w.text(item.title.clone(), ITEM_PX, w.c(|t| t.ink), REGULAR)),
    );
    let mut right = div().flex_none().flex().items_center().gap(w.z(SUB_GAP));
    if !item.sub.is_empty() {
        right = right.child(w.text(item.sub.clone(), SUB_PX, w.c(|t| t.ink_3_text), REGULAR));
    }
    if let Some(key) = item.key {
        right = right.child(kbd(w, key));
    }
    let run = item.run.clone();
    let mut cell = div()
        .id(ElementId::NamedInteger("palette-item".into(), i as u64))
        .test_support()
        .relative()
        .flex()
        .cursor_pointer()
        .when(selected, |d| d.bg(w.c(|t| t.raise)))
        .child(face.child(right))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.run_palette(run.clone(), window, cx);
            }),
        );
    if selected {
        cell = cell.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(w.z(SEL_EDGE))
                .bg(w.accent()),
        );
        if keep.pending.get() {
            cell = cell.child(keep.probe());
        }
    }
    div().child(cell)
}

/// A note (`nav::note`): an edge, then quiet words.
fn note(words: &'static str, w: &W) -> Div {
    div()
        .flex()
        .child(div().w(w.z(NOTE_EDGE)).flex_none().bg(w.c(|t| t.edge)))
        .child(div().py(w.z(NOTE_PAD.0)).px(w.z(NOTE_PAD.1)).child(w.text(
            words,
            NOTE_PX,
            w.c(|t| t.ink_3_text),
            REGULAR,
        )))
}

#[cfg(test)]
mod tests;
