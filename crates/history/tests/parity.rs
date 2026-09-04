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
use wowdps_model::{MissKind, Mitigation, Role, Row, Spec, View};
use wowdps_proto::history::{
    CardPlayer, FightCard, FightKind, FightRows, PlayerMitigation, TakenOther,
};
use wowdps_proto::json::Json;
use wowdps_proto::{
    ClientKind, ClientMsg, DaemonClient, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery,
    TrendBucket, TrendMeasure,
};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
/// R17's fixture (`crates/core/fixtures/taken.expected.md`): one kill with
/// a Protection Warrior, a Brewmaster Monk and a Fire Mage taking damage.
const TAKEN_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");
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
    start_over(tmp, FIXTURE)
}

fn start_over(
    tmp: &Temp,
    fixture: &str,
) -> (PathBuf, PathBuf, mpsc::Receiver<std::io::Result<()>>) {
    let socket = tmp.0.join("test.sock");
    let hist = tmp.0.join("history");
    let opts = DaemonOptions {
        socket: socket.clone(),
        lockfile: tmp.0.join("test.lock"),
        source: SourceSpec::File(PathBuf::from(fixture)),
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

/// A monotonically increasing request id, so a test can ask the daemon
/// anything without hand-numbering.
fn next_req() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(10_000);
    NEXT.fetch_add(1, Ordering::Relaxed)
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
            role: None,
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
            role: None,
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
            measure: TrendMeasure::Dps,
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
        ..CardPlayer::default()
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

// ---- R17 (step 2b): the Taken views ------------------------------------------------

/// One player's R17 numbers, in the order the mitigation view's columns
/// come back so the assertion can walk them by name.
struct Taken {
    guid: &'static str,
    /// (column, expected) for every measure this fixture pins.
    measures: [(&'static str, u64); 8],
}

impl Taken {
    const fn new(guid: &'static str, m: [u64; 8]) -> Self {
        Self {
            guid,
            measures: [
                ("taken", m[0]),
                ("mitigated", m[1]),
                ("prevented", m[2]),
                ("absorbed", m[3]),
                ("blocked", m[4]),
                ("stagger", m[5]),
                ("stagger_ticked", m[6]),
                ("misses", m[7]),
            ],
        }
    }

    fn of(&self, column: &str) -> u64 {
        self.measures
            .iter()
            .find(|(c, _)| *c == column)
            .map(|(_, v)| *v)
            .expect("a pinned measure")
    }
}

/// `taken.expected.md`'s own numbers, computed there independently of the
/// parser, over the fixture's one Encounter.
const TAKEN_EXPECTED: [Taken; 3] = [
    // W Durgan, Protection Warrior: partial block + partial absorb, a full
    // BLOCK miss of 55 000, and five misses.
    Taken::new(
        "Player-1168-0A1B2C11",
        [84_000, 85_000, 55_000, 12_000, 18_000, 0, 0, 5],
    ),
    // M Zenlí, Brewmaster Monk: two staggered swings taken in full, the
    // 124255 self-ticks excluded, one fully absorbed dot tick.
    Taken::new(
        "Player-1168-0A1B2C12",
        [70_200, 28_000, 3_000, 25_000, 0, 25_000, 10_000, 1],
    ),
    // F Pyralis, Fire Mage: both pet hits folded on, a full ABSORB of
    // 21 000, and five misses of five different kinds.
    Taken::new(
        "Player-1168-0A1B2C13",
        [52_000, 26_000, 21_000, 5_000, 0, 0, 0, 5],
    ),
];

/// Σ `taken_spells.amount` + `other.amount` = Σ `taken_sources.amount` =
/// the Taken row's amount, for every player of every stored fight — the
/// identity the cap is designed to keep (`TakenOther` is a struct exactly
/// so the rollup cannot be double counted as a row).
fn assert_taken_identities(lake: &Lake, tag: &str) {
    let t = lake
        .sql(
            "SELECT m.fight_id, m.guid, m.taken, m.other_amount, m.other_n, \
                    coalesce((SELECT sum(s.amount) FROM taken_spells s \
                              WHERE s.fight_id = m.fight_id AND s.guid = m.guid), 0)::BIGINT \
                      AS spells, \
                    coalesce((SELECT sum(s.amount) FROM taken_sources s \
                              WHERE s.fight_id = m.fight_id AND s.guid = m.guid), 0)::BIGINT \
                      AS sources \
             FROM mitigation m ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: no mitigation rows at all");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        let taken = r[2].as_u64().unwrap();
        let other = r[3].as_u64().unwrap();
        let spells = r[5].as_u64().unwrap();
        let sources = r[6].as_u64().unwrap();
        assert_eq!(spells + other, taken, "{who}: by-ability + other vs taken");
        assert_eq!(sources, taken, "{who}: by-attacker vs taken");
    }
}

/// The three pcts that must be one number: the mitigation view's (from the
/// record + the Taken row), the card's stored one, and the same formula
/// recomputed in SQL off the card's own columns.
fn assert_pcts_agree(lake: &Lake, tag: &str) {
    let t = lake
        .sql(
            "SELECT m.fight_id, m.guid, m.mitigated_pct, p.mitigated_pct, p.mitigated_pct_sql, \
                    m.mitigated, m.taken, m.prevented, p.mitigated, p.taken, p.prevented \
             FROM mitigation m JOIN players p USING (fight_id, guid) ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: nothing to compare");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        let (sql_pct, stored_pct, recomputed) = (
            r[2].as_f64().unwrap(),
            r[3].as_f64().unwrap(),
            r[4].as_f64().unwrap(),
        );
        assert_eq!(r[5].as_u64(), r[8].as_u64(), "{who}: mitigated");
        assert_eq!(r[6].as_u64(), r[9].as_u64(), "{who}: taken");
        assert_eq!(r[7].as_u64(), r[10].as_u64(), "{who}: prevented");
        let model = wowdps_model::mitigated_pct(
            r[5].as_u64().unwrap(),
            r[6].as_u64().unwrap(),
            r[7].as_u64().unwrap(),
        );
        for (name, got) in [
            ("mitigation.mitigated_pct", sql_pct),
            ("players.mitigated_pct", stored_pct),
            ("players.mitigated_pct_sql", recomputed),
        ] {
            assert!(
                (got - model).abs() < 1e-9,
                "{who}: {name} {got} vs the model's {model}"
            );
        }
    }
}

#[test]
fn the_taken_views_answer_the_r17_fixture() {
    let tmp = Temp::new("taken");
    let (socket, hist, _done) = start_over(&tmp, TAKEN_FIXTURE);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    wait_for_store(&mut client, 1);

    let lake = Lake::open(&hist).expect("lake opens");
    for view in ["taken", "mitigation", "taken_spells", "taken_sources"] {
        assert!(
            lake.views().contains(&view),
            "the daemon's own rows file did not carry {view}: {:?}",
            lake.views()
        );
    }
    // Every number of `taken.expected.md`, from the lake.
    let t = lake
        .sql(
            "SELECT m.guid, m.taken, m.mitigated, m.prevented, m.absorbed, m.blocked, \
                    m.stagger, m.stagger_ticked, m.misses, tk.amount, tk.extra, p.dtps, \
                    p.duration_ms \
             FROM mitigation m JOIN taken tk USING (fight_id, guid) \
                  JOIN players p USING (fight_id, guid) ORDER BY m.guid",
        )
        .unwrap();
    assert_eq!(t.rows.len(), 3, "three players take damage: {t:?}");
    for (row, want) in t.rows.iter().zip(&TAKEN_EXPECTED) {
        let guid = want.guid;
        assert_eq!(cell_str(&row[0]), guid);
        // The SELECT lists the eight measures in `Taken`'s own order.
        for (i, (name, value)) in want.measures.iter().enumerate() {
            assert_eq!(row[i + 1].as_u64(), Some(*value), "{guid} {name}");
        }
        // The Taken meter row is the same number, with the absorbs as
        // `extra`; dtps is it over the R7 duration (60.000 s).
        let taken = want.of("taken");
        assert_eq!(row[9].as_u64(), Some(taken), "{guid} taken row amount");
        assert_eq!(
            row[10].as_u64(),
            Some(want.of("absorbed")),
            "{guid} taken row extra"
        );
        let secs = row[12].as_f64().unwrap() / 1000.0;
        let dtps = row[11].as_f64().unwrap();
        assert!(
            (dtps - taken as f64 / secs).abs() < 1e-6,
            "{guid} dtps {dtps} over {secs}s"
        );
    }
    assert_taken_identities(&lake, "fixture");
    assert_pcts_agree(&lake, "fixture");
    // The store wrote the measures, so nothing is missing.
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_taken").and_then(Json::as_u64),
        Some(0),
        "{stats:?}"
    );
    assert_eq!(
        stats.get("rows_without_mitigation").and_then(Json::as_u64),
        Some(0),
        "{stats:?}"
    );

    // Trend by the two tank measures: the daemon's answer, per fight, is
    // the `players` view's columns.
    for (measure, amount_col, per_sec_col) in [
        (TrendMeasure::Dtps, "taken", "dtps"),
        (TrendMeasure::MitigatedPct, "mitigated", "mitigated_pct_sql"),
    ] {
        for want in &TAKEN_EXPECTED {
            let guid = want.guid;
            let HistoryAnswer::Trend(points) = ask(
                &mut client,
                next_req(),
                HistoryQuery::Trend {
                    guid: guid.to_string(),
                    spec: None,
                    encounter: None,
                    difficulty: None,
                    measure,
                    bucket: TrendBucket::None,
                    since_utc_ms: None,
                    limit: 0,
                    local_cutover_hour: None,
                },
            ) else {
                panic!("trend");
            };
            let sql = lake
                .sql_with(
                    &format!(
                        "SELECT fight_id, start_utc_ms, {amount_col} AS amount, \
                         {per_sec_col} AS per_sec FROM players \
                         WHERE guid = ? AND NOT aborted ORDER BY start_utc_ms DESC"
                    ),
                    &[Json::str(guid)],
                )
                .unwrap();
            assert_eq!(sql.rows.len(), points.len(), "{guid} {measure:?} count");
            for (row, p) in sql.rows.iter().zip(&points) {
                assert_eq!(cell_str(&row[0]), p.fight_id, "{guid} {measure:?} fight");
                assert_eq!(row[1].as_i64(), Some(p.bucket_utc_ms));
                assert_eq!(row[2].as_u64(), Some(p.amount), "{guid} {measure:?} amount");
                let per_sec = row[3].as_f64().unwrap();
                assert!(
                    (per_sec - p.per_sec).abs() < 1e-6,
                    "{guid} {measure:?}: {per_sec} vs {}",
                    p.per_sec
                );
            }
        }
    }
    client.send(&ClientMsg::Shutdown);
}

/// A stored `Row` with just the fields the Taken views read.
fn taken_row(key: &str, label: &str, amount: u64, extra: u64, count: u64) -> Row {
    Row {
        key: key.to_string(),
        label: label.to_string(),
        amount,
        extra,
        count,
        ..Row::default()
    }
}

/// One player's post-2b shape: the Taken meter row, the card line and the
/// mitigation record with both drills, all consistent by construction.
struct Tank {
    guid: &'static str,
    taken: u64,
    record: Mitigation,
    spells: Vec<Row>,
    other: TakenOther,
    sources: Vec<Row>,
}

impl Tank {
    fn card_player(&self) -> CardPlayer {
        CardPlayer {
            guid: self.guid.to_string(),
            name: self.guid.to_uppercase(),
            class: Some(Spec::ProtectionWarrior.class()),
            spec: Some(Spec::ProtectionWarrior),
            logged: true,
            taken: self.taken,
            mitigated: self.record.mitigated(),
            prevented: self.record.prevented(),
            // The card's duration is 100 s (see `card`).
            dtps: self.taken as f64 / 100.0,
            ..CardPlayer::default()
        }
    }
}

/// The hand-built post-2b lake: two players, one whose by-ability list was
/// capped (a non-empty `other`) and one who was only missed (no Taken row
/// at all, so the mitigation view falls back to 0 and the pct guard bites).
fn tanks() -> Vec<Tank> {
    let mut capped = Mitigation {
        absorbed: 500,
        blocked: 200,
        absorbed_full: 300,
        blocked_full: 100,
        stagger: 400,
        stagger_ticked: 250,
        ..Mitigation::default()
    };
    capped.miss(MissKind::Dodge);
    capped.miss(MissKind::Dodge);
    capped.miss(MissKind::Absorb);
    capped.miss(MissKind::Block);
    let mut missed = Mitigation::default();
    for _ in 0..3 {
        missed.miss(MissKind::Parry);
    }
    vec![
        Tank {
            guid: "Player-1-AAAA",
            taken: 11_000,
            record: capped,
            spells: vec![
                taken_row("Cinder Lash", "Cinder Lash", 5_000, 500, 4),
                taken_row("Melee", "Melee", 3_000, 0, 6),
                taken_row("Ember Spit", "Ember Spit", 2_000, 0, 2),
            ],
            other: TakenOther {
                amount: 1_000,
                extra: 0,
                count: 4,
                n: 3,
            },
            sources: vec![
                taken_row("Taken Test Boss", "Taken Test Boss", 7_000, 500, 8),
                taken_row("Taken Test Add", "Taken Test Add", 4_000, 0, 4),
            ],
        },
        Tank {
            guid: "Player-1-BBBB",
            taken: 0,
            record: missed,
            spells: Vec::new(),
            other: TakenOther::default(),
            sources: Vec::new(),
        },
    ]
}

/// The card and rows documents of one hand-built fight.
fn hand_built(id: &str, tanks: &[Tank]) -> (Json, Json) {
    let card = card(id, tanks.iter().map(Tank::card_player).collect());
    let mut rows = FightRows {
        id: id.to_string(),
        ..FightRows::default()
    };
    rows.views[View::Taken.index()] = tanks
        .iter()
        .filter(|t| t.taken > 0)
        .map(|t| {
            taken_row(
                t.guid,
                &t.guid.to_uppercase(),
                t.taken,
                t.record.absorbed,
                10,
            )
        })
        .collect();
    rows.mitigation = tanks
        .iter()
        .map(|t| PlayerMitigation {
            guid: t.guid.to_string(),
            record: t.record,
            taken_spells: t.spells.clone(),
            other: t.other.clone(),
            taken_sources: t.sources.clone(),
        })
        .collect();
    (card.to_json(), rows.to_json())
}

/// Write one fight's two documents into `dir`.
fn write_fight(dir: &Path, id: &str, card: &Json, rows: &Json) {
    for (sub, doc) in [("fights", card), ("rows", rows)] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join(format!("{id}.json")), doc.to_line()).unwrap();
    }
}

/// `v` without the named top-level keys — how a pre-2b document differs
/// from a post-2b one.
fn without(v: &Json, keys: &[&str]) -> Json {
    match v {
        Json::Obj(o) => Json::Obj(
            o.iter()
                .filter(|(k, _)| !keys.contains(&k.as_str()))
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The card as PR #16 wrote it: no tank measures on any player line.
fn pre_2b_card(card: &Json, id: &str) -> Json {
    let players = card
        .get("players")
        .and_then(Json::as_arr)
        .unwrap()
        .iter()
        .map(|p| {
            without(
                p,
                &["taken", "mitigated", "prevented", "dtps", "mitigated_pct"],
            )
        })
        .collect();
    let mut out = match without(card, &["players", "id"]) {
        Json::Obj(o) => o,
        _ => panic!("card"),
    };
    out.push(("id".to_string(), Json::str(id)));
    out.push(("players".to_string(), Json::Arr(players)));
    Json::Obj(out)
}

/// The rows file as PR #16 wrote it: no `mitigation` key and no Taken view
/// (`empty` instead keeps the keys but writes nothing into them — the
/// all-null JSON-typing trap the probes exist for).
fn pre_2b_rows(rows: &Json, id: &str, empty: bool) -> Json {
    let views = match rows.get("views") {
        Some(Json::Obj(o)) => Json::Obj(
            o.iter()
                .filter_map(|(k, v)| {
                    if k == "taken" && !empty {
                        None
                    } else if k == "taken" {
                        Some((k.clone(), Json::Arr(Vec::new())))
                    } else {
                        Some((k.clone(), v.clone()))
                    }
                })
                .collect(),
        ),
        _ => panic!("views"),
    };
    let mut out = match without(rows, &["views", "mitigation", "id"]) {
        Json::Obj(o) => o,
        _ => panic!("rows"),
    };
    out.push(("id".to_string(), Json::str(id)));
    out.push(("views".to_string(), views));
    if empty {
        out.push(("mitigation".to_string(), Json::Arr(Vec::new())));
    }
    Json::Obj(out)
}

#[test]
fn the_taken_identities_hold_in_sql() {
    let tmp = Temp::new("taken-sql");
    let tanks = tanks();
    let (card, rows) = hand_built("hand", &tanks);
    write_fight(&tmp.0, "hand", &card, &rows);
    let lake = Lake::open(&tmp.0).unwrap();
    for view in ["taken", "mitigation", "taken_spells", "taken_sources"] {
        assert!(lake.views().contains(&view), "{:?}", lake.views());
    }
    assert_taken_identities(&lake, "hand-built");
    assert_pcts_agree(&lake, "hand-built");
    // The shape, spelled out: the capped player's rollup and misses, and
    // the missed-only player's zero-denominator guard.
    let t = lake
        .sql(
            "SELECT guid, taken, mitigated, prevented, other_amount, other_n, misses, \
                    dodge, parry, block, absorb, stagger, stagger_ticked, mitigated_pct \
             FROM mitigation ORDER BY guid",
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
            // 1 100 mitigated of 11 400 swung = 9.649122807017545 %.
            r#"["Player-1-AAAA",11000,1100,400,1000,3,4,2,0,1,1,400,250,9.649122807017545]"#,
            // Nothing landed and nothing was prevented: 0, never a NaN.
            r#"["Player-1-BBBB",0,0,0,0,0,3,0,3,0,0,0,0,0]"#,
        ]
    );
    // The by-ability list is the meter's own rows, uncollapsed.
    let spells = lake
        .sql("SELECT key, amount, extra, count FROM taken_spells ORDER BY amount DESC")
        .unwrap();
    assert_eq!(spells.rows.len(), 3, "{spells:?}");
    assert_eq!(cell_str(&spells.rows[0][0]), "Cinder Lash");
    let sources = lake
        .sql("SELECT key, amount FROM taken_sources ORDER BY amount DESC")
        .unwrap();
    assert_eq!(sources.rows.len(), 2, "{sources:?}");
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_taken").and_then(Json::as_u64),
        Some(0)
    );
    assert_eq!(
        stats.get("rows_without_mitigation").and_then(Json::as_u64),
        Some(0)
    );
}

#[test]
fn a_mixed_lake_opens_and_says_which_taken_views_exist() {
    let tanks = tanks();
    let (card, rows) = hand_built("new", &tanks);
    // A pre-2b lake alone: the four views cannot be defined at all, and
    // neither can they when every rows file's `mitigation` is `[]` and
    // every `views.taken` is empty (DuckDB types both as JSON, not a list
    // of structs — the trap the probes exist for).
    for (tag, empty) in [("pre2b-missing", false), ("pre2b-empty", true)] {
        let tmp = Temp::new(tag);
        write_fight(
            &tmp.0,
            "old",
            &pre_2b_card(&card, "old"),
            &pre_2b_rows(&rows, "old", empty),
        );
        let lake = Lake::open(&tmp.0).unwrap();
        assert_eq!(
            lake.views(),
            ["fights", "players", "role_ranks", "rows"],
            "{tag}"
        );
        // Everything else still answers. No card carries the measures, so
        // the struct has no such field to select at all — and
        // `mitigated_pct_sql` is there regardless, reading 0 the way
        // `CardPlayer::from_json` does.
        assert!(
            lake.sql("SELECT taken FROM players").is_err(),
            "{tag}: a pre-2b card cannot have a taken column"
        );
        let t = lake
            .sql("SELECT guid, dps, role, mitigated_pct_sql FROM players ORDER BY 1")
            .unwrap();
        assert_eq!(t.rows.len(), 2, "{tag}");
        for r in &t.rows {
            assert_eq!(cell_str(&r[2]), "tank", "{tag}");
            assert_eq!(r[3].as_f64(), Some(0.0), "{tag}: nothing to compute from");
        }
        let stats = lake.stats();
        assert_eq!(
            stats.get("cards_without_taken").and_then(Json::as_u64),
            Some(1),
            "{tag}"
        );
        assert_eq!(
            stats.get("rows_without_mitigation").and_then(Json::as_u64),
            Some(1),
            "{tag}: {stats:?}"
        );
    }

    // The mixed lake: one post-2b fight beside both pre-2b shapes.
    let tmp = Temp::new("mixed");
    write_fight(&tmp.0, "new", &card, &rows);
    write_fight(
        &tmp.0,
        "old",
        &pre_2b_card(&card, "old"),
        &pre_2b_rows(&rows, "old", false),
    );
    write_fight(
        &tmp.0,
        "empty",
        &pre_2b_card(&card, "empty"),
        &pre_2b_rows(&rows, "empty", true),
    );
    let lake = Lake::open(&tmp.0).unwrap();
    for view in ["taken", "mitigation", "taken_spells", "taken_sources"] {
        assert!(lake.views().contains(&view), "mixed: {:?}", lake.views());
    }
    // Only the post-2b fight has any of it; the old cards read NULL.
    let t = lake
        .sql("SELECT fight_id, count(*) FROM mitigation GROUP BY 1")
        .unwrap();
    assert_eq!(t.rows.len(), 1, "{t:?}");
    assert_eq!(cell_str(&t.rows[0][0]), "new");
    let t = lake
        .sql(
            "SELECT fight_id, guid, taken, mitigated_pct, mitigated_pct_sql \
             FROM players ORDER BY 1, 2",
        )
        .unwrap();
    assert_eq!(t.rows.len(), 6, "{t:?}");
    for r in &t.rows {
        let new = cell_str(&r[0]) == "new";
        assert_eq!(r[2] != Json::Null, new, "{:?}", r);
        assert_eq!(r[3] != Json::Null, new, "{:?}", r);
    }
    assert_taken_identities(&lake, "mixed");
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_taken").and_then(Json::as_u64),
        Some(2),
        "{stats:?}"
    );
    assert_eq!(
        stats.get("rows_without_mitigation").and_then(Json::as_u64),
        Some(1),
        "only the key-less file counts: {stats:?}"
    );
}
