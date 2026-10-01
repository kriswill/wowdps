//! The screen Home's panels draw — its grid, its words for each state of
//! the store, its scope chips, its panels and charts — over the hand-made
//! cards gui-logic's `home::samples` builds (the derivation's own tests
//! are gui-logic's).

use super::charts::ParBar;
use super::panels::{MAX_TILES, columns_for, grid_columns, panel_count};
use super::*;
use crate::window::testkit::simulator;
use wowdps_gui_logic::home::samples::{self, *};
use wowdps_gui_logic::rail::Mark;
use wowdps_model::Spec;
use wowdps_proto::history::FightCard;

/// The par bar draws its run against the timer through gui-logic's
/// `par_x`: a quarter past the timer at the end.
#[test]
fn the_par_bar_places_its_ticks() {
    let bar = ParBar {
        clock_ms: 1_900_000,
        pars: (1_800_000, 1_440_000, 1_080_000),
        timed: false,
    };
    assert_eq!(bar.x_of(1_800_000, 250.0), 200.0, "par at 80%");
    assert_eq!(bar.x_of(9_000_000, 250.0), 250.0, "clamped at the end");
}

/// A wide grid of three, one column at the default 460 — and at the
/// prototype's 330 px a panel, two across a tiled window.
#[test]
fn the_panel_grid_follows_the_width() {
    assert_eq!(columns_for(436.0, 18.0), 1, "the default window");
    assert_eq!(columns_for(916.0, 18.0), 2, "a tiled window");
    assert_eq!(columns_for(1159.0, 18.0), 3, "wide, beside the rail");
    assert_eq!(columns_for(5000.0, 18.0), 3, "never more than three");
    // Degenerate widths must not divide by anything.
    assert_eq!(columns_for(0.0, 18.0), 1);
    assert_eq!(columns_for(f32::NAN, 18.0), 1);
    assert_eq!(columns_for(f32::INFINITY, 18.0), 1);
    // Every week shows the prototype's three panels, and one more per
    // further raid; never more columns than panels (`auto-fit` collapses
    // an empty track), so no row leaves a hole beside its panels.
    let mut panels = Panels::default();
    assert_eq!(panel_count(&panels), 3, "an empty week's three");
    assert_eq!(grid_columns(1159.0, panel_count(&panels)), 3);
    assert_eq!(grid_columns(1159.0, 2), 2, "two panels share the row");
    assert_eq!(grid_columns(1159.0, 0), 1);
    assert_eq!(grid_columns(436.0, 3), 1);
    let raid = || RaidPanel {
        title: String::new(),
        bosses: Vec::new(),
        newest: 0,
    };
    panels.raids = vec![raid(), raid()];
    assert_eq!(panel_count(&panels), 4, "a second raid of the week");
}
/// Loading, empty, off and degraded are four different screens — never
/// the same confident nothing. Until the store is on and has answered,
/// no panel says "none" of what nobody read: the night card, captioned
/// neutrally, carries the state's own words, and the grid waits.
#[test]
fn the_screen_words_each_empty_store_apart() {
    let draw = |home: &Home, panels: &Panels| {
        simulator(screen(home, panels, &chars(), theme::GOLD_ACCENT, false, 0))
    };
    let loading = Home::new();
    let mut ui = draw(&loading, &Panels::default());
    assert!(ui.find(panels::EMPTY_CAP).is_ok(), "a neutral caption");
    assert!(ui.find("Last night you played").is_err());
    assert!(
        ui.find("Your week is on its way from the history store.")
            .is_ok()
    );
    for none in [
        "No pulls stored this week.",
        "No keys this week.",
        "No raid pulls this week.",
    ] {
        assert!(ui.find(none).is_err(), "{none}: not empty, unread");
    }
    let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();

    let mut empty = Home::new();
    empty.answered = true;
    let mut ui = draw(&empty, &Panels::default());
    assert!(ui.find(panels::EMPTY_CAP).is_ok());
    assert!(ui.find("No pulls stored this week.").is_ok());
    assert!(ui.find("No keys this week.").is_ok());
    assert!(ui.find("No raid pulls this week.").is_ok());
    assert!(
        ui.find("Your week is on its way from the history store.")
            .is_err()
    );

    let mut off = empty;
    off.disabled_reason = Some("history_enabled = false".to_string());
    let mut ui = draw(&off, &Panels::default());
    assert!(
        ui.find("The history store is off: history_enabled = false.")
            .is_ok()
    );
    assert!(ui.find("No keys this week.").is_err(), "off is not empty");

    let mut degraded = Home::new();
    degraded.answered = true;
    degraded.dropped = 3;
    let mut ui = draw(&degraded, &Panels::default());
    assert!(
        ui.find("The daemon dropped 3 requests: this is not the whole week.")
            .is_ok()
    );
    assert!(ui.find("No keys this week.").is_ok(), "the week it has");
    degraded.dropped = 1;
    let mut ui = draw(&degraded, &Panels::default());
    assert!(
        ui.find("The daemon dropped 1 request: this is not the whole week.")
            .is_ok()
    );

    let unowned = Panels {
        unowned: true,
        ..Panels::default()
    };
    let mut ui = draw(&degraded, &unowned);
    assert!(
        ui.find(
            "The store has not named a character of yours yet: list them in \
             history_characters, or install the wowdps addon."
        )
        .is_ok()
    );
}

/// The chips: all characters, then each one; the scoped one pressed and
/// named in the title; a press is the scope's message.
#[test]
fn the_scope_chips_name_and_scope_the_week() {
    let sun = evening("2026-09-27");
    let cards = vec![
        boss("c", "The Coiled Altar", 1, sun + H, true, raid_of(250.0)),
        sigma("v", "The Venomous Abyss", sun, 15),
    ];
    let mut home = Home::new();
    samples::absorb(&mut home, cards.clone());
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    let mut ui = simulator(screen(
        &home,
        &panels,
        &chars(),
        theme::GOLD_ACCENT,
        true,
        0,
    ));
    assert!(ui.find("You, this week").is_ok());
    for chip in ["All characters", "Me", "Alt"] {
        assert!(ui.find(chip).is_ok(), "{chip}");
    }
    // The night, its tile and its standing.
    assert!(ui.find("The Venomous Abyss, Heroic").is_ok());
    assert!(ui.find("The Coiled Altar").is_ok());
    assert!(ui.find("17th").is_ok() && ui.find(" of 19, 250").is_ok());
    ui.click("Alt").unwrap();
    ui.click("All characters").unwrap();
    ui.click("The Coiled Altar").unwrap();
    let sent: Vec<crate::window::Message> = ui.into_messages().collect();
    assert!(
        matches!(
            sent.as_slice(),
            [
                crate::window::Message::HomeCharacter(Some(alt)),
                crate::window::Message::HomeCharacter(None),
                crate::window::Message::OpenStored(id),
            ] if alt == "Alt" && id == "c"
        ),
        "{sent:?}"
    );
    // Scoped: the title is theirs, and their empty week says so by name.
    home.scope = Some("Alt".to_string());
    let panels = derive(&cards, Some("Alt"), &Season::default(), &[], &[]);
    let mut ui = simulator(screen(
        &home,
        &panels,
        &chars(),
        theme::GOLD_ACCENT,
        true,
        0,
    ));
    assert!(ui.find("Alt, this week").is_ok());
    assert!(ui.find("No pulls on Alt this week.").is_ok());
    assert!(ui.find("No keys on Alt this week.").is_ok());
    let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();

    // A long night: a wipe's tile says how close it came, and the pulls
    // before the newest few are counted rather than listed.
    let mut cards = vec![sigma("v", "The Venomous Abyss", sun, 15)];
    for i in 0..(MAX_TILES + 2) as i64 {
        cards.push(boss(
            &format!("w{i}"),
            "Ula'tek",
            2,
            sun + (i + 1) * 10 * 60_000,
            false,
            raid_of(250.0),
        ));
    }
    if let Some(last) = cards.last_mut() {
        last.best_pct = Some(56);
    }
    let mut home = Home::new();
    samples::absorb(&mut home, cards.clone());
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    let mut ui = simulator(screen(
        &home,
        &panels,
        &chars(),
        theme::GOLD_ACCENT,
        true,
        0,
    ));
    assert!(ui.find("at 56%").is_ok(), "a wipe's best after its name");
    assert!(
        ui.find("2 pulls earlier that night, on the chart and the rail")
            .is_ok()
    );
}
/// The week's panels drawn: a key's result and time, a boss's outcome and
/// count, the best run's figure on the chart's legend — at a wide and a
/// narrow width alike.
#[test]
fn the_week_s_panels_render_at_every_width() {
    let fri = evening("2026-09-25");
    let cards = vec![
        key(
            "k1",
            "Kings' Rest +14",
            fri + 2 * H,
            1_900_000,
            Some(false),
            player("Me", Spec::Destruction, 150_000.0),
        ),
        key(
            "k2",
            "Murder Row +14",
            fri + H,
            1_300_000,
            Some(true),
            player("Me", Spec::Destruction, 292_600.0),
        ),
        FightCard {
            best_pct: Some(31),
            ..boss("b", "The Lost Explorers", 3, fri, false, raid_of(250.0))
        },
    ];
    let mut home = Home::new();
    samples::absorb(&mut home, cards.clone());
    let panels = derive(&cards, None, &Season::default(), &[], &[]);
    for w in [1440.0, 460.0] {
        let el = screen(&home, &panels, &chars(), theme::GOLD_ACCENT, true, 0);
        let mut ui = crate::window::testkit::simulator_as(
            crate::window::settings(),
            iced::Size::new(w, 1400.0),
            el,
        );
        for words in [
            "Keys this week",
            "over",
            " 31:40",
            "+2",
            " 21:40",
            "Best 31%",
            ", 1 pull",
            "Effective dps on keys",
            "personal best",
            "Ticks mark +3, +2 and the timer, left to right",
        ] {
            assert!(ui.find(words).is_ok(), "{words} at {w}");
        }
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
    }
}

/// The charts' rules: which ranks a crowded night keeps, hollow for what
/// went wrong, a rank that would print over the top guide's words hung
/// under its dot, and a night named under its first run alone.
#[test]
fn the_charts_mark_what_they_should() {
    use super::charts::{RankSlope, day_labels, hollow, rank_under};
    assert!(hollow(Mark::Bad), "a wipe, a key over time");
    assert!(!hollow(Mark::Good) && !hollow(Mark::Dash));
    let pull = |place: usize, mark| NightPull {
        fight_id: format!("p{place}"),
        name: "Boss".to_string(),
        mark,
        wipe_pct: None,
        standing: Standing {
            place,
            of: 19,
            value: 1.0,
        },
    };
    let slope = |pulls: &[NightPull]| RankSlope::new(pulls, iced::Color::WHITE);
    // A night of three: every rank is said.
    let few = [
        pull(17, Mark::Good),
        pull(10, Mark::Bad),
        pull(6, Mark::Good),
    ];
    assert_eq!(slope(&few).labelled(300.0), [true, true, true]);
    // A crowded night: the first, the lowest, the highest and the last.
    let crowd: Vec<NightPull> = [9, 12, 3, 15, 19, 8, 11, 14, 2, 10, 7, 13, 16, 5]
        .into_iter()
        .map(|p| pull(p, Mark::Bad))
        .collect();
    let kept: Vec<usize> = slope(&crowd)
        .labelled(300.0)
        .into_iter()
        .enumerate()
        .filter_map(|(i, k)| k.then_some(i))
        .collect();
    assert_eq!(kept, [0, 4, 8, 13]);
    // The night's best at its end tops the role under "top of the role":
    // its rank hangs under its dot. At the left end, or lower, above.
    let topped = slope(&[pull(17, Mark::Good), pull(1, Mark::Good)]).dots(300.0);
    assert!(!rank_under(topped[0].0, 300.0), "low: above its dot");
    assert!(rank_under(topped[1].0, 300.0), "under the words: under it");
    let early = slope(&[pull(1, Mark::Good), pull(17, Mark::Good)]).dots(300.0);
    assert!(!rank_under(early[0].0, 300.0), "clear of the words");
    // The day is said where it changes, and nowhere else.
    let run = |day| TrendPoint {
        fight_id: String::new(),
        who: Char::default(),
        day,
        value: 1.0,
        best: false,
    };
    assert_eq!(
        day_labels(&[run(1), run(1), run(2), run(2), run(1)]),
        [true, false, true, false, true]
    );
    assert!(day_labels(&[]).is_empty());
}
