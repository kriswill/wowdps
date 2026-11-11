use std::time::{Duration, Instant};

use wowdps_daemon::mock::MockDaemon;
use wowdps_model::Edition;
use wowdps_proto::history::FightCard;
use wowdps_proto::{ClientMsg, EditionRef, EditionSource, HistoryAnswer, HistoryQuery};

use super::*;

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/fixtures")
        .join(name)
}

/// A store holding the Retail fixture's fights and, with `forever`, the
/// Forever fixture's: one store, as a daemon following both keeps.
fn store(forever: bool) -> MockDaemon {
    let mock = MockDaemon::fixture().with_history();
    if forever {
        mock.with_history_of(&fixture("forever.txt"))
    } else {
        mock
    }
}

/// Every question the census asks, answered by `mock`, until it asks
/// nothing more at `now`.
fn settle(census: &mut Census, mock: &mut MockDaemon, now: Instant) -> bool {
    let mut changed = false;
    let mut id = 100;
    while let Some(msg) = census.next_request(id, now) {
        for reply in mock.handle(msg) {
            if let wowdps_proto::DaemonMsg::History { req_id, answer } = reply {
                changed |= census.absorb(req_id, &answer).1;
            }
        }
        id += 1;
    }
    changed
}

fn card(edition: Edition, build: (u16, u16, u16), start: i64) -> FightCard {
    FightCard {
        id: format!("{}-{start}", edition.key()),
        project_id: edition.project_id(),
        build,
        start_utc_ms: start,
        ..FightCard::default()
    }
}

#[test]
fn retail_goes_by_its_expansion_and_forever_by_its_name() {
    assert_eq!(expansion(12), Some("Midnight"));
    assert_eq!(expansion(11), Some("The War Within"));
    assert_eq!(expansion(13), None, "an expansion this build does not know");
    assert_eq!(name(Edition::Retail, Some((12, 1, 5))), "Midnight");
    assert_eq!(name(Edition::Unknown(0), Some((12, 0, 0))), "Midnight");
    assert_eq!(name(Edition::Retail, Some((13, 0, 0))), "Retail");
    assert_eq!(name(Edition::Retail, Some((0, 0, 0))), "Retail");
    assert_eq!(name(Edition::Retail, None), "Retail");
    assert_eq!(name(Edition::Forever, Some((1, 60, 1))), "Forever");
    assert_eq!(name(Edition::Classic(2), None), "Classic");
    assert_eq!(name(Edition::Unknown(7), None), "Unknown edition");
}

#[test]
fn the_build_is_the_tooltip_s() {
    assert_eq!(
        tip(Edition::Retail, Some((12, 1, 5))),
        "Retail, build 12.1.5"
    );
    assert_eq!(tip(Edition::Retail, None), "Retail");
    assert_eq!(tip(Edition::Retail, Some((13, 0, 2))), "Build 13.0.2");
    assert_eq!(tip(Edition::Forever, Some((1, 60, 1))), "Build 1.60.1");
    assert_eq!(tip(Edition::Forever, None), "Forever");
    assert_eq!(tip(Edition::Unknown(7), None), "Unknown edition 7");
    assert_eq!(build_words(Some((0, 0, 0))), None);
}

#[test]
fn entry_ids_name_the_edition_by_its_key() {
    assert_eq!(entry_id(EditionRef::Follow), "edition-follow");
    assert_eq!(entry_id(EditionRef::Pin(Edition::Retail)), "edition-retail");
    assert_eq!(
        entry_id(EditionRef::Pin(Edition::Unknown(0))),
        "edition-retail"
    );
    assert_eq!(
        entry_id(EditionRef::Pin(Edition::Forever)),
        "edition-forever"
    );
    assert_eq!(
        entry_id(EditionRef::Pin(Edition::Classic(2))),
        "edition-classic-2"
    );
}

/// Forever's log carries no builds, and the talent viewer says so; every
/// other edition's does, and it says nothing.
#[test]
fn an_edition_without_logged_builds_says_so() {
    assert!(logs_builds(Edition::Retail));
    assert!(!logs_builds(Edition::Forever));
    assert_eq!(builds_gap(Some(Edition::Retail)), None);
    assert_eq!(builds_gap(None), None);
    assert_eq!(
        builds_gap(Some(Edition::Forever)).as_deref(),
        Some("Forever's combat log does not carry talents or gear.")
    );
}

/// Following, the window is in the live edition (Retail before one is
/// named); pinned, in the pin. History reads are scoped only once several
/// editions are known.
#[test]
fn the_window_s_edition_and_what_its_reads_ask_for() {
    assert_eq!(window_edition(EditionRef::Follow, None), Edition::Retail);
    assert_eq!(
        window_edition(EditionRef::Follow, Some(Edition::Forever)),
        Edition::Forever
    );
    assert_eq!(
        window_edition(EditionRef::Follow, Some(Edition::Unknown(0))),
        Edition::Retail
    );
    assert_eq!(
        window_edition(EditionRef::Pin(Edition::Forever), Some(Edition::Retail)),
        Edition::Forever
    );
    assert_eq!(history_edition(false, Edition::Forever), None);
    assert_eq!(
        history_edition(true, Edition::Forever),
        Some(Edition::Forever)
    );
    assert_eq!(
        history_edition(true, Edition::Unknown(0)),
        Some(Edition::Retail)
    );
}

/// A Forever fight names Forever wherever it is drawn (step 2's rule); a
/// Retail fight names its expansion only in another edition's window.
#[test]
fn a_fight_header_names_its_edition_where_another_could_be_meant() {
    let heroic = || "Heroic, 20 players".to_string();
    let b = Some((12, 0, 0));
    assert_eq!(
        header_meta(heroic(), Some(Edition::Retail), None, b),
        heroic()
    );
    assert_eq!(
        header_meta(heroic(), Some(Edition::Retail), Some(Edition::Retail), b),
        heroic()
    );
    assert_eq!(
        header_meta(heroic(), Some(Edition::Retail), Some(Edition::Forever), b),
        "Midnight, Heroic, 20 players"
    );
    assert_eq!(
        header_meta(
            String::new(),
            Some(Edition::Unknown(0)),
            Some(Edition::Forever),
            b
        ),
        "Midnight"
    );
    assert_eq!(
        header_meta(heroic(), Some(Edition::Forever), Some(Edition::Retail), b),
        "Forever, Heroic, 20 players"
    );
    assert_eq!(
        header_meta(heroic(), Some(Edition::Forever), None, b),
        "Forever, Heroic, 20 players"
    );
}

/// The census over a store of Retail's fights alone: Retail is known by
/// its fights and named by its newest card's build, Forever is asked about
/// twice (an empty answer may be a refused read) and stays unknown, and no
/// switch is drawn.
#[test]
fn one_edition_known_draws_no_switch() {
    let mut mock = store(false);
    let mut census = Census::new();
    let now = Instant::now();
    assert!(settle(&mut census, &mut mock, now), "Retail is known");
    assert_eq!(census.known(), vec![Edition::Retail]);
    assert!(!census.several());
    assert_eq!(census.name(Edition::Retail), "Midnight");
    assert_eq!(census.tip(Edition::Retail), "Retail, build 12.0.0");
    // Forever's empty answer is asked once more after the pause.
    let later = now + Duration::from_secs(2);
    assert!(census.next_request(1, now).is_none(), "the pause first");
    let again = census.next_request(1, later);
    assert!(
        matches!(
            again,
            Some(ClientMsg::GetHistory {
                query: HistoryQuery::Fights {
                    edition: Some(Edition::Forever),
                    limit: 1,
                    ..
                },
                ..
            })
        ),
        "{again:?}"
    );
    census.absorb(
        1,
        &HistoryAnswer::Fights {
            cards: Vec::new(),
            total: 0,
        },
    );
    assert!(
        census.next_request(2, later).is_none(),
        "the store's answer"
    );
    assert_eq!(entries(&census, EditionRef::Follow, None), None);
}

/// Both editions' fights in the store: both known, each named, and the
/// switch lists Follow, then Midnight (Retail), then Forever.
#[test]
fn two_editions_known_draw_the_switch() {
    let mut mock = store(true);
    let mut census = Census::new();
    settle(&mut census, &mut mock, Instant::now());
    assert_eq!(census.known(), vec![Edition::Retail, Edition::Forever]);
    assert!(census.several());
    assert_eq!(census.name(Edition::Forever), "Forever");
    assert_eq!(census.tip(Edition::Forever), "Build 1.60.1");
    let e = entries(&census, EditionRef::Follow, Some(Edition::Retail)).expect("a switch");
    let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec![FOLLOW, "Midnight", "Forever"]);
    assert!(e[0].on && !e[1].on && !e[2].on);
    assert_eq!(
        e[0].note.as_deref(),
        Some("Midnight"),
        "what Follow shows now"
    );
    assert_eq!(
        e[1].note.as_deref(),
        Some("Retail"),
        "the edition behind the name"
    );
    assert_eq!(e[2].note, None);
    assert_eq!(e[2].id, "edition-forever");
    let pinned = entries(
        &census,
        EditionRef::Pin(Edition::Forever),
        Some(Edition::Retail),
    )
    .expect("a switch");
    assert!(!pinned[0].on && pinned[2].on);
    let f = face(&census, EditionRef::Follow, Some(Edition::Retail));
    assert_eq!(f.name, "Midnight");
    assert_eq!(f.tip, "Following the game you play: Retail, build 12.0.0");
    let f = face(
        &census,
        EditionRef::Pin(Edition::Forever),
        Some(Edition::Retail),
    );
    assert_eq!((f.name.as_str(), f.following), ("Forever", false));
}

/// An edition the daemon follows is known before it has a fight, and a
/// card read elsewhere makes its edition known and names its build — the
/// newest card's, whatever order they come in.
#[test]
fn sources_and_cards_make_an_edition_known() {
    let mut census = Census::new();
    let source = |e: Option<Edition>| EditionSource {
        edition: e,
        ..EditionSource::default()
    };
    assert!(census.on_sources(&[source(Some(Edition::Retail)), source(None)]));
    assert_eq!(census.known(), vec![Edition::Retail]);
    assert!(
        !census.on_sources(&[source(Some(Edition::Retail))]),
        "no change"
    );
    assert!(census.learn([&card(Edition::Forever, (1, 60, 1), 20)]));
    assert!(census.several());
    census.learn([&card(Edition::Retail, (12, 0, 7), 50)]);
    census.learn([&card(Edition::Retail, (11, 2, 0), 10)]);
    assert_eq!(
        census.build(Edition::Retail),
        Some((12, 0, 7)),
        "the newest card's"
    );
    assert_eq!(census.name(Edition::Retail), "Midnight");
    // A source gone (the daemon restarted following less) un-knows an
    // edition known by it alone.
    let mut census = Census::new();
    census.on_sources(&[source(Some(Edition::Forever))]);
    assert_eq!(census.known(), vec![Edition::Forever]);
    assert!(census.on_sources(&[]));
    assert!(census.known().is_empty());
}

/// A store write asks again about the editions it held no fight of — at
/// most every `REPROBE_EVERY` — and never about one it already knows.
#[test]
fn a_store_write_asks_again_about_an_edition_with_no_fights() {
    let mut mock = store(false);
    let mut census = Census::new();
    let now = Instant::now();
    settle(&mut census, &mut mock, now);
    settle(&mut census, &mut mock, now + Duration::from_secs(1));
    census.store_changed();
    let asked = census.next_request(7, now + Duration::from_secs(2));
    assert!(
        matches!(
            &asked,
            Some(ClientMsg::GetHistory {
                query: HistoryQuery::Fights {
                    edition: Some(Edition::Forever),
                    ..
                },
                ..
            })
        ),
        "only Forever, which had none: {asked:?}"
    );
    census.absorb(
        7,
        &HistoryAnswer::Fights {
            cards: vec![card(Edition::Forever, (1, 60, 1), 5)],
            total: 1,
        },
    );
    assert!(census.several());
    census.store_changed();
    let soon = census.next_request(8, now + Duration::from_secs(3));
    assert!(soon.is_none(), "not again within the interval: {soon:?}");
}
