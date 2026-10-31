//! The per-machine caches the GUI and the mcp tools read, generated from
//! the install and never committed (extracted Blizzard art and words stay
//! out of the repository): the class crests and spec icons
//! (`gen-icons`), every spell's icon (`gen-spell-icons`), the talent trees
//! (`gen-talent-trees`), the talent UI's art (`gen-talent-art`) and the
//! spell tips for the reader gui-logic gains with them (`gen-spell-tips`).

use std::path::Path;

use super::args::Args;
use super::install::{Install, load_tables};
use super::{Cmd, cmd, data_path, make_dir, read_text};
use wowdps_extract::atomic::{write_atomic, write_if_changed};
use wowdps_extract::{artgen, icongen, journal, spellicongen, spelltipgen, talentgen};

/// This module's commands, as the usage lists them.
pub const COMMANDS: &[Cmd] = &[
    cmd(
        "gen-icons",
        "gen-icons [wow-dir] --dbd-dir <dir> [-o class-icons.bin]
                       [--whole DIR [--whole-only]] [--upscaled DIR] [--keys tactkeys.txt]",
        icons,
    )
    .reads(&[&[icongen::TABLE]]),
    cmd(
        "gen-spell-icons",
        "gen-spell-icons [wow-dir] --dbd-dir <dir>
                       [-o spell-icons.bin] [--keys tactkeys.txt]",
        spell_icons,
    )
    .reads(&[&[spellicongen::TABLE]]),
    cmd(
        "gen-spell-tips",
        "gen-spell-tips [wow-dir] --dbd-dir <dir> [-o spell-tips.bin]
                       [--store history-dir] [--force]
                       [--keys tactkeys.txt]
                       (tools/gen-spell-tips.sh: the replay's spell tooltips and lane facts)",
        spell_tips,
    )
    .reads(&[&spelltipgen::TABLES, &journal::JOURNAL_TABLES]),
    cmd(
        "gen-talent-trees",
        "gen-talent-trees [wow-dir] --dbd-dir <dir>
                       [-o talents.json] [--keys tactkeys.txt]
                       [--listfile icons.csv]",
        talent_trees,
    )
    .reads(&[&talentgen::TABLES]),
    cmd(
        "gen-talent-art",
        "gen-talent-art [wow-dir] --dbd-dir <dir>
                       [-o talent-art.bin] [--keys tactkeys.txt]",
        talent_art,
    )
    .reads(&[&artgen::TABLES]),
];

/// `gen-icons`: the class crests and spec icons as class-icons.bin.
/// `--whole DIR` also writes each icon at its native 64 px as
/// `DIR/<fdid>.png`, for the upscaler (with `--whole-only`, those alone);
/// `--upscaled DIR` cuts the tiles from their 4× upscales there
/// (tools/gen-icons.sh runs Real-ESRGAN between).
pub fn icons(mut a: Args) -> Result<(), String> {
    let out = a.path(&["-o", "--out"])?;
    let whole = a.path(&["--whole"])?;
    let upscaled = a.path(&["--upscaled"])?;
    let whole_only = a.flag(&["--whole-only"]);
    let install = Install::with_tables(&mut a)?;
    a.finish()?;
    if whole_only && (whole.is_none() || out.is_some() || upscaled.is_some()) {
        return Err(
            "gen-icons --whole-only takes --whole DIR alone: it writes the icons whole and no cache"
                .into(),
        );
    }
    let (game, tables) = install.open_tables()?;
    let csv = tables.get(icongen::TABLE)?;

    if let Some(dir) = &whole {
        make_dir(dir)?;
    }
    let dirs = icongen::Dirs {
        whole: whole.as_deref(),
        upscaled: upscaled.as_deref(),
    };
    let g = icongen::generate(&game, csv, dirs)?;
    if whole_only {
        if let Some(dir) = &whole {
            eprintln!(
                "{}: {} class + {} spec icons whole: what the upscaler reads",
                dir.display(),
                g.classes,
                g.specs
            );
        }
        return Ok(());
    }
    // A per-machine cache like the spell icons — extracted Blizzard art
    // never lands in the repository.
    let out = match out {
        Some(p) => p,
        None => data_path("class-icons.bin")?,
    };
    if let Some(parent) = out.parent() {
        make_dir(parent)?;
    }
    // Renamed into place: a running GUI reads its tiles from the file it
    // opened, which must keep the bytes its header describes.
    write_atomic(&out, &g.bytes)?;
    eprintln!(
        "{}: {} class + {} spec icons, {} px{} ({} KiB)",
        out.display(),
        g.classes,
        g.specs,
        g.icon_px,
        if upscaled.is_some() { ", upscaled" } else { "" },
        g.bytes.len() / 1024
    );
    Ok(())
}

pub fn spell_icons(mut a: Args) -> Result<(), String> {
    let out = a.path(&["-o", "--out"])?;
    let install = Install::with_tables(&mut a)?;
    a.finish()?;
    let out = match out {
        Some(p) => p,
        None => data_path("spell-icons.bin")?,
    };
    let (game, tables) = install.open_tables()?;

    let g = spellicongen::generate(&game, tables.get(spellicongen::TABLE)?)?;
    if let Some(parent) = out.parent() {
        make_dir(parent)?;
    }
    // Renamed into place, like the class icons: the GUI seeks into it.
    write_atomic(&out, &g.bytes)?;
    eprintln!(
        "{}: {} spells -> {} tiles ({} MiB; {} icons skipped)",
        out.display(),
        g.spells,
        g.tiles,
        g.bytes.len() / (1024 * 1024),
        g.skipped
    );
    Ok(())
}

/// `gen-spell-tips`: the replay's spell tips (`spelltipgen`) as
/// spell-tips.bin beside the icons, for every spell the history store's
/// replays (`--store`, else `$XDG_DATA_HOME/wowdps/history/v1`) name and
/// every spell the curated tables list. A file whose stamp says it was
/// made by these rules from this build for these spells is current, and
/// no table is decoded (`--force` decodes them anyway); the bytes are the
/// same run after run, and a file already holding them is left alone.
pub fn spell_tips(mut a: Args) -> Result<(), String> {
    let out = a.path(&["-o", "--out"])?;
    let store = a.path(&["--store"])?;
    let force = a.flag(&["--force"]);
    let install = Install::with_tables(&mut a)?;
    a.finish()?;
    let started = std::time::Instant::now();
    let out = match out {
        Some(p) => p,
        None => data_path(spelltipgen::FILE)?,
    };
    let store = match store {
        Some(s) => s,
        None => data_path("history/v1")?,
    };
    let mut wanted = spelltipgen::Wanted::default();
    match spelltipgen::store_spells(&store) {
        Ok(scan) => {
            wanted.store = scan.ids.len();
            wanted.store_files = scan.read;
            wanted.store_unread = scan.unread;
            wanted.ids.extend(scan.ids);
            wanted.fights = scan.fights;
        }
        Err(e) => eprintln!("gen-spell-tips: no store read ({e})"),
    }
    let (curated, class) = spelltipgen::curated_spells();
    wanted.curated = curated.len();
    wanted.ids.extend(curated);
    wanted.class = class;
    eprintln!(
        "gen-spell-tips: {} spells asked for: {} from {} replay files in {} ({} passed over), {} from the curated tables",
        wanted.ids.len(),
        wanted.store,
        wanted.store_files,
        store.display(),
        wanted.store_unread,
        wanted.curated
    );
    let game = install.open()?;
    let stamp = spelltipgen::stamp(&game.build, &wanted.ids, &wanted.fights);
    let held = std::fs::read(&out).ok();
    if !force && held.as_deref().and_then(spelltipgen::read_stamp) == Some(stamp) {
        eprintln!(
            "{}: current (build {}, the same spells); nothing decoded ({:.1} s; --force regenerates)",
            out.display(),
            game.build,
            started.elapsed().as_secs_f32()
        );
        return Ok(());
    }
    let tables = load_tables(&game, install.dbd_dir()?, &install.tables)?;
    let g = spelltipgen::generate(&tables, &wanted, stamp)?;
    if let Some(parent) = out.parent() {
        make_dir(parent)?;
    }
    let wrote = write_if_changed(&out, &g.bytes)?;
    eprintln!(
        "{}: {} spells ({} with a parent, {} of them active; {} effects whose press some spec's logs omit, from {} players' fights; {} with the journal's words; {} asked for have no name), build {}, {} KiB{} in {:.1} s",
        out.display(),
        g.records,
        g.parents,
        g.active_parents,
        g.marked,
        wanted.fights.len(),
        g.journal,
        g.nameless,
        game.build,
        g.bytes.len() / 1024,
        if wrote { "" } else { ", unchanged" },
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

pub fn talent_art(mut a: Args) -> Result<(), String> {
    let out = a.path(&["-o", "--out"])?;
    let install = Install::with_tables(&mut a)?;
    a.finish()?;
    // A per-machine cache like the icon caches — extracted Blizzard art
    // never lands in the repository.
    let out = match out {
        Some(p) => p,
        None => data_path("talent-art.bin")?,
    };
    let (game, tables) = install.open_tables()?;

    let g = artgen::generate(&game, &tables)?;
    if let Some(parent) = out.parent() {
        make_dir(parent)?;
    }
    // Renamed into place, like the class icons: the GUI seeks into it.
    write_atomic(&out, &g.bytes)?;
    for m in &g.missing {
        eprintln!("unresolved: {m}");
    }
    eprintln!(
        "{}: {} spec backgrounds, {} hero medallions ({} MiB; {} unresolved)",
        out.display(),
        g.backgrounds,
        g.medallions,
        g.bytes.len() / (1024 * 1024),
        g.missing.len()
    );
    Ok(())
}

pub fn talent_trees(mut a: Args) -> Result<(), String> {
    let out = a.path(&["-o", "--out"])?;
    let listfile = a.path(&["--listfile"])?;
    let install = Install::with_tables(&mut a)?;
    a.finish()?;
    // A per-machine cache like the icon caches — Blizzard-derived strings
    // (spell names) never land in the repository.
    let out = match out {
        Some(p) => p,
        None => data_path("talents.json")?,
    };
    let (game, tables) = install.open_tables()?;
    let icon_names = match &listfile {
        Some(p) => icon_listfile(p)?,
        None => {
            eprintln!("no --listfile: icon names omitted (iconFdid still emitted)");
            std::collections::HashMap::new()
        }
    };

    let g = talentgen::generate(&tables, &icon_names, &game.build)?;
    if let Some(parent) = out.parent() {
        make_dir(parent)?;
    }
    write_atomic(&out, &g.content)?;
    eprintln!(
        "{}: {} trees, {} specs, {} nodes, build {} ({} KiB; {} spells nameless)",
        out.display(),
        g.trees,
        g.specs,
        g.nodes,
        game.build,
        g.content.len() / 1024,
        g.nameless
    );
    Ok(())
}

/// Parse a `fdid;path` listfile into FileDataID → icon name (basename, no
/// extension) for `interface/icons/` entries; other rows are skipped, so a
/// full community listfile and a pre-filtered subset both work.
fn icon_listfile(path: &Path) -> Result<std::collections::HashMap<u32, String>, String> {
    let text = read_text(path)?;
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let Some((fdid, p)) = line.split_once(';') else {
            continue;
        };
        let Some(rest) = p.strip_prefix("interface/icons/") else {
            continue;
        };
        let name = rest.split('.').next().unwrap_or(rest);
        if let Ok(fdid) = fdid.parse::<u32>() {
            out.insert(fdid, name.to_string());
        }
    }
    Ok(out)
}
