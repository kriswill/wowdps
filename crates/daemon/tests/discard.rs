//! R11: the trash can. Discarding drops closed out-of-instance Trash from
//! the list; the live segment and every visit member (their Σ needs them)
//! survive. Driven straight through the engine with the instance fixture,
//! which has city trash outside the visits and a live trailing pull inside
//! one.

use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::Engine;
use wowdps_model::SegmentKind;

const INSTANCE_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/instance.txt");

fn engine_with_fixture() -> Engine {
    let text = std::fs::read_to_string(INSTANCE_FIXTURE);
    assert!(text.is_ok(), "{INSTANCE_FIXTURE}: unreadable fixture");
    let text = text.unwrap_or_default();
    let mut e = Engine::new();
    let mut events = Vec::new();
    e.on_tail(
        TailEvent::Lines(text.lines().map(str::to_string).collect()),
        &mut events,
    );
    e
}

#[test]
fn discard_drops_world_trash_and_keeps_visits_and_the_live_segment() {
    let mut e = engine_with_fixture();
    let before = e.list_rows();
    let world_trash = |rows: &[wowdps_model::ListRow]| {
        rows.iter()
            .filter(|r| r.kind == SegmentKind::Trash && r.instance.is_none() && !r.live)
            .count()
    };
    assert_eq!(world_trash(&before), 2, "two city-dummy segments listed");

    e.discard_trash();
    let after = e.list_rows();
    assert_eq!(world_trash(&after), 0, "city trash gone");
    assert_eq!(
        after.len(),
        before.len() - 2,
        "nothing else was touched: visits, members and Σ rows survive"
    );
    assert!(
        after.iter().any(|r| r.live),
        "the live trailing pull is still listed"
    );
    assert_eq!(
        after
            .iter()
            .filter(|r| r.kind == SegmentKind::Overall)
            .count(),
        before
            .iter()
            .filter(|r| r.kind == SegmentKind::Overall)
            .count(),
        "every visit Σ survives"
    );
}
