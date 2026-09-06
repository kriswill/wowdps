//! The lake parity gate (spec §12): the daemon's `Fights` / `Progression`
//! / `Trend` answers over the fixture must equal what SQL says over the
//! files the same run wrote. Two readers of one lake, kept honest.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::tail::SourceSpec;
use wowdps_daemon::history::HistoryOptions;
use wowdps_daemon::{DaemonOptions, run};
use wowdps_history::Lake;
use wowdps_mcp::grade::grade;
use wowdps_model::{Role, Spec};
use wowdps_proto::history::{CardPlayer, FightCard, FightKind};
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

/// The lake's `role_ranks` view must be `wowdps_mcp::grade` over `cards`
/// (roadmap item 1a, step 1): every friendly DPS / healer either has a row
/// with the grader's rank, count, median and measure, or — when the floors
/// dropped them — no row and a place in the role's `excluded`; tanks and
/// unknown specs never appear. And `players.role` is `CardPlayer::role`.
fn assert_ranks_match_grader(lake: &Lake, cards: &[FightCard]) {
    let ranks = lake
        .sql(
            "SELECT fight_id, guid, role, rank_measure, rank, count, median, excluded \
             FROM role_ranks",
        )
        .unwrap();
    let rows: HashMap<(String, String), &Vec<Json>> = ranks
        .rows
        .iter()
        .map(|r| ((cell_str(&r[0]), cell_str(&r[1])), r))
        .collect();
    assert_eq!(rows.len(), ranks.rows.len(), "one row per fight + guid");
    let roles = lake
        .sql("SELECT fight_id, guid, role FROM players")
        .unwrap();
    let stored: HashMap<(String, String), Option<String>> = roles
        .rows
        .iter()
        .map(|r| {
            (
                (cell_str(&r[0]), cell_str(&r[1])),
                r[2].as_str().map(String::from),
            )
        })
        .collect();
    let mut ranked = 0;
    for card in cards {
        for p in &card.players {
            let key = (card.id.clone(), p.guid.clone());
            assert_eq!(
                stored.get(&key).cloned().flatten().as_deref(),
                p.role().map(Role::name),
                "players.role for {key:?}"
            );
            let g = grade(card, &p.guid).expect("on card");
            let row = rows.get(&key);
            if p.enemy {
                // Enemies never enter a pool, on either side.
                assert!(
                    g.rank.is_none(),
                    "{key:?} is an enemy yet the daemon ranks it"
                );
                assert!(row.is_none(), "{key:?} is an enemy yet SQL ranks it");
                continue;
            }
            if g.measure.is_none() {
                assert!(row.is_none(), "{key:?} has no measure yet ranks");
                continue;
            }
            let measure = g.measure.unwrap();
            match g.rank {
                Some(rank) => {
                    let row = row.unwrap_or_else(|| panic!("{key:?} ranked by the daemon only"));
                    ranked += 1;
                    assert_eq!(cell_str(&row[3]), measure.name(), "{key:?} measure");
                    assert_eq!(row[4].as_u64(), Some(rank as u64), "{key:?} rank");
                    assert_eq!(row[5].as_u64(), Some(g.count as u64), "{key:?} count");
                    let median = row[6].as_f64().expect("median");
                    assert!(
                        (median - g.median.expect("median")).abs() < 1e-6,
                        "{key:?} median {median} vs {:?}",
                        g.median
                    );
                    assert_eq!(row[7].as_u64(), Some(g.excluded as u64), "{key:?} excluded");
                }
                None => {
                    assert!(
                        row.is_none(),
                        "{key:?} excluded by the daemon but ranked in SQL"
                    );
                    // The role's ranked rows carry the count of the dropped.
                    let role = p.role().unwrap().name();
                    let peer = ranks
                        .rows
                        .iter()
                        .find(|r| cell_str(&r[0]) == card.id && cell_str(&r[2]) == role)
                        .unwrap_or_else(|| panic!("{key:?}: no ranked peer at all"));
                    assert_eq!(peer[5].as_u64(), Some(g.count as u64), "{key:?} count");
                    assert_eq!(
                        peer[7].as_u64(),
                        Some(g.excluded as u64),
                        "{key:?} excluded"
                    );
                    assert!(g.excluded >= 1);
                }
            }
        }
    }
    assert_eq!(
        ranked,
        ranks.rows.len(),
        "SQL ranks nobody the daemon does not"
    );
}

/// The cards the daemon stored, read back from the lake's files.
fn stored_cards(dir: &Path) -> Vec<FightCard> {
    let mut cards: Vec<FightCard> = std::fs::read_dir(dir.join("fights"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let text = std::fs::read_to_string(e.path()).unwrap();
            FightCard::from_json(&wowdps_proto::json::parse(&text).unwrap()).expect("card")
        })
        .collect();
    cards.sort_by(|a, b| a.id.cmp(&b.id));
    cards
}

/// A copy of `src`'s cards under `dst/fights` with `"role"` stripped from
/// every player of the cards `strip` picks — what PR #12 wrote.
fn copy_lake_stripping_role(src: &Path, dst: &Path, strip: impl Fn(usize) -> bool) {
    std::fs::create_dir_all(dst.join("fights")).unwrap();
    let mut names: Vec<_> = std::fs::read_dir(src.join("fights"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    names.sort();
    for (i, name) in names.iter().enumerate() {
        let mut text = std::fs::read_to_string(src.join("fights").join(name)).unwrap();
        if strip(i) {
            text = strip_role(&text);
        }
        std::fs::write(dst.join("fights").join(name), text).unwrap();
    }
}

/// A card's JSON with every `"role": …,` pair removed — the PR #12 shape.
fn strip_role(card: &str) -> String {
    let mut text = card.to_string();
    while let Some(at) = text.find("\"role\":") {
        let (_, rest) = text.split_at(at);
        let end = at + rest.find(',').expect("role is never last") + 1;
        text.replace_range(at..end, "");
    }
    assert!(text.len() < card.len(), "the card carried no role to strip");
    text
}

/// A lake of exactly these card texts under `fights/`.
fn lake_of(tag: &str, cards: &[(&str, String)]) -> (Temp, Lake) {
    let tmp = Temp::new(tag);
    std::fs::create_dir_all(tmp.0.join("fights")).unwrap();
    for (id, text) in cards {
        std::fs::write(tmp.0.join("fights").join(format!("{id}.json")), text).unwrap();
    }
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(lake.views(), ["fights", "players", "role_ranks"], "{tag}");
    (tmp, lake)
}

fn sorted(mut t: wowdps_history::Table) -> wowdps_history::Table {
    t.rows.sort_by(|a, b| {
        Json::Arr(a.clone())
            .to_line()
            .cmp(&Json::Arr(b.clone()).to_line())
    });
    t
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

    // Roles (roadmap item 1a, step 1): `players.role` is the card's, and
    // `role_ranks` is the daemon's grader — the Discipline priest ranks 1
    // of 1 healer by hps, the two DPS among themselves by dps.
    assert!(lake.views().contains(&"role_ranks"));
    assert_eq!(stored_cards(&hist).len(), cards.len());
    assert_ranks_match_grader(&lake, &cards);
    let healer = lake
        .sql("SELECT rank_measure, rank, count FROM role_ranks WHERE role = 'healer'")
        .unwrap();
    assert_eq!(healer.rows.len(), 2, "one healer per boss: {healer:?}");
    for r in &healer.rows {
        assert_eq!(cell_str(&r[0]), "hps");
        assert_eq!(r[1].as_u64(), Some(1));
        assert_eq!(r[2].as_u64(), Some(1));
    }
    assert_eq!(
        lake.stats()
            .get("cards_without_role")
            .and_then(Json::as_u64),
        Some(0),
        "every card the daemon writes carries role"
    );
    // The CASE fallback: a lake whose cards predate `role` (all of them,
    // and just one — `union_by_name` shapes the struct differently in each
    // case) answers `players.role` and `role_ranks` identically.
    let players = sorted(lake.sql("SELECT * FROM players").unwrap());
    let ranks = sorted(lake.sql("SELECT * FROM role_ranks").unwrap());
    for (tag, strip, without) in [
        ("stripped-all", (|_| true) as fn(usize) -> bool, 2),
        ("stripped-one", |i| i == 0, 1),
    ] {
        let copy = Temp::new(tag);
        copy_lake_stripping_role(&hist, &copy.0, strip);
        let old = Lake::open(&copy.0).unwrap();
        assert_eq!(old.views(), ["fights", "players", "role_ranks"]);
        assert_eq!(
            sorted(old.sql("SELECT * FROM players").unwrap()),
            players,
            "{tag}"
        );
        assert_eq!(
            sorted(old.sql("SELECT * FROM role_ranks").unwrap()),
            ranks,
            "{tag}"
        );
        assert_ranks_match_grader(&old, &cards);
        assert_eq!(
            old.stats().get("cards_without_role").and_then(Json::as_u64),
            Some(without),
            "{tag}"
        );
    }
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

#[test]
fn the_floors_are_the_graders_floors() {
    // The binary cannot link the mcp crate, so the constants are copied;
    // this is what keeps the copies honest.
    assert_eq!(wowdps_history::DPS_FLOOR, wowdps_mcp::DPS_FLOOR);
    assert_eq!(wowdps_history::DPS_TOP_FLOOR, wowdps_mcp::DPS_TOP_FLOOR);
}

fn player(guid: &str, spec: Spec, dps: f64, hps: f64) -> CardPlayer {
    CardPlayer {
        guid: guid.to_string(),
        name: guid.to_uppercase(),
        class: Some(spec.class()),
        spec: Some(spec),
        loadout: None,
        logged: true,
        enemy: false,
        damage: dps as u64 * 100,
        dps,
        healing: hps as u64 * 100,
        hps,
        deaths: 0,
    }
}

fn card(id: &str, players: Vec<CardPlayer>) -> FightCard {
    FightCard {
        schema: wowdps_proto::history::HISTORY_SCHEMA,
        id: id.to_string(),
        log: 1,
        content: 1,
        kind: FightKind::Encounter,
        name: "Hand-built".to_string(),
        encounter: None,
        key: None,
        start_local_ms: 0,
        tz_min: None,
        start_utc_ms: 0,
        duration_ms: 100_000,
        official_ms: None,
        pars_ms: None,
        success: Some(true),
        aborted: false,
        build: (12, 0, 0),
        project_id: 1,
        log_version: 22,
        owner: None,
        byte_range: None,
        pinned: false,
        best_pct: None,
        players,
        bosses: Vec::new(),
    }
}

#[test]
fn the_floors_exclude_in_sql_exactly_as_the_daemon_does() {
    // The fixture has nobody the floors drop, so a hand-built lake: three
    // healers with one at zero hps (under both floors, but still in the
    // median-of-others pool), four DPS with one at 1% of the top (under
    // the 10%-of-others' median floor, over the 1%-of-top one), two tanks
    // (unranked), an enemy, and an unknown spec — plus a second fight where
    // everyone tied at zero (nobody is dropped: 0 >= 0) and one where a
    // lone healer's others-median is null.
    let tmp = Temp::new("floors");
    std::fs::create_dir_all(tmp.0.join("fights")).unwrap();
    let mut unknown = player("who", Spec::Arms, 500.0, 0.0);
    unknown.spec = None;
    unknown.class = None;
    let cards = [
        card(
            "a",
            vec![
                player("h1", Spec::Discipline, 100.0, 800.0),
                player("h2", Spec::RestorationShaman, 50.0, 1000.0),
                player("h3", Spec::HolyPaladin, 0.0, 0.0),
                player("d1", Spec::Arms, 1000.0, 0.0),
                player("d2", Spec::Fire, 900.0, 0.0),
                player("d3", Spec::Marksmanship, 10.0, 0.0),
                player("d4", Spec::FrostMage, 900.0, 0.0),
                player("t1", Spec::Blood, 500.0, 50.0),
                player("t2", Spec::ProtectionWarrior, 400.0, 40.0),
                CardPlayer {
                    enemy: true,
                    ..player("e", Spec::Arms, 5000.0, 5000.0)
                },
                unknown,
            ],
        ),
        card(
            "b",
            vec![
                player("d1", Spec::Arms, 0.0, 0.0),
                player("d2", Spec::Fire, 0.0, 0.0),
                player("h1", Spec::Discipline, 0.0, 0.0),
            ],
        ),
        card(
            "c",
            vec![
                player("d1", Spec::Arms, 1000.0, 0.0),
                player("d2", Spec::Fire, 30.0, 0.0),
                player("h1", Spec::Discipline, 0.0, 0.0),
            ],
        ),
        // A false start: the others' median is 0 so d2 passes that floor
        // (5 >= 0) and only the 1%-of-top floor drops it.
        card(
            "d",
            vec![
                player("d1", Spec::Arms, 1000.0, 0.0),
                player("d2", Spec::Fire, 5.0, 0.0),
                player("d3", Spec::Marksmanship, 0.0, 0.0),
                player("d4", Spec::FrostMage, 0.0, 0.0),
            ],
        ),
    ];
    for c in &cards {
        std::fs::write(
            tmp.0.join("fights").join(format!("{}.json", c.id)),
            c.to_json().to_line(),
        )
        .unwrap();
    }
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(lake.views(), ["fights", "players", "role_ranks"]);
    assert_ranks_match_grader(&lake, &cards);
    // The shape, spelled out: fight a ranks 2 of 3 healers and 3 of 4 DPS.
    let t = lake
        .sql(
            "SELECT guid, rank, count, excluded FROM role_ranks WHERE fight_id = 'a' \
             ORDER BY role, rank, guid",
        )
        .unwrap();
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    assert_eq!(
        got,
        [
            r#"["d1",1,3,1]"#,
            r#"["d2",2,3,1]"#,
            r#"["d4",2,3,1]"#,
            r#"["h2",1,2,1]"#,
            r#"["h1",2,2,1]"#,
        ]
    );
    // Fight b: three zeros, nobody dropped; fight c: the lone healer at 0
    // hps ranks (no others, no top) and d2 is dropped by both floors;
    // fight d: d2 by the top floor alone, the zeros by both.
    let t = lake
        .sql("SELECT fight_id, guid, rank FROM role_ranks WHERE fight_id <> 'a' ORDER BY 1, 2")
        .unwrap();
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    assert_eq!(
        got,
        [
            r#"["b","d1",1]"#,
            r#"["b","d2",1]"#,
            r#"["b","h1",1]"#,
            r#"["c","d1",1]"#,
            r#"["c","h1",1]"#,
            r#"["d","d1",1]"#,
        ]
    );
    assert_eq!(
        lake.stats()
            .get("cards_without_role")
            .and_then(Json::as_u64),
        Some(0)
    );
}

#[test]
fn a_card_with_no_specs_at_all_still_answers_role_queries() {
    // An arena card (arena.txt carries no COMBATANT_INFO) or an R8-failed
    // roster stores `"role": null` on every player. With nothing but such
    // cards DuckDB sniffs the field as JSON, not VARCHAR — which is why the
    // views derive `role` from `spec` and never read the stored value.
    // Alone, beside a PR #12 card (no `role` key), beside a normal card:
    // `players` and `role_ranks` select, the specless rows carry role NULL
    // and never rank, and the grader agrees.
    let mut specless = card(
        "n",
        vec![
            player("p1", Spec::Arms, 800.0, 0.0),
            player("p2", Spec::Discipline, 100.0, 900.0),
        ],
    );
    for p in &mut specless.players {
        p.spec = None;
        p.class = None;
    }
    let text = specless.to_json().to_line();
    assert!(text.contains("\"role\":null"), "{text}");
    let normal = card(
        "z",
        vec![
            player("d1", Spec::Arms, 1000.0, 0.0),
            player("d2", Spec::Fire, 900.0, 0.0),
            player("d3", Spec::Marksmanship, 10.0, 0.0),
            player("h1", Spec::Discipline, 100.0, 900.0),
            player("h2", Spec::HolyPaladin, 0.0, 0.0),
        ],
    );
    let normal_text = normal.to_json().to_line();
    let (_keep, reference) = lake_of("specless-ref", &[("z", normal_text.clone())]);
    let reference_ranks = sorted(reference.sql("SELECT * FROM role_ranks").unwrap());
    let both = [specless.clone(), normal.clone()];
    for (tag, files, cards, without) in [
        ("specless-alone", vec![("n", text.clone())], &both[..1], 0),
        (
            "specless-beside-stripped",
            vec![("n", text.clone()), ("z", strip_role(&normal_text))],
            &both[..],
            1,
        ),
        (
            "specless-beside-normal",
            vec![("n", text.clone()), ("z", normal_text.clone())],
            &both[..],
            0,
        ),
    ] {
        let (_keep, lake) = lake_of(tag, &files);
        assert_ranks_match_grader(&lake, cards);
        let roles = lake
            .sql("SELECT role FROM players WHERE fight_id = 'n'")
            .unwrap();
        assert_eq!(roles.rows.len(), 2, "{tag}");
        assert!(
            roles.rows.iter().all(|r| r[0] == Json::Null),
            "{tag}: {roles:?}"
        );
        let ranks = sorted(lake.sql("SELECT * FROM role_ranks").unwrap());
        assert!(
            ranks.rows.iter().all(|r| cell_str(&r[0]) != "n"),
            "{tag}: a specless player ranked: {ranks:?}"
        );
        if cards.len() == 2 {
            assert_eq!(ranks, reference_ranks, "{tag}");
        } else {
            assert!(ranks.rows.is_empty(), "{tag}: {ranks:?}");
        }
        assert_eq!(
            lake.stats()
                .get("cards_without_role")
                .and_then(Json::as_u64),
            Some(without),
            "{tag}"
        );
    }
}
