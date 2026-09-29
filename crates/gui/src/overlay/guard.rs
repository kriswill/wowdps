//! The overlay's snapshot guard: the panel over the committed fixtures,
//! rendered headless as the running overlay draws it and hashed against
//! `crates/gui/snapshots/overlay/`. The window is being redesigned by
//! forking window-only paths off renderers the overlay shares; this is what
//! says the overlay did not move a pixel while that happened.
//!
//! Ignored by default: the hashes are of pixels, and pixels depend on the
//! machine — its fonts, and the per-machine icon caches under
//! `~/.local/share/wowdps/`. `WOWDPS_BLESS=1` rewrites them. Run it ALONE
//! (the filtered command in `crates/gui/SHOTS.md`): iced's font system is
//! process-global, so a window test that loads the window's own fonts
//! first would change the fallback the overlay's text is drawn with — the
//! guard refuses to run, or to bless, when it sees one.
//!
//! iced_test photographs at a scale factor of 2; the running overlay draws
//! at 1 and zooms by itself. The hashes prove the 2x pixels: a change that
//! only moves 1x rounding or pixel snapping can pass.

use std::collections::BTreeSet;
use std::os::unix::net::UnixStream;
use std::path::Path;

use iced_layershell::settings::LayerShellSettings;
use wowdps_daemon::mock::{MockDaemon, pump};
use wowdps_model::{Action, View};
use wowdps_proto::{ClientKind, ClientMsg, ClientState, Cursor, DaemonMsg, SegmentRef};

use super::{AUX_TOP_N, Overlay, aux_want, current_size, settings, theme, view};
use crate::config::Config;
use crate::window::testkit;

/// Where the blessed hashes live, one `<state>-<renderer>.sha256` each.
const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/snapshots/overlay");

/// The renderer iced_test names its files after. `testkit::simulator_as`
/// pins the backend (`force_tiny_skia`), so it is always this one.
const RENDERER: &str = "tiny-skia";

/// Fonts iced loads from its own bytes into the global font system (the
/// icon font and iced_test's Fira Sans). Any other in-memory face is one
/// some test loaded — and one the overlay, whose settings load none, never
/// draws with.
const ICED_OWN_FAMILIES: [&str; 2] = ["Iced-Icons", "Fira Sans"];

/// One photographed state: a fixture state over the mock daemon, as the
/// testkit builds them, and what the pointer, the grip or the footer did
/// to the panel (with the mock at hand, for what a second connection
/// would fetch).
struct Shot {
    name: &'static str,
    build: fn() -> (ClientState, MockDaemon),
    pose: fn(&mut Overlay, &mut MockDaemon),
}

/// The states the guard photographs: every renderer the overlay shares
/// with the window — the meter rows and the instance strip, a player's
/// drill with its graph, the Taken drill's mitigation line, the
/// comparison's tables and graphs, the Deaths recap rows, the ability
/// drill's breadcrumb / stat strip / target list, the enemy rows and their
/// attackers, a count view's rows, the hover mark on a meter row and a
/// drill row, the arena's team divider, a live pull's chrome, the footer's
/// ⚙ options card and view menu, the Σ split rows, a wheel-zoomed panel,
/// and the collapsed tab.
const STATES: [Shot; 18] = [
    Shot {
        name: "meter",
        build: testkit::kill,
        pose: as_built,
    },
    Shot {
        name: "drill",
        build: testkit::drilled,
        pose: as_built,
    },
    Shot {
        name: "taken-drill",
        build: taken_drilled,
        pose: as_built,
    },
    Shot {
        name: "compare",
        build: testkit::compared,
        pose: as_built,
    },
    Shot {
        name: "deaths-drill",
        build: deaths_drilled,
        pose: as_built,
    },
    Shot {
        name: "spell-drill",
        build: testkit::spell_drilled,
        pose: as_built,
    },
    Shot {
        name: "enemies",
        build: enemies,
        pose: as_built,
    },
    Shot {
        name: "enemies-drill",
        build: enemies_drilled,
        pose: as_built,
    },
    Shot {
        name: "interrupts",
        build: interrupts,
        pose: as_built,
    },
    Shot {
        name: "hover",
        build: testkit::kill,
        pose: hover_second_row,
    },
    Shot {
        name: "drill-hover",
        build: testkit::drilled,
        pose: hover_first_row,
    },
    Shot {
        name: "arena",
        build: arena,
        pose: as_built,
    },
    Shot {
        name: "live",
        build: live,
        pose: as_built,
    },
    Shot {
        name: "options",
        build: testkit::kill,
        pose: options,
    },
    Shot {
        name: "view-menu",
        build: testkit::kill,
        pose: view_menu,
    },
    Shot {
        name: "split",
        build: testkit::kill,
        pose: split,
    },
    Shot {
        name: "zoomed",
        build: testkit::kill,
        pose: zoomed,
    },
    Shot {
        name: "collapsed",
        build: testkit::kill,
        pose: collapse,
    },
];

fn as_built(_: &mut Overlay, _: &mut MockDaemon) {}

/// The pointer on the kill's second row — off the selection, so the mark
/// is the hover's own.
fn hover_second_row(ov: &mut Overlay, _: &mut MockDaemon) {
    ov.row_hover = Some(1);
}

fn hover_first_row(ov: &mut Overlay, _: &mut MockDaemon) {
    ov.row_hover = Some(0);
}

/// The footer's ⚙ clicked: the options card over the panel.
fn options(ov: &mut Overlay, _: &mut MockDaemon) {
    ov.options_open = true;
}

/// The footer's view name right-clicked, the pointer on a row of the menu
/// that is not the current view.
fn view_menu(ov: &mut Overlay, _: &mut MockDaemon) {
    ov.view_menu = true;
    ov.view_hover = Some(View::Deaths);
}

/// The footer's Σ toggle on: the visit's overall rows under the kill's,
/// as the second connection fetches them — the Watch `sync_aux` sends,
/// answered by the mock.
fn split(ov: &mut Overlay, mock: &mut MockDaemon) {
    ov.split = true;
    let (id, view) = aux_want(ov).expect("the kill sits inside a visit with a Σ");
    let pushes = mock.handle(ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Id(id),
        view,
        top_n: Some(AUX_TOP_N),
        drill: None,
        death: None,
        spell: None,
        range: None,
    }));
    let (info, rows) = pushes
        .into_iter()
        .find_map(|msg| match msg {
            DaemonMsg::Snapshot {
                segment: SegmentRef::Id(sid),
                info,
                rows,
                ..
            } if sid == id => Some((info, rows)),
            _ => None,
        })
        .expect("the mock answers the Σ's watch");
    assert!(!rows.is_empty(), "the visit's Σ has rows");
    ov.aux_watch = Some((id, view));
    ov.aux_info = Some(info);
    ov.aux_rows = rows;
}

/// Four wheel notches up from the default zoom, the panel grown with it —
/// what `Message::Zoom(4.0)` leaves, set directly (the handler saves the
/// config). An off-grid zoom, so every size the panel derives from it is
/// fractional.
fn zoomed(ov: &mut Overlay, _: &mut MockDaemon) {
    let old = ov.cfg.zoom;
    let new = old + 0.05 * 4.0;
    let ratio = new / old;
    ov.cfg.zoom = new;
    ov.cfg.width = (ov.cfg.width as f32 * ratio).round() as u32;
    ov.cfg.height = (ov.cfg.height as f32 * ratio).round() as u32;
}

/// The grip clicked shut: the tab, at the size the overlay asks for.
fn collapse(ov: &mut Overlay, _: &mut MockDaemon) {
    ov.expanded = false;
}

/// R17's fixture kill, its tank drilled.
fn taken_drilled() -> (ClientState, MockDaemon) {
    let (mut state, mut mock) = testkit::taken_kill();
    testkit::apply(&mut state, &mut mock, Action::Open);
    assert!(state.drill_mitigation().is_some());
    (state, mock)
}

/// R9: the kill's one death, its recap open.
fn deaths_drilled() -> (ClientState, MockDaemon) {
    let (mut state, mut mock) = testkit::kill();
    testkit::apply(&mut state, &mut mock, Action::SetView(View::Deaths));
    testkit::apply(&mut state, &mut mock, Action::Open);
    assert!(state.drill.is_some());
    assert!(!state.breakdown().0.is_empty(), "the recap has lines");
    (state, mock)
}

/// R24: the kill's Enemy Taken meter.
fn enemies() -> (ClientState, MockDaemon) {
    let (mut state, mut mock) = testkit::kill();
    testkit::apply(&mut state, &mut mock, Action::SetView(View::EnemyTaken));
    assert!(!state.rows().is_empty());
    (state, mock)
}

/// R24: the top enemy drilled — its attackers.
fn enemies_drilled() -> (ClientState, MockDaemon) {
    let (mut state, mut mock) = enemies();
    testkit::apply(&mut state, &mut mock, Action::Open);
    assert!(!state.breakdown().1.is_empty(), "the enemy has attackers");
    (state, mock)
}

/// A count view: the kill's interrupts, numbered rather than summed.
fn interrupts() -> (ClientState, MockDaemon) {
    let (mut state, mut mock) = testkit::kill();
    testkit::apply(&mut state, &mut mock, Action::SetView(View::Interrupts));
    assert!(!state.rows().is_empty(), "the kill has interrupts");
    (state, mock)
}

/// Mid-pull: the fixture's last fight still open, the panel following it —
/// the live chrome (the Σ badge LIVE, no "live" jump in the footer).
fn live() -> (ClientState, MockDaemon) {
    let (state, mock) = testkit::live();
    assert!(state.following_live() && state.is_live(), "a live pull");
    (state, mock)
}

/// R13: the first arena match (newest first) whose meter splits the
/// teams, so the divider is drawn.
fn arena() -> (ClientState, MockDaemon) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/arena.txt");
    let mut mock = MockDaemon::fixture_at(Path::new(path));
    let mut state = ClientState::new();
    let first = state.initial_request();
    pump(&mut state, &mut mock, vec![first]);
    testkit::apply(&mut state, &mut mock, Action::Open);
    for _ in 0..state.list_rows().len() {
        if crate::view::enemy_split(&state.rows()).is_some() {
            return (state, mock);
        }
        testkit::apply(&mut state, &mut mock, Action::OlderSegment);
    }
    panic!("no match in arena.txt splits the teams");
}

/// The panel over `state`, expanded, under the shipping config.
fn expanded(state: ClientState) -> (Overlay, UnixStream) {
    let (client, peer) = testkit::fake_client_as(ClientKind::Overlay);
    let cfg = Config {
        follow_game: false,
        ..Config::default()
    };
    let mut ov = Overlay::for_test(state, client, cfg);
    ov.expanded = true;
    (ov, peer)
}

/// The iced settings the simulator takes, from the overlay's own
/// layer-shell settings (`overlay::settings`): the fonts it loads, its
/// default font and text size — all the simulator reads.
fn overlay_settings() -> iced::Settings {
    let own = settings(LayerShellSettings::default());
    iced::Settings {
        fonts: own.fonts,
        default_font: own.default_font,
        default_text_size: own.default_text_size,
        antialiasing: own.antialiasing,
        ..iced::Settings::default()
    }
}

/// The file iced_test reads `state`'s hash from.
fn hash_file(state: &str) -> String {
    format!("{state}-{RENDERER}.sha256")
}

/// Every hash file in `DIR`.
fn hash_files() -> BTreeSet<String> {
    std::fs::read_dir(DIR)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".sha256"))
        .collect()
}

/// The families of every in-memory face in iced's global font system that
/// iced did not load itself: fonts some test loaded that the overlay's own
/// settings never would, and that cosmic-text's fallback may now pick for
/// a glyph the system fonts lack (the overlay draws ⚙ Σ ☠ ●).
fn foreign_fonts() -> BTreeSet<String> {
    use iced_tiny_skia::graphics::text::cosmic_text::fontdb::Source;
    assert!(
        overlay_settings().fonts.is_empty(),
        "the overlay loads fonts of its own now: count their families \
         as expected here, beside ICED_OWN_FAMILIES"
    );
    let mut system = iced_tiny_skia::graphics::text::font_system()
        .write()
        .expect("the font system lock");
    let loaded: BTreeSet<String> = system
        .raw()
        .db()
        .faces()
        .filter(|f| matches!(f.source, Source::Binary(_)))
        .flat_map(|f| f.families.iter().map(|(name, _)| name.clone()))
        .collect();
    // iced loads Fira Sans with the font system itself (iced_test's
    // "fira-sans"): finding it proves this is the database the snapshots
    // are drawn from.
    assert!(
        loaded.contains("Fira Sans"),
        "iced's own fonts are not where the guard looks: {loaded:?}"
    );
    loaded
        .into_iter()
        .filter(|name| !ICED_OWN_FAMILIES.contains(&name.as_str()))
        .collect()
}

/// Fail, naming them, when fonts the overlay never loads are in memory.
fn assert_no_foreign_fonts(when: &str) {
    let foreign = foreign_fonts();
    assert!(
        foreign.is_empty(),
        "{when}, fonts the overlay never loads are in iced's global font \
         system: {foreign:?}. A window test loaded them in this process; \
         run the guard alone: cargo test -p wowdps-gui overlay_snapshot_guard -- --ignored"
    );
}

#[test]
#[ignore = "pixels differ across machines' fonts and icon caches (crates/gui/SHOTS.md)"]
fn overlay_snapshot_guard() {
    let bless = std::env::var_os("WOWDPS_BLESS").is_some_and(|v| v == "1");
    // Before anything is drawn, deleted or written: a bless in a process
    // whose font system another test already filled would rewrite every
    // hash from the wrong glyphs.
    assert_no_foreign_fonts("before the guard ran");
    let expected: BTreeSet<String> = STATES.iter().map(|s| hash_file(s.name)).collect();
    let before = hash_files();
    if bless {
        // Stale states' files too: the directory is exactly STATES after.
        for old in &before {
            std::fs::remove_file(Path::new(DIR).join(old)).unwrap();
        }
    } else {
        // `matches_hash` writes a missing file and calls it a match: every
        // hash must be there BEFORE it is asked, or the guard passes
        // vacuously (a renamed renderer, a new state never blessed).
        let missing: Vec<&String> = expected.difference(&before).collect();
        assert!(
            missing.is_empty(),
            "no blessed hash {missing:?} in {DIR}; bless with WOWDPS_BLESS=1 \
             (only when the overlay's pixels are known to be right)"
        );
    }
    // Optional pictures of what is hashed, to look at a state that moved
    // (or a new one before it is blessed).
    let pictures = std::env::var_os("WOWDPS_GUARD_PNG").map(std::path::PathBuf::from);
    let mut moved = Vec::new();
    for shot in STATES {
        let (app, mut mock) = (shot.build)();
        let (mut ov, _peer) = expanded(app);
        (shot.pose)(&mut ov, &mut mock);
        // The surface the overlay would ask the compositor for.
        let (w, h) = current_size(&ov);
        let size = iced::Size::new(w as f32, h as f32);
        let mut ui = testkit::simulator_as(overlay_settings(), size, view(&ov));
        let snap = ui.snapshot(&theme(&ov)).unwrap();
        if let Some(dir) = &pictures {
            // `matches_image` saves only where no picture is yet.
            let _ = std::fs::remove_file(dir.join(format!("{}-{RENDERER}.png", shot.name)));
            assert!(snap.matches_image(dir.join(shot.name)).unwrap());
        }
        if !snap.matches_hash(Path::new(DIR).join(shot.name)).unwrap() {
            moved.push(shot.name);
        }
    }
    let after = hash_files();
    assert_no_foreign_fonts("after the guard ran");
    assert!(
        moved.is_empty(),
        "the overlay's pixels moved in {moved:?}. If that is intended, \
         re-bless: WOWDPS_BLESS=1 cargo test -p wowdps-gui overlay_snapshot_guard -- --ignored"
    );
    if bless {
        assert_eq!(
            after, expected,
            "the blessed files are not named `<state>-{RENDERER}.sha256`"
        );
    } else {
        assert_eq!(after, before, "the guard wrote or removed a hash file");
    }
}
