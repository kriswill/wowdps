//! The decoded, layout-ready build: a spec's tree (`tree_view`) and the
//! picked nodes laid out into the game's three panes, each node placed in
//! pane pixels with its frame state, ranks, shape and tooltip lines.

use std::collections::HashMap;
use std::rc::Rc;

use wowdps_proto::json::Json;
use wowdps_proto::talents as codec;

use crate::spell_icons::IconStyle;

/// Grid pitch: the dataset's node coordinates step by 600 per tree column.
pub const GRID: f32 = 600.0;
/// Pane pixels per grid step, and the node tile drawn on it.
pub const CELL: f32 = 44.0;
pub const TILE: f32 = 28.0;
/// The pane's margin around its outermost nodes.
pub const PAD: f32 = 24.0;

/// One node, positioned in pane pixels.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub selected: bool,
    pub granted: bool,
    /// Untaken, but pickable right now: a root or a child of a taken node,
    /// with the pane's point gate satisfied. Wears the green outline.
    pub available: bool,
    /// The node's point gate (`reqPoints`): how many points must be spent
    /// above the gate before this node unlocks. 0 = ungated.
    pub req: u64,
    pub ranks: u64,
    pub max_ranks: u64,
    pub choice: bool,
    /// The game's node shape: square = active ability, circle = passive,
    /// octagon = choice.
    pub shape: IconStyle,
    pub spell_id: u32,
    /// The choice alternatives, in entry order. Empty for plain nodes.
    pub options: Vec<ChoiceOption>,
    /// A tiered node's rank stages, in order — each tier is its own spell
    /// with its own description, shown as "Rank N" tooltip sections.
    pub tiers: Vec<ChoiceOption>,
    /// The hover tooltip's line: name, ranks, choice alternatives.
    pub detail: String,
    /// The picked entry's name alone, the tooltip's title.
    pub name: String,
    /// Tooltip lines from the dataset (empty when the dataset predates
    /// them): substituted description, cost, range, cast time.
    pub desc: String,
    pub cost: String,
    pub range: String,
    pub cast: String,
    /// Rank-scaled description variants of the picked entry, for plain
    /// multi-rank nodes.
    pub desc_ranks: Vec<String>,
}

/// An entry's tooltip fields, straight off the dataset.
fn entry_option(e: &Json) -> ChoiceOption {
    ChoiceOption {
        spell_id: get_u64(e, "spellId").unwrap_or(0) as u32,
        name: get_str(e, "name").to_string(),
        desc: get_str(e, "desc").to_string(),
        cost: get_str(e, "cost").to_string(),
        range: get_str(e, "range").to_string(),
        cast: get_str(e, "cast").to_string(),
        max_ranks: get_u64(e, "maxRanks").unwrap_or(1),
        desc_ranks: arr(e.get("descRanks"))
            .iter()
            .filter_map(|d| d.as_str().map(str::to_string))
            .collect(),
    }
}

/// One alternative of a choice node, with its own tooltip lines so the
/// expanded picker can describe each option.
#[derive(Debug, Clone)]
pub struct ChoiceOption {
    pub spell_id: u32,
    pub name: String,
    pub desc: String,
    pub cost: String,
    pub range: String,
    pub cast: String,
    /// This entry's own rank count (a tiered node's middle stage can hold
    /// several).
    pub max_ranks: u64,
    /// Rank-scaled description variants, when the entry ranks above 1.
    pub desc_ranks: Vec<String>,
}

/// A slot for whatever a GUI retains per pane between frames — the iced
/// viewer's tessellation caches. Every rebuild lays out a fresh pane, and a
/// fresh or cloned pane starts with the slot empty, so retained drawing
/// can never outlive the layout it was drawn from. Nothing here reads it.
#[derive(Default)]
pub struct Retained(std::cell::OnceCell<Box<dyn std::any::Any>>);

impl Retained {
    /// The retained `T`, made on first use. `None` only when this slot
    /// already holds some other type, which one GUI never does.
    pub fn get<T: std::any::Any + Default>(&self) -> Option<&T> {
        self.0
            .get_or_init(|| Box::new(T::default()))
            .downcast_ref::<T>()
    }
}

impl std::fmt::Debug for Retained {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Retained")
    }
}

impl Clone for Retained {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct PaneModel {
    pub points: u64,
    /// The pane's point budget at max level, from the tree currency's
    /// `max` (absent in datasets that predate it: no cap enforced).
    pub cap: Option<u64>,
    /// The class name, for the tooltip's "Requires …" line.
    pub requires: String,
    pub w: f32,
    pub h: f32,
    pub nodes: Vec<Node>,
    /// Indices into `nodes`.
    pub edges: Vec<(usize, usize)>,
    /// A GUI's retained drawing state for this layout.
    pub retained: Retained,
}

impl PaneModel {
    /// The pane is at its point cap: nothing further can be taken in it.
    pub fn full(&self) -> bool {
        self.cap.is_some_and(|c| self.points >= c)
    }
}

/// A decoded build (or a bare spec tree), ready to draw: the class pane on
/// the left, the spec pane on the right (the way the game and every build
/// site lay it out), and the picked hero tree between them.
#[derive(Debug, Clone)]
pub struct Build {
    pub class_name: String,
    pub spec_name: String,
    pub spec_id: u32,
    /// The picked hero tree, when the decode carried one.
    pub hero: Option<(u32, String)>,
    pub dataset_build: String,
    pub warnings: Vec<String>,
    pub class_pane: Rc<PaneModel>,
    pub spec_pane: Rc<PaneModel>,
    pub hero_pane: Option<Rc<PaneModel>>,
}

impl Build {
    /// The drawn panes, left to right: class, hero, spec.
    pub fn panes(&self) -> impl Iterator<Item = &Rc<PaneModel>> {
        [
            Some(&self.class_pane),
            self.hero_pane.as_ref(),
            Some(&self.spec_pane),
        ]
        .into_iter()
        .flatten()
    }
}

/// One selected node — decoded from a string, then edited in place by the
/// viewer's clicks. The map of these (plus the hero pick) IS the build;
/// the laid-out panes are re-derived from it after every edit.
#[derive(Debug, Clone)]
pub struct Sel {
    pub ranks: u64,
    pub granted: bool,
    pub choice_index: Option<u64>,
}

/// The decode result reshaped for editing.
pub fn selections_from_decode(dec: &Json) -> (HashMap<u64, Sel>, Option<u64>, Vec<String>) {
    let mut sels = HashMap::new();
    for s in arr(dec.get("selections")) {
        let Some(id) = get_u64(s, "node_id") else {
            continue;
        };
        sels.insert(
            id,
            Sel {
                ranks: get_u64(s, "ranks").unwrap_or(1),
                granted: s.get("granted") == Some(&Json::Bool(true)),
                choice_index: get_u64(s, "choice_index"),
            },
        );
    }
    let hero = dec.get("hero_tree").and_then(|h| get_u64(h, "id"));
    let warnings = arr(dec.get("warnings"))
        .iter()
        .filter_map(|w| w.as_str().map(str::to_string))
        .collect();
    (sels, hero, warnings)
}

pub(crate) fn arr(v: Option<&Json>) -> &[Json] {
    match v {
        Some(Json::Arr(items)) => items,
        _ => &[],
    }
}

pub(crate) fn get_u64(v: &Json, key: &str) -> Option<u64> {
    v.get(key).and_then(Json::as_u64)
}

pub(crate) fn get_str<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or("")
}

/// A decoded string, ready to edit: spec, selections, hero pick, warnings.
pub type Decoded = (u64, HashMap<u64, Sel>, Option<u64>, Vec<String>);

/// Decode a string into editable selections (the caller lays it out via
/// `Viewer::rebuild`, which caches the spec's `tree_view`).
pub fn decode_build(string: &str) -> Result<Decoded, String> {
    let ds = super::load_dataset()?;
    let dec = codec::decode(ds, string)?;
    let spec_id = get_u64(&dec, "spec_id").ok_or("decode returned no spec_id")?;
    let (sels, hero, warnings) = selections_from_decode(&dec);
    Ok((spec_id, sels, hero, warnings))
}

/// Lay a spec's `tree_view` out with `sels` taken: the class/spec halves
/// split at the midpoint of the non-hero nodes' grid x, the hero pane the
/// picked hero tree alone.
pub fn build_model(
    tv: &Json,
    sels: &HashMap<u64, Sel>,
    hero_id: Option<u64>,
    warnings: Vec<String>,
) -> Result<Build, String> {
    let class_name = get_str(tv, "class").to_string();
    let spec_name = get_str(tv, "spec").to_string();
    let dataset_build = get_str(tv, "build").to_string();

    let nodes = arr(tv.get("nodes"));
    // The class/spec divide: midpoint of the non-hero nodes' grid x.
    let xs: Vec<i64> = nodes
        .iter()
        .filter(|n| get_u64(n, "subTreeId").is_none())
        .filter_map(|n| n.get("posX").and_then(Json::as_f64))
        .map(|x| x as i64)
        .collect();
    let mid = match (xs.iter().min(), xs.iter().max()) {
        (Some(lo), Some(hi)) => (*lo + *hi) as f64 / 2.0,
        _ => return Err("tree has no positioned nodes".to_string()),
    };

    let hero_name = arr(tv.get("sub_trees"))
        .iter()
        .find(|s| get_u64(s, "id") == hero_id)
        .map(|s| get_str(s, "name").to_string());

    let side = |n: &Json| -> u8 {
        match get_u64(n, "subTreeId") {
            Some(_) => 2,
            None => {
                let x = n.get("posX").and_then(Json::as_f64).unwrap_or(0.0);
                u8::from(x > mid)
            }
        }
    };
    // The pane's point budget: its nodes' cost currency's max-level total.
    let cur_max: HashMap<u64, u64> = arr(tv.get("currencies"))
        .iter()
        .filter_map(|c| Some((get_u64(c, "id")?, get_u64(c, "max")?)))
        .collect();

    // The hero-choice selector node is not drawn: the pick shows as the
    // medallion + name between the panes, the way the game presents it.
    let pane = |which: u8| -> Option<Rc<PaneModel>> {
        let members: Vec<&Json> = nodes
            .iter()
            .filter(|n| get_str(n, "type") != "subtree")
            .filter(|n| side(n) == which)
            // The hero pane draws the PICKED tree only; without a pick (or
            // without a string at all) there is nothing to lay out.
            .filter(|n| which != 2 || get_u64(n, "subTreeId") == hero_id)
            .collect();
        let cap = members.iter().find_map(|n| {
            arr(n.get("costs"))
                .first()
                .and_then(|c| get_u64(c, "currency"))
                .and_then(|id| cur_max.get(&id).copied())
        });
        (!members.is_empty()).then(|| Rc::new(layout_pane(&members, sels, cap, &class_name)))
    };
    let (class_pane, spec_pane) = match (pane(0), pane(1)) {
        (Some(c), Some(s)) => (c, s),
        _ => return Err("tree is missing its class or spec half".to_string()),
    };
    let hero_pane = pane(2);

    let spec_id = get_u64(tv, "spec_id").unwrap_or(0) as u32;
    Ok(Build {
        class_name,
        spec_name,
        spec_id,
        hero: hero_id.zip(hero_name).map(|(id, name)| (id as u32, name)),
        dataset_build,
        warnings,
        class_pane,
        spec_pane,
        hero_pane,
    })
}

fn layout_pane(
    members: &[&Json],
    sels: &HashMap<u64, Sel>,
    cap: Option<u64>,
    requires: &str,
) -> PaneModel {
    let pos = |n: &Json, key: &str| n.get(key).and_then(Json::as_f64).unwrap_or(0.0) as f32;
    let min_x = members
        .iter()
        .map(|n| pos(n, "posX"))
        .fold(f32::MAX, f32::min);
    let min_y = members
        .iter()
        .map(|n| pos(n, "posY"))
        .fold(f32::MAX, f32::min);
    let max_x = members
        .iter()
        .map(|n| pos(n, "posX"))
        .fold(f32::MIN, f32::max);
    let max_y = members
        .iter()
        .map(|n| pos(n, "posY"))
        .fold(f32::MIN, f32::max);

    let mut points = 0u64;
    let mut index: HashMap<u64, usize> = HashMap::new();
    let mut out: Vec<Node> = Vec::new();
    let mut reqs: Vec<u64> = Vec::new();
    for n in members {
        let Some(id) = get_u64(n, "id") else {
            continue;
        };
        let entries = arr(n.get("entries"));
        let sel = sels.get(&id);
        let picked = sel
            .and_then(|s| s.choice_index)
            .and_then(|i| entries.get(i as usize))
            .or_else(|| entries.first());
        let choice = matches!(get_str(n, "type"), "choice" | "subtree");
        let max_ranks = get_u64(n, "maxRanks").unwrap_or(1);
        let ranks = sel.map_or(0, |s| s.ranks);
        if let Some(s) = sel
            && !s.granted
        {
            points += s.ranks;
        }
        let name = picked.map(|e| get_str(e, "name")).unwrap_or("");
        let mut detail = if choice && entries.len() > 1 {
            entries
                .iter()
                .map(|e| {
                    let n = get_str(e, "name");
                    if picked.map(|p| std::ptr::eq(p, e)) == Some(true) && sel.is_some() {
                        format!("▸ {n}")
                    } else {
                        n.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("  /  ")
        } else {
            name.to_string()
        };
        if max_ranks > 1 {
            detail.push_str(&format!("  —  {ranks}/{max_ranks}"));
        } else if sel.is_some() {
            detail.push_str("  —  taken");
        }
        if sel.is_some_and(|s| s.granted) {
            detail.push_str(" (granted)");
        }

        // The game's frame shapes: octagon for a choice, square for an
        // active ability (entryType 1), circle for a passive.
        let shape = if choice {
            IconStyle::Octagon
        } else if picked.and_then(|e| get_u64(e, "entryType")) == Some(1) {
            IconStyle::Square
        } else {
            IconStyle::Circle
        };
        index.insert(id, out.len());
        reqs.push(get_u64(n, "reqPoints").unwrap_or(0));
        out.push(Node {
            id,
            x: PAD + (pos(n, "posX") - min_x) / GRID * CELL,
            y: PAD + (pos(n, "posY") - min_y) / GRID * CELL,
            selected: sel.is_some(),
            granted: sel.is_some_and(|s| s.granted),
            available: false, // filled in below, once the edges exist
            req: get_u64(n, "reqPoints").unwrap_or(0),
            ranks,
            max_ranks,
            choice,
            shape,
            spell_id: picked.and_then(|e| get_u64(e, "spellId")).unwrap_or(0) as u32,
            options: if choice {
                entries.iter().map(entry_option).collect()
            } else {
                Vec::new()
            },
            tiers: if get_str(n, "type") == "tiered" && entries.len() > 1 {
                entries.iter().map(entry_option).collect()
            } else {
                Vec::new()
            },
            detail,
            name: name.to_string(),
            desc: picked.map(|e| get_str(e, "desc")).unwrap_or("").to_string(),
            cost: picked.map(|e| get_str(e, "cost")).unwrap_or("").to_string(),
            range: picked
                .map(|e| get_str(e, "range"))
                .unwrap_or("")
                .to_string(),
            cast: picked.map(|e| get_str(e, "cast")).unwrap_or("").to_string(),
            desc_ranks: picked
                .map(|e| {
                    arr(e.get("descRanks"))
                        .iter()
                        .filter_map(|d| d.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }

    let mut edges = Vec::new();
    for n in members {
        let Some(from) = get_u64(n, "id").and_then(|id| index.get(&id).copied()) else {
            continue;
        };
        for to in arr(n.get("next")) {
            if let Some(to) = to.as_u64().and_then(|id| index.get(&id).copied()) {
                edges.push((from, to));
            }
        }
    }

    // Availability (the green outline): untaken, its point gate satisfied,
    // and either a root (no incoming edge) or fed by a taken node.
    let mut has_incoming = vec![false; out.len()];
    let mut fed = vec![false; out.len()];
    for &(a, b) in &edges {
        if let Some(slot) = has_incoming.get_mut(b) {
            *slot = true;
        }
        if out.get(a).is_some_and(|n| n.selected)
            && let Some(slot) = fed.get_mut(b)
        {
            *slot = true;
        }
    }
    // A gate counts, like the game, only points spent ABOVE it (nodes with
    // a smaller reqPoints): points sunk below a gate can never hold that
    // gate open on their own.
    let above: Vec<u64> = reqs
        .iter()
        .map(|&req| {
            out.iter()
                .zip(&reqs)
                .filter(|(m, r)| m.selected && !m.granted && **r < req)
                .map(|(m, _)| m.ranks)
                .sum()
        })
        .collect();
    // A full pane (points at cap) has nothing further to offer.
    let full = cap.is_some_and(|c| points >= c);
    for (i, n) in out.iter_mut().enumerate() {
        let req = reqs.get(i).copied().unwrap_or(0);
        n.available = !n.selected
            && !full
            && above.get(i).copied().unwrap_or(0) >= req
            && (!has_incoming.get(i).copied().unwrap_or(false)
                || fed.get(i).copied().unwrap_or(false));
    }

    PaneModel {
        points,
        cap,
        requires: requires.to_string(),
        w: (max_x - min_x) / GRID * CELL + 2.0 * PAD,
        h: (max_y - min_y) / GRID * CELL + 2.0 * PAD,
        nodes: out,
        edges,
        retained: Retained::default(),
    }
}
