//! R29 over a real Forever log: every boss pull keeps its positions. The
//! Forever client names no UiMap inside a dungeon or a raid and writes 0
//! there on every block, so a cut that read 0 as nowhere posted nobody
//! (the pull opened 0 s long over bare ground). Each closed boss pull, cut
//! from its lazily loaded segment as the store cuts it, must post every
//! player the meter credits with damage or healing, cover the pull with
//! the players' posts from its first seconds to its last with no long
//! silence between; a pull on no UiMap (floor 0, the posts inside an
//! instance) must name the instance's map in its head. Each pull prints
//! its floors, its units and how its posts cover it.
//!
//! A real log carries real names: nothing here prints a name or a guid, a
//! failure included. A pull is named by its place in the log, its
//! encounter id, its size and its outcome; a player by their place in the
//! pull's roster (sorted by guid).
//!
//! Run: `WOWDPS_REAL_LOG_FOREVER=/path/to/WoWCombatLog-*.txt cargo test
//! --release -p wowdps-core --test real_log_posts -- --ignored --nocapture`
//! (which log it reads: `tests/real_log/mod.rs`). Its name holds `real` and
//! `forever`, so `-- --ignored real` and `-- --ignored forever` run it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod real_log;

use std::collections::BTreeSet;
use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, View};
use wowdps_core::model::replay::UnitKind;
use wowdps_core::replay::{NoPlaced, cut};

/// The longest the players may go unposted, at the pull's ends and between
/// two posts: a block lands on nearly every line a player is in.
const MAX_SILENCE_MS: u32 = 5_000;

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn posts_cover_every_real_forever_boss_pull() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    let path = Path::new(&path);
    let idx = scan(&mut std::fs::File::open(path).unwrap());
    let mut pulls = 0;
    let mut failures = Vec::new();
    for meta in idx
        .segments
        .iter()
        .filter(|m| m.kind == SegmentKind::Encounter && !m.arena && m.end_ms.is_some())
    {
        let Some(enc) = meta.encounter.as_ref() else {
            continue;
        };
        pulls += 1;
        let text = load_segment_text(path, meta).unwrap();
        let c = cut(text.seeds(), text.slice(), &NoPlaced, None);
        let meter = text.meter();
        let seg = meter
            .segments()
            .iter()
            .find(|s| s.kind == SegmentKind::Encounter)
            .expect("the pull's segment");
        let label = format!(
            "pull #{pulls} (encounter {}, size {}, {})",
            enc.id,
            enc.group_size,
            match meta.success {
                Some(true) => "kill",
                Some(false) => "wipe",
                None => "open",
            }
        );
        // The players the meter credits, by their place in a sorted roster.
        let credited: BTreeSet<String> = [View::Damage, View::Healing]
            .into_iter()
            .flat_map(|v| seg.rows(v))
            .filter(|r| r.amount > 0 && r.key.starts_with("Player-"))
            .map(|r| r.key)
            .collect();
        let roster: Vec<&String> = credited.iter().collect();
        let unposted: Vec<String> = roster
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                !c.units
                    .iter()
                    .any(|u| &u.guid == **g && u.kind == UnitKind::Player && !u.posts.is_empty())
            })
            .map(|(i, _)| format!("#{i}"))
            .collect();
        // The players' posts in time: the pull's start to its end, covered.
        let mut times: Vec<u32> = c
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Player)
            .flat_map(|u| u.posts.iter().map(|p| p.t_ms))
            .collect();
        times.sort_unstable();
        let span = u32::try_from(meta.end_ms.unwrap() - meta.start_ms).unwrap_or(u32::MAX);
        let first = times.first().copied();
        let last = times.last().copied();
        let gap = times.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
        let maps: Vec<String> = c
            .maps()
            .iter()
            .map(|m| format!("{}: {} ({} players')", m.map_id, m.posts, m.players))
            .collect();
        println!(
            "{label}: {} s, {} units, {} posts — first player post {:?} ms, last {:?} ms, \
             longest silence {gap} ms; floor {}, instance map {}; maps [{}]; \
             {}/{} credited players posted",
            span / 1000,
            c.units.len(),
            c.posts(),
            first,
            last,
            c.floor,
            c.head.map,
            maps.join(", "),
            roster.len() - unposted.len(),
            roster.len()
        );
        if c.posts() == 0 {
            failures.push(format!("{label}: no post at all"));
        }
        if !unposted.is_empty() {
            failures.push(format!("{label}: players {} unposted", unposted.join(" ")));
        }
        if first.is_none_or(|t| t > MAX_SILENCE_MS)
            || last.is_none_or(|t| t.saturating_add(MAX_SILENCE_MS) < span)
            || gap > MAX_SILENCE_MS
        {
            failures.push(format!(
                "{label}: the players' posts do not cover the pull (first {first:?}, last \
                 {last:?} of {span} ms, longest silence {gap} ms)"
            ));
        }
        // Inside an instance the log names no UiMap: a pull on floor 0
        // stands on the instance's own map, which its head must name.
        if c.floor == 0 && c.head.map == 0 {
            failures.push(format!("{label}: posts on no UiMap and no instance map"));
        }
    }
    println!("{pulls} boss pulls cut");
    assert!(pulls > 0, "the log holds no closed boss pull");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
