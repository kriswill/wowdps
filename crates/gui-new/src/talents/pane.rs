//! One pane of the tree as one canvas, painted in the iced viewer's layer
//! order — paths and their arrows, the dark backing under each icon, the
//! icon, its frame and carets, its rank badge, the hover ring, the open
//! choice picker — which GPUI keeps (spike S8), where iced needed two
//! stacked canvases. Every number is gui-logic's pane geometry times the
//! tree's fit `s`; the pointer is mapped back through the same `s`.
//!
//! The pane's own pointer handling mirrors the iced canvas's: an enter or
//! leave per pane, a left press picks (or opens a choice's picker, or
//! picks one of its options, or closes it), a right press refunds.

use std::f32::consts::PI;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Corners, CursorStyle, DispatchPhase, Hitbox, HitboxBehavior, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, Point, SharedString, TestSupportExt as _,
    TextAlign, TextRun, WeakEntity, Window, canvas, div, point, px, size,
};
use wowdps_gui_logic::spell_icons::IconStyle;
use wowdps_gui_logic::talents::{self as logic, Msg, PaneModel, TILE};

use super::{Motion, Paint, TalentViewer, art, face};
use crate::theme::hsla;

pub(crate) struct PaneArgs<'a> {
    /// Which pane: 0 class, 1 hero, 2 spec.
    pub index: usize,
    pub model: Rc<PaneModel>,
    pub paint: Paint,
    /// The open choice picker's node, any pane's.
    pub picker: Option<u64>,
    /// What this pane's pointer is over.
    pub hover: Option<(u64, Option<u64>)>,
    pub motion: &'a Motion,
    pub viewer: WeakEntity<TalentViewer>,
}

/// The pane's canvas, its own test target (`talent-pane-<index>`).
pub(crate) fn pane(args: PaneArgs<'_>) -> gpui_kit::AnyElement {
    let PaneArgs {
        index,
        model,
        paint,
        picker,
        hover,
        motion,
        viewer,
    } = args;
    let s = paint.s;
    let (w, h) = (model.w * s, model.h * s);
    // The motion this pane draws, copied out of the frame's samples.
    let paths: Vec<f32> = model
        .edges
        .iter()
        .map(|&(a, z)| {
            match (model.nodes.get(a), model.nodes.get(z)) {
                (Some(from), Some(to)) => motion.paths.get(&(from.id, to.id)).copied(),
                _ => None,
            }
            .unwrap_or(1.0)
        })
        .collect();
    let ripples: Vec<(u64, f32)> = motion
        .ripples
        .iter()
        .filter(|(id, _)| model.nodes.iter().any(|n| n.id == *id))
        .copied()
        .collect();
    let fan = motion.fan;
    let painter = Painter {
        model,
        paint,
        picker,
        hover,
        paths,
        ripples,
        fan,
    };
    div()
        .id(SharedString::from(format!("talent-pane-{index}")))
        .test_support()
        .w(px(w))
        .h(px(h))
        .child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, cx| {
                    painter.paint(bounds, window, cx);
                    painter.listen(index, bounds, hitbox, viewer, window);
                },
            )
            .size_full(),
        )
        .into_any_element()
}

struct Painter {
    model: Rc<PaneModel>,
    paint: Paint,
    picker: Option<u64>,
    hover: Option<(u64, Option<u64>)>,
    /// Each edge's lit growth (0 unlit, 1 lit), in `model.edges` order.
    paths: Vec<f32>,
    ripples: Vec<(u64, f32)>,
    fan: f32,
}

/// A closed polygon through `pts`.
fn polygon(path: &mut PathBuilder, pts: &[Point<Pixels>]) {
    let mut iter = pts.iter();
    if let Some(first) = iter.next() {
        path.move_to(*first);
        for p in iter {
            path.line_to(*p);
        }
        path.close();
    }
}

/// A circle as a 48-gon: finer than any frame needs at 28 px.
fn circle(path: &mut PathBuilder, c: Point<Pixels>, r: Pixels) {
    let pts: Vec<Point<Pixels>> = (0..48)
        .map(|k| {
            let a = k as f32 / 48.0 * 2.0 * PI;
            point(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    polygon(path, &pts);
}

impl Painter {
    /// A pane point (pane pixels) on the canvas at `origin`.
    fn at(&self, origin: Point<Pixels>, (x, y): logic::Pt) -> Point<Pixels> {
        point(
            origin.x + px(x * self.paint.s),
            origin.y + px(y * self.paint.s),
        )
    }

    /// The node shape's outline round `c` (canvas pixels), half-extent `r`
    /// in pane pixels.
    fn shape(&self, path: &mut PathBuilder, c: Point<Pixels>, r: f32, shape: IconStyle) {
        let r = r * self.paint.s;
        match shape {
            IconStyle::Circle => circle(path, c, px(r)),
            IconStyle::Square => polygon(
                path,
                &[
                    point(c.x - px(r), c.y - px(r)),
                    point(c.x + px(r), c.y - px(r)),
                    point(c.x + px(r), c.y + px(r)),
                    point(c.x - px(r), c.y + px(r)),
                ],
            ),
            IconStyle::Octagon => {
                let pts = logic::octagon(f32::from(c.x), f32::from(c.y), r)
                    .map(|(x, y)| point(px(x), px(y)));
                polygon(path, &pts);
            }
        }
    }

    fn fill_shape(
        &self,
        window: &mut Window,
        c: Point<Pixels>,
        r: f32,
        shape: IconStyle,
        color: Hsla,
    ) {
        let mut path = PathBuilder::fill();
        self.shape(&mut path, c, r, shape);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }

    fn stroke_shape(
        &self,
        window: &mut Window,
        c: Point<Pixels>,
        r: f32,
        shape: IconStyle,
        width: f32,
        color: Hsla,
    ) {
        let mut path = PathBuilder::stroke(px(width * self.paint.s));
        self.shape(&mut path, c, r, shape);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }

    fn line(
        &self,
        window: &mut Window,
        a: Point<Pixels>,
        b: Point<Pixels>,
        width: f32,
        color: Hsla,
    ) {
        let mut path = PathBuilder::stroke(px(width * self.paint.s));
        path.move_to(a);
        path.line_to(b);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }

    fn icon(&self, window: &mut Window, c: Point<Pixels>, tile: crate::images::Tile) {
        let half = px(TILE / 2.0 * self.paint.s);
        let at = Bounds::new(point(c.x - half, c.y - half), size(half * 2., half * 2.));
        let _ = window.paint_image(at, at, Corners::default(), tile, 0, false);
    }

    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let s = self.paint.s;
        let t = self.paint.t;
        let gold = hsla(t.gold);
        let nodes = &self.model.nodes;

        // Paths under the tiles: a taken path is gold and carries an
        // arrowhead into the node the point flowed to; the rest stay faint.
        // A path just lit grows from its parent (the first delight); one
        // fully grown is the iced viewer's line exactly.
        for (&(a, z), &grown) in self.model.edges.iter().zip(&self.paths) {
            let (Some(from), Some(to)) = (nodes.get(a), nodes.get(z)) else {
                continue;
            };
            let (p, q) = (self.at(o, (from.x, from.y)), self.at(o, (to.x, to.y)));
            if grown < 1.0 {
                self.line(window, p, q, 1.5, hsla(t.path));
            }
            if grown > 0.0 {
                let tip = point(p.x + (q.x - p.x) * grown, p.y + (q.y - p.y) * grown);
                self.line(window, p, tip, 2.0, hsla(t.gold.alpha(0.85)));
            }
            // The arrow arrives over the last stretch of the growth.
            let arrow = ((grown - 0.8) / 0.2).clamp(0.0, 1.0);
            if arrow > 0.0 {
                let head =
                    logic::arrowhead((from.x, from.y), (to.x, to.y)).map(|pt| self.at(o, pt));
                let mut path = PathBuilder::fill();
                polygon(&mut path, &head);
                if let Ok(path) = path.build() {
                    window.paint_path(path, gold.opacity(arrow));
                }
            }
        }

        for n in nodes {
            let c = self.at(o, (n.x, n.y));
            // A dark backing so the shaped icon's clipped corners read as
            // the shape even over bright background art.
            self.fill_shape(window, c, TILE / 2.0 + 1.5, n.shape, hsla(t.backing));
            // Coloured art for what the build has; untaken talents are
            // desaturated and dimmed, as every talent UI mutes them.
            match art::spell(n.spell_id, n.shape, !n.selected) {
                Some(tile) => self.icon(window, c, tile),
                None => self.fill_shape(
                    window,
                    c,
                    TILE / 2.0 - 2.0,
                    n.shape,
                    hsla(t.blank.alpha(if n.selected { 0.30 } else { 0.10 })),
                ),
            }
            // The frame: gold taken, teal granted, green available, faint
            // grey out of reach.
            let border = if n.granted {
                t.granted
            } else if n.selected {
                t.gold
            } else if n.available {
                t.available
            } else {
                t.locked
            };
            self.stroke_shape(
                window,
                c,
                TILE / 2.0 + 1.5,
                n.shape,
                if n.selected || n.available { 2.0 } else { 1.0 },
                hsla(border),
            );
            if n.choice {
                for caret in logic::carets(n) {
                    let mut path = PathBuilder::fill();
                    polygon(&mut path, &caret.map(|pt| self.at(o, pt)));
                    if let Ok(path) = path.build() {
                        let a = if n.selected { 1.0 } else { 0.35 };
                        window.paint_path(path, hsla(border.alpha(a)));
                    }
                }
            }
            // The rank badge: white on black over the tile's lower right
            // corner, clear of the paths.
            let (words, [bx, by, bw, bh]) = logic::badge(n);
            let plate = Bounds::new(self.at(o, (bx, by)), size(px(bw * s), px(bh * s)));
            window.paint_quad(gpui_kit::fill(plate, hsla(t.badge)).corner_radii(px(2. * s)));
            let run = TextRun {
                len: words.len(),
                font: face(&self.paint),
                color: hsla(t.badge_ink),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let font_size = px(9. * s);
            let shaped =
                window
                    .text_system()
                    .shape_line(SharedString::from(words), font_size, &[run], None);
            let line_h = px(bh * s);
            let _ = shaped.paint(
                point(plate.center().x - shaped.width / 2., plate.origin.y),
                line_h,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }

        // Just taken: a gold ring spreads and fades (a delight; reduced
        // motion never starts one).
        for &(id, v) in &self.ripples {
            if let Some(n) = nodes.iter().find(|n| n.id == id) {
                let c = self.at(o, (n.x, n.y));
                self.stroke_shape(
                    window,
                    c,
                    TILE / 2.0 + 2.0 + 10.0 * v,
                    n.shape,
                    2.0,
                    gold.opacity(0.7 * (1.0 - v)),
                );
            }
        }

        if let Some((id, None)) = self.hover
            && let Some(n) = nodes.iter().find(|n| n.id == id)
        {
            let c = self.at(o, (n.x, n.y));
            self.stroke_shape(
                window,
                c,
                TILE / 2.0 + 4.0,
                n.shape,
                1.5,
                hsla(t.hover_ring),
            );
        }

        // The open choice picker: its options fanned out through the node
        // (from the node itself, as it opens), the current pick ringed gold.
        if let Some(node) = logic::picker_node(&self.model, self.picker) {
            let spots: Vec<logic::Pt> = logic::picker_spots(&self.model, node)
                .into_iter()
                .map(|(x, y)| {
                    (
                        node.x + (x - node.x) * self.fan,
                        node.y + (y - node.y) * self.fan,
                    )
                })
                .collect();
            if let Some([x, y, w, h]) = logic::picker_plate(&spots, node) {
                let plate = Bounds::new(self.at(o, (x, y)), size(px(w * s), px(h * s)));
                window.paint_quad(
                    gpui_kit::fill(plate, hsla(t.picker.alpha(t.picker.a * self.fan.max(0.4))))
                        .corner_radii(px(8. * s)),
                );
            }
            for (i, (spot, opt)) in spots.iter().zip(node.options.iter()).enumerate() {
                let c = self.at(o, *spot);
                match art::spell(opt.spell_id, IconStyle::Octagon, false) {
                    Some(tile) => self.icon(window, c, tile),
                    None => self.fill_shape(
                        window,
                        c,
                        TILE / 2.0 - 2.0,
                        IconStyle::Octagon,
                        hsla(t.blank.alpha(0.25)),
                    ),
                }
                let current = node.selected && node.spell_id == opt.spell_id;
                let hovered = self.hover == Some((node.id, Some(i as u64)));
                let ring = if hovered {
                    t.hover_ring
                } else if current {
                    t.gold
                } else {
                    t.option_ring
                };
                self.stroke_shape(
                    window,
                    c,
                    TILE / 2.0 + 2.5,
                    IconStyle::Octagon,
                    2.0,
                    hsla(ring),
                );
            }
        }
    }

    /// The pane's pointer: enter/leave, left and right presses, the hand
    /// over anything that answers a press.
    fn listen(
        &self,
        index: usize,
        bounds: Bounds<Pixels>,
        hitbox: Hitbox,
        viewer: WeakEntity<TalentViewer>,
        window: &mut Window,
    ) {
        let s = self.paint.s;
        let local = move |p: Point<Pixels>| {
            (
                f32::from(p.x - bounds.origin.x) / s,
                f32::from(p.y - bounds.origin.y) / s,
            )
        };
        let (x, y) = local(window.mouse_position());
        if hitbox.is_hovered(window) && logic::hit(&self.model, self.picker, x, y).is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }

        let model = Rc::clone(&self.model);
        let picker = self.picker;
        let moves = viewer.clone();
        let over_box = hitbox.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = local(e.position);
            let over = over_box
                .is_hovered(window)
                .then(|| logic::hit(&model, picker, x, y))
                .flatten();
            let key = over.map(|(id, opt, _)| (id, opt));
            let _ = moves.update(cx, |v, cx| {
                let Some(slot) = v.pane_hover.get_mut(index) else {
                    return;
                };
                if *slot == key {
                    return;
                }
                let prev = std::mem::replace(slot, key);
                // The tile's centre in window coordinates: the tooltip
                // anchors beside the icon, not the pointer.
                let msg = match (over, prev) {
                    (Some((id, opt, (cx_, cy_))), _) => Msg::HoverSet(
                        id,
                        opt,
                        f32::from(bounds.origin.x) + cx_ * s,
                        f32::from(bounds.origin.y) + cy_ * s,
                    ),
                    (None, Some((id, _))) => Msg::HoverClear(id),
                    (None, None) => return,
                };
                v.apply(msg, cx);
            });
        });

        let model = Rc::clone(&self.model);
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered_at(e.position, window) {
                return;
            }
            let (x, y) = local(e.position);
            let msg = match e.button {
                MouseButton::Left => logic::option_at(&model, picker, x, y)
                    .map(|(node, i)| Msg::PickChoice(node, i))
                    .or_else(|| logic::node_at(&model, x, y).map(|n| Msg::NodeClick(n.id)))
                    .or_else(|| picker.map(|_| Msg::ClosePicker)),
                MouseButton::Right => {
                    logic::node_at(&model, x, y).map(|n| Msg::NodeRightClick(n.id))
                }
                _ => None,
            };
            if let Some(msg) = msg {
                cx.stop_propagation();
                let _ = viewer.update(cx, |v, cx| v.apply(msg, cx));
            }
        });
    }
}
