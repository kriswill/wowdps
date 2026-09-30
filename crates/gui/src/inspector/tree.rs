//! R26: the inspector's ability list as a tree — a player's by-ability
//! rows folded under the groups the daemon names (a pet's abilities under
//! the spell that summoned it, a trinket's effects under the item) and
//! each row split into its (spell id, periodic) parts, flattened into the
//! lines the list draws and the keys walk. A group of one is no group: its
//! row stands alone with the group's name after it. Folds start shut;
//! which are open is the window's (`Gui::tree_open`), never sent anywhere.
//!
//! Window-only.

use std::collections::HashSet;

use wowdps_model::{GroupKind, Row, SpellGroup, SpellMeta, SpellPart, SpellTree};

use crate::table::{self, Col};

use super::list::split_pet;

/// A line's identity: what the keys rest on and what a press names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Node {
    /// A group of two or more rows, by its key.
    Group(String),
    /// A by-ability row, by its index in the breakdown's list.
    Row(usize),
    /// Part `j` (in the drawn order) of row `i`.
    Part(usize, usize),
}

/// One drawn line of the tree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub node: Node,
    /// What the cells draw: the row itself, a group's sum, a part's share
    /// — its `spell_id` the icon.
    pub row: Row,
    /// How far in: 0 at the top, 1 under a group or a row, 2 for a part of
    /// a group's row.
    pub depth: u8,
    /// `Some(open)` when the line folds; `fold_key` is its key in the
    /// window's open set.
    pub fold: Option<bool>,
    pub fold_key: Option<String>,
    /// The ability's own name, and the quieter words after it: the pet
    /// that cast it, the trinket it came from, or which part it is.
    pub name: String,
    pub tail: Option<String>,
    /// The by-ability row a press or Enter opens: a part opens its row's
    /// ability; a group opens nothing (a press folds it).
    pub opens: Option<usize>,
    /// A group's rows, by index — empty on every other line.
    pub members: Vec<usize>,
    /// R26 (step 2): the key of the top-level entry this line belongs to —
    /// its group's, or its row's — what a stacked band is keyed by.
    pub entry: String,
}

/// The open-set key of a group's fold.
pub(crate) fn group_fold(key: &str) -> String {
    format!("g\u{0}{key}")
}

/// The open-set key of a row's parts' fold.
pub(crate) fn row_fold(key: &str) -> String {
    format!("r\u{0}{key}")
}

/// One top-level entry before it is drawn.
enum Entry<'a> {
    Group(&'a SpellGroup, Vec<usize>),
    Row(usize, Option<&'a SpellGroup>),
}

/// The lines `rows` draw as a tree, in the drawn order: the top level
/// sorted by `sort` (a group by its sum) — by amount, largest first, with
/// none, which is the daemon's own order for a plain list — each open
/// group's rows under it in the same order, each open row's parts under
/// it by amount.
pub(crate) fn lines(
    rows: &[Row],
    tree: &SpellTree,
    open: &HashSet<String>,
    sort: Option<(Col, bool)>,
) -> Vec<Line> {
    // The model's entries, so the daemon's stacked series and these lines
    // agree on what a top-level entry is.
    let entries: Vec<Entry> = tree
        .entries(rows)
        .into_iter()
        .filter_map(|e| {
            let group = e.group.as_deref().and_then(|g| tree.group(g));
            match (group, e.members.as_slice()) {
                (Some(g), [_, _, ..]) => Some(Entry::Group(g, e.members)),
                (group, [i]) => Some(Entry::Row(*i, group)),
                _ => None,
            }
        })
        .collect();
    let heads: Vec<(usize, Row)> = entries
        .iter()
        .enumerate()
        .map(|(e, entry)| {
            let head = match entry {
                Entry::Group(g, members) => sum_row(rows, g, members),
                Entry::Row(i, _) => rows.get(*i).cloned().unwrap_or_else(blank),
            };
            (e, head)
        })
        .collect();
    let mut out = Vec::new();
    for (e, head) in order(heads, sort) {
        let Some(entry) = entries.get(e) else {
            continue;
        };
        match entry {
            Entry::Group(g, members) => {
                let fold_key = group_fold(&g.key);
                let is_open = open.contains(&fold_key);
                out.push(Line {
                    node: Node::Group(g.key.clone()),
                    name: g.label.clone(),
                    tail: None,
                    row: head,
                    depth: 0,
                    fold: Some(is_open),
                    fold_key: Some(fold_key),
                    opens: None,
                    members: members.clone(),
                    entry: g.key.clone(),
                });
                if !is_open {
                    continue;
                }
                // A pet's group drops the pet from its members' names when
                // they are all the one pet: the group already says whose.
                let pets: HashSet<Option<&str>> = members
                    .iter()
                    .filter_map(|&i| rows.get(i))
                    .map(|r| split_pet(&r.label).1)
                    .collect();
                let one_pet = g.kind != GroupKind::Item && pets.len() == 1;
                let member_rows: Vec<(usize, Row)> = members
                    .iter()
                    .filter_map(|&i| rows.get(i).map(|r| (i, r.clone())))
                    .collect();
                for (i, r) in order(member_rows, sort) {
                    let (name, pet) = split_pet(&r.label);
                    let (name, tail) = if one_pet || g.kind == GroupKind::Item {
                        (name.to_string(), None)
                    } else {
                        (name.to_string(), pet.map(str::to_string))
                    };
                    row_lines(&mut out, tree, open, i, r, 1, name, tail, &g.key);
                }
            }
            Entry::Row(i, group) => {
                let Some(r) = rows.get(*i) else { continue };
                let (name, pet) = split_pet(&r.label);
                // A group of one keeps the group's word after the row: the
                // pet (already in the label), or the trinket when its name
                // is not the ability's own.
                let tail = match group {
                    Some(g) if g.kind == GroupKind::Item && g.label != name => {
                        Some(g.label.clone())
                    }
                    _ => pet.map(str::to_string),
                };
                row_lines(
                    &mut out,
                    tree,
                    open,
                    *i,
                    r.clone(),
                    0,
                    name.to_string(),
                    tail,
                    &r.key,
                );
            }
        }
    }
    out
}

/// A row's line and, when it is open, its parts'.
#[allow(clippy::too_many_arguments)]
fn row_lines(
    out: &mut Vec<Line>,
    tree: &SpellTree,
    open: &HashSet<String>,
    i: usize,
    r: Row,
    depth: u8,
    name: String,
    tail: Option<String>,
    entry: &str,
) {
    let parts: &[SpellPart] = tree.meta(&r.key).map_or(&[], |m: &SpellMeta| &m.parts);
    let fold_key = (!parts.is_empty()).then(|| row_fold(&r.key));
    let is_open = fold_key.as_ref().is_some_and(|k| open.contains(k));
    let part_rows: Vec<Row> = if is_open {
        part_rows(&r, parts, &name)
    } else {
        Vec::new()
    };
    let words = part_words(parts);
    out.push(Line {
        node: Node::Row(i),
        row: r,
        depth,
        fold: fold_key.as_ref().map(|_| is_open),
        fold_key,
        name: name.clone(),
        tail,
        opens: Some(i),
        members: Vec::new(),
        entry: entry.to_string(),
    });
    let mut drawn: Vec<(usize, Row)> = part_rows.into_iter().enumerate().collect();
    drawn.sort_by_key(|p| std::cmp::Reverse(p.1.amount));
    for (j, p) in drawn {
        out.push(Line {
            node: Node::Part(i, j),
            row: p,
            depth: depth + 1,
            fold: None,
            fold_key: None,
            // The row over it names the ability; a part says which it is,
            // first, where a narrow column cannot cut it off.
            name: words.get(j).map_or_else(|| name.clone(), |w| sentence(w)),
            tail: None,
            opens: Some(i),
            members: Vec::new(),
            entry: entry.to_string(),
        });
    }
}

/// `words` with its first letter a capital, as the window writes every
/// label.
fn sentence(words: &str) -> String {
    let mut c = words.chars();
    c.next()
        .map(|first| first.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Which part each is, in words: "direct" or "over time" where the parts
/// differ in that, else the spell id ("spell 445468").
fn part_words(parts: &[SpellPart]) -> Vec<String> {
    let ticks = parts.iter().filter(|p| p.periodic).count();
    let mixed = ticks > 0 && ticks < parts.len();
    // One part of each kind: the kind alone tells them apart.
    let unique = ticks <= 1 && parts.len() - ticks <= 1;
    parts
        .iter()
        .map(|p| {
            let kind = if p.periodic { "over time" } else { "direct" };
            match (mixed, unique) {
                (true, true) => kind.to_string(),
                (true, false) => format!("{kind}, spell {}", p.spell_id),
                (false, _) => format!("spell {}", p.spell_id),
            }
        })
        .collect()
}

/// Each part as a row the cells can draw: its own figures, its share of
/// the player (the row's share, split by amount), the part's spell icon.
fn part_rows(r: &Row, parts: &[SpellPart], name: &str) -> Vec<Row> {
    parts
        .iter()
        .map(|p| {
            let frac = if r.amount == 0 {
                0.0
            } else {
                p.amount as f64 / r.amount as f64
            };
            Row {
                key: format!("{}\u{1}{}{}", r.key, p.spell_id, u8::from(p.periodic)),
                label: name.to_string(),
                amount: p.amount,
                extra: p.extra,
                count: p.count,
                crits: p.crits,
                per_sec: r.per_sec * frac,
                pct: r.pct * frac,
                spell_id: if p.spell_id == 0 {
                    r.spell_id
                } else {
                    p.spell_id
                },
                ..r.clone()
            }
        })
        .collect()
}

/// A group's line as a row: its members summed, the group's icon (else
/// its largest member's), its label.
fn sum_row(rows: &[Row], g: &SpellGroup, members: &[usize]) -> Row {
    let members: Vec<&Row> = members.iter().filter_map(|&i| rows.get(i)).collect();
    let top = members.iter().max_by_key(|r| r.amount).copied();
    let mut sum = top.cloned().unwrap_or_else(blank);
    sum.key = format!("group\u{0}{}", g.key);
    sum.label = g.label.clone();
    sum.amount = members.iter().map(|r| r.amount).sum();
    sum.extra = members.iter().map(|r| r.extra).sum();
    sum.count = members.iter().map(|r| r.count).sum();
    sum.crits = members.iter().map(|r| r.crits).sum();
    sum.per_sec = members.iter().map(|r| r.per_sec).sum();
    sum.pct = members.iter().map(|r| r.pct).sum();
    if g.spell_id != 0 {
        sum.spell_id = g.spell_id;
    }
    sum
}

fn blank() -> Row {
    Row {
        key: String::new(),
        label: String::new(),
        amount: 0,
        extra: 0,
        count: 0,
        crits: 0,
        per_sec: 0.0,
        pct: 0.0,
        class: None,
        spec: None,
        hp: None,
        gain: false,
        spell_id: 0,
        enemy: false,
        school: 0,
        mine: false,
        offset_ms: None,
    }
}

/// `table::sorted`, but with no sort the largest first — the daemon's own
/// order for plain rows, and where a group's sum belongs among them.
fn order(rows: Vec<(usize, Row)>, sort: Option<(Col, bool)>) -> Vec<(usize, Row)> {
    match sort {
        Some(_) => table::sorted(rows, sort),
        None => {
            let mut rows = rows;
            rows.sort_by_key(|r| std::cmp::Reverse(r.1.amount));
            rows
        }
    }
}

/// Where the keys rest in `lines`: `cursor` when it is drawn; else the
/// row `sel` names; else, when that row is folded away, the line that
/// holds it (its group, or the row whose part it was); else the first.
pub(crate) fn keyed(lines: &[Line], cursor: Option<&Node>, sel: usize) -> Option<usize> {
    if let Some(c) = cursor
        && let Some(at) = lines.iter().position(|l| &l.node == c)
    {
        return Some(at);
    }
    let want = match cursor {
        Some(Node::Part(i, _)) => *i,
        _ => sel,
    };
    if let Some(at) = lines.iter().position(|l| l.node == Node::Row(want)) {
        return Some(at);
    }
    // Folded away: the shut group that sums it.
    lines
        .iter()
        .position(|l| l.members.contains(&want))
        .or_else(|| (!lines.is_empty()).then_some(0))
}

/// The line one step from `at` in `lines`, down or up, clamped at the
/// ends.
pub(crate) fn step(lines: &[Line], at: Option<usize>, down: bool) -> Option<usize> {
    let last = lines.len().checked_sub(1)?;
    Some(match at {
        None => 0,
        Some(a) if down => (a + 1).min(last),
        Some(a) => a.saturating_sub(1),
    })
}

/// The line that holds line `at` — the group over a member, the row over a
/// part — found as the nearest line above it one level out.
pub(crate) fn parent(lines: &[Line], at: usize) -> Option<usize> {
    let depth = lines.get(at)?.depth;
    if depth == 0 {
        return None;
    }
    (0..at)
        .rev()
        .find(|&i| lines.get(i).is_some_and(|l| l.depth < depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, amount: u64, spell_id: u32) -> Row {
        Row {
            key: key.to_string(),
            label: key.replace('\u{0}', " (").to_string()
                + if key.contains('\u{0}') { ")" } else { "" },
            amount,
            count: amount / 10,
            spell_id,
            pct: amount as f64 / 10.0,
            ..blank()
        }
    }

    fn group(key: &str, label: &str, kind: GroupKind) -> SpellGroup {
        SpellGroup {
            key: key.to_string(),
            label: label.to_string(),
            spell_id: 7,
            kind,
        }
    }

    fn meta(key: &str, group: &str, parts: Vec<SpellPart>) -> SpellMeta {
        SpellMeta {
            key: key.to_string(),
            group: group.to_string(),
            casts: 0,
            parts,
            misses: 0,
            uptime_ms: 0,
        }
    }

    fn part(spell_id: u32, periodic: bool, amount: u64) -> SpellPart {
        SpellPart {
            spell_id,
            periodic,
            amount,
            extra: 0,
            count: 1,
            crits: 0,
        }
    }

    /// A warlock's drill: Chaos Bolt alone, Wither in two parts, the
    /// Sayaad's two abilities under its summon, one trinket proc.
    fn fixture() -> (Vec<Row>, SpellTree) {
        let rows = vec![
            row("Chaos Bolt", 780, 1),
            row("Wither", 110, 2),
            row("Araz's Ritual Forge", 80, 3),
            row("Lash of Pain\u{0}Sayaad", 36, 4),
            row("Melee\u{0}Sayaad", 12, 0),
        ];
        let mut tree = SpellTree {
            groups: vec![
                group("item:1", "Araz's Ritual Forge", GroupKind::Item),
                group("summon:Summon Sayaad", "Summon Sayaad", GroupKind::Summon),
            ],
            rows: vec![
                meta("Araz's Ritual Forge", "item:1", vec![]),
                meta("Lash of Pain\u{0}Sayaad", "summon:Summon Sayaad", vec![]),
                meta("Melee\u{0}Sayaad", "summon:Summon Sayaad", vec![]),
                meta(
                    "Wither",
                    "",
                    vec![part(445468, false, 50), part(445474, true, 60)],
                ),
            ],
        };
        tree.rows.sort_by(|a, b| a.key.cmp(&b.key));
        (rows, tree)
    }

    type Drawn = (u8, String, Option<String>, Option<bool>, u64);

    fn drawn(lines: &[Line]) -> Vec<Drawn> {
        lines
            .iter()
            .map(|l| {
                (
                    l.depth,
                    l.name.clone(),
                    l.tail.clone(),
                    l.fold,
                    l.row.amount,
                )
            })
            .collect()
    }

    #[test]
    fn shut_folds_draw_the_top_level_largest_first() {
        let (rows, tree) = fixture();
        let lines = lines(&rows, &tree, &HashSet::new(), None);
        assert_eq!(
            drawn(&lines),
            vec![
                (0, "Chaos Bolt".into(), None, None, 780),
                (0, "Wither".into(), None, Some(false), 110),
                (0, "Araz's Ritual Forge".into(), None, None, 80),
                (0, "Summon Sayaad".into(), None, Some(false), 48),
            ]
        );
        assert_eq!(lines[3].row.spell_id, 7, "the group's own icon");
        assert!((lines[3].row.pct - 4.8).abs() < 1e-9, "shares sum");
        assert_eq!(lines[3].opens, None, "a group opens no ability");
    }

    #[test]
    fn open_folds_nest_members_and_parts() {
        let (rows, tree) = fixture();
        let open: HashSet<String> = [group_fold("summon:Summon Sayaad"), row_fold("Wither")]
            .into_iter()
            .collect();
        let lines = lines(&rows, &tree, &open, None);
        assert_eq!(
            drawn(&lines),
            vec![
                (0, "Chaos Bolt".into(), None, None, 780),
                (0, "Wither".into(), None, Some(true), 110),
                (1, "Over time".into(), None, None, 60),
                (1, "Direct".into(), None, None, 50),
                (0, "Araz's Ritual Forge".into(), None, None, 80),
                (0, "Summon Sayaad".into(), None, Some(true), 48),
                (1, "Lash of Pain".into(), None, None, 36),
                (1, "Melee".into(), None, None, 12),
            ]
        );
        assert_eq!(lines[2].node, Node::Part(1, 1));
        assert_eq!(lines[2].row.spell_id, 445474, "a part wears its own icon");
        assert_eq!(lines[2].opens, Some(1), "a part opens its row");
        assert_eq!(parent(&lines, 3), Some(1));
        assert_eq!(parent(&lines, 7), Some(5));
        assert_eq!(parent(&lines, 5), None);
    }

    /// A group of one is its row, the trinket named after it when its name
    /// is not the ability's; a pet keeps its name after its ability.
    #[test]
    fn a_group_of_one_is_its_row() {
        let rows = vec![
            row("Fel Firebolt\u{0}Wild Imp", 50, 1),
            row("Rune Blast", 40, 2),
        ];
        let tree = SpellTree {
            groups: vec![
                group("item:9", "Omnium Folio", GroupKind::Item),
                group("summon:Wild Imp", "Wild Imp", GroupKind::Summon),
            ],
            rows: vec![
                meta("Fel Firebolt\u{0}Wild Imp", "summon:Wild Imp", vec![]),
                meta("Rune Blast", "item:9", vec![]),
            ],
        };
        let lines = lines(&rows, &tree, &HashSet::new(), None);
        assert_eq!(
            drawn(&lines),
            vec![
                (0, "Fel Firebolt".into(), Some("Wild Imp".into()), None, 50),
                (
                    0,
                    "Rune Blast".into(),
                    Some("Omnium Folio".into()),
                    None,
                    40
                ),
            ]
        );
    }

    /// A sort orders the top level by the group's sum and the members
    /// within it the same way.
    #[test]
    fn a_sort_orders_groups_by_their_sums() {
        let (rows, tree) = fixture();
        let open: HashSet<String> = [group_fold("summon:Summon Sayaad")].into_iter().collect();
        let lines = lines(&rows, &tree, &open, Some((Col::Amount, false)));
        let names: Vec<&str> = lines.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Summon Sayaad",
                "Melee",
                "Lash of Pain",
                "Araz's Ritual Forge",
                "Wither",
                "Chaos Bolt"
            ]
        );
    }

    /// The keys rest on the cursor when drawn; a row folded away puts
    /// them on its group; a step clamps at the ends.
    #[test]
    fn the_keys_find_their_line() {
        let (rows, tree) = fixture();
        let shut = lines(&rows, &tree, &HashSet::new(), None);
        assert_eq!(keyed(&shut, None, 0), Some(0));
        assert_eq!(keyed(&shut, None, 3), Some(3), "Lash of Pain's shut group");
        assert_eq!(
            keyed(&shut, Some(&Node::Group("summon:Summon Sayaad".into())), 0),
            Some(3)
        );
        assert_eq!(
            keyed(&shut, Some(&Node::Part(1, 0)), 0),
            Some(1),
            "a shut part's row"
        );
        assert_eq!(step(&shut, Some(3), true), Some(3));
        assert_eq!(step(&shut, Some(0), false), Some(0));
        assert_eq!(step(&shut, Some(1), true), Some(2));
        assert_eq!(step(&[], Some(1), true), None);
    }

    /// Parts that differ only by id say which spell; mixed kinds say
    /// "direct" and "over time", with the id when a kind repeats.
    #[test]
    fn parts_say_which_they_are() {
        assert_eq!(
            part_words(&[part(1, false, 1), part(2, false, 1)]),
            ["spell 1", "spell 2"]
        );
        assert_eq!(
            part_words(&[part(1, false, 1), part(1, true, 1)]),
            ["direct", "over time"]
        );
        assert_eq!(
            part_words(&[part(1, false, 1), part(2, false, 1), part(2, true, 1)]),
            ["direct, spell 1", "direct, spell 2", "over time, spell 2"]
        );
    }
}
