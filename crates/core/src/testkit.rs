//! Test-only helpers shared across the workspace's test suites: the
//! canonical fixture, and its lines.

pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample.txt");

/// R10 fixture: two instance visits (a completed +12 key, then a mythic
/// dungeon left and re-entered mid-visit) with city combat between them.
pub const INSTANCE_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/instance.txt");

pub fn fixture_lines() -> Vec<String> {
    // The fixture is committed alongside the source; an unreadable one yields
    // an empty vec, which every caller's assertions reject anyway.
    std::fs::read_to_string(FIXTURE)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}
