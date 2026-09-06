//! `wowdps-history` — ad hoc analytics over the history store's lake, reached
//! as `wowdps history …`. Everything but `import` reads files; `import` is a
//! thin client of the daemon's `ImportLog` (the daemon stays the only writer).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use wowdps_history::{Lake, Table, default_dir};
use wowdps_proto::json::Json;
use wowdps_proto::{ClientKind, ClientMsg, DaemonClient, DaemonMsg, HistoryAnswer};

const USAGE: &str = "\
wowdps-history - SQL over the history store (DuckDB), normally run as `wowdps history`

Usage:
  wowdps history sql <query> [--params <json array>] [--json|--objects]
                                                 run SQL over the lake views (? placeholders)
  wowdps history best-kill <encounter> <difficulty>
  wowdps history progression <encounter> <difficulty>
  wowdps history trend <guid> [--healing] [--limit N]
  wowdps history role-night <encounter> <difficulty> <day_utc_ms>
                                                 one UTC night of one boss as a role roster
                                                 (the day_utc_ms `progression` lists)
  wowdps history materialize                     write cache.duckdb beside the lake
  wowdps history import <log|dir>                ask the daemon to import a log
  wowdps history regrade <fight_id | --encounter N [--difficulty D] | --kind K>
                                                 rewrite stored cards from their logs
                                                 (pins + annotations kept; before/after)
  wowdps history export <fight_id>               one fight as one JSON document
  wowdps history stats
  wowdps history views                            which views this lake defines

Options:
  --dir <path>   the lake (config `history_dir`, else $XDG_DATA_HOME/wowdps/history/v1)

Views: fights (one row per stored fight), players (the cards' player lines,
one row per player per fight, `role` and the R19 `support` flag from the
spec — no card stores either), role_ranks (the daemon's grader: friendly
dps ranked by effective_dps_sql among dps, healers by hps among healers,
floors applied, `excluded` per fight + role; tanks unranked), rows (the
seven views' meter rows + death recaps), details (breakdowns +
timelines), loadouts, annotations.

`players` also carries the R17 tank measures taken / mitigated / prevented
/ dtps / mitigated_pct, with mitigated_pct_sql the same number recomputed
in SQL — on a lake whose cards predate them only the two pct columns
exist, both 0 — and R19's healing split and support scalars overheal /
absorbed / support_given / support_received / healed_received /
self_healed with the stored effective_dps beside effective_dps_sql =
greatest(0, damage − support_received + support_given) per second, which
reads as dps on a card that predates the scalars (their columns exist
only once one card carries them; effective_dps_sql always does), and
R18's span scalars am_uptime_ms / externals_given / externals_given_ms /
externals_received / externals_received_ms with the stored am_uptime_pct
beside am_uptime_pct_sql = am_uptime_ms × 100 / duration_ms — on a lake
whose cards predate them only the two pct columns exist, both 0
(`stats.cards_without_am_uptime` counts the cards `regrade` would fill —
ones with a SPECCED player lacking am_uptime_ms; a card with no specced
player at all counts in neither that nor its complement, so on a real lake
`cards_without_am_uptime` < the fight count is not corruption).

R17 (only on a lake whose rows files carry them — `regrade` fills an older
one; `views` says which exist): taken (the Taken meter rows, one per player
per fight — an arena's enemy players included, flagged `enemy`), mitigation
(per friendly player: the record's seven amounts, the ten miss kinds as
columns + their total, the two capped lists' `other_*` and
`other_sources_*` rollups, taken, mitigated and mitigated_pct),
taken_spells and taken_sources (the by-ability and by-attacker Taken
drills, each at most 16 rows per player; Σ rows + the rollup = taken).

R19 (likewise, and only once some fight had a supporter): support (per
fight × supporter: given_damage / given_healing / received_damage /
received_healing), support_targets (the supporter's per-target table:
target = the buffed player's guid, name, damage, healing, lines, class,
spec; Σ damage over a supporter's targets = their given_damage).

R18 (likewise, and only once some fight had a role aura): uptime (the
aura-uptime rollup, one row per fight × TARGET × (spell, caster): guid is
the buffed player, src the caster, kind the name — external,
active_mitigation, defensive, support_buff, cooldown — count, total_ms;
Σ total_ms where kind = 'external' grouped by src = the caster's card
externals_given_ms — the rollup is friendly-only and so are the card's
five span scalars: an arena's enemy players store zeros, so the identity
holds on an arena lake too), coarse (per friendly player: taken10 /
heal10, the 10 s taken and healing series as BIGINT lists, and marks,
the drill's mark list with kind as the code — unnest per query;
Σ taken10 = the player's Taken row).

R20 (step 5): `players` carries the shield ledger's card scalars
absorb_wasted (BIGINT, NULL when the player closed no shield with a known
waste — and NULL on a card that predates the ledger: the honest value,
never 0) and shields_unknown (shields whose applied size was never seen),
with the stored absorb_efficiency beside absorb_efficiency_sql = absorbed
/ (absorbed + absorb_wasted), NULL when the waste is unknown or the sum is
0 — on a lake whose cards predate them the three read NULL / 0 / NULL
(`stats.cards_without_shields` counts the cards `regrade` would fill —
ones with a SPECCED player lacking the shields_unknown KEY, a null
absorb_wasted being a stored answer). shields (only once some fight had a
shield; `stats.rows_without_shields` counts the files lacking the key):
the ledger per fight × ABSORBER × spell — guid is the caster, then
spell_id, label, applied, consumed, wasted, count, unknown; Σ consumed per
(fight, guid) = the card's absorbed, and applied = consumed + wasted on
every row with unknown 0. role-night is the daemon's fixed question in
SQL: per friendly player of the night's non-aborted pulls, the most-played
spec and its role, pulls, the MEAN per-pull role measure (effective dps /
hps / mitigated_pct by role) and its best, Σ taken, mean dtps, mean
am_uptime_pct, mean overheal %, absorb_efficiency as a ratio of sums over
the pulls with a known waste, Σ externals_given — tanks, healers, dps,
then measure desc. Recipes: docs/history-queries.md.";

fn main() {
    let code = match run(std::env::args().skip(1).collect()) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(e) => {
            eprintln!("wowdps-history: {e}");
            2
        }
    };
    // The one place a non-zero exit is the contract (the workspace bans
    // `exit` elsewhere).
    #[allow(clippy::exit)]
    std::process::exit(code);
}

fn run(args: Vec<String>) -> Result<String, String> {
    let mut dir: Option<PathBuf> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = Some(PathBuf::from(it.next().ok_or("--dir needs a path")?)),
            "-h" | "--help" | "help" => return Ok(format!("{USAGE}\n")),
            _ => rest.push(a),
        }
    }
    // The same resolution as the daemon's: `--dir`, else the config's
    // `history_dir`, else the XDG default — so SQL always reads the lake
    // the daemon writes. The one key is read here rather than through the
    // daemon crate: CONTRACT.md fixes this binary at model + proto + duckdb.
    let dir = match dir.or_else(configured_history_dir) {
        Some(d) => d,
        None => default_dir().ok_or("no data dir: set XDG_DATA_HOME or HOME")?,
    };
    let Some(cmd) = rest.first().map(String::as_str) else {
        return Ok(format!("{USAGE}\n"));
    };
    let arg = |i: usize| rest.get(i).map(String::as_str);
    let flag = |f: &str| rest.iter().any(|a| a == f);
    let after = |f: &str| {
        rest.iter()
            .position(|a| a == f)
            .and_then(|i| rest.get(i + 1))
            .map(String::as_str)
    };
    let table = |t: Table| -> String {
        if flag("--objects") {
            format!("{}\n", t.to_objects().to_line())
        } else if flag("--json") {
            format!("{}\n", t.to_json().to_line())
        } else {
            t.to_text()
        }
    };
    match cmd {
        "sql" => {
            let query = arg(1).ok_or("sql needs a query")?;
            let params: Vec<Json> = match after("--params") {
                Some(text) => match wowdps_proto::json::parse(text) {
                    Ok(Json::Arr(p)) => p,
                    Ok(_) => return Err("--params must be a JSON array".to_string()),
                    Err(e) => return Err(format!("--params: {e}")),
                },
                None => Vec::new(),
            };
            Ok(table(Lake::open(&dir)?.sql_with(query, &params)?))
        }
        "best-kill" | "progression" => {
            let encounter: u32 = arg(1)
                .and_then(|s| s.parse().ok())
                .ok_or(format!("{cmd} needs <encounter> <difficulty>"))?;
            let difficulty: u32 = arg(2)
                .and_then(|s| s.parse().ok())
                .ok_or(format!("{cmd} needs <encounter> <difficulty>"))?;
            let lake = Lake::open(&dir)?;
            let t = if cmd == "best-kill" {
                lake.best_kill(encounter, difficulty)?
            } else {
                lake.progression(encounter, difficulty)?
            };
            Ok(table(t))
        }
        "trend" => {
            let guid = arg(1).ok_or("trend needs a player guid")?;
            let limit: u32 = after("--limit").and_then(|s| s.parse().ok()).unwrap_or(50);
            Ok(table(Lake::open(&dir)?.trend(
                guid,
                flag("--healing"),
                limit,
            )?))
        }
        "role-night" => {
            let need = "role-night needs <encounter> <difficulty> <day_utc_ms>";
            let encounter: u32 = arg(1).and_then(|s| s.parse().ok()).ok_or(need)?;
            let difficulty: u32 = arg(2).and_then(|s| s.parse().ok()).ok_or(need)?;
            let night: i64 = arg(3).and_then(|s| s.parse().ok()).ok_or(need)?;
            let rows = Lake::open(&dir)?.role_night(encounter, difficulty, night)?;
            let columns = [
                "guid",
                "name",
                "spec",
                "role",
                "pulls",
                "measure",
                "best",
                "taken",
                "dtps",
                "am_uptime_pct",
                "overheal_pct",
                "absorb_efficiency",
                "externals_given",
            ];
            let opt = |v: Option<f64>| v.map_or(Json::Null, Json::num);
            Ok(table(Table {
                columns: columns.iter().map(|c| c.to_string()).collect(),
                rows: rows
                    .iter()
                    .map(|r| {
                        vec![
                            Json::str(&*r.guid),
                            Json::str(&*r.name),
                            r.spec.map_or(Json::Null, Json::num),
                            r.role.map_or(Json::Null, |x| Json::str(x.name())),
                            Json::num(r.pulls),
                            Json::num(r.measure),
                            Json::num(r.best),
                            Json::u64(r.taken),
                            Json::num(r.dtps),
                            Json::num(r.am_uptime_pct),
                            Json::num(r.overheal_pct),
                            opt(r.absorb_efficiency),
                            Json::num(r.externals_given),
                        ]
                    })
                    .collect(),
            }))
        }
        "materialize" => {
            let path = Lake::open_writable(&dir)?.materialize()?;
            Ok(format!("{}\n", path.display()))
        }
        "export" => {
            let id = arg(1).ok_or("export needs a fight id")?;
            Ok(format!("{}\n", Lake::open(&dir)?.export(id)?.to_line()))
        }
        "stats" => Ok(format!("{}\n", Lake::open(&dir)?.stats().to_line())),
        "views" => {
            let lake = Lake::open(&dir)?;
            Ok(format!(
                "{}\n",
                Json::Arr(lake.views().iter().map(|v| Json::str(*v)).collect()).to_line()
            ))
        }
        "import" => {
            let path = arg(1).ok_or("import needs a log file or directory")?;
            let path = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
            import(&path)
        }
        "regrade" => {
            let fight_id = arg(1).filter(|a| !a.starts_with("--")).map(str::to_string);
            let encounter: Option<u32> = after("--encounter").and_then(|s| s.parse().ok());
            let difficulty: Option<u32> = after("--difficulty").and_then(|s| s.parse().ok());
            let kind = match after("--kind") {
                None => None,
                Some(k) => Some(
                    wowdps_proto::history::FightKind::parse(&k.to_lowercase())
                        .ok_or_else(|| format!("unknown kind {k:?}"))?,
                ),
            };
            if fight_id.is_none() && encounter.is_none() && kind.is_none() {
                return Err("regrade needs a fight id, --encounter N or --kind K".to_string());
            }
            regrade(&dir, fight_id, encounter, difficulty, kind)
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

/// Ask the daemon (spawned on demand, like every client) to sweep `path`.
fn import(path: &std::path::Path) -> Result<String, String> {
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("wowdps")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("wowdps"));
    let mut client = DaemonClient::connect(&bin, None, ClientKind::Mcp)
        .map_err(|e| format!("cannot reach or spawn the daemon: {e}"))?;
    client.send(&ClientMsg::ImportLog {
        req_id: 1,
        path: path.display().to_string(),
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::History {
                req_id: 1,
                answer: HistoryAnswer::Imported { queued },
            } = msg
            {
                return Ok(format!(
                    "{}\n",
                    Json::Obj(vec![
                        ("path".to_string(), Json::str(path.display().to_string())),
                        ("queued".to_string(), Json::u64(u64::from(queued))),
                    ])
                    .to_line()
                ));
            }
        }
        if client.is_dead() {
            return Err("the daemon went away".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err("the daemon did not answer in time; the import may still be running — watch `wowdps status`".to_string())
}

/// Ask the daemon to rewrite the selected cards from their logs, wait for
/// the queue to drain, and show what changed — before/after per card.
fn regrade(
    dir: &std::path::Path,
    fight_id: Option<String>,
    encounter: Option<u32>,
    difficulty: Option<u32>,
    kind: Option<wowdps_proto::history::FightKind>,
) -> Result<String, String> {
    let selection = match &fight_id {
        Some(id) => format!("id = '{}'", id.replace('\'', "''")),
        None => {
            let mut clauses: Vec<String> = Vec::new();
            if let Some(e) = encounter {
                clauses.push(format!("encounter.id = {e}"));
            }
            if let Some(d) = difficulty {
                clauses.push(format!("encounter.difficulty = {d}"));
            }
            if let Some(k) = kind {
                clauses.push(format!("kind = '{}'", k.as_str()));
            }
            if clauses.is_empty() {
                return Err("regrade needs a fight id, --encounter N or --kind K".to_string());
            }
            clauses.join(" AND ")
        }
    };
    let snapshot = |lake: &Lake| -> Result<Vec<(String, String, String, String)>, String> {
        let t = lake.sql(&format!(
            "SELECT id, name, coalesce(cast(best_pct AS VARCHAR), '-'), \
             coalesce(cast(success AS VARCHAR), '-') FROM fights WHERE {selection} ORDER BY start_utc_ms"
        ))?;
        Ok(t.rows
            .iter()
            .map(|r| {
                let s = |i: usize| match r.get(i) {
                    Some(Json::Str(s)) => s.clone(),
                    Some(other) => other.to_line(),
                    None => String::new(),
                };
                (s(0), s(1), s(2), s(3))
            })
            .collect())
    };
    let before = snapshot(&Lake::open(dir)?)?;
    if before.is_empty() {
        return Err("no stored fight matches".to_string());
    }
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("wowdps")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("wowdps"));
    let mut client = DaemonClient::connect(&bin, None, ClientKind::Mcp)
        .map_err(|e| format!("cannot reach or spawn the daemon: {e}"))?;
    client.send(&ClientMsg::Regrade {
        req_id: 1,
        fight_id,
        encounter,
        difficulty,
        kind,
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut queued = None;
    while queued.is_none() && Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::History {
                answer: HistoryAnswer::Regraded { queued: n },
                ..
            } = msg
            {
                queued = Some(n);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let queued = queued.ok_or("the daemon did not answer the regrade")?;
    // Wait for the import queue to drain (the rewrites ride on it).
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut req_id = 2;
    'wait: while Instant::now() < deadline {
        client.send(&ClientMsg::GetStatus { req_id });
        req_id += 1;
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            for msg in client.poll() {
                if let DaemonMsg::Status { history, .. } = msg
                    && history.importing == 0
                {
                    break 'wait;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let after = snapshot(&Lake::open(dir)?)?;
    let mut out = format!(
        "queued {queued} of {} card(s)\nid\tname\tbest_pct\tsuccess\n",
        before.len()
    );
    for b in &before {
        let a = after.iter().find(|a| a.0 == b.0);
        let (pct, ok) = match a {
            Some(a) => (
                if a.2 == b.2 {
                    a.2.clone()
                } else {
                    format!("{} -> {}", b.2, a.2)
                },
                if a.3 == b.3 {
                    a.3.clone()
                } else {
                    format!("{} -> {}", b.3, a.3)
                },
            ),
            None => ("gone".to_string(), "gone".to_string()),
        };
        out.push_str(&format!("{}\t{}\t{}\t{}\n", b.0, b.1, pct, ok));
    }
    Ok(out)
}

/// `history_dir = "…"` from `~/.config/wowdps/config.toml` (the daemon's
/// path resolution: `$XDG_CONFIG_HOME`, else `$HOME/.config`), read with
/// the same tolerance as the daemon's subset parser: the last `key = value`
/// line wins, quoted or bare, `#` comments and section headers skipped.
fn configured_history_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let text = std::fs::read_to_string(base.join("wowdps/config.toml")).ok()?;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "history_dir" {
            continue;
        }
        let value = value.split('#').next().unwrap_or("").trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        if !value.is_empty() {
            found = Some(PathBuf::from(value));
        }
    }
    found
}
