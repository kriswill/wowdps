//! The talent viewer: a window-local screen showing a player's talent tree
//! from the per-machine dataset (`talents.json`, R14) — the same file the
//! mcp talent tools read, through the same `wowdps_proto::talents` codec.
//!
//! Input is a paste: either a bare in-game import string, or a whole
//! SimulationCraft addon export (`simc.rs`), which also brings every saved
//! loadout, the equipped gear, the bag items and the currency lines. A
//! parsed simc paste is persisted per character under
//! `$XDG_DATA_HOME/wowdps/simc/`, so opening the viewer on a meter row
//! whose player has pasted before shows their build immediately.
//!
//! v19: opening on a meter row also asks the daemon for the player's
//! COMBATANT_INFO loadout (`GetLoadout`); when it lands, the logged build —
//! talents and equipped gear, the ones actually used in the watched fight —
//! wins over any stored paste (`adopt_logged`), with simc loadout chips one
//! click away. Logged builds are never persisted; the daemon re-answers on
//! every open.
//!
//! Rendering mirrors the game's three panes — class, spec, hero — split
//! the way the in-game frame does it: hero nodes carry `subTreeId`, and
//! the class/spec halves divide at the midpoint of the remaining nodes'
//! grid x. Node art comes from the spell-icon cache when present.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{Space, column, container, mouse_area, row, scrollable, text, text_input};
use iced::{Border, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme};

use wowdps_proto::json::Json;
use wowdps_proto::talents;

use crate::simc;
use crate::spell_icons::IconStyle;
use crate::talent_art;
use crate::view::{DIM, GREEN, RED, YELLOW};

/// The talent gold — selection frames, lit paths, the pane titles. Close
/// to the game's `ffd100` toned for the dark theme.
const GOLD: Color = Color::from_rgb(0.94, 0.78, 0.31);

/// Grid pitch: the dataset's node coordinates step by 600 per tree column.
const GRID: f32 = 600.0;
/// Canvas pixels per grid step, and the node tile drawn on it.
const CELL: f32 = 44.0;
const TILE: f32 = 28.0;
const PAD: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Talents,
    Inventory,
}

#[derive(Debug, Clone)]
pub(crate) enum Msg {
    /// The one-line input changed (a bare import string is typed/pasted).
    Input(String),
    /// Decode the input line.
    Submit,
    /// Read the system clipboard (how a multi-line simc export arrives —
    /// a single-line text_input would fold it).
    PasteClipboard,
    Clipboard(Option<String>),
    SelectLoadout(usize),
    SetTab(Tab),
    /// Tab key: flip between the talents and inventory tabs.
    ToggleTab,
    /// Left-click on a node: select / add a rank, or open the choice
    /// picker on an octagon node.
    NodeClick(u64),
    /// Right-click on a node: refund a rank / deselect.
    NodeRightClick(u64),
    /// A choice-picker option was clicked: (node id, entry index).
    PickChoice(u64, u64),
    /// A click landed outside the open picker.
    ClosePicker,
    /// The pointer entered a node (or, with an index, one of the open
    /// picker's option tiles) — the tab-wide overlay draws its tooltip
    /// (drawn per-pane it would clip at the pane's edge). Carries the
    /// tile's center in window coordinates: the tooltip anchors beside
    /// the icon instead of chasing the pointer.
    HoverSet(u64, Option<u64>, f32, f32),
    /// The pointer left the node (carries the id so a stale clear from
    /// one pane cannot cancel a fresh hover in another).
    HoverClear(u64),
    /// Encode the current (possibly edited) build and copy the string.
    CopyString,
    Close,
}

// ---- the decoded, layout-ready model ---------------------------------------

/// One node, positioned in canvas pixels within its pane.
#[derive(Debug, Clone)]
pub(crate) struct Node {
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
pub(crate) struct ChoiceOption {
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

/// The pane's retained canvas geometry: everything that depends only on
/// the model, tessellated once per rebuild and reused across the redraws
/// iced requests on every cursor movement over a canvas. A fresh (or
/// cloned) model starts empty and re-tessellates once.
#[derive(Default)]
pub(crate) struct PaneCaches {
    under: canvas::Cache,
    over: canvas::Cache,
}

impl std::fmt::Debug for PaneCaches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PaneCaches")
    }
}

impl Clone for PaneCaches {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PaneModel {
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
    pub caches: PaneCaches,
}

/// A decoded build (or a bare spec tree), ready to draw: the class pane on
/// the left, the spec pane on the right (the way the game and every build
/// site lay it out), and the picked hero tree between them.
#[derive(Debug, Clone)]
pub(crate) struct Build {
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
    fn panes(&self) -> impl Iterator<Item = &Rc<PaneModel>> {
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
pub(crate) struct Sel {
    ranks: u64,
    granted: bool,
    choice_index: Option<u64>,
}

/// The decode result reshaped for editing.
fn selections_from_decode(dec: &Json) -> (HashMap<u64, Sel>, Option<u64>, Vec<String>) {
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

fn arr(v: Option<&Json>) -> &[Json] {
    match v {
        Some(Json::Arr(items)) => items,
        _ => &[],
    }
}

fn get_u64(v: &Json, key: &str) -> Option<u64> {
    v.get(key).and_then(Json::as_u64)
}

fn get_str<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or("")
}

/// A decoded string, ready to edit: spec, selections, hero pick, warnings.
type Decoded = (u64, HashMap<u64, Sel>, Option<u64>, Vec<String>);

/// Decode a string into editable selections (the caller lays it out via
/// `rebuild`, which caches the spec's `tree_view`).
fn decode_build(string: &str) -> Result<Decoded, String> {
    let ds = talents::load()?;
    let dec = talents::decode(ds, string)?;
    let spec_id = get_u64(&dec, "spec_id").ok_or("decode returned no spec_id")?;
    let (sels, hero, warnings) = selections_from_decode(&dec);
    Ok((spec_id, sels, hero, warnings))
}

fn build_model(
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
        caches: PaneCaches::default(),
    }
}

// ---- persisted pastes ------------------------------------------------------

/// `$XDG_DATA_HOME/wowdps/simc/<character>.simc` — same per-machine home
/// as the icon caches; personal data, never in the repo. The key keeps the
/// whole "Name-Realm" (the combat log's own spelling): a bare name would
/// make same-named characters on different realms share one file, so the
/// viewer could restore a stranger's build — or overwrite the user's.
fn store_path(player: &str) -> Option<PathBuf> {
    let mut key: String = player
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if key.is_empty() {
        key.push('_');
    }
    wowdps_proto::talents::data_path(&format!("simc/{key}.simc"))
}

fn load_stored(player: &str) -> Option<String> {
    std::fs::read_to_string(store_path(player)?).ok()
}

/// Best-effort, like the config save: a failure costs recall, not data.
fn save_stored(player: &str, paste: &str) {
    let Some(path) = store_path(player) else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("wowdps-gui: cannot create {}: {e}", dir.display());
        return;
    }
    if let Err(e) = std::fs::write(&path, paste) {
        eprintln!("wowdps-gui: cannot save {}: {e}", path.display());
    }
}

// ---- the screen's state ----------------------------------------------------

pub(crate) struct TalentsUi {
    pub input: String,
    /// The meter row this was opened from, if any.
    pub player: Option<String>,
    pub profile: Option<simc::Profile>,
    pub loadout_sel: usize,
    pub tab: Tab,
    /// The editable source of truth: spec + selected nodes + hero pick.
    /// `build` is re-derived from it after every click.
    pub spec_id: Option<u64>,
    sels: HashMap<u64, Sel>,
    hero: Option<u64>,
    /// The spec's `tree_view` output, cached per spec id: it deep-clones
    /// every tooltip string and depends only on the spec, so per-click
    /// rebuilds must not re-run it.
    tree: Option<(u64, Json)>,
    /// Warnings from the last decoded string; shown until a fresh string
    /// is loaded.
    warnings: Vec<String>,
    pub build: Option<Build>,
    pub error: Option<String>,
    /// The choice node whose option picker is expanded.
    pub picker: Option<u64>,
    /// The node under the pointer (and the picker option's index when the
    /// pointer is on an option tile), for the tab-wide tooltip overlay.
    pub hover: Option<(u64, Option<u64>)>,
    /// The hovered tile's center, window coordinates — the tooltip's
    /// anchor.
    pub hover_at: (f32, f32),
    /// Any click changed the build since it was decoded.
    pub edited: bool,
    /// v19: the shown build came from the daemon's COMBATANT_INFO loadout —
    /// the player's actual logged picks, not a paste. Cleared the moment the
    /// user loads anything else (a paste, a simc loadout chip).
    pub logged: bool,
    /// v19: the logged equipped gear, for the inventory tab. Ids only — the
    /// log carries no item names.
    logged_gear: Option<Vec<wowdps_model::GearItem>>,
}

impl TalentsUi {
    /// Open, optionally on a player from the meter: a stored simc paste
    /// wins, else the spec id from the wire draws the empty tree.
    pub fn open(player: Option<(String, Option<u32>)>) -> Self {
        let mut ui = Self {
            input: String::new(),
            player: None,
            profile: None,
            loadout_sel: 0,
            tab: Tab::Talents,
            spec_id: None,
            sels: HashMap::new(),
            hero: None,
            tree: None,
            warnings: Vec::new(),
            build: None,
            error: None,
            picker: None,
            hover: None,
            hover_at: (0.0, 0.0),
            edited: false,
            logged: false,
            logged_gear: None,
        };
        if let Some((name, spec_id)) = player {
            ui.player = Some(name.clone());
            if let Some(text) = load_stored(&name) {
                ui.ingest(&text);
            } else if let Some(spec_id) = spec_id {
                ui.spec_id = Some(spec_id as u64);
                ui.rebuild();
            }
        }
        ui
    }

    /// Re-derive the laid-out panes from the selection state, through the
    /// per-spec `tree_view` cache.
    fn rebuild(&mut self) {
        let Some(spec_id) = self.spec_id else {
            return;
        };
        if self.tree.as_ref().map(|(id, _)| *id) != Some(spec_id) {
            match talents::load().and_then(|ds| talents::tree_view(ds, spec_id)) {
                Ok(tv) => self.tree = Some((spec_id, tv)),
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            }
        }
        let Some((_, tv)) = &self.tree else {
            return;
        };
        match build_model(tv, &self.sels, self.hero, self.warnings.clone()) {
            Ok(b) => self.build = Some(b),
            Err(e) => self.error = Some(e),
        }
    }

    /// Install a freshly decoded string as the editing state.
    fn adopt(&mut self, string: &str) {
        match decode_build(string) {
            Ok((spec_id, sels, hero, warnings)) => {
                self.spec_id = Some(spec_id);
                self.sels = sels;
                self.hero = hero;
                self.warnings = warnings;
                self.picker = None;
                self.edited = false;
                self.rebuild();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// v19: install the daemon's COMBATANT_INFO loadout — the build this
    /// player actually ran in the watched fight. It wins over whatever the
    /// viewer opened with (a stored simc paste stays one loadout-chip click
    /// away), and is never persisted: the daemon re-answers on every open.
    /// The picks round-trip through the real codec (`picks_to_selections` →
    /// `encode` → `adopt`), so validation, granted/hero handling and "copy
    /// string" all behave exactly as for a pasted build.
    pub fn adopt_logged(&mut self, l: &wowdps_model::Loadout) {
        let Some(spec_id) = l.spec_id.map(u64::from).or(self.spec_id) else {
            return;
        };
        let ds = match talents::load() {
            Ok(ds) => ds,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let picks: Vec<(u32, u32, u32)> = l
            .talents
            .iter()
            .map(|t| (t.node_id, t.entry_id, t.rank))
            .collect();
        let converted = match talents::picks_to_selections(ds, spec_id, &picks) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let sels = match converted.get("selections") {
            Some(Json::Arr(s)) => s.clone(),
            _ => Vec::new(),
        };
        let string = match talents::encode(ds, spec_id, &sels) {
            Ok(enc) => enc.get("string").and_then(Json::as_str).map(str::to_string),
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let Some(string) = string else {
            return;
        };
        // `adopt` reports failure only through `self.error` — a failed
        // round-trip (codec drift) must not hang the "from combat log"
        // badge and logged gear over whatever build was already showing.
        self.error = None;
        self.adopt(&string);
        if self.error.is_some() {
            return;
        }
        // Conversion warnings (skipped drift picks, clamped ranks) surface
        // with the decode's own; the model is rebuilt to carry them.
        if let Some(Json::Arr(ws)) = converted.get("warnings")
            && !ws.is_empty()
        {
            let extra = ws.iter().filter_map(Json::as_str).map(str::to_string);
            self.warnings.splice(0..0, extra);
            self.rebuild();
        }
        self.logged = true;
        self.logged_gear = (!l.gear.is_empty()).then(|| l.gear.clone());
    }

    /// Forget the logged build wholesale. `logged` and `logged_gear` must
    /// move together: the inventory chip gates on the gear, the content arm
    /// on the flag, and a half-cleared pair renders one tab's chrome over
    /// the other's content. Falls back to the talents tab when the simc
    /// profile has no inventory left to show.
    fn drop_logged(&mut self) {
        self.logged = false;
        self.logged_gear = None;
        let profile_has_inventory = self.profile.as_ref().is_some_and(|p| {
            !p.equipped.is_empty() || !p.bags.is_empty() || !p.currencies.is_empty()
        });
        if self.tab == Tab::Inventory && !profile_has_inventory {
            self.tab = Tab::Talents;
        }
    }

    /// The current (possibly edited) build as an import string.
    pub fn encode_current(&self) -> Option<String> {
        let spec_id = self.spec_id?;
        let ds = talents::load().ok()?;
        let sels: Vec<Json> = self
            .sels
            .iter()
            .map(|(id, s)| {
                let mut o = vec![
                    ("node_id".to_string(), Json::u64(*id)),
                    ("ranks".to_string(), Json::u64(s.ranks)),
                ];
                if s.granted {
                    o.push(("granted".to_string(), Json::Bool(true)));
                }
                if let Some(c) = s.choice_index {
                    o.push(("choice_index".to_string(), Json::u64(c)));
                }
                Json::Obj(o)
            })
            .collect();
        let enc = talents::encode(ds, spec_id, &sels).ok()?;
        enc.get("string").and_then(Json::as_str).map(str::to_string)
    }

    /// The node's current pane-local view, cloned out of the build.
    fn find_node(&self, id: u64) -> Option<Node> {
        self.build
            .as_ref()?
            .panes()
            .flat_map(|p| p.nodes.iter())
            .find(|n| n.id == id)
            .cloned()
    }

    /// Is the pane holding this node already at its point cap?
    fn pane_full(&self, id: u64) -> bool {
        self.build
            .iter()
            .flat_map(Build::panes)
            .find(|p| p.nodes.iter().any(|n| n.id == id))
            .is_some_and(|p| p.cap.is_some_and(|c| p.points >= c))
    }

    /// Left-click: pick / add a rank; octagons expand their option picker.
    fn click_node(&mut self, id: u64) {
        self.picker = None;
        let Some(node) = self.find_node(id) else {
            return;
        };
        if node.granted {
            return;
        }
        if node.choice {
            if node.available || node.selected {
                self.picker = Some(id);
            }
            return;
        }
        if node.selected {
            // Another rank costs another point: the pane cap gates it.
            if node.ranks < node.max_ranks
                && !self.pane_full(id)
                && let Some(s) = self.sels.get_mut(&id)
            {
                s.ranks += 1;
                self.edited = true;
                self.rebuild();
            }
        } else if node.available {
            self.sels.insert(
                id,
                Sel {
                    ranks: 1,
                    granted: false,
                    choice_index: None,
                },
            );
            self.edited = true;
            self.rebuild();
        }
    }

    /// Right-click: refund a rank; removing a mid-tree node cascades — every
    /// node left without a taken parent goes with it.
    fn unclick_node(&mut self, id: u64) {
        self.picker = None;
        let Some(node) = self.find_node(id) else {
            return;
        };
        if node.granted || !node.selected {
            return;
        }
        if node.ranks > 1 {
            if let Some(s) = self.sels.get_mut(&id) {
                s.ranks -= 1;
                self.edited = true;
                self.enforce_gates();
            }
            return;
        }
        self.sels.remove(&id);
        self.cascade_orphans(id);
        self.edited = true;
        self.enforce_gates();
    }

    /// After a refund, drop any selected node whose point gate is no longer
    /// met (counting, like the game, only points spent above the gate),
    /// cascading its orphans, to a fixpoint — so an edited build can never
    /// encode into a string the game rejects. Rebuilds as it goes; the
    /// final state is laid out on return.
    fn enforce_gates(&mut self) {
        loop {
            self.rebuild();
            let Some(build) = &self.build else {
                return;
            };
            let mut broke: Option<u64> = None;
            'panes: for pane in build.panes() {
                for n in &pane.nodes {
                    if !n.selected || n.granted || n.req == 0 {
                        continue;
                    }
                    let above: u64 = pane
                        .nodes
                        .iter()
                        .filter(|m| m.selected && !m.granted && m.req < n.req)
                        .map(|m| m.ranks)
                        .sum();
                    if above < n.req {
                        broke = Some(n.id);
                        break 'panes;
                    }
                }
            }
            match broke {
                Some(id) => {
                    self.sels.remove(&id);
                    self.cascade_orphans(id);
                }
                None => return,
            }
        }
    }

    /// Drop every selected node that lost its last taken parent, to a
    /// fixpoint, within the pane the removed node lived in.
    fn cascade_orphans(&mut self, removed: u64) {
        let Some(build) = &self.build else {
            return;
        };
        let Some(pane) = build
            .panes()
            .find(|p| p.nodes.iter().any(|n| n.id == removed))
        else {
            return;
        };
        let pane = Rc::clone(pane);
        loop {
            let mut dropped = false;
            for (i, n) in pane.nodes.iter().enumerate() {
                if n.granted || !self.sels.contains_key(&n.id) {
                    continue;
                }
                let mut has_parent_edge = false;
                let mut fed = false;
                for &(a, b) in &pane.edges {
                    if b != i {
                        continue;
                    }
                    has_parent_edge = true;
                    if pane
                        .nodes
                        .get(a)
                        .is_some_and(|p| self.sels.contains_key(&p.id))
                    {
                        fed = true;
                        break;
                    }
                }
                if has_parent_edge && !fed {
                    self.sels.remove(&n.id);
                    dropped = true;
                }
            }
            if !dropped {
                break;
            }
        }
    }

    /// A choice-picker option was clicked.
    fn pick_choice(&mut self, id: u64, index: u64) {
        self.picker = None;
        let Some(node) = self.find_node(id) else {
            return;
        };
        if node.granted || index as usize >= node.options.len().max(1) {
            return;
        }
        if !(node.selected || node.available) {
            return;
        }
        self.sels.insert(
            id,
            Sel {
                ranks: 1,
                granted: false,
                choice_index: Some(index),
            },
        );
        self.edited = true;
        self.rebuild();
    }

    /// A paste arrived (clipboard or the input line): a multi-line text is
    /// a simc export, one base64 word is an import string.
    fn ingest(&mut self, pasted: &str) {
        let pasted = pasted.trim();
        self.error = None;
        self.picker = None;
        // The user loaded something explicitly: the logged build steps aside
        // ENTIRELY — gear included, or the inventory chip would gate on gear
        // the content arm no longer shows.
        self.drop_logged();
        if pasted.is_empty() {
            self.error = Some("the clipboard is empty".to_string());
            return;
        }
        if simc::looks_like_profile(pasted) {
            match simc::parse(pasted) {
                Ok(profile) => {
                    if let Some(name) = profile.name.as_deref() {
                        // The realm-qualified key: the paste's own
                        // name-server pair, so realms never collide.
                        let key = match profile.server.as_deref() {
                            Some(server) => format!("{name}-{server}"),
                            None => name.to_string(),
                        };
                        save_stored(&key, pasted);
                        // The viewer may have been opened on a meter row
                        // whose realm spelling differs from the paste's
                        // `server` line; save under that name too, so the
                        // row's reopen finds it. Same character only — a
                        // paste for someone else must not shadow the row's
                        // player.
                        if let Some(player) = &self.player
                            && player.split('-').next().unwrap_or(player).to_lowercase()
                                == name.to_lowercase()
                            && store_path(player) != store_path(&key)
                        {
                            save_stored(player, pasted);
                        }
                        // Adopt the paste's character as the viewed player
                        // unless the viewer was opened on someone specific.
                        if self.player.is_none() {
                            self.player = Some(key);
                        }
                    }
                    self.profile = Some(profile);
                    self.loadout_sel = 0;
                    self.decode_selected();
                }
                Err(e) => self.error = Some(e),
            }
        } else {
            self.profile = None;
            self.loadout_sel = 0;
            self.adopt(pasted);
        }
    }

    fn decode_selected(&mut self) {
        let Some(string) = self
            .profile
            .as_ref()
            .and_then(|p| p.loadouts.get(self.loadout_sel))
            .map(|l| l.string.clone())
        else {
            self.error = Some("the paste carried no talent strings".to_string());
            return;
        };
        self.adopt(&string);
    }

    /// Everything except Close and PasteClipboard, which the window
    /// handles (one drops the screen, the other needs a clipboard task).
    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Input(s) => self.input = s,
            Msg::Submit => {
                let line = self.input.clone();
                self.ingest(&line);
            }
            Msg::Clipboard(Some(text)) => self.ingest(&text),
            Msg::Clipboard(None) => self.error = Some("the clipboard is empty".to_string()),
            Msg::SelectLoadout(i) => {
                self.loadout_sel = i;
                self.picker = None;
                // A simc chip click is an explicit load: it wins over the
                // logged build (gear included) until the viewer reopens.
                self.drop_logged();
                self.decode_selected();
            }
            Msg::SetTab(tab) => self.tab = tab,
            Msg::ToggleTab => {
                // Keyboard parity with the tab chips; inventory only exists
                // once a profile — or logged gear — is in.
                if self.profile.is_some() || self.logged_gear.is_some() {
                    self.tab = match self.tab {
                        Tab::Talents => Tab::Inventory,
                        Tab::Inventory => Tab::Talents,
                    };
                }
            }
            Msg::NodeClick(id) => self.click_node(id),
            Msg::NodeRightClick(id) => self.unclick_node(id),
            Msg::PickChoice(id, index) => self.pick_choice(id, index),
            Msg::ClosePicker => self.picker = None,
            Msg::HoverSet(id, option, x, y) => {
                self.hover = Some((id, option));
                self.hover_at = (x, y);
                // Hovering any other node dismisses an open picker, the
                // way the in-game popover behaves.
                if option.is_none() && self.picker.is_some() && self.picker != Some(id) {
                    self.picker = None;
                }
            }
            Msg::HoverClear(id) => {
                if self.hover.map(|(n, _)| n) == Some(id) {
                    self.hover = None;
                }
            }
            // CopyString needs a clipboard task; the window handles it.
            Msg::CopyString | Msg::PasteClipboard | Msg::Close => {}
        }
    }
}

// ---- rendering -------------------------------------------------------------

pub(crate) fn screen(ui: &TalentsUi) -> Element<'_, Msg> {
    let mut top = row![text("talents").size(16)]
        .spacing(8)
        .align_y(iced::Alignment::Center);
    if let Some(player) = &ui.player {
        top = top.push(
            text(player.split('-').next().unwrap_or(player).to_string())
                .size(14)
                .color(YELLOW),
        );
    }
    if let Some(b) = &ui.build {
        top = top.push(
            text(format!("{} — {}", b.class_name, b.spec_name))
                .size(12)
                .color(DIM),
        );
    }
    top = top
        .push(Space::new().width(Length::Fill))
        .push(mouse_area(text("✕").size(14).color(DIM)).on_press(Msg::Close));

    let input_line = row![
        text_input("paste an in-game talent string…", &ui.input)
            .on_input(Msg::Input)
            .on_submit(Msg::Submit)
            .size(13)
            .font(Font::MONOSPACE),
        chip("paste simc/string", false, Msg::PasteClipboard),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let mut body = column![top, input_line].spacing(8).height(Length::Fill);

    if let Some(p) = &ui.profile {
        body = body.push(identity_line(p));
        if p.loadouts.len() > 1 {
            let mut chips = row![text("loadouts").size(11).color(DIM)]
                .spacing(6)
                .align_y(iced::Alignment::Center);
            for (i, l) in p.loadouts.iter().enumerate() {
                chips = chips.push(chip(
                    if l.active { "active" } else { &l.name },
                    i == ui.loadout_sel,
                    Msg::SelectLoadout(i),
                ));
            }
            body = body.push(
                scrollable(chips).direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                )),
            );
        }
    }
    // v19: logged gear opens the inventory tab too, without any paste.
    let has_inventory =
        ui.profile.as_ref().is_some_and(|p| {
            !p.equipped.is_empty() || !p.bags.is_empty() || !p.currencies.is_empty()
        }) || ui.logged_gear.is_some();
    if has_inventory {
        body = body.push(
            row![
                chip("talents", ui.tab == Tab::Talents, Msg::SetTab(Tab::Talents)),
                chip(
                    "inventory",
                    ui.tab == Tab::Inventory,
                    Msg::SetTab(Tab::Inventory)
                ),
            ]
            .spacing(6),
        );
    }

    if let Some(e) = &ui.error {
        body = body.push(text(e.clone()).size(12).color(RED));
    }

    // While the logged build is showing, its gear is the inventory (the
    // fight's actual equipment); a simc profile's inventory returns with it.
    let content: Element<'_, Msg> = match (ui.tab, &ui.profile, &ui.logged_gear) {
        (Tab::Inventory, _, Some(gear)) if ui.logged => logged_inventory(gear),
        (Tab::Inventory, Some(p), _) => inventory(p),
        _ => talents_tab(ui),
    };
    body.push(content)
        .push(
            text("click picks (+1 rank) · right-click refunds · octagons open their option picker · esc closes · tab flips inventory")
                .size(11)
                .color(DIM),
        )
        .into()
}

fn talents_tab(ui: &TalentsUi) -> Element<'_, Msg> {
    let Some(b) = &ui.build else {
        return container(
            text("paste a talent string or a SimulationCraft export to see a build")
                .size(13)
                .color(DIM),
        )
        .height(Length::Fill)
        .into();
    };
    let mut col = column![].spacing(6).height(Length::Fill);
    for w in &b.warnings {
        col = col.push(text(format!("⚠ {w}")).size(11).color(YELLOW));
    }
    // A fixed-height provenance line (node details live in the hover
    // tooltip on the canvas — a strip that changed height with its content
    // used to shift the whole tree while hovering). "edited" marks a build
    // that no longer matches the decoded string.
    let mut provenance = row![
        text(format!("dataset build {}", b.dataset_build))
            .size(11)
            .color(DIM),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if ui.logged && !ui.edited {
        // v19: this is the build the player actually ran (COMBATANT_INFO).
        provenance = provenance.push(text("from combat log").size(11).color(GREEN));
    }
    if ui.edited {
        provenance = provenance.push(text("edited").size(11).color(YELLOW));
    }
    provenance = provenance.push(chip("copy string", false, Msg::CopyString));
    col = col.push(provenance);

    // Class pane | hero column | spec pane — the game's own arrangement,
    // centered over ONE full-width backdrop: the spec's whole background
    // painting (class art fading in from the left edge, spec art from the
    // right) spans the client, washed dark so the trees read on top.
    // A `Fill` width inside a horizontally-scrollable axis collapses to
    // the content's width, so centering needs the real viewport size:
    // responsive() centers when the trees fit and falls back to a
    // two-axis scroll when they don't.
    let hero_w = b
        .hero_pane
        .as_ref()
        .map_or(0.0f32, |p| p.w + 16.0)
        .max(if b.hero.is_some() { 168.0 } else { 0.0 });
    let content_w = b.class_pane.w + b.spec_pane.w + hero_w + 2.0 * 24.0 + 32.0;
    let picker = ui.picker;
    let trees = iced::widget::responsive(move |size| {
        let fits = size.width >= content_w;
        let panes = container(tree_row(b, picker)).padding(iced::Padding {
            top: 8.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        });
        if fits {
            scrollable(
                container(panes)
                    .width(Length::Fill)
                    .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            scrollable(panes)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    });

    // The tab-wide tooltip layer, above everything: a per-pane tooltip
    // would clip at its own canvas edge (the hero pane's did). With the
    // picker open, only its own option tiles carry tooltips.
    let tip = Canvas::new(TipOverlay {
        anchor: ui.hover_at,
        node: ui.hover.and_then(|(id, opt)| {
            let (node, requires) = b.panes().find_map(|p| {
                p.nodes
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| (n.clone(), p.requires.clone()))
            })?;
            match (ui.picker, opt) {
                (Some(p), Some(i)) if p == id => Some((option_view(&node, i as usize)?, requires)),
                (Some(_), _) => None,
                (None, _) => Some((node, requires)),
            }
        }),
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let area: Element<'_, Msg> = match talent_art::background(b.spec_id) {
        // The painting is drawn by a canvas, not an image widget: the
        // widget's ContentFit::Cover paints its overflow outside its own
        // bounds, while canvas geometry clips. The dark veil is a plain
        // container ABOVE it (vector inside the same canvas would
        // composite under the image).
        Some((bg, w, h)) => iced::widget::stack![
            Canvas::new(Backdrop { bg, w, h })
                .width(Length::Fill)
                .height(Length::Fill),
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_: &Theme| {
                container::Style {
                    background: Some(Color::from_rgba(0.02, 0.02, 0.04, 0.35).into()),
                    ..container::Style::default()
                }
            }),
            trees,
            tip,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        None => iced::widget::stack![trees, tip]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    };
    col.push(area).into()
}

/// The full-width backdrop: the spec's background painting, cover-fit and
/// clipped by the canvas.
struct Backdrop {
    bg: iced::widget::image::Handle,
    w: u16,
    h: u16,
}

impl canvas::Program<Msg> for Backdrop {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let (iw, ih) = (f32::from(self.w).max(1.0), f32::from(self.h).max(1.0));
        let scale = (w / iw).max(h / ih);
        let (dw, dh) = (iw * scale, ih * scale);
        frame.draw_image(
            Rectangle {
                x: (w - dw) / 2.0,
                y: (h - dh) / 2.0,
                width: dw,
                height: dh,
            },
            canvas::Image::new(self.bg.clone()),
        );
        vec![frame.into_geometry()]
    }
}

/// The model's `Class` for a spec id, for the crest lookup.
fn model_class(spec_id: u32) -> Option<wowdps_model::Class> {
    wowdps_model::Spec::from_id(spec_id).map(|s| s.class())
}

/// The three panes side by side: class, the hero column, spec.
fn tree_row(b: &Build, picker: Option<u64>) -> Element<'_, Msg> {
    let class_icon = model_class(b.spec_id).and_then(crate::icons::class_handle);
    let spec_icon = crate::icons::spec_handle(b.spec_id);
    let mut panes = row![].spacing(24).align_y(iced::Alignment::Start);
    panes = panes.push(
        column![
            pane_header(class_icon, &b.class_name, &b.class_pane),
            pane_canvas(Rc::clone(&b.class_pane), picker),
        ]
        .spacing(4),
    );
    if let Some((hero_id, hero_name)) = &b.hero {
        panes = panes.push(hero_column(
            *hero_id,
            hero_name,
            b.hero_pane.as_ref(),
            picker,
        ));
    }
    panes = panes.push(
        column![
            pane_header(spec_icon, &b.spec_name, &b.spec_pane),
            pane_canvas(Rc::clone(&b.spec_pane), picker),
        ]
        .spacing(4),
    );
    panes.into()
}

/// "12/34 pts" when the cap is known, else "12 pts"; gold once full.
fn points_label(points: u64, cap: Option<u64>) -> (String, Color) {
    match cap {
        Some(cap) => (
            format!("{points}/{cap} pts"),
            if points >= cap { GOLD } else { DIM },
        ),
        None => (format!("{points} pts"), DIM),
    }
}

/// A pane's header bar: its round icon, its name, its points spent.
fn pane_header(
    icon: Option<iced::widget::image::Handle>,
    name: &str,
    pane: &PaneModel,
) -> Element<'static, Msg> {
    let mut line = row![].spacing(8).align_y(iced::Alignment::Center);
    if let Some(h) = icon {
        line = line.push(
            iced::widget::image(h)
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0)),
        );
    }
    let (label, color) = points_label(pane.points, pane.cap);
    line.push(text(name.to_uppercase()).size(13).color(GOLD))
        .push(Space::new().width(Length::Fill))
        .push(text(label).size(11).color(color).font(Font::MONOSPACE))
        .width(Length::Fixed(pane.w.max(160.0)))
        .into()
}

/// The center column: the hero tree's medallion under the game's golden
/// ring, its name, and its mini-tree on a dark backplate.
fn hero_column(
    hero_id: u32,
    hero_name: &str,
    pane: Option<&Rc<PaneModel>>,
    picker: Option<u64>,
) -> Element<'static, Msg> {
    // Measured off the ring crop's pixels: within its 192px tile (mostly
    // drop-shadow padding) the gold circle's inner diameter is ~55% — the
    // full-bleed medallion art must shrink to sit inside it.
    const RING: f32 = 168.0;
    const MEDALLION: f32 = RING * 0.56;
    let mut col = column![].spacing(6).align_x(iced::Alignment::Center);
    if let Some(art) = talent_art::medallion(hero_id) {
        let medallion = container(
            iced::widget::image(art)
                .width(Length::Fixed(MEDALLION))
                .height(Length::Fixed(MEDALLION)),
        )
        .width(Length::Fixed(RING))
        .height(Length::Fixed(RING))
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center);
        col = col.push(match talent_art::ring() {
            Some(ring) => Element::from(iced::widget::stack![
                medallion,
                iced::widget::image(ring)
                    .width(Length::Fixed(RING))
                    .height(Length::Fixed(RING)),
            ]),
            None => medallion.into(),
        });
    }
    col = col.push(text(hero_name.to_uppercase()).size(14).color(GOLD));
    if let Some(pane) = pane {
        let (label, color) = points_label(pane.points, pane.cap);
        col = col.push(text(label).size(11).color(color).font(Font::MONOSPACE));
        col = col.push(
            container(pane_canvas(Rc::clone(pane), picker))
                .padding(8)
                .style(|_: &Theme| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                    border: Border {
                        color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
                        width: 1.0,
                        radius: 10.into(),
                    },
                    ..container::Style::default()
                }),
        );
    }
    col.into()
}

fn identity_line(p: &simc::Profile) -> Element<'static, Msg> {
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
    text(parts.join(" · ")).size(11).color(DIM).into()
}

/// A small clickable pill, the loadout picker's and the tabs' unit.
fn chip(label: &str, selected: bool, msg: Msg) -> Element<'static, Msg> {
    let color = if selected { Color::WHITE } else { DIM };
    mouse_area(
        container(text(label.to_string()).size(11).color(color))
            .padding([3, 8])
            .style(move |_: &Theme| container::Style {
                background: Some(
                    Color::from_rgba(1.0, 1.0, 1.0, if selected { 0.12 } else { 0.05 }).into(),
                ),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, if selected { 0.4 } else { 0.15 }),
                    width: 1.0,
                    radius: 8.into(),
                },
                ..container::Style::default()
            }),
    )
    .on_press(msg)
    .into()
}

// ---- the inventory tab -----------------------------------------------------

fn inventory(p: &simc::Profile) -> Element<'static, Msg> {
    let mut col = column![].spacing(4);
    let section = |t: &'static str| text(t).size(12).color(YELLOW);
    if !p.equipped.is_empty() {
        col = col.push(section("equipped"));
        for i in &p.equipped {
            col = col.push(item_row(i));
        }
    }
    if !p.bags.is_empty() {
        col = col.push(Space::new().height(6)).push(section("in bags"));
        for i in &p.bags {
            col = col.push(item_row(i));
        }
    }
    if !p.currencies.is_empty() {
        col = col.push(Space::new().height(6)).push(section("currencies"));
        for c in &p.currencies {
            let kind = match (c.is_currency, c.catalyst) {
                (true, true) => "catalyst",
                (true, false) => "currency",
                (false, _) => "item",
            };
            col = col.push(
                row![
                    text(kind).size(11).color(DIM).width(Length::Fixed(70.0)),
                    text(format!("{}", c.id))
                        .size(12)
                        .font(Font::MONOSPACE)
                        .width(Length::Fixed(80.0)),
                    text(format!("× {}", c.amount))
                        .size(12)
                        .font(Font::MONOSPACE)
                        .color(Color::WHITE),
                ]
                .spacing(8),
            );
        }
    }
    scrollable(crate::view::scroll_clear(col))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

/// v19: COMBATANT_INFO's equippedItems dump is positional — the standard
/// inventory-slot order. Labels apply only when the count fits the table;
/// an unexpected shape falls back to unlabeled rows rather than lying.
const GEAR_SLOTS: [&str; 18] = [
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

/// v19: the logged gear list. Ids only — item names live in game data no
/// client-side dataset carries, so `item {id}` is the honest label, exactly
/// like the simc tab's fallback and the currencies section.
fn logged_inventory(gear: &[wowdps_model::GearItem]) -> Element<'static, Msg> {
    let mut col = column![].spacing(4);
    col = col.push(text("equipped — from combat log").size(12).color(YELLOW));
    let labeled = gear.len() <= GEAR_SLOTS.len();
    for (i, g) in gear.iter().enumerate() {
        // Empty slots log as zeroed tuples; a row of zeros says nothing.
        if g.item_id == 0 {
            continue;
        }
        let slot = if labeled {
            GEAR_SLOTS.get(i).copied().unwrap_or("")
        } else {
            ""
        };
        let mut extras: Vec<String> = Vec::new();
        if !g.enchants.is_empty() {
            extras.push("enchanted".to_string());
        }
        if !g.gems.is_empty() {
            extras.push(format!(
                "{} gem{}",
                g.gems.len(),
                if g.gems.len() == 1 { "" } else { "s" }
            ));
        }
        col = col.push(
            row![
                text(slot)
                    .size(11)
                    .color(DIM)
                    .font(Font::MONOSPACE)
                    .width(Length::Fixed(80.0)),
                text(format!("item {}", g.item_id))
                    .size(12)
                    .font(Font::MONOSPACE)
                    .width(Length::Fill),
                text(extras.join(" · ")).size(10).color(DIM),
                text(if g.ilvl > 0 {
                    g.ilvl.to_string()
                } else {
                    String::new()
                })
                .size(12)
                .color(GREEN)
                .font(Font::MONOSPACE)
                .width(Length::Fixed(36.0))
                .align_x(iced::Alignment::End),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }
    scrollable(crate::view::scroll_clear(col))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn item_row(i: &simc::Item) -> Element<'static, Msg> {
    let name = i.name.clone().unwrap_or_else(|| format!("item {}", i.id));
    let ilvl = i.ilvl.map(|v| v.to_string()).unwrap_or_default();
    let mut extras: Vec<String> = Vec::new();
    if i.enchant_id.is_some() {
        extras.push("enchanted".to_string());
    }
    if !i.gem_ids.is_empty() {
        extras.push(format!(
            "{} gem{}",
            i.gem_ids.len(),
            if i.gem_ids.len() == 1 { "" } else { "s" }
        ));
    }
    row![
        text(i.slot.clone())
            .size(11)
            .color(DIM)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(80.0)),
        text(name).size(12).width(Length::Fill),
        text(extras.join(" · ")).size(10).color(DIM),
        text(ilvl)
            .size(12)
            .color(GREEN)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(36.0))
            .align_x(iced::Alignment::End),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

// ---- the tree canvas -------------------------------------------------------

/// One tree canvas. It draws no background of its own: the whole tab sits
/// on the spec's full-width painting (see `talents_tab`) — a canvas frame
/// composites all of its images above all of its vector paths, so the
/// painting can never live inside the canvas without burying the edges
/// and frames.
/// A pane: two stacked canvases over one model. The LOWER canvas draws the
/// edges, arrows, shaped backings and the icon art; the UPPER draws
/// everything that must read over the icons — frames, carets, the
/// white-on-black rank badges, the choice picker and the hover tooltip —
/// and owns the mouse. Two canvases because a single frame composites all
/// of its images above all of its vector paths, which would bury any
/// chrome overlapping a tile.
fn pane_canvas(model: Rc<PaneModel>, picker: Option<u64>) -> Element<'static, Msg> {
    let (w, h) = (model.w, model.h);
    iced::widget::stack![
        Canvas::new(PaneUnder {
            model: Rc::clone(&model),
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h)),
        Canvas::new(PaneOver { model, picker })
            .width(Length::Fixed(w))
            .height(Length::Fixed(h)),
    ]
    .into()
}

fn node_at(model: &PaneModel, pos: Point) -> Option<&Node> {
    model
        .nodes
        .iter()
        .min_by(|a, b| {
            let d = |n: &Node| (n.x - pos.x).hypot(n.y - pos.y);
            d(a).total_cmp(&d(b))
        })
        .filter(|n| (n.x - pos.x).hypot(n.y - pos.y) <= TILE / 2.0 + 3.0)
}

struct PaneUnder {
    model: Rc<PaneModel>,
}

impl canvas::Program<Msg> for PaneUnder {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // Everything here depends only on the model, so the tessellation is
        // cached on it: iced redraws a canvas on every cursor movement, and
        // without the cache each redraw re-tessellated every edge and tile.
        let geometry = self
            .model
            .caches
            .under
            .draw(renderer, bounds.size(), |frame| {
                // Edges under the tiles: a taken path is gold and carries an
                // arrowhead at its destination end, pointing into the node the
                // point flowed to (the way the game draws its paths); the rest
                // stay faint gray.
                for &(a, b) in &self.model.edges {
                    let (Some(from), Some(to)) = (self.model.nodes.get(a), self.model.nodes.get(b))
                    else {
                        continue;
                    };
                    let lit = from.selected && to.selected;
                    let (p, q) = (Point::new(from.x, from.y), Point::new(to.x, to.y));
                    frame.stroke(
                        &Path::line(p, q),
                        Stroke::default()
                            .with_width(if lit { 2.0 } else { 1.5 })
                            .with_color(if lit {
                                Color { a: 0.85, ..GOLD }
                            } else {
                                Color::from_rgba(0.75, 0.78, 0.85, 0.22)
                            }),
                    );
                    if lit {
                        let (dx, dy) = (q.x - p.x, q.y - p.y);
                        let len = dx.hypot(dy).max(1.0);
                        let (ux, uy) = (dx / len, dy / len);
                        // Tip just outside the destination tile's frame.
                        let tip = Point::new(
                            q.x - ux * (TILE / 2.0 + 2.5),
                            q.y - uy * (TILE / 2.0 + 2.5),
                        );
                        let base = Point::new(tip.x - ux * 7.0, tip.y - uy * 7.0);
                        let arrow = Path::new(|b| {
                            b.move_to(tip);
                            b.line_to(Point::new(base.x - uy * 4.5, base.y + ux * 4.5));
                            b.line_to(Point::new(base.x + uy * 4.5, base.y - ux * 4.5));
                            b.close();
                        });
                        frame.fill(&arrow, GOLD);
                    }
                }

                for n in &self.model.nodes {
                    let center = Point::new(n.x, n.y);
                    let rect = Rectangle {
                        x: n.x - TILE / 2.0,
                        y: n.y - TILE / 2.0,
                        width: TILE,
                        height: TILE,
                    };
                    // A dark backing so the shaped icon's clipped corners read as
                    // the shape even over bright background art.
                    frame.fill(
                        &shape_path(center, TILE / 2.0 + 1.5, n.shape),
                        Color::from_rgba(0.0, 0.0, 0.0, 0.60),
                    );
                    // Colored art for anything the build has; untaken talents are
                    // desaturated and dimmed, the way every talent UI mutes them.
                    match crate::spell_icons::styled(n.spell_id, n.shape, !n.selected) {
                        Some(icon) => frame.draw_image(rect, canvas::Image::new(icon)),
                        None => frame.fill(
                            &shape_path(center, TILE / 2.0 - 2.0, n.shape),
                            Color::from_rgba(1.0, 1.0, 1.0, if n.selected { 0.30 } else { 0.10 }),
                        ),
                    }
                }
            });
        vec![geometry]
    }
}

/// Option tiles of the expanded choice picker: a horizontal strip through
/// the node, clamped inside the pane.
fn picker_spots(model: &PaneModel, node: &Node) -> Vec<Point> {
    const STEP: f32 = 40.0;
    let k = node.options.len().max(1);
    let total = k as f32 * STEP;
    let mut x0 = node.x - total / 2.0 + STEP / 2.0;
    x0 = x0.max(TILE / 2.0 + 6.0);
    x0 = x0.min(model.w - total + STEP / 2.0 - TILE / 2.0 - 6.0);
    (0..k)
        .map(|i| Point::new(x0 + i as f32 * STEP, node.y))
        .collect()
}

struct PaneOver {
    model: Rc<PaneModel>,
    /// The choice node whose picker is expanded (any pane's — only the one
    /// that actually holds the node draws it).
    picker: Option<u64>,
}

#[derive(Default)]
struct OverState {
    /// (node id, picker-option index) under the pointer.
    hover: Option<(u64, Option<u64>)>,
}

impl PaneOver {
    fn picker_node(&self) -> Option<&Node> {
        let id = self.picker?;
        self.model.nodes.iter().find(|n| n.id == id)
    }

    /// The picker option index under the cursor, if the picker is open in
    /// this pane.
    fn option_at(&self, pos: Point) -> Option<(u64, u64)> {
        let node = self.picker_node()?;
        picker_spots(&self.model, node)
            .iter()
            .position(|c| (c.x - pos.x).hypot(c.y - pos.y) <= TILE / 2.0 + 4.0)
            .map(|i| (node.id, i as u64))
    }
}

impl canvas::Program<Msg> for PaneOver {
    type State = OverState;

    fn update(
        &self,
        state: &mut OverState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        use iced::mouse::{Button, Event as Mouse};
        let iced::Event::Mouse(mouse) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds);
        match mouse {
            Mouse::CursorMoved { .. } => {
                // Picker option tiles sit over neighboring nodes: they win.
                // The tile center rides along in WINDOW coordinates so the
                // tooltip can anchor beside the icon.
                let over = pos
                    .and_then(|p| self.option_at(p))
                    .and_then(|(node, i)| {
                        let n = self.picker_node()?;
                        let c = picker_spots(&self.model, n).get(i as usize).copied()?;
                        Some((node, Some(i), c))
                    })
                    .or_else(|| {
                        pos.and_then(|p| node_at(&self.model, p))
                            .map(|n| (n.id, None, Point::new(n.x, n.y)))
                    });
                let over_key = over.map(|(id, opt, _)| (id, opt));
                if over_key != state.hover {
                    let prev = state.hover;
                    state.hover = over_key;
                    // The tab-wide overlay draws the tooltip; tell it.
                    return Some(canvas::Action::publish(match (over, prev) {
                        (Some((id, opt, c)), _) => {
                            Msg::HoverSet(id, opt, bounds.x + c.x, bounds.y + c.y)
                        }
                        (None, Some((id, _))) => Msg::HoverClear(id),
                        (None, None) => return None,
                    }));
                }
                None
            }
            Mouse::ButtonPressed(Button::Left) => {
                let p = pos?;
                if let Some((node, index)) = self.option_at(p) {
                    return Some(
                        canvas::Action::publish(Msg::PickChoice(node, index)).and_capture(),
                    );
                }
                if let Some(n) = node_at(&self.model, p) {
                    return Some(canvas::Action::publish(Msg::NodeClick(n.id)).and_capture());
                }
                if self.picker.is_some() {
                    return Some(canvas::Action::publish(Msg::ClosePicker).and_capture());
                }
                None
            }
            Mouse::ButtonPressed(Button::Right) => {
                let p = pos?;
                let n = node_at(&self.model, p)?;
                Some(canvas::Action::publish(Msg::NodeRightClick(n.id)).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        _state: &OverState,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(p) if self.option_at(p).is_some() || node_at(&self.model, p).is_some() => {
                iced::mouse::Interaction::Pointer
            }
            _ => iced::mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        state: &OverState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        const GREEN_OUTLINE: Color = Color::from_rgb(0.30, 0.85, 0.35);

        // The model-only chrome — frames, carets, rank badges — caches on
        // the model; the hover ring and the open picker change without a
        // rebuild, so they draw on a fresh frame each time.
        let chrome = self
            .model
            .caches
            .over
            .draw(renderer, bounds.size(), |frame| {
                for n in &self.model.nodes {
                    let center = Point::new(n.x, n.y);
                    let frame_path = shape_path(center, TILE / 2.0 + 1.5, n.shape);
                    // The frame: gold = taken, teal = granted for free, green =
                    // available to pick, faint gray = out of reach.
                    let border = if n.granted {
                        Color::from_rgb(0.35, 0.80, 0.75)
                    } else if n.selected {
                        GOLD
                    } else if n.available {
                        GREEN_OUTLINE
                    } else {
                        Color::from_rgba(0.72, 0.74, 0.80, 0.35)
                    };
                    frame.stroke(
                        &frame_path,
                        Stroke::default()
                            .with_width(if n.selected || n.available { 2.0 } else { 1.0 })
                            .with_color(border),
                    );
                    // A choice node wears the game's side carets.
                    if n.choice {
                        for side in [-1.0f32, 1.0] {
                            let bx = n.x + side * (TILE / 2.0 + 3.0);
                            let caret = Path::new(|b| {
                                b.move_to(Point::new(bx + side * 4.0, n.y));
                                b.line_to(Point::new(bx, n.y - 4.0));
                                b.line_to(Point::new(bx, n.y + 4.0));
                                b.close();
                            });
                            frame.fill(
                                &caret,
                                Color {
                                    a: if n.selected { 1.0 } else { 0.35 },
                                    ..border
                                },
                            );
                        }
                    }
                    // The rank badge: white on black, overlapping the tile's lower
                    // right corner so it never covers the path lines. Every node
                    // wears one (0/1 included), like the game's editor.
                    let content = format!("{}/{}", n.ranks, n.max_ranks);
                    let bw = content.len() as f32 * 6.0 + 6.0;
                    let badge = Rectangle {
                        x: n.x + TILE / 2.0 + 5.0 - bw,
                        y: n.y + TILE / 2.0 - 7.0,
                        width: bw,
                        height: 13.0,
                    };
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(badge.x, badge.y),
                            Size::new(badge.width, badge.height),
                            2.0.into(),
                        ),
                        Color::from_rgba(0.0, 0.0, 0.0, 0.88),
                    );
                    frame.fill_text(canvas::Text {
                        content,
                        position: Point::new(
                            badge.x + badge.width / 2.0,
                            badge.y + badge.height / 2.0,
                        ),
                        color: Color::WHITE,
                        size: 9.0.into(),
                        font: Font::MONOSPACE,
                        align_x: iced::alignment::Horizontal::Center.into(),
                        align_y: iced::alignment::Vertical::Center,
                        ..canvas::Text::default()
                    });
                }
            });

        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some((id, None)) = state.hover
            && let Some(n) = self.model.nodes.iter().find(|n| n.id == id)
        {
            frame.stroke(
                &shape_path(Point::new(n.x, n.y), TILE / 2.0 + 4.0, n.shape),
                Stroke::default().with_width(1.5).with_color(Color::WHITE),
            );
        }

        // The expanded choice picker: a horizontal strip of the options
        // through the node, current pick ringed gold.
        if let Some(node) = self.picker_node() {
            let spots = picker_spots(&self.model, node);
            if let (Some(first), Some(last)) = (spots.first(), spots.last()) {
                let pad = TILE / 2.0 + 8.0;
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(first.x - pad, node.y - pad),
                        Size::new(last.x - first.x + 2.0 * pad, 2.0 * pad),
                        8.0.into(),
                    ),
                    Color::from_rgba(0.04, 0.04, 0.08, 0.96),
                );
            }
            for (i, (c, opt)) in spots.iter().zip(node.options.iter()).enumerate() {
                let rect = Rectangle {
                    x: c.x - TILE / 2.0,
                    y: c.y - TILE / 2.0,
                    width: TILE,
                    height: TILE,
                };
                match crate::spell_icons::styled(opt.spell_id, IconStyle::Octagon, false) {
                    Some(icon) => frame.draw_image(rect, canvas::Image::new(icon)),
                    None => frame.fill(
                        &shape_path(*c, TILE / 2.0 - 2.0, IconStyle::Octagon),
                        Color::from_rgba(1.0, 1.0, 1.0, 0.25),
                    ),
                }
                // Ring outside the tile (vector composites under images,
                // so it must not overlap the icon). The node's spell_id is
                // its picked entry's, which marks the current option; the
                // hovered option flares white.
                let is_current = node.selected && node.spell_id == opt.spell_id;
                let hovered = state.hover == Some((node.id, Some(i as u64)));
                frame.stroke(
                    &shape_path(*c, TILE / 2.0 + 2.5, IconStyle::Octagon),
                    Stroke::default().with_width(2.0).with_color(if hovered {
                        Color::WHITE
                    } else if is_current {
                        GOLD
                    } else {
                        Color::from_rgba(0.85, 0.87, 0.92, 0.55)
                    }),
                );
            }
        }

        vec![chrome, frame.into_geometry()]
    }
}

/// The node reshaped as one of its choice options, so the picker's option
/// tiles get full tooltips of their own.
fn option_view(node: &Node, index: usize) -> Option<Node> {
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

/// The tab-wide tooltip layer: sits above the scrollable so the tooltip
/// can never be clipped by a pane's own canvas. The tooltip is anchored
/// beside the hovered icon (not the pointer), so the neighbors stay
/// visible while reading. It never captures events — clicks and scrolls
/// fall through to the trees below.
struct TipOverlay {
    /// The hovered node with its pane's "Requires …" class, if any.
    node: Option<(Node, String)>,
    /// The hovered tile's center in window coordinates.
    anchor: (f32, f32),
}

impl canvas::Program<Msg> for TipOverlay {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some((n, requires)) = &self.node {
            let anchor = Point::new(self.anchor.0 - bounds.x, self.anchor.1 - bounds.y);
            draw_tooltip(&mut frame, n, requires, anchor, bounds.width, bounds.height);
        }
        vec![frame.into_geometry()]
    }
}

/// One tooltip line: text, color, size, and whether a second right-aligned
/// text shares the row (the cost/range line).
struct TipLine {
    text: String,
    right: Option<String>,
    color: Color,
    size: f32,
}

/// The game-style tooltip: title, "Talent", cost + range, cast time,
/// "Requires <class>", then the description — yellow, with any trailing
/// restriction paragraph in blue, exactly how the game colors them.
fn draw_tooltip(frame: &mut canvas::Frame, n: &Node, requires: &str, cur: Point, w: f32, h: f32) {
    const TIP_W: f32 = 300.0;
    const PAD_X: f32 = 9.0;
    let tw = TIP_W.min(w - 8.0).max(180.0);
    // Wrap budget from the box width (canvas text has no layout engine);
    // ~5.6px per character at 11px.
    let budget = ((tw - 2.0 * PAD_X) / 5.6) as usize;
    let gray = Color::from_rgb(0.62, 0.64, 0.70);
    let yellow = Color::from_rgb(1.0, 0.84, 0.25);
    let blue = Color::from_rgb(0.42, 0.62, 1.0);

    let mut lines: Vec<TipLine> = Vec::new();
    let mut push = |text: String, right: Option<String>, color: Color, size: f32| {
        lines.push(TipLine {
            text,
            right,
            color,
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
        Color::WHITE,
        13.0,
    );
    push("Talent".to_string(), None, gray, 10.0);
    match (n.cost.is_empty(), n.range.is_empty()) {
        (false, false) => push(n.cost.clone(), Some(n.range.clone()), Color::WHITE, 11.0),
        (false, true) => push(n.cost.clone(), None, Color::WHITE, 11.0),
        (true, false) => push(n.range.clone(), None, Color::WHITE, 11.0),
        (true, true) => {}
    }
    if !n.cast.is_empty() {
        push(n.cast.clone(), None, Color::WHITE, 11.0);
    }
    if !requires.is_empty() {
        push(format!("Requires {requires}"), None, Color::WHITE, 11.0);
    }
    push(
        format!("Rank {}/{}", n.ranks, n.max_ranks),
        None,
        gray,
        10.0,
    );
    // Unpicked ranks read dim, the way the game greys them out.
    let dim = Color::from_rgb(0.55, 0.58, 0.64);
    let rank_paras =
        |text: &str,
         prefix: Option<String>,
         color: Color,
         push: &mut dyn FnMut(String, Option<String>, Color, f32)| {
            for (pi, para) in text.split("\n\n").enumerate() {
                if pi > 0 {
                    push(String::new(), None, Color::WHITE, 3.0);
                }
                let para = para.replace('\n', " ");
                let text = match (&prefix, pi) {
                    (Some(p), 0) => format!("{p}{para}"),
                    _ => para,
                };
                for line in wrap_text(&text, budget) {
                    push(line, None, color, 11.0);
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
            push(String::new(), None, Color::WHITE, 4.0);
            push(format!("Rank {}", ti + 1), None, Color::WHITE, 11.0);
            if tier.max_ranks > 1 && tier.desc_ranks.len() >= tier.max_ranks as usize {
                for k in 1..=tier.max_ranks {
                    if k > 1 {
                        push(String::new(), None, Color::WHITE, 3.0);
                    }
                    let color = if cum + k <= n.ranks { yellow } else { dim };
                    if let Some(text) = tier.desc_ranks.get((k - 1) as usize) {
                        rank_paras(text, Some(format!("({k}): ")), color, &mut push);
                    }
                }
            } else {
                let color = if cum < n.ranks { yellow } else { dim };
                rank_paras(&tier.desc, None, color, &mut push);
            }
            cum += tier.max_ranks;
        }
    } else if n.max_ranks > 1 && n.desc_ranks.len() >= n.max_ranks as usize {
        // A plain multi-rank talent: every rank's values, unreached ranks
        // greyed.
        for k in 1..=n.max_ranks {
            push(String::new(), None, Color::WHITE, 4.0);
            let color = if k <= n.ranks { yellow } else { dim };
            if let Some(text) = n.desc_ranks.get((k - 1) as usize) {
                rank_paras(text, Some(format!("({k}): ")), color, &mut push);
            }
        }
    } else if !n.desc.is_empty() {
        // Paragraphs split on blank lines; the trailing restriction
        // paragraph(s) — "Curses: …" — go blue like the game's.
        let paras: Vec<&str> = n.desc.split("\n\n").collect();
        let n_paras = paras.len();
        for (pi, para) in paras.into_iter().enumerate() {
            push(String::new(), None, Color::WHITE, 4.0); // paragraph gap
            let color = if pi + 1 == n_paras && n_paras > 1 {
                blue
            } else {
                yellow
            };
            for line in wrap_text(&para.replace('\n', " "), budget) {
                push(line, None, color, 11.0);
            }
        }
    }
    if n.choice && n.options.len() > 1 {
        push(String::new(), None, Color::WHITE, 4.0);
        let names: Vec<&str> = n.options.iter().map(|o| o.name.as_str()).collect();
        for line in wrap_text(&names.join(" / "), budget) {
            push(line, None, gray, 10.0);
        }
    }

    let th: f32 = lines.iter().map(|l| l.size + 4.0).sum::<f32>() + 12.0;
    // Anchored beside the icon, top edge a little above it (the in-game
    // placement): the hovered talent's neighbors stay visible. Flip to
    // the left when the right side has no room.
    let mut at = Point::new(cur.x + TILE / 2.0 + 12.0, cur.y - TILE / 2.0 - 8.0);
    if at.x + tw > w - 2.0 {
        at.x = (cur.x - TILE / 2.0 - 12.0 - tw).max(2.0);
    }
    at.y = at.y.clamp(2.0, (h - th - 2.0).max(2.0));
    frame.fill(
        &Path::rounded_rectangle(at, Size::new(tw, th), 4.0.into()),
        Color::from_rgba(0.02, 0.03, 0.12, 0.97),
    );
    frame.stroke(
        &Path::rounded_rectangle(at, Size::new(tw, th), 4.0.into()),
        Stroke::default()
            .with_width(1.0)
            .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.30)),
    );
    let mut y = at.y + 7.0;
    for line in &lines {
        if !line.text.is_empty() {
            frame.fill_text(canvas::Text {
                content: line.text.clone(),
                position: Point::new(at.x + PAD_X, y),
                color: line.color,
                size: line.size.into(),
                align_x: iced::alignment::Horizontal::Left.into(),
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
        if let Some(right) = &line.right {
            frame.fill_text(canvas::Text {
                content: right.clone(),
                position: Point::new(at.x + tw - PAD_X, y),
                color: line.color,
                size: line.size.into(),
                align_x: iced::alignment::Horizontal::Right.into(),
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
        y += line.size + 4.0;
    }
}

/// Greedy word wrap at a character budget (canvas text has no layout).
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
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

/// The outline for a node shape, centered at `c` with "radius" `r`
/// (half-extent for the square and octagon).
fn shape_path(c: Point, r: f32, shape: IconStyle) -> Path {
    match shape {
        IconStyle::Circle => Path::circle(c, r),
        IconStyle::Square => {
            Path::rectangle(Point::new(c.x - r, c.y - r), Size::new(2.0 * r, 2.0 * r))
        }
        IconStyle::Octagon => Path::new(|b| {
            // Corner cut matching the icon mask (29% of the tile edge).
            let k = 2.0 * r * 0.29;
            b.move_to(Point::new(c.x - r + k, c.y - r));
            b.line_to(Point::new(c.x + r - k, c.y - r));
            b.line_to(Point::new(c.x + r, c.y - r + k));
            b.line_to(Point::new(c.x + r, c.y + r - k));
            b.line_to(Point::new(c.x + r - k, c.y + r));
            b.line_to(Point::new(c.x - r + k, c.y + r));
            b.line_to(Point::new(c.x - r, c.y + r - k));
            b.line_to(Point::new(c.x - r, c.y - r + k));
            b.close();
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mcp codec's synthetic two-spec dataset, reduced: node 1 and 2 in
    /// the class half, 3 in the spec half, 4 the subtree selector, 5 a hero
    /// node in tree 77.
    fn dataset() -> Json {
        wowdps_proto::json::parse(
            r#"{
              "build": "12.1.0.69497",
              "trees": [{
                "treeId": 10, "classId": 8, "className": "Mage",
                "specs": [{"specId": 62, "name": "Arcane", "role": 2}],
                "currencies": [],
                "subTrees": [{"id": 77, "name": "Sunfury", "specs": [62]}],
                "nodeOrder": [1, 2, 3, 4, 5],
                "nodes": [
                  {"id": 1, "type": "single", "posX": 0, "posY": 0, "maxRanks": 2,
                   "next": [2],
                   "entries": [{"id": 101, "spellId": 1001, "name": "Filler", "maxRanks": 2}]},
                  {"id": 2, "type": "choice", "posX": 0, "posY": 600, "maxRanks": 1,
                   "reqPoints": 2,
                   "entries": [{"id": 131, "spellId": 1031, "name": "Left", "maxRanks": 1},
                               {"id": 132, "spellId": 1032, "name": "Right", "maxRanks": 1}]},
                  {"id": 3, "type": "single", "posX": 3000, "posY": 0, "maxRanks": 1,
                   "entries": [{"id": 104, "spellId": 1004, "name": "Gated", "maxRanks": 1}]},
                  {"id": 4, "type": "subtree", "posX": 3000, "posY": 600, "maxRanks": 1,
                   "entries": [{"id": 151, "subTreeId": 77, "name": "", "maxRanks": 1}]},
                  {"id": 5, "type": "single", "posX": 6000, "posY": 0, "maxRanks": 1,
                   "subTreeId": 77,
                   "entries": [{"id": 106, "spellId": 1006, "name": "Hero", "maxRanks": 1}]}
                ]
              }]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_decoded_build_lays_out_three_panes() {
        let ds = dataset();
        let sels = [
            r#"{"node_id": 1, "ranks": 1}"#,
            r#"{"node_id": 2, "choice_index": 1}"#,
            r#"{"node_id": 4, "choice_index": 0}"#,
            r#"{"node_id": 5}"#,
        ]
        .map(|s| wowdps_proto::json::parse(s).unwrap());
        let enc = talents::encode(&ds, 62, &sels).unwrap();
        let s = enc.get("string").and_then(Json::as_str).unwrap();

        let dec = talents::decode(&ds, s).unwrap();
        let tv = talents::tree_view(&ds, 62).unwrap();
        let (sel_map, hero, warnings) = selections_from_decode(&dec);
        let b = build_model(&tv, &sel_map, hero, warnings).unwrap();

        assert_eq!(b.class_name, "Mage");
        assert_eq!(b.spec_name, "Arcane");
        assert_eq!(b.spec_id, 62);
        let (class, spec) = (&b.class_pane, &b.spec_pane);
        let hero = b.hero_pane.as_ref().expect("hero pane");
        assert_eq!(b.hero, Some((77, "Sunfury".to_string())));
        assert_eq!(class.nodes.len(), 2);
        // The subtree-selector node is not drawn — the hero pick shows as
        // the medallion + name — so the spec half holds only node 3.
        assert_eq!(spec.nodes.len(), 1);
        assert_eq!(hero.nodes.len(), 1);

        // Node 1: partial rank; node 2: choice picked "Right".
        let n1 = class.nodes.iter().find(|n| n.id == 1).unwrap();
        assert!(n1.selected && n1.ranks == 1 && n1.max_ranks == 2);
        // No entryType in the fixture → passive → circle; the choice node
        // wears the octagon.
        assert_eq!(n1.shape, IconStyle::Circle);
        let n2 = class.nodes.iter().find(|n| n.id == 2).unwrap();
        assert!(n2.choice);
        assert_eq!(n2.shape, IconStyle::Octagon);
        assert_eq!(n2.spell_id, 1032);
        assert!(n2.detail.contains("▸ Right"), "{}", n2.detail);
        // The class pane spent 2 points (1 + the choice's 1); the hero
        // pick's own point belongs to no drawn pane.
        assert_eq!(class.points, 2);
        // The edge 1 → 2 made it into the pane.
        assert_eq!(class.edges.len(), 1);

        // Unpicked spec node 3 is present, unselected.
        let n3 = spec.nodes.iter().find(|n| n.id == 3).unwrap();
        assert!(!n3.selected && n3.ranks == 0);
    }

    #[test]
    fn a_bare_spec_tree_has_no_hero_pane_and_nothing_selected() {
        let ds = dataset();
        let tv = talents::tree_view(&ds, 62).unwrap();
        let b = build_model(&tv, &HashMap::new(), None, Vec::new()).unwrap();
        assert!(b.hero_pane.is_none(), "no hero pick, no hero pane");
        assert!(b.hero.is_none());
        assert!(b.panes().all(|p| p.nodes.iter().all(|n| !n.selected)));
        assert!(b.panes().all(|p| p.points == 0));
        // With nothing taken, exactly the roots (no incoming edge) are
        // green-outlined available: node 1 (class) and node 3 (spec) —
        // node 2 waits on the 1 → 2 edge.
        let avail: Vec<u64> = b
            .panes()
            .flat_map(|p| p.nodes.iter())
            .filter(|n| n.available)
            .map(|n| n.id)
            .collect();
        assert_eq!(avail, vec![1, 3]);
    }

    #[test]
    fn removing_mid_tree_cascades_the_orphans() {
        // Class chain 1 → 2 both taken; removing 1 must sweep 2 with it.
        let ds = dataset();
        let tv = talents::tree_view(&ds, 62).unwrap();
        let mut sels: HashMap<u64, Sel> = HashMap::new();
        for id in [1u64, 2] {
            sels.insert(
                id,
                Sel {
                    ranks: 1,
                    granted: false,
                    choice_index: (id == 2).then_some(0),
                },
            );
        }
        let build = build_model(&tv, &sels, None, Vec::new()).unwrap();
        let mut ui = TalentsUi {
            input: String::new(),
            player: None,
            profile: None,
            loadout_sel: 0,
            tab: Tab::Talents,
            spec_id: Some(62),
            sels,
            hero: None,
            tree: None,
            warnings: Vec::new(),
            build: Some(build),
            error: None,
            picker: None,
            hover: None,
            hover_at: (0.0, 0.0),
            edited: false,
            logged: false,
            logged_gear: None,
        };
        ui.sels.remove(&1);
        ui.cascade_orphans(1);
        assert!(
            !ui.sels.contains_key(&2),
            "node 2 lost its only taken parent and must cascade away"
        );
    }

    #[test]
    fn a_refund_below_a_gate_drops_the_gated_node() {
        // Node 2 is gated at 2 points and node 1 (2 ranks) alone funds it.
        // Refunding node 1 to one rank breaks the gate: node 2 must go
        // with it, or the edited build would encode into an import string
        // the game rejects.
        let ds = dataset();
        let tv = talents::tree_view(&ds, 62).unwrap();
        let mut sels: HashMap<u64, Sel> = HashMap::new();
        sels.insert(
            1,
            Sel {
                ranks: 2,
                granted: false,
                choice_index: None,
            },
        );
        sels.insert(
            2,
            Sel {
                ranks: 1,
                granted: false,
                choice_index: Some(0),
            },
        );
        let build = build_model(&tv, &sels, None, Vec::new()).unwrap();
        let mut ui = TalentsUi {
            input: String::new(),
            player: None,
            profile: None,
            loadout_sel: 0,
            tab: Tab::Talents,
            spec_id: Some(62),
            sels,
            hero: None,
            // Injected tree: rebuild must not reach for the real dataset.
            tree: Some((62, tv)),
            warnings: Vec::new(),
            build: Some(build),
            error: None,
            picker: None,
            hover: None,
            hover_at: (0.0, 0.0),
            edited: false,
            logged: false,
            logged_gear: None,
        };
        ui.unclick_node(1);
        assert_eq!(
            ui.sels.get(&1).map(|s| s.ranks),
            Some(1),
            "the refund itself lands"
        );
        assert!(
            !ui.sels.contains_key(&2),
            "gate at 2 points broken by the refund: node 2 must drop"
        );
        // And the rebuilt layout agrees: node 2 is unselected and, with
        // only one point above its gate, not even available.
        let n2 = ui
            .build
            .as_ref()
            .unwrap()
            .panes()
            .flat_map(|p| p.nodes.iter())
            .find(|n| n.id == 2)
            .unwrap()
            .clone();
        assert!(!n2.selected && !n2.available);
    }

    #[test]
    fn store_path_keeps_the_realm_and_sanitizes() {
        // The realm is part of the key: same-named characters on different
        // realms must not share a file.
        let p = store_path("Tranqlock-Proudmoore").unwrap();
        assert!(
            p.ends_with("wowdps/simc/tranqlock_proudmoore.simc"),
            "{}",
            p.display()
        );
        assert_ne!(
            store_path("Tranqlock-Proudmoore"),
            store_path("Tranqlock-Illidan")
        );
        let p = store_path("Wëïrd Nàme").unwrap();
        assert!(p.to_string_lossy().ends_with(".simc"), "{}", p.display());
    }

    /// Against the REAL per-machine dataset: every spec's tree lays out into
    /// class + spec panes with plausible node counts, and a string minted
    /// from real node ids decodes back into a laid-out build. Ignored like
    /// the `real_log` gates — it needs `talents.json` on this machine.
    #[test]
    #[ignore = "needs the per-machine talents.json (tools/gen-talent-trees.sh)"]
    fn real_dataset_lays_out_every_spec() {
        let ds = talents::load().expect("no talents.json on this machine");
        for tree in arr(ds.get("trees")) {
            for spec in arr(tree.get("specs")) {
                let spec_id = get_u64(spec, "specId").unwrap();
                let tv = talents::tree_view(ds, spec_id).unwrap();
                let b = build_model(&tv, &HashMap::new(), None, Vec::new())
                    .unwrap_or_else(|e| panic!("spec {spec_id}: {e}"));
                assert!(b.hero_pane.is_none(), "spec {spec_id}: hero without a pick");
                for p in b.panes() {
                    assert!(
                        p.nodes.len() > 20,
                        "spec {spec_id}: a pane has only {} nodes",
                        p.nodes.len()
                    );
                    assert!(!p.edges.is_empty(), "spec {spec_id}: no edges");
                    assert!(p.w > CELL && p.h > CELL);
                }
                // Both frame shapes appear in every real tree.
                assert!(
                    b.panes()
                        .flat_map(|p| p.nodes.iter())
                        .any(|n| n.shape == IconStyle::Square),
                    "spec {spec_id}: no active (square) node"
                );
                assert!(
                    b.panes()
                        .flat_map(|p| p.nodes.iter())
                        .any(|n| n.shape == IconStyle::Circle),
                    "spec {spec_id}: no passive (circle) node"
                );
            }
        }

        // Mint a real string: first tree, first spec, first three plain
        // single nodes of its order, then decode and lay it out.
        let tree = arr(ds.get("trees")).first().unwrap();
        let spec_id = arr(tree.get("specs"))
            .iter()
            .find_map(|s| get_u64(s, "specId"))
            .unwrap();
        let singles: Vec<Json> = arr(tree.get("nodes"))
            .iter()
            .filter(|n| {
                get_str(n, "type") == "single"
                    && get_u64(n, "subTreeId").is_none()
                    && n.get("visibleFor").is_none()
            })
            .take(3)
            .filter_map(|n| get_u64(n, "id"))
            .map(|id| wowdps_proto::json::parse(&format!("{{\"node_id\": {id}}}")).unwrap())
            .collect();
        assert_eq!(singles.len(), 3);
        let enc = talents::encode(ds, spec_id, &singles).unwrap();
        let s = enc.get("string").and_then(Json::as_str).unwrap();
        let (dec_spec, dec_sels, dec_hero, _) = decode_build(s).unwrap();
        let tv = talents::tree_view(ds, dec_spec).unwrap();
        let b = build_model(&tv, &dec_sels, dec_hero, Vec::new()).unwrap();
        let taken: usize = b
            .panes()
            .map(|p| p.nodes.iter().filter(|n| n.selected).count())
            .sum();
        assert_eq!(taken, 3, "the three minted picks survive the layout");

        // Interactive editing against the real tree: click a root onto the
        // empty tree, watch the frontier open, chain a child, then cascade
        // the whole path away with one right-click on the root.
        let mut ui = TalentsUi {
            input: String::new(),
            player: None,
            profile: None,
            loadout_sel: 0,
            tab: Tab::Talents,
            spec_id: Some(spec_id),
            sels: HashMap::new(),
            hero: None,
            tree: None,
            warnings: Vec::new(),
            build: None,
            error: None,
            picker: None,
            hover: None,
            hover_at: (0.0, 0.0),
            edited: false,
            logged: false,
            logged_gear: None,
        };
        ui.rebuild();
        let all = |ui: &TalentsUi, f: fn(&Node) -> bool| -> Vec<u64> {
            ui.build
                .iter()
                .flat_map(Build::panes)
                .flat_map(|p| p.nodes.iter())
                .filter(|n| f(n))
                .map(|n| n.id)
                .collect()
        };
        let root = *all(&ui, |n| n.available && !n.choice && n.max_ranks == 1)
            .first()
            .expect("an available plain root");
        let avail_before = all(&ui, |n| n.available);
        ui.click_node(root);
        assert!(ui.sels.contains_key(&root), "click takes the root");
        assert!(ui.edited);
        // A NEWLY available plain node is fed by the root alone: take it,
        // then cascade both away with one right-click on the root.
        let newly: Vec<u64> = all(&ui, |n| n.available && !n.choice && n.max_ranks == 1)
            .into_iter()
            .filter(|id| !avail_before.contains(id))
            .collect();
        if let Some(&child) = newly.first() {
            ui.click_node(child);
            assert!(ui.sels.contains_key(&child));
            ui.unclick_node(root);
            assert!(!ui.sels.contains_key(&root), "root refunded");
            assert!(
                !ui.sels.contains_key(&child),
                "the orphaned child cascades away with the root"
            );
        }
        let encoded = ui.encode_current().expect("edited build encodes");
        assert!(!encoded.is_empty());
    }
}
