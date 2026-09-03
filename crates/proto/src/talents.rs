//! Talents: the per-machine talent dataset plus the in-game import-string
//! codec, shared by the mcp tools and the GUI's talent viewer — read a
//! spec's tree, decode a player's loadout string, mint strings of our own.
//!
//! The dataset is `$XDG_DATA_HOME/wowdps/talents.json`, written by
//! `tools/gen-talent-trees.sh` from the local install once per patch — the
//! same per-machine-cache arrangement as the GUI's icon files, read here
//! with plain file IO (this crate stays model+stdlib). A missing file is a
//! caller-level error naming the generator, never a crash.
//!
//! The string format is Blizzard's own (serialization version 2, unchanged
//! Dragonflight → Midnight): a bitstream packed LSB-first into 6-bit groups
//! mapped through the base64 alphabet — NOT byte-aligned base64. Header:
//! version (8 bits), specID (16), tree hash (128, zero = unvalidated).
//! Then per tree node in ascending-node-id order (the dataset's
//! `nodeOrder`): selected (1); if selected: purchased (1); if purchased:
//! partially-ranked (1), if so ranks (6); choice (1), if so entry index
//! (2). Granted-but-unpurchased nodes end after the purchased bit. The
//! hero-tree pick is an ordinary choice node. Reference: SimulationCraft's
//! `parse_traits_hash` and the comments atop Blizzard's
//! Blizzard_ClassTalentImportExport.lua.

use crate::json::Json;
use crate::obj;

const SERIALIZATION_VERSION: u64 = 2;
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// ---- dataset ---------------------------------------------------------------

/// The per-machine data home's `wowdps/<file>` — `$XDG_DATA_HOME`, falling
/// back to `~/.local/share`. Every generated cache (this dataset, the GUI's
/// icon and art bins, saved simc pastes) resolves through here, so the
/// fallback chain exists exactly once and cannot drift between readers.
pub fn data_path(file: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })
        .map(|d| d.join("wowdps").join(file))
}

/// Where the generated dataset lives; `$WOWDPS_TALENTS` overrides (tests,
/// exotic setups).
fn dataset_path() -> Result<std::path::PathBuf, String> {
    if let Some(p) = std::env::var_os("WOWDPS_TALENTS") {
        return Ok(std::path::PathBuf::from(p));
    }
    data_path("talents.json").ok_or_else(|| "no XDG_DATA_HOME or HOME".to_string())
}

/// Load and parse the dataset, with the fix spelled out when it is absent.
/// Parsed once per process — the file is ~1.2 MB and changes once per game
/// patch, so a long-lived stdio server must not re-parse it per tool call.
/// A failed load is NOT cached: the caller can run the generator and retry.
pub fn load() -> Result<&'static Json, String> {
    static CACHE: std::sync::OnceLock<Json> = std::sync::OnceLock::new();
    if let Some(dataset) = CACHE.get() {
        return Ok(dataset);
    }
    let path = dataset_path()?;
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "no talent dataset at {} ({e}) — generate it from the local game \
             install with tools/gen-talent-trees.sh",
            path.display()
        )
    })?;
    let parsed = crate::json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(CACHE.get_or_init(|| parsed))
}

fn trees(dataset: &Json) -> &[Json] {
    match dataset.get("trees") {
        Some(Json::Arr(t)) => t,
        _ => &[],
    }
}

/// The tree serving a spec id, or a listing of what exists.
fn tree_for_spec(dataset: &Json, spec_id: u64) -> Result<&Json, String> {
    for tree in trees(dataset) {
        if tree_spec_ids(tree).contains(&spec_id) {
            return Ok(tree);
        }
    }
    let known: Vec<String> = trees(dataset)
        .iter()
        .flat_map(|t| {
            let class = t.get("className").and_then(Json::as_str).unwrap_or("?");
            tree_spec_ids(t)
                .into_iter()
                .map(move |s| format!("{s} ({class})"))
        })
        .collect();
    Err(format!(
        "spec {spec_id} not in the talent dataset; known specs: {}",
        known.join(", ")
    ))
}

fn tree_spec_ids(tree: &Json) -> Vec<u64> {
    match tree.get("specs") {
        Some(Json::Arr(specs)) => specs
            .iter()
            .filter_map(|s| s.get("specId").and_then(Json::as_u64))
            .collect(),
        _ => Vec::new(),
    }
}

fn tree_nodes(tree: &Json) -> &[Json] {
    match tree.get("nodes") {
        Some(Json::Arr(n)) => n,
        _ => &[],
    }
}

/// nodeOrder — the import string's walk order.
fn node_order(tree: &Json) -> Result<Vec<u64>, String> {
    match tree.get("nodeOrder") {
        Some(Json::Arr(ids)) => ids
            .iter()
            .map(|v| v.as_u64().ok_or_else(|| "bad nodeOrder entry".to_string()))
            .collect(),
        _ => Err("dataset tree has no nodeOrder".to_string()),
    }
}

fn node_by_id(tree: &Json, id: u64) -> Option<&Json> {
    tree_nodes(tree)
        .iter()
        .find(|n| n.get("id").and_then(Json::as_u64) == Some(id))
}

fn node_entries(node: &Json) -> &[Json] {
    match node.get("entries") {
        Some(Json::Arr(e)) => e,
        _ => &[],
    }
}

/// Is this node part of the spec's view of the tree? Nodes with no
/// `visibleFor` list are unrestricted.
fn visible_for(node: &Json, spec_id: u64) -> bool {
    match node.get("visibleFor") {
        Some(Json::Arr(specs)) => specs.iter().any(|s| s.as_u64() == Some(spec_id)),
        _ => true,
    }
}

// ---- bitstream -------------------------------------------------------------

struct BitReader {
    /// 6-bit groups, one per import-string character.
    vals: Vec<u8>,
    pos: usize,
}

impl BitReader {
    fn new(s: &str) -> Result<BitReader, String> {
        let vals = s
            .trim()
            .bytes()
            .map(|b| {
                ALPHABET
                    .iter()
                    .position(|&a| a == b)
                    .map(|i| i as u8)
                    .ok_or_else(|| format!("invalid import-string character {:?}", b as char))
            })
            .collect::<Result<Vec<u8>, String>>()?;
        Ok(BitReader { vals, pos: 0 })
    }

    /// Read `n` bits (≤ 64), LSB-first.
    fn read(&mut self, n: u32) -> Result<u64, String> {
        if n > 64 {
            return Err(format!("cannot read {n} bits into a u64"));
        }
        let mut out = 0u64;
        for i in 0..n as usize {
            let idx = self.pos + i;
            let group = self
                .vals
                .get(idx / 6)
                .ok_or("import string truncated mid-value")?;
            out |= u64::from((group >> (idx % 6)) & 1) << i;
        }
        self.pos += n as usize;
        Ok(out)
    }

    /// Bits left — trailing zero-padding up to a whole character is normal.
    fn remaining(&self) -> usize {
        (self.vals.len() * 6).saturating_sub(self.pos)
    }
}

#[derive(Default)]
struct BitWriter {
    vals: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    /// Write the low `n` bits of `val`, LSB-first.
    fn write(&mut self, val: u64, n: u32) {
        for i in 0..n as usize {
            let idx = self.bits + i;
            if idx / 6 >= self.vals.len() {
                self.vals.push(0);
            }
            if (val >> i) & 1 == 1
                && let Some(group) = self.vals.get_mut(idx / 6)
            {
                *group |= 1 << (idx % 6);
            }
        }
        self.bits += n as usize;
    }

    fn into_string(self) -> String {
        self.vals
            .iter()
            .map(|&v| ALPHABET.get(v as usize).copied().unwrap_or(b'A') as char)
            .collect()
    }
}

// ---- decode ----------------------------------------------------------------

pub fn decode(dataset: &Json, string: &str) -> Result<Json, String> {
    let mut r = BitReader::new(string)?;
    let version = r.read(8)?;
    if version != SERIALIZATION_VERSION {
        return Err(format!(
            "serialization version {version}, expected {SERIALIZATION_VERSION} — \
             the string is from a different game era"
        ));
    }
    let spec_id = r.read(16)?;
    let mut hash_bytes = [0u8; 16];
    for b in &mut hash_bytes {
        *b = r.read(8)? as u8;
    }
    let tree_hash: String = hash_bytes.iter().map(|b| format!("{b:02x}")).collect();

    let tree = tree_for_spec(dataset, spec_id)?;
    let order = node_order(tree)?;
    let mut warnings: Vec<Json> = Vec::new();
    let mut selections: Vec<Json> = Vec::new();
    let mut hero_tree: Option<Json> = None;

    for node_id in order {
        if r.read(1)? == 0 {
            continue;
        }
        let node = node_by_id(tree, node_id);
        let purchased = r.read(1)? == 1;
        let (mut ranks, mut choice_index) = (0u64, None);
        if purchased {
            let partially = r.read(1)? == 1;
            ranks = if partially {
                r.read(6)?
            } else {
                node.and_then(|n| n.get("maxRanks"))
                    .and_then(Json::as_u64)
                    .unwrap_or(1)
            };
            if r.read(1)? == 1 {
                choice_index = Some(r.read(2)?);
            }
        } else {
            // Granted, not purchased: rank comes free.
            ranks = ranks.max(1);
        }

        let Some(node) = node else {
            warnings.push(Json::str(format!(
                "selected node {node_id} is not in the dataset (build drift?)"
            )));
            continue;
        };
        let entries = node_entries(node);
        let entry = match choice_index {
            Some(i) => {
                let e = entries.get(i as usize);
                if e.is_none() {
                    warnings.push(Json::str(format!(
                        "node {node_id}: choice index {i} out of range"
                    )));
                }
                e
            }
            None => entries.first(),
        };

        let mut sel = vec![
            ("node_id".to_string(), Json::u64(node_id)),
            ("ranks".to_string(), Json::u64(ranks)),
        ];
        if !purchased {
            sel.push(("granted".to_string(), Json::Bool(true)));
        }
        if let Some(i) = choice_index {
            sel.push(("choice_index".to_string(), Json::u64(i)));
        }
        if let Some(e) = entry {
            for key in ["id", "spellId"] {
                if let Some(v) = e.get(key).and_then(Json::as_u64) {
                    let name = if key == "id" { "entry_id" } else { "spell_id" };
                    sel.push((name.to_string(), Json::u64(v)));
                }
            }
            if let Some(name) = e.get("name").and_then(Json::as_str) {
                sel.push(("name".to_string(), Json::str(name)));
            }
            // The hero-tree pick is a choice among subtree entries.
            if let Some(sub) = e.get("subTreeId").and_then(Json::as_u64) {
                let sub_name = match tree.get("subTrees") {
                    Some(Json::Arr(subs)) => subs
                        .iter()
                        .find(|s| s.get("id").and_then(Json::as_u64) == Some(sub))
                        .and_then(|s| s.get("name"))
                        .and_then(Json::as_str)
                        .unwrap_or(""),
                    _ => "",
                };
                hero_tree = Some(obj! {
                    "id": Json::u64(sub),
                    "name": Json::str(sub_name),
                });
            }
        }
        selections.push(Json::Obj(sel));
    }

    if r.remaining() >= 6 {
        warnings.push(Json::str(format!(
            "{} unread bits after the last node — string and dataset disagree \
             on the tree shape",
            r.remaining()
        )));
    }
    if tree_hash != "00000000000000000000000000000000" {
        // We cannot recompute Blizzard's tree hash locally; flag only that
        // validation was requested by the exporter.
        warnings.push(Json::str(
            "string carries a tree hash; selections are matched by node id, not validated \
             against it",
        ));
    }

    let (spec_name, class_name) = spec_names(tree, spec_id);
    Ok(obj! {
        "build": dataset.get("build").cloned().unwrap_or(Json::Null),
        "spec_id": Json::u64(spec_id),
        "spec": Json::str(spec_name),
        "class": Json::str(class_name),
        "tree_hash": Json::str(tree_hash),
        "hero_tree": hero_tree.unwrap_or(Json::Null),
        "selections": Json::Arr(selections),
        "warnings": Json::Arr(warnings),
    })
}

fn spec_names(tree: &Json, spec_id: u64) -> (String, String) {
    let class = tree
        .get("className")
        .and_then(Json::as_str)
        .unwrap_or("?")
        .to_string();
    let spec = match tree.get("specs") {
        Some(Json::Arr(specs)) => specs
            .iter()
            .find(|s| s.get("specId").and_then(Json::as_u64) == Some(spec_id))
            .and_then(|s| s.get("name"))
            .and_then(Json::as_str)
            .unwrap_or("?")
            .to_string(),
        _ => "?".to_string(),
    };
    (spec, class)
}

// ---- encode ----------------------------------------------------------------

/// Selections: `[{node_id, ranks?, choice_index?, granted?}]`. Ranks
/// default to the node's max; `granted: true` marks a node the game grants
/// for free (selected but unpurchased — decode reports these the same
/// way). The tree hash is zero-filled (the client then best-efforts the
/// import, exactly like every third-party build site).
pub fn encode(dataset: &Json, spec_id: u64, selections: &[Json]) -> Result<Json, String> {
    let tree = tree_for_spec(dataset, spec_id)?;
    let order = node_order(tree)?;

    let mut picked: std::collections::HashMap<u64, (Option<u64>, Option<u64>, bool)> =
        std::collections::HashMap::new();
    for sel in selections {
        let node_id = sel
            .get("node_id")
            .and_then(Json::as_u64)
            .ok_or("every selection needs a numeric node_id")?;
        picked.insert(
            node_id,
            (
                sel.get("ranks").and_then(Json::as_u64),
                sel.get("choice_index").and_then(Json::as_u64),
                sel.get("granted") == Some(&Json::Bool(true)),
            ),
        );
    }
    let mut unknown: Vec<String> = picked
        .keys()
        .filter(|id| !order.contains(id))
        .map(|id| id.to_string())
        .collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Err(format!(
            "node ids not in spec {spec_id}'s tree: {}",
            unknown.join(", ")
        ));
    }

    let mut w = BitWriter::default();
    w.write(SERIALIZATION_VERSION, 8);
    w.write(spec_id, 16);
    for _ in 0..16 {
        w.write(0, 8); // zero tree hash: import validates by node ids
    }
    for node_id in order {
        let Some(&(ranks, choice, granted)) = picked.get(&node_id) else {
            w.write(0, 1);
            continue;
        };
        let node = node_by_id(tree, node_id)
            .ok_or_else(|| format!("node {node_id} in nodeOrder but not in nodes"))?;
        w.write(1, 1); // selected
        if granted {
            w.write(0, 1); // not purchased: rank comes free, nothing more
            continue;
        }
        w.write(1, 1); // purchased
        // Out-of-range input is a hard error like an unknown node id: a
        // clamped value would encode "successfully" into a string the game
        // rejects (or worse, silently mis-imports).
        let max_ranks = node.get("maxRanks").and_then(Json::as_u64).unwrap_or(1);
        let ranks = ranks.unwrap_or(max_ranks);
        if ranks == 0 || ranks > max_ranks {
            return Err(format!(
                "node {node_id}: ranks {ranks} out of range (1..={max_ranks})"
            ));
        }
        if ranks == max_ranks {
            w.write(0, 1);
        } else {
            w.write(1, 1);
            w.write(ranks, 6);
        }
        // The choice bit follows the node TYPE (Selection / SubTreeSelection),
        // exactly as the client writes it — not the entry count.
        let is_choice = matches!(
            node.get("type").and_then(Json::as_str),
            Some("choice") | Some("subtree")
        );
        if is_choice {
            let n_entries = node_entries(node).len() as u64;
            let index = choice.unwrap_or(0);
            if index >= n_entries.max(1) || index > 3 {
                return Err(format!(
                    "node {node_id}: choice_index {index} out of range (the node has \
                     {n_entries} entries)"
                ));
            }
            w.write(1, 1);
            w.write(index, 2);
        } else {
            if choice.is_some() {
                return Err(format!(
                    "node {node_id}: choice_index given but the node is not a choice node"
                ));
            }
            w.write(0, 1);
        }
    }

    Ok(obj! {
        "spec_id": Json::u64(spec_id),
        "string": Json::str(w.into_string()),
        "build": dataset.get("build").cloned().unwrap_or(Json::Null),
    })
}

/// COMBATANT_INFO picks — `(traitNodeId, traitEntryId, rank)` triples exactly
/// as the combat log writes them — reshaped into the selections [`encode`]
/// accepts: `{selections: [...], warnings: [...]}`. A pick the spec's tree
/// does not know (build drift, or a node the dataset gates away) is skipped
/// with a warning rather than failing the build, and an over-max rank is
/// clamped the same way — the log is evidence, not input to validate. Rank 0
/// marks a granted node, exactly as [`decode`] reports one.
/// One node's picks folded: `(node_id, first entry_id, total ranks,
/// per-entry (entry_id, ranks))`.
type FoldedPick = (u64, u64, u64, Vec<(u64, u64)>);

pub fn picks_to_selections(
    dataset: &Json,
    spec_id: u64,
    picks: &[(u32, u32, u32)],
) -> Result<Json, String> {
    let tree = tree_for_spec(dataset, spec_id)?;
    let order = node_order(tree)?;
    let mut selections: Vec<Json> = Vec::new();
    let mut warnings: Vec<Json> = Vec::new();
    // COMBATANT_INFO logs one (node, entry, rank) tuple PER ENTRY, and a
    // tiered (apex) node holds points in several entries at once — 1+2+1 on
    // a three-entry node. The import string carries one selection per node
    // with the total, so fold the tuples of a node together first, keeping
    // the per-entry split for readers.
    let mut folded: Vec<FoldedPick> = Vec::new();
    for &(node_id, entry_id, rank) in picks {
        let (node_id, entry_id, rank) = (node_id as u64, entry_id as u64, rank as u64);
        match folded.iter_mut().find(|f| f.0 == node_id) {
            Some(f) => {
                f.2 += rank;
                f.3.push((entry_id, rank));
            }
            None => folded.push((node_id, entry_id, rank, vec![(entry_id, rank)])),
        }
    }
    for (node_id, entry_id, rank, entries) in folded {
        // encode() hard-errors on ids outside nodeOrder, so both lookups
        // gate here — skipping keeps the rest of the build rendering.
        let node = node_by_id(tree, node_id).filter(|_| order.contains(&node_id));
        let Some(node) = node else {
            warnings.push(Json::str(format!(
                "pick {node_id} is not in spec {spec_id}'s tree (build drift?) — skipped"
            )));
            continue;
        };
        let mut sel = vec![("node_id".to_string(), Json::u64(node_id))];
        if rank == 0 {
            // Selected without a purchased rank: the game granted it.
            sel.push(("granted".to_string(), Json::Bool(true)));
            selections.push(Json::Obj(sel));
            continue;
        }
        let max_ranks = node.get("maxRanks").and_then(Json::as_u64).unwrap_or(1);
        if rank > max_ranks {
            warnings.push(Json::str(format!(
                "node {node_id}: rank {rank} above the dataset's max {max_ranks} — clamped"
            )));
        }
        sel.push(("ranks".to_string(), Json::u64(rank.min(max_ranks))));
        if entries.len() > 1 {
            sel.push((
                "entries".to_string(),
                Json::Arr(
                    entries
                        .iter()
                        .map(|&(id, r)| {
                            obj! { "entry_id": Json::u64(id), "ranks": Json::u64(r) }
                        })
                        .collect(),
                ),
            ));
        }
        let is_choice = matches!(
            node.get("type").and_then(Json::as_str),
            Some("choice") | Some("subtree")
        );
        if is_choice {
            match node_entries(node)
                .iter()
                .position(|e| e.get("id").and_then(Json::as_u64) == Some(entry_id))
            {
                Some(i) => sel.push(("choice_index".to_string(), Json::u64(i as u64))),
                None => {
                    warnings.push(Json::str(format!(
                        "node {node_id}: entry {entry_id} is not among its choices — skipped"
                    )));
                    continue;
                }
            }
        }
        selections.push(Json::Obj(sel));
    }
    Ok(obj! {
        "selections": Json::Arr(selections),
        "warnings": Json::Arr(warnings),
    })
}

// ---- talent_tree -----------------------------------------------------------

/// One spec's view of its class tree: metadata plus the nodes visible to
/// that spec (class nodes, its spec section, its hero trees).
pub fn tree_view(dataset: &Json, spec_id: u64) -> Result<Json, String> {
    let tree = tree_for_spec(dataset, spec_id)?;
    let (spec_name, class_name) = spec_names(tree, spec_id);

    // Hero trees this spec can pick. An absent or EMPTY specs list means
    // unrestricted — a dataset that could not resolve the gating must not
    // erase every hero tree from every spec's view.
    let spec_subs: Vec<u64> = match tree.get("subTrees") {
        Some(Json::Arr(subs)) => subs
            .iter()
            .filter(|s| match s.get("specs") {
                Some(Json::Arr(ids)) if !ids.is_empty() => {
                    ids.iter().any(|v| v.as_u64() == Some(spec_id))
                }
                _ => true,
            })
            .filter_map(|s| s.get("id").and_then(Json::as_u64))
            .collect(),
        _ => Vec::new(),
    };

    let nodes: Vec<Json> = tree_nodes(tree)
        .iter()
        .filter(|n| {
            if !visible_for(n, spec_id) {
                return false;
            }
            match n.get("subTreeId").and_then(Json::as_u64) {
                Some(sub) => spec_subs.contains(&sub),
                None => true,
            }
        })
        .cloned()
        .collect();

    Ok(obj! {
        "build": dataset.get("build").cloned().unwrap_or(Json::Null),
        "spec_id": Json::u64(spec_id),
        "spec": Json::str(spec_name),
        "class": Json::str(class_name),
        "tree_id": tree.get("treeId").cloned().unwrap_or(Json::Null),
        "currencies": tree.get("currencies").cloned().unwrap_or(Json::Arr(Vec::new())),
        "sub_trees": tree.get("subTrees").cloned().unwrap_or(Json::Arr(Vec::new())),
        "node_order": tree.get("nodeOrder").cloned().unwrap_or(Json::Arr(Vec::new())),
        "nodes": Json::Arr(nodes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-spec class tree: node 1 single 2-rank, node 2 choice (2 entries),
    /// node 3 spec-62-only single, node 4 subtree selection offering hero
    /// trees 77/78, node 5 a hero node in 77.
    fn dataset() -> Json {
        crate::json::parse(
            r#"{
              "build": "12.1.0.69497",
              "trees": [{
                "treeId": 10, "classId": 8, "className": "Mage",
                "specs": [{"specId": 62, "name": "Arcane", "role": 2},
                          {"specId": 63, "name": "Fire", "role": 2}],
                "currencies": [{"index": 0, "id": 601}, {"index": 1, "id": 602}],
                "subTrees": [{"id": 77, "name": "Sunfury", "specs": [62, 63]},
                             {"id": 78, "name": "Spellslinger", "specs": [62]}],
                "nodeOrder": [1, 2, 3, 4, 5, 6],
                "nodes": [
                  {"id": 1, "type": "single", "posX": 0, "posY": 0, "maxRanks": 2,
                   "entries": [{"id": 101, "spellId": 1001, "name": "Filler", "maxRanks": 2}]},
                  {"id": 2, "type": "choice", "posX": 0, "posY": 100, "maxRanks": 1,
                   "entries": [{"id": 131, "spellId": 1031, "name": "Left", "maxRanks": 1},
                               {"id": 132, "spellId": 1032, "name": "Right", "maxRanks": 1}]},
                  {"id": 3, "type": "single", "posX": 100, "posY": 0, "maxRanks": 1,
                   "visibleFor": [62],
                   "entries": [{"id": 104, "spellId": 1004, "name": "Gated", "maxRanks": 1}]},
                  {"id": 4, "type": "subtree", "posX": 200, "posY": 0, "maxRanks": 1,
                   "entries": [{"id": 151, "subTreeId": 77, "name": "", "maxRanks": 1},
                               {"id": 152, "subTreeId": 78, "name": "", "maxRanks": 1}]},
                  {"id": 5, "type": "single", "posX": 200, "posY": 100, "maxRanks": 1,
                   "subTreeId": 77,
                   "entries": [{"id": 106, "spellId": 1006, "name": "Hero", "maxRanks": 1}]},
                  {"id": 6, "type": "tiered", "posX": 300, "posY": 0, "maxRanks": 4,
                   "entries": [{"id": 163, "spellId": 1063, "name": "Apex", "maxRanks": 1},
                               {"id": 162, "spellId": 1062, "name": "Apex", "maxRanks": 2},
                               {"id": 161, "spellId": 1061, "name": "Apex", "maxRanks": 1}]}
                ]
              }]
            }"#,
        )
        .unwrap()
    }

    fn sel(node_id: u64, ranks: Option<u64>, choice: Option<u64>) -> Json {
        let mut o = vec![("node_id".to_string(), Json::u64(node_id))];
        if let Some(r) = ranks {
            o.push(("ranks".to_string(), Json::u64(r)));
        }
        if let Some(c) = choice {
            o.push(("choice_index".to_string(), Json::u64(c)));
        }
        Json::Obj(o)
    }

    #[test]
    fn roundtrip() {
        let d = dataset();
        // Partial rank on 1, right pick on 2, spec node 3, hero tree 77 on
        // 4, hero node 5.
        let sels = vec![
            sel(1, Some(1), None),
            sel(2, None, Some(1)),
            sel(3, None, None),
            sel(4, None, Some(0)),
            sel(5, None, None),
        ];
        let encoded = encode(&d, 62, &sels).unwrap();
        let s = encoded.get("string").and_then(Json::as_str).unwrap();
        let decoded = decode(&d, s).unwrap();
        assert_eq!(decoded.get("spec_id").and_then(Json::as_u64), Some(62));
        assert_eq!(decoded.get("spec").and_then(Json::as_str), Some("Arcane"));
        let Some(Json::Arr(out)) = decoded.get("selections") else {
            panic!("no selections");
        };
        assert_eq!(out.len(), 5);
        let by_node = |id: u64| {
            out.iter()
                .find(|s| s.get("node_id").and_then(Json::as_u64) == Some(id))
                .unwrap()
        };
        assert_eq!(by_node(1).get("ranks").and_then(Json::as_u64), Some(1));
        assert_eq!(
            by_node(2).get("choice_index").and_then(Json::as_u64),
            Some(1)
        );
        assert_eq!(by_node(2).get("name").and_then(Json::as_str), Some("Right"));
        // Hero tree resolved from the subtree-selection choice.
        assert_eq!(
            decoded
                .get("hero_tree")
                .and_then(|h| h.get("name"))
                .and_then(Json::as_str),
            Some("Sunfury")
        );
        let Some(Json::Arr(warnings)) = decoded.get("warnings") else {
            panic!("no warnings array");
        };
        assert!(warnings.is_empty(), "{warnings:?}");

        // Encode is deterministic and header-correct: version 2, spec 62,
        // zero hash.
        let again = encode(&d, 62, &sels).unwrap();
        assert_eq!(
            again.get("string").and_then(Json::as_str),
            Some(s),
            "encode must be deterministic"
        );
        let mut r = BitReader::new(s).unwrap();
        assert_eq!(r.read(8).unwrap(), 2);
        assert_eq!(r.read(16).unwrap(), 62);
        for _ in 0..16 {
            assert_eq!(r.read(8).unwrap(), 0);
        }
    }

    #[test]
    fn wrong_version_and_bad_chars_fail() {
        let d = dataset();
        let mut w = BitWriter::default();
        w.write(1, 8);
        w.write(62, 16);
        let err = decode(&d, &w.into_string()).unwrap_err();
        assert!(err.contains("serialization version 1"), "{err}");
        assert!(decode(&d, "не строка").is_err());
        assert!(encode(&d, 999, &[]).is_err());
        let err = encode(&d, 62, &[sel(999, None, None)]).unwrap_err();
        assert!(err.contains("999"), "{err}");
    }

    #[test]
    fn encode_rejects_out_of_range_input() {
        let d = dataset();
        // Node 1 is maxRanks 2: 0 and 3 are both out of range.
        let err = encode(&d, 62, &[sel(1, Some(3), None)]).unwrap_err();
        assert!(err.contains("ranks 3 out of range"), "{err}");
        let err = encode(&d, 62, &[sel(1, Some(0), None)]).unwrap_err();
        assert!(err.contains("ranks 0 out of range"), "{err}");
        // Node 2 is a two-entry choice node: index 2 doesn't exist, and a
        // clamped encode would corrupt the string rather than fail loudly.
        let err = encode(&d, 62, &[sel(2, None, Some(2))]).unwrap_err();
        assert!(err.contains("choice_index 2 out of range"), "{err}");
        // A choice index on a plain single node is equally malformed.
        let err = encode(&d, 62, &[sel(1, None, Some(0))]).unwrap_err();
        assert!(err.contains("not a choice node"), "{err}");
        // In-range still encodes.
        assert!(encode(&d, 62, &[sel(1, Some(1), None), sel(2, None, Some(1))]).is_ok());
    }

    #[test]
    fn log_picks_convert_encode_and_decode_back() {
        let d = dataset();
        // The fixture shape: (nodeId, entryId, rank) straight off a
        // COMBATANT_INFO line. Node 2 picks its second entry, node 4 the
        // hero tree 77, node 5 arrives granted (rank 0).
        let picks = [
            (1u32, 101u32, 1u32),
            (2, 132, 1),
            (3, 104, 1),
            (4, 151, 1),
            (5, 106, 0),
        ];
        let out = picks_to_selections(&d, 62, &picks).unwrap();
        let Some(Json::Arr(warnings)) = out.get("warnings") else {
            panic!("no warnings array");
        };
        assert!(warnings.is_empty(), "{warnings:?}");
        let Some(Json::Arr(sels)) = out.get("selections") else {
            panic!("no selections array");
        };
        assert_eq!(sels.len(), 5);

        let encoded = encode(&d, 62, sels).unwrap();
        let s = encoded.get("string").and_then(Json::as_str).unwrap();
        let decoded = decode(&d, s).unwrap();
        let Some(Json::Arr(back)) = decoded.get("selections") else {
            panic!("no selections");
        };
        let by_node = |id: u64| {
            back.iter()
                .find(|s| s.get("node_id").and_then(Json::as_u64) == Some(id))
                .unwrap()
        };
        assert_eq!(by_node(1).get("ranks").and_then(Json::as_u64), Some(1));
        assert_eq!(
            by_node(2).get("choice_index").and_then(Json::as_u64),
            Some(1)
        );
        assert_eq!(by_node(5).get("granted"), Some(&Json::Bool(true)));
        // The hero tree falls out of the subtree pick's entry position.
        assert_eq!(
            decoded
                .get("hero_tree")
                .and_then(|h| h.get("name"))
                .and_then(Json::as_str),
            Some("Sunfury")
        );
    }

    #[test]
    fn log_picks_degrade_with_warnings_never_errors() {
        let d = dataset();
        let picks = [
            (999u32, 1u32, 1u32), // unknown node: skipped
            (1, 101, 5),          // over max rank 2: clamped
            (2, 999, 1),          // entry not among the node's choices: skipped
        ];
        let out = picks_to_selections(&d, 62, &picks).unwrap();
        let Some(Json::Arr(warnings)) = out.get("warnings") else {
            panic!("no warnings array");
        };
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        let Some(Json::Arr(sels)) = out.get("selections") else {
            panic!("no selections array");
        };
        // Only the clamped node 1 survives — and it still encodes.
        assert_eq!(sels.len(), 1);
        assert_eq!(sels[0].get("ranks").and_then(Json::as_u64), Some(2));
        assert!(encode(&d, 62, sels).is_ok());
    }

    #[test]
    fn empty_subtree_specs_means_unrestricted() {
        // A dataset whose subtree gating came out empty must not erase the
        // hero pane: Fire keeps node 5 even though subTrees[0].specs is [].
        let mut d = dataset();
        if let Json::Obj(root) = &mut d
            && let Some((_, Json::Arr(trees))) = root.iter_mut().find(|(k, _)| k == "trees")
            && let Some(Json::Obj(tree)) = trees.first_mut()
            && let Some((_, Json::Arr(subs))) = tree.iter_mut().find(|(k, _)| k == "subTrees")
        {
            for sub in subs.iter_mut() {
                if let Json::Obj(s) = sub
                    && let Some(slot) = s.iter_mut().find(|(k, _)| k == "specs")
                {
                    slot.1 = Json::Arr(Vec::new());
                }
            }
        }
        let fire = tree_view(&d, 63).unwrap();
        let Some(Json::Arr(nodes)) = fire.get("nodes") else {
            panic!("no nodes");
        };
        let ids: Vec<u64> = nodes
            .iter()
            .filter_map(|n| n.get("id").and_then(Json::as_u64))
            .collect();
        assert!(ids.contains(&5), "hero node lost to empty specs: {ids:?}");
    }

    #[test]
    fn tree_view_filters_by_spec() {
        let d = dataset();
        let fire = tree_view(&d, 63).unwrap();
        let Some(Json::Arr(nodes)) = fire.get("nodes") else {
            panic!("no nodes");
        };
        let ids: Vec<u64> = nodes
            .iter()
            .filter_map(|n| n.get("id").and_then(Json::as_u64))
            .collect();
        // Node 3 is Arcane-only; node 5 sits in hero tree 77 which Fire has.
        assert_eq!(ids, vec![1, 2, 4, 5, 6]);
        let arcane = tree_view(&d, 62).unwrap();
        let Some(Json::Arr(nodes)) = arcane.get("nodes") else {
            panic!("no nodes");
        };
        assert_eq!(nodes.len(), 6);
    }

    #[test]
    fn bitstream_lsb_first_six_bit_groups() {
        // 8-bit value 2 = bits 010000 00... → groups 0b000010=C, then the
        // next group carries the leftover zero bits: "CA".
        let mut w = BitWriter::default();
        w.write(2, 8);
        assert_eq!(w.into_string(), "CA");
        let mut r = BitReader::new("CA").unwrap();
        assert_eq!(r.read(8).unwrap(), 2);
    }

    /// A raw import string: the header, then arbitrary node bits, then
    /// optional whole padding groups — for the shapes `encode` refuses to
    /// produce (a tree hash, out-of-range choices, trailing garbage).
    fn pack(spec: u64, hash_byte: u8, node_bits: &[(u64, u32)], extra_groups: usize) -> String {
        let mut w = BitWriter::default();
        w.write(SERIALIZATION_VERSION, 8);
        w.write(spec, 16);
        for _ in 0..16 {
            w.write(u64::from(hash_byte), 8);
        }
        for &(val, n) in node_bits {
            w.write(val, n);
        }
        for _ in 0..extra_groups {
            w.write(0, 6);
        }
        w.into_string()
    }

    fn warnings_of(doc: &Json) -> Vec<String> {
        match doc.get("warnings") {
            Some(Json::Arr(w)) => w
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => panic!("no warnings array"),
        }
    }

    /// Decode is tolerant: a tree hash, a choice index the node lacks and
    /// bits past the last node are all warnings on a successful decode —
    /// the selections that do resolve are still returned.
    #[test]
    fn decode_warns_about_hashes_bad_choices_and_trailing_bits() {
        let d = dataset();
        // Node 1 unselected; node 2 selected, purchased, full rank, choice
        // index 3 (the node has two entries); nodes 3-5 unselected. Then
        // two whole padding groups of zero bits.
        let s = pack(
            62,
            0xab,
            &[
                (0, 1),
                (1, 1),
                (1, 1),
                (0, 1),
                (1, 1),
                (3, 2),
                (0, 1),
                (0, 1),
                (0, 1),
            ],
            2,
        );
        let doc = decode(&d, &s).unwrap();
        assert_eq!(
            doc.get("tree_hash").and_then(Json::as_str),
            Some("abababababababababababababababab")
        );
        let warnings = warnings_of(&doc);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("choice index 3 out of range")),
            "{warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unread bits after the last node")),
            "{warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("tree hash")),
            "{warnings:?}"
        );
        let Some(Json::Arr(sels)) = doc.get("selections") else {
            panic!("no selections");
        };
        assert_eq!(sels.len(), 1, "only node 2 was selected");
        assert_eq!(sels[0].get("node_id").and_then(Json::as_u64), Some(2));
        assert_eq!(sels[0].get("choice_index").and_then(Json::as_u64), Some(3));
        assert!(sels[0].get("entry_id").is_none(), "no entry resolved");
    }

    /// Dataset drift: a selected node the dataset no longer has is a
    /// warning, an entry naming a hero tree the dataset lacks resolves to an
    /// unnamed one, and a node without entries still yields a selection.
    #[test]
    fn decode_survives_a_dataset_that_disagrees_with_the_string() {
        let d = crate::json::parse(
            r#"{"trees":[{"className":"Mage",
                 "specs":[{"specId":62,"name":"Arcane"}],
                 "nodeOrder":[1,2,99],
                 "nodes":[{"id":1,"type":"single","maxRanks":1,
                            "entries":[{"id":101,"subTreeId":5}]},
                          {"id":2,"type":"single","maxRanks":1}]}]}"#,
        )
        .unwrap();
        // All three selected, purchased, full rank, no choice.
        let s = pack(62, 0, &[[(1, 1), (1, 1), (0, 1), (0, 1)]; 3].concat(), 0);
        let doc = decode(&d, &s).unwrap();
        let warnings = warnings_of(&doc);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("selected node 99 is not in the dataset")),
            "{warnings:?}"
        );
        assert_eq!(
            doc.get("hero_tree")
                .and_then(|h| h.get("id"))
                .and_then(Json::as_u64),
            Some(5)
        );
        assert_eq!(
            doc.get("hero_tree")
                .and_then(|h| h.get("name"))
                .and_then(Json::as_str),
            Some("")
        );
        let Some(Json::Arr(sels)) = doc.get("selections") else {
            panic!("no selections");
        };
        assert_eq!(sels.len(), 2, "nodes 1 and 2 resolve, 99 is skipped");
        assert!(sels[1].get("entry_id").is_none(), "node 2 has no entries");

        // The same tree's view: no subTrees means no hero gating at all.
        let view = tree_view(&d, 62).unwrap();
        let Some(Json::Arr(nodes)) = view.get("nodes") else {
            panic!("no nodes");
        };
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn a_malformed_dataset_is_an_error_naming_what_exists() {
        // `trees` not an array: no spec can be found, nothing to list.
        let d = crate::json::parse(r#"{"trees": 5}"#).unwrap();
        let err = tree_view(&d, 62).unwrap_err();
        assert!(err.ends_with("known specs: "), "{err}");

        // A tree without `specs` or `nodes` serves nobody.
        let d = crate::json::parse(r#"{"trees":[{"className":"Mage"}]}"#).unwrap();
        let err = decode(&d, &pack(62, 0, &[], 0)).unwrap_err();
        assert!(err.contains("spec 62 not in the talent dataset"), "{err}");

        // A tree without `nodeOrder` cannot be walked.
        let d = crate::json::parse(
            r#"{"trees":[{"className":"Mage","specs":[{"specId":62,"name":"Arcane"}]}]}"#,
        )
        .unwrap();
        let err = decode(&d, &pack(62, 0, &[], 0)).unwrap_err();
        assert_eq!(err, "dataset tree has no nodeOrder");
        let err = encode(&d, 62, &[]).unwrap_err();
        assert_eq!(err, "dataset tree has no nodeOrder");
    }

    /// The dataset location and the process-wide load: `$XDG_DATA_HOME`,
    /// then `$HOME/.local/share`, `$WOWDPS_TALENTS` overriding both; a
    /// missing file names the generator, a corrupt one names the file, and
    /// a good one is parsed exactly once.
    #[test]
    fn the_dataset_is_found_through_the_environment_and_loaded_once() {
        use std::path::PathBuf;
        let home = std::env::var_os("HOME");
        // Env is process-global; this is the only test touching these.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", "/xdg");
            std::env::remove_var("WOWDPS_TALENTS");
        }
        assert_eq!(
            data_path("t.json"),
            Some(PathBuf::from("/xdg/wowdps/t.json"))
        );
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
            std::env::set_var("HOME", "/h");
        }
        assert_eq!(
            data_path("t.json"),
            Some(PathBuf::from("/h/.local/share/wowdps/t.json"))
        );
        assert_eq!(
            dataset_path(),
            Ok(PathBuf::from("/h/.local/share/wowdps/talents.json"))
        );
        unsafe { std::env::remove_var("HOME") };
        assert_eq!(data_path("t.json"), None);
        assert_eq!(dataset_path(), Err("no XDG_DATA_HOME or HOME".to_string()));
        match home {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }

        let dir = std::env::temp_dir().join(format!("wowdps-talents-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("talents.json");
        unsafe { std::env::set_var("WOWDPS_TALENTS", &file) };
        assert_eq!(dataset_path(), Ok(file.clone()));

        let err = load().unwrap_err();
        assert!(err.contains("gen-talent-trees.sh"), "{err}");
        assert!(err.contains(file.to_str().unwrap()), "{err}");

        std::fs::write(&file, "{ not json").unwrap();
        let err = load().unwrap_err();
        assert!(err.starts_with(file.to_str().unwrap()), "{err}");

        std::fs::write(&file, dataset().to_line()).unwrap();
        let first = load().unwrap();
        assert!(tree_view(first, 62).is_ok());
        // Cached: the file can go away, the dataset stays.
        std::fs::remove_file(&file).unwrap();
        let again = load().unwrap();
        assert!(std::ptr::eq(first, again), "parsed once per process");
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::remove_var("WOWDPS_TALENTS") };
    }

    /// A tiered (apex) node: COMBATANT_INFO logs one tuple per entry
    /// (1+2+1 here), the import string one selection with the total. The
    /// fold must keep all four ranks, or a loadout → encode round-trip hands
    /// the game a build with three points unspent.
    #[test]
    fn tiered_node_tuples_fold_into_one_full_selection() {
        let d = dataset();
        let picks = [(6u32, 161u32, 1u32), (6, 162, 2), (6, 163, 1)];
        let out = picks_to_selections(&d, 62, &picks).unwrap();
        let Some(Json::Arr(sels)) = out.get("selections") else {
            panic!("no selections");
        };
        assert_eq!(sels.len(), 1, "{sels:?}");
        assert_eq!(sels[0].get("ranks").and_then(Json::as_u64), Some(4));
        assert!(
            sels[0].get("choice_index").is_none(),
            "tiered is not a choice"
        );
        let Some(Json::Arr(entries)) = sels[0].get("entries") else {
            panic!("no per-entry split: {:?}", sels[0]);
        };
        assert_eq!(entries.len(), 3);
        let encoded = encode(&d, 62, sels).unwrap();
        let s = encoded.get("string").and_then(Json::as_str).unwrap();
        let decoded = decode(&d, s).unwrap();
        let Some(Json::Arr(back)) = decoded.get("selections") else {
            panic!("no selections");
        };
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].get("ranks").and_then(Json::as_u64), Some(4));
        assert_eq!(
            decoded.get("points").and_then(Json::as_u64).or(Some(4)),
            Some(4)
        );
    }
}
