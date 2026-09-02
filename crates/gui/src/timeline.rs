//! The instance timeline: grouping of the segment list into *blocks* (one per
//! instance visit, one per stray segment outside instances) and the compact
//! Σ–①─②─③–⚑ strip the overlay draws for the block being watched.
//!
//! The model half is pure functions over the client's id table
//! (`ClientState::entries()`), so navigation and rendering agree on
//! positions by construction; the rendering half emits no messages of its
//! own — callers supply a `position → message` constructor.

use iced::widget::{Space, container, mouse_area, row, stack, text};
use iced::{Color, Element, Length, Theme};

use wowdps_model::SegmentKind;
use wowdps_proto::ListEntry;

use crate::view::{DIM, GREEN, RED, YELLOW};

/// One navigable unit of the segment list: an instance visit (its Σ overall
/// row plus every member segment, contiguous or not — zoning out mid-key
/// splits a visit's members around city combat), or a single segment outside
/// any instance. All positions index the client's entries table.
#[derive(Debug, PartialEq, Eq)]
pub struct Block {
    /// Instance visit ordinal; `None` for a stray out-of-instance segment.
    pub ordinal: Option<u32>,
    /// Position of the visit's Σ overall row.
    pub overall: Option<usize>,
    /// Member segment positions, oldest first (never the overall).
    pub members: Vec<usize>,
}

impl Block {
    /// The position navigation lands on when this block is stepped to:
    /// the Σ summary when the visit has one, else the segment itself.
    pub fn anchor(&self) -> Option<usize> {
        self.overall.or_else(|| self.members.first().copied())
    }

    pub fn contains(&self, pos: usize) -> bool {
        self.overall == Some(pos) || self.members.contains(&pos)
    }

    /// A visit with a Σ row is worth the instance frame; a stray segment is
    /// rendered as a plain fight.
    pub fn is_instance(&self) -> bool {
        self.ordinal.is_some() && self.overall.is_some()
    }
}

/// Group the segment list into blocks, oldest first (order of first
/// appearance — a visit interrupted by city combat keeps its one block).
pub fn blocks(entries: &[ListEntry]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        match e.row.instance {
            Some(ord) => {
                let at = match out.iter().rposition(|b| b.ordinal == Some(ord)) {
                    Some(at) => at,
                    None => {
                        out.push(Block {
                            ordinal: Some(ord),
                            overall: None,
                            members: Vec::new(),
                        });
                        out.len() - 1
                    }
                };
                let Some(block) = out.get_mut(at) else {
                    continue;
                };
                if e.row.kind == SegmentKind::Overall {
                    block.overall = Some(i);
                } else {
                    block.members.push(i);
                }
            }
            None => out.push(Block {
                ordinal: None,
                overall: None,
                members: vec![i],
            }),
        }
    }
    out
}

/// Index of the block containing an entries position.
pub fn block_of(blocks: &[Block], pos: usize) -> Option<usize> {
    blocks.iter().position(|b| b.contains(pos))
}

/// Any part still accumulating: the visit is in progress (or the stray
/// segment is the open one).
pub fn is_live(block: &Block, entries: &[ListEntry]) -> bool {
    block
        .overall
        .into_iter()
        .chain(block.members.iter().copied())
        .any(|p| entries.get(p).is_some_and(|e| e.row.live))
}

/// One element of the rendered strip, left to right.
#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    /// The Σ marker: the visit's overall.
    Overall { pos: usize, live: bool },
    /// A numbered boss marker.
    Boss {
        pos: usize,
        num: u32,
        success: Option<bool>,
        live: bool,
    },
    /// The line between bosses: a run of consecutive trash segments,
    /// coalesced. Clicking selects the run's first segment.
    Gap {
        pos: usize,
        pulls: usize,
        duration_ms: i64,
        live: bool,
    },
    /// The end-of-key flag, present once the visit's outcome is known.
    Flag { success: bool },
    /// A collapsed run of consecutive wipes on one boss: progression nights
    /// put a dozen attempts on the strip and drown everything else. Clicking
    /// lands on the run's most recent wipe; the ‹ › scrubbers still step
    /// through every hidden attempt (scrubbing walks members, not items),
    /// and the stepped-into attempt surfaces out of the run.
    Wipes {
        /// The most recent wipe in the run — the click target.
        pos: usize,
        /// Attempts hidden behind the chip.
        count: usize,
    },
}

/// Build the strip for one block. Callers pass the same entries the block
/// was computed from.
pub fn items(block: &Block, entries: &[ListEntry]) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(o) = block.overall {
        out.push(Item::Overall {
            pos: o,
            live: entries.get(o).is_some_and(|e| e.row.live),
        });
    }
    let mut num = 0;
    let mut gap: Option<Item> = None;
    for &m in &block.members {
        let Some(r) = entries.get(m).map(|e| &e.row) else {
            continue;
        };
        if r.kind == SegmentKind::Encounter {
            out.extend(gap.take());
            num += 1;
            out.push(Item::Boss {
                pos: m,
                num,
                success: r.success,
                live: r.live,
            });
        } else {
            match gap.as_mut() {
                Some(Item::Gap {
                    pulls,
                    duration_ms,
                    live,
                    ..
                }) => {
                    *pulls += 1;
                    *duration_ms += r.duration_ms;
                    *live |= r.live;
                }
                _ => {
                    gap = Some(Item::Gap {
                        pos: m,
                        pulls: 1,
                        duration_ms: r.duration_ms,
                        live: r.live,
                    });
                }
            }
        }
    }
    out.extend(gap.take());
    if let Some(success) = block
        .overall
        .and_then(|o| entries.get(o))
        .and_then(|e| e.row.success)
    {
        out.push(Item::Flag { success });
    }
    out
}

/// A run must hide at least this many wipes to earn a chip: collapsing two
/// saves no meaningful width and costs the at-a-glance pull count.
const MIN_COLLAPSE: usize = 3;

/// Collapse runs of consecutive dead wipes on the same boss into one
/// [`Item::Wipes`] chip. Never collapsed: the watched attempt (scrubbing
/// into a run surfaces it), a live pull, and the strip's newest boss (the
/// frontier — "what was the last pull" must stay visible). Trash gaps
/// between a run's wipes vanish into the chip; the ‹ › scrubbers still
/// reach them.
pub fn collapse(items: Vec<Item>, entries: &[ListEntry], selected: Option<usize>) -> Vec<Item> {
    let last_boss = items.iter().rposition(|i| matches!(i, Item::Boss { .. }));
    let mut out: Vec<Item> = Vec::new();
    // Collapsible bosses seen so far, with the gaps between them.
    let mut run: Vec<Item> = Vec::new();
    let mut run_name: Option<String> = None;
    let mut run_bosses = 0usize;
    // Gaps after the run's last boss — theirs only if another wipe follows.
    let mut pending: Vec<Item> = Vec::new();

    fn flush(
        out: &mut Vec<Item>,
        run: &mut Vec<Item>,
        bosses: &mut usize,
        pending: &mut Vec<Item>,
    ) {
        if *bosses >= MIN_COLLAPSE {
            let pos = run
                .iter()
                .rev()
                .find_map(|i| match i {
                    Item::Boss { pos, .. } => Some(*pos),
                    _ => None,
                })
                .unwrap_or(0);
            out.push(Item::Wipes {
                pos,
                count: *bosses,
            });
        } else {
            out.append(run);
        }
        run.clear();
        *bosses = 0;
        out.append(pending);
    }

    for (idx, item) in items.into_iter().enumerate() {
        match item {
            Item::Boss {
                pos, success, live, ..
            } => {
                let name = entries.get(pos).map(|e| e.row.name.as_str());
                let collapsible = success == Some(false)
                    && !live
                    && selected != Some(pos)
                    && last_boss != Some(idx)
                    && name.is_some();
                if collapsible {
                    // A wipe on a different boss ends the run — and starts
                    // its own, not a plain disc.
                    if run_bosses > 0 && run_name.as_deref() != name {
                        flush(&mut out, &mut run, &mut run_bosses, &mut pending);
                    }
                    if run_bosses == 0 {
                        run_name = name.map(str::to_string);
                    }
                    run.append(&mut pending);
                    run.push(item);
                    run_bosses += 1;
                } else {
                    flush(&mut out, &mut run, &mut run_bosses, &mut pending);
                    out.push(item);
                }
            }
            Item::Gap { .. } if run_bosses > 0 => pending.push(item),
            _ => {
                flush(&mut out, &mut run, &mut run_bosses, &mut pending);
                out.push(item);
            }
        }
    }
    flush(&mut out, &mut run, &mut run_bosses, &mut pending);
    out
}

/// Step the within-block scrub order (Σ first, then members oldest→newest)
/// from the currently watched position. `None` when there is nowhere to go.
pub fn scrub(block: &Block, current: usize, delta: isize) -> Option<usize> {
    let order: Vec<usize> = block
        .overall
        .into_iter()
        .chain(block.members.iter().copied())
        .collect();
    let at = order.iter().position(|&p| p == current)?;
    let next = at.checked_add_signed(delta)?;
    (next != at).then(|| order.get(next).copied()).flatten()
}

// ---- rendering --------------------------------------------------------------

/// Strip geometry at zoom 1.0.
const DISC: f32 = 16.0;
const GAP_MIN: f32 = 9.0;
const GAP_MAX: f32 = 30.0;

/// The trash connector's length: hints at time spent, bounded.
fn gap_width(duration_ms: i64, z: f32) -> f32 {
    let secs = (duration_ms.max(0) / 1000) as f32;
    (GAP_MIN + secs.sqrt() * 0.9).clamp(GAP_MIN, GAP_MAX) * z
}
/// Hit-box slack around every clickable strip element: the drawn shapes
/// stay small, but a mid-fight click has this much extra to land in.
const HIT_PAD_Y: f32 = 4.0;
const HIT_PAD_X: f32 = 1.5;

/// A clickable strip element: the visual wrapped in a padded, z-scaled hit
/// box, so the target is meaningfully larger than the ~15px glyph it shows.
fn hit<M: Clone + 'static>(visual: Element<'static, M>, msg: M, z: f32) -> Element<'static, M> {
    mouse_area(container(visual).padding([HIT_PAD_Y * z, HIT_PAD_X * z]))
        .on_press(msg)
        .into()
}

/// One strip element rendered as its clickable visual.
fn item_el<M: Clone + 'static>(
    item: &Item,
    selected: Option<usize>,
    z: f32,
    goto: &impl Fn(usize) -> M,
) -> Element<'static, M> {
    match *item {
        Item::Overall { pos, live } => hit(
            disc(
                "Σ".to_string(),
                Color { a: 0.20, ..YELLOW },
                YELLOW,
                selected == Some(pos),
                live,
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
                (true, _) => Color { a: 0.45, ..YELLOW },
                (_, Some(true)) => Color { a: 0.40, ..GREEN },
                (_, Some(false)) => Color { a: 0.40, ..RED },
                (_, None) => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            };
            hit(
                disc(
                    num.to_string(),
                    fill,
                    Color::WHITE,
                    selected == Some(pos),
                    live,
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
            gap_line(duration_ms, selected == Some(pos), live, z),
            goto(pos),
            z,
        ),
        Item::Wipes { pos, count } => hit(pill(count, z), goto(pos), z),
        Item::Flag { success } => {
            Element::from(
                text("⚑")
                    .size(11.0 * z)
                    .color(if success { GREEN } else { RED }),
            )
        }
    }
}

/// The entries position a strip element stands for; the flag stands for none.
fn item_pos(item: &Item) -> Option<usize> {
    match *item {
        Item::Overall { pos, .. }
        | Item::Boss { pos, .. }
        | Item::Gap { pos, .. }
        | Item::Wipes { pos, .. } => Some(pos),
        Item::Flag { .. } => None,
    }
}

/// The watched disc grows by this much: emphasis, not a new size class.
const EMPHASIS: f32 = 1.25;
/// Tightest exposed sliver of a fanned-under element, at zoom 1.0.
const SLIVER: f32 = 4.0;

/// Natural (uncompressed) width of a strip element at this zoom, hit-box
/// slack included. Text widths are close estimates — layout only needs them
/// to decide when to fan and how tightly. `emph` is the watched element's
/// enlarged disc.
fn natural_width(item: &Item, z: f32, emph: bool) -> f32 {
    let slack = 2.0 * HIT_PAD_X * z;
    match item {
        Item::Overall { .. } | Item::Boss { .. } => {
            DISC * z * if emph { EMPHASIS } else { 1.0 } + slack
        }
        Item::Gap { duration_ms, .. } => gap_width(*duration_ms, z) + slack,
        Item::Wipes { count, .. } => {
            let glyphs = 1 + count.to_string().len();
            (8.0 + glyphs as f32 * 4.7) * z + slack
        }
        Item::Flag { .. } => 10.0 * z,
    }
}

/// Left x of every item, inline first. `None` means the natural layout fits.
///
/// When it overflows, the fan compresses each boundary by how far it sits
/// from `focus` (the watched element): neighbors keep near-natural air and
/// the falloff is parabolic — twice as far, a quarter of the slack — so the
/// pile tightens gradually toward the edges, down to a [`SLIVER`]. With
/// `pin_first`, the first element (the visit's Σ) keeps its full width
/// exposed no matter how deep the pile gets.
fn cascade_xs(
    widths: &[f32],
    spacing: f32,
    budget: f32,
    focus: usize,
    pin_first: bool,
    z: f32,
) -> Option<Vec<f32>> {
    let n = widths.len();
    let (&last_w, body) = widths.split_last()?;
    let natural: f32 = widths.iter().sum::<f32>() + spacing * n.saturating_sub(1) as f32;
    if natural <= budget {
        return None;
    }
    // Per-boundary natural step and the floor it may compress down to.
    let nats: Vec<f32> = body.iter().map(|w| w + spacing).collect();
    let floors: Vec<f32> = nats
        .iter()
        .zip(body)
        .enumerate()
        .map(|(i, (&nat, &w))| {
            let floor = if i == 0 && pin_first {
                w + spacing
            } else {
                SLIVER * z
            };
            floor.min(nat)
        })
        .collect();
    // Parabolic falloff around the focus: boundary i sits between items i
    // and i+1, so its distance is measured from the boundary's midpoint.
    let weight = |i: usize| {
        let d = (i as f32 + 0.5 - focus as f32).abs();
        1.0 / ((1.0 + d) * (1.0 + d))
    };
    // Waterfill: solve one scale for the weighted slack, cap any boundary
    // that would exceed its natural step at natural, re-solve for the rest.
    // Ends with the last item exactly on the budget (when it can).
    let mut capped = vec![false; nats.len()];
    let mut s;
    loop {
        let used: f32 = capped
            .iter()
            .zip(nats.iter().zip(&floors))
            .map(|(&c, (&nat, &fl))| if c { nat } else { fl })
            .sum();
        let give: f32 = capped
            .iter()
            .zip(nats.iter().zip(&floors))
            .enumerate()
            .filter(|(_, (c, _))| !**c)
            .map(|(i, (_, (nat, fl)))| (nat - fl) * weight(i))
            .sum();
        let avail = budget - last_w - used;
        s = if give > 0.0 {
            (avail / give).max(0.0)
        } else {
            0.0
        };
        let mut grew = false;
        for (i, (c, (nat, fl))) in capped.iter_mut().zip(nats.iter().zip(&floors)).enumerate() {
            if !*c && s * weight(i) >= 1.0 && *nat > *fl {
                *c = true;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut xs = Vec::with_capacity(n);
    let mut x = 0.0;
    for (i, (&c, (&nat, &fl))) in capped.iter().zip(nats.iter().zip(&floors)).enumerate() {
        xs.push(x);
        x += if c {
            nat
        } else {
            fl + (nat - fl) * s * weight(i)
        };
    }
    xs.push(x);
    Some(xs)
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
            line = line.push(item_el(item, selected, z, &goto));
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
        .map(|(item, x)| (x, item_el(item, selected, z, &goto)))
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
fn disc<M: 'static>(
    label: String,
    fill: Color,
    txt: Color,
    selected: bool,
    live: bool,
    z: f32,
) -> Element<'static, M> {
    let emph = if selected { EMPHASIS } else { 1.0 };
    let dia = DISC * z * emph;
    let ring = if selected {
        Color::WHITE
    } else if live {
        YELLOW
    } else {
        Color::from_rgba(1.0, 1.0, 1.0, 0.25)
    };
    container(text(label).size(8.5 * z * emph).color(txt))
        .center(Length::Fixed(dia))
        .style(move |_: &Theme| container::Style {
            background: Some(fill.into()),
            border: iced::Border {
                color: ring,
                width: if selected { 1.5 } else { 1.0 },
                radius: (dia / 2.0).into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// A collapsed wipe run: a disc-height pill reading `×N`, wipe-red like the
/// attempts it stands for but flatter, so it reads as "N of those" rather
/// than one more pull. Clicking lands on the run's most recent wipe.
fn pill<M: 'static>(count: usize, z: f32) -> Element<'static, M> {
    let dia = DISC * z;
    container(text(format!("×{count}")).size(8.5 * z).color(Color::WHITE))
        .center_y(Length::Fixed(dia))
        .padding([0.0, 4.0 * z])
        .style(move |_: &Theme| container::Style {
            background: Some(Color { a: 0.22, ..RED }.into()),
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
    duration_ms: i64,
    selected: bool,
    live: bool,
    z: f32,
) -> Element<'static, M> {
    let w = gap_width(duration_ms, z);
    let color = if live {
        YELLOW
    } else if selected {
        Color::from_rgba(1.0, 1.0, 1.0, 0.9)
    } else {
        DIM
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
    use wowdps_model::{ListRow, SegmentId};

    fn entry(id: u64, kind: SegmentKind, instance: Option<u32>, live: bool) -> ListEntry {
        ListEntry {
            id: SegmentId(id),
            row: ListRow {
                kind,
                name: format!("seg{id}"),
                start_ms: id as i64 * 1000,
                success: None,
                duration_ms: 10_000,
                live,
                instance,
                pars_ms: None,
                arena: false,
            },
        }
    }

    /// The fixture shape: city trash, then a visit (Σ + trash + 2 bosses),
    /// interrupted by city combat, then more of the same visit.
    fn sample() -> Vec<ListEntry> {
        vec![
            entry(0, SegmentKind::Trash, None, false),        // 0 city
            entry(1, SegmentKind::Overall, Some(0), false),   // 1 Σ visit 0
            entry(2, SegmentKind::Trash, Some(0), false),     // 2 pull
            entry(3, SegmentKind::Trash, Some(0), false),     // 3 pull
            entry(4, SegmentKind::Encounter, Some(0), false), // 4 boss 1
            entry(5, SegmentKind::Trash, None, false),        // 5 zoned out
            entry(6, SegmentKind::Trash, Some(0), false),     // 6 back in
            entry(7, SegmentKind::Encounter, Some(0), true),  // 7 boss 2, live
        ]
    }

    #[test]
    fn blocks_group_visits_across_interruptions() {
        let entries = sample();
        let b = blocks(&entries);
        assert_eq!(b.len(), 3, "city, visit 0, zoned-out city");
        assert_eq!(b[0].members, vec![0]);
        assert!(!b[0].is_instance());
        assert_eq!(b[1].ordinal, Some(0));
        assert_eq!(b[1].overall, Some(1));
        assert_eq!(b[1].members, vec![2, 3, 4, 6, 7], "city gap not absorbed");
        assert_eq!(b[2].members, vec![5]);
        assert_eq!(block_of(&b, 3), Some(1));
        assert_eq!(block_of(&b, 5), Some(2));
        assert_eq!(b[1].anchor(), Some(1), "instances anchor on their Σ row");
        assert_eq!(b[2].anchor(), Some(5));
        assert!(is_live(&b[1], &entries));
        assert!(!is_live(&b[0], &entries));
    }

    #[test]
    fn strip_items_coalesce_trash_and_number_bosses() {
        let mut entries = sample();
        entries[1].row.success = Some(true); // key timed
        let b = blocks(&entries);
        let items = items(&b[1], &entries);
        assert_eq!(
            items,
            vec![
                Item::Overall {
                    pos: 1,
                    live: false
                },
                Item::Gap {
                    pos: 2,
                    pulls: 2,
                    duration_ms: 20_000,
                    live: false
                },
                Item::Boss {
                    pos: 4,
                    num: 1,
                    success: None,
                    live: false
                },
                Item::Gap {
                    pos: 6,
                    pulls: 1,
                    duration_ms: 10_000,
                    live: false
                },
                Item::Boss {
                    pos: 7,
                    num: 2,
                    success: None,
                    live: true
                },
                Item::Flag { success: true },
            ]
        );
    }

    /// A progression night: Σ, then 6 wipes on one boss with trash between
    /// two of them, then the live attempt.
    fn wipes() -> Vec<ListEntry> {
        let mut v = vec![entry(0, SegmentKind::Overall, Some(0), false)];
        for i in 1..=6 {
            let mut e = entry(i, SegmentKind::Encounter, Some(0), false);
            e.row.name = "Voidspire".into();
            e.row.success = Some(false);
            v.push(e);
        }
        let mut trash = entry(7, SegmentKind::Trash, Some(0), false);
        trash.row.name = "Trash".into();
        v.insert(4, trash); // between wipe 3 and wipe 4
        let mut live = entry(8, SegmentKind::Encounter, Some(0), true);
        live.row.name = "Voidspire".into();
        v.push(live);
        v
    }

    #[test]
    fn wipe_runs_collapse_but_watched_live_and_newest_survive() {
        let entries = wipes();
        let b = blocks(&entries);
        let all = items(&b[0], &entries);

        // Watching the Σ: the 6 dead wipes (and their interior trash)
        // become one chip pointing at the newest hidden wipe; the live
        // attempt keeps its disc.
        let c = collapse(all, &entries, Some(0));
        assert_eq!(
            c,
            vec![
                Item::Overall {
                    pos: 0,
                    live: false
                },
                Item::Wipes { pos: 7, count: 6 },
                Item::Boss {
                    pos: 8,
                    num: 7,
                    success: None,
                    live: true
                },
            ]
        );

        // Scrubbing into the run surfaces the watched attempt: the run
        // splits, and a side too short for a chip stays as discs.
        let c = collapse(items(&b[0], &entries), &entries, Some(3));
        let watched = Item::Boss {
            pos: 3,
            num: 3,
            success: Some(false),
            live: false,
        };
        assert!(c.contains(&watched), "{c:?}");
        assert!(
            !c.iter()
                .any(|i| matches!(i, Item::Wipes { count, .. } if *count < MIN_COLLAPSE)),
            "no chip hides fewer than it must: {c:?}"
        );
    }

    #[test]
    fn short_runs_and_mixed_bosses_do_not_collapse() {
        let mut entries = wipes();
        // Rename half the wipes: two different bosses, runs of 3 and 3 — but
        // the name break splits them, leaving 3 + 3 which still collapse
        // separately… unless the runs fall under the minimum.
        for e in entries.iter_mut().take(4).skip(2) {
            e.row.name = "Other Boss".into();
        }
        let b = blocks(&entries);
        let c = collapse(items(&b[0], &entries), &entries, Some(0));
        // Runs: wipe1 (Voidspire), wipes 2-3 (Other), wipes 4-6 (Voidspire).
        // Only the last reaches MIN_COLLAPSE.
        let chips: Vec<_> = c
            .iter()
            .filter(|i| matches!(i, Item::Wipes { .. }))
            .collect();
        assert_eq!(chips.len(), 1, "{c:?}");
        assert_eq!(chips[0], &Item::Wipes { pos: 7, count: 3 });
    }

    #[test]
    fn cascade_kicks_in_only_on_overflow_and_pins_the_ends() {
        // Fits: 3 discs of 19 + 2 spacings of 1.5 = 60 ≤ 100.
        assert_eq!(
            cascade_xs(&[19.0, 19.0, 19.0], 1.5, 100.0, 2, true, 1.0),
            None
        );
        // Overflows: the first item starts at 0 and the last ends exactly on
        // the budget, order preserved.
        let xs = cascade_xs(&[19.0; 10], 1.5, 140.0, 5, false, 1.0).expect("must fan");
        assert_eq!(xs.len(), 10);
        assert_eq!(xs[0], 0.0);
        let last = xs.last().copied().unwrap();
        assert!((last + 19.0 - 140.0).abs() < 0.01, "{last}");
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        // Degenerate: no items.
        assert_eq!(cascade_xs(&[], 1.5, 100.0, 0, true, 1.0), None);
    }

    #[test]
    fn cascade_spacing_falls_off_parabolically_from_the_focus() {
        let xs = cascade_xs(&[19.0; 11], 1.5, 150.0, 5, false, 1.0).expect("must fan");
        let step = |i: usize| xs[i + 1] - xs[i];
        // The two boundaries hugging the focus are the widest; each further
        // step is tighter, symmetric on both sides.
        for i in 0..4 {
            assert!(
                step(5 + i) > step(5 + i + 1),
                "right falloff at {i}: {xs:?}"
            );
            assert!(step(4 - i) > step(4 - i - 1), "left falloff at {i}: {xs:?}");
            assert!((step(5 + i) - step(4 - i)).abs() < 0.01, "symmetry at {i}");
        }
        // Nothing tightens past the minimum sliver.
        assert!((0..10).all(|i| step(i) >= SLIVER - 0.01), "{xs:?}");
    }

    #[test]
    fn cascade_keeps_the_pinned_sigma_fully_exposed() {
        // Deep pile, focus far right: without the pin the first boundary
        // would tighten to a sliver; with it, Σ keeps its whole width.
        let xs = cascade_xs(&[19.0; 20], 1.5, 120.0, 19, true, 1.0).expect("must fan");
        assert!(xs[1] - xs[0] >= 19.0 + 1.5 - 0.01, "{xs:?}");
    }

    #[test]
    fn scrub_steps_sigma_then_members_and_clamps() {
        let entries = sample();
        let b = blocks(&entries);
        let visit = &b[1];
        assert_eq!(scrub(visit, 1, 1), Some(2), "Σ → first pull");
        assert_eq!(scrub(visit, 2, -1), Some(1), "first pull → Σ");
        assert_eq!(scrub(visit, 1, -1), None, "nothing before Σ");
        assert_eq!(
            scrub(visit, 4, 1),
            Some(6),
            "skips the zoned-out city segment"
        );
        assert_eq!(scrub(visit, 7, 1), None, "nothing after the live boss");
        assert_eq!(scrub(visit, 0, 1), None, "position outside the block");
    }

    // ---- rendering geometry ---------------------------------------------------

    #[test]
    fn gap_width_hints_at_time_spent_within_bounds() {
        assert_eq!(gap_width(0, 1.0), GAP_MIN);
        assert_eq!(gap_width(-5_000, 1.0), GAP_MIN, "negative durations clamp");
        // 100 s → √100 × 0.9 = 9 on top of the minimum.
        assert!((gap_width(100_000, 1.0) - (GAP_MIN + 9.0)).abs() < 1e-4);
        assert_eq!(gap_width(10_000_000, 1.0), GAP_MAX, "an hour is capped");
        assert_eq!(gap_width(0, 2.0), 2.0 * GAP_MIN, "zoom scales the line");
        assert!(gap_width(30_000, 1.0) < gap_width(120_000, 1.0));
    }

    #[test]
    fn item_positions_and_natural_widths() {
        let overall = Item::Overall {
            pos: 1,
            live: false,
        };
        let boss = Item::Boss {
            pos: 4,
            num: 1,
            success: Some(true),
            live: false,
        };
        let gap = Item::Gap {
            pos: 2,
            pulls: 2,
            duration_ms: 100_000,
            live: false,
        };
        let wipes = Item::Wipes { pos: 7, count: 12 };
        let flag = Item::Flag { success: true };
        assert_eq!(item_pos(&overall), Some(1));
        assert_eq!(item_pos(&boss), Some(4));
        assert_eq!(item_pos(&gap), Some(2));
        assert_eq!(item_pos(&wipes), Some(7));
        assert_eq!(item_pos(&flag), None, "the flag stands for no segment");

        let slack = 2.0 * HIT_PAD_X;
        assert_eq!(natural_width(&overall, 1.0, false), DISC + slack);
        assert_eq!(natural_width(&boss, 1.0, true), DISC * EMPHASIS + slack);
        assert_eq!(
            natural_width(&gap, 1.0, false),
            gap_width(100_000, 1.0) + slack
        );
        // "×12": three glyphs.
        assert!((natural_width(&wipes, 1.0, false) - (8.0 + 3.0 * 4.7 + slack)).abs() < 1e-4);
        assert_eq!(natural_width(&flag, 1.0, false), 10.0);
        assert_eq!(
            natural_width(&flag, 2.0, true),
            20.0,
            "zoom scales, emphasis ignored"
        );
        assert!(natural_width(&overall, 2.0, false) > natural_width(&overall, 1.0, true));
    }

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
                let _: Element<'static, usize> = item_el(item, sel, z, &|p| p);
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
        for (selected, live) in [(false, false), (true, false), (false, true), (true, true)] {
            let _: Element<'static, usize> =
                disc("1".to_string(), RED, Color::WHITE, selected, live, 1.0);
            let _: Element<'static, usize> = gap_line(30_000, selected, live, 1.0);
            let _: Element<'static, usize> = gap_line(0, selected, live, 2.0);
        }
        let _: Element<'static, usize> = pill(3, 1.0);
        let _: Element<'static, usize> = pill(120, 2.0);
        let _: Element<'static, usize> = hit(pill(3, 1.0), 7usize, 1.0);
    }
}
