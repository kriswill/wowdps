//! The sweep's source fingerprint (`swept::SOURCES`): a hash of the Rust
//! source that decides which fights a read of a log yields and the history
//! store keeps. The sweep's manifest answers to it, so a build that changes
//! any of it reads every log once on its first start, and one that changes
//! none (the hub, the engine, a client) reads nothing again. No one has to
//! remember to bump a number: the code is the version.
//!
//! What is hashed, by unit:
//! - whole crates' `src` where the deciding code lives throughout (model;
//!   core: the parser, the scanner, the meter, the replay cut an import
//!   meters through, the generated tables), less [`LEFT_OUT`];
//! - modules elsewhere: `<m>.rs` or `<m>/mod.rs` and everything under
//!   `<m>/`, so a module split into files stays covered.
//!
//! What is not: test code — a top-level `#[cfg(test)] mod x { … }` block
//! that is its file's last item, formatted as rustfmt writes one (lines
//! indented up to a `}` alone in the first column, nothing but blank lines
//! after it), and the file of a `#[cfg(test)] mod x;` — and dot-files and
//! entries that are not regular files (an editor's lock, a dangling link).
//! Only `.rs` files count: the packages' source filter keeps those and
//! leaves Markdown out, so every build of one revision agrees. The cut reads
//! lines, not Rust: it could take production text only from an item
//! rustfmt leaves alone (a `macro_rules!` body, a `#[rustfmt::skip]` one)
//! that closes its file and holds first-column `#[cfg(test)]` and `mod x {`
//! lines of its own (`build/fingerprint.rs`, `strip_tests`).
//!
//! The build fails when a unit or a left-out file is missing (a rename the
//! lists must follow), or when a definition in [`DECIDING`] is not in the
//! hashed code — code that decides what a read stores, an entry point or a
//! helper of one, moved somewhere the fingerprint cannot see.

use std::path::PathBuf;

#[path = "build/fingerprint.rs"]
mod fingerprint;

use fingerprint::Unit;

const SOURCES: &[Unit] = &[
    Unit::Tree("../model/src"),
    Unit::Tree("../core/src"),
    Unit::Module("../proto/src/history"),
    Unit::Module("src/history"),
    Unit::Module("src/loader"),
    Unit::Module("src/cache"),
    Unit::Module("src/swept"),
];

/// Files inside the trees that decide no stored fight.
const LEFT_OUT: &[&str] = &[
    // The dispatcher and the install discovery: which folders are followed,
    // not what a read of a log yields.
    "../core/src/cli.rs",
    // Live tailing. An older log is read through `index::scan`; a tailed
    // log is never recorded, so which log is tailed decides nothing kept.
    "../core/src/tail.rs",
    // Test helpers (`cfg(test)` or the `testkit` feature).
    "../core/src/testkit.rs",
];

/// Definitions that decide what a read of a log yields and the store
/// keeps. Each must appear in the hashed code: one moved out of it (or
/// renamed) fails the build until the lists here follow it.
const DECIDING: &[&str] = &[
    // core: the reading.
    "pub fn parse_line(",
    "pub fn scan<",
    "pub fn scan_from<",
    "fn is_combat(",
    "pub fn load_segment_text(",
    "pub fn feed(",
    "pub fn cut_and_meter<",
    // core: what the cut keeps of a block's place (R29: a UiMap 0, by edition).
    "fn stood(",
    // proto: the identities and the card schema.
    "pub fn log_id(",
    "pub fn fight_id(",
    "pub const HISTORY_SCHEMA",
    // daemon: the resumed scan, the import, the store, retention.
    "fn resume_state(",
    "fn load_for(",
    "fn scan_next(",
    "fn enqueue_metas(",
    "fn fight_from_import(",
    "fn wants_meta(",
    "fn wants(",
    "fn store_impl(",
    "fn extract(",
    "fn retain(",
    "fn protected(",
    "fn is_progression(",
    "fn owner_in(",
    // R31: a card's owner — the fight's logger, from a log this daemon
    // follows, among the rest.
    "pub fn owner_of(",
    "pub fn logger_of(",
    "fn follows(",
    "pub fn logger(",
    "pub fn rules(",
    // daemon: the helpers they decide through — a log's identity, the
    // configured characters' matching, retention's groups and kept-whole
    // set, the tailed test, the sweep itself.
    "impl LogFacts",
    "fn name_matches(",
    "fn group_key(",
    "fn kept_whole(",
    "fn keeps_whole(",
    "fn same_file(",
    "fn sweep(",
];

fn main() -> Result<(), String> {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let hashed = fingerprint::hashed(&root, SOURCES, LEFT_OUT)?;
    for path in &hashed.watched {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    fingerprint::check_deciding(&hashed.files, DECIDING)?;
    println!(
        "cargo:rustc-env=WOWDPS_SWEEP_SOURCES={:016x}",
        fingerprint::fingerprint(&hashed.files)
    );
    Ok(())
}
