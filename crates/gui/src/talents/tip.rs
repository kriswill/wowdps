//! What lies under and over the trees: the spec's background painting,
//! cover-fit and clipped to the tree area, and the game-style tooltip
//! beside the hovered icon — its lines gui-logic's (`tooltip_lines`), so
//! both GUIs wrap and colour a talent's words alike.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, Corners, Hsla, Pixels, TestSupportExt as _, canvas, div, point, px, size,
};
use wowdps_gui_logic::talents::{self as logic, Node, Tone};

use super::Paint;
use crate::images::Tile;

/// The painting, scaled to cover the area and centred, the overflow
/// clipped by the area's own bounds.
pub(crate) fn backdrop(painting: Tile, w: u16, h: u16) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let (iw, ih) = (f32::from(w).max(1.0), f32::from(h).max(1.0));
            let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let scale = (bw / iw).max(bh / ih);
            let (dw, dh) = (iw * scale, ih * scale);
            let at = Bounds::new(
                point(
                    bounds.origin.x + px((bw - dw) / 2.0),
                    bounds.origin.y + px((bh - dh) / 2.0),
                ),
                size(px(dw), px(dh)),
            );
            let _ = window.paint_image(bounds, at, Corners::default(), painting.clone(), 0, false);
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

/// A line's colour: the game's for its role.
pub(crate) fn tone(p: &Paint, tone: Tone) -> Hsla {
    p.c(|t| match tone {
        Tone::Plain => t.tip_ink,
        Tone::Meta => t.tip_meta,
        Tone::Desc => t.tip_desc,
        Tone::Note => t.tip_note,
        Tone::Unreached => t.tip_unreached,
    })
}

/// The tooltip for `node`, its tile centred at `anchor` (window pixels),
/// inside `area` (the tree area's window bounds). `arrive` (0..1) fades it
/// in from a few pixels below — 1 is the iced viewer's box exactly.
pub(crate) fn tooltip(
    p: &Paint,
    node: &Node,
    requires: &str,
    anchor: (f32, f32),
    area: Bounds<Pixels>,
    arrive: f32,
) -> AnyElement {
    let (w, h) = (f32::from(area.size.width), f32::from(area.size.height));
    let tw = logic::tip_width(w);
    let lines = logic::tooltip_lines(node, requires, logic::tip_budget(tw));
    let th = logic::tip_height(&lines);
    let cur = (
        anchor.0 - f32::from(area.origin.x),
        anchor.1 - f32::from(area.origin.y),
    );
    let (x, y) = logic::tip_origin(cur, tw, th, w, h);
    let rows = lines.into_iter().map(|line| {
        let color = tone(p, line.tone);
        let row = div()
            .h(px(line.size + 4.0))
            .flex()
            .items_center()
            .text_size(px(line.size))
            .text_color(color)
            .whitespace_nowrap()
            .child(div().flex_1().child(line.text));
        match line.right {
            Some(right) => row.child(right),
            None => row,
        }
    });
    let tip = div()
        .id("talent-tip")
        .test_support()
        .absolute()
        .left(px(x))
        .top(px(y + 4.0 * (1.0 - arrive)))
        .w(px(tw))
        .h(px(th))
        .opacity(arrive)
        .pt(px(7.0))
        .px(px(logic::TIP_PAD_X))
        .rounded(p.r(4.0))
        .font_family(crate::theme::face(&p.def.faces.ui))
        .flex()
        .flex_col()
        .children(rows);
    p.tip_face(tip).into_any_element()
}
