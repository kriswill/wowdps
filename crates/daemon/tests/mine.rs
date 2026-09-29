//! v35: the daemon marks the reader's own characters `mine` — on the
//! meter's rows, on the drill rows that name a player, on the raid
//! timeline's deaths and on a stored fight's rows — from the account's
//! characters as the history store knows them. Nobody is marked while the
//! store knows nobody; nothing is ever stored with the flag.

use wowdps_core::model::View;
use wowdps_daemon::history::Backend;
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{RaidTimeline, Row};
use wowdps_proto::{
    Breakdown, ClientMsg, Cursor, DaemonMsg, HistoryAnswer, HistoryQuery, SegmentRef,
};

const THRAXX: &str = "Player-1168-0A1B2C01";

/// The fixture's store with Thraxx named as the account's character, the
/// way `history_characters = ["Thraxx"]` names him to the real daemon.
fn thraxx() -> MockDaemon {
    MockDaemon::fixture()
        .with_characters(&["Thraxx".to_string()])
        .with_history()
}

/// One meter snapshot of the fixture's last pull: rows, drill, raid.
fn watch(
    mock: &mut MockDaemon,
    view: View,
    drill: Option<&str>,
) -> (Vec<Row>, Option<Breakdown>, Option<RaidTimeline>) {
    let replies = mock.handle(ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Live,
        view,
        top_n: None,
        drill: drill.map(str::to_string),
        death: None,
        spell: None,
        range: None,
    }));
    replies
        .into_iter()
        .find_map(|m| match m {
            DaemonMsg::Snapshot {
                rows,
                breakdown,
                raid,
                ..
            } => Some((rows, breakdown, raid)),
            _ => None,
        })
        .unwrap_or_default()
}

fn mine(rows: &[Row]) -> Vec<&str> {
    rows.iter()
        .filter(|r| r.mine)
        .map(|r| r.key.as_str())
        .collect()
}

/// The meter marks the account's character alone, on every view that has
/// him; his death on the raid timeline is his; a drill naming him by NAME
/// (a heal's targets, a death's attackers) marks him there too.
#[test]
fn the_accounts_character_is_mine_wherever_the_answer_names_him() {
    let mut mock = thraxx();
    for view in [View::Damage, View::Healing, View::Taken, View::Deaths] {
        let (rows, _, _) = watch(&mut mock, view, None);
        if rows.iter().any(|r| r.key == THRAXX) {
            assert_eq!(mine(&rows), [THRAXX], "{view:?}");
        }
    }
    let (_, _, raid) = watch(&mut mock, View::Deaths, None);
    let raid = raid.expect("a resolved segment carries its raid timeline");
    let marked: Vec<(&str, bool)> = raid
        .deaths
        .iter()
        .map(|d| (d.guid.as_str(), d.mine))
        .collect();
    assert!(marked.contains(&(THRAXX, true)), "{marked:?}");
    assert!(
        marked.iter().all(|(g, m)| (*g == THRAXX) == *m),
        "{marked:?}"
    );
    // Whoever healed him lists him by name among their targets.
    let (rows, _, _) = watch(&mut mock, View::Healing, None);
    let mut healed = false;
    for healer in rows.iter().filter(|r| r.key != THRAXX) {
        let (_, drill, _) = watch(&mut mock, View::Healing, Some(&healer.key));
        let targets = drill.map(|b| b.by_target).unwrap_or_default();
        let marked: Vec<&str> = targets
            .iter()
            .filter(|r| r.mine)
            .map(|r| r.label.as_str())
            .collect();
        let named = targets.iter().any(|r| r.label == "Thraxx-Nebula-US");
        assert_eq!(!marked.is_empty(), named, "{}: {marked:?}", healer.label);
        healed |= named;
    }
    assert!(healed, "somebody in the fixture heals Thraxx");
}

/// A store that knows nobody marks nobody.
#[test]
fn nobody_is_mine_while_the_store_knows_nobody() {
    let mut mock = MockDaemon::fixture();
    for view in [View::Damage, View::Deaths] {
        let (rows, _, raid) = watch(&mut mock, view, None);
        assert!(!rows.is_empty());
        assert!(mine(&rows).is_empty(), "{view:?}");
        assert!(raid.is_some_and(|r| r.deaths.iter().all(|d| !d.mine)));
    }
}

/// A stored fight answers with the flag set at ANSWER time: its rows file
/// never carries it, and the store still says whose row is whose.
#[test]
fn a_stored_fight_marks_its_rows_when_it_answers() {
    let mut mock = thraxx();
    let replies = mock.handle(ClientMsg::GetHistory {
        req_id: 1,
        query: HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: wowdps_proto::FightSort::Newest,
            limit: 10,
            after_id: None,
            role: None,
        },
    });
    let cards = replies.into_iter().find_map(|m| match m {
        DaemonMsg::History {
            answer: HistoryAnswer::Fights { cards, .. },
            ..
        } => Some(cards),
        _ => None,
    });
    let card = cards
        .unwrap_or_default()
        .into_iter()
        .find(|c| c.players.iter().any(|p| p.guid == THRAXX))
        .expect("a stored fight with Thraxx in it");
    let replies = mock.handle(ClientMsg::GetFight {
        req_id: 2,
        fight_id: card.id.clone(),
        view: View::Damage,
        drill: None,
        death: None,
        boss: None,
    });
    let fight = replies.into_iter().find_map(|m| match m {
        DaemonMsg::Fight { fight, .. } => fight,
        _ => None,
    });
    let rows = fight.map(|f| f.rows).unwrap_or_default();
    assert_eq!(mine(&rows), [THRAXX]);
    // Never stored: the rows tier's own bytes carry no "mine" — the decoded
    // rows would say nothing, since the reader never sets the flag.
    let file = mock
        .history()
        .backend()
        .read("rows", &format!("{}.json", card.id))
        .expect("the rows tier on disk");
    let text = String::from_utf8(file).expect("json");
    assert!(text.contains(THRAXX), "the rows name Thraxx");
    assert!(!text.contains("\"mine\""), "never stored");
}
