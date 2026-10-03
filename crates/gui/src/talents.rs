//! The talent viewer (plan phase 4, spec §8.1): a player's talent tree from
//! the per-machine dataset, drawn the way the game draws it — class pane
//! left, spec pane right, the picked hero tree between them under its
//! medallion — over the spec's background painting. Its logic (decode,
//! layout, edits under the game's rules, the paste store, tooltip lines) is
//! gui-logic's `talents`, shared with the iced viewer; this module draws it
//! on GPUI and turns gestures into its `Msg`s.
//!
//! The window opens it (`t`, or the inspector's "Talents and gear") as a
//! [`TalentViewer`] entity:
//!
//! - [`TalentViewer::open`] on a meter row's player — a stored simc paste,
//!   else the spec's empty tree;
//! - [`TalentViewer::ask_loadout`] sends `GetLoadout` through the window's
//!   `Session` and adopts the logged build when its `Reply` lands (a stored
//!   pull's answer already carries it: [`TalentViewer::adopt_logged`]);
//! - Esc emits [`TalentEvent::Close`]; the host drops the entity.
//!
//! Unlike iced, GPUI paints a canvas's images and paths in the order they
//! are written (spike S8), so each pane is ONE canvas: edges, icons, frames,
//! badges and the picker, with the painting under it all.

mod art;
mod inventory;
mod pane;
mod tip;

#[cfg(test)]
mod shots;
#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::{Easing, Sequence, Transition, transition};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, ClipboardItem, Context, Entity, EventEmitter, ExternalPaths, FocusHandle,
    Hsla, KeyBinding, MouseButton, Pixels, SharedString, Subscription, TestSupportExt as _, Window,
    div, font, img, px,
};
use wowdps_gui_logic::talents::{self as logic, Build, Msg, PaneModel, Tab, Viewer};
use wowdps_gui_logic::theme::{Def, TalentTokens};
use wowdps_model::Loadout;
use wowdps_proto::{ClientMsg, DaemonMsg, SegmentRef};

use crate::session::{Reply, Session};
use crate::theme::{Look, hsla};

/// The key context the viewer's own bindings live in: while it is open it
/// holds the keys, so the meter's never fire under it.
pub const TALENTS: &str = "Talents";

/// Esc: close the viewer.
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct CloseTalents;

/// Tab: flip between the talents and the inventory.
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = wowdps, no_json)]
pub struct FlipTab;

/// The viewer's bindings. The window registers them with its own.
pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("escape", CloseTalents, Some(TALENTS)),
        KeyBinding::new("tab", FlipTab, Some(TALENTS)),
    ]
}

/// What the viewer tells its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TalentEvent {
    /// Esc or ✕: the host drops the viewer.
    Close,
}

/// The meter row a viewer opens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    /// The row's label ("Name-Realm"), the paste store's key.
    pub name: String,
    pub spec_id: Option<u32>,
    /// The row's key: what `GetLoadout` asks about.
    pub guid: String,
}

/// How long a just-taken node's ripple runs, a lit path takes to grow, the
/// picker takes to fan out, and a tooltip to arrive. Reduced motion
/// settles each at once, onto the iced viewer's pixels.
const RIPPLE: Duration = Duration::from_millis(520);
const PATH_GROW: Duration = Duration::from_millis(240);
const FAN_OUT: Duration = Duration::from_millis(170);
const TIP_IN: Duration = Duration::from_millis(140);
/// How long "copy string" says "copied".
const COPIED_FOR: Duration = Duration::from_millis(1400);
/// The tree's smallest fit: below it the panes scroll rather than shrink.
const MIN_FIT: f32 = 0.7;
/// The biggest file a drop reads (a simc export is a few KiB).
const DROP_MAX: u64 = 1 << 20;

/// A process-wide id for the viewer's own requests, far from the window's.
fn next_req_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0x7A1E_0000);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub struct TalentViewer {
    pub(crate) ui: Viewer,
    input: Entity<InputState>,
    focus: FocusHandle,
    /// The `GetLoadout` this viewer is waiting on.
    pending: Option<u32>,
    /// Per pane (class, hero, spec), what its pointer was last over —
    /// iced's per-canvas hover, so a stale leave from one pane never
    /// clears a fresh enter in another.
    pane_hover: [Option<(u64, Option<u64>)>; 3],
    /// Nodes just taken, each with its generation: a ripple apiece.
    ripples: Vec<(u64, u64)>,
    ripple_gen: u64,
    /// Bumped on every tooltip target, so each one arrives anew.
    tip_gen: u64,
    /// The tree area's bounds at the last paint: the tooltip's frame.
    area: Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// "copy string" just copied, until this instant.
    copied_until: Option<std::time::Instant>,
    /// The trees' fit at the last frame (1 where they fit).
    pub(crate) last_fit: f32,
    /// Whether the trees fit the width at that fit: centred if so, else
    /// scrolled from their left edge.
    fits: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TalentEvent> for TalentViewer {}

impl TalentViewer {
    /// A viewer on `player` (or on nobody): a stored simc paste for that
    /// name wins, else the spec's empty tree, else the paste prompt.
    pub fn open(player: Option<Player>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("paste an in-game talent string…"));
        let typed = cx.subscribe_in(&input, window, |this, state, event, _, cx| match event {
            InputEvent::Change => {
                let text = state.read(cx).value().to_string();
                this.ui.on_msg(Msg::Input(text));
            }
            InputEvent::PressEnter { .. } => {
                let text = state.read(cx).value().to_string();
                this.ui.on_msg(Msg::Input(text));
                this.apply(Msg::Submit, cx);
            }
            InputEvent::Focus | InputEvent::Blur => {}
        });
        let ui = Viewer::open(player.map(|p| (p.name, p.spec_id)));
        Self {
            ui,
            input,
            focus: cx.focus_handle(),
            pending: None,
            pane_hover: [None; 3],
            ripples: Vec::new(),
            ripple_gen: 0,
            tip_gen: 0,
            area: Rc::new(std::cell::Cell::new(Bounds::default())),
            copied_until: None,
            last_fit: 1.0,
            fits: true,
            _subscriptions: vec![typed],
        }
    }

    /// The handle the host focuses so Esc and Tab reach the viewer.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Ask the daemon for `guid`'s COMBATANT_INFO loadout in `segment`; the
    /// build adopts when the answer comes back on `session`. A later ask
    /// replaces this one, and an answer to anything else is ignored.
    pub fn ask_loadout(
        &mut self,
        session: &Entity<Session>,
        segment: SegmentRef,
        guid: String,
        cx: &mut Context<Self>,
    ) {
        let req_id = next_req_id();
        self.pending = Some(req_id);
        let answered = cx.subscribe(session, |this, _, Reply(msg): &Reply, cx| {
            if let DaemonMsg::Loadout {
                req_id, loadout, ..
            } = msg
                && this.pending == Some(*req_id)
            {
                this.pending = None;
                // `None`: the daemon knows no loadout; the viewer keeps
                // what it opened with (a paste or the empty tree).
                if let Some(l) = loadout {
                    this.adopt_logged(l, cx);
                }
            }
        });
        self._subscriptions.push(answered);
        session.update(cx, |s, _| {
            s.request(ClientMsg::GetLoadout {
                req_id,
                segment,
                guid,
            });
        });
    }

    /// Install a logged loadout (the daemon's, or a stored pull's answer).
    pub fn adopt_logged(&mut self, loadout: &Loadout, cx: &mut Context<Self>) {
        self.ui.adopt_logged(loadout);
        cx.notify();
    }

    /// Read a paste (an import string or a whole simc export).
    pub fn paste(&mut self, text: &str, cx: &mut Context<Self>) {
        self.apply(Msg::Clipboard(Some(text.to_string())), cx);
    }

    /// One message through the logic, the host-owned ones handled here; a
    /// node that a click takes (or adds a rank to) gets its ripple.
    pub(crate) fn apply(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::Close => cx.emit(TalentEvent::Close),
            Msg::PasteClipboard => {
                let text = cx.read_from_clipboard().and_then(|item| item.text());
                self.ui.on_msg(Msg::Clipboard(text));
            }
            Msg::CopyString => {
                if let Some(s) = self.ui.encode_current() {
                    cx.write_to_clipboard(ClipboardItem::new_string(s));
                    self.copied_until = Some(std::time::Instant::now() + COPIED_FOR);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(COPIED_FOR).await;
                        let _ = this.update(cx, |this, cx| {
                            this.copied_until = None;
                            cx.notify();
                        });
                    })
                    .detach();
                }
            }
            msg => {
                let target = match &msg {
                    Msg::NodeClick(id) | Msg::PickChoice(id, _) => Some(*id),
                    _ => None,
                };
                let before = target.and_then(|id| self.ui.find_node(id)).map(|n| n.ranks);
                if let Msg::HoverSet(id, opt, ..) = msg
                    && self.ui.hover != Some((id, opt))
                {
                    self.tip_gen += 1;
                }
                self.ui.on_msg(msg);
                if let Some(id) = target {
                    let after = self.ui.find_node(id).map(|n| (n.ranks, n.selected));
                    let took = match (before, after) {
                        (Some(b), Some((a, true))) => a > b || b == 0,
                        _ => false,
                    };
                    if took {
                        self.ripple_gen += 1;
                        self.ripples.push((id, self.ripple_gen));
                    }
                }
            }
        }
        cx.notify();
    }

    /// A file dropped on the viewer: its text, as if pasted.
    fn drop_files(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let Some(path) = paths.paths().first() else {
            return;
        };
        let read = std::fs::metadata(path)
            .ok()
            .filter(|m| m.len() <= DROP_MAX)
            .and_then(|_| std::fs::read_to_string(path).ok());
        match read {
            Some(text) => self.paste(&text, cx),
            None => {
                self.ui.error = Some(format!("cannot read {}", path.display()));
                cx.notify();
            }
        }
    }
}

// ---- drawing ------------------------------------------------------------------

/// What every piece of the viewer draws with: the theme's window and
/// talent tokens, its face, and the tree's fit.
#[derive(Clone, Copy)]
pub(crate) struct Paint {
    pub def: &'static Def,
    pub t: TalentTokens,
    /// The trees' scale: 1 when they fit, less (to `MIN_FIT`) when not.
    pub s: f32,
}

impl Paint {
    pub fn w(
        &self,
        pick: impl FnOnce(&wowdps_gui_logic::theme::WindowTokens) -> wowdps_gui_logic::theme::Color,
    ) -> Hsla {
        hsla(pick(&self.def.window))
    }

    pub fn c(&self, pick: impl FnOnce(&TalentTokens) -> wowdps_gui_logic::theme::Color) -> Hsla {
        hsla(pick(&self.t))
    }

    /// Words that wrap at their container's width.
    pub fn words(&self, text: impl Into<SharedString>, size: f32, color: Hsla) -> gpui_kit::Div {
        div()
            .font_family(self.def.faces.ui)
            .text_size(px(size))
            .text_color(color)
            .child(text.into())
    }

    /// `text` at `size` in the window's face and `color`.
    pub fn text(&self, text: impl Into<SharedString>, size: f32, color: Hsla) -> gpui_kit::Div {
        div()
            .font_family(self.def.faces.ui)
            .text_size(px(size))
            .text_color(color)
            .whitespace_nowrap()
            .child(text.into())
    }

    /// A corner the viewer draws at `v` px, at the theme's shape.
    pub fn r(&self, v: f32) -> gpui_kit::Pixels {
        px(self.def.shape.radius(v))
    }

    /// A pill's corner, `v` its half-height, at the theme's shape.
    pub fn pill(&self, v: f32) -> gpui_kit::Pixels {
        px(self.def.shape.pill(v))
    }

    /// The tooltip's face: its box in `tip` inside `tip_edge`, and, where
    /// the theme is glass, a faint sheen down it and the specular rim
    /// along its top (the window's `W::float`).
    pub fn tip_face<E: Styled>(&self, el: E) -> E {
        let el = el.border_1().border_color(self.c(|t| t.tip_edge));
        if !self.def.effects.glass {
            return el.bg(self.c(|t| t.tip));
        }
        let w = &self.def.window;
        let sheen = w
            .glass_sheen
            .over(self.t.tip.alpha(1.0))
            .alpha(self.t.tip.a);
        el.bg(gpui_kit::linear_gradient(
            180.,
            gpui_kit::linear_color_stop(hsla(sheen), 0.),
            gpui_kit::linear_color_stop(self.c(|t| t.tip), 0.30),
        ))
        .shadow(vec![gpui_kit::BoxShadow {
            color: hsla(w.glass_rim),
            offset: gpui_kit::point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }])
    }
}

/// The tree row's natural width, as the iced viewer sums it.
fn content_width(b: &Build) -> f32 {
    let hero_w = b
        .hero_pane
        .as_ref()
        .map_or(0.0f32, |p| p.w + 16.0)
        .max(if b.hero.is_some() { 168.0 } else { 0.0 });
    b.class_pane.w + b.spec_pane.w + hero_w + 2.0 * 24.0 + 32.0
}

impl Render for TalentViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::global(cx);
        let def = look.def;
        // The viewer fills the window inside its 10 px margin; the trees
        // fit that width, down to `MIN_FIT`, before they scroll.
        let avail = f32::from(window.viewport_size().width) - 20.0;
        let s = self
            .ui
            .build
            .as_ref()
            .map_or(1.0, |b| (avail / content_width(b)).clamp(MIN_FIT, 1.0));
        self.last_fit = s;
        self.fits = self
            .ui
            .build
            .as_ref()
            .is_none_or(|b| content_width(b) * s <= avail + 0.5);
        let p = Paint {
            def,
            t: def.talents,
            s,
        };
        let motion = self.motion(window, cx);
        let ui = &self.ui;
        let (ink, ink_2) = (p.w(|t| t.ink), p.w(|t| t.ink_2));

        // The top line: who, what, and the way out.
        let mut top = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(p.text("talents", 16., ink));
        if let Some(player) = &ui.player {
            top = top.child(
                p.text(
                    player.split('-').next().unwrap_or(player).to_string(),
                    14.,
                    ink,
                )
                .font_weight(gpui_kit::FontWeight::SEMIBOLD),
            );
        }
        if let Some(b) = &ui.build {
            top = top.child(p.text(format!("{} — {}", b.class_name, b.spec_name), 12., ink_2));
        }
        let close = div()
            .id("talents-close")
            .test_support()
            .cursor_pointer()
            .child(p.text("✕", 14., ink_2))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.apply(Msg::Close, cx)),
            );
        top = top.child(div().flex_1()).child(close);

        let input = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().child(Input::new(&self.input).small()))
            .child(chip(
                &p,
                "paste-simc",
                "paste simc/string",
                false,
                Msg::PasteClipboard,
                cx,
            ));

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .size_full()
            .child(top)
            .child(input);

        if let Some(profile) = &ui.profile {
            body = body.child(p.text(logic::identity(profile), 12., ink_2));
            if profile.loadouts.len() > 1 {
                let mut chips = div()
                    .id("talent-loadouts")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .overflow_x_scroll()
                    .child(p.text("loadouts", 12., ink_2));
                for (i, l) in profile.loadouts.iter().enumerate() {
                    let label = if l.active {
                        "active".to_string()
                    } else {
                        l.name.clone()
                    };
                    chips = chips.child(chip(
                        &p,
                        ("loadout", i),
                        label,
                        i == ui.loadout_sel,
                        Msg::SelectLoadout(i),
                        cx,
                    ));
                }
                body = body.child(chips);
            }
        }
        if ui.has_inventory() {
            body = body.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(chip(
                        &p,
                        "tab-talents",
                        "talents",
                        ui.tab == Tab::Talents,
                        Msg::SetTab(Tab::Talents),
                        cx,
                    ))
                    .child(chip(
                        &p,
                        "tab-inventory",
                        "inventory",
                        ui.tab == Tab::Inventory,
                        Msg::SetTab(Tab::Inventory),
                        cx,
                    )),
            );
        }
        if let Some(e) = &ui.error {
            body = body.child(p.text(e.clone(), 12., p.w(|t| t.bad)));
        }

        // While the logged build is showing, its gear is the inventory (the
        // fight's actual equipment); a simc profile's returns with it.
        let content = match (ui.tab, &ui.profile, &ui.logged_gear) {
            (Tab::Inventory, _, Some(gear)) if ui.logged => inventory::logged(&p, gear),
            (Tab::Inventory, Some(profile), _) => inventory::profile(&p, profile),
            _ => self.talents_tab(&p, &motion, cx),
        };
        body = body.child(div().flex_1().min_h_0().child(content)).child(p.words(
            "click picks (+1 rank) · right-click refunds · octagons open their option picker · esc closes · tab flips inventory · drop a simc export here",
            12.,
            ink_2,
        ));

        div()
            .id("talents")
            .test_support()
            .track_focus(&self.focus)
            .key_context(TALENTS)
            .on_action(cx.listener(|this, _: &CloseTalents, _, cx| this.apply(Msg::Close, cx)))
            .on_action(cx.listener(|this, _: &FlipTab, _, cx| this.apply(Msg::ToggleTab, cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.drop_files(paths, cx)))
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style
                    .border_color(hsla(p.t.taken))
                    .bg(hsla(p.t.taken.alpha(0.04)))
            })
            .size_full()
            .p(px(10.))
            .border_1()
            .border_color(hsla(wowdps_gui_logic::theme::Color::TRANSPARENT))
            .bg(p.w(|t| t.ground))
            .font_family(def.faces.ui)
            .line_height(gpui_kit::relative(1.3))
            .text_color(ink)
            .child(body)
    }
}

/// The animated values one frame draws with, sampled while rendering.
pub(crate) struct Motion {
    /// How far each lit path has grown, by (from id, to id).
    pub paths: std::collections::HashMap<(u64, u64), f32>,
    /// How far the open picker has fanned out.
    pub fan: f32,
    /// Each live ripple: (node id, progress 0..1).
    pub ripples: Vec<(u64, f32)>,
    /// The tooltip's arrival (0..1).
    pub tip: f32,
}

impl TalentViewer {
    fn motion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Motion {
        let mut paths = std::collections::HashMap::new();
        if let Some(b) = &self.ui.build {
            for pane in b.panes() {
                for &(a, z) in &pane.edges {
                    let (Some(from), Some(to)) = (pane.nodes.get(a), pane.nodes.get(z)) else {
                        continue;
                    };
                    let lit = from.selected && to.selected;
                    let grown = transition(
                        (
                            SharedString::from(format!("talent-path-{}-{}", from.id, to.id)),
                            "grow",
                        ),
                        if lit { 1.0f32 } else { 0.0 },
                        Transition::new(PATH_GROW).easing(Easing::EaseOut),
                        window,
                        cx,
                    );
                    paths.insert((from.id, to.id), grown);
                }
            }
        }
        let fan = match self.ui.picker {
            Some(id) => *Sequence::new(SharedString::from(format!("talent-fan-{id}")), 0.0f32)
                .with_step(1.0, Transition::new(FAN_OUT).easing(Easing::EaseOut))
                .sample(window, cx)
                .value(),
            None => 1.0,
        };
        let mut live = Vec::new();
        let mut ripples = Vec::new();
        for &(id, generation) in &self.ripples {
            let sample = Sequence::new(
                SharedString::from(format!("talent-ripple-{id}-{generation}")),
                0.0f32,
            )
            .with_step(1.0, Transition::new(RIPPLE).easing(Easing::EaseOut))
            .sample(window, cx);
            if sample.status() != gpui_kit::base::MotionStatus::Finished {
                live.push((id, generation));
                ripples.push((id, *sample.value()));
            }
        }
        self.ripples = live;
        let tip = if self.ui.hover.is_some() {
            *Sequence::new(
                SharedString::from(format!("talent-tip-{}", self.tip_gen)),
                0.0f32,
            )
            .with_step(1.0, Transition::new(TIP_IN).easing(Easing::EaseOut))
            .sample(window, cx)
            .value()
        } else {
            1.0
        };
        Motion {
            paths,
            fan,
            ripples,
            tip,
        }
    }

    fn talents_tab(&self, p: &Paint, motion: &Motion, cx: &mut Context<Self>) -> AnyElement {
        let ui = &self.ui;
        let Some(b) = &ui.build else {
            return p
                .text(
                    "paste a talent string or a SimulationCraft export to see a build",
                    13.,
                    p.w(|t| t.ink_2),
                )
                .into_any_element();
        };
        let mut col = div().flex().flex_col().gap(px(6.)).size_full();
        for w in &b.warnings {
            col = col.child(p.text(format!("⚠ {w}"), 12., p.w(|t| t.bad)));
        }
        // A fixed-height provenance line: node details live in the hover
        // tooltip, so nothing here changes height while hovering.
        let mut provenance = div().flex().items_center().gap(px(8.)).child(p.text(
            format!("dataset build {}", b.dataset_build),
            12.,
            p.w(|t| t.ink_2),
        ));
        if ui.logged && !ui.edited {
            provenance = provenance.child(p.text("from combat log", 12., p.w(|t| t.good)));
        }
        if ui.edited {
            provenance = provenance.child(p.text("edited", 12., p.w(|t| t.label_ink)));
        }
        let copied = self
            .copied_until
            .is_some_and(|until| std::time::Instant::now() < until);
        provenance = provenance.child(chip(
            p,
            "copy-string",
            if copied { "copied ✓" } else { "copy string" },
            copied,
            Msg::CopyString,
            cx,
        ));
        col = col.child(provenance);

        let viewer = cx.entity().downgrade();
        let s = p.s;
        let pane_el = |i: usize, model: &Rc<PaneModel>| {
            pane::pane(pane::PaneArgs {
                index: i,
                model: Rc::clone(model),
                paint: *p,
                picker: ui.picker,
                hover: self.pane_hover.get(i).copied().flatten(),
                motion,
                viewer: viewer.clone(),
            })
        };
        let header = |icon: Option<crate::images::Tile>, name: &str, model: &PaneModel| {
            let (label, full) = logic::points_label(model.points, model.cap);
            div()
                .flex()
                .items_center()
                .gap(px(8. * s))
                .w(px(model.w.max(160.0) * s))
                .children(icon.map(|i| img(i).size(px(20. * s))))
                .child(p.text(name.to_uppercase(), 13. * s, p.c(|t| t.taken)))
                .child(div().flex_1())
                .child(p.text(
                    label,
                    12. * s,
                    if full {
                        p.c(|t| t.taken)
                    } else {
                        p.w(|t| t.ink_2)
                    },
                ))
        };
        let class = model_class(b.spec_id);
        let mut row = div()
            .flex()
            .items_start()
            .gap(px(24. * s))
            .pt(px(8.))
            .pr(px(16. * s))
            .pb(px(12.))
            .pl(px(16. * s))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(header(
                        class.and_then(art::class_icon),
                        &b.class_name,
                        &b.class_pane,
                    ))
                    .child(pane_el(0, &b.class_pane)),
            );
        if let Some((hero_id, hero_name)) = &b.hero {
            row = row.child(hero_column(
                p,
                *hero_id,
                hero_name,
                b.hero_pane.as_ref().map(|m| pane_el(1, m)),
                b.hero_pane.as_deref(),
            ));
        }
        row = row.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(header(
                    art::spec_icon(b.spec_id),
                    &b.spec_name,
                    &b.spec_pane,
                ))
                .child(pane_el(2, &b.spec_pane)),
        );
        // Centred while it fits; scrolling both ways when even the
        // smallest fit does not.
        let trees = div()
            .id("talent-trees")
            .size_full()
            .child(
                div()
                    .min_w_full()
                    .flex()
                    .when(self.fits, |d| d.justify_center())
                    .child(row),
            )
            .overflow_scrollbar();

        let area_cell = Rc::clone(&self.area);
        let mut area = div()
            .id("talent-area")
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            // Full bleed: the painting runs to the window's edges.
            .mx(px(-10.))
            .overflow_hidden()
            .child(
                gpui_kit::canvas(move |bounds, _, _| area_cell.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            );
        if let Some((painting, w, h)) = art::background(b.spec_id) {
            area = area
                .child(tip::backdrop(painting, w, h))
                .child(div().absolute().size_full().bg(p.c(|t| t.veil)));
        }
        area = area.child(trees);
        if let Some((node, requires)) = ui.hovered() {
            area = area.child(tip::tooltip(
                p,
                &node,
                &requires,
                ui.hover_at,
                self.area.get(),
                motion.tip,
            ));
        }
        col.child(area).into_any_element()
    }
}

/// The model's `Class` for a spec id, for the crest.
fn model_class(spec_id: u32) -> Option<wowdps_model::Class> {
    wowdps_model::Spec::from_id(spec_id).map(|s| s.class())
}

/// The centre column: the hero tree's medallion inside the game's golden
/// ring, its name, its points, and its mini-tree on a dark backplate.
fn hero_column(
    p: &Paint,
    hero_id: u32,
    hero_name: &str,
    pane: Option<AnyElement>,
    model: Option<&PaneModel>,
) -> AnyElement {
    // Measured off the ring crop's pixels: within its 192 px tile (mostly
    // drop-shadow padding) the gold circle's inner diameter is ~55%, so
    // the full-bleed medallion art shrinks to sit inside it.
    let ring_d = 168.0 * p.s;
    let medallion_d = ring_d * 0.56;
    let mut col = div().flex().flex_col().items_center().gap(px(6.));
    if let Some(art) = art::medallion(hero_id) {
        col = col.child(
            div()
                .relative()
                .size(px(ring_d))
                .flex()
                .items_center()
                .justify_center()
                .child(img(art).size(px(medallion_d)))
                .children(art::ring().map(|r| img(r).absolute().size(px(ring_d)))),
        );
    }
    col = col.child(p.text(hero_name.to_uppercase(), 14. * p.s, p.c(|t| t.taken)));
    if let (Some(pane), Some(model)) = (pane, model) {
        let (label, full) = logic::points_label(model.points, model.cap);
        col = col
            .child(p.text(
                label,
                12. * p.s,
                if full {
                    p.c(|t| t.taken)
                } else {
                    p.w(|t| t.ink_2)
                },
            ))
            .child(
                div()
                    .p(px(8. * p.s))
                    .bg(p.c(|t| t.plate))
                    .border_1()
                    .border_color(p.c(|t| t.plate_edge))
                    .rounded(p.r(10.))
                    .child(pane),
            );
    }
    col.into_any_element()
}

/// A small pill: the loadout picker's, the tabs' and the actions' unit.
fn chip(
    p: &Paint,
    id: impl Into<gpui_kit::ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    msg: Msg,
    cx: &mut Context<TalentViewer>,
) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .aria_selected(selected)
        .cursor_pointer()
        .py(px(3.))
        .px(px(8.))
        .rounded(p.pill(8.))
        .bg(p.c(|t| if selected { t.chip_on } else { t.chip }))
        .border_1()
        .border_color(p.c(|t| {
            if selected {
                t.chip_edge_on
            } else {
                t.chip_edge
            }
        }))
        .hover(|style| style.bg(hsla(p.t.chip_on)))
        .child(p.text(
            label,
            12.,
            if selected {
                p.w(|t| t.ink)
            } else {
                p.w(|t| t.ink_2)
            },
        ))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.apply(msg.clone(), cx)),
        )
        .into_any_element()
}

/// The badge text's font: the window's face.
pub(crate) fn face(p: &Paint) -> gpui_kit::Font {
    font(p.def.faces.ui)
}
