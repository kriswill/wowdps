//! The viewer's logic over a synthetic dataset, through the test hook —
//! moved with the code from the iced viewer, whose rendering tests stay
//! there.

use std::collections::HashMap;

use wowdps_model::{GearItem, Loadout, TalentPick};
use wowdps_proto::json::Json;
use wowdps_proto::talents as codec;

use super::fixture::{Sandbox, dataset, full_string, simc_paste, string_for};
use super::*;
use crate::simc::{save_stored, store_path};
use crate::spell_icons::IconStyle;

fn node(ui: &Viewer, id: u64) -> Node {
    ui.find_node(id)
        .unwrap_or_else(|| panic!("node {id} not laid out"))
}

fn selected_ids(ui: &Viewer) -> Vec<u64> {
    let mut v: Vec<u64> = ui.sels.keys().copied().collect();
    v.sort_unstable();
    v
}

/// A viewer over `sels` with the spec's tree already in hand, laid out.
fn viewer_with(sels: HashMap<u64, Sel>) -> Viewer {
    let tv = codec::tree_view(dataset(), 62).unwrap();
    let build = build_model(&tv, &sels, None, Vec::new()).unwrap();
    let mut ui = Viewer::empty();
    ui.spec_id = Some(62);
    ui.sels = sels;
    // Injected tree: rebuild must not reach for the real dataset.
    ui.tree = Some((62, tv));
    ui.build = Some(build);
    ui
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
    let enc = codec::encode(ds, 62, &sels).unwrap();
    let s = enc.get("string").and_then(Json::as_str).unwrap();

    let dec = codec::decode(ds, s).unwrap();
    let tv = codec::tree_view(ds, 62).unwrap();
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
    // the medallion + name — so the spec half holds nodes 3 and 6 only.
    assert_eq!(spec.nodes.len(), 2);
    assert_eq!(hero.nodes.len(), 1);

    // Node 1: partial rank; node 2: choice picked "Right".
    let n1 = class.nodes.iter().find(|n| n.id == 1).unwrap();
    assert!(n1.selected && n1.ranks == 1 && n1.max_ranks == 3);
    // entryType 1 → active → square; the choice node wears the
    // octagon; the unmarked spec node 3 is a passive circle.
    assert_eq!(n1.shape, IconStyle::Square);
    assert_eq!(
        spec.nodes.iter().find(|n| n.id == 3).unwrap().shape,
        IconStyle::Circle
    );
    // The pane caps come from the cost currencies' max.
    assert_eq!(class.cap, Some(3));
    assert_eq!(spec.cap, Some(2));
    assert_eq!(hero.cap, None, "hero nodes carry no cost in the fixture");
    // The tiered node keeps its stages for the tooltip.
    let n6 = spec.nodes.iter().find(|n| n.id == 6).unwrap();
    assert_eq!(n6.tiers.len(), 2);
    assert_eq!(n6.tiers[1].name, "Stage Two");
    assert!(n6.options.is_empty(), "tiers are not choices");
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
    let tv = codec::tree_view(ds, 62).unwrap();
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
    let mut ui = viewer_with(sels);
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
    let mut ui = viewer_with(sels);
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
    let n2 = node(&ui, 2);
    assert!(!n2.selected && !n2.available);
}

/// Against the REAL per-machine dataset: every spec's tree lays out into
/// class + spec panes with plausible node counts, and a string minted
/// from real node ids decodes back into a laid-out build. Ignored like
/// the `real_log` gates — it needs `talents.json` on this machine.
#[test]
#[ignore = "needs the per-machine talents.json (tools/gen-talent-trees.sh)"]
fn real_dataset_lays_out_every_spec() {
    let ds = codec::load().expect("no talents.json on this machine");
    for tree in model::arr(ds.get("trees")) {
        for spec in model::arr(tree.get("specs")) {
            let spec_id = model::get_u64(spec, "specId").unwrap();
            let tv = codec::tree_view(ds, spec_id).unwrap();
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
    let tree = model::arr(ds.get("trees")).first().unwrap();
    let spec_id = model::arr(tree.get("specs"))
        .iter()
        .find_map(|s| model::get_u64(s, "specId"))
        .unwrap();
    let singles: Vec<Json> = model::arr(tree.get("nodes"))
        .iter()
        .filter(|n| {
            model::get_str(n, "type") == "single"
                && model::get_u64(n, "subTreeId").is_none()
                && n.get("visibleFor").is_none()
        })
        .take(3)
        .filter_map(|n| model::get_u64(n, "id"))
        .map(|id| wowdps_proto::json::parse(&format!("{{\"node_id\": {id}}}")).unwrap())
        .collect();
    assert_eq!(singles.len(), 3);
    let enc = codec::encode(ds, spec_id, &singles).unwrap();
    let s = enc.get("string").and_then(Json::as_str).unwrap();
    let (dec_spec, dec_sels, dec_hero, _) = decode_build(s).unwrap();
    let tv = codec::tree_view(ds, dec_spec).unwrap();
    let b = build_model(&tv, &dec_sels, dec_hero, Vec::new()).unwrap();
    let taken: usize = b
        .panes()
        .map(|p| p.nodes.iter().filter(|n| n.selected).count())
        .sum();
    assert_eq!(taken, 3, "the three minted picks survive the layout");

    // Interactive editing against the real tree: click a root onto the
    // empty tree, watch the frontier open, chain a child, then cascade
    // the whole path away with one right-click on the root.
    let mut ui = Viewer::empty();
    ui.spec_id = Some(spec_id);
    ui.rebuild();
    let all = |ui: &Viewer, f: fn(&Node) -> bool| -> Vec<u64> {
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

// ---- the viewer's state machine, over the hook --------------------------

#[test]
fn open_without_a_player_shows_nothing() {
    let _sb = Sandbox::new("open-empty");
    let ui = Viewer::open(None);
    assert!(ui.build.is_none() && ui.player.is_none() && ui.error.is_none());
    assert_eq!(ui.tab, Tab::Talents);
    assert_eq!(ui.encode_current(), None, "no spec, nothing to encode");
    assert!(!ui.pane_full(1), "no build, no full pane");
    assert!(!ui.has_inventory() && ui.hovered().is_none());
    assert!(under_test(), "the hook is this thread's");
}

#[test]
fn open_on_a_meter_row_draws_the_bare_spec_tree() {
    let _sb = Sandbox::new("open-row");
    let ui = Viewer::open(Some(("Frosty-Proudmoore".to_string(), Some(62))));
    assert_eq!(ui.player.as_deref(), Some("Frosty-Proudmoore"));
    assert_eq!(ui.spec_id, Some(62));
    let b = ui.build.as_ref().expect("the spec id alone lays out");
    assert_eq!(
        (b.class_name.as_str(), b.spec_name.as_str()),
        ("Mage", "Arcane")
    );
    assert!(b.hero.is_none() && b.hero_pane.is_none());
    assert!(ui.sels.is_empty() && !ui.edited && !ui.logged);
    // Only the roots are green.
    assert!(node(&ui, 1).available && node(&ui, 3).available);
    assert!(!node(&ui, 2).available && !node(&ui, 6).available);

    // No spec id and nothing stored: an empty viewer, no error.
    let ui = Viewer::open(Some(("Nobody-Realm".to_string(), None)));
    assert!(ui.build.is_none() && ui.error.is_none());
    assert_eq!(ui.player.as_deref(), Some("Nobody-Realm"));
}

#[test]
fn open_restores_a_stored_paste() {
    let sb = Sandbox::new("open-stored");
    let paste = simc_paste("Frosty", Some("proudmoore"), &[&full_string()], true);
    save_stored("Frosty-Proudmoore", &paste);
    assert_eq!(
        sb.stored("Frosty-Proudmoore").as_deref(),
        Some(paste.as_str())
    );
    assert!(
        sb.dir.join("simc/frosty_proudmoore.simc").is_file(),
        "the store lives under the sandbox, never the real data home"
    );

    let ui = Viewer::open(Some(("Frosty-Proudmoore".to_string(), Some(62))));
    let p = ui.profile.as_ref().expect("the stored paste is parsed");
    assert_eq!(p.name.as_deref(), Some("Frosty"));
    assert_eq!(p.equipped.len(), 2);
    assert_eq!(selected_ids(&ui), vec![1, 2, 3, 4, 5]);
    assert_eq!(ui.hero, Some(77));
    assert!(ui.build.as_ref().is_some_and(|b| b.hero_pane.is_some()));
}

#[test]
fn a_pasted_string_decodes_and_round_trips() {
    let _sb = Sandbox::new("paste-string");
    let mut ui = Viewer::open(None);
    let s = full_string();
    ui.ingest(&s);
    assert_eq!(ui.error, None);
    assert_eq!(ui.spec_id, Some(62));
    assert_eq!(selected_ids(&ui), vec![1, 2, 3, 4, 5]);
    assert_eq!(ui.sels.get(&2).and_then(|s| s.choice_index), Some(1));
    assert_eq!(ui.hero, Some(77));
    assert!(ui.profile.is_none() && !ui.edited);
    let b = ui.build.as_ref().unwrap();
    assert_eq!(b.hero, Some((77, "Sunfury".to_string())));
    assert_eq!(b.class_pane.points, 2);
    assert_eq!(b.spec_pane.points, 1);
    assert_eq!(b.hero_pane.as_ref().map(|p| p.points), Some(1));
    // encode is deterministic: the unedited build gives the string back.
    assert_eq!(ui.encode_current().as_deref(), Some(s.as_str()));

    // Garbage keeps the last good build and reports.
    ui.ingest("!!! not a string !!!");
    assert!(ui.error.is_some(), "{:?}", ui.error);
    assert!(ui.build.is_some(), "the previous build stays on screen");
    ui.ingest("   ");
    assert_eq!(ui.error.as_deref(), Some("the clipboard is empty"));
}

#[test]
fn decode_warnings_ride_into_the_build() {
    let _sb = Sandbox::new("paste-warn");
    let mut ui = Viewer::open(None);
    // Trailing junk after the last node: the codec flags the unread bits.
    ui.ingest(&format!("{}AAAA", full_string()));
    assert_eq!(ui.error, None);
    assert!(
        ui.warnings.iter().any(|w| w.contains("unread bits")),
        "{:?}",
        ui.warnings
    );
    let b = ui.build.as_ref().unwrap();
    assert_eq!(b.warnings, ui.warnings);
    // A fresh string clears them.
    ui.ingest(&full_string());
    assert!(ui.warnings.is_empty());
}

#[test]
fn clicks_take_ranks_open_pickers_and_stop_at_the_cap() {
    let _sb = Sandbox::new("clicks");
    let mut ui = Viewer::open(Some(("Frosty-Proudmoore".to_string(), Some(62))));

    ui.click_node(1);
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(1));
    assert!(ui.edited);
    assert!(
        !node(&ui, 2).available,
        "gate at 2 points: one is not enough"
    );
    ui.click_node(1);
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(2));
    assert!(node(&ui, 2).available, "two points above the gate open it");

    // An octagon opens its picker instead of taking a point.
    ui.click_node(2);
    assert_eq!(ui.picker, Some(2));
    assert!(!ui.sels.contains_key(&2));
    // An out-of-range option is ignored (and closes the picker).
    ui.pick_choice(2, 7);
    assert_eq!(ui.picker, None);
    assert!(!ui.sels.contains_key(&2));
    ui.pick_choice(2, 1);
    assert_eq!(ui.sels.get(&2).and_then(|s| s.choice_index), Some(1));
    let n2 = node(&ui, 2);
    assert!(n2.selected && n2.spell_id == 1032);
    assert!(n2.detail.contains("▸ Right"), "{}", n2.detail);
    // Re-picking the other option swaps it in place.
    ui.pick_choice(2, 0);
    assert_eq!(node(&ui, 2).spell_id, 1031);

    // The class pane is at its cap (3): no third rank on node 1.
    assert!(ui.pane_full(1));
    let b = ui.build.as_ref().unwrap();
    assert_eq!((b.class_pane.points, b.class_pane.cap), (3, Some(3)));
    assert!(b.class_pane.full());
    ui.click_node(1);
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(2), "cap holds");
    assert_eq!(points_label(3, Some(3)), ("3/3 pts".to_string(), true));

    // The spec pane is its own budget (cap 2): 3, then 6, then nothing.
    assert!(!ui.pane_full(3));
    ui.click_node(3);
    assert!(node(&ui, 6).available);
    ui.click_node(6);
    assert!(ui.pane_full(6));
    ui.click_node(6);
    assert_eq!(ui.sels.get(&6).map(|s| s.ranks), Some(1), "spec cap holds");
    assert_eq!(selected_ids(&ui), vec![1, 2, 3, 6]);

    // Unknown ids are ignored; refunding the choice frees a class point.
    ui.click_node(999);
    ui.unclick_node(999);
    ui.unclick_node(2);
    assert!(!ui.sels.contains_key(&2));
    assert!(!ui.pane_full(1));
    ui.click_node(1);
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(3));
    assert!(!node(&ui, 1).available, "a taken node is not 'available'");

    // Everything the clicks built encodes and decodes back identically.
    let s = ui.encode_current().expect("encodes");
    let (spec, sels, hero, warnings) = decode_build(&s).unwrap();
    assert_eq!((spec, hero), (62, None));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(sels.get(&1).map(|s| s.ranks), Some(3));
    assert_eq!(sels.get(&6).map(|s| s.ranks), Some(1));
}

#[test]
fn a_gated_choice_cannot_be_picked_early() {
    let _sb = Sandbox::new("gated-choice");
    let mut ui = Viewer::open(Some(("Frosty-Proudmoore".to_string(), Some(62))));
    ui.click_node(2);
    assert_eq!(ui.picker, None, "unavailable octagon: no picker");
    ui.pick_choice(2, 0);
    assert!(ui.sels.is_empty() && !ui.edited);
    // Right-clicking something untaken is a no-op too.
    ui.unclick_node(1);
    assert!(ui.sels.is_empty() && !ui.edited);
}

#[test]
fn a_refund_of_the_root_cascades_the_whole_path() {
    let _sb = Sandbox::new("cascade");
    let mut ui = Viewer::open(None);
    ui.ingest(&full_string());
    // Node 1 holds one rank: the refund removes it, and node 2 — fed
    // by 1 alone — goes with it. The spec and hero halves stay.
    ui.unclick_node(1);
    assert_eq!(selected_ids(&ui), vec![3, 4, 5]);
    assert!(ui.edited);
    assert!(node(&ui, 1).available && !node(&ui, 2).available);
}

#[test]
fn adopt_logged_installs_the_combat_log_build() {
    let _sb = Sandbox::new("logged");
    let pick = |node_id, entry_id, rank| TalentPick {
        node_id,
        entry_id,
        rank,
    };
    let gear = vec![
        GearItem {
            item_id: 212095,
            ilvl: 639,
            enchants: vec![7328],
            bonus_ids: vec![1],
            gems: vec![1, 2],
        },
        GearItem::default(), // an empty slot logs as zeros
        GearItem {
            item_id: 252009,
            ilvl: 600,
            ..GearItem::default()
        },
    ];
    let loadout = Loadout {
        spec_id: Some(62),
        talents: vec![
            pick(1, 101, 2),
            pick(2, 132, 1),
            pick(3, 104, 0), // granted
            pick(4, 151, 1),
            pick(5, 106, 1),
            pick(999, 1, 1), // build drift
        ],
        gear: gear.clone(),
        stats: vec![],
        auras: vec![],
    };

    let mut ui = Viewer::open(None);
    ui.adopt_logged(&loadout);
    assert_eq!(ui.error, None);
    assert!(ui.logged && !ui.edited);
    assert_eq!(ui.logged_gear.as_ref(), Some(&gear));
    assert_eq!(ui.spec_id, Some(62));
    assert_eq!(selected_ids(&ui), vec![1, 2, 3, 4, 5]);
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(2));
    assert_eq!(ui.sels.get(&2).and_then(|s| s.choice_index), Some(1));
    assert!(ui.sels.get(&3).is_some_and(|s| s.granted));
    assert_eq!(ui.hero, Some(77));
    // The drift warning leads the list and reaches the model.
    assert!(
        ui.warnings.first().is_some_and(|w| w.contains("999")),
        "{:?}",
        ui.warnings
    );
    assert_eq!(ui.build.as_ref().unwrap().warnings, ui.warnings);
    // A granted node draws teal and takes no clicks either way.
    let n3 = node(&ui, 3);
    assert!(n3.granted && n3.selected && n3.detail.ends_with("(granted)"));
    assert_eq!(
        ui.build.as_ref().unwrap().spec_pane.points,
        0,
        "granted ranks cost nothing"
    );
    ui.click_node(3);
    ui.unclick_node(3);
    assert!(ui.sels.get(&3).is_some_and(|s| s.granted) && !ui.edited);
    // The granted flag survives the codec.
    let s = ui.encode_current().expect("logged build encodes");
    let (_, sels, _, _) = decode_build(&s).unwrap();
    assert!(sels.get(&3).is_some_and(|s| s.granted));

    // The inventory tab opens on the logged gear alone.
    assert!(ui.has_inventory() && !ui.profile_has_inventory());
    ui.on_msg(Msg::ToggleTab);
    assert_eq!(ui.tab, Tab::Inventory);

    // Loading anything else drops the logged build wholesale — gear
    // included — and, with no simc inventory to show, leaves the tab.
    ui.ingest(&full_string());
    assert!(!ui.logged && ui.logged_gear.is_none());
    assert_eq!(ui.tab, Tab::Talents);
    ui.on_msg(Msg::ToggleTab);
    assert_eq!(ui.tab, Tab::Talents, "no inventory left to flip to");

    // No spec anywhere: nothing to do.
    let mut fresh = Viewer::open(None);
    fresh.adopt_logged(&Loadout::default());
    assert!(!fresh.logged && fresh.build.is_none() && fresh.error.is_none());
    // The viewer's own spec fills in for a loadout without one.
    let mut fresh = Viewer::open(Some(("Frosty-Proudmoore".to_string(), Some(62))));
    fresh.adopt_logged(&Loadout {
        spec_id: None,
        talents: vec![pick(1, 101, 1)],
        gear: Vec::new(),
        stats: vec![],
        auras: vec![],
    });
    assert!(fresh.logged && fresh.logged_gear.is_none());
    assert_eq!(selected_ids(&fresh), vec![1]);
    assert!(fresh.warnings.is_empty(), "{:?}", fresh.warnings);
    // A spec the dataset lacks is an error, not a badge.
    let mut fresh = Viewer::open(None);
    fresh.adopt_logged(&Loadout {
        spec_id: Some(999),
        ..Loadout::default()
    });
    assert!(!fresh.logged);
    assert!(
        fresh.error.as_deref().is_some_and(|e| e.contains("999")),
        "{:?}",
        fresh.error
    );
}

#[test]
fn a_simc_paste_persists_under_both_keys_and_switches_loadouts() {
    let sb = Sandbox::new("simc");
    let (a, b) = (full_string(), string_for(&[r#"{"node_id": 3}"#]));
    let paste = simc_paste("Frosty", Some("proudmoore"), &[&a, &b], true);

    // Opened on a meter row whose realm spelling differs from the paste.
    let mut ui = Viewer::open(Some(("Frosty-Area52".to_string(), Some(62))));
    ui.ingest(&paste);
    assert_eq!(ui.error, None);
    let p = ui.profile.as_ref().expect("profile");
    assert_eq!(p.loadouts.len(), 2);
    assert!(
        sb.stored("Frosty-proudmoore").is_some(),
        "the paste's own key"
    );
    assert!(sb.stored("Frosty-Area52").is_some(), "the row's key too");
    assert_eq!(ui.player.as_deref(), Some("Frosty-Area52"), "row wins");
    assert_eq!(selected_ids(&ui), vec![1, 2, 3, 4, 5]);

    // The second chip decodes the saved loadout.
    ui.on_msg(Msg::SelectLoadout(1));
    assert_eq!(ui.loadout_sel, 1);
    assert_eq!(selected_ids(&ui), vec![3]);
    assert!(ui.hero.is_none());
    // The inventory tab exists now (equipped, bags, currencies).
    assert!(ui.has_inventory() && ui.profile_has_inventory());
    ui.on_msg(Msg::ToggleTab);
    assert_eq!(ui.tab, Tab::Inventory);
    ui.on_msg(Msg::SetTab(Tab::Talents));
    assert_eq!(ui.tab, Tab::Talents);
    // A chip out of range is the "no strings" error.
    ui.on_msg(Msg::SelectLoadout(9));
    assert_eq!(
        ui.error.as_deref(),
        Some("the paste carried no talent strings")
    );

    // Someone else's paste never shadows the row's player.
    let mut other = Viewer::open(Some(("Other-Realm".to_string(), Some(62))));
    other.ingest(&simc_paste("Frosty", None, &[&a], false));
    assert!(sb.stored("Other-Realm").is_none());
    assert!(sb.stored("Frosty").is_some());
    assert_eq!(other.player.as_deref(), Some("Other-Realm"));

    // Opened on nobody: the paste's character becomes the viewed player.
    let mut anon = Viewer::open(None);
    anon.ingest(&simc_paste("Frosty", None, &[&a], false));
    assert_eq!(anon.player.as_deref(), Some("Frosty"));
    assert!(anon.profile.as_ref().is_some_and(|p| p.equipped.is_empty()));
    anon.on_msg(Msg::ToggleTab);
    assert_eq!(anon.tab, Tab::Inventory, "a profile flips even when empty");

    // A paste without strings and one that is not a paste at all.
    let mut bare = Viewer::open(None);
    bare.ingest(&simc_paste("Frosty", None, &[], true));
    assert_eq!(
        bare.error.as_deref(),
        Some("the paste carried no talent strings")
    );
    bare.ingest("what=ever\nnothing=here");
    assert!(
        bare.error
            .as_deref()
            .is_some_and(|e| e.contains("SimulationCraft")),
        "{:?}",
        bare.error
    );
}

#[test]
fn on_msg_routes_every_arm() {
    let _sb = Sandbox::new("msgs");
    let mut ui = Viewer::open(None);
    ui.on_msg(Msg::Input("abc".to_string()));
    assert_eq!(ui.input, "abc");
    ui.on_msg(Msg::Submit);
    assert!(ui.error.is_some(), "'abc' is no talent string");
    ui.on_msg(Msg::Input(full_string()));
    ui.on_msg(Msg::Submit);
    assert_eq!(ui.error, None);
    assert_eq!(selected_ids(&ui), vec![1, 2, 3, 4, 5]);

    ui.on_msg(Msg::Clipboard(None));
    assert_eq!(ui.error.as_deref(), Some("the clipboard is empty"));
    ui.on_msg(Msg::Clipboard(Some(string_for(&[
        r#"{"node_id": 1, "ranks": 1}"#,
    ]))));
    assert_eq!(ui.error, None);
    assert_eq!(selected_ids(&ui), vec![1]);
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(1));

    ui.on_msg(Msg::ToggleTab);
    assert_eq!(ui.tab, Tab::Talents, "no profile, no inventory tab");
    ui.on_msg(Msg::SetTab(Tab::Inventory));
    assert_eq!(ui.tab, Tab::Inventory);
    ui.on_msg(Msg::SetTab(Tab::Talents));

    // Clicks through the message path.
    ui.on_msg(Msg::NodeClick(1));
    assert_eq!(ui.sels.get(&1).map(|s| s.ranks), Some(2));
    ui.on_msg(Msg::NodeClick(2));
    assert_eq!(ui.picker, Some(2));
    ui.on_msg(Msg::PickChoice(2, 0));
    assert_eq!(ui.sels.get(&2).and_then(|s| s.choice_index), Some(0));
    ui.on_msg(Msg::NodeRightClick(2));
    assert!(!ui.sels.contains_key(&2));

    // Hover bookkeeping: hovering another node dismisses the picker,
    // hovering the picker's own options does not; a stale clear from a
    // different node cannot cancel a fresh hover.
    ui.picker = Some(2);
    ui.on_msg(Msg::HoverSet(2, Some(1), 5.0, 6.0));
    assert_eq!(ui.picker, Some(2));
    assert_eq!(ui.hover, Some((2, Some(1))));
    assert_eq!(ui.hover_at, (5.0, 6.0));
    let (tip, requires) = ui.hovered().expect("the picker option's tooltip");
    assert_eq!((tip.name.as_str(), requires.as_str()), ("Right", "Mage"));
    ui.on_msg(Msg::HoverSet(1, None, 7.0, 8.0));
    assert_eq!(ui.picker, None);
    assert_eq!(ui.hover, Some((1, None)));
    assert_eq!(ui.hovered().map(|(n, _)| n.id), Some(1), "a node's tooltip");
    ui.picker = Some(2);
    assert!(ui.hovered().is_none(), "picker open elsewhere: no tooltip");
    ui.on_msg(Msg::ClosePicker);
    assert_eq!(ui.picker, None);
    ui.on_msg(Msg::HoverClear(5));
    assert_eq!(ui.hover, Some((1, None)), "another node's clear is ignored");
    ui.on_msg(Msg::HoverClear(1));
    assert_eq!(ui.hover, None);

    // The host-owned arms change nothing here.
    let before = selected_ids(&ui);
    ui.on_msg(Msg::CopyString);
    ui.on_msg(Msg::PasteClipboard);
    ui.on_msg(Msg::Close);
    assert_eq!(selected_ids(&ui), before);
}

#[test]
fn degenerate_trees_are_errors_not_panics() {
    let _sb = Sandbox::new("degenerate");
    // Only hero nodes: nothing positions the class/spec divide.
    let hero_only = wowdps_proto::json::parse(
        r#"{"trees": [{"treeId": 1, "className": "X",
             "specs": [{"specId": 7, "name": "Y"}],
             "subTrees": [{"id": 9, "name": "H", "specs": [7]}],
             "nodeOrder": [5],
             "nodes": [{"id": 5, "type": "single", "posX": 0, "posY": 0, "subTreeId": 9,
                        "entries": [{"id": 1, "spellId": 1, "name": "n"}]}]}]}"#,
    )
    .unwrap();
    let tv = codec::tree_view(&hero_only, 7).unwrap();
    assert_eq!(
        build_model(&tv, &HashMap::new(), None, Vec::new()).err(),
        Some("tree has no positioned nodes".to_string())
    );
    // One lone node: a class half with no spec half.
    let lone = wowdps_proto::json::parse(
        r#"{"trees": [{"treeId": 1, "className": "X",
             "specs": [{"specId": 7, "name": "Y"}],
             "nodeOrder": [1],
             "nodes": [{"id": 1, "type": "single", "posX": 0, "posY": 0,
                        "entries": [{"id": 1, "spellId": 1, "name": "n"}]}]}]}"#,
    )
    .unwrap();
    let tv = codec::tree_view(&lone, 7).unwrap();
    assert_eq!(
        build_model(&tv, &HashMap::new(), None, Vec::new()).err(),
        Some("tree is missing its class or spec half".to_string())
    );
    // A spec the dataset lacks fails the rebuild with the codec's error.
    let mut ui = Viewer::open(None);
    ui.spec_id = Some(999);
    ui.rebuild();
    assert!(
        ui.error.as_deref().is_some_and(|e| e.contains("999")),
        "{:?}",
        ui.error
    );
    assert!(ui.build.is_none());
}

#[test]
fn a_failed_save_costs_recall_not_data() {
    let sb = Sandbox::new("save-fail");
    // The store root is a plain file: the directory cannot be created.
    std::fs::write(&sb.dir, b"in the way").unwrap();
    save_stored("Frosty-Proudmoore", "paste");
    assert!(sb.stored("Frosty-Proudmoore").is_none());
    std::fs::remove_file(&sb.dir).unwrap();
    // The file's own slot is a directory: the write fails, quietly.
    let path = store_path("Frosty-Proudmoore").unwrap();
    std::fs::create_dir_all(&path).unwrap();
    save_stored("Frosty-Proudmoore", "paste");
    assert!(sb.stored("Frosty-Proudmoore").is_none());
    // And a viewer opened on that player simply gets the bare tree.
    let ui = Viewer::open(Some(("Frosty-Proudmoore".to_string(), Some(62))));
    assert!(ui.profile.is_none() && ui.build.is_some());
    // The paste's own realm-qualified key names the viewed player.
    let mut anon = Viewer::open(None);
    anon.ingest(&simc_paste(
        "Frosty",
        Some("proudmoore"),
        &[&full_string()],
        false,
    ));
    assert_eq!(anon.player.as_deref(), Some("Frosty-proudmoore"));
}

// ---- geometry ----------------------------------------------------------------

#[test]
fn hits_find_nodes_and_the_pickers_tiles_win() {
    let _sb = Sandbox::new("hits");
    let mut ui = Viewer::open(None);
    ui.ingest(&full_string());
    let model = std::rc::Rc::clone(&ui.build.as_ref().unwrap().class_pane);
    let (n1, n2) = (node(&ui, 1), node(&ui, 2));
    let empty = (model.w - 4.0, (n1.y + n2.y) / 2.0);
    assert_eq!(node_at(&model, n1.x, n1.y).map(|n| n.id), Some(1));
    assert!(node_at(&model, empty.0, empty.1).is_none());
    assert_eq!(hit(&model, None, n1.x, n1.y), Some((1, None, (n1.x, n1.y))));
    assert_eq!(hit(&model, None, empty.0, empty.1), None);

    // Node 2's picker: two tiles through the node, the second wins over
    // whatever sits under it.
    let spots = picker_spots(&model, &n2);
    assert_eq!(spots.len(), 2);
    assert!(spots[1].0 > spots[0].0 && spots[0].1 == n2.y);
    let opt = spots[1];
    assert_eq!(option_at(&model, Some(2), opt.0, opt.1), Some((2, 1)));
    assert_eq!(option_at(&model, Some(2), n1.x, n1.y), None);
    assert_eq!(hit(&model, Some(2), opt.0, opt.1), Some((2, Some(1), opt)));
    // A picker for a node this pane does not hold answers nothing.
    assert!(picker_node(&model, Some(3)).is_none());
    assert_eq!(option_at(&model, Some(3), opt.0, opt.1), None);
    let plate = picker_plate(&spots, &n2).unwrap();
    assert!(plate[0] < spots[0].0 && plate[0] + plate[2] > spots[1].0);
    assert!(picker_plate(&[], &n2).is_none());

    // The option view reshapes a choice node into one alternative.
    let v = option_view(&n2, 1).unwrap();
    assert_eq!(
        (v.name.as_str(), v.spell_id, v.cost.as_str()),
        ("Right", 1032, "1 rune")
    );
    assert!(!v.choice && v.options.is_empty());
    assert!(option_view(&n2, 9).is_none());
}

#[test]
fn shapes_badges_and_arrowheads_sit_where_the_game_puts_them() {
    let o = octagon(10.0, 10.0, 10.0);
    // The cut is 29% of the edge, from each corner.
    let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
    assert!(near(o[0], (5.8, 0.0)) && near(o[2], (20.0, 5.8)), "{o:?}");
    // An arrow down a vertical edge: its tip just above the tile.
    let a = arrowhead((0.0, 0.0), (0.0, 100.0));
    assert_eq!(a[0], (0.0, 100.0 - (TILE / 2.0 + 2.5)));
    assert_eq!(a[1].1, a[2].1, "the base is level");
    // A coincident edge still has a direction-free arrow, no NaN.
    assert!(
        arrowhead((5.0, 5.0), (5.0, 5.0))
            .iter()
            .all(|p| p.0.is_finite())
    );

    let _sb = Sandbox::new("badges");
    let mut ui = Viewer::open(None);
    ui.ingest(&full_string());
    let n1 = node(&ui, 1);
    let (words, rect) = badge(&n1);
    assert_eq!(words, "1/3");
    assert_eq!(rect[2], 3.0 * 6.0 + 6.0);
    assert!(rect[0] + rect[2] > n1.x + TILE / 2.0, "over the corner");
    let n2 = node(&ui, 2);
    let [left, right] = carets(&n2);
    assert!(left[0].0 < n2.x - TILE / 2.0 && right[0].0 > n2.x + TILE / 2.0);
    assert_eq!(points_label(2, None), ("2 pts".to_string(), false));
    assert_eq!(points_label(1, Some(3)), ("1/3 pts".to_string(), false));
}

#[test]
fn the_tooltip_lays_out_every_line_kind() {
    let _sb = Sandbox::new("tooltip");
    let mut ui = Viewer::open(None);
    ui.ingest(&full_string());
    let budget = tip_budget(tip_width(400.0));

    // Node 1: cost + range on one line, the cast, "Requires", per-rank
    // descriptions (one reached, two greyed).
    let n1 = node(&ui, 1);
    let lines = tooltip_lines(&n1, "Mage", budget);
    assert_eq!(lines[0].text, "Filler");
    assert_eq!((lines[0].tone, lines[0].size), (Tone::Plain, 13.0));
    assert_eq!(lines[1].tone, Tone::Meta);
    assert_eq!(lines[2].right.as_deref(), Some("40 yd"));
    assert!(lines.iter().any(|l| l.text == "Requires Mage"));
    let ranks: Vec<Tone> = lines
        .iter()
        .filter(|l| l.text.starts_with('('))
        .map(|l| l.tone)
        .collect();
    assert_eq!(ranks, vec![Tone::Desc, Tone::Unreached, Tone::Unreached]);
    assert_eq!(
        tip_height(&lines),
        lines.iter().map(|l| l.size + 4.0).sum::<f32>() + 12.0
    );

    // Node 2: a choice with its alternatives line.
    let n2 = node(&ui, 2);
    let lines = tooltip_lines(&n2, "", budget);
    assert_eq!(lines.last().map(|l| l.text.as_str()), Some("Left / Right"));
    assert!(!lines.iter().any(|l| l.text.starts_with("Requires")));

    // A plain node with a trailing restriction paragraph: blue.
    let mut plain = node(&ui, 3);
    plain.desc = "Does a thing.\nAcross lines.\n\nCurses: only one.".to_string();
    let lines = tooltip_lines(&plain, "Mage", budget);
    assert_eq!(lines.last().map(|l| l.tone), Some(Tone::Note));
    assert!(
        lines
            .iter()
            .any(|l| l.text == "Does a thing. Across lines.")
    );
    // No name: the detail is the title.
    plain.name = String::new();
    plain.detail = "fallback title".to_string();
    assert_eq!(tooltip_lines(&plain, "", budget)[0].text, "fallback title");

    // A tiered node: "Rank N" sections, the unreached one dim.
    let n6 = node(&ui, 6);
    let lines = tooltip_lines(&n6, "Mage", budget);
    assert!(lines.iter().any(|l| l.text == "Rank 2"));
    assert_eq!(
        lines.iter().find(|l| l.text == "Second.").map(|l| l.tone),
        Some(Tone::Unreached)
    );

    // Placement: beside the icon, flipped at the right edge, held inside.
    let (tw, th) = (tip_width(400.0), 100.0);
    assert_eq!(
        tip_origin((40.0, 40.0), tw, th, 400.0, 300.0).0,
        40.0 + TILE / 2.0 + 12.0
    );
    let flipped = tip_origin((395.0, 295.0), tw, th, 400.0, 300.0);
    assert!(flipped.0 + tw < 395.0 && flipped.1 + th <= 298.0);
    assert_eq!(
        tip_width(120.0),
        180.0,
        "a narrow surface keeps a readable box"
    );
}

#[test]
fn text_wraps_at_its_budget() {
    assert_eq!(
        wrap_text("aaa bbb ccc", 7),
        vec!["aaa bbb".to_string(), "ccc".to_string()]
    );
    assert_eq!(wrap_text("   ", 7), Vec::<String>::new());
    assert_eq!(wrap_text("toolongword x", 3), vec!["toolongword", "x"]);
}

#[test]
fn options_fan_around_their_node_inside_the_pane() {
    let wide = PaneModel {
        points: 0,
        cap: None,
        requires: String::new(),
        w: 400.0,
        h: 100.0,
        nodes: Vec::new(),
        edges: Vec::new(),
        retained: Retained::default(),
    };
    let mut n = Node {
        id: 1,
        x: 200.0,
        y: 50.0,
        selected: false,
        granted: false,
        available: false,
        req: 0,
        ranks: 0,
        max_ranks: 1,
        choice: true,
        shape: IconStyle::Octagon,
        spell_id: 0,
        options: Vec::new(),
        tiers: Vec::new(),
        detail: String::new(),
        name: String::new(),
        desc: String::new(),
        cost: String::new(),
        range: String::new(),
        cast: String::new(),
        desc_ranks: Vec::new(),
    };
    assert_eq!(
        picker_spots(&wide, &n).len(),
        1,
        "no options still gets a spot"
    );
    n.options = (0..3)
        .map(|i| ChoiceOption {
            spell_id: i,
            name: format!("o{i}"),
            desc: String::new(),
            cost: String::new(),
            range: String::new(),
            cast: String::new(),
            max_ranks: 1,
            desc_ranks: Vec::new(),
        })
        .collect();
    let spots = picker_spots(&wide, &n);
    assert_eq!(spots.len(), 3);
    assert_eq!(spots[1], (200.0, 50.0), "centered on the node");
    n.x = 2.0;
    assert!(
        picker_spots(&wide, &n)[0].0 >= TILE / 2.0,
        "clamped inside on the left"
    );
    n.x = 398.0;
    assert!(
        picker_spots(&wide, &n)[2].0 <= wide.w - TILE / 2.0,
        "and on the right"
    );
    // A retained slot holds one type, empties on clone, prints flat.
    let slot: &Vec<u8> = wide.retained.get().unwrap();
    assert!(slot.is_empty());
    assert!(wide.retained.get::<String>().is_none(), "one type per slot");
    assert_eq!(format!("{:?}", wide.retained.clone()), "Retained");
}

#[test]
fn the_inventory_words_are_honest() {
    assert_eq!(gear_slot(0, 3), "head");
    assert_eq!(gear_slot(13, 18), "trinket 2");
    assert_eq!(
        gear_slot(0, GEAR_SLOTS.len() + 1),
        "",
        "a dump that does not fit"
    );
    assert_eq!(extras(true, 2), "enchanted · 2 gems");
    assert_eq!(extras(false, 1), "1 gem");
    assert_eq!(extras(false, 0), "");
    let p = crate::simc::parse(&simc_paste("Frosty", Some("proudmoore"), &["x"], true)).unwrap();
    assert_eq!(
        identity(&p),
        "level 80 · arcane · mage · proudmoore · WoW 12.1.0.69497"
    );
    assert_eq!(item_name(&p.equipped[0]), "Abyssal Hood");
    assert_eq!(item_name(&p.equipped[1]), "item 252009");
    let kinds: Vec<&str> = p.currencies.iter().map(currency_kind).collect();
    assert_eq!(kinds, vec!["currency", "item", "catalyst"]);
}
