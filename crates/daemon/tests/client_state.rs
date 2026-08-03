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
    assert_eq!(rows.len(), 4, "the fixture's four segments are listed");
    assert_eq!(state.list_selection(), 3);
    assert!(rows.iter().all(|r| !r.live), "everything is history");
    assert_eq!(rows[1].name, "The Ashen Warden");
    assert_eq!(rows[1].success, Some(true));
    assert_eq!(rows[1].duration_ms, 60_000);
    assert_eq!(state.source.as_deref(), Some("sample.txt"));
}

#[test]
fn opening_a_listed_segment_lands_on_its_meter() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::Open);
    assert_eq!(state.screen, Screen::Meter);
    assert_eq!(state.segment_index(), 3);
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
    assert_eq!(state.segment_index(), 3);

    apply(&mut state, &mut mock, Action::OlderSegment);
    assert_eq!(state.segment_index(), 2);
    assert!(!state.following_live(), "stepping back unpins from live");
    assert!(!state.rows().is_empty());

    for _ in 0..5 {
        apply(&mut state, &mut mock, Action::OlderSegment);
    }
    assert_eq!(state.segment_index(), 0, "clamped at the oldest");

    for _ in 0..5 {
        apply(&mut state, &mut mock, Action::NewerSegment);
    }
    assert_eq!(state.segment_index(), 3);
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
    assert_eq!(state.list_selection(), 3, "cursor follows the segment");
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
fn quit_is_sticky_and_view_keys_work_on_the_list() {
    let (mut state, mut mock) = indexed();
    apply(&mut state, &mut mock, Action::SetView(View::Healing));
    assert_eq!(state.view, View::Healing);
    assert!(!state.quit);
    apply(&mut state, &mut mock, Action::Quit);
    assert!(state.quit);
}
