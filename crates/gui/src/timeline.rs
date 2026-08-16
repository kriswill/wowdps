//! The instance timeline: grouping of the segment list into *blocks* (one per
//! instance visit, one per stray segment outside instances) and the compact
//! Σ–①─②─③–⚑ strip the overlay draws for the block being watched.
//!
//! The model half is pure functions over the client's id table
//! (`ClientState::entries()`), so navigation and rendering agree on
//! positions by construction; the rendering half emits no messages of its
//! own — callers supply a `position → message` constructor.

use iced::widget::{Space, container, mouse_area, row, scrollable, text};
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

/// Render the strip. `selected` is the watched entries position; `goto`
/// turns a clicked element's position into the frontend's message.
pub fn strip<M: Clone + 'static>(
    items: &[Item],
    selected: Option<usize>,
    z: f32,
    goto: impl Fn(usize) -> M,
) -> Element<'static, M> {
    let mut line = row![].spacing(1.5 * z).align_y(iced::Alignment::Center);
    for item in items {
        line =
            line.push(match *item {
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
                Item::Flag { success } => Element::from(
                    text("⚑")
                        .size(11.0 * z)
                        .color(if success { GREEN } else { RED }),
                ),
            });
    }
    scrollable(line)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(2.0 * z)
                .scroller_width(2.0 * z),
        ))
        .anchor_right()
        .width(Length::Fill)
        .into()
}

/// A circular marker: number or Σ, colored fill, selection ring.
fn disc<M: 'static>(
    label: String,
    fill: Color,
    txt: Color,
    selected: bool,
    live: bool,
    z: f32,
) -> Element<'static, M> {
    let dia = DISC * z;
    let ring = if selected {
        Color::WHITE
    } else if live {
        YELLOW
    } else {
        Color::from_rgba(1.0, 1.0, 1.0, 0.25)
    };
    container(text(label).size(8.5 * z).color(txt))
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
    let secs = (duration_ms.max(0) / 1000) as f32;
    let w = (GAP_MIN + secs.sqrt() * 0.9).clamp(GAP_MIN, GAP_MAX) * z;
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
}
