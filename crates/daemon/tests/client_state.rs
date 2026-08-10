//! The client-side state machine against the in-process mock daemon: the
//! same semantics the old `App` tests pinned, now exercised through the real
//! request/snapshot cycle.

use wowdps_core::model::{Screen, View};
use wowdps_daemon::mock::{MockDaemon, pump};
use wowdps_model::Action;
use wowdps_proto::ClientState;

fn indexed() -> (ClientState, MockDaemon) {
    let mut mock = MockDaemon::fixture();
    let mut state = ClientState::new();
    let first = state.initial_request();
    pump(&mut state, &mut mock, vec![first]);
    (state, mock)
}

fn live() -> (ClientState, MockDaemon) {
    let mut mock = MockDaemon::fixture_live();
    let mut state = ClientState::new();
    let first = state.initial_request();
    pump(&mut state, &mut mock, vec![first]);
    (state, mock)
}

fn apply(state: &mut ClientState, mock: &mut MockDaemon, action: Action) {
    let reqs = state.apply(action);
    pump(state, mock, reqs);
}

#[test]
fn a_finished_log_starts_on_the_list_with_the_newest_selected() {
    let (state, _mock) = indexed();
    assert_eq!(state.screen, Screen::List);
    let rows = state.list_rows();
    assert_eq!(rows.len(), 5, "four segments plus the visit overall (R10)");
    assert_eq!(state.list_selection(), 4);
    assert!(rows.iter().skip(1).all(|r| !r.live), "members are history");
    assert_eq!(rows[0].kind, wowdps_core::model::SegmentKind::Overall);
    assert_eq!(rows[0].name, "Sepulcher of the Ashen Vow");
    assert_eq!(rows[2].name, "The Ashen Warden");
    assert_eq!(rows[2].success, Some(true));
    assert_eq!(rows[2].duration_ms, 60_000);
    assert_eq!(state.source.as_deref(), Some("sample.txt"));
}

#[test]
fn opening_a_listed_segment_lands_on_its_meter() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    assert_eq!(state.screen, Screen::Meter);
    assert_eq!(state.segment_index(), 4);
    assert_eq!(state.segment_name().as_deref(), Some("Verkath the Hollow"));
    assert_eq!(
        state.segment_success(),
        Some(false),
        "the wipe reads as one"
    );
    assert_eq!(state.duration_ms(), 45_000);
    assert!(!state.is_live(), "history is never LIVE");
    assert!(!state.rows().is_empty(), "the lazily loaded rows are there");
    assert!(state.following_live(), "the newest row pins to live");
}

#[test]
fn bracket_navigation_walks_history_and_repins_at_the_end() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    assert_eq!(state.segment_index(), 4);

    apply(&mut state, &mut mock, Action::OlderSegment);
    assert_eq!(state.segment_index(), 3);
    assert!(!state.following_live(), "stepping back unpins from live");
    assert!(!state.rows().is_empty());

    for _ in 0..5 {
        apply(&mut state, &mut mock, Action::OlderSegment);
    }
    assert_eq!(state.segment_index(), 0, "clamped at the oldest");
    // R10: the head of the list is the visit's Overall — the arrows reach it.
    assert_eq!(
        state.segment_name().as_deref(),
        Some("Sepulcher of the Ashen Vow"),
        "the oldest position serves the visit overall"
    );
    assert!(!state.rows().is_empty(), "merged rows arrive");

    for _ in 0..5 {
        apply(&mut state, &mut mock, Action::NewerSegment);
    }
    assert_eq!(state.segment_index(), 4);
    assert!(state.following_live(), "reaching the newest re-pins");
}

#[test]
fn the_drilldown_follows_the_player_across_views_and_stays_fresh() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    apply(&mut state, &mut mock, Action::OlderSegment);
    apply(&mut state, &mut mock, Action::OlderSegment);
    assert_eq!(state.segment_name().as_deref(), Some("The Ashen Warden"));

    apply(&mut state, &mut mock, Action::Open); // drill into the top row
    let who = state.drill.as_ref().unwrap().key.clone();
    assert_eq!(state.rows()[0].key, who);
    let (by_spell, by_target) = state.breakdown();
    assert!(!by_spell.is_empty() && !by_target.is_empty());

    // Interrupts rank the same players in a different order.
    apply(&mut state, &mut mock, Action::SetView(View::Interrupts));
    assert_ne!(state.rows()[0].key, who, "this view must re-order");
    assert_eq!(
        state.drill.as_ref().unwrap().key,
        who,
        "drill follows the guid"
    );
    assert!(!state.breakdown().0.is_empty(), "and still resolves them");
}

#[test]
fn selection_moves_locally_without_a_round_trip() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    assert!(state.rows().len() > 1);

    // Up/Down are pure clamps against the cached snapshot: no requests.
    assert!(
        state.apply(Action::Down).is_empty(),
        "Down must not round-trip"
    );
    assert!(state.apply(Action::Up).is_empty(), "Up must not round-trip");
    for _ in 0..20 {
        assert!(state.apply(Action::Down).is_empty());
    }
    assert_eq!(state.row_sel, state.rows().len() - 1);
    for _ in 0..20 {
        assert!(state.apply(Action::Up).is_empty());
    }
    assert_eq!(state.row_sel, 0);
    let _ = mock;
}

#[test]
fn esc_walks_out_of_the_drilldown_then_back_to_the_list() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    apply(&mut state, &mut mock, Action::Open); // drill in
    assert!(state.drill.is_some());
    apply(&mut state, &mut mock, Action::Back);
    assert!(state.drill.is_none());
    assert_eq!(state.screen, Screen::Meter);
    apply(&mut state, &mut mock, Action::Back);
    assert_eq!(state.screen, Screen::List);
    assert_eq!(state.list_selection(), 4, "cursor follows the segment");
}

#[test]
fn an_in_progress_fight_jumps_straight_to_its_live_meter() {
    let (state, _mock) = live();
    assert_eq!(state.screen, Screen::Meter, "mid-fight: skip the list");
    assert!(state.following_live());
    assert!(state.is_live());
    assert_eq!(state.segment_name().as_deref(), Some("Verkath the Hollow"));
    assert!(!state.rows().is_empty(), "the live fight has data");
}

#[test]
fn fresh_combat_snaps_the_list_to_the_live_meter_but_backing_out_sticks() {
    let (mut state, mut mock) = live();
    // Back out mid-fight: the list must hold…
    apply(&mut state, &mut mock, Action::Back);
    assert_eq!(state.screen, Screen::List);
    let more = vec![
        "7/27/2026 22:30:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil".to_string(),
    ];
    let replies = mock.feed(more);
    let mut reqs = Vec::new();
    for m in replies {
        reqs.extend(state.on_msg(m));
    }
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.screen, Screen::List, "same pull: no snap-back");

    // …until the next pull begins: the open encounter ends, a new one opens.
    let next_pull = vec![
        "7/27/2026 22:44:00.000-7  ENCOUNTER_END,3184,\"Verkath the Hollow\",16,20,0,45000"
            .to_string(),
        "7/27/2026 22:45:00.000-7  ENCOUNTER_START,3185,\"The Next One\",16,20,2913".to_string(),
        "7/27/2026 22:45:05.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil".to_string(),
    ];
    let replies = mock.feed(next_pull);
    let mut reqs = Vec::new();
    for m in replies {
        reqs.extend(state.on_msg(m));
    }
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.screen, Screen::Meter, "a new pull pulls you in");
    assert!(state.following_live());
    assert!(state.is_live());
}

#[test]
fn segments_closed_inside_one_flush_burst_stay_navigable() {
    // The game flushes combat-log writes in multi-minute bursts, so one tail
    // batch can carry whole fights. Only a batch's still-open tail gets a
    // `SegmentOpened`; the daemon's list broadcast is what keeps every
    // client's id table complete — without it, stepping back re-pins Live.
    let (mut state, mut mock) = live();
    assert!(state.following_live());
    let count0 = state.segment_count();

    // One burst: the open fight ends, a trash pull opens *and closes*, and
    // the next encounter opens — three boundary crossings, one batch.
    let burst = vec![
        "7/27/2026 22:44:00.000-7  ENCOUNTER_END,3131,\"Verkath the Hollow\",16,20,0,45000"
            .to_string(),
        "7/27/2026 22:44:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Straggler\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil".to_string(),
        "7/27/2026 22:45:00.000-7  ENCOUNTER_START,3185,\"The Next One\",16,20,2913".to_string(),
        "7/27/2026 22:45:05.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil".to_string(),
    ];
    let replies = mock.feed(burst);
    let mut reqs = Vec::new();
    for m in replies {
        reqs.extend(state.on_msg(m));
    }
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.segment_count(), count0 + 2);
    assert!(state.following_live());

    // ◀ lands on the trash pull that never announced itself…
    apply(&mut state, &mut mock, Action::OlderSegment);
    assert!(
        !state.following_live(),
        "stepping back must unpin from live"
    );
    assert_eq!(state.segment_index(), count0);

    // …and ◀ again on the fight that closed inside the same burst.
    apply(&mut state, &mut mock, Action::OlderSegment);
    assert_eq!(state.segment_index(), count0 - 1);
    assert_eq!(state.segment_name().as_deref(), Some("Verkath the Hollow"));

    for _ in 0..3 {
        apply(&mut state, &mut mock, Action::NewerSegment);
    }
    assert!(state.following_live(), "walking forward re-pins");
}

// ---- R12: the comparison round trip ---------------------------------------

/// Open the wipe's meter, which has several players on it.
fn on_a_meter() -> (ClientState, MockDaemon) {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    (state, mock)
}

#[test]
fn one_pick_shows_nothing_and_two_picks_open_the_comparison() {
    let (mut state, mut mock) = on_a_meter();
    let rows = state.rows();
    assert!(rows.len() >= 2, "need two players to compare");
    let (a, b) = (rows[0].clone(), rows[1].clone());

    // First pick: badged, but the meter stays up — a half-made comparison
    // has nothing to show.
    let reqs = state.toggle_compare(&a.key, &a.label);
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.screen, Screen::Meter);
    assert_eq!(state.compare_slot(&a.key), Some(0));
    assert!(state.compare_sides().is_none());

    // Second pick opens it, and the daemon answers with both sides.
    let reqs = state.toggle_compare(&b.key, &b.label);
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.screen, Screen::Compare);
    let (sa, sb) = state.compare_sides().expect("comparison arrived");
    assert_eq!(sa.guid, a.key);
    assert_eq!(sb.guid, b.key);
    assert_eq!(sa.total.amount, a.amount, "same totals as the meter row");
    assert!(!sa.spells.is_empty(), "per-spell rows for the tables");
    assert!(
        sa.spells.iter().all(|r| r.count > 0),
        "hits back the crit% and average columns"
    );
    assert!(!sa.timeline.buckets.is_empty(), "a curve to draw");
    assert_eq!(sa.timeline.bucket_ms, 1_000);
}

#[test]
fn unpicking_closes_the_comparison_and_a_third_pick_replaces_the_older() {
    let (mut state, mut mock) = on_a_meter();
    let rows = state.rows();
    assert!(rows.len() >= 3, "need three players");
    let (a, b, c) = (rows[0].clone(), rows[1].clone(), rows[2].clone());

    for r in [&a, &b] {
        let reqs = state.toggle_compare(&r.key, &r.label);
        pump(&mut state, &mut mock, reqs);
    }
    assert_eq!(state.screen, Screen::Compare);

    // A third pick keeps a pair rather than demanding a clear step.
    let reqs = state.toggle_compare(&c.key, &c.label);
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.screen, Screen::Compare);
    assert_eq!(state.compare_slot(&a.key), None, "the oldest pick dropped");
    assert_eq!(state.compare_slot(&b.key), Some(0));
    assert_eq!(state.compare_slot(&c.key), Some(1));

    // Unpicking one breaks the pair and falls back to the meter.
    let reqs = state.toggle_compare(&b.key, &b.label);
    pump(&mut state, &mut mock, reqs);
    assert_eq!(state.screen, Screen::Meter);
    assert!(state.compare_sides().is_none());
    assert!(!state.rows().is_empty(), "the meter is served again");
}

#[test]
fn segment_navigation_keeps_the_comparison_open() {
    let (mut state, mut mock) = on_a_meter();
    let rows = state.rows();
    let picks: Vec<_> = rows.iter().take(2).cloned().collect();
    for r in &picks {
        let reqs = state.toggle_compare(&r.key, &r.label);
        pump(&mut state, &mut mock, reqs);
    }
    assert_eq!(state.screen, Screen::Compare);
    let here = state.segment_index();

    // Step to the older segment: the pair sticks, the sides are the new
    // segment's, and the header follows.
    apply(&mut state, &mut mock, Action::OlderSegment);
    assert_eq!(state.screen, Screen::Compare, "diff sticks across [ ]");
    assert_eq!(state.segment_index(), here - 1);
    assert_eq!(state.compare_slot(&picks[0].key), Some(0));
    assert_eq!(state.compare_slot(&picks[1].key), Some(1));
    let (sa, sb) = state.compare_sides().expect("new segment's comparison");
    assert_eq!(sa.guid, picks[0].key);
    assert_eq!(sb.guid, picks[1].key);

    // And back to the newest, which re-pins Live — still comparing.
    apply(&mut state, &mut mock, Action::NewerSegment);
    assert_eq!(state.screen, Screen::Compare);
    assert_eq!(state.segment_index(), here);
    assert!(state.compare_sides().is_some());
}

#[test]
fn esc_leaves_the_comparison_and_the_graph_toggle_is_local() {
    let (mut state, mut mock) = on_a_meter();
    let rows = state.rows();
    for r in rows.iter().take(2) {
        let reqs = state.toggle_compare(&r.key, &r.label);
        pump(&mut state, &mut mock, reqs);
    }
    assert_eq!(state.screen, Screen::Compare);

    // The graph mode never round-trips: both curves come from one set of
    // buckets the client already holds.
    let before = state.graph_mode();
    let reqs = state.apply(Action::ToggleGraph);
    assert!(reqs.is_empty(), "no request for a local toggle");
    assert_ne!(state.graph_mode(), before);

    apply(&mut state, &mut mock, Action::Back);
    assert_eq!(state.screen, Screen::Meter);
    assert!(state.compare_picks().is_empty());
}

#[test]
fn quit_is_sticky_and_view_keys_work_on_the_list() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::SetView(View::Healing));
    assert_eq!(state.view, View::Healing);
    assert!(!state.quit);
    apply(&mut state, &mut mock, Action::Quit);
    assert!(state.quit);
}
