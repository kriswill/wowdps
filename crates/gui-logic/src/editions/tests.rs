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
                changed |= census.absorb(req_id, &answer, now).1;
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
    // No build known yet: the newest expansion, so the name does not
    // change when the first fight is stored.
    assert_eq!(newest_expansion(), "Midnight");
    assert_eq!(name(Edition::Retail, Some((0, 0, 0))), "Midnight");
    assert_eq!(name(Edition::Retail, None), "Midnight");
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
    assert_eq!(
        empty_stage("Forever"),
        (
            "No Forever fights yet".to_string(),
            "Play a fight in Forever with combat logging on and it shows up here.".to_string()
        )
    );
    assert_eq!(tip(Edition::Retail, Some((13, 0, 2))), "Build 13.0.2");
    assert_eq!(tip(Edition::Forever, Some((1, 60, 1))), "Build 1.60.1");
    assert_eq!(tip(Edition::Forever, None), "Forever");
    assert_eq!(tip(Edition::Unknown(7), None), "Unknown edition 7");
    assert_eq!(build_words(Some((0, 0, 0))), None);
}

#[test]
fn entry_ids_name_the_edition_by_its_key() {
    assert_eq!(entry_id(Edition::Retail), "edition-retail");
    assert_eq!(entry_id(Edition::Unknown(0)), "edition-retail");
    assert_eq!(entry_id(Edition::Forever), "edition-forever");
    assert_eq!(entry_id(Edition::Classic(2)), "edition-classic-2");
}

/// Picking the game the window would follow anyway is following; any
/// other is a pin. With nothing named live, every pick pins.
#[test]
fn a_pick_follows_the_game_being_followed_and_pins_any_other() {
    assert_eq!(
        choice_for(Edition::Retail, Some(Edition::Retail)),
        EditionRef::Follow
    );
    assert_eq!(
        choice_for(Edition::Unknown(0), Some(Edition::Retail)),
        EditionRef::Follow
    );
    assert_eq!(
        choice_for(Edition::Forever, Some(Edition::Retail)),
        EditionRef::Pin(Edition::Forever)
    );
    assert_eq!(
        choice_for(Edition::Retail, Some(Edition::Forever)),
        EditionRef::Pin(Edition::Retail)
    );
    assert_eq!(
        choice_for(Edition::Forever, None),
        EditionRef::Pin(Edition::Forever)
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

/// Following, the window is in the live edition — until one is named, the
/// edition it was in, so a pick never passes through Retail on its way;
/// pinned, in the pin. History reads are scoped only once several editions
/// are known.
#[test]
fn the_window_s_edition_and_what_its_reads_ask_for() {
    let follow = |live, last| window_edition(EditionRef::Follow, live, last);
    assert_eq!(follow(None, Edition::Retail), Edition::Retail);
    assert_eq!(follow(None, Edition::Forever), Edition::Forever, "kept");
    assert_eq!(
        follow(Some(Edition::Forever), Edition::Retail),
        Edition::Forever
    );
    assert_eq!(
        follow(Some(Edition::Unknown(0)), Edition::Forever),
        Edition::Retail
    );
    assert_eq!(
        window_edition(
            EditionRef::Pin(Edition::Forever),
            Some(Edition::Retail),
            Edition::Retail
        ),
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
/// Retail fight names its expansion only in another edition's window; a
/// stage with no fight names no edition.
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
    assert_eq!(
        header_meta(String::new(), None, Some(Edition::Forever), b),
        "",
        "nothing on the stage, nothing said"
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
        later,
    );
    assert!(
        census.next_request(2, later).is_none(),
        "the store's answer"
    );
    assert_eq!(entries(&census, Edition::Retail), None);
}

/// Both editions' fights in the store: both known, each named, and the
/// switch lists the games alone — Midnight, then Forever — the one shown
/// lit, the one being played noted, the face its name with the live dot
/// only while it is the game being played.
#[test]
fn two_editions_known_draw_the_switch() {
    let mut mock = store(true);
    let mut census = Census::new();
    settle(&mut census, &mut mock, Instant::now());
    assert_eq!(census.known(), vec![Edition::Retail, Edition::Forever]);
    assert!(census.several());
    assert_eq!(census.name(Edition::Forever), "Forever");
    assert_eq!(census.tip(Edition::Forever), "Build 1.60.1");
    let e = entries(&census, Edition::Retail).expect("a switch");
    let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Midnight", "Forever"],
        "the games, nothing else"
    );
    assert!(e[0].on && !e[1].on);
    assert_eq!((e[0].note, e[1].note), (None, None), "no game running");
    assert_eq!(e[0].tip, "Retail, build 12.0.0");
    assert_eq!(e[1].id, "edition-forever");
    let shown = entries(&census, Edition::Forever).expect("a switch");
    assert!(!shown[0].on && shown[1].on);
    let f = face(&census, Edition::Retail);
    assert_eq!((f.name.as_str(), f.playing), ("Midnight", false));

    // Retail's game runs: its row says so, and its face wears the dot.
    census.on_sources(&[EditionSource {
        edition: Some(Edition::Retail),
        live: true,
        running: true,
        ..EditionSource::default()
    }]);
    assert_eq!(census.live(), Some(Edition::Retail));
    let e = entries(&census, Edition::Forever).expect("a switch");
    assert_eq!((e[0].note, e[1].note), (Some(PLAYING_NOW), None));
    assert!(face(&census, Edition::Retail).playing);
    assert!(
        !face(&census, Edition::Forever).playing,
        "looking at another game: no dot"
    );
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
    // A source gone (the daemon restarted following less) leaves an
    // edition known by it alone known for the session: a switch once drawn
    // never goes away under the reader.
    let mut census = Census::new();
    census.on_sources(&[
        source(Some(Edition::Retail)),
        source(Some(Edition::Forever)),
    ]);
    assert!(census.several());
    census.on_sources(&[source(Some(Edition::Retail))]);
    assert_eq!(census.known(), vec![Edition::Retail, Edition::Forever]);
    assert!(census.several(), "still drawn");
}

/// A store write asks again about the editions it held no fight of, and
/// never about one it already knows.
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
        now + Duration::from_secs(2),
    );
    assert!(census.several());
}

/// Two store writes within `REPROBE_EVERY` ask once: the second round
/// waits out the interval, then asks.
#[test]
fn store_writes_ask_again_at_most_every_interval() {
    let mut mock = store(false);
    let mut census = Census::new();
    let now = Instant::now();
    settle(&mut census, &mut mock, now);
    settle(&mut census, &mut mock, now + Duration::from_secs(1));
    assert!(!census.several(), "Forever still has no fight");
    let empty = HistoryAnswer::Fights {
        cards: Vec::new(),
        total: 0,
    };
    let t0 = now + Duration::from_secs(2);
    census.store_changed();
    assert!(
        census.next_request(20, t0).is_some(),
        "the first round asks"
    );
    census.absorb(20, &empty, t0);
    census.store_changed();
    let soon = t0 + Duration::from_secs(1);
    // The first round's own retry (a pause after an empty answer) comes
    // due; answer it, and the second round still waits.
    let mut retries = 0;
    while let Some(ClientMsg::GetHistory { req_id, .. }) = census.next_request(21, soon) {
        census.absorb(req_id, &empty, soon);
        retries += 1;
    }
    assert_eq!(retries, 1, "the retry, once");
    assert!(
        census
            .next_request(22, t0 + Duration::from_secs(4))
            .is_none(),
        "the second round waits out the interval"
    );
    assert!(
        census
            .next_request(23, t0 + REPROBE_EVERY + Duration::from_millis(1))
            .is_some(),
        "then asks"
    );
}

/// A question lost with the connection is asked again.
#[test]
fn a_lost_question_is_asked_again() {
    let mut census = Census::new();
    let now = Instant::now();
    let first = census.next_request(1, now);
    assert!(first.is_some());
    assert!(census.next_request(2, now).is_none(), "one in flight");
    census.lost();
    let again = census.next_request(3, now);
    assert_eq!(
        format!("{first:?}").replace("req_id: 1", "req_id: 3"),
        format!("{again:?}"),
        "the same question again"
    );
}

// ---- marks and colors ---------------------------------------------------------

/// Each game has one mark: Midnight's crescent (Retail by its expansion,
/// the newest one before any build is known), Forever's loop, Classic's
/// hourglass, and the neutral ring for a game this build does not know —
/// and for Retail under an expansion with no mark of ours, so a new
/// expansion never wears Midnight's crescent under its own name.
#[test]
fn each_game_has_one_mark_and_retail_s_is_its_expansion_s() {
    assert_eq!(mark(Edition::Retail, Some((12, 1, 5))), MIDNIGHT);
    assert_eq!(mark(Edition::Retail, None), MIDNIGHT, "the newest's");
    assert_eq!(mark(Edition::Retail, Some((0, 0, 0))), MIDNIGHT);
    assert_eq!(
        mark(Edition::Unknown(0), None),
        MIDNIGHT,
        "counts as Retail"
    );
    assert_eq!(mark(Edition::Retail, Some((13, 0, 2))), NEUTRAL);
    assert_eq!(name(Edition::Retail, Some((13, 0, 2))), "Retail");
    assert_eq!(
        mark(Edition::Retail, Some((11, 2, 7))),
        NEUTRAL,
        "drawn for none"
    );
    assert_eq!(mark(Edition::Forever, Some((1, 60, 1))), FOREVER);
    assert_eq!(mark(Edition::Classic(2), None), CLASSIC);
    assert_eq!(mark(Edition::Unknown(40), None), NEUTRAL);
    let glyphs: std::collections::HashSet<_> = [MIDNIGHT, FOREVER, CLASSIC, NEUTRAL]
        .iter()
        .map(|m| m.glyph)
        .collect();
    assert_eq!(glyphs.len(), 4, "every mark its own shape");
    assert!(MIDNIGHT.own() && FOREVER.own() && CLASSIC.own());
    assert!(!NEUTRAL.own(), "no emblem stands for the neutral ring");
    assert_eq!(MIDNIGHT.tone, Tone::Midnight);
    assert_eq!(FOREVER.tone, Tone::Forever);
    assert_eq!(CLASSIC.tone, Tone::Muted);
}

fn themes() -> [(&'static str, &'static crate::theme::Def); 3] {
    [
        ("navy", &crate::theme::NAVY),
        ("onyx", &crate::theme::ONYX),
        ("frost", &crate::theme::FROST),
    ]
}

/// On every built-in theme the games' colors read at a glance on the bar
/// and on a menu (3:1 and more), differ from each other in lightness as
/// well as hue, keep clear of what the window's colors already mean
/// (the chrome, a kill, a wipe, a best), and carry words on a chip's fill
/// at AA — the muted fill of a game with no color too.
#[test]
fn the_games_colors_read_on_every_theme() {
    use crate::theme::{AA_CONTRAST, contrast, difference, oklab};
    for (name, def) in themes() {
        let t = &def.window;
        let menu = t.glass.over(t.ground);
        for tone in [Tone::Midnight, Tone::Forever] {
            let c = tone.color(t);
            for (on, under) in [("bar", t.surface), ("menu", menu)] {
                let ratio = contrast(c, under);
                assert!(ratio >= 3.0, "{name} {tone:?} on the {on}: {ratio}");
            }
            for (what, meaning) in [
                ("the chrome", t.accent),
                ("a kill", t.good),
                ("a wipe", t.bad),
                ("a best", t.legendary),
            ] {
                let d = difference(c, meaning);
                assert!(d >= NEAR, "{name} {tone:?} too like {what}: {d}");
            }
        }
        let (m, f) = (oklab(t.midnight).0, oklab(t.forever).0);
        assert!((f - m).abs() >= 0.15, "{name}: lightness {m} and {f}");
        assert!(difference(t.midnight, t.forever) >= 0.3, "{name}");
        for tone in [Tone::Midnight, Tone::Forever, Tone::Muted] {
            let fill = tone.fill(t);
            assert!(reads_on(fill, t), "{name} {tone:?}'s fill");
            assert!(contrast(ink_on(fill, t), fill) >= AA_CONTRAST);
        }
        let muted = contrast(Tone::Muted.color(t), t.surface);
        assert!(muted >= 3.0, "{name} muted: {muted}");
    }
}

/// A class chrome that looks like a game's color would read as that game
/// (a Warlock's lavender beside Midnight's violet): the mark then wears the
/// bar's ink and its shape tells the game. On the built-in themes that is
/// the Warlock's chrome beside Midnight's color and no other pair; a
/// theme's own chrome never takes a mark's color away, and the muted ink
/// is never swapped.
#[test]
fn a_chrome_too_like_a_game_s_color_takes_its_mark_to_ink() {
    use crate::theme::{CLASSES, chrome_base, difference};
    use wowdps_model::Class;
    for (name, def) in themes() {
        let t = &def.window;
        for tone in [Tone::Midnight, Tone::Forever, Tone::Muted] {
            assert_eq!(tone.mark_color(t, t.accent), tone.color(t), "{name}");
        }
        let near: Vec<(Class, Tone)> = CLASSES
            .into_iter()
            .flat_map(|class| {
                [Tone::Midnight, Tone::Forever]
                    .into_iter()
                    .filter(move |tone| tone.mark_color(t, chrome_base(class)) != tone.color(t))
                    .map(move |tone| (class, tone))
            })
            .collect();
        assert_eq!(near, vec![(Class::Warlock, Tone::Midnight)], "{name}");
        let warlock = chrome_base(Class::Warlock);
        assert!(difference(warlock, t.midnight) < NEAR);
        assert_eq!(Tone::Midnight.mark_color(t, warlock), t.ink_2);
        assert_eq!(Tone::Forever.mark_color(t, warlock), t.forever);
        assert_eq!(Tone::Muted.mark_color(t, t.ink_3), t.ink_3, "{name}");
    }
}

/// The live dot on a chip filled with a game's color: red on violet or
/// teal is lost, so it wears a ring of the bar's color; on the bar itself
/// it needs none.
#[test]
fn the_live_dot_wears_a_ring_where_a_fill_drowns_it() {
    for (name, def) in themes() {
        let t = &def.window;
        assert!(!dot_needs_ring(t.bad, t.surface), "{name}: the bar");
        for tone in [Tone::Midnight, Tone::Forever] {
            assert!(dot_needs_ring(t.bad, tone.fill(t)), "{name} {tone:?}");
        }
    }
}

/// The switch is a row of chips while it fits — up to three games on a
/// bar that is not narrow — and the dropdown otherwise. A chip's tooltip
/// is its game's build, and says so of the game being played.
#[test]
fn the_switch_is_chips_while_they_fit() {
    assert!(chips(2, false) && chips(3, false));
    assert!(!chips(4, false), "more than three fold into the dropdown");
    assert!(!chips(2, true), "a narrow bar has no room for them");
    let mut census = Census::new();
    let source = |e: Option<Edition>| EditionSource {
        edition: e,
        ..EditionSource::default()
    };
    census.on_sources(&[
        source(Some(Edition::Retail)),
        source(Some(Edition::Forever)),
    ]);
    let entries = entries(&census, Edition::Retail);
    let entry = |e: Edition| {
        entries
            .iter()
            .flatten()
            .find(|x| x.edition == e)
            .cloned()
            .expect("the entry")
    };
    let mut retail = entry(Edition::Retail);
    assert_eq!(retail.mark, MIDNIGHT);
    assert_eq!(entry(Edition::Forever).mark, FOREVER);
    retail.note = None;
    assert_eq!(chip_tip(&retail), retail.tip);
    retail.note = Some(PLAYING_NOW);
    assert_eq!(chip_tip(&retail), format!("{}, playing now", retail.tip));
}

/// With several games known, the game a header would lead its words with
/// is a tag of its own — the mark and the name — and the words are the
/// meta alone; with one game known the line is as it always was, and a
/// fight that names no other game has no tag.
#[test]
fn a_fight_header_tags_its_game_once_several_are_known() {
    let heroic = || "Heroic, 20 players".to_string();
    let b = Some((12, 0, 0));
    assert_eq!(
        header_parts(heroic(), Some(Edition::Forever), None, b),
        (None, "Forever, Heroic, 20 players".to_string()),
        "one game known: the words as ever"
    );
    let (tag, words) = header_parts(heroic(), Some(Edition::Forever), Some(Edition::Forever), b);
    assert_eq!(words, heroic());
    assert_eq!(
        tag,
        Some(Tag {
            edition: Edition::Forever,
            name: "Forever".to_string(),
            mark: FOREVER
        })
    );
    let (tag, words) = header_parts(heroic(), Some(Edition::Retail), Some(Edition::Forever), b);
    assert_eq!(words, heroic());
    assert_eq!(
        tag.map(|t| (t.name, t.mark)),
        Some(("Midnight".to_string(), MIDNIGHT))
    );
    assert_eq!(
        header_parts(heroic(), Some(Edition::Retail), Some(Edition::Retail), b),
        (None, heroic())
    );
    assert_eq!(
        header_parts(String::new(), None, Some(Edition::Forever), b),
        (None, String::new())
    );
}
