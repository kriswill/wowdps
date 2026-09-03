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
  wowdps history sql <query> [--json|--objects]   run SQL over the lake views
  wowdps history best-kill <encounter> <difficulty>
  wowdps history progression <encounter> <difficulty>
  wowdps history trend <guid> [--healing] [--limit N]
  wowdps history materialize                     write cache.duckdb beside the lake
  wowdps history import <log|dir>                ask the daemon to import a log
  wowdps history export <fight_id>               one fight as one JSON document
  wowdps history stats
  wowdps history views                            which views this lake defines

Options:
  --dir <path>   the lake ($XDG_DATA_HOME/wowdps/history/v1 by default)

Views: fights (one row per stored fight), players (the cards' player lines,
one row per player per fight), rows (the six views' meter rows + death
recaps), details (breakdowns + timelines), loadouts, annotations.";

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
    let dir = match dir {
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
            Ok(table(Lake::open(&dir)?.sql(query)?))
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
        "materialize" => {
            let path = Lake::open(&dir)?.materialize()?;
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
    Err("the daemon did not answer".to_string())
}
