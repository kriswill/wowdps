//! R29's boss rule over a real log: every closed boss pull and keystone run
//! is cut as the store cuts it, and its hostiles are read again, here and
//! independently, against the encounter titles its boss rows name.
//! Printed, as counts and encounter ids only:
//! - how often the exact rule (a hostile named as the title, or as a piece
//!   its commas and lone "and"s divide it into) names nobody;
//! - in those cuts, the hostile names one edit from a title piece (a
//!   transposition, an insertion, a deletion or a substitution; case and
//!   punctuation folded; both 5+ letters): none, one or several, and where
//!   one, whether the cut took it (its units the only bosses) or refused
//!   it as the rule says (the cut opens several encounters, or the name
//!   is not the strongest posted hostile, read here from the lines);
//! - how many units the cut calls bosses there;
//! - two distinct hostiles one edit apart anywhere, and hostiles one edit
//!   from a title the exact rule already matched.
//!
//! Held: a near-miss name is either taken alone or refused for a reason
//! the rule gives. The title pieces are the cut's own (`title_parts`).
//! Measured over this machine's 87 logs on 2026-11-13, before the near
//! miss existed: the exact rule named nobody in 510 of 1,456 Retail cuts
//! with hostiles and 2 of 18 Forever pulls; the near miss found one name
//! in those 2 (encounter 2914, the strongest hostile both times) and none
//! in the 510; no two hostiles were one edit apart.
//!
//! A real log carries real names: nothing here prints a name or a guid.
//!
//! Run: `WOWDPS_REAL_LOG=<Retail log> WOWDPS_REAL_LOG_FOREVER=<Forever log>
//! cargo test --release -p wowdps-core --test real_log_boss_names --
//! --ignored --nocapture`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod real_log;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use wowdps_core::index::{SegmentMeta, load_segment_text, scan};
use wowdps_core::meter::SegmentKind;
use wowdps_core::model::replay::{EventKind, UnitKind};
use wowdps_core::parser::parse_line;
use wowdps_core::replay::{NoPlaced, cut, title_parts};

/// The hostiles of a slice as its own lines show them, read here apart
/// from the cut: each guid's first name and its most health over the
/// blocks that post it (a readable place, map 0 only where the edition
/// takes it for one; a player, a creature or a vehicle).
fn hostiles_of<'a>(
    slice: impl Iterator<Item = &'a str>,
    zero_is_a_place: bool,
) -> Vec<(String, u64)> {
    let mut name: HashMap<String, String> = HashMap::new();
    let mut hostile: BTreeSet<String> = BTreeSet::new();
    let mut most: HashMap<String, u64> = HashMap::new();
    for raw in slice {
        let Some(line) = parse_line(raw) else {
            continue;
        };
        for u in line.event.units().into_iter().flatten() {
            if !u.guid.contains('-') {
                continue;
            }
            name.entry(u.guid.clone()).or_insert_with(|| u.name.clone());
            if u.flags & 0x40 != 0 && !u.guid.starts_with("Player-") {
                hostile.insert(u.guid.clone());
            }
        }
        if let Some(h) = &line.hp_hint
            && h.pos.is_some_and(|p| p.map != 0 || zero_is_a_place)
            && ["Player-", "Creature-", "Vehicle-"]
                .iter()
                .any(|k| h.unit_guid.starts_with(k))
        {
            let m = most.entry(h.unit_guid.clone()).or_insert(0);
            *m = (*m).max(h.max);
        }
    }
    hostile
        .into_iter()
        .map(|g| {
            (
                name.get(&g).cloned().unwrap_or_default(),
                most.get(&g).copied().unwrap_or(0),
            )
        })
        .collect()
}

/// Case and punctuation folded: letters and digits lowercased, one space
/// between words, everything else dropped.
fn fold(s: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() && out.last().is_some_and(|l| *l != ' ') {
            out.push(' ');
        }
    }
    while out.last() == Some(&' ') {
        out.pop();
    }
    out
}

fn letters(s: &[char]) -> usize {
    s.iter().filter(|c| c.is_alphanumeric()).count()
}

/// The optimal-string-alignment distance (a transposition of two
/// neighbors counts one edit).
fn osa(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

#[derive(Default)]
struct Tally {
    cuts: usize,
    with_hostiles: usize,
    exact: usize,
    nobody: usize,
    near: BTreeMap<&'static str, usize>,
    took: usize,
    refused: usize,
    boss_sizes: BTreeMap<usize, usize>,
    broken: Vec<String>,
    sibling_pairs: usize,
    near_beside_exact: usize,
    cases: Vec<String>,
}

fn measure(
    label: &str,
    encounter: u32,
    c: &wowdps_core::model::replay::Cut,
    seen: &[(String, u64)],
    t: &mut Tally,
) {
    t.cuts += 1;
    let titles: BTreeSet<String> = c
        .events
        .iter()
        .filter(|e| e.kind == EventKind::BossEngaged)
        .map(|e| e.spell.clone())
        .collect();
    let parts: BTreeSet<String> = titles.iter().flat_map(|t| title_parts(t)).collect();
    let hostile: Vec<&wowdps_core::model::replay::Unit> = c
        .units
        .iter()
        .filter(|u| matches!(u.kind, UnitKind::Boss | UnitKind::Add))
        .collect();
    if hostile.is_empty() {
        return;
    }
    t.with_hostiles += 1;
    let names: BTreeSet<&str> = hostile.iter().map(|u| u.name.as_str()).collect();
    let exact: BTreeSet<&str> = names
        .iter()
        .copied()
        .filter(|n| parts.contains(&n.to_ascii_lowercase()))
        .collect();
    let folded_parts: Vec<Vec<char>> = parts.iter().map(|p| fold(p)).collect();
    let near = |n: &str| {
        let f = fold(n);
        letters(&f) >= 5
            && folded_parts
                .iter()
                .any(|p| letters(p) >= 5 && osa(&f, p) == 1)
    };
    // Two distinct hostiles one edit apart (5+ letters each).
    let fn_names: Vec<Vec<char>> = names.iter().map(|n| fold(n)).collect();
    for i in 0..fn_names.len() {
        for j in i + 1..fn_names.len() {
            let (a, b) = (&fn_names[i], &fn_names[j]);
            if letters(a) >= 5 && letters(b) >= 5 && a != b && osa(a, b) == 1 {
                t.sibling_pairs += 1;
            }
        }
    }
    if !exact.is_empty() {
        t.exact += 1;
        let beside = names
            .iter()
            .filter(|n| !exact.contains(*n) && near(n))
            .count();
        if beside > 0 {
            t.near_beside_exact += 1;
            t.cases.push(format!(
                "{label} (encounter {encounter}): exact names {} creature(s); {beside} other within one edit",
                exact.len()
            ));
        }
        return;
    }
    t.nobody += 1;
    let bosses = hostile.iter().filter(|u| u.kind == UnitKind::Boss).count();
    *t.boss_sizes.entry(bosses).or_default() += 1;
    let cand: BTreeSet<&str> = names.iter().copied().filter(|n| near(n)).collect();
    let cand_npcs: BTreeSet<u32> = hostile
        .iter()
        .filter(|u| cand.contains(u.name.as_str()))
        .map(|u| u.npc)
        .collect();
    let bucket = match cand.len() {
        0 => "none",
        1 => "one",
        _ => "several",
    };
    *t.near.entry(bucket).or_default() += 1;
    let mut verdict = "";
    if cand.len() == 1 {
        let of_it = |u: &&&wowdps_core::model::replay::Unit| cand.contains(u.name.as_str());
        let its = hostile.iter().filter(of_it).count();
        let its_bosses = hostile
            .iter()
            .filter(of_it)
            .filter(|u| u.kind == UnitKind::Boss)
            .count();
        if its_bosses == its && bosses == its {
            t.took += 1;
            verdict = ", taken";
        } else if its_bosses == 0 {
            // A refusal the rule explains: several encounters, or a name
            // not the strongest posted hostile (read here from the lines).
            let fc: Vec<Vec<char>> = cand.iter().map(|n| fold(n)).collect();
            let strongest = seen
                .iter()
                .filter(|(n, _)| fc.contains(&fold(n)))
                .map(|(_, hp)| *hp)
                .max()
                .unwrap_or(0);
            let top = seen.iter().map(|(_, hp)| *hp).max().unwrap_or(0);
            if titles.len() > 1 {
                t.refused += 1;
                verdict = ", refused: several encounters";
            } else if strongest == 0 || strongest < top {
                t.refused += 1;
                verdict = ", refused: not the strongest posted hostile";
            } else {
                verdict = ", REFUSED THOUGH THE STRONGEST";
                t.broken.push(format!("{label} (encounter {encounter})"));
            }
        } else {
            verdict = ", NEITHER TAKEN NOR REFUSED";
            t.broken.push(format!("{label} (encounter {encounter})"));
        }
    }
    t.cases.push(format!(
        "{label} (encounter {encounter}): exact nobody; {} hostile names, {} hostile units; \
         the cut calls {bosses} unit(s) boss; near-miss {bucket} ({} name(s), {} creature id(s)){verdict}",
        names.len(),
        hostile.len(),
        cand.len(),
        cand_npcs.len(),
    ));
}

fn check(path: &str, edition: &str) {
    let p = Path::new(path);
    let idx = scan(&mut std::fs::File::open(p).unwrap());
    let mut t = Tally::default();
    let pulls: Vec<&SegmentMeta> = idx
        .segments
        .iter()
        .filter(|m| m.kind == SegmentKind::Encounter && !m.arena && m.end_ms.is_some())
        .collect();
    let keys: Vec<&SegmentMeta> = idx
        .overalls
        .iter()
        .filter(|m| m.pars_ms.is_some() && m.end_ms.is_some())
        .collect();
    let zero = edition == "forever";
    for (n, meta) in pulls.iter().enumerate() {
        let text = load_segment_text(p, meta).unwrap();
        let c = cut(text.seeds(), text.slice(), &NoPlaced, None);
        let seen = hostiles_of(text.slice(), zero);
        let enc = meta.encounter.as_ref().map_or(0, |e| e.id);
        measure(&format!("pull #{n}"), enc, &c, &seen, &mut t);
    }
    for (n, meta) in keys.iter().enumerate() {
        let text = load_segment_text(p, meta).unwrap();
        let c = cut(text.seeds(), text.slice(), &NoPlaced, None);
        let seen = hostiles_of(text.slice(), zero);
        measure(
            &format!("key #{n} (map {})", c.head.map),
            0,
            &c,
            &seen,
            &mut t,
        );
    }
    println!(
        "{edition}: {} cuts ({} pulls, {} keys), {} with hostiles; exact names someone in {}, nobody in {}",
        t.cuts,
        pulls.len(),
        keys.len(),
        t.with_hostiles,
        t.exact,
        t.nobody
    );
    println!(
        "{edition}: near-miss where exact names nobody: {:?}; the one name taken {} times, refused as the rule says {}",
        t.near, t.took, t.refused
    );
    println!(
        "{edition}: units the cut calls boss where exact names nobody: {:?}",
        t.boss_sizes
    );
    println!(
        "{edition}: hostile pairs one edit apart: {}; cuts with another hostile one edit from a title the exact rule matched: {}",
        t.sibling_pairs, t.near_beside_exact
    );
    for c in &t.cases {
        println!("{edition}: {c}");
    }
    assert!(
        t.broken.is_empty(),
        "a near-miss name taken with others, or refused though the rule takes it: {:?}",
        t.broken
    );
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real Retail combat log"]
fn boss_names_measured_on_a_real_log() {
    let Some(path) = real_log::retail_log() else {
        return;
    };
    check(&path, "retail");
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn boss_names_measured_on_a_real_forever_log() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    check(&path, "forever");
}
