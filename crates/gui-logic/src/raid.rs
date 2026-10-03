//! A synthetic raid kill for the GUIs' tests (`test-support` only): what
//! the committed fixtures cannot hold — a handful of players at most —
//! built as a `ClientState` the way the daemon's snapshots build one, so
//! both GUIs measure their chrome over the same 25 players. Moved from the
//! iced window's `testkit`.

use wowdps_proto::{ClientState, DaemonMsg};

/// A Heroic raid kill of `players` — what the committed fixtures cannot
/// hold (a handful of players at most): one segment in the list, the
/// meter on it, the rows in the daemon's order, every spec in turn so
/// tanks and healers are among them. Row `i` is "Raider{i}-Realm-US"
/// with guid "Player-1-{i}", and the amounts step down from 100 M.
pub fn raid(players: usize) -> ClientState {
    raid_with(players, None, wowdps_model::View::Damage)
}

/// [`raid`] with its raid timeline (R25, v35), as the live daemon sends
/// one: a 1 s damage series that bumps under the Heroism at 3:54, and
/// six deaths — Raider3 at 1:10 (rezzed at 2:03), Raider5 at 1:54,
/// Raider7 at 3:01 (rezzed at 4:25, no damage logged), Raider9 at
/// 5:15, the owner Raider16 at 5:45, Raider20 at 6:01 — the prototype's
/// Coiled Altar kill, near enough. Raider16's death is marked `mine`,
/// and so is their row.
pub fn raided(players: usize) -> ClientState {
    raid_with(players, Some(raid_timeline()), wowdps_model::View::Damage)
}

/// [`raided`] on its Deaths view, as the daemon answers it: a row per
/// player who died (their deaths the amount; the owner's marked `mine`)
/// beside the same timeline.
pub fn raided_deaths(players: usize) -> ClientState {
    raid_with(players, Some(raid_timeline()), wowdps_model::View::Deaths)
}

/// The same Deaths rows with no timeline beside them — a store that
/// kept the pull's card alone, or an older daemon: the count table.
pub fn raid_deaths_bare(players: usize) -> ClientState {
    raid_with(players, None, wowdps_model::View::Deaths)
}

/// [`raided`] on `view` from a daemon that marks nobody — its history
/// store off, or not yet published: no row and no death is `mine`.
pub fn raided_unmarked(players: usize, view: wowdps_model::View) -> ClientState {
    let mut t = raid_timeline();
    for d in &mut t.deaths {
        d.mine = false;
    }
    raid_with(players, Some(t), view)
}

/// The timeline [`raided`] carries.
pub fn raid_timeline() -> wowdps_model::RaidTimeline {
    use wowdps_model::{LustWindow, RaidDeath, RaidTimeline, Rez, View};
    let series = (0..422_u64)
        .map(|s| {
            let lust = (234..274).contains(&s);
            3_000_000 + (s % 17) * 90_000 + if lust { 6_000_000 } else { 0 }
        })
        .collect();
    let death = |i: usize, at_ms: i64, blow: &str, rez: Option<i64>| {
        // A cheat death running out (Purgatory) is a "hit" of 1 the
        // player dealt themselves, as the log writes it.
        let own = blow == "Purgatory";
        RaidDeath {
            guid: format!("Player-1-{i}"),
            name: format!("Raider{i}-Realm-US"),
            class: spec_of(i).map(wowdps_model::Spec::class),
            spec: spec_of(i),
            index: 0,
            at_ms,
            blow: blow.to_string(),
            source: if blow.is_empty() {
                String::new()
            } else if own {
                format!("Raider{i}-Realm-US")
            } else {
                "Zul'jan".to_string()
            },
            hit: if blow.is_empty() {
                0
            } else if own {
                1
            } else {
                150_000 + i as u64
            },
            overkill: (!blow.is_empty() && !own).then_some(10_000 + i as u64),
            rez: rez.map(|at_ms| Rez {
                at_ms,
                by: "Player-1-4".to_string(),
                by_name: "Raider4-Realm-US".to_string(),
                spell: "Intercession".to_string(),
            }),
            mine: i == 16,
            enemy: false,
        }
    };
    RaidTimeline {
        view: View::Damage,
        bucket_ms: 1000,
        series,
        deaths: vec![
            death(3, 70_000, "Venom Rupture", Some(123_200)),
            death(5, 114_000, "Venom Rupture", None),
            death(7, 181_100, "", Some(265_900)),
            death(9, 315_400, "Purgatory", None),
            death(16, 345_500, "Coalesced Venom", None),
            death(20, 361_800, "Dreadmarch", None),
        ],
        lust: vec![LustWindow {
            at_ms: 234_500,
            dur_ms: 40_000,
            label: "Heroism".to_string(),
        }],
    }
}

fn raid_with(
    players: usize,
    timeline: Option<wowdps_model::RaidTimeline>,
    view: wowdps_model::View,
) -> ClientState {
    use wowdps_model::{Encounter, ListRow, Row, SegmentId, SegmentInfo, SegmentKind, Spec, View};
    // The daemon marks the owner's row when it marked their death.
    let owner = timeline
        .as_ref()
        .and_then(|t| t.deaths.iter().find(|d| d.mine))
        .map(|d| d.guid.clone());
    use wowdps_proto::{ListEntry, SegmentRef};
    let secs = 422.04;
    let rows: Vec<Row> = (0..players)
        .map(|i| {
            let spec = spec_of(i);
            let amount = 100_000_000_u64.saturating_sub(i as u64 * 2_000_000);
            Row {
                key: format!("Player-1-{i}"),
                label: format!("Raider{i}-Realm-US"),
                amount,
                extra: 50_000,
                count: 1_000,
                crits: 300,
                per_sec: amount as f64 / secs,
                pct: 100.0 / players as f64,
                class: spec.map(Spec::class),
                spec,
                mine: owner.as_deref() == Some(format!("Player-1-{i}").as_str()),
                ..Row::default()
            }
        })
        .collect();
    // On the Deaths view the daemon's rows are the dead: one per player
    // who died, their deaths the amount, in the timeline's order.
    let rows: Vec<Row> = match view {
        View::Deaths => {
            let t = timeline.clone().unwrap_or_else(raid_timeline);
            let mut dead: Vec<Row> = Vec::new();
            for d in &t.deaths {
                match dead.iter_mut().find(|r| r.key == d.guid) {
                    Some(r) => r.amount += 1,
                    None => dead.push(Row {
                        key: d.guid.clone(),
                        label: d.name.clone(),
                        amount: 1,
                        count: 1,
                        class: d.class,
                        spec: d.spec,
                        mine: d.mine,
                        ..Row::default()
                    }),
                }
            }
            dead
        }
        _ => rows,
    };
    let encounter = Some(Encounter {
        id: 3492,
        difficulty: 15,
        group_size: 25,
    });
    let info = SegmentInfo {
        kind: SegmentKind::Encounter,
        name: "The Coiled Altar".to_string(),
        start_ms: 1_000,
        duration_ms: 422_040,
        combat_ms: 422_040,
        success: Some(true),
        live: false,
        instance: Some(0),
        pars_ms: None,
        arena: false,
        encounter,
    };
    let mut state = ClientState::new();
    let _ = state.on_msg(DaemonMsg::SegmentList {
        seq: 1,
        entries: vec![ListEntry {
            id: SegmentId(1),
            row: ListRow {
                kind: SegmentKind::Encounter,
                name: info.name.clone(),
                start_ms: info.start_ms,
                success: info.success,
                duration_ms: info.duration_ms,
                live: false,
                instance: Some(0),
                pars_ms: None,
                arena: false,
                encounter,
            },
        }],
        source: Some("raid.txt".to_string()),
        active: true,
        log_id: None,
    });
    state.view = view;
    let _ = state.on_msg(DaemonMsg::Snapshot {
        seq: 2,
        segment: SegmentRef::Live,
        id: Some(SegmentId(1)),
        view,
        info,
        total_rows: rows.len() as u32,
        rows,
        breakdown: None,
        segment_count: 1,
        source: Some("raid.txt".to_string()),
        status: None,
        raid: timeline,
    });
    assert_eq!(state.screen, wowdps_model::Screen::Meter);
    state
}

/// Raider `i`'s spec: every spec in turn, so tanks and healers are among
/// any raid of a dozen or more.
fn spec_of(i: usize) -> Option<wowdps_model::Spec> {
    let all = wowdps_model::Spec::ALL;
    all.get(i % all.len()).copied()
}
