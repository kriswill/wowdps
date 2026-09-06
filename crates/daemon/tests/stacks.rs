//! R21 through the daemon: a Taken drill on the stacks fixture carries the
//! player's stacking debuffs and cells (v27), the rows tier writes the
//! `stacks[]` block for every player under a hostile debuff, and the stored
//! Taken drill equals the live one — cells, debuffs and the dropped count.
//! Numbers are `crates/core/fixtures/stacks.expected.md`'s.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{Row, SegmentKind, View};
use wowdps_proto::{Breakdown, ClientMsg, Cursor, DaemonMsg, SegmentRef};

const STACKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/stacks.txt");

const TANK: &str = "Player-1168-0A1B2C51";
const HEALER: &str = "Player-1168-0A1B2C52";
const MAGE: &str = "Player-1168-0A1B2C53";
const TECTONIC: u32 = 1305225;
const SMASH: u32 = 1305230;
const CLAWS: u32 = 1305240;

fn ids(mock: &mut MockDaemon) -> Vec<wowdps_model::SegmentId> {
    let out = mock.handle(ClientMsg::Watch(Cursor::List));
    out.iter()
        .find_map(|m| match m {
            DaemonMsg::SegmentList { entries, .. } => Some(entries.iter().map(|e| e.id).collect()),
            _ => None,
        })
        .expect("a list answers the list cursor")
}

fn watch(
    mock: &mut MockDaemon,
    id: wowdps_model::SegmentId,
    view: View,
    drill: Option<&str>,
) -> (wowdps_model::SegmentInfo, Vec<Row>, Option<Breakdown>) {
    let out = mock.handle(ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Id(id),
        view,
        top_n: None,
        drill: drill.map(str::to_string),
        spell: None,
    }));
    out.into_iter()
        .rev()
        .find_map(|m| match m {
            DaemonMsg::Snapshot {
                view: v,
                info,
                rows,
                breakdown,
                ..
            } if v == view => Some((info, rows, breakdown)),
            _ => None,
        })
        .expect("a snapshot answers the segment cursor")
}

fn boss(mock: &mut MockDaemon) -> wowdps_model::SegmentId {
    ids(mock)
        .into_iter()
        .find(|id| {
            let (info, _, _) = watch(mock, *id, View::Damage, None);
            info.kind == SegmentKind::Encounter && info.name == "Stacks Test Boss"
        })
        .expect("stacks.txt lists its boss")
}

#[test]
fn a_taken_drill_carries_the_stack_ledger_and_the_stored_one_equals_it() {
    let mut mock = MockDaemon::fixture_at(Path::new(STACKS)).with_history();
    let boss = boss(&mut mock);

    // Live: the tank's ledger, v27.
    let (_, _, live) = watch(&mut mock, boss, View::Taken, Some(TANK));
    let live = live.expect("the live Taken drill");
    let debuffs: Vec<(u32, u16, u32)> = live
        .stacking
        .iter()
        .map(|d| (d.spell_id, d.max_level, d.hits))
        .collect();
    assert_eq!(debuffs, vec![(TECTONIC, 3, 11), (CLAWS, 2, 1)]);
    assert_eq!(live.stacking[0].src, "Stacks Test Boss");
    let top = live
        .stacks
        .iter()
        .find(|c| c.aura_spell_id == TECTONIC && c.damage_spell_id == SMASH && c.level == 3)
        .expect("the level-3 Crushing Smash cell");
    assert_eq!((top.hits, top.sum, top.max), (4, 2_010_000, 620_000));
    assert_eq!(live.stacks.len(), 7);
    assert_eq!(live.stacks_dropped, 0);
    // Other views carry none of it.
    let (_, _, dmg) = watch(&mut mock, boss, View::Damage, Some(TANK));
    let dmg = dmg.expect("the Damage drill");
    assert!(dmg.stacking.is_empty() && dmg.stacks.is_empty());

    // Stored: the rows tier's block, and the drill off it.
    let cards: Vec<_> = mock
        .history()
        .cards()
        .iter()
        .filter(|c| c.name == "Stacks Test Boss")
        .cloned()
        .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    for (guid, want_debuffs) in [(TANK, 2usize), (HEALER, 1), (MAGE, 1)] {
        let out = mock.handle(ClientMsg::GetFight {
            req_id: 9,
            fight_id: cards[0].id.clone(),
            view: View::Taken,
            drill: Some(guid.to_string()),
            boss: None,
        });
        let [
            DaemonMsg::Fight {
                req_id: 9,
                fight: Some(f),
            },
        ] = out.as_slice()
        else {
            panic!("{out:?}");
        };
        let stored = f.breakdown.clone().expect("the stored Taken drill");
        let (_, _, live) = watch(&mut mock, boss, View::Taken, Some(guid));
        let live = live.expect("the live drill");
        assert_eq!(stored.stacking, live.stacking, "{guid}: debuffs");
        assert_eq!(stored.stacks, live.stacks, "{guid}: cells");
        assert_eq!(stored.stacks_dropped, live.stacks_dropped, "{guid}");
        assert_eq!(stored.stacking.len(), want_debuffs, "{guid}");
    }
    // The mage's cell is her pet's hit, folded.
    let (_, _, mage) = watch(&mut mock, boss, View::Taken, Some(MAGE));
    let mage = mage.expect("the mage's drill");
    assert_eq!(mage.stacks.len(), 1);
    assert_eq!((mage.stacks[0].level, mage.stacks[0].sum), (1, 60_000));
}
