//! Design shots: the window's screens rendered to PNG through the code the
//! running window draws with — `view::view`, `window::settings`, the
//! window's own theme — for review against the redesign's prototype
//! (`docs/design/window-redesign.html`). Nothing is compared: a shot is a
//! picture for a reader to look at, and `crates/gui/SHOTS.md` has the
//! commands.
//!
//! Every state is reached the way a user reaches it — a fresh window over
//! the mock daemon, driven by the messages its clicks and keys send — and
//! photographed at the prototype's three sizes. The log is parsed once:
//! each fresh window hands the mock, and the segments its engine already
//! parsed, on to the next.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use iced::Size;
use iced::keyboard::key::Named;
use wowdps_daemon::mock::{MockDaemon, pump};
use wowdps_model::{Action, ListRow, Screen, SegmentKind, View};
use wowdps_proto::ClientState;
use wowdps_proto::history::FightCard;

use super::testkit::{Bridge, chr, isolate_config, named, simulator_as};
use super::{Gui, Message, settings, style, theme, update};
use crate::config::Config;
use crate::rail::{Line, Mark, Pull};
use crate::view;

/// The prototype's three layouts, in logical pixels at zoom 1. iced_test
/// photographs at a scale of 2, so a wide PNG is 2880×1800.
const SIZES: [(&str, Size); 3] = [
    ("wide", Size::new(1440.0, 900.0)),
    ("tile", Size::new(960.0, 880.0)),
    ("narrow", Size::new(460.0, 860.0)),
];

/// The scale the prototype's reference renders were captured at
/// (`design-shots/reference/*.png`): a wide reference is 1800×1125.
const REFERENCE_SCALE: f32 = 1.25;

/// The scale iced_test photographs at (`Snapshot` is always 2x): a PNG is
/// this many pixels per logical one.
const SHOT_SCALE: f32 = 2.0;

/// How a fresh window reaches one state. `Err` says why this log cannot
/// show it, and the state is skipped.
type Reach = fn(&mut Bridge, &Scene) -> Result<(), String>;

/// Every state, by the file stem it is saved under.
const STATES: [(&str, Reach); 24] = [
    ("damage", damage),
    ("healing", healing),
    ("taken", taken),
    ("deaths", deaths),
    ("enemies", enemies),
    ("drill", drill),
    ("taken-drill", taken_drill),
    ("deaths-drill", deaths_drill),
    ("enemies-drill", enemies_drill),
    ("compare", compare),
    ("home", home),
    // A pull of an earlier night, opened from the history store: the same
    // renderers, fed from a stored card.
    ("stored", stored),
    // A stored raid wipe and a stored key on the stage: the header's
    // verdicts a card words (its best health, its timer).
    ("stored-wipe", stored_wipe),
    ("stored-key", stored_key),
    // The pull rail as a drawer (tile and narrow) over the fight; beside
    // it at the wide frame, as every wide shot has it.
    ("rail-open", rail_open),
    // The rail with its trash hidden.
    ("hide-trash", hide_trash),
    // A pull of an earlier night on the stage and the rail open over it,
    // scrolled to its row as the drawer opens there.
    ("rail-earlier", rail_earlier),
    // The window's own surfaces over the meter, and its looks.
    ("spell-drill", spell_drill),
    ("options", options),
    ("keys", keys),
    ("picker", picker),
    ("filter", filter),
    ("damage-class", damage_class),
    ("talents", talents),
];

/// The states photographed with the row filter FOCUSED: the harness clicks
/// the field in the picture's own simulator, the way a user focuses it.
const FOCUSED: [&str; 1] = ["filter"];

/// The states photographed with the rail scrolled to the pull on the stage,
/// as the running window scrolls it when the drawer opens (`OpenRail`
/// asks for it): the picture's own simulator is a fresh widget tree, so
/// the harness wheels the rail there itself.
const REVEALED: [&str; 1] = ["rail-earlier"];

/// The featured fight and its owner, resolved from the log once.
struct Scene {
    /// The fight's position in the segment list (oldest first) — what a
    /// click on its row sends — and its name.
    fight: Option<(usize, String)>,
    /// The owner's row label ("Name-Realm") and guid, when they fought in
    /// it.
    owner: Option<(String, String)>,
    /// Opening the fight pinned Live: it is the log's newest segment, so
    /// every fight shot wears the live chrome (the window's own rule —
    /// a click on the newest row follows it).
    live: bool,
    /// The night the log's newest segment began on: what every window
    /// calls "Tonight", pinned, so a shot never depends on the wall clock
    /// (the day after the log was cut, the clock's tonight is another).
    tonight: Option<i64>,
}

#[test]
#[ignore = "design review: writes PNGs to $WOWDPS_SHOTS_DIR (crates/gui/SHOTS.md)"]
fn design_shots() {
    let Some(dir) = std::env::var_os("WOWDPS_SHOTS_DIR").map(PathBuf::from) else {
        eprintln!("design_shots: set WOWDPS_SHOTS_DIR to a directory to write the shots into");
        return;
    };
    isolate_config();
    let log = std::env::var_os("WOWDPS_SHOTS_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(wowdps_daemon::mock::FIXTURE));
    let owner = std::env::var("WOWDPS_SHOTS_OWNER").unwrap_or_else(|_| "Tranqlock".to_string());
    let fight = std::env::var("WOWDPS_SHOTS_FIGHT").ok();
    let store = std::env::var_os("WOWDPS_SHOTS_HISTORY").map(PathBuf::from);
    std::fs::create_dir_all(&dir).unwrap();
    if let Err(why) = check_ours(&dir) {
        panic!(
            "design_shots: refusing to write into {}: {why}",
            dir.display()
        );
    }
    clear_previous(&dir);

    let parse = Instant::now();
    let mut mock = MockDaemon::fixture_at(&log);
    let scene = resolve(&mut mock, fight.as_deref(), &owner);
    // The daemon reads `history_characters` from the same config file the
    // window does, so the store stamps the owner on their cards too.
    let names: Vec<String> = scene.owner.iter().map(|(label, _)| label.clone()).collect();
    let mut mock = mock.with_characters(&names);
    // A real store, read-only (`MemBackend::over_dir`): the log's own
    // cards alone are one night, and Home and History are about weeks.
    if let Some(store) = &store {
        assert!(
            store.join("fights").is_dir(),
            "WOWDPS_SHOTS_HISTORY={} holds no fights/ (point it at a store's v1 directory)",
            store.display()
        );
        mock = mock.with_store_dir(store);
    }
    let mut mock = mock.with_history();
    let cards = mock.history().cards().len();
    let newest = newest_card(mock.history().cards());
    let parse = parse.elapsed();
    match &scene.fight {
        Some((_, name)) => eprintln!("design_shots: fight {name:?}"),
        None => eprintln!("design_shots: no boss fight in the log; fight states skipped"),
    }
    match &scene.owner {
        Some((label, _)) => eprintln!("design_shots: owner {label}"),
        None => eprintln!("design_shots: {owner:?} is not in the fight; no owner"),
    }

    let cfg = shot_config(scene.owner.as_ref());
    let (mut drive, mut render) = (Duration::ZERO, Duration::ZERO);
    let mut written = Vec::new();
    let mut skipped = Vec::new();
    let mut troubles: Vec<String> = Vec::new();
    for (state, reach) in STATES {
        let at = Instant::now();
        let mut b = launch(mock, cfg.clone(), scene.tonight);
        let reached = reach(&mut b, &scene);
        drive += at.elapsed();
        match reached {
            Ok(()) => {
                let at = Instant::now();
                let focus = FOCUSED.contains(&state);
                let reveal = REVEALED.contains(&state);
                for (size, px) in SIZES {
                    let name = format!("{size}-{state}");
                    let (path, trouble) = shoot(&b.gui, px, &dir, &name, focus, reveal);
                    if let Some(why) = trouble {
                        eprintln!("design_shots: {name}: {why}");
                        troubles.push(format!("{name}: {why}"));
                    }
                    written.push((path, px));
                }
                render += at.elapsed();
            }
            Err(why) => {
                eprintln!("design_shots: skipping {state}: {why}");
                skipped.push(format!("{state}: {why}"));
            }
        }
        mock = b.mock;
    }

    let mut manifest = String::new();
    let _ = writeln!(manifest, "rev: {}", revision());
    let _ = writeln!(manifest, "src: {}", source_fingerprint());
    for cache in ["class-icons.bin", "spell-icons.bin"] {
        let _ = writeln!(manifest, "cache: {}", cache_stamp(cache));
    }
    let log = std::fs::canonicalize(&log).unwrap_or(log);
    let _ = writeln!(manifest, "log: {}", log.display());
    let history = match &store {
        Some(s) => format!(
            "{} (read-only; {}) + the log's own",
            s.display(),
            store_fingerprint(s)
        ),
        None => "the log's own".to_string(),
    };
    let _ = writeln!(manifest, "history: {history}, {cards} cards");
    let _ = writeln!(manifest, "newest card: {newest}");
    let _ = writeln!(
        manifest,
        "tonight: {} (the log's newest night, pinned: the rail's headings never read the wall clock)",
        scene.tonight.map_or_else(
            || "-".to_string(),
            |night| {
                let (y, m, d) = crate::rail::civil(night);
                format!("{y:04}-{m:02}-{d:02}")
            }
        )
    );
    let fight_name = scene.fight.as_ref().map_or("-", |(_, n)| n.as_str());
    let _ = writeln!(manifest, "fight: {fight_name}");
    let _ = writeln!(
        manifest,
        "following live: {} (the fight {} the log's newest segment)",
        if scene.live { "yes" } else { "no" },
        if scene.live { "is" } else { "is not" }
    );
    let owner_label = scene.owner.as_ref().map_or("-", |(l, _)| l.as_str());
    let _ = writeln!(manifest, "owner: {owner_label}");
    let _ = writeln!(
        manifest,
        "display: hide_realms {}, show_ranks {}, density {}, zoom {}, home_on_start {}",
        cfg.hide_realms, cfg.show_ranks, cfg.density, cfg.zoom, cfg.home_on_start
    );
    let _ = writeln!(manifest, "parse: {:.1} s", parse.as_secs_f64());
    let _ = writeln!(manifest, "drive: {:.1} s", drive.as_secs_f64());
    let _ = writeln!(
        manifest,
        "render: {:.1} s ({} shots)",
        render.as_secs_f64(),
        written.len()
    );
    let _ = writeln!(
        manifest,
        "scale: {SHOT_SCALE} (a PNG is twice its logical size; the references are \
         {REFERENCE_SCALE}x: compare harness px / {SHOT_SCALE} with reference px / {REFERENCE_SCALE})"
    );
    for (path, px) in &written {
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        let _ = writeln!(
            manifest,
            "{file}  {}x{} logical, {}x{} px",
            px.width,
            px.height,
            px.width * SHOT_SCALE,
            px.height * SHOT_SCALE
        );
    }
    for why in &skipped {
        let _ = writeln!(manifest, "skipped {why}");
    }
    // A picture that is not what its name says — a FOCUSED state whose
    // field could not be focused — is named here, never silently kept.
    for why in &troubles {
        let _ = writeln!(manifest, "trouble {why}");
    }
    std::fs::write(dir.join("manifest.txt"), &manifest).unwrap();
    eprint!("{manifest}");
    assert!(!written.is_empty(), "every state was skipped");
}

/// Whether `dir` is the harness's to write into: a directory holding
/// pictures under the harness's names must hold the harness's
/// `manifest.txt` (first line `rev:`) too. The prototype's references
/// (`design-shots/reference/`) share several names — `wide-damage.png`,
/// `wide-home.png` — and a run pointed there by mistake would delete them.
fn check_ours(dir: &Path) -> Result<(), String> {
    let manifest = std::fs::read_to_string(dir.join("manifest.txt")).unwrap_or_default();
    if manifest.starts_with("rev:") {
        return Ok(());
    }
    let clash: Vec<String> = our_names().filter(|f| dir.join(f).exists()).collect();
    if clash.is_empty() {
        return Ok(());
    }
    Err(format!(
        "it holds {} under the harness's names ({}{}) but no manifest.txt the \
         harness wrote; pick another directory, or empty this one first",
        if clash.len() == 1 {
            "a picture".to_string()
        } else {
            format!("{} pictures", clash.len())
        },
        clash[0],
        if clash.len() > 1 { ", …" } else { "" }
    ))
}

/// Every file name a run may write.
fn our_names() -> impl Iterator<Item = String> {
    STATES.iter().flat_map(|(state, _)| {
        SIZES
            .iter()
            .map(move |(size, _)| format!("{size}-{state}.png"))
    })
}

/// Clear what an earlier run left in `dir`, so no picture is mistaken for
/// this run's: the scratch directory a panicked run left behind, every
/// PNG the previous `manifest.txt` lists (states since renamed or removed
/// included), and every name this run may write. Nothing else is touched —
/// the directory may hold pictures the harness did not make.
fn clear_previous(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.join(".render"));
    let listed = std::fs::read_to_string(dir.join("manifest.txt")).unwrap_or_default();
    let previous = listed
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|f| f.ends_with(".png") && !f.contains('/'))
        .map(str::to_string);
    for file in previous.chain(our_names()) {
        let _ = std::fs::remove_file(dir.join(file));
    }
}

/// The checkout's commit, read from `.git` without running git: HEAD, then
/// the branch's ref file or `packed-refs`. A worktree's `.git` file is
/// followed to its gitdir, whose refs live in the common directory.
fn revision() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut git = root.join(".git");
    if let Ok(link) = std::fs::read_to_string(&git)
        && let Some(to) = link.trim().strip_prefix("gitdir:")
    {
        git = root.join(to.trim());
    }
    let common = std::fs::read_to_string(git.join("commondir"))
        .map_or_else(|_| git.clone(), |c| git.join(c.trim()));
    let Ok(head) = std::fs::read_to_string(git.join("HEAD")) else {
        return "unknown (no .git)".to_string();
    };
    let head = head.trim();
    let Some(name) = head.strip_prefix("ref:").map(str::trim) else {
        return format!("{head} (detached)");
    };
    let loose = std::fs::read_to_string(common.join(name)).ok();
    let packed = || {
        let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
        packed.lines().find_map(|l| {
            let (sha, r) = l.split_once(' ')?;
            (r == name).then(|| sha.to_string())
        })
    };
    match loose.map(|s| s.trim().to_string()).or_else(packed) {
        Some(sha) => format!("{sha} ({name})"),
        None => format!("unknown ({name})"),
    }
}

/// A fingerprint of the source a shot is drawn from — what `rev` cannot
/// say about a dirty tree: FNV-64 over every file's path and bytes under
/// the `src/` of the crates the window and the mock are built from.
fn source_fingerprint() -> String {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let names = ["gui", "proto", "model", "daemon", "core"];
    let mut files = Vec::new();
    for name in names {
        walk(&crates.join(name).join("src"), &mut files);
    }
    files.sort();
    let mut all = Vec::new();
    for f in &files {
        all.extend_from_slice(
            f.strip_prefix(&crates)
                .unwrap_or(f)
                .as_os_str()
                .as_encoded_bytes(),
        );
        all.push(0);
        all.extend_from_slice(&std::fs::read(f).unwrap_or_default());
        all.push(0);
    }
    format!(
        "fnv64 {:016x} over {} files in crates/{{{}}}/src",
        wowdps_proto::history::fnv64(&all),
        files.len(),
        names.join(",")
    )
}

/// A per-machine icon cache the shots draw from, by size and mtime — a
/// regenerated cache changes pictures with no change to the code.
fn cache_stamp(file: &str) -> String {
    let Some(path) = wowdps_proto::talents::data_path(file) else {
        return format!("{file} (no data dir)");
    };
    match std::fs::metadata(&path) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            format!("{} {} bytes, mtime {mtime}", path.display(), m.len())
        }
        Err(_) => format!("{} absent (drawn without)", path.display()),
    }
}

/// What tells two stores apart when their card counts agree: FNV-64 over
/// the sorted names and sizes of the cards in `fights/`.
fn store_fingerprint(dir: &Path) -> String {
    let mut files: Vec<(String, u64)> = std::fs::read_dir(dir.join("fights"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let len = e.metadata().ok()?.len();
            name.ends_with(".json").then_some((name, len))
        })
        .collect();
    files.sort();
    let mut all = Vec::new();
    for (name, len) in &files {
        all.extend_from_slice(name.as_bytes());
        all.push(0);
        all.extend_from_slice(&len.to_le_bytes());
    }
    format!(
        "fnv64 {:016x} over {} fights/ files",
        wowdps_proto::history::fnv64(&all),
        files.len()
    )
}

/// The newest card Home and History answer from — what their shots are
/// "as of" — by its start on the log's own clock.
fn newest_card(cards: &[FightCard]) -> String {
    cards.iter().max_by_key(|c| c.start_utc_ms).map_or_else(
        || "none".to_string(),
        |c| {
            format!(
                "{} at {} log-local (start_utc_ms {})",
                c.name,
                wall_clock(c.start_local_ms),
                c.start_utc_ms
            )
        },
    )
}

/// Milliseconds since the epoch as `YYYY-MM-DD HH:MM:SS` — Howard
/// Hinnant's civil-from-days, the inverse of `home::parse_ymd`'s days.
fn wall_clock(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

/// Find the fight and the owner by driving a bare `ClientState` over the
/// mock — which also parses the fight, so every window after this one
/// opens it from the engine's cache.
fn resolve(mock: &mut MockDaemon, fight: Option<&str>, owner: &str) -> Scene {
    let mut state = ClientState::new();
    let first = state.initial_request();
    pump(&mut state, mock, vec![first]);
    let fight = pick_fight(&state.list_rows(), fight);
    let owner = fight.as_ref().and_then(|(pos, _)| {
        state.set_list_selection(*pos);
        let reqs = state.apply(Action::Open);
        pump(&mut state, mock, reqs);
        state
            .rows()
            .into_iter()
            .find(|r| is_owner(&r.label, owner))
            .map(|r| (r.label, r.key))
    });
    let live = fight.is_some() && state.following_live();
    let tonight = state
        .list_rows()
        .iter()
        .map(|r| r.start_ms)
        .max()
        .map(crate::rail::night_of);
    Scene {
        fight,
        owner,
        live,
        tonight,
    }
}

/// The featured fight: the named one — its first kill, else its first
/// pull — or, unnamed, the log's first boss kill (else first boss pull).
fn pick_fight(rows: &[ListRow], name: Option<&str>) -> Option<(usize, String)> {
    let wanted = |r: &ListRow| match name {
        Some(n) => r.name.eq_ignore_ascii_case(n),
        None => r.kind == SegmentKind::Encounter && !r.arena,
    };
    let pos = rows
        .iter()
        .position(|r| wanted(r) && r.success == Some(true))
        .or_else(|| rows.iter().position(wanted))?;
    Some((pos, rows[pos].name.clone()))
}

/// A row label names the owner: "Name-Realm" whole, or a bare "Name" that
/// matches its name half — how the daemon reads `history_characters`.
fn is_owner(label: &str, owner: &str) -> bool {
    let (label, owner) = (label.to_lowercase(), owner.to_lowercase());
    label == owner
        || label
            .strip_prefix(&owner)
            .is_some_and(|r| r.starts_with('-'))
}

/// The window's config for the shots: the shipping defaults — Home at
/// launch included (`launch`) — at zoom 1 (a shot is in logical pixels),
/// with the display keys the prototype's "Look" assumes and the user's
/// config sets: realms hidden in every pane, ranks shown, comfortable
/// density. The owner is named as the user's config names them: as one of
/// `history_characters`, and as the locked `character`.
fn shot_config(owner: Option<&(String, String)>) -> Config {
    let mut cfg = Config {
        zoom: 1.0,
        hide_realms: true,
        show_ranks: true,
        density: "comfortable".to_string(),
        ..Config::default()
    };
    if let Some((label, guid)) = owner {
        cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::Array(vec![toml::Value::String(label.clone())]),
        );
        cfg.character = Some(guid.clone());
    }
    cfg
}

/// A fresh window over the mock, arrived the way a user's does. The
/// shipping config offers Home at launch (a live pull then replaces it);
/// a window that never saw it there, and whose rail named no one, visits
/// Home (`~`) and comes back. Either way what Home tells a window — the
/// owner, for the top bar's character picker — is there on every screen,
/// as it is for any window that has been to Home once. Its tonight is
/// pinned to `tonight` (the log's newest night, [`Scene::tonight`]).
fn launch(mock: MockDaemon, cfg: Config, tonight: Option<i64>) -> Bridge {
    let mut b = Bridge::with_config(mock, cfg);
    b.gui.tonight = tonight;
    if b.gui.home.is_none() && b.gui.known_characters.is_empty() {
        b.send(chr("~"));
        b.send(chr("~"));
    }
    b
}

/// Photograph the window at `px` into `dir/<name>.png`, and say what went
/// wrong with the picture if something did: a FOCUSED state whose field the
/// click could not focus is a picture of the unfocused field.
///
/// The page is painted the way the running app paints it — through the
/// window's `style` (its background and default text color), not the
/// theme's base the snapshot would otherwise clear to — but opaque: the
/// live window shows that background at `window_alpha` over the desktop.
fn shoot(
    gui: &Gui,
    px: Size,
    dir: &Path,
    name: &str,
    focus_filter: bool,
    reveal_rail: bool,
) -> (PathBuf, Option<String>) {
    let th = theme(gui);
    let app = style(gui, &th);
    let background = iced::Color {
        a: 1.0,
        ..app.background_color
    };
    let text_color = app.text_color;
    let page = iced::widget::container(view::view(gui))
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .style(move |_| iced::widget::container::Style {
            background: Some(background.into()),
            text_color: Some(text_color),
            ..Default::default()
        });
    let mut ui = simulator_as(settings(), px, page.into());
    let mut trouble = None;
    if focus_filter && let Err(e) = ui.click(crate::nav::filter_id()) {
        trouble = Some(format!("the filter could not be focused: {e}"));
    }
    if reveal_rail && let Err(why) = reveal_current_row(&mut ui) {
        trouble = Some(why);
    }
    let snap = ui.snapshot(&th).unwrap();
    // `matches_image` only saves when nothing is there, and names the file
    // after the renderer ("<name>-tiny-skia.png"): let it write into a
    // scratch directory of its own, then move the one file it made.
    let scratch = dir.join(".render");
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(snap.matches_image(scratch.join(name)).unwrap());
    let made = std::fs::read_dir(&scratch)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let out = dir.join(format!("{name}.png"));
    std::fs::rename(made, &out).unwrap();
    let _ = std::fs::remove_dir(&scratch);
    (out, trouble)
}

/// Stand the rail on the pull on the stage as the running window's drawer
/// opens on it (`rail::Reveal::open`: in sight with room under it, else its
/// night's heading at the top, else the row centred) — with the wheel, over
/// the rail, in the picture's own simulator (a fresh widget tree runs no
/// operation the window asked for; the wheel stops at the list's end).
fn reveal_current_row(ui: &mut iced_test::Simulator<'_, Message>) -> Result<(), String> {
    let list = ui
        .find(crate::rail::scroll_id())
        .map_err(|e| format!("the rail is not drawn: {e}"))?
        .bounds();
    let row = ui
        .find(crate::rail::current_id())
        .map_err(|e| format!("the pull on the stage has no row on the rail: {e}"))?
        .bounds();
    let heading = ui
        .find(crate::rail::current_night_id())
        .ok()
        .map(|h| h.bounds().y - list.y);
    let to = crate::rail::open_offset(
        0.0,
        list.height,
        f32::INFINITY,
        (row.y - list.y, row.y + row.height - list.y),
        heading,
    );
    if to > 0.0 {
        ui.point_at(list.center());
        let _ = ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -to },
        })]);
    }
    Ok(())
}

// ---- the states --------------------------------------------------------------

/// The meter on the fight, the owner's row selected.
fn damage(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::Damage)
}

fn healing(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::Healing)
}

fn taken(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::Taken)
}

fn deaths(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::Deaths)
}

fn enemies(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::EnemyTaken)
}

/// The owner's row selected and the keys handed to the inspector beside
/// it (Enter) — the top row's when the owner was not there. A narrow
/// window shows the inspector pushed over the meter.
fn drill(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    open_fight(b, scene)?;
    let at = owner_row(b, scene).unwrap_or(0);
    b.send(Message::MeterRow(at));
    drill_opened(b)
}

/// The Taken view's top row inspected: normally a tank, with the
/// mitigation line over their graph.
fn taken_drill(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::Taken)?;
    b.send(Message::MeterRow(0));
    drill_opened(b)
}

/// The owner's death recap: the Deaths view's inspector on their row — the
/// top row's when the owner did not die.
fn deaths_drill(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::Deaths)?;
    let at = owner_row(b, scene).unwrap_or(0);
    b.send(Message::MeterRow(at));
    drill_opened(b)
}

/// The top enemy inspected: who hit it.
fn enemies_drill(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    in_view(b, scene, View::EnemyTaken)?;
    b.send(Message::MeterRow(0));
    drill_opened(b)
}

/// The owner pinned (`v`) and the top damage row selected — the second
/// row when the owner IS the top one, the top two without an owner — so
/// the inspector overlays the two; Enter hands it the keys, which a
/// narrow window shows by pushing it over the meter.
fn compare(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    open_fight(b, scene)?;
    if b.gui.fight().rows().len() < 2 {
        return Err("fewer than two players".to_string());
    }
    let mine = owner_row(b, scene).unwrap_or(1);
    let top = usize::from(mine == 0);
    b.send(Message::MeterRow(mine));
    b.send(chr("v"));
    b.send(Message::MeterRow(top));
    b.send(named(Named::Enter));
    if b.gui.state.screen == Screen::Compare && b.gui.state.compare_sides().is_some() {
        Ok(())
    } else {
        Err("the comparison did not open".to_string())
    }
}

/// Home: up since launch when nothing was live, else opened by its place
/// on the top bar. An empty store is a Home the window really shows, so it
/// is photographed rather than skipped.
fn home(b: &mut Bridge, _: &Scene) -> Result<(), String> {
    if b.gui.home.is_none() {
        b.send(Message::GotoHome);
    }
    b.gui
        .home
        .as_ref()
        .map(|_| ())
        .ok_or_else(|| "Home did not open".to_string())
}

/// A pull from an earlier night, opened on the stage from the history
/// store: the rail's newest stored kill — drawn by the renderers a pull of
/// the log is, fed from its card — the owner's row selected when they were
/// in it. A store holding nothing but the log's own pulls (they open as the
/// log's) has none to show.
fn stored(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    open_stored(
        b,
        scene,
        |l| l.mark == Mark::Good && l.key.is_none() && !l.trash,
        "the store holds no kill outside the log",
    )
}

/// A stored raid wipe on the stage: the rail's newest stored boss pull
/// that was not a kill — its header words how close it came.
fn stored_wipe(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    open_stored(
        b,
        scene,
        |l| l.mark == Mark::Bad && l.key.is_none() && !l.trash,
        "the store holds no wipe outside the log",
    )
}

/// A stored Mythic+ key on the stage: the rail's newest stored key, timed
/// or over — the visit's Σ on the key clock.
fn stored_key(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    open_stored(
        b,
        scene,
        |l| l.key.is_some(),
        "the store holds no key outside the log",
    )
}

/// Open the rail's newest stored pull that `pick` takes, and select the
/// owner's row on it: a press on its row, as a reader opens it.
fn open_stored(
    b: &mut Bridge,
    scene: &Scene,
    pick: impl Fn(&Line) -> bool,
    none: &str,
) -> Result<(), String> {
    let id = b
        .gui
        .rail()
        .lines()
        .find_map(|l| match &l.pull {
            Pull::Stored(id) if pick(l) => Some(id.clone()),
            _ => None,
        })
        .ok_or_else(|| none.to_string())?;
    b.send(Message::Pull(Pull::Stored(id)));
    match b.gui.stored.as_ref() {
        Some(s) if !s.missing => {}
        _ => return Err("the store did not answer for it".to_string()),
    }
    select_owner(b, scene);
    Ok(())
}

/// The featured fight with the pull rail open over it: the drawer the
/// fight header's list button opens where the rail is not docked.
fn rail_open(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    b.send(Message::OpenRail);
    Ok(())
}

/// The rail with "Hide trash" pressed — the toggle raised, and the nights
/// without their trash (the pull on the stage kept) — open over the fight.
/// A rail with no trash row is skipped: its picture would be `rail-open`'s
/// under another name.
fn hide_trash(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    if !b.gui.rail().lines().any(|l| l.trash) {
        return Err("the rail holds no trash to hide".to_string());
    }
    b.send(Message::HideTrash);
    b.send(Message::OpenRail);
    if b.gui.hide_trash {
        Ok(())
    } else {
        Err("the toggle did not take".to_string())
    }
}

/// A pull of an earlier night on the stage — a kill from the deepest of
/// the first four earlier nights the rail holds, far enough down that the
/// drawer must scroll to it — with the rail open over it, scrolled to its
/// row (`REVEALED`).
fn rail_earlier(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    let rail = b.gui.rail();
    let first = rail
        .earlier()
        .ok_or_else(|| "the rail holds no earlier night".to_string())?;
    let lines_of = |n: &crate::rail::Night| -> Vec<Line> {
        n.visits
            .iter()
            .flat_map(|v| v.lines.iter().cloned())
            .collect()
    };
    let pick = |lines: Vec<Line>| {
        lines
            .iter()
            .find(|l| l.mark == Mark::Good)
            .or(lines.first())
            .map(|l| l.pull.clone())
    };
    let pull = (first..first + 4)
        .rev()
        .find_map(|i| rail.nights.get(i).and_then(|n| pick(lines_of(n))))
        .ok_or_else(|| "the earlier nights hold no pull".to_string())?;
    b.send(Message::Pull(pull));
    if b.gui.stored.as_ref().is_some_and(|s| s.missing) {
        return Err("the store did not answer for it".to_string());
    }
    select_owner(b, scene);
    b.send(Message::OpenRail);
    Ok(())
}

/// The owner's drill, one ability deeper: its top spell's stat strip,
/// targets and focus curve.
fn spell_drill(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    drill(b, scene)?;
    b.send(Message::SpellRow(0));
    b.gui
        .fight()
        .drill_spell()
        .map(|_| ())
        .ok_or_else(|| "the ability drill did not open".to_string())
}

/// The gear's options card over the meter: the chrome's two chips.
fn options(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    b.send(Message::ToggleOptions);
    Ok(())
}

/// The `?` sheet over the meter.
fn keys(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    b.send(Message::ToggleShortcuts);
    Ok(())
}

/// The character menu, opened from the bar's picker over the meter.
fn picker(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    if b.gui.known_characters.is_empty() {
        return Err("the window knows no characters".to_string());
    }
    b.send(Message::TogglePicker);
    Ok(())
}

/// The row filter, focused (see `FOCUSED`) and narrowing the meter to the
/// owner's first two letters.
fn filter(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    let needle: String = scene
        .owner
        .as_ref()
        .map_or("a", |(label, _)| label.as_str())
        .chars()
        .take(2)
        .collect();
    b.send(Message::Filter(needle));
    // What the click's release tells a running window: the field has
    // focus, and the box widens and frames itself for it. The picture's
    // own click (`FOCUSED`) gives iced's focus, the caret.
    b.send(Message::FocusFilter);
    Ok(())
}

/// The meter in the class chrome: the owner's class on the underlines and
/// chips, where the shipping default is gold.
fn damage_class(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    b.send(Message::SetChrome(crate::theme::Chrome::Class));
    damage(b, scene)
}

/// The talent viewer (`t`) on the owner's meter row: their logged build
/// laid out against this machine's talent dataset and art, when the
/// per-machine caches are there (`tools/gen-talent-trees.sh`,
/// `tools/gen-talent-art.sh`) — else the viewer's own "no dataset" page,
/// which is what such a machine shows.
fn talents(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    damage(b, scene)?;
    b.send(chr("t"));
    b.gui
        .talents
        .as_ref()
        .map(|_| ())
        .ok_or_else(|| "the talent viewer did not open".to_string())
}

// ---- driving -----------------------------------------------------------------

/// Open the featured fight: a press on its row on the rail — away from
/// Home, or from the live meter a launch landed on.
fn open_fight(b: &mut Bridge, scene: &Scene) -> Result<(), String> {
    let (pos, name) = scene
        .fight
        .as_ref()
        .ok_or_else(|| "no boss fight in the log".to_string())?;
    let id = b
        .gui
        .state
        .entries()
        .get(*pos)
        .map(|e| e.id)
        .ok_or_else(|| "the fight left the list".to_string())?;
    b.send(Message::Pull(Pull::Log(id)));
    match b.gui.fight().segment_name() {
        Some(n) if n == *name => Ok(()),
        other => Err(format!("opened {other:?}, not {name:?}")),
    }
}

/// The fight in `view`, as its tab shows it, the owner's row selected
/// when they have one there.
fn in_view(b: &mut Bridge, scene: &Scene, view: View) -> Result<(), String> {
    open_fight(b, scene)?;
    b.send(Message::PickView(view));
    if b.gui.fight().rows().is_empty() {
        return Err(format!("no {} rows", wowdps_model::fmt::view_name(view)));
    }
    select_owner(b, scene);
    Ok(())
}

/// The owner's row selected on the pull on the stage, the way the keys
/// get there, when they have one.
fn select_owner(b: &mut Bridge, scene: &Scene) {
    if let Some(at) = owner_row(b, scene) {
        for _ in 0..b.gui.fight().rows().len() {
            let sel = b.gui.fight().row_sel;
            if sel == at {
                break;
            }
            let _ = update(&mut b.gui, chr(if sel < at { "j" } else { "k" }));
        }
        // The inspector follows the selection: let its drill's answer in.
        b.settle();
    }
}

/// The selection's drill is in the inspector, and Enter hands it the
/// keys — what a narrow window draws as the inspector pushed over the
/// meter.
fn drill_opened(b: &mut Bridge) -> Result<(), String> {
    b.send(named(Named::Enter));
    match (&b.gui.fight().drill, b.gui.fight().inspecting()) {
        (Some(_), true) => Ok(()),
        (None, _) => Err("the selection has no drill".to_string()),
        (Some(_), false) => Err("the inspector did not take the keys".to_string()),
    }
}

/// The owner's row on the meter as it stands — on a stored pull, whichever
/// of their characters played it, as the window resolves it.
fn owner_row(b: &Bridge, scene: &Scene) -> Option<usize> {
    let rows = b.gui.fight().rows();
    if b.gui.stored.is_some() {
        return b.gui.owner_of(&rows);
    }
    let (_, guid) = scene.owner.as_ref()?;
    rows.iter().position(|r| r.key == *guid)
}

/// The fight header's chrome budget over a real log: at the wide frame,
/// in the window's own fonts, the featured fight's first meter row starts
/// no more than 230 px down and 19 of its rows show without a scroll (or
/// every row, for a smaller group). Ignored like the shots — it parses the
/// log whole — and a no-op without `WOWDPS_SHOTS_LOG`, which it reads with
/// `WOWDPS_SHOTS_FIGHT` and `WOWDPS_SHOTS_OWNER` as the shots do.
#[test]
#[ignore = "design review: measures the meter over $WOWDPS_SHOTS_LOG (crates/gui/SHOTS.md)"]
fn the_chrome_budget_holds_on_the_log() {
    let Some(log) = std::env::var_os("WOWDPS_SHOTS_LOG").map(PathBuf::from) else {
        eprintln!("chrome budget: set WOWDPS_SHOTS_LOG to a combat log to measure over");
        return;
    };
    isolate_config();
    let owner = std::env::var("WOWDPS_SHOTS_OWNER").unwrap_or_else(|_| "Tranqlock".to_string());
    let fight = std::env::var("WOWDPS_SHOTS_FIGHT").ok();
    let mut mock = MockDaemon::fixture_at(&log);
    let scene = resolve(&mut mock, fight.as_deref(), &owner);
    let names: Vec<String> = scene.owner.iter().map(|(label, _)| label.clone()).collect();
    let mock = mock.with_characters(&names).with_history();
    let mut b = launch(mock, shot_config(scene.owner.as_ref()), scene.tonight);
    damage(&mut b, &scene).expect("the featured fight's meter");
    let (_, size) = SIZES[0];
    let mut ui = simulator_as(settings(), size, view::view(&b.gui));
    let list = ui
        .find(view::meter_list_id())
        .expect("the meter's rows")
        .bounds();
    let players = b.gui.state.rows().len();
    let shown = (list.height / crate::theme::pitch::ROW).floor() as usize;
    eprintln!(
        "chrome budget: the first row starts {:.1} px down; {shown} rows show of {players}",
        list.y
    );
    assert!(list.y <= 230.0, "the first row starts {} px down", list.y);
    assert!(
        shown >= 19_usize.min(players),
        "{shown} rows show of {players}"
    );
}

#[test]
fn the_featured_fight_is_the_named_one_or_the_first_kill() {
    let row = |name: &str, kind, success| ListRow {
        kind,
        name: name.to_string(),
        start_ms: 0,
        success,
        duration_ms: 0,
        live: false,
        instance: None,
        pars_ms: None,
        arena: false,
        encounter: None,
    };
    let rows = [
        row("Trash", SegmentKind::Trash, None),
        row("Boss A", SegmentKind::Encounter, Some(false)),
        row("Boss B", SegmentKind::Encounter, Some(true)),
        row("Boss A", SegmentKind::Encounter, Some(true)),
    ];
    assert_eq!(pick_fight(&rows, None), Some((2, "Boss B".to_string())));
    assert_eq!(
        pick_fight(&rows, Some("boss a")),
        Some((3, "Boss A".to_string()))
    );
    assert_eq!(
        pick_fight(&rows, Some("Trash")),
        Some((0, "Trash".to_string()))
    );
    assert_eq!(pick_fight(&rows, Some("Nobody")), None);
    // No kill at all: the first boss pull stands in.
    assert_eq!(
        pick_fight(&rows[..2], None),
        Some((1, "Boss A".to_string()))
    );
}

#[test]
fn the_owner_is_matched_whole_or_by_the_name_half() {
    assert!(is_owner("Tranqlock-Proudmoore-US", "Tranqlock"));
    assert!(is_owner(
        "Tranqlock-Proudmoore-US",
        "tranqlock-proudmoore-us"
    ));
    assert!(is_owner("Akanôs-Nebula-US", "AKANÔS"));
    assert!(!is_owner("Tranqlocker-Proudmoore-US", "Tranqlock"));
    assert!(!is_owner("Tranqlock-Proudmoore-US", "Proudmoore"));
}

/// The harness's driving over the committed fixtures: every state reaches
/// what its name says (or is skipped, where the log cannot show it), and
/// one photograph lands under the name SHOTS.md promises. Photographing
/// every state at three sizes is what the ignored run is for — too slow to
/// pay on every `cargo test`.
#[test]
fn every_state_is_reachable_over_the_fixture() {
    isolate_config();
    let mut mock = MockDaemon::fixture_at(Path::new(wowdps_daemon::mock::FIXTURE));
    let scene = resolve(&mut mock, None, "Thraxx");
    assert_eq!(
        scene.fight.as_ref().map(|(_, n)| n.as_str()),
        Some("The Ashen Warden")
    );
    let (label, _) = scene.owner.clone().expect("Thraxx fought the kill");
    assert_eq!(label, "Thraxx-Nebula-US");
    assert!(!scene.live, "the kill is not the fixture's newest segment");
    let mut mock = mock.with_characters(&[label]).with_history();
    let cfg = shot_config(scene.owner.as_ref());
    let mut skipped = Vec::new();
    for (state, reach) in STATES {
        let mut b = launch(mock, cfg.clone(), scene.tonight);
        if reach(&mut b, &scene).is_err() {
            skipped.push(state);
            mock = b.mock;
            continue;
        }
        let fight = b.gui.fight();
        match state {
            "compare" => assert_eq!(fight.screen, Screen::Compare),
            "home" => assert!(!b.gui.home.as_ref().unwrap().cards.is_empty()),
            "talents" => assert!(b.gui.talents.is_some()),
            _ => assert_eq!(
                fight.segment_name().as_deref(),
                Some("The Ashen Warden"),
                "{state}"
            ),
        }
        match state {
            "stored" => assert!(b.gui.stored.is_some(), "the store's copy"),
            "rail-open" => assert!(b.gui.rail_open),
            "hide-trash" => assert!(b.gui.rail_open && b.gui.hide_trash),
            _ => assert!(b.gui.stored.is_none(), "{state}: the log's own pull"),
        }
        // Home was up at launch, so the window remembers the owner the
        // store stamped — the strip's character picker has them.
        assert!(
            b.gui
                .known_characters
                .iter()
                .any(|c| c.name == "Thraxx-Nebula-US"),
            "{state}"
        );
        if state != "home" {
            assert!(b.gui.home.is_none(), "{state}: Home stepped aside");
        }
        let mine = owner_row(&b, &scene);
        let fight = b.gui.fight();
        match state {
            "damage" => assert!(mine.is_some(), "Thraxx dealt damage"),
            "drill" => assert_eq!(
                fight.drill.as_ref().map(|d| d.label.as_str()),
                Some("Thraxx-Nebula-US")
            ),
            // Thraxx lived through the kill: the recap is the one death's.
            "deaths-drill" => {
                assert_eq!(fight.view, View::Deaths);
                assert_eq!(
                    fight.drill.as_ref().map(|d| d.label.as_str()),
                    Some("Mírelle-Nebula-US")
                );
            }
            "enemies-drill" => {
                assert_eq!(fight.view, View::EnemyTaken);
                let top = fight.rows().first().map(|r| r.label.clone());
                assert_eq!(fight.drill.as_ref().map(|d| d.label.clone()), top);
            }
            _ => {}
        }
        // Wherever the owner has a row on a meter the reader is on, it is
        // the selected one — and the inspector beside it is theirs.
        if !fight.inspecting()
            && fight.screen == Screen::Meter
            && let Some(mine) = mine
        {
            assert_eq!(fight.row_sel, mine, "{state}: owner selected");
            let rows = fight.rows();
            assert_eq!(
                fight.drill.as_ref().map(|d| d.key.as_str()),
                rows.get(mine).map(|r| r.key.as_str()),
                "{state}: the inspector follows the selection"
            );
        }
        mock = b.mock;
    }
    // Nobody was hit on the fixture's kill (its goldens say `taken 0`): the
    // Taken states are skipped, not photographed empty or panicked over.
    // And the store holds the fixture's own pulls alone, which open as the
    // log's: there is no stored pull to show.
    assert_eq!(
        skipped,
        [
            "taken",
            "taken-drill",
            "stored",
            "stored-wipe",
            "stored-key",
            "rail-earlier"
        ]
    );
    // R17's fixture has a tank, and both Taken states reach over it.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");
    let mut taken_mock = MockDaemon::fixture_at(Path::new(path));
    let taken_scene = resolve(&mut taken_mock, None, "Nobody");
    for (state, reach) in STATES.iter().filter(|(s, _)| s.starts_with("taken")) {
        let mut b = launch(taken_mock, shot_config(None), taken_scene.tonight);
        reach(&mut b, &taken_scene).unwrap_or_else(|why| panic!("{state}: {why}"));
        assert_eq!(b.gui.fight().view, View::Taken);
        if *state == "taken-drill" {
            assert_eq!(
                b.gui.fight().drill.as_ref().map(|d| d.label.as_str()),
                Some("Durgan-Nebula-US"),
                "the top of Taken is the tank"
            );
        }
        taken_mock = b.mock;
    }
    // One real photograph, through the same path the ignored test takes.
    let dir = std::env::temp_dir().join(format!("wowdps-shots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut b = launch(mock, cfg, scene.tonight);
    rail_open(&mut b, &scene).unwrap();
    // The drawer covers the stage: there is no filter to focus.
    let (out, trouble) = shoot(
        &b.gui,
        Size::new(960.0, 880.0),
        &dir,
        "tile-rail-open",
        false,
        true,
    );
    assert_eq!(trouble, None);
    assert_eq!(out, dir.join("tile-rail-open.png"));
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
    assert!(
        !dir.join(".render").exists(),
        "the scratch directory is gone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A run clears what the last one wrote — renamed states and a panicked
/// run's scratch directory included — and nothing it did not write.
#[test]
fn a_run_clears_only_what_the_harness_wrote() {
    let dir = std::env::temp_dir().join(format!("wowdps-shots-clear-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".render")).unwrap();
    std::fs::write(
        dir.join("manifest.txt"),
        "rev: x\nwide-renamed.png  1440x900\nskipped taken: none\n",
    )
    .unwrap();
    for f in [
        "wide-renamed.png",
        "narrow-rail-open.png",
        "wide-home-reference.png",
    ] {
        std::fs::write(dir.join(f), b"png").unwrap();
    }
    clear_previous(&dir);
    assert!(!dir.join(".render").exists(), "a panicked run's scratch");
    assert!(
        !dir.join("wide-renamed.png").exists(),
        "the last manifest's"
    );
    assert!(
        !dir.join("narrow-rail-open.png").exists(),
        "one of this run's names"
    );
    assert!(
        dir.join("wide-home-reference.png").exists(),
        "a picture the harness never wrote stays"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory holding pictures under the harness's names is written into
/// only when the harness's own manifest says it made them: the prototype's
/// references share several names.
#[test]
fn a_run_refuses_a_directory_the_harness_did_not_write() {
    let dir = std::env::temp_dir().join(format!("wowdps-shots-ours-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Empty, or holding only names the harness never writes: fine.
    assert_eq!(check_ours(&dir), Ok(()));
    std::fs::write(dir.join("wide-taken-mehna.png"), b"png").unwrap();
    assert_eq!(check_ours(&dir), Ok(()));
    // The references' `wide-damage.png`, no manifest: refused.
    std::fs::write(dir.join("wide-damage.png"), b"png").unwrap();
    let why = check_ours(&dir).unwrap_err();
    assert!(why.contains("wide-damage.png"), "{why}");
    // A manifest some other tool wrote does not make it ours.
    std::fs::write(dir.join("manifest.txt"), "log: x\n").unwrap();
    assert!(check_ours(&dir).is_err());
    // The harness's own manifest does.
    std::fs::write(
        dir.join("manifest.txt"),
        "rev: x\nwide-damage.png  1440x900\n",
    )
    .unwrap();
    assert_eq!(check_ours(&dir), Ok(()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_wall_clock_is_the_inverse_of_home_s_dates() {
    for ymd in ["1970-01-01", "2000-02-29", "2026-09-27", "2100-03-01"] {
        let day = crate::home::parse_ymd(ymd).unwrap();
        assert_eq!(wall_clock(day), format!("{ymd} 00:00:00"));
    }
    // A card's start on the featured night: 19:24:48.980 on the log's clock.
    assert_eq!(wall_clock(1_790_537_088_980), "2026-09-27 19:24:48");
    assert_eq!(wall_clock(-1), "1969-12-31 23:59:59");
}

/// Two stores with as many cards are told apart by the fingerprint: a card
/// that grew, or one swapped for another.
#[test]
fn the_store_fingerprint_sees_more_than_the_count() {
    let root = std::env::temp_dir().join(format!("wowdps-shots-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("fights")).unwrap();
    let card =
        |name: &str, body: &str| std::fs::write(root.join("fights").join(name), body).unwrap();
    card("a-1.json", "{}");
    card("a-2.json", "{}");
    card("a-3.json.tmp", "in flight");
    let first = store_fingerprint(&root);
    assert!(first.ends_with("over 2 fights/ files"), "{first}");
    card("a-2.json", "{\"pinned\":true}");
    let grown = store_fingerprint(&root);
    assert_ne!(first, grown);
    std::fs::remove_file(root.join("fights").join("a-2.json")).unwrap();
    card("a-9.json", "{\"pinned\":true}");
    assert_ne!(store_fingerprint(&root), grown, "same count, another card");
    let _ = std::fs::remove_dir_all(&root);
}

/// A window that never saw Home at launch — here a config without the
/// offer (the user's own `home_on_start = false`), over a live pull —
/// knows the owner like every other shot's window: the rail's first page
/// of the store names them, and `launch` leaves it where it launched.
#[test]
fn a_window_that_skipped_home_at_launch_still_knows_the_owner() {
    isolate_config();
    let mut mock = MockDaemon::fixture_live();
    let scene = resolve(&mut mock, None, "Thraxx");
    let owner = scene.owner.expect("Thraxx fought the kill");
    let mock = mock
        .with_characters(std::slice::from_ref(&owner.0))
        .with_history();
    let cfg = Config {
        home_on_start: false,
        ..shot_config(Some(&owner))
    };
    let b = Bridge::with_config(mock, cfg.clone());
    assert!(b.gui.state.is_live(), "the fixture's last fight is open");
    assert!(b.gui.home.is_none(), "no Home at launch");
    assert!(
        b.gui
            .known_characters
            .iter()
            .any(|c| c.name == "Thraxx-Nebula-US"),
        "the rail's page named them"
    );
    let b = launch(b.mock, cfg, scene.tonight);
    assert!(b.gui.home.is_none(), "where it launched");
    assert!(b.gui.state.following_live() && b.gui.state.is_live());
    assert!(
        b.gui
            .known_characters
            .iter()
            .any(|c| c.name == "Thraxx-Nebula-US"),
        "{:?}",
        b.gui.known_characters
    );
}
