//! The lake parity gate (spec §12): the daemon's `Fights` / `Progression`
//! / `Trend` answers over the fixture must equal what SQL says over the
//! files the same run wrote. Two readers of one lake, kept honest.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::tail::SourceSpec;
use wowdps_daemon::history::HistoryOptions;
use wowdps_daemon::{DaemonOptions, run};
use wowdps_history::Lake;
use wowdps_proto::json::Json;
use wowdps_proto::{
    ClientKind, ClientMsg, DaemonClient, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery,
    TrendBucket,
};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(20);

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("wowdps-lake-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start(tmp: &Temp) -> (PathBuf, PathBuf, mpsc::Receiver<std::io::Result<()>>) {
    let socket = tmp.0.join("test.sock");
    let hist = tmp.0.join("history");
    let opts = DaemonOptions {
        socket: socket.clone(),
        lockfile: tmp.0.join("test.lock"),
        source: SourceSpec::File(PathBuf::from(FIXTURE)),
        linger: true,
        idle_grace: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        version: "test".to_string(),
        cache_dir: None,
        game_pattern: None,
        loader_workers: 2,
        auto_overlay: false,
        overlay_exit_grace: Duration::ZERO,
        gui_bin: None,
        history: Some(HistoryOptions {
            dir: hist.clone(),
            store_trash: false,
            keep_per_encounter: 200,
            keep_details_per_encounter: 10,
            characters: Vec::new(),
            cache_dir: None,
        }),
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(opts));
    });
    let deadline = Instant::now() + DEADLINE;
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon never bound");
        thread::sleep(Duration::from_millis(5));
    }
    (socket, hist, rx)
}

fn ask(client: &mut DaemonClient, req_id: u32, query: HistoryQuery) -> HistoryAnswer {
    client.send(&ClientMsg::GetHistory { req_id, query });
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::History {
                req_id: got,
                answer,
            } = msg
                && got == req_id
            {
                return answer;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("no answer to {req_id}");
}

fn wait_for_store(client: &mut DaemonClient, fights: u32) {
    let deadline = Instant::now() + DEADLINE;
    let mut req_id = 1000;
    while Instant::now() < deadline {
        client.send(&ClientMsg::GetStatus { req_id });
        req_id += 1;
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            for msg in client.poll() {
                if let DaemonMsg::Status { history, .. } = msg
                    && history.fights == fights
                    && history.importing == 0
                {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    panic!("store never reached {fights} fights");
}

fn cell_str(v: &Json) -> String {
    match v {
        Json::Str(s) => s.clone(),
        other => other.to_line(),
    }
}

#[test]
fn the_daemon_and_sql_agree_over_the_same_lake() {
    let tmp = Temp::new("parity");
    let (socket, hist, _done) = start(&tmp);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    wait_for_store(&mut client, 2);

    let lake = Lake::open(&hist).expect("lake opens");
    assert!(lake.views().contains(&"fights"));
    assert!(lake.views().contains(&"players"));
    assert!(lake.views().contains(&"rows"));
    assert!(lake.views().contains(&"details"));
    assert!(lake.views().contains(&"loadouts"));
    assert!(!lake.views().contains(&"annotations"), "none written yet");

    // Fights, newest first.
    let HistoryAnswer::Fights { cards, .. } = ask(
        &mut client,
        1,
        HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Newest,
            limit: 0,
            after_id: None,
        },
    ) else {
        panic!("fights");
    };
    let sql = lake
        .sql("SELECT id, name, duration_ms, success FROM fights ORDER BY start_utc_ms DESC")
        .unwrap();
    assert_eq!(sql.columns, ["id", "name", "duration_ms", "success"]);
    assert_eq!(sql.rows.len(), cards.len());
    for (row, card) in sql.rows.iter().zip(&cards) {
        assert_eq!(cell_str(&row[0]), card.id);
        assert_eq!(cell_str(&row[1]), card.name);
        assert_eq!(row[2].as_i64(), Some(card.duration_ms));
        assert_eq!(row[3].as_bool(), card.success);
    }

    // Best kill.
    let HistoryAnswer::Fights { cards: best, .. } = ask(
        &mut client,
        2,
        HistoryQuery::Fights {
            encounter: Some(3130),
            difficulty: Some(15),
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Fastest,
            limit: 1,
            after_id: None,
        },
    ) else {
        panic!("best");
    };
    let sql = lake.best_kill(3130, 15).unwrap();
    assert_eq!(sql.rows.len(), 1);
    assert_eq!(cell_str(&sql.rows[0][0]), best[0].id);

    // Progression: pulls / kills per night.
    let HistoryAnswer::Progression {
        pulls,
        kills,
        nights,
        ..
    } = ask(
        &mut client,
        3,
        HistoryQuery::Progression {
            encounter: 3130,
            difficulty: 15,
            local_cutover_hour: None,
        },
    )
    else {
        panic!("progression");
    };
    let sql = lake.progression(3130, 15).unwrap();
    assert_eq!(sql.rows.len(), nights.len());
    let sql_pulls: i64 = sql.rows.iter().map(|r| r[1].as_i64().unwrap()).sum();
    let sql_kills = sql
        .rows
        .iter()
        .filter(|r| r[2].as_bool() == Some(true))
        .count();
    assert_eq!(sql_pulls, i64::from(pulls));
    assert_eq!(sql_kills as u32, kills.min(1) * nights.len() as u32);
    for (row, night) in sql.rows.iter().zip(&nights) {
        assert_eq!(row[0].as_i64(), Some(night.day_utc_ms));
        assert_eq!(row[1].as_i64(), Some(i64::from(night.pulls)));
        assert_eq!(row[2].as_bool(), Some(night.kill));
    }

    // Trend for the first player, per fight.
    let guid = cards[0].players[0].guid.clone();
    let HistoryAnswer::Trend(points) = ask(
        &mut client,
        4,
        HistoryQuery::Trend {
            guid: guid.clone(),
            spec: None,
            encounter: None,
            difficulty: None,
            view: wowdps_model::View::Damage,
            bucket: TrendBucket::None,
            since_utc_ms: None,
            limit: 0,
            local_cutover_hour: None,
        },
    ) else {
        panic!("trend");
    };
    let sql = lake.trend(&guid, false, 50).unwrap();
    assert_eq!(sql.rows.len(), points.len());
    for (row, p) in sql.rows.iter().zip(&points) {
        assert_eq!(cell_str(&row[0]), p.fight_id);
        assert_eq!(row[1].as_i64(), Some(p.bucket_utc_ms));
        let per_sec = row[3].as_f64().unwrap();
        assert!(
            (per_sec - p.per_sec).abs() < 1e-6,
            "{per_sec} vs {}",
            p.per_sec
        );
    }

    // The players view unnests the cards' player lines: 3 per boss.
    let sql = lake
        .sql("SELECT count(*) AS n, count(DISTINCT guid) AS players FROM players")
        .unwrap();
    assert_eq!(sql.rows[0][0].as_i64(), Some(6));
    assert_eq!(sql.rows[0][1].as_i64(), Some(3));
    // Bound parameters: a string literal that never crosses a quoting
    // layer, and numbers that stay numbers.
    let bound = lake
        .sql_with(
            "SELECT count(*) AS n FROM players WHERE name LIKE ? AND difficulty = ?",
            &[Json::str("Thraxx%"), Json::num(15.0)],
        )
        .unwrap();
    assert_eq!(bound.rows[0][0].as_i64(), Some(2), "{bound:?}");
    let err = lake
        .sql_with("SELECT ?", &[Json::Arr(Vec::new())])
        .unwrap_err();
    assert!(err.contains("not a scalar"), "{err}");

    // Export is the three documents in one; materialize writes the cache.
    let doc = lake.export(&cards[0].id).unwrap();
    assert_eq!(
        doc.get("fight")
            .and_then(|f| f.get("id"))
            .and_then(Json::as_str),
        Some(cards[0].id.as_str())
    );
    assert!(doc.get("rows").is_some_and(|r| *r != Json::Null));
    // The reading lake can touch no file: not ATTACH, not COPY out, not
    // read_text in — `history_sql` runs an LLM's query verbatim.
    assert!(
        lake.materialize().is_err(),
        "a read-only lake cannot ATTACH"
    );
    let probe = tmp.0.join("probe.csv");
    let copy_out = format!(
        "COPY (SELECT 1) TO '{}'",
        probe.display().to_string().replace('\'', "''")
    );
    assert!(lake.sql(&copy_out).is_err());
    assert!(
        !probe.exists(),
        "COPY wrote a file through the read-only lake"
    );
    assert!(
        lake.sql("SELECT length(content) FROM read_text('/etc/hostname')")
            .is_err()
    );
    let cache = Lake::open_writable(&hist).unwrap().materialize().unwrap();
    assert!(cache.exists());
    let cached = Lake::open(&hist).unwrap();
    let n = cached.sql("SELECT count(*) AS n FROM fights").unwrap();
    assert_eq!(n.rows[0][0].as_i64(), Some(2));

    // Shut the daemon down.
    client.send(&ClientMsg::Shutdown);
}

#[test]
fn an_empty_lake_opens_with_no_views_and_says_so() {
    let tmp = Temp::new("empty");
    let lake = Lake::open(&tmp.0).unwrap();
    assert!(lake.views().is_empty());
    assert!(lake.sql("SELECT 1 AS one").is_ok());
    assert!(lake.sql("SELECT * FROM fights").is_err());
    let stats = lake.stats();
    assert_eq!(
        stats
            .get("directories")
            .and_then(|d| d.get("fights"))
            .and_then(|f| f.get("files"))
            .and_then(Json::as_u64),
        Some(0)
    );
}

#[test]
fn network_access_is_off() {
    let tmp = Temp::new("offline");
    let lake = Lake::open(&tmp.0).unwrap();
    // Extensions can neither be installed (the repository is a path that
    // does not exist — never the network) nor loaded (the extension
    // directory is the lake's own, empty), and the settings are locked.
    assert!(lake.sql("INSTALL spatial").is_err());
    assert!(lake.sql("LOAD httpfs").is_err());
    assert!(lake.sql("SET autoinstall_known_extensions = true").is_err());
    assert!(
        lake.sql("SET custom_extension_repository = 'http://x'")
            .is_err()
    );
    // The statically linked extensions are all a lake ever needs.
    let t = lake
        .sql(
            "SELECT extension_name FROM duckdb_extensions() WHERE loaded AND \
             extension_name IN ('json', 'parquet', 'icu') ORDER BY 1",
        )
        .unwrap();
    assert_eq!(t.rows.len(), 3, "{t:?}");
}
