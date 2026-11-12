//! R29 over a real Forever log, through the daemon's placed table (proto's
//! `placed`, each edition's own): every boss pull, cut as the store cuts
//! it, keeps each placing spell a player cast as a placed row standing
//! where the caster stood, and its summon's unit — a totem — posted where
//! the log posts it. That is what Forever's entries are found by (`Unit`,
//! `Caster`): its client names no UiMap inside a dungeon, and the cut posts
//! its units there on map 0, on the instance's map. Prints, per pull, the
//! placing casts and summons, and how many of their units were posted.
//!
//! A real log carries real names: nothing here prints a name or a guid, a
//! failure included. A pull is named by its place in the log and its
//! encounter id, a placed row by its spell.
//!
//! Run: `WOWDPS_REAL_LOG_FOREVER=/path/to/WoWCombatLog-*.txt cargo test
//! --release -p wowdps-daemon --test real_log_placed -- --ignored
//! --nocapture` (which log it reads: core's `tests/real_log/mod.rs`). Its
//! name holds `real` and `forever`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "../../core/tests/real_log/mod.rs"]
mod real_log;

use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::SegmentKind;
use wowdps_core::model::Edition;
use wowdps_core::model::replay::PlacedKind;
use wowdps_daemon::replay::cut_text;
use wowdps_proto::placed;

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn placed_casts_stand_where_cast_on_every_real_forever_pull() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let pulls: Vec<_> = idx
        .segments
        .iter()
        .filter(|m| m.kind == SegmentKind::Encounter && m.end_ms.is_some())
        .collect();
    assert!(!pulls.is_empty(), "a real log has closed boss pulls");
    let (mut casts, mut summons, mut posted, mut touches) = (0usize, 0usize, 0usize, 0usize);
    for (n, meta) in pulls.iter().enumerate() {
        let text = load_segment_text(Path::new(&path), meta).expect("load the pull");
        let cut = cut_text(&text, None);
        let label = format!(
            "pull #{n} (encounter {})",
            meta.encounter.as_ref().map_or(0, |e| e.id)
        );
        let (mut c, mut s, mut p) = (0usize, 0usize, 0usize);
        for row in &cut.placed {
            let entry = placed::placed(Edition::Forever, row.spell_id);
            match row.kind {
                PlacedKind::Cast => {
                    assert!(
                        entry.is_some(),
                        "{label}: a cast of {} no table places",
                        row.spell_id
                    );
                    // Found by its caster: where they stood as it was cast.
                    assert!(
                        row.at.is_some(),
                        "{label}: {} cast at no place",
                        row.spell_id
                    );
                    c += 1;
                }
                PlacedKind::Summon => {
                    s += 1;
                    // Found by its unit: the totem's own posts, where the
                    // log posts it.
                    if let Some(u) = row.target.and_then(|t| cut.units.get(t as usize))
                        && !u.posts.is_empty()
                    {
                        p += 1;
                    }
                }
                PlacedKind::Touch => touches += 1,
                PlacedKind::Gone => {}
            }
        }
        println!(
            "{label}: {c} placing casts, all at a place; {s} summons, {p} of their units posted"
        );
        casts += c;
        summons += s;
        posted += p;
    }
    println!(
        "{} pulls: {casts} placing casts at a place, {summons} summons ({posted} units posted), \
         {touches} touches",
        pulls.len()
    );
}
