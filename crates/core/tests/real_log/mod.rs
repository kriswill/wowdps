//! What the real-log gates share: which log each reads, by the edition its
//! header names. A Retail gate reads `WOWDPS_REAL_LOG`; its Forever variant
//! reads `WOWDPS_REAL_LOG_FOREVER`, or `WOWDPS_REAL_LOG` when that log's
//! header names Forever, so one run can check a log of each:
//!
//! ```sh
//! WOWDPS_REAL_LOG=<Retail log> WOWDPS_REAL_LOG_FOREVER=<Forever log> \
//!   cargo test --release -p wowdps-core -- --ignored real_log --nocapture
//! ```
//!
//! A real log carries real names. A Forever variant prints and asserts on
//! no name and no guid, a failure included: a fight is named by its kind,
//! its place in the log and its encounter id, a player by their place in a
//! roster sorted by guid. A Retail gate passes over a Forever log, which
//! only the variants read.

// The gates' shared helpers are test code outside any `#[test]` function,
// where clippy's test allowances do not reach: a panic is how they fail.
#![allow(dead_code, clippy::expect_used, clippy::panic)]

use wowdps_core::meter::{Segment, SegmentKind};
use wowdps_core::parser::{Event, parse_line};
use wowdps_model::Edition;

/// The variable a Retail gate reads.
pub const RETAIL: &str = "WOWDPS_REAL_LOG";
/// The variable a Forever variant reads first.
pub const FOREVER: &str = "WOWDPS_REAL_LOG_FOREVER";

/// The edition a log's header names (its first line's `PROJECT_ID`, as it
/// counts), Retail for a log whose first line names none.
pub fn edition_of(path: &str) -> Edition {
    let mut head = String::new();
    let file = std::fs::File::open(path).expect("open the log");
    std::io::BufRead::read_line(&mut std::io::BufReader::new(file), &mut head)
        .expect("read the header");
    match parse_line(head.trim_end()).map(|l| l.event) {
        Some(Event::Version { project_id, .. }) => Edition::from_project_id(project_id).counts_as(),
        _ => Edition::Retail,
    }
}

/// The log a Retail gate reads: `WOWDPS_REAL_LOG`, required. `None`, with a
/// line saying so, for a Forever log: its gate is the Forever variant,
/// which prints no name.
pub fn retail_log() -> Option<String> {
    let path = std::env::var(RETAIL).unwrap_or_else(|_| panic!("set {RETAIL}"));
    if edition_of(&path) == Edition::Forever {
        println!("{RETAIL} is a Forever log: the Forever variant reads it");
        return None;
    }
    Some(path)
}

/// The log a Forever variant reads: `WOWDPS_REAL_LOG_FOREVER`, else
/// `WOWDPS_REAL_LOG` when its header names Forever. `None`, with a line
/// saying so, when neither names a Forever log, so a run over a Retail log
/// alone passes the variants over. A log the Forever variable names whose
/// header names another edition fails, saying only which edition it names.
pub fn forever_log() -> Option<String> {
    if let Ok(path) = std::env::var(FOREVER) {
        let edition = edition_of(&path);
        assert_eq!(
            edition,
            Edition::Forever,
            "{FOREVER} names a log of {edition:?}, not a Forever log"
        );
        return Some(path);
    }
    match std::env::var(RETAIL) {
        Ok(path) if edition_of(&path) == Edition::Forever => Some(path),
        _ => {
            println!("no Forever log: set {FOREVER} to run the Forever variant");
            None
        }
    }
}

/// A fight as a Forever variant names it: its kind and place among the
/// fights checked, and an encounter's id, size and outcome. Never its name:
/// a trash fight in the open world can be named after a player.
pub fn fight(n: usize, seg: &Segment) -> String {
    let kind = match seg.kind {
        SegmentKind::Encounter => "pull",
        SegmentKind::Trash => "trash",
        SegmentKind::Overall => "overall",
    };
    match seg.encounter.as_ref() {
        Some(e) => format!(
            "{kind} #{n} (encounter {}, size {}, {})",
            e.id,
            e.group_size,
            match seg.success {
                Some(true) => "kill",
                Some(false) => "wipe",
                None => "open",
            }
        ),
        None => format!("{kind} #{n}"),
    }
}

/// Where `guid` stands in `roster` (sorted, so both sides of a comparison
/// number it the same): `#3`. `#?` for one the roster lacks.
pub fn place(roster: &[&String], guid: &str) -> String {
    roster
        .iter()
        .position(|g| g.as_str() == guid)
        .map_or_else(|| "#?".to_string(), |i| format!("#{i}"))
}
