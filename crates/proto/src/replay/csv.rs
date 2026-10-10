//! v45 (R29): a [`Cut`] as the seven files a replay reads — `units.tsv`,
//! `tracks.csv`, `events.csv`, `placed.csv`, `markers.csv`, `raid.csv` and
//! `pull.txt` — column for column as the extractor's pull cutter wrote them,
//! so a pull exported from the history store (`wowdps history
//! replay-export`) and one cut from a log read the same way.
//!
//! Each file's heading names its columns (`TRACKS_HEAD` …). Numbers are
//! written as the log wrote them: a position with two decimals, a facing
//! with three (the log's four, rounded), health as a share with one. A
//! name is quoted only when it holds a comma (the readers take a quoted
//! field whole). Rows go in the cut's order — events, placed rows and
//! markers by time as the log had them — but for `tracks.csv`, which lists
//! every post by time and, within a millisecond, by unit: the cut keeps
//! each unit's track whole, and a reader sorts each unit's track anyway.
//!
//! Beyond the extractor's columns, two additions a reader by column takes
//! or leaves: (format 2) `tracks.csv`'s trailing `map_id`, the floor each
//! post stands on, and `events.csv`'s boss rows (`boss_engaged`,
//! `boss_killed`, `boss_wiped`: the encounter's id and name in `spell_id`
//! and `spell`, `unit` empty).

use std::fmt::Write as _;
use std::io;
use std::path::Path;

use wowdps_model::replay::{Cut, MarkerKind, PlacedKind, UnitKind};

/// Each file's heading line.
pub const UNITS_HEAD: &str = "unit\tkind\tname\tclass\tspec\tspec_name\trole\tyou\tnpc\n";
pub const TRACKS_HEAD: &str = "unit,t_ms,x,y,facing,hp,power_type,power,power_max,map_id\n";
pub const EVENTS_HEAD: &str = "t_ms,kind,unit,spell_id,spell,x,y,src,stacks,base,target\n";
pub const PLACED_HEAD: &str = "t_ms,kind,unit,spell_id,spell,x,y,src,target\n";
pub const RAID_HEAD: &str = "t_ms,damage\n";
pub const MARKERS_HEAD: &str = "t_ms,kind,marker,x,y\n";

/// The files' names, in the order [`write_dir`] writes them.
pub const FILES: [&str; 7] = [
    "units.tsv",
    "tracks.csv",
    "events.csv",
    "placed.csv",
    "raid.csv",
    "markers.csv",
    "pull.txt",
];

/// The difficulty ids `pull.txt`'s `detail` names.
const DIFFICULTY: [(u32, &str); 10] = [
    (1, "Normal"),
    (2, "Heroic"),
    (8, "Mythic+"),
    (14, "Normal"),
    (15, "Heroic"),
    (16, "Mythic"),
    (17, "LFR"),
    (23, "Mythic"),
    (24, "Timewalking"),
    (33, "Timewalking"),
];

/// The difficulty of a keystone.
const KEYSTONE: u32 = 8;

/// A name as a field: quoted only when it holds a comma.
pub fn csv_name(name: &str) -> String {
    let n = name.replace('"', "");
    if n.contains(',') {
        format!("\"{n}\"")
    } else {
        n
    }
}

/// Hundredths of a yard as the log writes them: two decimals.
fn yd(v: i32) -> String {
    format!("{:.2}", f64::from(v) / 100.0)
}

fn opt(u: Option<u32>) -> String {
    u.map_or_else(String::new, |u| u.to_string())
}

/// `units.tsv`: a line per unit in its number's order — a player's class
/// (the model's name for it, `DeathKnight`), spec id, spec name and role,
/// and `you`; an NPC's kind and creature id.
pub fn units_tsv(cut: &Cut) -> String {
    let mut out = String::from(UNITS_HEAD);
    for (u, unit) in cut.units.iter().enumerate() {
        match unit.kind {
            UnitKind::Player => {
                let _ = writeln!(
                    out,
                    "{u}\tplayer\t{}\t{}\t{}\t{}\t{}\t{}\t",
                    unit.name,
                    unit.class.map_or_else(String::new, |c| format!("{c:?}")),
                    unit.spec.map_or_else(String::new, |s| s.id().to_string()),
                    unit.spec.map_or("", |s| s.name()),
                    unit.spec.map_or("", |s| s.role().name()),
                    u8::from(unit.you),
                );
            }
            kind => {
                let npc = if unit.npc == 0 {
                    String::new()
                } else {
                    unit.npc.to_string()
                };
                let _ = writeln!(out, "{u}\t{}\t{}\t\t\t\t\t0\t{npc}", kind.word(), unit.name);
            }
        }
    }
    out
}

/// `tracks.csv`: every post — by time, then unit — with its facing (three
/// decimals), its health share (one), its power (empty where none) and
/// the floor it stands on.
pub fn tracks_csv(cut: &Cut) -> String {
    let mut rows: Vec<(u32, usize, usize)> = cut
        .units
        .iter()
        .enumerate()
        .flat_map(|(u, unit)| {
            unit.posts
                .iter()
                .enumerate()
                .map(move |(i, p)| (p.t_ms, u, i))
        })
        .collect();
    rows.sort_unstable();
    let mut out = String::with_capacity(TRACKS_HEAD.len() + rows.len() * 48);
    out.push_str(TRACKS_HEAD);
    for (_, u, i) in rows {
        let Some(p) = cut.units.get(u).and_then(|unit| unit.posts.get(i)) else {
            continue;
        };
        let power = p.power.map_or_else(
            || ",,".to_string(),
            |w| format!("{},{},{}", w.kind, w.current, w.max),
        );
        let _ = writeln!(
            out,
            "{u},{},{},{},{:.3},{}.{},{power},{}",
            p.t_ms,
            yd(p.x),
            yd(p.y),
            f64::from(p.facing) / 10_000.0,
            p.hp / 10,
            p.hp % 10,
            p.map_id,
        );
    }
    out
}

/// `events.csv`: a line per event, each kind up to its last column
/// (`EventKind::columns`).
pub fn events_csv(cut: &Cut) -> String {
    let mut out = String::from(EVENTS_HEAD);
    for e in &cut.events {
        let (spell_id, spell) = if e.kind.has_spell() {
            (e.spell_id.to_string(), csv_name(&e.spell))
        } else {
            (String::new(), String::new())
        };
        let (x, y) =
            e.at.map_or((String::new(), String::new()), |(x, y)| (yd(x), yd(y)));
        let columns = [
            e.t_ms.to_string(),
            e.kind.word().to_string(),
            opt(e.unit),
            spell_id,
            spell,
            x,
            y,
            opt(e.src),
            e.stacks.map_or_else(String::new, |s| s.to_string()),
            e.base.map_or_else(String::new, |b| b.to_string()),
            opt(e.target),
        ];
        let line: Vec<String> = columns.into_iter().take(e.kind.columns()).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// `placed.csv`: a line per placed row; a `gone` line names its unit alone.
pub fn placed_csv(cut: &Cut) -> String {
    let mut out = String::from(PLACED_HEAD);
    for p in &cut.placed {
        if p.kind == PlacedKind::Gone {
            let _ = writeln!(out, "{},gone,{},,,,,,", p.t_ms, opt(p.unit));
            continue;
        }
        let (x, y) =
            p.at.map_or((String::new(), String::new()), |(x, y)| (yd(x), yd(y)));
        let _ = writeln!(
            out,
            "{},{},{},{},{},{x},{y},{},{}",
            p.t_ms,
            p.kind.word(),
            opt(p.unit),
            p.spell_id,
            csv_name(&p.spell),
            opt(p.src),
            opt(p.target),
        );
    }
    out
}

/// `markers.csv`: those standing at the start (t 0), then each change.
pub fn markers_csv(cut: &Cut) -> String {
    let mut out = String::from(MARKERS_HEAD);
    for m in &cut.markers {
        let (x, y) =
            m.at.map_or((String::new(), String::new()), |(x, y)| (yd(x), yd(y)));
        let kind = match m.kind {
            MarkerKind::Placed => "placed",
            MarkerKind::Removed => "removed",
        };
        let _ = writeln!(out, "{},{kind},{},{x},{y}", m.t_ms, m.marker);
    }
    out
}

/// `raid.csv`: the group's damage in each `bucket_ms` from the start —
/// R25's raid series, which the store keeps on the details tier, never in
/// the cut.
pub fn raid_csv(series: &[u64], bucket_ms: u64) -> String {
    let mut out = String::from(RAID_HEAD);
    for (k, amount) in series.iter().enumerate() {
        let _ = writeln!(out, "{},{amount}", k as u64 * bucket_ms);
    }
    out
}

/// What `pull.txt` takes from beyond the cut: the instance as the journal
/// names it (else the cut's zone), the log it was cut from, its owner and
/// its order among the instance's encounters.
#[derive(Debug, Clone, Copy, Default)]
pub struct PullFacts<'a> {
    pub instance: Option<&'a str>,
    pub log: &'a str,
    pub owner: Option<&'a str>,
    pub order: Option<u32>,
}

/// `pull.txt`: the header's words (`title`, `detail`, `outcome`,
/// `instance`, `kind`, `encounter_id`, `map`, `date`), then the pull's
/// identity (`log`, `start`, `owner`) and its `order`. A keystone run's
/// (a cut with no encounter at its head) says its dungeon and level, timed
/// or over, and names no encounter.
pub fn pull_txt(cut: &Cut, facts: &PullFacts<'_>) -> String {
    let h = &cut.head;
    let players = cut
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Player)
        .count();
    let level = h
        .key
        .as_ref()
        .map_or_else(|| "?".to_string(), |k| k.level.to_string());
    let s = h.fight_ms.unwrap_or(0) / 1000;
    let clock = format!("{}:{:02}", s / 60, s % 60);
    let mut out = String::new();
    match &h.encounter {
        Some(e) => {
            let detail = if e.difficulty == KEYSTONE {
                format!("Mythic+ {level}, {} players", e.size)
            } else {
                let name = DIFFICULTY.iter().find(|d| d.0 == e.difficulty).map_or_else(
                    || format!("Difficulty {}", e.difficulty),
                    |d| d.1.to_string(),
                );
                format!("{name}, {} players", e.size)
            };
            let outcome = if h.success == Some(true) {
                "Kill"
            } else {
                "Wipe"
            };
            let kind = if e.difficulty == KEYSTONE || e.size <= 5 {
                "dungeon"
            } else {
                "raid"
            };
            let _ = write!(
                out,
                "title = {}\ndetail = {detail}\noutcome = {outcome}  {clock}\ninstance = {}\nkind = {kind}\nencounter_id = {}\n",
                e.name,
                facts.instance.unwrap_or(&h.zone),
                e.id,
            );
        }
        None => {
            let title = h
                .key
                .as_ref()
                .map_or_else(|| h.zone.clone(), |k| format!("{} +{}", k.name, k.level));
            let outcome = if h.success == Some(true) {
                "Timed"
            } else {
                "Over"
            };
            let _ = write!(
                out,
                "title = {title}\ndetail = Mythic+ {level}, {players} players\noutcome = {outcome}  {clock}\ninstance = {}\nkind = dungeon\n",
                facts.instance.unwrap_or(&h.zone),
            );
        }
    }
    let (y, m, d) = h.date;
    let _ = write!(
        out,
        "map = {}\ndate = {y:04}-{m:02}-{d:02}\nlog = {}\nstart = {}\n",
        h.map, facts.log, h.start_utc_ms
    );
    if let Some(owner) = facts.owner.filter(|o| !o.is_empty() && *o != "-") {
        let _ = writeln!(out, "owner = {owner}");
    }
    if let Some(order) = facts.order {
        let _ = writeln!(out, "order = {order}");
    }
    out
}

/// The seven files into `dir` (made if missing): the cut's, `raid.csv`
/// from `raid` (a series and its bucket; heading only without one), and
/// `pull.txt` from `facts`.
pub fn write_dir(
    dir: &Path,
    cut: &Cut,
    raid: Option<(&[u64], u64)>,
    facts: &PullFacts<'_>,
) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let raid = raid.map_or_else(|| RAID_HEAD.to_string(), |(s, b)| raid_csv(s, b));
    for (name, text) in [
        ("units.tsv", units_tsv(cut)),
        ("tracks.csv", tracks_csv(cut)),
        ("events.csv", events_csv(cut)),
        ("placed.csv", placed_csv(cut)),
        ("raid.csv", raid),
        ("markers.csv", markers_csv(cut)),
        ("pull.txt", pull_txt(cut, facts)),
    ] {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(())
}
