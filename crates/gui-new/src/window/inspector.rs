//! The window's inspector (plan step 3.3, the prototype's `.insp`): master
//! and detail beside the meter — 520 px wide, 410 in a tile — and pushed
//! over the whole stage at 820 px and under while the keys are in it. It
//! follows the selection through `ClientState`'s opt-in follow-selection,
//! so every move of the meter's selection re-watches with that row as the
//! drill. The model is `model.rs`, the frame `view.rs`, the lists
//! `list.rs`, the recap `recap.rs`; the graph, the R21 matrices and the
//! death chips are their own components.

pub mod chips;
pub mod list;
pub mod matrix;
pub mod model;
pub mod plot;
pub mod recap;
pub mod view;

#[cfg(test)]
mod tests;

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Window, div};
use wowdps_gui_logic::inspect::curves::stack_keys;
use wowdps_gui_logic::inspect::matrix::matrices;
use wowdps_gui_logic::table::{sorted, step_in};
use wowdps_gui_logic::tree;
use wowdps_model::{Action, Pane, Row, Screen, View};
use wowdps_proto::ClientState;

use self::model::{Ctx, Deaths, Graph, Insp, Press, RangeTo, Stacks};
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

    /// A key of the shared keymap that is the inspector's (or the Deaths
    /// table's beside it) — iced's `tree_arrow`, `death_step`, `stage_key`
    /// and `filtered_step` in their order. `true` when it was answered here.
    pub(crate) fn inspector_key(&mut self, action: Action, cx: &mut Context<Self>) -> bool {
        let fit = Fit::of(self.width);
        let narrow = fit == Fit::Narrow;
        let app = self.session.read(cx).state();
        let inspecting = app.inspecting();
        match action {
            Action::OlderSegment | Action::NewerSegment if inspecting => {
                let right = action == Action::NewerSegment;
                self.tree_arrow(right, cx) || self.death_step(right, cx)
            }
            Action::Open => match app.screen {
                // Enter on a pair beside the meter does nothing.
                Screen::Compare => !narrow,
                // R25: the Deaths table keeps the keys; the recap has no row
                // to key (a narrow window's push is its one way to show it).
                Screen::Meter if deaths_table_keys(app) => {
                    if narrow {
                        self.act(
                            |s| {
                                s.inspect();
                                Vec::new()
                            },
                            cx,
                        );
                    }
                    true
                }
                Screen::Meter => self.tree_enter(cx),
                Screen::List => false,
            },
            Action::SwapPane | Action::ToggleGraph if app.screen == Screen::Meter => {
                if narrow && !inspecting {
                    self.act(
                        |s| {
                            s.inspect();
                            Vec::new()
                        },
                        cx,
                    );
                }
                action == Action::SwapPane && self.stacks_tab(cx)
            }
            Action::Up | Action::Down if inspecting && app.screen == Screen::Meter => {
                self.list_step(action == Action::Down, cx)
            }
            _ => false,
        }
    }

    /// R26: ← → while the keys are in a Damage or Healing ability list: →
    /// opens the keyed line's fold, ← shuts it — or, inside one, goes to
    /// the line that holds it. A key with nothing to do is swallowed rather
    /// than leave the fight.
    fn tree_arrow(&mut self, right: bool, cx: &mut Context<Self>) -> bool {
        let app = self.session.read(cx).state();
        let Some(lines) = model::tree_lines(app, &self.insp) else {
            return false;
        };
        let Some(line) = model::tree_keyed(app, &self.insp, &lines)
            .and_then(|at| lines.get(at).map(|l| (at, l.clone())))
        else {
            return true;
        };
        let (at, line) = line;
        match (right, line.fold, line.fold_key) {
            (true, Some(false), Some(k)) | (false, Some(true), Some(k)) => {
                self.fold(&k, cx);
            }
            (false, _, _) => {
                if let Some(up) = tree::parent(&lines, at)
                    .and_then(|p| lines.get(p))
                    .map(|l| l.node.clone())
                {
                    self.tree_rest(up, cx);
                }
            }
            _ => {}
        }
        true
    }

    /// v28: on a Deaths drill whose keys are in the inspector, ← → step the
    /// death windows; with one death the key is swallowed.
    fn death_step(&mut self, right: bool, cx: &mut Context<Self>) -> bool {
        let app = self.session.read(cx).state();
        if app.view != View::Deaths || app.drill.is_none() {
            return false;
        }
        let (deaths, shown) = app.deaths();
        let Some(last) = (deaths.len() as u32).checked_sub(1).filter(|l| *l > 0) else {
            return true;
        };
        let at = shown.unwrap_or(last);
        let to = if right {
            (at + 1).min(last)
        } else {
            at.saturating_sub(1)
        };
        self.act(move |s| s.select_death(Some(to)), cx);
        true
    }

    /// R26: Enter on the keyed tree line. A group's folds (it has no
    /// ability to open) and is answered here; a part's selects its row, so
    /// the state machine's Open opens that row.
    fn tree_enter(&mut self, cx: &mut Context<Self>) -> bool {
        let app = self.session.read(cx).state();
        if !app.inspecting() {
            return false;
        }
        let Some(line) = model::tree_lines(app, &self.insp).and_then(|lines| {
            model::tree_keyed(app, &self.insp, &lines).and_then(|at| lines.get(at).cloned())
        }) else {
            return false;
        };
        match (line.opens, line.fold_key) {
            (None, Some(key)) => {
                self.fold(&key, cx);
                true
            }
            (Some(i), _) => {
                self.act(
                    move |s| {
                        if let Some(d) = s.drill.as_mut() {
                            d.spell_sel = i;
                        }
                        Vec::new()
                    },
                    cx,
                );
                false
            }
            (None, None) => false,
        }
    }

    /// Tab on a Taken drill whose player has an R21 ledger: the Stacks tab
    /// joins the walk. `true` when this Tab was the walk's.
    fn stacks_tab(&mut self, cx: &mut Context<Self>) -> bool {
        let app = self.session.read(cx).state();
        let ledger = app.view == View::Taken
            && app.drill_spell().is_none()
            && app
                .drill_stacks()
                .is_some_and(|(s, c, b)| !matrices(s, c, b).is_empty());
        let Some(pane) = app.drill.as_ref().filter(|_| ledger).map(|d| d.pane) else {
            return false;
        };
        if self.insp.stacks_open {
            self.insp.stacks_open = false;
            self.act(
                |s| {
                    if let Some(d) = s.drill.as_mut() {
                        d.pane = Pane::Spell;
                    }
                    Vec::new()
                },
                cx,
            );
            true
        } else if pane == Pane::Target {
            self.insp.stacks_open = true;
            cx.notify();
            true
        } else {
            false
        }
    }

    /// j/k inside the inspector's list: a tree walks its drawn lines, a
    /// sorted ability list its drawn order; else the state machine's own
    /// step is right.
    fn list_step(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        let app = self.session.read(cx).state();
        if let Some(lines) = model::tree_lines(app, &self.insp) {
            let at = model::tree_keyed(app, &self.insp, &lines);
            if let Some(node) = tree::step(&lines, at, down)
                .and_then(|p| lines.get(p))
                .map(|l| l.node.clone())
            {
                self.tree_rest(node, cx);
            }
            return true;
        }
        let Some(d) = app.drill.as_ref() else {
            return false;
        };
        if d.spell.is_some() || d.pane != Pane::Spell || self.insp.drill_sort.is_none() {
            return false;
        }
        let (by_spell, _) = app.breakdown();
        let order: Vec<usize> = sorted(
            by_spell.into_iter().enumerate().collect(),
            self.insp.drill_sort,
        )
        .into_iter()
        .map(|(i, _)| i)
        .collect();
        let action = if down { Action::Down } else { Action::Up };
        if let Some(row) = step_in(&order, d.spell_sel, action) {
            self.act(
                move |s| {
                    if let Some(d) = s.drill.as_mut() {
                        d.spell_sel = row;
                    }
                    Vec::new()
                },
                cx,
            );
        }
        true
    }

    /// R26: rest the keys on `node`. A row's or a part's line also selects
    /// the row, so Enter opens its ability through the state machine.
    fn tree_rest(&mut self, node: tree::Node, cx: &mut Context<Self>) {
        let Some(key) = self
            .session
            .read(cx)
            .state()
            .drill
            .as_ref()
            .map(|d| d.key.clone())
        else {
            return;
        };
        let row = match node {
            tree::Node::Row(i) | tree::Node::Part(i, _) => Some(i),
            tree::Node::Group(_) => None,
        };
        if let Some(i) = row {
            self.act(
                move |s| {
                    if let Some(d) = s.drill.as_mut() {
                        d.spell_sel = i;
                    }
                    Vec::new()
                },
                cx,
            );
        }
        self.insp.tree_cursor = match node {
            tree::Node::Row(_) => None,
            other => Some((key, other)),
        };
        cx.notify();
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
            Press::Fold(key) => self.fold(&key, cx),
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

/// The graph's plot: a drag selects a zoom window — the drill's, or the
/// pair's, which the daemon echoes — and a right press gives the whole
/// fight back.
pub fn graph(g: &Graph, w: &W, cx: &Context<Gui>) -> AnyElement {
    let input = plot::Input {
        window: g.window,
        peak: g.peak,
        curves: g.curves.clone(),
        dead: g.dead.clone(),
        lanes: g.lanes.clone(),
        total: g.total,
        word: g.word,
    };
    let gui = cx.entity();
    let to = g.range_to;
    plot::plot("inspector-plot", input, w)
        .on_range(Rc::new(move |range, _, cx| {
            gui.update(cx, |g, cx| {
                g.act(
                    move |s| match to {
                        RangeTo::Drill => s.set_drill_range(range),
                        RangeTo::Compare => s.set_compare_range(range),
                    },
                    cx,
                );
            });
        }))
        .into_any_element()
}

/// R9's death chips: a press shows that death window's recap.
pub fn death_chips(d: &Deaths, w: &W, cx: &Context<Gui>) -> Option<AnyElement> {
    let gui = cx.entity();
    chips::chips(
        &d.windows,
        d.shown,
        d.dropped,
        w,
        Rc::new(move |i, window, cx| {
            gui.update(cx, |g, cx| g.insp_press(Press::PickDeath(i), window, cx));
        }),
    )
}

/// R21's matrices, one per stacking debuff, behind the Stacks tab.
pub fn stack_matrix(s: &Stacks, w: &W, cx: &Context<Gui>) -> AnyElement {
    matrix::matrix(
        "inspector-stacks",
        &matrices(&s.stacking, &s.cells, &s.base),
        s.dropped,
        w,
        cx,
    )
    .unwrap_or_else(|| div().into_any_element())
}

impl Gui {
    /// R26: open a fold of the ability tree, or shut it.
    fn fold(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.insp.tree_open.remove(key) {
            self.insp.tree_open.insert(key.to_string());
        }
        cx.notify();
    }
}

/// R25: the Deaths table holds the keys — on the stage (the Deaths view
/// with a raid timeline) and not handed to the inspector.
fn deaths_table_keys(app: &ClientState) -> bool {
    app.view == View::Deaths && app.raid().is_some() && !app.inspecting()
}

impl Gui {
    /// A step moved the inspector's keys: their line comes into sight on
    /// the next layout.
    pub(crate) fn reveal_insp(&mut self, cx: &mut Context<Self>) {
        if self.session.read(cx).state().inspecting() {
            self.insp.reveal.set(true);
        }
        cx.notify();
    }
}
