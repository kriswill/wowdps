//! `seed-creatures` (tools/seed-creatures.sh): NPC classifications from
//! Wowhead (`wowhead`) laid under the addon's in the history store's
//! creatures file, the daemon's table (`wowdps_proto::creatures`).

use super::args::Args;
use super::{Cmd, at, cmd, data_path, make_dir};
use wowdps_extract::atomic::write_atomic;
use wowdps_extract::wowhead;
use wowdps_proto::creatures;

/// This module's commands, as the usage lists them.
pub const COMMANDS: &[Cmd] = &[
    cmd(
        "seed-creatures",
        "seed-creatures [--store history-dir] [-o creatures.tsv] [--cache dir] [--dry-run]
                       (tools/seed-creatures.sh: NPC classifications from Wowhead, under the addon's)",
        seed_creatures,
    ),
];

/// Seed the history store's creatures file from Wowhead (`wowhead`): every
/// hostile NPC of the store's replay tier and every NPC of the season's
/// zone listings, classified, laid under the rows the addon saw
/// (`wowhead::lay_seed`) and written whole. The store is
/// `$XDG_DATA_HOME/wowdps/history/v1` unless `--store` says, the file its
/// `creatures.tsv` unless `-o` does, the cache
/// `$XDG_CACHE_HOME/wowdps/wowhead` unless `--cache` does. `--dry-run`
/// fetches and reports but writes nothing.
fn seed_creatures(mut a: Args) -> Result<(), String> {
    let store = a.path(&["--store"])?;
    let out = a.path(&["-o", "--out"])?;
    let cache = a.path(&["--cache"])?;
    let dry_run = a.flag(&["--dry-run"]);
    a.finish()?;
    let started = std::time::Instant::now();
    let store = match store {
        Some(s) => s,
        None => data_path("history/v1")?,
    };
    let out = out.unwrap_or_else(|| store.join(creatures::FILE));
    let cache = match cache {
        Some(c) => c,
        None => wowdps_proto::dirs::cache_home()
            .ok_or("no XDG_CACHE_HOME or HOME")?
            .join("wowdps/wowhead"),
    };
    // The file this run lays its seed under, read first: one in a format
    // this build cannot read whole is never rewritten.
    let mut table = match std::fs::read_to_string(&out) {
        Ok(text) => creatures::parse(&text).map_err(at(&out))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => return Err(at(&out)(e)),
    };
    let npcs = wowhead::store_npcs(&store)?;
    eprintln!(
        "seed-creatures: {} replay files in {} ({} passed over): {} hostile NPC ids on {} maps, {} friendly ids left out",
        npcs.files,
        store.display(),
        npcs.unread,
        npcs.ids.len(),
        npcs.maps.len(),
        npcs.friendly.len()
    );
    for (map, m) in &npcs.maps {
        if !wowhead::ZONES.iter().any(|z| z.map == *map) {
            eprintln!(
                "  map {map} {}: {} cuts, {} NPC ids, no zone listing (by tooltip)",
                m.zone,
                m.cuts,
                m.ids.len()
            );
        }
    }
    let mut fetch = wowhead::Fetcher::new(cache.clone());
    let seed = wowhead::seed(&npcs, wowhead::ZONES, &mut fetch).map_err(|e| {
        format!(
            "{e}\nevery answer so far is cached under {}; a rerun resumes there",
            cache.display()
        )
    })?;
    for z in &seed.zones {
        if !z.title.contains(z.zone.name) {
            eprintln!(
                "warning: zone {} is titled {:?}, not {:?}: check ZONES",
                z.zone.zone, z.title, z.zone.name
            );
        }
    }
    for (id, why) in &seed.failed {
        eprintln!("  npc {id}: {why}");
    }
    eprintln!(
        "ids: {} from the store ({} of them classified), {} from {} zone listings, {} classified ({} by a listing, {} by a tooltip), {} left out (a tooltip without a rank), {} failed",
        npcs.ids.len(),
        npcs.ids
            .keys()
            .filter(|id| seed.rows.contains_key(id))
            .count(),
        seed.zone_ids,
        seed.zones.len(),
        seed.rows.len(),
        seed.by_listing,
        seed.by_tooltip,
        seed.unranked,
        seed.failed.len()
    );
    eprintln!(
        "requests: {} sent, {} answers from the cache ({})",
        fetch.requests,
        fetch.cached,
        cache.display()
    );
    let laid = wowhead::lay_seed(&mut table, seed.rows.into_values().collect());
    let histogram = wowhead::histogram(&table)
        .iter()
        .map(|(c, n)| format!("{c} {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if dry_run { "would write" } else { "wrote" };
    if !dry_run {
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            make_dir(dir)?;
        }
        write_atomic(&out, creatures::render(table.values()).as_bytes())?;
    }
    eprintln!(
        "{}: {verb} {} rows: {} added, {} seed rows refreshed, {} sightings kept over the seed, {} other rows kept",
        out.display(),
        table.len(),
        laid.added,
        laid.refreshed,
        laid.sighted,
        laid.untouched
    );
    eprintln!("classifications: {histogram}");
    eprintln!("elapsed {:.1} s", started.elapsed().as_secs_f64());
    Ok(())
}
