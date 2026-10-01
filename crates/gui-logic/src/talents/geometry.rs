//! Where things go and what they say: hit-testing a pane, the choice
//! picker's tiles, the shapes a pane strokes (octagons, arrowheads, carets,
//! rank badges), the tooltip's lines and its placement beside the hovered
//! icon, and the inventory tab's words. Every number here is the iced
//! viewer's, so both GUIs lay a tree out to the same pixel.

use super::model::{Node, PaneModel, TILE};
use crate::simc;

/// A point in pane (or window) pixels.
pub type Pt = (f32, f32);

/// The node whose tile is under `(x, y)`, pane-local: the nearest within
/// the tile's radius and a little slack.
pub fn node_at(model: &PaneModel, x: f32, y: f32) -> Option<&Node> {
    model
        .nodes
        .iter()
        .min_by(|a, b| {
            let d = |n: &Node| (n.x - x).hypot(n.y - y);
            d(a).total_cmp(&d(b))
        })
        .filter(|n| (n.x - x).hypot(n.y - y) <= TILE / 2.0 + 3.0)
}

/// Option tiles of the expanded choice picker: a horizontal strip through
/// the node, clamped inside the pane.
pub fn picker_spots(model: &PaneModel, node: &Node) -> Vec<Pt> {
    const STEP: f32 = 40.0;
    let k = node.options.len().max(1);
    let total = k as f32 * STEP;
    let mut x0 = node.x - total / 2.0 + STEP / 2.0;
    x0 = x0.max(TILE / 2.0 + 6.0);
    x0 = x0.min(model.w - total + STEP / 2.0 - TILE / 2.0 - 6.0);
    (0..k).map(|i| (x0 + i as f32 * STEP, node.y)).collect()
}

/// The picker's node, when `picker` names one this pane holds.
pub fn picker_node(model: &PaneModel, picker: Option<u64>) -> Option<&Node> {
    let id = picker?;
    model.nodes.iter().find(|n| n.id == id)
}

/// The open picker's option under `(x, y)`, if the picker is in this pane:
/// (node id, option index).
pub fn option_at(model: &PaneModel, picker: Option<u64>, x: f32, y: f32) -> Option<(u64, u64)> {
    let node = picker_node(model, picker)?;
    picker_spots(model, node)
        .iter()
        .position(|c| (c.0 - x).hypot(c.1 - y) <= TILE / 2.0 + 4.0)
        .map(|i| (node.id, i as u64))
}

/// What the pointer at `(x, y)` is over: a picker option tile (they sit
/// over neighbouring nodes, so they win), else a node — with the tile's
/// center, pane-local, for the tooltip's anchor.
pub fn hit(
    model: &PaneModel,
    picker: Option<u64>,
    x: f32,
    y: f32,
) -> Option<(u64, Option<u64>, Pt)> {
    option_at(model, picker, x, y)
        .and_then(|(node, i)| {
            let n = picker_node(model, picker)?;
            let c = picker_spots(model, n).get(i as usize).copied()?;
            Some((node, Some(i), c))
        })
        .or_else(|| node_at(model, x, y).map(|n| (n.id, None, (n.x, n.y))))
}

/// The node reshaped as one of its choice options, so the picker's option
/// tiles get full tooltips of their own.
pub fn option_view(node: &Node, index: usize) -> Option<Node> {
    let opt = node.options.get(index)?;
    let mut n = node.clone();
    n.name = opt.name.clone();
    n.spell_id = opt.spell_id;
    n.desc = opt.desc.clone();
    n.cost = opt.cost.clone();
    n.range = opt.range.clone();
    n.cast = opt.cast.clone();
    n.desc_ranks = opt.desc_ranks.clone();
    // The alternatives line would repeat the strip below the pointer.
    n.options = Vec::new();
    n.tiers = Vec::new();
    n.choice = false;
    Some(n)
}

/// The octagon's eight corners around `(cx, cy)` with half-extent `r`, the
/// corner cut matching the icon mask (29% of the tile edge).
pub fn octagon(cx: f32, cy: f32, r: f32) -> [Pt; 8] {
    let k = 2.0 * r * 0.29;
    [
        (cx - r + k, cy - r),
        (cx + r - k, cy - r),
        (cx + r, cy - r + k),
        (cx + r, cy + r - k),
        (cx + r - k, cy + r),
        (cx - r + k, cy + r),
        (cx - r, cy + r - k),
        (cx - r, cy - r + k),
    ]
}

/// A lit edge's arrowhead: its tip just outside the destination tile's
/// frame, pointing into the node the point flowed to.
pub fn arrowhead(from: Pt, to: Pt) -> [Pt; 3] {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = dx.hypot(dy).max(1.0);
    let (ux, uy) = (dx / len, dy / len);
    let tip = (
        to.0 - ux * (TILE / 2.0 + 2.5),
        to.1 - uy * (TILE / 2.0 + 2.5),
    );
    let base = (tip.0 - ux * 7.0, tip.1 - uy * 7.0);
    [
        tip,
        (base.0 - uy * 4.5, base.1 + ux * 4.5),
        (base.0 + uy * 4.5, base.1 - ux * 4.5),
    ]
}

/// A choice node's side carets, left then right.
pub fn carets(n: &Node) -> [[Pt; 3]; 2] {
    [-1.0f32, 1.0].map(|side| {
        let bx = n.x + side * (TILE / 2.0 + 3.0);
        [(bx + side * 4.0, n.y), (bx, n.y - 4.0), (bx, n.y + 4.0)]
    })
}

/// The rank badge: its words and its rect (x, y, w, h), overlapping the
/// tile's lower right corner so it never covers the path lines. Every node
/// wears one (0/1 included), like the game's editor.
pub fn badge(n: &Node) -> (String, [f32; 4]) {
    let content = format!("{}/{}", n.ranks, n.max_ranks);
    let bw = content.len() as f32 * 6.0 + 6.0;
    let rect = [
        n.x + TILE / 2.0 + 5.0 - bw,
        n.y + TILE / 2.0 - 7.0,
        bw,
        13.0,
    ];
    (content, rect)
}

/// The open picker's backing plate (x, y, w, h) behind its option tiles.
pub fn picker_plate(spots: &[Pt], node: &Node) -> Option<[f32; 4]> {
    let (first, last) = (spots.first()?, spots.last()?);
    let pad = TILE / 2.0 + 8.0;
    Some([
        first.0 - pad,
        node.y - pad,
        last.0 - first.0 + 2.0 * pad,
        2.0 * pad,
    ])
}

/// "12/34 pts" when the cap is known, else "12 pts"; `true` once full (the
/// label wears the talent gold then).
pub fn points_label(points: u64, cap: Option<u64>) -> (String, bool) {
    match cap {
        Some(cap) => (format!("{points}/{cap} pts"), points >= cap),
        None => (format!("{points} pts"), false),
    }
}

// ---- the tooltip ------------------------------------------------------------

/// The tooltip's widest box.
pub const TIP_W: f32 = 300.0;
/// Its words' inset from either side.
pub const TIP_PAD_X: f32 = 9.0;

/// A tooltip line's role, which each GUI colours from its theme: the
/// game's white for names and costs, grey for kinds and ranks, yellow for
/// the description, blue for a trailing restriction, dim for ranks the
/// build has not reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Meta,
    Desc,
    Note,
    Unreached,
}

/// One tooltip line: its text, a second right-aligned text sharing the row
/// (the cost/range line), its role and its size. An empty text with a
/// small size is a paragraph gap.
#[derive(Debug, Clone, PartialEq)]
pub struct TipLine {
    pub text: String,
    pub right: Option<String>,
    pub tone: Tone,
    pub size: f32,
}

/// The box's width on a surface `w` wide.
pub fn tip_width(w: f32) -> f32 {
    TIP_W.min(w - 8.0).max(180.0)
}

/// The wrap budget, in characters, for a box `tw` wide: canvas text has no
/// layout engine, ~5.6 px per character at 11 px.
pub fn tip_budget(tw: f32) -> usize {
    ((tw - 2.0 * TIP_PAD_X) / 5.6) as usize
}

/// The game-style tooltip: title, "Talent", cost + range, cast time,
/// "Requires <class>", then the description — yellow, with any trailing
/// restriction paragraph in blue, exactly how the game colours them —
/// wrapped at `budget` characters.
pub fn tooltip_lines(n: &Node, requires: &str, budget: usize) -> Vec<TipLine> {
    let mut lines: Vec<TipLine> = Vec::new();
    let mut push = |text: String, right: Option<String>, tone: Tone, size: f32| {
        lines.push(TipLine {
            text,
            right,
            tone,
            size,
        });
    };
    push(
        if n.name.is_empty() {
            n.detail.clone()
        } else {
            n.name.clone()
        },
        None,
        Tone::Plain,
        13.0,
    );
    push("Talent".to_string(), None, Tone::Meta, 10.0);
    match (n.cost.is_empty(), n.range.is_empty()) {
        (false, false) => push(n.cost.clone(), Some(n.range.clone()), Tone::Plain, 11.0),
        (false, true) => push(n.cost.clone(), None, Tone::Plain, 11.0),
        (true, false) => push(n.range.clone(), None, Tone::Plain, 11.0),
        (true, true) => {}
    }
    if !n.cast.is_empty() {
        push(n.cast.clone(), None, Tone::Plain, 11.0);
    }
    if !requires.is_empty() {
        push(format!("Requires {requires}"), None, Tone::Plain, 11.0);
    }
    push(
        format!("Rank {}/{}", n.ranks, n.max_ranks),
        None,
        Tone::Meta,
        10.0,
    );
    // Unpicked ranks read dim, the way the game greys them out.
    let rank_paras =
        |text: &str,
         prefix: Option<String>,
         tone: Tone,
         push: &mut dyn FnMut(String, Option<String>, Tone, f32)| {
            for (pi, para) in text.split("\n\n").enumerate() {
                if pi > 0 {
                    push(String::new(), None, Tone::Plain, 3.0);
                }
                let para = para.replace('\n', " ");
                let text = match (&prefix, pi) {
                    (Some(p), 0) => format!("{p}{para}"),
                    _ => para,
                };
                for line in wrap_text(&text, budget) {
                    push(line, None, tone, 11.0);
                }
            }
        };
    if !n.tiers.is_empty() {
        // A tiered node: each tier is its own spell — "Rank N" sections
        // with each stage's description, the way the game presents them.
        // A multi-rank stage lists every rank's values ("(1): …, (2): …");
        // ranks the build has not reached grey out.
        let mut cum: u64 = 0;
        for (ti, tier) in n.tiers.iter().enumerate() {
            push(String::new(), None, Tone::Plain, 4.0);
            push(format!("Rank {}", ti + 1), None, Tone::Plain, 11.0);
            if tier.max_ranks > 1 && tier.desc_ranks.len() >= tier.max_ranks as usize {
                for k in 1..=tier.max_ranks {
                    if k > 1 {
                        push(String::new(), None, Tone::Plain, 3.0);
                    }
                    let tone = if cum + k <= n.ranks {
                        Tone::Desc
                    } else {
                        Tone::Unreached
                    };
                    if let Some(text) = tier.desc_ranks.get((k - 1) as usize) {
                        rank_paras(text, Some(format!("({k}): ")), tone, &mut push);
                    }
                }
            } else {
                let tone = if cum < n.ranks {
                    Tone::Desc
                } else {
                    Tone::Unreached
                };
                rank_paras(&tier.desc, None, tone, &mut push);
            }
            cum += tier.max_ranks;
        }
    } else if n.max_ranks > 1 && n.desc_ranks.len() >= n.max_ranks as usize {
        // A plain multi-rank talent: every rank's values, unreached ranks
        // greyed.
        for k in 1..=n.max_ranks {
            push(String::new(), None, Tone::Plain, 4.0);
            let tone = if k <= n.ranks {
                Tone::Desc
            } else {
                Tone::Unreached
            };
            if let Some(text) = n.desc_ranks.get((k - 1) as usize) {
                rank_paras(text, Some(format!("({k}): ")), tone, &mut push);
            }
        }
    } else if !n.desc.is_empty() {
        // Paragraphs split on blank lines; the trailing restriction
        // paragraph(s) — "Curses: …" — go blue like the game's.
        let paras: Vec<&str> = n.desc.split("\n\n").collect();
        let n_paras = paras.len();
        for (pi, para) in paras.into_iter().enumerate() {
            push(String::new(), None, Tone::Plain, 4.0); // paragraph gap
            let tone = if pi + 1 == n_paras && n_paras > 1 {
                Tone::Note
            } else {
                Tone::Desc
            };
            for line in wrap_text(&para.replace('\n', " "), budget) {
                push(line, None, tone, 11.0);
            }
        }
    }
    if n.choice && n.options.len() > 1 {
        push(String::new(), None, Tone::Plain, 4.0);
        let names: Vec<&str> = n.options.iter().map(|o| o.name.as_str()).collect();
        for line in wrap_text(&names.join(" / "), budget) {
            push(line, None, Tone::Meta, 10.0);
        }
    }
    lines
}

/// The box's height for its lines: each line its size and 4 px, and 12 px
/// of padding.
pub fn tip_height(lines: &[TipLine]) -> f32 {
    lines.iter().map(|l| l.size + 4.0).sum::<f32>() + 12.0
}

/// The box's top-left for a tile centred at `cur`: beside the icon, its top
/// a little above it (the in-game placement), so the hovered talent's
/// neighbours stay visible; flipped to the left when the right side has no
/// room, and held inside the `w` × `h` surface.
pub fn tip_origin(cur: Pt, tw: f32, th: f32, w: f32, h: f32) -> Pt {
    let mut at = (cur.0 + TILE / 2.0 + 12.0, cur.1 - TILE / 2.0 - 8.0);
    if at.0 + tw > w - 2.0 {
        at.0 = (cur.0 - TILE / 2.0 - 12.0 - tw).max(2.0);
    }
    at.1 = at.1.clamp(2.0, (h - th - 2.0).max(2.0));
    at
}

/// Greedy word wrap at a character budget (canvas text has no layout).
pub fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > max_chars {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

// ---- the inventory tab ------------------------------------------------------

/// v19: COMBATANT_INFO's equippedItems dump is positional — the standard
/// inventory-slot order. Labels apply only when the count fits the table;
/// an unexpected shape falls back to unlabeled rows rather than lying.
pub const GEAR_SLOTS: [&str; 18] = [
    "head",
    "neck",
    "shoulder",
    "shirt",
    "chest",
    "waist",
    "legs",
    "feet",
    "wrist",
    "hands",
    "finger 1",
    "finger 2",
    "trinket 1",
    "trinket 2",
    "back",
    "main hand",
    "off hand",
    "tabard",
];

/// The slot label of the `i`th of `total` logged items: "" when the dump's
/// shape does not fit the slot table.
pub fn gear_slot(i: usize, total: usize) -> &'static str {
    if total <= GEAR_SLOTS.len() {
        GEAR_SLOTS.get(i).copied().unwrap_or("")
    } else {
        ""
    }
}

/// "enchanted · 2 gems": what an item carries beyond itself.
pub fn extras(enchanted: bool, gems: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    if enchanted {
        out.push("enchanted".to_string());
    }
    if gems > 0 {
        out.push(format!("{gems} gem{}", if gems == 1 { "" } else { "s" }));
    }
    out.join(" · ")
}

/// A pasted item's name, else the honest `item {id}`.
pub fn item_name(i: &simc::Item) -> String {
    i.name.clone().unwrap_or_else(|| format!("item {}", i.id))
}

/// A currency line's kind word.
pub fn currency_kind(c: &simc::Currency) -> &'static str {
    match (c.is_currency, c.catalyst) {
        (true, true) => "catalyst",
        (true, false) => "currency",
        (false, _) => "item",
    }
}

/// The paste's identity line: level, spec, class, realm, game version.
pub fn identity(p: &simc::Profile) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(l) = p.level {
        parts.push(format!("level {l}"));
    }
    if let Some(s) = &p.spec {
        parts.push(s.clone());
    }
    if let Some(c) = &p.class_token {
        parts.push(c.clone());
    }
    if let Some(s) = &p.server {
        parts.push(s.clone());
    }
    if let Some(v) = &p.wow_version {
        parts.push(format!("WoW {v}"));
    }
    parts.join(" · ")
}
