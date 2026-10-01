//! The overlay's rows, as the iced overlay draws them (`view::overlay_row`,
//! `under_bar`, `class_bar`, `rank_cell`, `compare::class_icon`): a 20z
//! line of text over a 3z bar, the bar a left-to-right ramp of the row's
//! colour on a faint track, figures in fixed monospace columns that never
//! move.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, Hsla, PathBuilder, Pixels, div, img, px};
use wowdps_gui_logic::labels::{class_tag, display_name};
use wowdps_gui_logic::theme::Color;
use wowdps_model::fmt::human;
use wowdps_model::{Class, Row, Spec};

use super::ov::{Ov, bar_ramp, circle, share};
use crate::images;
use crate::theme::hsla;

/// A bar's colour: the spell school's on an ability row, hostile red for a
/// classless enemy, the class's own, else the classless grey.
pub fn bar_color(ov: &Ov, r: &Row) -> Hsla {
    if r.school != 0
        && let Some(c) = wowdps_gui_logic::theme::school_color(r.school)
    {
        return hsla(c);
    }
    if r.enemy && r.class.is_none() {
        return ov.c(|t| t.hostile);
    }
    match r.class {
        Some(class) => hsla(Color::of_class(class)),
        None => ov.c(|t| t.classless),
    }
}

/// A row's share of the longest, in whole percent as the iced overlay
/// rounds it, as a fraction.
pub fn fill(amount: u64, max: u64) -> f32 {
    let pct = (amount as f64 / max.max(1) as f64 * 100.0)
        .round()
        .clamp(0.0, 100.0);
    pct as f32 / 100.0
}

/// `content` over a bar: the text band takes what is left of `height`
/// after a 1z gap and the 3z bar on a faint track.
pub fn under_bar(ov: &Ov, color: Hsla, frac: f32, content: Div, height: Pixels) -> Div {
    let bar = div()
        .w(share(frac))
        .h_full()
        .rounded(px(2.))
        .when(frac > 0.0, |b| b.bg(bar_ramp(color)));
    div()
        .w_full()
        .h(height)
        .flex()
        .flex_col()
        .gap(ov.z(1.))
        .overflow_hidden()
        .rounded(px(3.))
        .child(content.flex_1().min_h_0().w_full())
        .child(
            div()
                .w_full()
                .h(ov.z(3.))
                .flex_none()
                .rounded(px(2.))
                .bg(ov.c(|t| t.track))
                .child(bar),
        )
}

/// A rank, right-aligned in a 14z cell.
pub fn rank_cell(ov: &Ov, rank: usize) -> Div {
    ov.metric(rank.to_string(), 10., ov.c(|t| t.ink), 14.)
}

/// A meter row (`view::overlay_row`): the name, then amount, rate and share
/// in their columns, over the row's bar. `rank` rides inside the row (the
/// Σ split's rows; the meter's own sits outside, left of the icon). `frac`
/// is the bar's length, which the caller may animate toward `fill`.
pub fn meter_row(ov: &Ov, r: &Row, rank: Option<usize>, frac: f32) -> Div {
    let rate = if r.per_sec >= 1.0 {
        human(r.per_sec as u64)
    } else {
        String::new()
    };
    let line = div()
        .flex()
        .items_center()
        .gap(ov.z(8.))
        .px(px(8.))
        .children(rank.map(|n| rank_cell(ov, n)))
        .child(div().flex_1().min_w_0().overflow_hidden().child(ov.words(
            display_name(&r.label).to_string(),
            13.,
            ov.c(|t| t.text),
        )))
        .child(ov.metric(human(r.amount), 12., ov.c(|t| t.ink), 52.))
        .child(ov.metric(rate, 12., ov.c(|t| t.rate), 50.))
        .child(ov.metric(format!("{:.1}%", r.pct), 11., ov.c(|t| t.dim), 44.));
    under_bar(ov, bar_color(ov, r), frac, line, ov.z(20.))
}

/// A player's badge `d` across: the spec icon, else the class crest, at
/// 80% (whole once picked for a comparison, and ringed white); without
/// art, a disc in the class colour with its two-letter tag.
pub fn class_icon(
    ov: &Ov,
    class: Option<Class>,
    spec: Option<Spec>,
    picked: bool,
    d: Pixels,
) -> AnyElement {
    let art = spec
        .and_then(|s| images::spec_icon(s.id()))
        .or_else(|| class.and_then(images::class_icon));
    let ring = picked.then(|| ring(d, hsla(Color::WHITE)));
    let base = match art {
        Some(tile) => img(tile)
            .size(d)
            .opacity(if picked { 1.0 } else { 0.8 })
            .into_any_element(),
        None => {
            let fill = class.map_or(ov.t.classless, Color::of_class);
            disc(
                ov,
                fill.alpha(if picked { 1.0 } else { 0.55 }),
                class_tag(class),
                d,
            )
        }
    };
    div()
        .relative()
        .size(d)
        .flex_none()
        .child(base)
        .children(ring.map(|r| div().absolute().inset_0().child(r)))
        .into_any_element()
}

/// An enemy's badge: the hostile disc with a skull.
pub fn enemy_icon(ov: &Ov, d: Pixels) -> AnyElement {
    disc(ov, ov.t.hostile.alpha(0.55), "☠", d)
}

/// A disc `d` across in `fill`, with `tag` centred in dark ink.
fn disc(ov: &Ov, fill: Color, tag: &'static str, d: Pixels) -> AnyElement {
    let r = d / 2. - px(1.);
    div()
        .size(d)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .child(
            div()
                .size(r * 2.)
                .rounded_full()
                .bg(hsla(fill))
                .flex()
                .items_center()
                .justify_center()
                .font_family(ov.mono)
                .text_size(r * 0.9)
                .text_color(hsla(Color::rgba(0.0, 0.0, 0.0, 0.85)))
                .child(tag),
        )
        .into_any_element()
}

/// A 2 px ring `d` across.
fn ring(d: Pixels, color: Hsla) -> impl IntoElement {
    gpui_kit::canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let c = b.center();
            let mut path = PathBuilder::stroke(px(2.));
            circle(&mut path, c.x, c.y, b.size.width / 2. - px(1.));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(d)
}

/// R13: the line between the teams in a PvP chart.
pub fn team_divider(ov: &Ov) -> Div {
    let line = || div().flex_1().h(px(1.)).bg(ov.c(|t| t.bad.alpha(0.4)));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(line())
        .child(ov.words("enemy team", 9., ov.c(|t| t.bad)))
        .child(line())
}

#[cfg(test)]
mod tests {
    use super::fill;

    /// The iced overlay rounds a bar to whole percent of the longest.
    #[test]
    fn a_bar_is_its_share_of_the_longest_in_whole_percent() {
        assert_eq!(fill(50, 100), 0.5);
        assert_eq!(fill(1, 3), 0.33);
        assert_eq!(fill(0, 100), 0.0);
        assert_eq!(fill(100, 0), 1.0, "a zero max is one");
    }
}
