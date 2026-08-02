//! Test-only helpers shared by the `app` and `ui` test modules.
//!
//! Milestone 1 exercised the TUI against `stub.rs`, whose `Meter::new()` handed
//! back pre-baked demo segments. The real `Meter::new()` correctly starts empty,
//! so view/selection/render tests seed themselves by replaying the canonical
//! fixture through the real parser and meter — the same path the binary uses.

use crate::app::App;

pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample.txt");

pub fn fixture_lines() -> Vec<String> {
    std::fs::read_to_string(FIXTURE)
        .expect("fixtures/sample.txt is committed alongside the source")
        .lines()
        .map(str::to_string)
        .collect()
}

fn app_from(lines: &[String]) -> App {
    let mut app = App::new();
    for line in lines {
        app.feed_line(line);
    }
    // Feeding the whole fixture emulates the live path; these fixtures serve
    // the meter-screen tests, so land there like a live jump would.
    app.screen = crate::app::Screen::Meter;
    app
}

/// The whole fixture: 4 segments — Trash, "The Ashen Warden" (kill), Trash,
/// "Verkath the Hollow" (wipe). Every segment is closed.
pub fn fixture_app() -> App {
    app_from(&fixture_lines())
}

/// The fixture stopped just before its final `ENCOUNTER_END`, so the last
/// segment is still live — the state the meter is in while you're actually
/// raiding.
pub fn fixture_app_live() -> App {
    let lines = fixture_lines();
    let end = lines
        .iter()
        .rposition(|l| l.contains("ENCOUNTER_END"))
        .expect("fixture ends with an encounter");
    app_from(&lines[..end])
}

/// The indexed startup path against the fixture file: switched and indexed as
/// a stale (finished) log — the list screen, with nothing lazily loaded yet.
pub fn fixture_app_indexed() -> App {
    let bytes = std::fs::read(FIXTURE).expect("fixture exists");
    let index = crate::index::scan(&mut &bytes[..]);
    let mut app = App::new();
    app.on_tail(crate::tail::TailEvent::Switched(std::path::PathBuf::from(
        FIXTURE,
    )));
    app.on_tail(crate::tail::TailEvent::Index {
        index,
        file_age_ms: Some(60 * 60 * 1000),
    });
    app
}
