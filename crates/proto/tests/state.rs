//! `ClientState` against hand-built daemon messages: every accessor and
//! transition the frontends lean on, exercised without an engine — the
//! daemon-side round trip is covered by the daemon crate's mock suite; this
//! pins the client-side machine on its own, message by message.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use wowdps_model::{
    Action, Drill, GraphMode, ListRow, Loadout, Pane, Row, Screen, SegmentId, SegmentInfo,
    SegmentKind, Timeline, View,
};
use wowdps_proto::{
    Breakdown, ClientMsg, ClientState, CompareSide, Cursor, DaemonMsg, HistoryStatus, ListEntry,
    LoadError, OverlayState, SegmentRef,
};

fn list_row(kind: SegmentKind, start_ms: i64, duration_ms: i64) -> ListRow {
    ListRow {
        kind,
        name: format!("seg@{start_ms}"),
        start_ms,
        success: Some(true),
        duration_ms,
        live: false,
        instance: None,
        pars_ms: None,
        arena: false,
        encounter: None,
    }
}

/// `n` closed encounters, ids 1..=n, one minute apart.
fn entries(n: u64) -> Vec<ListEntry> {
    (1..=n)
        .map(|i| ListEntry {
            id: SegmentId(i),
            row: list_row(SegmentKind::Encounter, i as i64 * 60_000, 30_000),
        })
        .collect()
}

fn segment_list(n: u64, active: bool, source: Option<&str>) -> DaemonMsg {
    DaemonMsg::SegmentList {
        seq: 1,
        entries: entries(n),
        source: source.map(str::to_string),
        active,
        log_id: None,
    }
}

fn info(kind: SegmentKind) -> SegmentInfo {
    SegmentInfo {
        kind,
        name: "Boss".to_string(),
        start_ms: 60_000,
        duration_ms: 30_000,
        success: Some(false),
        live: false,
        instance: Some(3),
        pars_ms: None,
        arena: false,
        encounter: None,
    }
}

fn row(key: &str, amount: u64) -> Row {
    Row {
        key: key.to_string(),
        label: key.to_string(),
        amount,
        count: 4,
        crits: 1,
        ..Row::default()
    }
}

fn timeline(buckets: &[u64]) -> Timeline {
    Timeline {
        bucket_ms: 1000,
        buckets: buckets.to_vec(),
        marks: Vec::new(),
    }
}

fn snapshot(
    segment: SegmentRef,
    id: Option<SegmentId>,
    view: View,
    rows: Vec<Row>,
    breakdown: Option<Breakdown>,
    segment_count: u32,
) -> DaemonMsg {
    DaemonMsg::Snapshot {
        seq: 2,
        segment,
        id,
        view,
        info: info(SegmentKind::Encounter),
        rows,
        total_rows: 0,
        breakdown,
        segment_count,
        source: Some("log.txt".to_string()),
        status: None,
    }
}

fn drilled_breakdown() -> Breakdown {
    Breakdown {
        by_spell: vec![row("Fireball", 70), row("Frostbolt", 30)],
        by_target: vec![row("Boss", 100)],
        timeline: Some(timeline(&[10, 20, 30])),
        spell_timeline: Some(timeline(&[1, 2, 3])),
        spell_targets: Some(vec![row("Boss", 70)]),
    }
}

fn side(guid: &str) -> Box<CompareSide> {
    Box::new(CompareSide {
        guid: guid.to_string(),
        total: row(guid, 100),
        spells: vec![row("Fireball", 100)],
        timeline: timeline(&[50, 50]),
        spell_timeline: None,
    })
}

fn compare_snapshot(
    segment: SegmentRef,
    a: &str,
    b: &str,
    range: Option<(u32, u32)>,
    source: Option<&str>,
) -> DaemonMsg {
    DaemonMsg::CompareSnapshot {
        seq: 3,
        segment,
        id: Some(SegmentId(3)),
        info: info(SegmentKind::Encounter),
        a: side(a),
        b: side(b),
        range,
        source: source.map(str::to_string),
        status: Some("note".to_string()),
    }
}

/// A state on the meter of the newest of three segments, one snapshot in.
fn on_meter() -> ClientState {
    let mut st = ClientState::new();
    assert!(
        st.on_msg(segment_list(3, false, Some("log.txt")))
            .is_empty()
    );
    let reqs = st.apply(Action::Open);
    assert_eq!(st.screen, Screen::Meter);
    assert!(matches!(
        &reqs[..],
        [ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Live,
            ..
        })]
    ));
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(3)),
        View::Damage,
        vec![row("A", 300), row("B", 200), row("C", 100)],
        None,
        3,
    ));
    st
}

fn watch_of(reqs: &[ClientMsg]) -> &Cursor {
    match reqs {
        [ClientMsg::Watch(c)] => c,
        other => panic!("expected exactly one Watch, got {other:?}"),
    }
}

#[test]
fn default_and_with_top_n_shape_the_first_request() {
    let st = ClientState::default();
    assert_eq!(st.screen, Screen::List);
    assert_eq!(st.initial_request(), ClientMsg::Watch(Cursor::List));
    assert_eq!(st.watched_segment(), SegmentRef::Live);
    assert!(st.following_live());
    assert_eq!(st.segment_count(), 0);
    assert_eq!(st.segment_index(), 0);
    assert!(st.entries().is_empty());
    assert!(st.list_rows().is_empty());
    assert_eq!(st.list_selection(), 0);
    assert!(st.rows().is_empty());
    assert_eq!(st.breakdown(), (Vec::new(), Vec::new()));
    assert!(st.segment_name().is_none());
    assert!(st.segment_success().is_none());
    assert!(st.segment_pars_ms().is_none());
    assert!(st.segment_kind().is_none());
    assert!(st.segment_instance().is_none());
    assert!(!st.segment_arena());
    assert!(!st.is_live());
    assert_eq!(st.duration_ms(), 0);
    assert_eq!(st.graph_mode(), GraphMode::Dps);
    assert!(st.compare_picks().is_empty());
    assert!(st.compare_sides().is_none());
    assert!(st.compare_shown_range().is_none());
    assert!(st.compare_spell().is_none());
    assert!(st.drill_timeline().is_none());
    assert!(st.drill_range().is_none());
    assert!(st.drill_spell().is_none());
    assert!(st.spell_timeline().is_none());
    assert!(st.drill_spell_row().is_none());
    assert!(st.spell_target_rows().is_empty());

    // The overlay's row cap rides on every meter Watch.
    let mut st = ClientState::with_top_n(Some(5));
    st.on_msg(segment_list(2, true, None));
    let reqs = st.apply(Action::SetView(View::Healing));
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            top_n: Some(5),
            view: View::Healing,
            ..
        }
    ));
}

#[test]
fn a_snapshot_fills_every_header_accessor() {
    let st = on_meter();
    assert_eq!(st.segment_name().as_deref(), Some("Boss"));
    assert_eq!(st.segment_success(), Some(false));
    assert_eq!(st.segment_kind(), Some(SegmentKind::Encounter));
    assert_eq!(st.segment_instance(), Some(3));
    assert_eq!(st.duration_ms(), 30_000);
    assert_eq!(st.source.as_deref(), Some("log.txt"));
    assert_eq!(st.segment_count(), 3);
    assert_eq!(st.segment_index(), 2, "Live is the newest position");
    assert_eq!(st.rows().len(), 3);
    assert_eq!(st.watched_segment(), SegmentRef::Live);
    // The same rows under another view are not this view's rows.
    let mut st = st;
    st.view = View::Healing;
    assert!(st.rows().is_empty());
}

#[test]
fn stale_snapshots_for_another_cursor_are_dropped() {
    let mut st = on_meter();
    // Wrong view.
    st.on_msg(snapshot(
        SegmentRef::Live,
        None,
        View::Healing,
        vec![row("H", 1)],
        None,
        9,
    ));
    assert_eq!(st.rows().len(), 3, "a Healing push cannot replace Damage");
    // Wrong segment.
    st.on_msg(snapshot(
        SegmentRef::Id(SegmentId(1)),
        None,
        View::Damage,
        vec![row("X", 1)],
        None,
        9,
    ));
    assert_eq!(st.rows().len(), 3);
    // Wrong screen.
    st.screen = Screen::List;
    st.on_msg(snapshot(
        SegmentRef::Live,
        None,
        View::Damage,
        vec![row("X", 1)],
        None,
        9,
    ));
    assert_eq!(st.segment_count(), 3, "the list counts its own entries");
}

#[test]
fn a_live_snapshot_with_a_new_id_extends_the_id_table() {
    let mut st = on_meter();
    assert_eq!(st.entries().len(), 3);
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(4)),
        View::Damage,
        vec![row("A", 1)],
        None,
        4,
    ));
    let entries = st.entries();
    assert_eq!(
        entries.len(),
        4,
        "the daemon named a segment we had no row for"
    );
    assert_eq!(entries[3].id, SegmentId(4));
    assert_eq!(
        entries[3].row.name, "Boss",
        "the row is built from the snapshot header"
    );
    assert_eq!(entries[3].row.instance, Some(3));
    assert_eq!(st.list_rows().len(), 4);
    // Selection clamps to the (shorter) row list.
    st.row_sel = 7;
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(4)),
        View::Damage,
        vec![row("A", 1), row("B", 1)],
        None,
        4,
    ));
    assert_eq!(st.row_sel, 1);
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(4)),
        View::Damage,
        Vec::new(),
        None,
        4,
    ));
    assert_eq!(st.row_sel, 0, "no rows: selection rests at zero");
}

#[test]
fn the_drill_accessors_read_the_breakdown_only_for_the_current_view() {
    let mut st = on_meter();
    let reqs = st.apply(Action::Open);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            drill: Some(k),
            spell: None,
            ..
        } if k == "A"
    ));
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(3)),
        View::Damage,
        vec![row("A", 300)],
        Some(drilled_breakdown()),
        3,
    ));
    assert_eq!(st.breakdown().0.len(), 2);
    assert_eq!(st.drill_timeline().map(|t| t.buckets.len()), Some(3));
    assert!(st.spell_timeline().is_none(), "no ability drill yet");
    assert!(st.drill_spell_row().is_none());
    assert!(st.spell_target_rows().is_empty());

    // Descend into the selected ability.
    let reqs = st.apply(Action::Open);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment { spell: Some(s), .. } if s == "Fireball"
    ));
    assert_eq!(st.drill_spell().map(|(k, _)| k.as_str()), Some("Fireball"));
    assert_eq!(st.spell_timeline().map(|t| t.buckets.len()), Some(3));
    assert_eq!(st.drill_spell_row().map(|r| r.amount), Some(70));
    assert_eq!(st.spell_target_rows().len(), 1);

    // Opening again inside the ability view is a no-op.
    assert!(st.apply(Action::Open).is_empty());
    // And the row selection is frozen there.
    st.apply(Action::Down);
    assert_eq!(st.drill.as_ref().unwrap().spell_sel, 0);
    // SwapPane is meaningless in the ability view.
    st.apply(Action::SwapPane);
    assert_eq!(st.drill.as_ref().unwrap().pane, Pane::Spell);

    // The zoom window is local and degenerate windows clear it.
    st.set_drill_range(Some((1000, 2000)));
    assert_eq!(st.drill_range(), Some((1000, 2000)));
    st.set_drill_range(Some((2000, 2000)));
    assert_eq!(st.drill_range(), None);
    st.set_drill_range(Some((100, 900)));

    // A view change closes the ability drill (by-spell keys are view-local)
    // but keeps the player drill.
    let reqs = st.apply(Action::SetView(View::Healing));
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            view: View::Healing,
            drill: Some(_),
            spell: None,
            ..
        }
    ));
    assert!(st.drill_spell().is_none());
    assert!(st.drill_range().is_none());
    // Under the other view, the cached (Damage) breakdown is not shown.
    assert_eq!(st.breakdown(), (Vec::new(), Vec::new()));
    assert!(st.drill_timeline().is_none());
}

#[test]
fn the_ability_drill_needs_a_rate_view_and_the_spell_pane() {
    let mut st = on_meter();
    st.apply(Action::Open);
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(3)),
        View::Damage,
        vec![row("A", 300)],
        Some(drilled_breakdown()),
        3,
    ));
    // Target pane: Enter does nothing, j/k walk the target rows.
    st.apply(Action::SwapPane);
    assert_eq!(st.drill.as_ref().unwrap().pane, Pane::Target);
    assert!(st.apply(Action::Open).is_empty());
    st.apply(Action::Down);
    st.apply(Action::Down);
    assert_eq!(
        st.drill.as_ref().unwrap().target_sel,
        0,
        "one target row: clamped"
    );
    st.apply(Action::SwapPane);
    st.apply(Action::Down);
    st.apply(Action::Down);
    assert_eq!(st.drill.as_ref().unwrap().spell_sel, 1, "two spell rows");
    st.apply(Action::Up);
    assert_eq!(st.drill.as_ref().unwrap().spell_sel, 0);

    // Count views have no ability graph.
    st.apply(Action::SetView(View::Interrupts));
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(3)),
        View::Interrupts,
        vec![row("A", 3)],
        Some(drilled_breakdown()),
        3,
    ));
    assert!(st.apply(Action::Open).is_empty());
    assert!(st.drill_spell().is_none());

    // A drill with an empty by-spell pane has nothing to descend into.
    st.apply(Action::SetView(View::Damage));
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(3)),
        View::Damage,
        vec![row("A", 3)],
        Some(Breakdown::default()),
        3,
    ));
    assert!(st.apply(Action::Open).is_empty());
    st.apply(Action::Down);
    assert_eq!(
        st.drill.as_ref().unwrap().spell_sel,
        0,
        "nothing to move through"
    );
}

#[test]
fn back_walks_ability_then_drill_then_list_with_the_cursor_on_the_segment() {
    let mut st = on_meter();
    st.apply(Action::OlderSegment);
    assert_eq!(st.watched_segment(), SegmentRef::Id(SegmentId(2)));
    st.on_msg(DaemonMsg::Snapshot {
        seq: 5,
        segment: SegmentRef::Id(SegmentId(2)),
        id: Some(SegmentId(2)),
        view: View::Damage,
        info: info(SegmentKind::Encounter),
        rows: vec![row("A", 1)],
        total_rows: 1,
        breakdown: None,
        segment_count: 3,
        source: Some("log.txt".to_string()),
        status: None,
    });
    st.apply(Action::Open);
    st.on_msg(DaemonMsg::Snapshot {
        seq: 6,
        segment: SegmentRef::Id(SegmentId(2)),
        id: Some(SegmentId(2)),
        view: View::Damage,
        info: info(SegmentKind::Encounter),
        rows: vec![row("A", 1)],
        total_rows: 1,
        breakdown: Some(drilled_breakdown()),
        segment_count: 3,
        source: Some("log.txt".to_string()),
        status: None,
    });
    st.apply(Action::Open);
    st.set_drill_range(Some((0, 5)));
    assert!(st.drill_spell().is_some());

    st.apply(Action::Back);
    assert!(st.drill_spell().is_none());
    assert!(st.drill_range().is_none());
    assert!(st.drill.is_some());
    st.apply(Action::Back);
    assert!(st.drill.is_none());
    let reqs = st.apply(Action::Back);
    assert_eq!(watch_of(&reqs), &Cursor::List);
    assert_eq!(st.screen, Screen::List);
    assert_eq!(
        st.list_selection(),
        1,
        "the list cursor lands on the segment"
    );
}

#[test]
fn list_actions_move_locally_and_open_by_position() {
    let mut st = ClientState::new();
    // Nothing loaded: Open is a no-op, Down cannot move.
    assert!(st.apply(Action::Open).is_empty());
    assert!(st.apply(Action::Down).is_empty());
    assert_eq!(st.list_selection(), 0);
    st.set_list_selection(9);
    assert_eq!(st.list_selection(), 0, "no rows: clamps to zero");

    st.on_msg(segment_list(3, false, None));
    assert_eq!(st.list_selection(), 2, "first list selects the newest");
    st.apply(Action::Up);
    st.apply(Action::Up);
    st.apply(Action::Up);
    assert_eq!(st.list_selection(), 0, "saturates at the top");
    st.apply(Action::Down);
    assert_eq!(st.list_selection(), 1);
    st.set_list_selection(99);
    assert_eq!(st.list_selection(), 2, "clamped to the last row");
    st.set_list_selection(1);
    // A view key on the list only changes the view for later.
    assert!(st.apply(Action::SetView(View::Dispels)).is_empty());
    assert_eq!(st.view, View::Dispels);
    // Quit is sticky.
    st.apply(Action::Quit);
    assert!(st.quit);

    let reqs = st.apply(Action::Open);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            segment: SegmentRef::Id(SegmentId(2)),
            view: View::Dispels,
            ..
        }
    ));
    assert_eq!(st.screen, Screen::Meter);
    assert!(!st.following_live());

    // A shorter list later pulls a now-invalid selection back in range.
    st.screen = Screen::List;
    st.set_list_selection(2);
    st.on_msg(segment_list(1, false, None));
    assert_eq!(st.list_selection(), 0);
}

#[test]
fn goto_list_pos_pins_live_on_the_newest_and_ids_elsewhere() {
    let mut st = ClientState::new();
    assert!(
        st.goto_list_pos(0).is_empty(),
        "no entries: nothing to go to"
    );
    st.on_msg(segment_list(3, false, None));

    let reqs = st.goto_list_pos(0);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            segment: SegmentRef::Id(SegmentId(1)),
            ..
        }
    ));
    assert_eq!(st.screen, Screen::Meter);
    assert_eq!(st.segment_index(), 0);

    let reqs = st.goto_list_pos(2);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            segment: SegmentRef::Live,
            ..
        }
    ));
    assert!(st.following_live());

    // Beyond the end still means "newest".
    st.goto_list_pos(0);
    st.goto_list_pos(99);
    assert!(st.following_live());

    // The daemon says there are more segments than the id table holds: a
    // position the table cannot resolve stays put rather than re-pinning.
    st.on_msg(snapshot(
        SegmentRef::Live,
        None,
        View::Damage,
        Vec::new(),
        None,
        6,
    ));
    assert_eq!(st.segment_count(), 6);
    let before = st.watched_segment();
    assert!(st.goto_list_pos(4).is_empty());
    assert_eq!(st.watched_segment(), before);
}

#[test]
fn segment_navigation_clamps_at_both_ends_and_unknown_ids_read_as_newest() {
    let mut st = on_meter();
    assert!(st.apply(Action::NewerSegment).is_empty(), "already newest");
    st.apply(Action::OlderSegment);
    st.apply(Action::OlderSegment);
    assert_eq!(st.segment_index(), 0);
    assert!(st.apply(Action::OlderSegment).is_empty(), "already oldest");
    assert_eq!(st.watched_segment(), SegmentRef::Id(SegmentId(1)));
    st.apply(Action::NewerSegment);
    assert_eq!(st.segment_index(), 1);

    // An id the table no longer holds counts as the newest position.
    st.on_msg(segment_list(0, false, None));
    assert_eq!(st.segment_count(), 0);
    assert_eq!(st.segment_index(), 0);
    st.on_msg(DaemonMsg::SegmentList {
        seq: 9,
        entries: vec![ListEntry {
            id: SegmentId(50),
            row: list_row(SegmentKind::Trash, 0, 1),
        }],
        source: None,
        active: false,
        log_id: None,
    });
    assert_eq!(st.segment_index(), 0);
}

#[test]
fn pin_live_returns_to_the_live_meter_from_anywhere_but_is_a_no_op_when_there() {
    let mut st = on_meter();
    assert!(
        st.pin_live().is_empty(),
        "already following live on the meter"
    );

    st.apply(Action::OlderSegment);
    st.apply(Action::Open);
    let reqs = st.pin_live();
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            segment: SegmentRef::Live,
            drill: None,
            ..
        }
    ));
    assert!(st.drill.is_none());
    assert!(st.rows().is_empty(), "the stale snapshot is dropped");

    st.apply(Action::Back);
    assert_eq!(st.screen, Screen::List);
    let reqs = st.pin_live();
    assert!(!reqs.is_empty());
    assert_eq!(st.screen, Screen::Meter);
}

#[test]
fn set_list_selection_and_entries_are_position_aligned() {
    let mut st = ClientState::new();
    st.on_msg(segment_list(4, false, None));
    st.set_list_selection(2);
    assert_eq!(st.list_selection(), 2);
    assert_eq!(st.entries()[2].id, SegmentId(3));
    assert_eq!(st.list_rows()[2].name, st.entries()[2].row.name);
}

#[test]
fn arriving_mid_fight_jumps_to_the_live_meter_once() {
    let mut st = ClientState::new();
    let reqs = st.on_msg(segment_list(2, true, None));
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            segment: SegmentRef::Live,
            ..
        }
    ));
    assert_eq!(st.screen, Screen::Meter);
    // Backing out sticks: a later active list does not yank us back.
    st.apply(Action::Back);
    assert!(st.on_msg(segment_list(2, true, None)).is_empty());
    assert_eq!(st.screen, Screen::List);
}

#[test]
fn segment_opened_adds_a_placeholder_and_snaps_a_following_list_to_the_meter() {
    let mut st = ClientState::new();
    // Before the first list: the placeholder is added but nothing moves.
    assert!(
        st.on_msg(DaemonMsg::SegmentOpened { id: SegmentId(9) })
            .is_empty()
    );
    assert_eq!(st.entries().len(), 1);
    assert!(st.entries()[0].row.live);
    assert_eq!(st.entries()[0].row.kind, SegmentKind::Trash);

    st.on_msg(segment_list(2, false, None));
    let reqs = st.on_msg(DaemonMsg::SegmentOpened { id: SegmentId(3) });
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Segment {
            segment: SegmentRef::Live,
            ..
        }
    ));
    assert_eq!(st.screen, Screen::Meter);
    assert_eq!(st.entries().len(), 3);
    // Known id, already on the meter: nothing to add, nothing to do.
    assert!(
        st.on_msg(DaemonMsg::SegmentOpened { id: SegmentId(3) })
            .is_empty()
    );
    assert_eq!(st.entries().len(), 3);

    // Parked on history, a new pull does not steal the screen.
    st.apply(Action::OlderSegment);
    st.apply(Action::Back);
    assert!(!st.following_live());
    assert!(
        st.on_msg(DaemonMsg::SegmentOpened { id: SegmentId(4) })
            .is_empty()
    );
    assert_eq!(st.screen, Screen::List);
}

#[test]
fn load_failures_fatal_and_benign_messages_land_in_the_status() {
    let mut st = ClientState::new();
    for (error, text) in [
        (LoadError::NotFound, "segment not found"),
        (LoadError::Rotated, "segment gone: the log rotated"),
        (LoadError::Io("disk gone".to_string()), "disk gone"),
    ] {
        assert!(
            st.on_msg(DaemonMsg::LoadFailed {
                segment: SegmentId(1),
                error,
            })
            .is_empty()
        );
        assert_eq!(st.status.as_deref(), Some(text));
    }
    st.on_msg(DaemonMsg::Fatal("boom".to_string()));
    assert_eq!(st.status.as_deref(), Some("boom"));

    for benign in [
        DaemonMsg::HelloAck {
            proto: 1,
            version: "x".to_string(),
        },
        DaemonMsg::Status {
            req_id: 1,
            game_running: false,
            source: None,
            clients: 1,
            linger: false,
            overlay: OverlayState::Absent,
            history: HistoryStatus::default(),
        },
        DaemonMsg::SetVisible(false),
        DaemonMsg::Loadout {
            req_id: 1,
            guid: "A".to_string(),
            loadout: Some(Loadout::default()),
        },
    ] {
        assert!(st.on_msg(benign).is_empty());
    }
    assert_eq!(
        st.status.as_deref(),
        Some("boom"),
        "untouched by benign traffic"
    );
}

#[test]
fn a_rotated_log_resets_to_the_list_but_keeps_top_n_and_quit() {
    let mut st = ClientState::with_top_n(Some(3));
    st.on_msg(segment_list(3, false, Some("log.txt")));
    st.apply(Action::Open);
    st.apply(Action::Quit);
    assert_eq!(st.screen, Screen::Meter);

    // Rotation seen on a snapshot.
    let reqs = st.on_msg(snapshot(
        SegmentRef::Live,
        None,
        View::Damage,
        Vec::new(),
        None,
        1,
    ));
    assert!(reqs.is_empty(), "same source: no reset");
    let mut rotated = snapshot(SegmentRef::Live, None, View::Damage, Vec::new(), None, 1);
    if let DaemonMsg::Snapshot { source, .. } = &mut rotated {
        *source = Some("b.txt".to_string());
    }
    let reqs = st.on_msg(rotated);
    assert_eq!(watch_of(&reqs), &Cursor::List);
    assert_eq!(st.screen, Screen::List);
    assert_eq!(st.source.as_deref(), Some("b.txt"));
    assert!(st.entries().is_empty());
    assert!(st.quit, "quit survives the reset");
    st.on_msg(segment_list(1, false, Some("b.txt")));
    let reqs = st.apply(Action::Open);
    assert!(
        matches!(watch_of(&reqs), Cursor::Segment { top_n: Some(3), .. }),
        "top_n survives the reset"
    );

    // Rotation seen on a list.
    let reqs = st.on_msg(segment_list(2, false, Some("c.txt")));
    assert_eq!(watch_of(&reqs), &Cursor::List);
    assert_eq!(st.screen, Screen::List);

    // Rotation seen on a comparison snapshot.
    st.on_msg(segment_list(2, false, Some("c.txt")));
    st.apply(Action::Open);
    let reqs = st.on_msg(compare_snapshot(
        SegmentRef::Live,
        "A",
        "B",
        None,
        Some("d.txt"),
    ));
    assert_eq!(watch_of(&reqs), &Cursor::List);
    assert_eq!(st.source.as_deref(), Some("d.txt"));
}

#[test]
fn comparison_picks_open_the_screen_and_the_snapshot_must_match_the_pair() {
    let mut st = on_meter();
    // Pick via the keyboard: the highlighted row.
    st.apply(Action::Down);
    let reqs = st.apply(Action::PickCompare);
    assert!(
        matches!(watch_of(&reqs), Cursor::Segment { .. }),
        "one pick keeps the meter cursor"
    );
    assert_eq!(st.compare_picks(), &[("B".to_string(), "B".to_string())]);
    assert_eq!(st.compare_slot("B"), Some(0));
    assert_eq!(st.compare_slot("A"), None);
    assert_eq!(st.screen, Screen::Meter);
    // Off the compare screen the window and spell requests are ignored.
    assert!(st.set_compare_range(Some((0, 10))).is_empty());
    assert!(st.drill_compare_spell("Fireball", "Fireball").is_empty());

    let reqs = st.toggle_compare("A", "Ana");
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Compare { a, b, range: None, spell: None, .. } if a == "B" && b == "A"
    ));
    assert_eq!(st.screen, Screen::Compare);
    assert_eq!(st.compare_slot("A"), Some(1));

    // A snapshot for another pair, segment or screen is dropped.
    st.on_msg(compare_snapshot(
        SegmentRef::Live,
        "A",
        "B",
        None,
        Some("log.txt"),
    ));
    assert!(st.compare_sides().is_none(), "reversed pair is not ours");
    st.on_msg(compare_snapshot(
        SegmentRef::Id(SegmentId(1)),
        "B",
        "A",
        None,
        Some("log.txt"),
    ));
    assert!(st.compare_sides().is_none(), "other segment");
    st.on_msg(compare_snapshot(
        SegmentRef::Live,
        "B",
        "A",
        Some((5, 9)),
        Some("log.txt"),
    ));
    let (a, b) = st.compare_sides().expect("the matching pair lands");
    assert_eq!((a.guid.as_str(), b.guid.as_str()), ("B", "A"));
    assert_eq!(st.compare_shown_range(), Some((5, 9)));
    assert_eq!(st.status.as_deref(), Some("note"));
    assert_eq!(st.segment_name().as_deref(), Some("Boss"));

    // The window: degenerate clears, unchanged is silent, real re-Watches.
    assert!(
        st.set_compare_range(Some((10, 10))).is_empty(),
        "degenerate = None = unchanged"
    );
    let reqs = st.set_compare_range(Some((10, 20)));
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Compare {
            range: Some((10, 20)),
            ..
        }
    ));
    assert!(st.set_compare_range(Some((10, 20))).is_empty());

    // The graph toggle is local.
    st.apply(Action::ToggleGraph);
    assert_eq!(st.graph_mode(), GraphMode::Total);
    st.toggle_graph();
    assert_eq!(st.graph_mode(), GraphMode::Dps);

    // Unrelated keys do nothing here; Quit still quits.
    assert!(st.apply(Action::Up).is_empty());
    assert!(st.apply(Action::Open).is_empty());
    st.apply(Action::Quit);
    assert!(st.quit);
}

#[test]
fn a_third_pick_replaces_the_older_and_unpicking_closes_the_comparison() {
    let mut st = on_meter();
    st.toggle_compare("A", "A");
    st.toggle_compare("B", "B");
    st.toggle_compare("C", "C");
    assert_eq!(
        st.compare_picks()
            .iter()
            .map(|(g, _)| g.as_str())
            .collect::<Vec<_>>(),
        ["B", "C"]
    );
    assert_eq!(st.screen, Screen::Compare);
    let reqs = st.toggle_compare("B", "B");
    assert!(matches!(watch_of(&reqs), Cursor::Segment { .. }));
    assert_eq!(st.screen, Screen::Meter);
    assert_eq!(st.compare_picks().len(), 1);

    // Nothing to clear once the pair is gone.
    st.toggle_compare("C", "C");
    assert!(st.compare_picks().is_empty());
    assert!(st.clear_compare().is_empty());
}

#[test]
fn comparison_survives_segment_moves_and_pin_live_and_esc_leaves_it() {
    let mut st = on_meter();
    st.toggle_compare("A", "A");
    st.toggle_compare("B", "B");
    st.on_msg(compare_snapshot(
        SegmentRef::Live,
        "A",
        "B",
        None,
        Some("log.txt"),
    ));
    assert!(st.compare_sides().is_some());

    assert!(st.apply(Action::NewerSegment).is_empty(), "already newest");
    let reqs = st.apply(Action::OlderSegment);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Compare {
            segment: SegmentRef::Id(SegmentId(2)),
            ..
        }
    ));
    assert_eq!(st.screen, Screen::Compare);
    assert!(st.compare_sides().is_none(), "stale sides dropped");
    assert!(st.apply(Action::OlderSegment).len() == 1);
    assert!(st.apply(Action::OlderSegment).is_empty(), "oldest");
    let reqs = st.apply(Action::NewerSegment);
    assert!(matches!(watch_of(&reqs), Cursor::Compare { .. }));

    // pin_live keeps the pair and moves it onto the live fight.
    let reqs = st.pin_live();
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Compare {
            segment: SegmentRef::Live,
            ..
        }
    ));
    assert_eq!(st.screen, Screen::Compare);
    assert!(st.pin_live().is_empty(), "already there");

    // A comparison snapshot with no meter snapshot underneath still gives
    // the header its info.
    assert!(st.segment_name().is_none());
    st.on_msg(compare_snapshot(
        SegmentRef::Live,
        "A",
        "B",
        None,
        Some("log.txt"),
    ));
    assert_eq!(st.segment_name().as_deref(), Some("Boss"));
    assert_eq!(st.segment_count(), 3);

    // Esc (Back) and the pick key both leave the comparison for the meter.
    let reqs = st.apply(Action::Back);
    assert!(matches!(watch_of(&reqs), Cursor::Segment { .. }));
    assert_eq!(st.screen, Screen::Meter);
    assert!(st.compare_picks().is_empty());
}

#[test]
fn the_compare_ability_drill_needs_the_pair_and_pops_first() {
    let mut st = on_meter();
    st.toggle_compare("A", "A");
    st.toggle_compare("B", "B");
    let reqs = st.drill_compare_spell("Fireball", "Fireball");
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Compare { spell: Some(s), .. } if s == "Fireball"
    ));
    assert_eq!(
        st.compare_spell(),
        Some(&("Fireball".to_string(), "Fireball".to_string()))
    );
    // PickCompare on the compare screen pops one level, like Esc.
    let reqs = st.apply(Action::PickCompare);
    assert!(matches!(
        watch_of(&reqs),
        Cursor::Compare { spell: None, .. }
    ));
    assert_eq!(st.screen, Screen::Compare);
    // A pick resets the spell drill too.
    st.drill_compare_spell("Frostbolt", "Frostbolt");
    st.toggle_compare("C", "C");
    assert!(st.compare_spell().is_none());
}

#[test]
fn pick_compare_with_nothing_highlighted_does_nothing() {
    let mut st = ClientState::new();
    st.on_msg(segment_list(1, true, None));
    assert_eq!(st.screen, Screen::Meter);
    assert!(st.rows().is_empty());
    assert!(st.apply(Action::PickCompare).is_empty());
    assert!(st.apply(Action::Open).is_empty(), "no row to drill into");
    // A drill that is somehow already open is not reopened.
    st.drill = Some(Drill {
        key: "A".to_string(),
        label: "A".to_string(),
        pane: Pane::Spell,
        spell_sel: 0,
        target_sel: 0,
        spell: None,
    });
    assert!(st.apply(Action::Open).is_empty(), "empty by-spell pane");
}

#[test]
fn encounter_spans_skip_degenerate_and_misaligned_members() {
    let mut st = ClientState::new();
    st.screen = Screen::Meter;
    st.on_msg(DaemonMsg::SegmentList {
        seq: 1,
        entries: vec![
            ListEntry {
                id: SegmentId(1),
                row: ListRow {
                    instance: Some(0),
                    ..list_row(SegmentKind::Encounter, 1_000, 0)
                },
            },
            ListEntry {
                id: SegmentId(2),
                row: ListRow {
                    instance: Some(0),
                    ..list_row(SegmentKind::Encounter, 0, 5_000)
                },
            },
            ListEntry {
                id: SegmentId(3),
                row: ListRow {
                    instance: Some(1),
                    ..list_row(SegmentKind::Encounter, 9_000, 5_000)
                },
            },
        ],
        source: None,
        active: false,
        log_id: None,
    });
    st.screen = Screen::Meter;
    let overall = |instance: Option<u32>| DaemonMsg::Snapshot {
        seq: 2,
        segment: SegmentRef::Live,
        id: None,
        view: View::Damage,
        info: SegmentInfo {
            instance,
            start_ms: 2_000,
            ..info(SegmentKind::Overall)
        },
        rows: Vec::new(),
        total_rows: 0,
        breakdown: None,
        segment_count: 3,
        source: None,
        status: None,
    };
    st.on_msg(overall(None));
    assert!(
        st.encounter_spans().is_empty(),
        "an overall without a visit has no lane"
    );
    st.on_msg(overall(Some(0)));
    // Member 1 has zero duration; member 2 starts before the overall, so
    // its start is clamped to zero and its whole duration follows.
    assert_eq!(st.encounter_spans(), vec![(0, 5_000)]);
    // A non-overall segment draws no lane even inside a visit.
    st.on_msg(snapshot(
        SegmentRef::Live,
        None,
        View::Damage,
        Vec::new(),
        None,
        3,
    ));
    assert!(st.encounter_spans().is_empty());
}

#[test]
fn the_meter_graph_toggle_is_local_and_odd_states_still_watch_something() {
    let mut st = on_meter();
    assert!(st.apply(Action::ToggleGraph).is_empty());
    assert_eq!(st.graph_mode(), GraphMode::Total);

    // A view switched under an open ability drill: the cached breakdown
    // belongs to the other view, so the ability accessors go quiet.
    st.apply(Action::Open);
    st.on_msg(snapshot(
        SegmentRef::Live,
        Some(SegmentId(3)),
        View::Damage,
        vec![row("A", 300)],
        Some(drilled_breakdown()),
        3,
    ));
    st.apply(Action::Open);
    assert!(st.spell_timeline().is_some());
    st.view = View::Healing;
    assert!(st.spell_timeline().is_none());
    assert!(st.spell_target_rows().is_empty());
    assert!(st.drill_spell_row().is_none());

    // The compare screen with fewer than two picks cannot happen through
    // the API; forced, it falls back to a plain meter Watch.
    let mut st = on_meter();
    st.screen = Screen::Compare;
    assert!(matches!(
        st.initial_request(),
        ClientMsg::Watch(Cursor::Segment {
            drill: None,
            spell: None,
            ..
        })
    ));
}
