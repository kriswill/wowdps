//! The instance timeline: grouping of the segment list into *blocks* (one per
//! instance visit, one per stray segment outside instances) and the compact
//! Σ–①─②─③–⚑ strip the overlay draws for the block being watched.
//!
//! The model half is pure functions over the client's id table
//! (`ClientState::entries()`), so navigation and rendering agree on
//! positions by construction; the rendering half emits no messages of its
//! own — callers supply a `position → message` constructor.
//!
//! [`strip`] draws in the overlay's colours; [`strip_in`] takes a surface's
//! [`Look`] and now draws for the overlay alone (tests aside) — the window
//! draws no strip, its fight header and pull rail replaced it. The window
//! still uses the model half: the rail groups tonight's pulls by
//! [`blocks`].

//! The model half (blocks, items, scrubbing, the strip's width arithmetic)
//! lives in gui-logic's `timeline`, shared with gui-new, and is
//! re-exported here.

use iced::widget::{Space, container, mouse_area, row, stack, text};
use iced::{Color, Element, Length, Theme};

use crate::theme::Look;

pub(crate) use wowdps_gui_logic::timeline::*;

/// A clickable strip element: the visual wrapped in a padded, z-scaled hit
/// box, so the target is meaningfully larger than the ~15px glyph it shows.
fn hit<M: Clone + 'static>(visual: Element<'static, M>, msg: M, z: f32) -> Element<'static, M> {
    mouse_area(container(visual).padding([HIT_PAD_Y * z, HIT_PAD_X * z]))
        .on_press(msg)
        .into()
}

/// One strip element rendered as its clickable visual.
fn item_el<M: Clone + 'static>(
    look: &Look,
    item: &Item,
    selected: Option<usize>,
    z: f32,
    goto: &impl Fn(usize) -> M,
) -> Element<'static, M> {
    match *item {
        Item::Overall { pos, live } => hit(
            disc(
                look,
                "Σ".to_string(),
                Color {
                    a: 0.20,
                    ..look.sum
                },
                look.sum,
                selected == Some(pos),
                live,
                None,
                z,
            ),
            goto(pos),
            z,
        ),
        Item::Boss {
            pos,
            num,
            success,
            live,
        } => {
            let fill = match (live, success) {
                // The window says "live" with a dot, never a hue a kill or
                // a wipe could be mistaken for: the disc stays neutral.
                (true, _) if look.live_dot => Color {
                    a: 0.20,
                    ..look.dim
                },
                (true, _) => Color {
                    a: 0.45,
                    ..look.live
                },
                (_, Some(true)) => Color {
                    a: 0.40,
                    ..look.good
                },
                // The window's wipe is a hollow ring in its red: an outcome
                // by SHAPE as well as hue, so a kill and a wipe never rest
                // on green against red alone.
                (_, Some(false)) if look.wipe_ring => Color::TRANSPARENT,
                (_, Some(false)) => Color {
                    a: 0.40,
                    ..look.bad
                },
                (_, None) => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            };
            let hollow = (look.wipe_ring && !live && success == Some(false)).then_some(look.bad);
            hit(
                disc(
                    look,
                    num.to_string(),
                    fill,
                    look.ink,
                    selected == Some(pos),
                    live,
                    hollow,
                    z,
                ),
                goto(pos),
                z,
            )
        }
        Item::Gap {
            pos,
            duration_ms,
            live,
            ..
        } => hit(
            gap_line(look, duration_ms, selected == Some(pos), live, z),
            goto(pos),
            z,
        ),
        Item::Wipes { pos, count } => hit(pill(look, count, z), goto(pos), z),
        Item::Flag { success } => Element::from(text("⚑").size(11.0 * z).color(if success {
            look.good
        } else {
            look.bad
        })),
    }
}

/// Render the strip. `selected` is the watched entries position; `goto`
/// turns a clicked element's position into the frontend's message.
///
/// When the badges outgrow `budget` they fan into an overlapping stack —
/// later elements on top, the watched one raised above all so it reads whole
/// — instead of hiding behind a scrollbar; every element keeps a clickable
/// sliver (iced's `stack` hands events to the top layer first), and the
/// caller's wheel gesture scrubs through what the fan compresses.
pub fn strip<M: Clone + 'static>(
    items: &[Item],
    selected: Option<usize>,
    z: f32,
    budget: f32,
    goto: impl Fn(usize) -> M,
) -> Element<'static, M> {
    strip_in(&Look::OVERLAY, items, selected, z, budget, goto)
}

/// [`strip`] in a surface's own [`Look`]: the window's Σ is secondary ink
/// and its live pull red, where the overlay's are yellow.
pub(crate) fn strip_in<M: Clone + 'static>(
    look: &Look,
    items: &[Item],
    selected: Option<usize>,
    z: f32,
    budget: f32,
    goto: impl Fn(usize) -> M,
) -> Element<'static, M> {
    let watched = |item: &Item| item_pos(item).is_some() && item_pos(item) == selected;
    let widths: Vec<f32> = items
        .iter()
        .map(|i| natural_width(i, z, watched(i)))
        .collect();
    // The fan tightens away from the watched element; without one (or with
    // it off-strip) the frontier — the newest pull — is what matters.
    let focus = items
        .iter()
        .position(watched)
        .unwrap_or(items.len().saturating_sub(1));
    let pin_first = matches!(items.first(), Some(Item::Overall { .. }));
    let Some(xs) = cascade_xs(&widths, 1.5 * z, budget, focus, pin_first, z) else {
        let mut line = row![].spacing(1.5 * z).align_y(iced::Alignment::Center);
        for item in items {
            line = line.push(item_el(look, item, selected, z, &goto));
        }
        // Same FIXED height as the fan below: sized for the emphasized
        // watched disc whether or not one is on the strip, so watching Σ
        // (or nothing) never shifts everything under the strip.
        return container(line)
            .align_y(iced::Alignment::Center)
            .height(Length::Fixed((DISC * EMPHASIS + 2.0 * HIT_PAD_Y) * z))
            .into();
    };

    let mut layers: Vec<(f32, Element<'static, M>)> = items
        .iter()
        .zip(xs)
        .map(|(item, x)| (x, item_el(look, item, selected, z, &goto)))
        .collect();
    // Raise the watched element to the top of the fan so it shows whole.
    if let Some(at) = items.iter().position(watched) {
        let raised = layers.remove(at);
        layers.push(raised);
    }
    let mut fan = stack![]
        .width(Length::Fill)
        .height(Length::Fixed((DISC * EMPHASIS + 2.0 * HIT_PAD_Y) * z));
    for (x, el) in layers {
        fan = fan.push(
            container(el)
                .align_y(iced::Alignment::Center)
                .height(Length::Fill)
                .padding(iced::Padding {
                    left: x,
                    ..iced::Padding::ZERO
                }),
        );
    }
    fan.into()
}

/// A circular marker: number or Σ, colored fill, selection ring. The watched
/// disc is drawn a step larger — emphasis the ring alone loses in a fan.
/// `hollow` is a wipe's ring colour where the look draws outcomes by shape
/// ([`Look::wipe_ring`]): the ring carries the outcome, a step heavier,
/// around a clear middle.
#[allow(clippy::too_many_arguments)]
fn disc<M: 'static>(
    look: &Look,
    label: String,
    fill: Color,
    txt: Color,
    selected: bool,
    live: bool,
    hollow: Option<Color>,
    z: f32,
) -> Element<'static, M> {
    let emph = if selected { EMPHASIS } else { 1.0 };
    let dia = DISC * z * emph;
    let ring = if selected {
        look.ink
    } else if let Some(outcome) = hollow {
        outcome
    } else if live && !look.live_dot {
        look.live
    } else {
        look.ring
    };
    let ring_w = if selected || hollow.is_some() {
        1.5
    } else {
        1.0
    };
    let body = container(text(label).size(8.5 * z * emph).color(txt))
        .center(Length::Fixed(dia))
        .style(move |_: &Theme| container::Style {
            background: Some(fill.into()),
            border: iced::Border {
                color: ring,
                width: ring_w,
                radius: (dia / 2.0).into(),
            },
            ..container::Style::default()
        });
    if !(live && look.live_dot) {
        return body.into();
    }
    // The live pull's mark: the header's red dot (`.pulse`), on the disc's
    // shoulder — a shape of its own, so live and a wipe never share one.
    let d = (dia * 0.4).round().max(4.0);
    let live_color = look.live;
    let dot = container(Space::new())
        .width(Length::Fixed(d))
        .height(Length::Fixed(d))
        .style(move |_: &Theme| container::Style {
            background: Some(live_color.into()),
            border: iced::border::rounded(d / 2.0),
            ..container::Style::default()
        });
    stack![
        body,
        container(dot)
            .width(Length::Fixed(dia))
            .height(Length::Fixed(dia))
            .align_x(iced::Alignment::End)
            .align_y(iced::Alignment::Start),
    ]
    .into()
}

/// A collapsed wipe run: a disc-height pill reading `×N`, wipe-red like the
/// attempts it stands for but flatter, so it reads as "N of those" rather
/// than one more pull. Clicking lands on the run's most recent wipe.
fn pill<M: 'static>(look: &Look, count: usize, z: f32) -> Element<'static, M> {
    let dia = DISC * z;
    let bad = look.bad;
    container(text(format!("×{count}")).size(8.5 * z).color(look.ink))
        .center_y(Length::Fixed(dia))
        .padding([0.0, 4.0 * z])
        .style(move |_: &Theme| container::Style {
            background: Some(Color { a: 0.22, ..bad }.into()),
            border: iced::Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
                width: 1.0,
                radius: (dia / 2.0).into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The trash connector: a thin line whose length hints at time spent, inside
/// a disc-height hit area so it is clickable mid-fight.
fn gap_line<M: 'static>(
    look: &Look,
    duration_ms: i64,
    selected: bool,
    live: bool,
    z: f32,
) -> Element<'static, M> {
    let w = gap_width(duration_ms, z);
    let color = if live && look.live_dot {
        // Live trash in the window: a connector in secondary ink — the
        // live dot on the disc says the rest.
        look.dim
    } else if live {
        look.live
    } else if selected {
        Color::from_rgba(1.0, 1.0, 1.0, 0.9)
    } else {
        look.faint
    };
    let bar = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(w))
        .height(Length::Fixed(if selected { 3.0 * z } else { 2.0 * z }))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: iced::border::rounded(1),
            ..container::Style::default()
        });
    container(bar)
        .center_y(Length::Fixed(DISC * z))
        .width(Length::Fixed(w))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every strip element variant, in one list: Σ, a live boss, a kill, a
    /// wipe, an unresolved boss, a trash gap, a collapsed run, both flags.
    fn every_item() -> Vec<Item> {
        vec![
            Item::Overall { pos: 0, live: true },
            Item::Gap {
                pos: 1,
                pulls: 3,
                duration_ms: 45_000,
                live: true,
            },
            Item::Boss {
                pos: 2,
                num: 1,
                success: Some(true),
                live: false,
            },
            Item::Boss {
                pos: 3,
                num: 2,
                success: Some(false),
                live: false,
            },
            Item::Wipes { pos: 4, count: 5 },
            Item::Boss {
                pos: 5,
                num: 3,
                success: None,
                live: false,
            },
            Item::Gap {
                pos: 6,
                pulls: 1,
                duration_ms: 0,
                live: false,
            },
            Item::Boss {
                pos: 7,
                num: 4,
                success: None,
                live: true,
            },
            Item::Flag { success: false },
            Item::Flag { success: true },
        ]
    }

    #[test]
    fn the_strip_renders_inline_when_it_fits_and_fans_when_it_does_not() {
        let items = every_item();
        // Every element builds as its own visual, watched or not, at both
        // zooms — the message is the clicked position, straight through.
        for item in &items {
            for (sel, z) in [(None, 1.0), (item_pos(item), 1.0), (Some(99), 2.0)] {
                for look in [&Look::OVERLAY, &Look::WINDOW] {
                    let _: Element<'static, usize> = item_el(look, item, sel, z, &|p| p);
                }
            }
        }
        // Widths sum below the budget: the inline row.
        let natural: f32 = items
            .iter()
            .map(|i| natural_width(i, 1.0, false))
            .sum::<f32>()
            + 1.5 * (items.len() - 1) as f32;
        let _: Element<'static, usize> = strip(&items, Some(3), 1.0, natural + 50.0, |p| p);
        let _: Element<'static, usize> = strip(&items, None, 1.0, natural + 50.0, |p| p);
        // Squeezed to a third: the fan, with the watched element raised —
        // wherever it sits, including off the strip entirely.
        for sel in [None, Some(0), Some(3), Some(7), Some(99)] {
            let _: Element<'static, usize> = strip(&items, sel, 1.0, natural / 3.0, |p| p);
        }
        let _: Element<'static, usize> = strip(&items, Some(2), 1.5, natural, |p| p);
        // Degenerate strips.
        let _: Element<'static, usize> = strip(&[], None, 1.0, 100.0, |p| p);
        let _: Element<'static, usize> = strip(&items[..1], Some(0), 1.0, 1.0, |p| p);
        // The window's own look: the same strip, its Σ and live not yellow.
        let _: Element<'static, usize> =
            strip_in(&Look::WINDOW, &items, Some(3), 1.0, natural / 3.0, |p| p);
    }

    #[test]
    fn the_strip_geometry_matches_the_fan_math() {
        // The strip and cascade_xs must agree on widths: a fit at the
        // natural sum, a fan one pixel under it.
        let items = every_item();
        let widths: Vec<f32> = items.iter().map(|i| natural_width(i, 1.0, false)).collect();
        let natural: f32 = widths.iter().sum::<f32>() + 1.5 * (widths.len() - 1) as f32;
        assert!(cascade_xs(&widths, 1.5, natural, 0, true, 1.0).is_none());
        let xs = cascade_xs(&widths, 1.5, natural - 1.0, 0, true, 1.0).expect("fans");
        assert_eq!(xs.len(), items.len());
        // With Σ pinned, its boundary keeps the full natural step.
        assert!((xs[1] - xs[0] - (widths[0] + 1.5)).abs() < 1e-3, "{xs:?}");
        // The watched disc is wider than an unwatched one, so a strip that
        // just fits unwatched fans once something on it is watched.
        let emph: Vec<f32> = items
            .iter()
            .map(|i| natural_width(i, 1.0, item_pos(i) == Some(3)))
            .collect();
        assert!(emph.iter().sum::<f32>() > widths.iter().sum::<f32>());
        assert!(cascade_xs(&emph, 1.5, natural, 3, true, 1.0).is_some());
    }

    #[test]
    fn discs_pills_and_gap_lines_build_in_every_state() {
        let o = &Look::OVERLAY;
        for (selected, live) in [(false, false), (true, false), (false, true), (true, true)] {
            let _: Element<'static, usize> =
                disc(o, "1".to_string(), o.bad, o.ink, selected, live, None, 1.0);
            let w = &Look::WINDOW;
            let _: Element<'static, usize> = disc(
                w,
                "2".to_string(),
                Color::TRANSPARENT,
                w.ink,
                selected,
                live,
                Some(w.bad),
                1.0,
            );
            let _: Element<'static, usize> = gap_line(o, 30_000, selected, live, 1.0);
            let _: Element<'static, usize> = gap_line(&Look::WINDOW, 0, selected, live, 2.0);
        }
        let _: Element<'static, usize> = pill(o, 3, 1.0);
        let _: Element<'static, usize> = pill(&Look::WINDOW, 120, 2.0);
        let _: Element<'static, usize> = hit(pill(o, 3, 1.0), 7usize, 1.0);
    }
}
