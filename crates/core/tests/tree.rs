//! R26: the ability tree — how a player's by-ability rows nest — over the
//! tree fixture (`tree.expected.md`) and every committed one: a pet's rows
//! under the spell that summoned it, a trinket's under the item, a label's
//! rows split by (spell id, periodic) into parts that sum to the row, casts
//! on the row they name (passive: a precast, a cast after the kill, a
//! hostile cast and one in the trash dead zone land nowhere), a hunter's
//! "Call Pet N" grouped by the pet, Damage and Healing only, lazy = full,
//! and the R10 merge. Every fixture in `FIXTURES` must exist — a missing
//! one fails, never skips.

use std::path::Path;

use wowdps_core::index::{load_segment, scan};
use wowdps_core::meter::{Meter, Segment, View, meter_from_lines};
use wowdps_model::{GroupKind, Row, SpellGroup, SpellPart, SpellTree};

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
    "support.txt",
    "spans.txt",
    "shields.txt",
    "stacks.txt",
    "tree.txt",
];

/// The tree fixture's roster (see `tree.expected.md`).
const W: &str = "Player-1168-0A1B2C61"; // Destruction Warlock: pets, Wither, trinkets
const P: &str = "Player-1168-0A1B2C62"; // Priest: Shadow Word: Pain, Renew

fn fixture_path(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn read(name: &str) -> String {
    let text = std::fs::read_to_string(fixture_path(name));
    assert!(
        text.is_ok(),
        "{name}: unreadable fixture: {:?}",
        text.as_ref().err()
    );
    text.unwrap_or_default()
}

fn replay(text: &str) -> Meter {
    meter_from_lines(text.lines())
}

fn tree_fight() -> Meter {
    replay(&read("tree.txt"))
}

fn part(spell_id: u32, periodic: bool, amount: u64, count: u64, crits: u64) -> SpellPart {
    SpellPart {
        spell_id,
        periodic,
        amount,
        extra: 0,
        count,
        crits,
    }
}

fn group(key: &str, label: &str, spell_id: u32, kind: GroupKind) -> SpellGroup {
    SpellGroup {
        key: key.to_string(),
        label: label.to_string(),
        spell_id,
        kind,
    }
}

/// (group, casts, parts) of the row keyed `key`, or the empty answer for a
/// row the tree has nothing to say about.
fn meta(tree: &SpellTree, key: &str) -> (String, u64, Vec<SpellPart>) {
    tree.meta(key).map_or_else(
        || (String::new(), 0, Vec::new()),
        |m| (m.group.clone(), m.casts, m.parts.clone()),
    )
}

// ---- the tree fixture -------------------------------------------------------

/// The pets hang under the spells that summoned them, both trinkets under
/// their items, and nothing else is grouped.
#[test]
fn a_warlock_s_rows_nest_under_summons_and_trinkets() {
    let m = tree_fight();
    let seg = &m.segments()[0];
    let tree = seg.spell_tree(W, View::Damage);
    assert_eq!(
        tree.groups,
        vec![
            group("item:242394", "Eradicating Arcanocore", 0, GroupKind::Item),
            group("item:242402", "Araz's Ritual Forge", 0, GroupKind::Item),
            group(
                "summon:Summon Infernal",
                "Summon Infernal",
                1122,
                GroupKind::Summon
            ),
            group(
                "summon:Summon Sayaad",
                "Summon Sayaad",
                366222,
                GroupKind::Summon
            ),
        ]
    );
    for (key, want) in [
        ("Melee\u{0}Sayaad", "summon:Summon Sayaad"),
        ("Lash of Pain\u{0}Sayaad", "summon:Summon Sayaad"),
        ("Immolation\u{0}Infernal", "summon:Summon Infernal"),
        ("Melee\u{0}Infernal", "summon:Summon Infernal"),
        ("Araz's Ritual Forge", "item:242402"),
        ("Eradicating Arcanocore", "item:242394"),
        ("Chaos Bolt", ""),
        ("Wither", ""),
    ] {
        assert_eq!(meta(&tree, key).0, want, "{key}");
    }
}

/// Wither's hit and its tick are two ids under one label; the Priest's
/// Shadow Word: Pain is ONE id that lands both ways; the trinket's two
/// effects share its name. Each splits into parts; a one-part row has none.
#[test]
fn a_label_splits_by_id_and_by_tick() {
    let m = tree_fight();
    let seg = &m.segments()[0];
    let w = seg.spell_tree(W, View::Damage);
    assert_eq!(
        meta(&w, "Wither").2,
        vec![
            part(445468, false, 50_000, 1, 0),
            part(445474, true, 60_000, 2, 1),
        ]
    );
    assert_eq!(
        meta(&w, "Araz's Ritual Forge").2,
        vec![
            part(1232797, false, 60_000, 1, 0),
            part(1232802, true, 20_000, 1, 0),
        ]
    );
    assert!(meta(&w, "Chaos Bolt").2.is_empty(), "one part is no split");
    let p = seg.spell_tree(P, View::Damage);
    assert_eq!(
        meta(&p, "Shadow Word: Pain").2,
        vec![
            part(589, false, 15_000, 1, 0),
            part(589, true, 16_000, 2, 0)
        ]
    );
}

/// Casts land on the row they name — a pet's on its own row — and a cast
/// with no row under it (Summon Infernal did no damage) is in the total
/// but on no row.
#[test]
fn casts_count_on_the_row_they_name() {
    let m = tree_fight();
    let seg = &m.segments()[0];
    let tree = seg.spell_tree(W, View::Damage);
    assert_eq!(
        meta(&tree, "Chaos Bolt").1,
        2,
        "the precast and the post-kill cast land nowhere"
    );
    assert_eq!(meta(&tree, "Wither").1, 1);
    assert_eq!(meta(&tree, "Lash of Pain\u{0}Sayaad").1, 2);
    assert_eq!(meta(&tree, "Melee\u{0}Sayaad").1, 0, "a swing is no cast");
    assert_eq!(
        meta(&tree, "Eradicating Arcanocore").1,
        0,
        "a proc is no cast"
    );
    assert!(
        tree.meta("Summon Infernal").is_none(),
        "no damage row, no meta"
    );
    assert_eq!(seg.casts(W), 6, "…but it is one of the player's six");
    assert_eq!(seg.casts(P), 3);
    // The trash: the cast before the first hit and the one in the dead zone
    // are nobody's; each pull keeps the one after its opening hit.
    let trash: Vec<&Segment> = m.segments().iter().skip(1).collect();
    assert_eq!(trash.len(), 2);
    for s in trash {
        assert_eq!(s.casts(W), 1);
        assert_eq!(meta(&s.spell_tree(W, View::Damage), "Incinerate").1, 1);
    }
}

/// Healing nests the same way: Renew's instant heal and its ticks are two
/// parts of one row, each cast counted.
#[test]
fn healing_rows_split_the_same_way() {
    let m = tree_fight();
    let seg = &m.segments()[0];
    let tree = seg.spell_tree(P, View::Healing);
    let (group, casts, parts) = meta(&tree, "Renew");
    assert_eq!((group.as_str(), casts), ("", 1));
    let mut want = vec![
        part(139, false, 20_000, 1, 0),
        part(139, true, 18_000, 2, 0),
    ];
    want[1].extra = 2_000;
    assert_eq!(parts, want);
    assert_eq!(meta(&tree, "Flash Heal").1, 1);
    assert_eq!(seg.periodic_amount(P, View::Healing), 18_000);
}

/// The count views and the destination views have no tree.
#[test]
fn only_damage_and_healing_have_a_tree() {
    let m = tree_fight();
    let seg = &m.segments()[0];
    for view in View::ALL {
        let tree = seg.spell_tree(W, view);
        match view {
            View::Damage => assert!(!tree.is_empty()),
            View::Healing => {}
            _ => assert!(tree.is_empty(), "{view:?}"),
        }
    }
}

// ---- every fixture ----------------------------------------------------------

/// The shape every tree keeps: each meta names a by-ability row that
/// exists, its parts (two or more) sum to that row exactly, each group it
/// names is listed and each listed group is named, and both lists are
/// sorted by key.
fn check_tree(what: &str, rows: &[Row], tree: &SpellTree) {
    for m in &tree.rows {
        let row = rows.iter().find(|r| r.key == m.key);
        assert!(row.is_some(), "{what}: meta for a missing row {:?}", m.key);
        let Some(row) = row else { continue };
        if !m.parts.is_empty() {
            assert!(m.parts.len() >= 2, "{what}: a single part {:?}", m.key);
            let sum = |f: fn(&SpellPart) -> u64| m.parts.iter().map(f).sum::<u64>();
            assert_eq!(sum(|p| p.amount), row.amount, "{what}: {:?} amount", m.key);
            assert_eq!(sum(|p| p.extra), row.extra, "{what}: {:?} extra", m.key);
            assert_eq!(sum(|p| p.count), row.count, "{what}: {:?} count", m.key);
            assert_eq!(sum(|p| p.crits), row.crits, "{what}: {:?} crits", m.key);
        }
        if !m.group.is_empty() {
            assert!(tree.group(&m.group).is_some(), "{what}: {:?}", m.group);
        }
    }
    for g in &tree.groups {
        assert!(
            tree.rows.iter().any(|m| m.group == g.key),
            "{what}: group {:?} names no row",
            g.key
        );
    }
    assert!(tree.rows.is_sorted_by(|a, b| a.key < b.key), "{what}");
    assert!(tree.groups.is_sorted_by(|a, b| a.key < b.key), "{what}");
}

fn check_segment(what: &str, seg: &Segment) -> usize {
    let mut checked = 0;
    for view in [View::Damage, View::Healing] {
        for player in seg.rows(view) {
            let (by_spell, _) = seg.breakdown(&player.key, view);
            let tree = seg.spell_tree(&player.key, view);
            check_tree(
                &format!("{what} / {view:?} / {}", player.label),
                &by_spell,
                &tree,
            );
            checked += tree.rows.len();
        }
    }
    checked
}

#[test]
fn every_tree_names_real_rows_and_its_parts_sum_to_them() {
    let mut checked = 0;
    for name in FIXTURES {
        let meter = replay(&read(name));
        for seg in meter.segments() {
            checked += check_segment(&format!("{name} / {}", seg.name), seg);
        }
        for (ordinal, _) in meter.visits().iter().enumerate() {
            if let Some(ov) = meter.overall(ordinal as u32) {
                checked += check_segment(&format!("{name} / Σ{ordinal}"), &ov);
            }
        }
    }
    assert!(checked > 0, "some tree carries rows");
}

/// sample.txt's hunter summons Sharptooth with "Call Pet 1": the button's
/// name says nothing, so the group is the pet's, wearing the spell's icon.
#[test]
fn a_hunter_s_called_pet_groups_by_the_pet() {
    let meter = replay(&read("sample.txt"));
    let hunter = "Player-1168-0A1B2C03";
    let mut seen = false;
    for seg in meter.segments() {
        let tree = seg.spell_tree(hunter, View::Damage);
        if let Some(g) = tree.group("pet:Sharptooth") {
            assert_eq!(
                *g,
                group("pet:Sharptooth", "Sharptooth", 883, GroupKind::Pet)
            );
            seen = true;
        }
        assert!(tree.groups.iter().all(|g| !g.key.starts_with("summon:")));
    }
    assert!(seen, "Sharptooth fights in sample.txt");
}

/// A lazily loaded segment (its slice plus the seed lines) answers the
/// same tree as the full replay — summons are seeds, casts are passive.
#[test]
fn the_tree_survives_lazy_loading_on_every_fixture() {
    let mut compared = 0;
    for name in FIXTURES {
        let text = read(name);
        let path = fixture_path(name);
        let idx = scan(&mut text.as_bytes());
        let full = replay(&text);
        let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
        assert_eq!(metas.len(), full.segments().len(), "{name}: segment count");
        let picture = |seg: &Segment| -> Vec<(String, View, SpellTree, u64)> {
            let mut out = Vec::new();
            for view in [View::Damage, View::Healing] {
                for r in seg.rows(view) {
                    out.push((
                        r.key.clone(),
                        view,
                        seg.spell_tree(&r.key, view),
                        seg.casts(&r.key),
                    ));
                }
            }
            out.sort_by(|a, b| (&a.0, a.1.index()).cmp(&(&b.0, b.1.index())));
            out
        };
        for (meta, seg) in metas.iter().zip(full.segments()) {
            let lines = load_segment(Path::new(&path), meta);
            assert!(lines.is_ok(), "{name}: slice loads");
            let lines = lines.unwrap_or_default();
            let lazy = meter_from_lines(lines.iter().map(String::as_str));
            let want = picture(seg);
            compared += want.len();
            assert_eq!(picture(&lazy.segments()[0]), want, "{name} / {}", meta.name);
        }
    }
    assert!(compared > 0);
}

/// R10: an Overall's tree is its members' — casts and parts summed, the
/// groups the same.
#[test]
fn an_overall_sums_its_members_casts_and_parts() {
    let m = tree_fight();
    assert_eq!(m.visits().len(), 1);
    let ov = m.overall(0);
    assert!(ov.is_some());
    let Some(ov) = ov else { return };
    assert_eq!(ov.casts(W), 8, "6 in the pull, 1 in each trash");
    let tree = ov.spell_tree(W, View::Damage);
    assert_eq!(meta(&tree, "Incinerate").1, 2);
    assert_eq!(
        meta(&tree, "Wither").2,
        vec![
            part(445468, false, 50_000, 1, 0),
            part(445474, true, 60_000, 2, 1),
        ]
    );
    assert_eq!(tree.groups.len(), 4);
}

/// R26 on a real log: every segment's every Damage and Healing tree keeps
/// its shape (each meta names a row, parts sum to it, groups named and
/// listed), and the tree's casts on the rows never exceed the player's.
///
/// Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
/// -p wowdps-core --test tree -- --ignored --nocapture`
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn every_real_tree_keeps_its_shape() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let text = std::fs::read_to_string(&path).expect("read the log");
    let meter = replay(&text);
    let (mut trees, mut groups, mut split) = (0, 0, 0);
    for seg in meter.segments() {
        for view in [View::Damage, View::Healing] {
            for player in seg.rows(view) {
                let (by_spell, _) = seg.breakdown(&player.key, view);
                let tree = seg.spell_tree(&player.key, view);
                check_tree(
                    &format!("{} / {view:?} / {}", seg.name, player.label),
                    &by_spell,
                    &tree,
                );
                let on_rows: u64 = tree.rows.iter().map(|m| m.casts).sum();
                assert!(on_rows <= seg.casts(&player.key), "{}", player.label);
                trees += 1;
                groups += tree.groups.len();
                split += tree.rows.iter().filter(|m| !m.parts.is_empty()).count();
            }
        }
    }
    println!("{trees} trees, {groups} groups, {split} split rows");
    assert!(groups > 0 && split > 0, "a real log nests something");
}

// ---- step 2: the stacked graph's series ------------------------------------

/// Every entry's curve stacked is the player's own curve, bucket for
/// bucket — Damage against `timeline`, Healing against `heal_timeline` —
/// on every fixture's segments and Overalls.
#[test]
fn the_stack_sums_to_the_player_s_curve_on_every_fixture() {
    let mut checked = 0;
    for name in FIXTURES {
        let meter = replay(&read(name));
        let mut segs: Vec<Segment> = meter.segments().to_vec();
        for (ordinal, _) in meter.visits().iter().enumerate() {
            segs.extend(meter.overall(ordinal as u32));
        }
        for seg in &segs {
            for view in [View::Damage, View::Healing] {
                for player in seg.rows(view) {
                    let (rows, _) = seg.breakdown(&player.key, view);
                    let tree = seg.spell_tree(&player.key, view);
                    let stack = seg.ability_series(&player.key, view, &rows, &tree, usize::MAX);
                    let whole = match view {
                        View::Damage => seg.timeline(&player.key).buckets,
                        _ => seg.heal_timeline(&player.key).buckets,
                    };
                    let len = stack.iter().map(|s| s.buckets.len()).max().unwrap_or(0);
                    let mut sum = vec![0u64; len.max(whole.len())];
                    for s in &stack {
                        for (i, b) in s.buckets.iter().enumerate() {
                            sum[i] += b;
                        }
                    }
                    let mut whole = whole;
                    whole.resize(sum.len(), 0);
                    assert_eq!(
                        sum, whole,
                        "{name} / {} / {view:?} / {}",
                        seg.name, player.label
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 0);
}

/// The tree fixture's stack: the six entries largest first, keyed as the
/// tree keys them (a group by its key); an open Chaos Bolt splits onto the
/// one enemy it hit; the healer's two heals.
#[test]
fn the_stack_is_the_tree_s_largest_entries() {
    let m = tree_fight();
    let seg = &m.segments()[0];
    let (rows, _) = seg.breakdown(W, View::Damage);
    let tree = seg.spell_tree(W, View::Damage);
    let keys: Vec<String> = seg
        .ability_series(W, View::Damage, &rows, &tree, 6)
        .into_iter()
        .map(|s| s.key)
        .collect();
    assert_eq!(
        keys,
        [
            "Chaos Bolt",
            "Wither",
            "Araz's Ritual Forge",
            "summon:Summon Sayaad",
            "Eradicating Arcanocore",
            "summon:Summon Infernal",
        ]
    );
    let top2 = seg.ability_series(W, View::Damage, &rows, &tree, 2);
    assert_eq!(top2.len(), 2, "the rest is the reader's Other");
    let targets = seg.target_series(W, "Chaos Bolt", 6);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].key, "Tree Test Boss");
    assert_eq!(targets[0].buckets.iter().sum::<u64>(), 780_000);
    let lash = seg.target_series(W, "Lash of Pain\u{0}Sayaad", 6);
    assert_eq!(
        lash[0].buckets.iter().sum::<u64>(),
        36_000,
        "a pet's ability"
    );
    let (heals, _) = seg.breakdown(P, View::Healing);
    let tree = seg.spell_tree(P, View::Healing);
    let keys: Vec<String> = seg
        .ability_series(P, View::Healing, &heals, &tree, 6)
        .into_iter()
        .map(|s| s.key)
        .collect();
    assert_eq!(keys, ["Renew", "Flash Heal"]);
    assert!(
        seg.ability_series(W, View::Taken, &rows, &tree, 6)
            .is_empty()
    );
}
