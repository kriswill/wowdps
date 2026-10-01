//! The window's inspector (plan step 3.3, the prototype's `.insp`): master
//! and detail beside the meter — 520 px wide, 410 in a tile — and pushed
//! over the whole stage at 820 px and under while the keys are in it. It
//! follows the selection through `ClientState`'s opt-in follow-selection,
//! so every move of the meter's selection re-watches with that row as the
//! drill. The model is `model.rs`, the frame `view.rs`, the lists
//! `list.rs`, the recap `recap.rs`; the graph, the R21 matrices and the
//! death chips are their own components.

pub mod list;
pub mod model;
pub mod recap;
pub mod view;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Window, div};
use wowdps_gui_logic::inspect::curves::stack_keys;
use wowdps_model::{Action, Pane, Row, View};

use self::model::{Ctx, Deaths, Graph, Insp, Press, Stacks};
use crate::window::Gui;
use crate::window::w::{Fit, W};

/// The inspector's width beside the meter (`.split`): its maximum, which a
/// grid always gives it; a tile's.
pub const WIDE: f32 = wowdps_gui_logic::inspect::WIDE;
pub const TILE: f32 = wowdps_gui_logic::inspect::TILE;

impl Gui {
    /// The inspector for the stage, as owned data.
    pub(crate) fn insp(&self, w: &W, cx: &gpui_kit::App) -> Insp {
        let app = self.session.read(cx).state();
        let rows = app.rows();
        let owner = self.owner_in(&rows, app.view);
        let owner_of = |rows: &[Row]| self.owner_of(rows);
        Insp::of(&Ctx {
            app,
            st: &self.insp,
            t: &w.t,
            hide: self.cfg.hide_realms,
            owner,
            owner_of: &owner_of,
        })
    }

    /// How wide the inspector stands: beside the meter, else the stage.
    pub(crate) fn inspector_width(&self, w: &W) -> f32 {
        match w.fit() {
            Fit::Wide => WIDE,
            Fit::Tile => TILE,
            Fit::Narrow => w.width,
        }
    }

    /// What the window learns of each snapshot for the inspector: who is
    /// who (a meter's players, never the enemies), and where each stacked
    /// band sits.
    pub(crate) fn inspector_learns(&mut self, cx: &gpui_kit::App) {
        let app = self.session.read(cx).state();
        if app.view != View::EnemyTaken {
            let rows = app.rows();
            self.insp.roster.observe(&rows);
        }
        if let Some((context, keys)) = stack_keys(app) {
            self.insp.stack_slots.observe(&context, &keys);
        }
    }

    /// A press in the inspector.
    pub(crate) fn insp_press(&mut self, press: Press, window: &mut Window, cx: &mut Context<Self>) {
        match press {
            Press::PinCompare => self.act(|s| s.apply(Action::PickCompare), cx),
            Press::OpenTalents => self.open_talents(window, cx),
            Press::PickView(v) => self.act(move |s| s.apply(Action::SetView(v)), cx),
            Press::ToggleGraph => self.act(
                |s| {
                    s.toggle_graph();
                    Vec::new()
                },
                cx,
            ),
            Press::ToggleStack => {
                self.insp.stack_graph = !self.insp.stack_graph;
                cx.notify();
            }
            Press::CloseAbility => self.act(
                |s| {
                    if s.drill_spell().is_some() || s.compare_spell().is_some() {
                        s.apply(Action::Back)
                    } else {
                        Vec::new()
                    }
                },
                cx,
            ),
            Press::Tab(pane) => {
                self.act(
                    move |s| {
                        if let Some(d) = s.drill.as_mut() {
                            d.pane = pane;
                        }
                        Vec::new()
                    },
                    cx,
                );
                self.insp.stacks_open = false;
                cx.notify();
            }
            Press::ShowStacks(on) => {
                self.insp.stacks_open = on;
                cx.notify();
            }
            Press::SpellRow(i) => {
                self.insp.tree_cursor = None;
                self.act(
                    move |s| {
                        if let Some(d) = s.drill.as_mut() {
                            d.spell_sel = i;
                            d.pane = Pane::Spell;
                        }
                        s.inspect();
                        s.apply(Action::Open)
                    },
                    cx,
                );
            }
            Press::AttackerRow(i) => self.act(
                move |s| {
                    if let Some(d) = s.drill.as_mut() {
                        d.target_sel = i;
                        d.pane = Pane::Target;
                    }
                    s.inspect();
                    s.apply(Action::Open)
                },
                cx,
            ),
            Press::CompareSpell(key, label) => {
                self.act(move |s| s.drill_compare_spell(&key, &label), cx)
            }
            Press::Fold(key) => {
                if !self.insp.tree_open.remove(&key) {
                    self.insp.tree_open.insert(key);
                }
                cx.notify();
            }
            Press::PickDeath(i) => self.act(move |s| s.select_death(Some(i)), cx),
            Press::Uninspect => self.act(
                |s| {
                    s.uninspect();
                    Vec::new()
                },
                cx,
            ),
            Press::Sort(col) => {
                self.insp.drill_sort = match self.insp.drill_sort {
                    Some((c, true)) if c == col => Some((col, false)),
                    Some((c, false)) if c == col => None,
                    _ => Some((col, true)),
                };
                cx.notify();
            }
        }
    }
}

/// The graph's plot (the plot component lands with its own commit; until
/// then its place, as tall as iced's plot without lanes).
pub fn plot(_g: &Graph, _gui: &Gui, w: &W, _cx: &mut Context<Gui>) -> AnyElement {
    div()
        .id("inspector-plot")
        .h(w.z(96.))
        .w_full()
        .bg(w.c(|t| t.track))
        .into_any_element()
}

/// R9's death chips (a component of their own; none until it lands).
pub fn chips(_d: &Deaths, _w: &W, _cx: &Context<Gui>) -> Option<AnyElement> {
    None
}

/// R21's matrices (a component of their own; nothing until it lands).
pub fn matrix(_s: &Stacks, _w: &W, _cx: &Context<Gui>) -> AnyElement {
    div().into_any_element()
}
