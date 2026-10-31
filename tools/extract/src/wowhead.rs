//! seed-creatures: NPC classifications from Wowhead's NPC data, laid under
//! the addon's sightings in the history store's `creatures.tsv`
//! (`proto::creatures`), so the replay's rings read silver for a rare or a
//! rare elite before the addon has seen it in game.
//!
//! Two of Wowhead's answers, fetched with `curl` (the crate stays
//! stdlib-only) and cached whole under `~/.cache/wowdps/wowhead/`:
//!
//! - a zone's page, `https://www.wowhead.com/zone=<id>`, whose `npcs`
//!   Listview holds an object per NPC with `id`, `name` and a numeric
//!   `classification` (`npcs`, `listing_class`), for every zone in `ZONES`
//!   (the season's dungeons and the raids the store holds);
//! - an NPC's tooltip,
//!   `https://nether.wowhead.com/tooltip/npc/<id>?dataEnv=1&locale=0`, JSON
//!   whose `tooltip` has a type line ending in the rank in parentheses,
//!   `Elemental (Elite)` (`tooltip`, `tooltip_class`), for an NPC the
//!   stored replays hold, or a listing names without a classification,
//!   that no listing classifies. A tooltip without a rank gives no row:
//!   Wowhead writes `(Normal)` for a normal NPC, so it holds no rank for
//!   that one.
//!
//! The NPCs asked about are every listing's and every hostile NPC of the
//! store's replay tier (`store_npcs`). A seed row is `seen_unix` 0 and
//! never a lieutenant (Wowhead keeps no lieutenant flag and no `minus`
//! rank), so any sighting the addon records wins over it under
//! `creatures::merge`; a rerun replaces the seed rows it finds and keeps
//! every sighting (`lay_seed`).
//!
//! Wowhead's numbers and words, each checked against the other on
//! 2026-10-10 (an NPC's listing number beside its tooltip's rank):
//!
//! | listing | tooltip        | `creatures.tsv` |
//! |---------|----------------|-----------------|
//! | 0       | `(Normal)`     | `normal`        |
//! | 1       | `(Elite)`      | `elite`         |
//! | 2       | `(Rare Elite)` | `rareelite`     |
//! | 3       | `(Boss)`       | `worldboss`     |
//! | 4       | `(Rare)`       | `rare`          |

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use wowdps_core::model::replay::{Cut, UnitKind};
use wowdps_proto::creatures::{self, Creature};
use wowdps_proto::json::{self, Json};

/// One instance whose zone listing seeds the file: the instance map the
/// log names (`Cut::head.map`), Wowhead's zone id for it (the client's
/// AreaTable id of the map's top area, `AreaTable.ContinentID = map` and
/// `ParentAreaID = 0`) and its name as the zone page titles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    pub map: u32,
    pub zone: u32,
    pub name: &'static str,
}

/// The zones listed: the sixteen dungeons whose levels are rendered under
/// `~/.local/share/wowdps/floors/` (their sidecars name the maps), then the
/// raids the history store holds this season. A map the store holds that
/// is not here is said in the run's report; its NPCs come by tooltip.
pub const ZONES: &[Zone] = &[
    zone(2526, 14032, "Algeth'ar Academy"),
    zone(2993, 16865, "Altar of Fangs"),
    zone(2825, 16368, "Den of Nalorakk"),
    zone(1762, 9526, "Kings' Rest"),
    zone(2811, 15829, "Magisters' Terrace"),
    zone(2874, 16395, "Maisara Caverns"),
    zone(2813, 16091, "Murder Row"),
    zone(2915, 16573, "Nexus-Point Xenas"),
    zone(658, 4813, "Pit of Saron"),
    zone(2521, 14063, "Ruby Life Pools"),
    zone(1753, 8910, "The Seat of the Triumvirate"),
    zone(1209, 6988, "Skyreach"),
    zone(1877, 9527, "Temple of Sethraliss"),
    zone(2859, 16359, "The Blinding Vale"),
    zone(2923, 16425, "Voidscar Arena"),
    zone(2805, 15808, "Windrunner Spire"),
    zone(2912, 16340, "The Voidspire"),
    zone(2913, 16342, "Isle of Quel'Danas"),
    zone(2987, 16671, "The Tidebound Grotto"),
    zone(3004, 16915, "The Venomous Abyss"),
    zone(1592, 16279, "Sporefall"),
];

/// A row of `ZONES`.
const fn zone(map: u32, zone: u32, name: &'static str) -> Zone {
    Zone { map, zone, name }
}

/// A zone's page.
pub fn zone_url(zone: u32) -> String {
    format!("https://www.wowhead.com/zone={zone}")
}

/// An NPC's tooltip, in English.
pub fn tooltip_url(id: u32) -> String {
    format!("https://nether.wowhead.com/tooltip/npc/{id}?dataEnv=1&locale=0")
}

/// A zone listing's numeric classification as `creatures.tsv` spells it.
pub fn listing_class(n: u64) -> Option<&'static str> {
    Some(match n {
        0 => "normal",
        1 => "elite",
        2 => "rareelite",
        3 => "worldboss",
        4 => "rare",
        _ => return None,
    })
}

/// A tooltip's rank, the word in the type line's parentheses, as
/// `creatures.tsv` spells it.
pub fn tooltip_class(word: &str) -> Option<&'static str> {
    Some(match word {
        "Normal" => "normal",
        "Elite" => "elite",
        "Rare Elite" => "rareelite",
        "Boss" => "worldboss",
        "Rare" => "rare",
        _ => return None,
    })
}

// ---- the store's NPCs ----------------------------------------------------------------

/// One instance map's NPCs in the stored replays.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MapNpcs {
    /// The zone the log was in as a cut began (`Cut::head.zone`).
    pub zone: String,
    pub cuts: usize,
    pub ids: BTreeSet<u32>,
}

/// What the history store's replay tier says of its NPCs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StoreNpcs {
    /// Every hostile NPC's creature id (a boss's or an add's), with the
    /// first name the log gave it.
    pub ids: BTreeMap<u32, String>,
    /// By instance map.
    pub maps: BTreeMap<u32, MapNpcs>,
    /// Friendly NPCs' ids no hostile unit shares: left out, since a cut
    /// keeps no GUID for an NPC to tell a pet from a guardian or a friendly
    /// NPC, and a pet's id is its beast's.
    pub friendly: BTreeSet<u32>,
    /// Replay files read, and those passed over (not a format this build
    /// decodes).
    pub files: usize,
    pub unread: usize,
}

impl StoreNpcs {
    /// Take one cut's NPCs in.
    pub fn add(&mut self, cut: &Cut) {
        let map = self.maps.entry(cut.head.map).or_default();
        if map.zone.is_empty() {
            map.zone.clone_from(&cut.head.zone);
        }
        map.cuts += 1;
        for u in cut.units.iter().filter(|u| u.npc > 0) {
            match u.kind {
                UnitKind::Boss | UnitKind::Add => {
                    map.ids.insert(u.npc);
                    let name = self.ids.entry(u.npc).or_default();
                    if name.is_empty() || name == "?" {
                        name.clone_from(&u.name);
                    }
                }
                UnitKind::Friendly => {
                    self.friendly.insert(u.npc);
                }
                UnitKind::Player => {}
            }
        }
        let ids = &self.ids;
        self.friendly.retain(|id| !ids.contains_key(id));
    }
}

/// Every hostile NPC in the replay tier of the history store at `store`
/// (`replay/<id>.bin`, read offline as `level-posts` reads it).
pub fn store_npcs(store: &Path) -> Result<StoreNpcs, String> {
    let dir = store.join("replay");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "bin"))
        .collect();
    files.sort();
    let mut out = StoreNpcs::default();
    for f in &files {
        match std::fs::read(f)
            .ok()
            .and_then(|b| wowdps_proto::replay::decode(&b))
        {
            Some(cut) => {
                out.add(&cut);
                out.files += 1;
            }
            None => out.unread += 1,
        }
    }
    Ok(out)
}

// ---- Wowhead's answers read ----------------------------------------------------------

/// One NPC of a zone listing; `class` is `None` where the listing's
/// classification is null or absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: u32,
    pub name: String,
    pub class: Option<&'static str>,
}

/// The JSON array opening at byte `start` of `text`: through its matching
/// bracket, strings and their escapes stepped over.
fn array_at(text: &str, start: usize) -> Option<&str> {
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for (i, c) in text.get(start..)?.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return text.get(start..=start + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A zone page's `npcs` Listview: its NPCs, and whether Wowhead cut the
/// listing short (`_truncated` among its options). An error for a page
/// without one or whose data does not read.
pub fn npcs(page: &str) -> Result<(Vec<Listed>, bool), String> {
    let at = ["id: 'npcs'", "id:'npcs'", "\"id\":\"npcs\""]
        .iter()
        .filter_map(|m| page.find(m))
        .min()
        .ok_or("no npcs listing on the page")?;
    let open = page
        .get(..at)
        .and_then(|head| head.rfind("new Listview("))
        .ok_or("the npcs listing is not a Listview")?;
    let rest = page.get(at..).unwrap_or_default();
    let data = ["data: [", "data:[", "\"data\":["]
        .iter()
        .filter_map(|m| rest.find(m).map(|i| at + i + m.len() - 1))
        .min()
        .ok_or("the npcs listing has no data")?;
    let truncated = page
        .get(open..data)
        .is_some_and(|opts| opts.contains("_truncated"));
    let array = array_at(page, data).ok_or("the npcs listing's data does not close")?;
    let parsed = json::parse(array).map_err(|e| format!("the npcs listing's data: {e}"))?;
    let listed = parsed
        .as_arr()
        .ok_or("the npcs listing's data is not an array")?
        .iter()
        .filter_map(|o| {
            let id = u32::try_from(o.get("id")?.as_u64()?)
                .ok()
                .filter(|id| *id > 0)?;
            Some(Listed {
                id,
                name: o
                    .get("name")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_string(),
                class: o
                    .get("classification")
                    .and_then(Json::as_u64)
                    .and_then(listing_class),
            })
        })
        .collect();
    Ok((listed, truncated))
}

/// A zone page's `<title>`, to check the zone id names the instance meant.
pub fn page_title(page: &str) -> Option<&str> {
    let (_, rest) = page.split_once("<title>")?;
    let (title, _) = rest.split_once("</title>")?;
    Some(title.trim())
}

/// An NPC's tooltip read: its name and classification, `None` where no
/// type line carries a rank. Wowhead writes `(Normal)` for a normal NPC, so
/// a tooltip without a rank is one it holds no rank for, not a normal one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip {
    pub name: String,
    pub class: Option<&'static str>,
}

/// A cell's text with its tags left out.
fn cell_text(cell: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in cell.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// The rank of a tooltip's HTML: the first cell whose text ends in a rank
/// in parentheses (`Level 52 Elemental (Rare Elite)`).
pub fn tooltip_rank(html: &str) -> Option<&'static str> {
    html.split("<td>").skip(1).find_map(|cell| {
        let text = cell_text(cell.split("</td>").next().unwrap_or_default());
        let (_, word) = text.strip_suffix(')')?.rsplit_once('(')?;
        tooltip_class(word.trim())
    })
}

/// An NPC's tooltip answer (`{"name": …, "tooltip": "<table>…"}`) read.
pub fn tooltip(text: &str) -> Result<Tip, String> {
    let j = json::parse(text)?;
    let name = j
        .get("name")
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or("no name in the tooltip")?
        .to_string();
    let html = j.get("tooltip").and_then(Json::as_str).unwrap_or_default();
    Ok(Tip {
        name,
        class: tooltip_rank(html),
    })
}

// ---- the fetches ---------------------------------------------------------------------

/// The pause between two requests: two a second at most.
pub const GAP: Duration = Duration::from_millis(500);

/// Tries per request before the run stops.
const TRIES: u32 = 6;

/// What a fetch got: the answer, or Wowhead's 404.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Got {
    Body(String),
    Missing,
}

/// The fetches, paced and cached: every answer is kept under the cache
/// directory before it is read, a 404 as an empty `.404` file beside where
/// its answer would be, so a rerun asks only for what it lacks.
pub struct Fetcher {
    cache: PathBuf,
    agent: String,
    last: Option<Instant>,
    /// Requests sent (retries among them), and answers the cache gave.
    pub requests: usize,
    pub cached: usize,
}

impl Fetcher {
    pub fn new(cache: PathBuf) -> Fetcher {
        Fetcher {
            cache,
            agent: format!(
                "wowdps-seed-creatures/{} (a developer's tool seeding a local NPC classification \
                 file for a combat-log replay; at most 2 requests a second, every answer cached; \
                 +{})",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_REPOSITORY")
            ),
            last: None,
            requests: 0,
            cached: 0,
        }
    }

    /// Where `name` is cached.
    pub fn path(&self, name: &str) -> PathBuf {
        self.cache.join(name)
    }

    /// Wait out the gap since the last request.
    fn pace(&mut self) {
        if let Some(last) = self.last {
            let since = last.elapsed();
            if since < GAP {
                std::thread::sleep(GAP.saturating_sub(since));
            }
        }
        self.last = Some(Instant::now());
    }

    /// `url`'s answer, cached as `name`: from the cache where it holds it,
    /// else fetched. A 429 or a 5xx (or no answer at all) is asked again
    /// after a pause that doubles each time (Retry-After where Wowhead
    /// gives one); any other status, or a request still failing after
    /// TRIES, is an error that ends the run, every answer so far kept.
    pub fn get(&mut self, url: &str, name: &str) -> Result<Got, String> {
        let file = self.path(name);
        let missing = self.path(&format!("{name}.404"));
        if let Ok(text) = std::fs::read_to_string(&file) {
            self.cached += 1;
            return Ok(Got::Body(text));
        }
        if missing.exists() {
            self.cached += 1;
            return Ok(Got::Missing);
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = crate::atomic::tmp_beside(&file);
        let mut wait = Duration::from_secs(2);
        for attempt in 1..=TRIES {
            self.pace();
            self.requests += 1;
            let out = Command::new("curl")
                .args(["-sS", "-L", "--compressed", "--max-time", "60", "-A"])
                .arg(&self.agent)
                .args(["-w", "%{http_code} %header{retry-after}", "-o"])
                .arg(&tmp)
                .arg(url)
                .output()
                .map_err(|e| format!("curl: {e}"))?;
            let said = String::from_utf8_lossy(&out.stdout).to_string();
            let mut words = said.split_whitespace();
            let status: u16 = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
            let retry_after = words.next().and_then(|w| w.parse::<u64>().ok());
            match status {
                200 => {
                    crate::atomic::rename_into(&tmp, &file)?;
                    let text = std::fs::read_to_string(&file)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    return Ok(Got::Body(text));
                }
                404 => {
                    let _ = std::fs::remove_file(&tmp);
                    std::fs::write(&missing, b"")
                        .map_err(|e| format!("{}: {e}", missing.display()))?;
                    return Ok(Got::Missing);
                }
                0 | 429 | 500..=599 if attempt < TRIES => {
                    let pause = retry_after
                        .map(|s| Duration::from_secs(s.min(300)))
                        .unwrap_or(wait)
                        .max(wait);
                    eprintln!(
                        "  {url}: {} (try {attempt} of {TRIES}), again in {} s",
                        if status == 0 {
                            String::from_utf8_lossy(&out.stderr).trim().to_string()
                        } else {
                            format!("HTTP {status}")
                        },
                        pause.as_secs()
                    );
                    std::thread::sleep(pause);
                    wait = wait.saturating_mul(2);
                }
                _ => {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(format!(
                        "{url}: {}",
                        if status == 0 {
                            String::from_utf8_lossy(&out.stderr).trim().to_string()
                        } else {
                            format!("HTTP {status}")
                        }
                    ));
                }
            }
        }
        let _ = std::fs::remove_file(&tmp);
        Err(format!("{url}: no answer after {TRIES} tries"))
    }
}

// ---- the seed ------------------------------------------------------------------------

/// One zone listing as the run read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneRead {
    pub zone: Zone,
    /// The page's title; NPCs listed, and how many had no classification.
    pub title: String,
    pub listed: usize,
    pub unclassed: usize,
    pub truncated: bool,
    /// Why it gave nothing, where it did not read.
    pub error: Option<String>,
}

/// What a run made: the seed rows and how each was found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Seed {
    pub rows: BTreeMap<u32, Creature>,
    pub zones: Vec<ZoneRead>,
    /// Distinct NPC ids the zone listings name.
    pub zone_ids: usize,
    /// Rows classified by a listing and by a tooltip, and NPCs whose
    /// tooltip carries no rank (no row: Wowhead holds none for them).
    pub by_listing: usize,
    pub by_tooltip: usize,
    pub unranked: usize,
    /// NPC ids no answer classified: a 404, or a tooltip with no name.
    pub failed: Vec<(u32, String)>,
}

/// The seed row for an NPC: never a lieutenant, seen at 0.
fn seed_row(id: u32, class: &str, name: &str) -> Creature {
    Creature {
        id,
        classification: class.to_string(),
        lieutenant: false,
        seen_unix: 0,
        name: name.to_string(),
    }
}

/// Build the seed: every zone in `zones` listed, then a tooltip for each
/// NPC of `store` (and each listed NPC without a classification) that no
/// listing classified. Progress goes to stderr.
pub fn seed(store: &StoreNpcs, zones: &[Zone], fetch: &mut Fetcher) -> Result<Seed, String> {
    let mut out = Seed::default();
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let mut ask: BTreeSet<u32> = BTreeSet::new();
    let mut listed_ids: BTreeSet<u32> = BTreeSet::new();
    for z in zones {
        let mut read = ZoneRead {
            zone: *z,
            title: String::new(),
            listed: 0,
            unclassed: 0,
            truncated: false,
            error: None,
        };
        match fetch.get(&zone_url(z.zone), &format!("zone/{}.html", z.zone))? {
            Got::Missing => read.error = Some("404".into()),
            Got::Body(page) => {
                read.title = page_title(&page).unwrap_or_default().to_string();
                match npcs(&page) {
                    Err(e) => read.error = Some(e),
                    Ok((listed, truncated)) => {
                        read.listed = listed.len();
                        read.truncated = truncated;
                        for l in listed {
                            listed_ids.insert(l.id);
                            match l.class {
                                Some(class) => {
                                    out.rows
                                        .entry(l.id)
                                        .or_insert_with(|| seed_row(l.id, class, &l.name));
                                }
                                None => {
                                    read.unclassed += 1;
                                    ask.insert(l.id);
                                    names.entry(l.id).or_insert(l.name);
                                }
                            }
                        }
                    }
                }
            }
        }
        eprintln!(
            "  zone {} {} (map {}): {} NPCs{}{}",
            z.zone,
            z.name,
            z.map,
            read.listed,
            if read.truncated { ", cut short" } else { "" },
            read.error
                .as_deref()
                .map(|e| format!(", {e}"))
                .unwrap_or_default()
        );
        out.zones.push(read);
    }
    out.zone_ids = listed_ids.len();
    out.by_listing = out.rows.len();
    for (id, name) in &store.ids {
        names.entry(*id).or_insert_with(|| name.clone());
        ask.insert(*id);
    }
    ask.retain(|id| !out.rows.contains_key(id));
    let total = ask.len();
    eprintln!("  {total} NPCs to classify by tooltip");
    for (n, id) in ask.into_iter().enumerate() {
        match fetch.get(&tooltip_url(id), &format!("npc/{id}.json"))? {
            Got::Missing => out.failed.push((id, "404".into())),
            Got::Body(text) => match tooltip(&text) {
                Ok(Tip {
                    name,
                    class: Some(class),
                }) => {
                    out.by_tooltip += 1;
                    out.rows.insert(id, seed_row(id, class, &name));
                }
                Ok(Tip { class: None, .. }) => out.unranked += 1,
                Err(e) => out.failed.push((id, e)),
            },
        }
        if (n + 1) % 100 == 0 {
            eprintln!(
                "  {} of {total} tooltips ({} fetched, {} cached)",
                n + 1,
                fetch.requests,
                fetch.cached
            );
        }
    }
    for (id, why) in &mut out.failed {
        if let Some(name) = names.get(id).filter(|n| !n.is_empty()) {
            *why = format!("{why} ({name})");
        }
    }
    Ok(out)
}

/// What laying the seed under a table did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Laid {
    /// Rows the table lacked.
    pub added: usize,
    /// Seed rows already there (`seen_unix` 0) replaced by this seed's.
    pub refreshed: usize,
    /// Rows the addon saw in game, kept over this seed's.
    pub sighted: usize,
    /// Rows of the table this seed has no row for, kept as they were.
    pub untouched: usize,
}

/// Lay `seed` under `table`: an id the table lacks takes its seed row, a
/// seed row already there (`seen_unix` 0 or less) is replaced, and a row the
/// addon saw (`seen_unix` above 0) stays — `creatures::merge`, after the
/// old seed rows the new seed covers are taken out.
pub fn lay_seed(table: &mut BTreeMap<u32, Creature>, seed: Vec<Creature>) -> Laid {
    let ids: BTreeSet<u32> = seed.iter().map(|c| c.id).collect();
    let mut laid = Laid {
        untouched: table.keys().filter(|id| !ids.contains(id)).count(),
        ..Laid::default()
    };
    let before = table.len();
    table.retain(|id, c| {
        let old_seed = c.seen_unix <= 0 && ids.contains(id);
        if old_seed {
            laid.refreshed += 1;
        }
        !old_seed
    });
    laid.sighted = before - laid.refreshed - laid.untouched;
    let changed = creatures::merge(table, seed);
    laid.added = changed - laid.refreshed;
    laid
}

/// Rows by classification, in `creatures::CLASSIFICATIONS`' order.
pub fn histogram(rows: &BTreeMap<u32, Creature>) -> Vec<(&'static str, usize)> {
    creatures::CLASSIFICATIONS
        .iter()
        .map(|c| (*c, rows.values().filter(|r| r.classification == *c).count()))
        .collect()
}

#[cfg(test)]
mod tests;
