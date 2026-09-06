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
use wowdps_model::{Mark, MarkKind, MissKind, Mitigation, Role, Row, Spec, UptimeCell, View};
use wowdps_proto::history::{
    CardPlayer, FightCard, FightKind, FightRows, PlayerCoarse, PlayerMitigation, PlayerShields,
    PlayerSupport, PlayerUptime, TakenOther,
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
/// R19's fixture (`crates/core/fixtures/support.expected.md`): one kill
/// with an Augmentation Evoker buffing a Fire Mage (and its pet) and an
/// Arms Warrior, a Holy Priest healing, a self-supported proc and two
/// heal-support shares — then a trash tail the store does not keep.
const SUPPORT_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/support.txt");
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

/// `GetFight` on a stored fight, answered or `None` when the daemon has no
/// such fight.
fn fetch_fight(
    client: &mut DaemonClient,
    req_id: u32,
    fight_id: &str,
    view: View,
    drill: Option<&str>,
) -> Option<wowdps_proto::StoredFight> {
    client.send(&ClientMsg::GetFight {
        req_id,
        fight_id: fight_id.to_string(),
        view,
        drill: drill.map(str::to_string),
        boss: None,
    });
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        for msg in client.poll() {
            if let DaemonMsg::Fight { req_id: got, fight } = msg
                && got == req_id
            {
                return fight;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("no fight for {req_id}");
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

    // The players view unnests the cards' player lines: 3 per boss, plus
    // (R19, step 3b) the supporter the second boss's log only ever trails
    // with — `Player-1168-0A1B2C04` gives 29 400 and never swings, and the
    // roster carries them so Σ effective = Σ damage holds on the card.
    let sql = lake
        .sql("SELECT count(*) AS n, count(DISTINCT guid) AS players FROM players")
        .unwrap();
    assert_eq!(sql.rows[0][0].as_i64(), Some(7));
    assert_eq!(sql.rows[0][1].as_i64(), Some(4));

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

/// Σ `taken_spells.amount` + `other.amount` = Σ `taken_sources.amount` +
/// `other_sources.amount` = the Taken row's amount, for every player of
/// every stored fight — the identity the cap is designed to keep
/// (`TakenOther` is a struct exactly so a rollup cannot be double counted
/// as a row). The sums are DuckDB HUGEINTs and arrive as numbers: no cast.
fn assert_taken_identities(lake: &Lake, tag: &str) {
    let t = lake
        .sql(
            "SELECT m.fight_id, m.guid, m.taken, m.other_amount, m.other_n, \
                    coalesce((SELECT sum(s.amount) FROM taken_spells s \
                              WHERE s.fight_id = m.fight_id AND s.guid = m.guid), 0) \
                      AS spells, \
                    coalesce((SELECT sum(s.amount) FROM taken_sources s \
                              WHERE s.fight_id = m.fight_id AND s.guid = m.guid), 0) \
                      AS sources, \
                    m.other_sources_amount, m.other_sources_n \
             FROM mitigation m ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: no mitigation rows at all");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        let taken = r[2].as_u64().unwrap();
        let other = r[3].as_u64().unwrap();
        let spells = r[5].as_u64().unwrap_or_else(|| panic!("{who}: {:?}", r[5]));
        let sources = r[6].as_u64().unwrap_or_else(|| panic!("{who}: {:?}", r[6]));
        let other_sources = r[7].as_u64().unwrap();
        assert_eq!(spells + other, taken, "{who}: by-ability + other vs taken");
        assert_eq!(
            sources + other_sources,
            taken,
            "{who}: by-attacker + other_sources vs taken"
        );
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

    // Drill parity: the daemon's `GetFight { view: Taken, drill }` answers
    // `by_spell` / `by_target` from the same rows file the two drill views
    // unnest — row for row (key, label, amount, extra, count), both sides
    // put in one order since SQL keeps none.
    let fight_id = cell_str(
        &lake
            .sql("SELECT DISTINCT fight_id FROM mitigation")
            .unwrap()
            .rows[0][0],
    );
    let sql_rows = |view: &str, guid: &str| -> Vec<String> {
        lake.sql_with(
            &format!(
                "SELECT key, label, amount, extra, count FROM {view} \
                 WHERE fight_id = ? AND guid = ? ORDER BY amount DESC, key"
            ),
            &[Json::str(&fight_id), Json::str(guid)],
        )
        .unwrap()
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect()
    };
    let daemon_rows = |rows: &[Row]| -> Vec<String> {
        let mut rows: Vec<&Row> = rows.iter().collect();
        rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.key.cmp(&b.key)));
        rows.iter()
            .map(|r| {
                Json::Arr(vec![
                    Json::str(&*r.key),
                    Json::str(&*r.label),
                    Json::u64(r.amount),
                    Json::u64(r.extra),
                    Json::u64(r.count),
                ])
                .to_line()
            })
            .collect()
    };
    for want in &TAKEN_EXPECTED {
        let guid = want.guid;
        let fight = fetch_fight(&mut client, next_req(), &fight_id, View::Taken, Some(guid))
            .unwrap_or_else(|| panic!("{guid}: the daemon serves the stored fight"));
        let b = fight
            .breakdown
            .unwrap_or_else(|| panic!("{guid}: a drilled Taken fight carries a breakdown"));
        assert!(!b.by_spell.is_empty() && !b.by_target.is_empty(), "{guid}");
        assert_eq!(
            daemon_rows(&b.by_spell),
            sql_rows("taken_spells", guid),
            "{guid}: by_ability vs taken_spells"
        );
        assert_eq!(
            daemon_rows(&b.by_target),
            sql_rows("taken_sources", guid),
            "{guid}: by_target vs taken_sources"
        );
    }
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
    other_sources: TakenOther,
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

/// The hand-built post-2b lake: two players, one whose lists were both
/// capped (a non-empty `other` and `other_sources`) and one who was only
/// missed (no Taken row at all, so the mitigation view falls back to 0 and
/// the pct guard bites).
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
                taken_row("Taken Test Add", "Taken Test Add", 3_000, 0, 3),
            ],
            other_sources: TakenOther {
                amount: 1_000,
                extra: 0,
                count: 1,
                n: 2,
            },
        },
        Tank {
            guid: "Player-1-BBBB",
            taken: 0,
            record: missed,
            spells: Vec::new(),
            other: TakenOther::default(),
            sources: Vec::new(),
            other_sources: TakenOther::default(),
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
            other_sources: t.other_sources.clone(),
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
    card_without(
        card,
        id,
        &["taken", "mitigated", "prevented", "dtps", "mitigated_pct"],
    )
}

/// The card as PR #19 wrote it: no healing split, no support scalars and
/// no derived `effective_dps` on any player line — the same seven keys
/// `crates/proto/tests/history.rs` strips.
fn pre_3b_card(card: &Json, id: &str) -> Json {
    card_without(card, id, &PRE_3B_KEYS)
}

const PRE_3B_KEYS: [&str; 7] = [
    "overheal",
    "absorbed",
    "support_given",
    "support_received",
    "healed_received",
    "self_healed",
    "effective_dps",
];

/// `card` re-identified as `id` with `keys` gone from every player line.
fn card_without(card: &Json, id: &str, keys: &[&str]) -> Json {
    let players = card
        .get("players")
        .and_then(Json::as_arr)
        .unwrap()
        .iter()
        .map(|p| without(p, keys))
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
            "SELECT guid, taken, mitigated, prevented, other_amount, other_n, \
                    other_sources_amount, other_sources_n, misses, \
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
            r#"["Player-1-AAAA",11000,1100,400,1000,3,1000,2,4,2,0,1,1,400,250,9.649122807017545]"#,
            // Nothing landed and nothing was prevented: 0, never a NaN.
            r#"["Player-1-BBBB",0,0,0,0,0,0,0,3,0,3,0,0,0,0,0]"#,
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
        // the struct has no such field to select at all — and both
        // `mitigated_pct` and `mitigated_pct_sql` are there regardless,
        // reading 0 the way `CardPlayer::from_json` does, so one query
        // works on any lake.
        assert!(
            lake.sql("SELECT taken FROM players").is_err(),
            "{tag}: a pre-2b card cannot have a taken column"
        );
        let t = lake
            .sql(
                "SELECT guid, dps, role, mitigated_pct_sql, mitigated_pct \
                 FROM players ORDER BY 1",
            )
            .unwrap();
        assert_eq!(t.rows.len(), 2, "{tag}");
        for r in &t.rows {
            assert_eq!(cell_str(&r[2]), "tank", "{tag}");
            assert_eq!(r[3].as_f64(), Some(0.0), "{tag}: nothing to compute from");
            assert_eq!(
                r[4].as_f64(),
                Some(0.0),
                "{tag}: the same column by its name"
            );
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

// ---- R19 (step 3b): the healing split, support, effective dps ----------------------

/// `support.expected.md`'s roster on the kill (60.000 s): the card scalars
/// per player, in the order `ORDER BY guid` returns them.
struct Supported {
    guid: &'static str,
    damage: u64,
    given: u64,
    received: u64,
    overheal: u64,
    absorbed: u64,
    healed_received: u64,
    self_healed: u64,
    /// The rows tier's block: (given_damage, given_healing,
    /// received_damage, received_healing).
    block: [u64; 4],
}

const EVOKER: &str = "Player-1168-0A1B2C21";
const MAGE: &str = "Player-1168-0A1B2C22";
const WARRIOR: &str = "Player-1168-0A1B2C23";
const PRIEST: &str = "Player-1168-0A1B2C24";

const SUPPORT_EXPECTED: [Supported; 4] = [
    // E Vessyra, Augmentation: gives 23 900 of damage shares (7 500 of it to
    // itself — the twice-logged Bombardments) and 2 100 of healing shares.
    Supported {
        guid: EVOKER,
        damage: 69_500,
        given: 23_900,
        received: 7_500,
        overheal: 0,
        absorbed: 0,
        healed_received: 10_000,
        self_healed: 0,
        block: [23_900, 2_100, 7_500, 0],
    },
    // M Ignatia, Fire: 1 650 received, the Water Elemental's 90 folded on.
    Supported {
        guid: MAGE,
        damage: 271_000,
        given: 0,
        received: 1_650,
        overheal: 0,
        absorbed: 0,
        healed_received: 5_000,
        self_healed: 0,
        block: [0, 0, 1_650, 0],
    },
    // W Brakkar, Arms: 14 750 received (two shares on the Execute), 50 000
    // healed incl. the NPC's 5 000. No heal share: a `_HEAL_SUPPORT` line's
    // received side is its SOURCE's (the healer who cast the buffed heal),
    // as the metric definition and `support.expected.tsv` have it — the
    // md's prose table puts the Fate Mirror 2 000 on the Warrior, its own
    // definition and the TSV on the Priest.
    Supported {
        guid: WARRIOR,
        damage: 242_000,
        given: 0,
        received: 14_750,
        overheal: 0,
        absorbed: 0,
        healed_received: 50_000,
        self_healed: 0,
        block: [0, 0, 14_750, 0],
    },
    // H Seraphíne, Holy: the healer's split — 16 000 overhealed, 15 000
    // absorbed (PWS, absorber ≠ defender), both Renew ticks on itself, and
    // both heal shares received (Fate Mirror 2 000 on its Flash Heal,
    // Shifting Sands 100 on its Renew: TSV `support_received_heal` 2100).
    Supported {
        guid: PRIEST,
        damage: 0,
        given: 0,
        received: 0,
        overheal: 16_000,
        absorbed: 15_000,
        healed_received: 13_000,
        self_healed: 13_000,
        block: [0, 0, 0, 2_100],
    },
];

/// The Evoker's per-target table: (target, damage shares, healing shares,
/// support lines) — Σ damage = its given_damage, Σ healing = its
/// given_healing; the Priest's row is the two heal shares alone.
const EVOKER_TARGETS: [(&str, u64, u64, u64); 4] = [
    (MAGE, 1_650, 0, 5),
    (WARRIOR, 14_750, 0, 5),
    (EVOKER, 7_500, 0, 1),
    (PRIEST, 0, 2_100, 2),
];

fn bits(v: &Json) -> u64 {
    v.as_f64()
        .unwrap_or_else(|| panic!("{v:?} is not a number"))
        .to_bits()
}

/// R19's per-supporter identities in SQL: every `support` row's given
/// sides are the sums of its `support_targets` rows (`damage` / `healing`
/// — never `extra` / `count`), and over a fight Σ given = Σ received on
/// each side (every share's source folds to a player, so nothing leaks).
fn assert_support_identities(lake: &Lake, tag: &str) {
    let t = lake
        .sql(
            "SELECT s.fight_id, s.guid, s.given_damage, s.given_healing, \
                    coalesce((SELECT sum(t.damage) FROM support_targets t \
                              WHERE t.fight_id = s.fight_id AND t.guid = s.guid), 0), \
                    coalesce((SELECT sum(t.healing) FROM support_targets t \
                              WHERE t.fight_id = s.fight_id AND t.guid = s.guid), 0) \
             FROM support s ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: no support rows at all");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        assert_eq!(
            r[2].as_u64(),
            r[4].as_u64(),
            "{who}: Σ targets.damage vs given_damage"
        );
        assert_eq!(
            r[3].as_u64(),
            r[5].as_u64(),
            "{who}: Σ targets.healing vs given_healing"
        );
    }
    let t = lake
        .sql(
            "SELECT fight_id, sum(given_damage), sum(received_damage), \
                    sum(given_healing), sum(received_healing) \
             FROM support GROUP BY 1 ORDER BY 1",
        )
        .unwrap();
    for r in &t.rows {
        let fight = cell_str(&r[0]);
        assert_eq!(
            r[1].as_u64(),
            r[2].as_u64(),
            "{tag} {fight}: Σ given = Σ received (damage)"
        );
        assert_eq!(
            r[3].as_u64(),
            r[4].as_u64(),
            "{tag} {fight}: Σ given = Σ received (healing)"
        );
    }
}

/// The three effective rates that must be one number, bit for bit: the
/// card's stored `effective_dps`, `effective_dps_sql` recomputed off the
/// card's own columns, and `CardPlayer::effective_dps` over the parsed
/// card — for every player of every `cards` fight. Returns the stored
/// column's bits per (fight, guid) for the callers that hold them against
/// the daemon.
fn assert_effective_agrees(
    lake: &Lake,
    cards: &[FightCard],
    tag: &str,
) -> HashMap<(String, String), u64> {
    // Only `cards`' fights: a mixed lake holds older ones beside them.
    let ids: Vec<String> = cards
        .iter()
        .map(|c| format!("'{}'", c.id.replace('\'', "''")))
        .collect();
    let t = lake
        .sql(&format!(
            "SELECT fight_id, guid, effective_dps, effective_dps_sql, damage, \
                    support_received, support_given, duration_ms FROM players \
             WHERE fight_id IN ({}) ORDER BY 1, 2",
            ids.join(", ")
        ))
        .unwrap();
    let mut out = HashMap::new();
    for r in &t.rows {
        let key = (cell_str(&r[0]), cell_str(&r[1]));
        let card = cards
            .iter()
            .find(|c| c.id == key.0)
            .unwrap_or_else(|| panic!("{tag}: {key:?} is not a card of this lake"));
        let p = card
            .players
            .iter()
            .find(|p| p.guid == key.1)
            .expect("on card");
        let model = p.effective_dps(card.duration_ms);
        let sql = bits(&r[3]);
        assert_eq!(
            sql,
            model.to_bits(),
            "{tag} {key:?}: effective_dps_sql {} vs the model's {model}",
            r[3].to_line()
        );
        let stored = bits(&r[2]);
        assert_eq!(
            stored,
            model.to_bits(),
            "{tag} {key:?}: stored effective_dps {} vs the model's {model}",
            r[2].to_line()
        );
        // The columns it folds are the card's.
        assert_eq!(r[4].as_u64(), Some(p.damage), "{tag} {key:?} damage");
        assert_eq!(
            r[5].as_u64(),
            Some(p.support_received),
            "{tag} {key:?} received"
        );
        assert_eq!(r[6].as_u64(), Some(p.support_given), "{tag} {key:?} given");
        assert_eq!(
            r[7].as_i64(),
            Some(card.duration_ms),
            "{tag} {key:?} duration"
        );
        out.insert(key, stored);
    }
    assert_eq!(
        out.len(),
        cards.iter().map(|c| c.players.len()).sum::<usize>(),
        "{tag}: one players row per card player"
    );
    out
}

#[test]
fn the_support_views_answer_the_r19_fixture() {
    let tmp = Temp::new("support");
    let (socket, hist, _done) = start_over(&tmp, SUPPORT_FIXTURE);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    // The trash tail (2 s, Σ effective 14 000) is not stored: `store_trash`
    // is off, so the lake is the one kill.
    wait_for_store(&mut client, 1);

    let lake = Lake::open(&hist).expect("lake opens");
    for view in ["support", "support_targets"] {
        assert!(
            lake.views().contains(&view),
            "the daemon's own rows file did not carry {view}: {:?}",
            lake.views()
        );
    }
    let cards = stored_cards(&hist);
    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card.duration_ms, 60_000, "R4: the kill is 60.000 s");

    // Every card scalar of `support.expected.md`, from the `players` view;
    // `support` is derived from the spec (the Evoker alone); `effective_dps`
    // stored = recomputed = the model, bit for bit.
    let t = lake
        .sql(
            "SELECT guid, damage, support_given, support_received, overheal, absorbed, \
                    healed_received, self_healed, support, spec \
             FROM players ORDER BY guid",
        )
        .unwrap();
    assert_eq!(t.rows.len(), 4, "four players on the card: {t:?}");
    for (row, want) in t.rows.iter().zip(&SUPPORT_EXPECTED) {
        let guid = want.guid;
        assert_eq!(cell_str(&row[0]), guid);
        for (i, (name, value)) in [
            ("damage", want.damage),
            ("support_given", want.given),
            ("support_received", want.received),
            ("overheal", want.overheal),
            ("absorbed", want.absorbed),
            ("healed_received", want.healed_received),
            ("self_healed", want.self_healed),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(row[i + 1].as_u64(), Some(*value), "{guid} {name}");
        }
        assert_eq!(
            row[8].as_bool(),
            Some(guid == EVOKER),
            "{guid}: `support` is the Augmentation alone"
        );
        assert!(row[9].as_u64().is_some(), "{guid} has a spec");
    }
    let stored_bits = assert_effective_agrees(&lake, &cards, "fixture");

    // Identity 1: Σ effective = Σ damage = 582 500 — a true partition of the
    // raid's damage — computed in SQL off the card, where the fold is the
    // model's (`greatest(0, damage − received + given)`).
    let t = lake
        .sql(
            "SELECT sum(damage), \
                    sum(greatest(0, damage - support_received + support_given)), \
                    sum(effective_dps_sql * duration_ms / 1000.0) \
             FROM players",
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_u64(), Some(582_500), "Σ damage");
    assert_eq!(t.rows[0][1].as_u64(), Some(582_500), "Σ effective");
    let back = t.rows[0][2].as_f64().unwrap();
    assert!(
        (back - 582_500.0).abs() < 1e-6,
        "Σ effective_dps_sql × secs = {back}"
    );

    // The `support` view is the rows tier's blocks: one per friendly player
    // with any support — the Priest's is a received heal share alone.
    let t = lake
        .sql(
            "SELECT guid, given_damage, given_healing, received_damage, received_healing \
             FROM support ORDER BY guid",
        )
        .unwrap();
    assert_eq!(t.rows.len(), 4, "{t:?}");
    for (row, want) in t.rows.iter().zip(&SUPPORT_EXPECTED) {
        assert_eq!(cell_str(&row[0]), want.guid);
        for (i, name) in [
            "given_damage",
            "given_healing",
            "received_damage",
            "received_healing",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                row[i + 1].as_u64(),
                Some(want.block[i]),
                "{} {name}",
                want.guid
            );
        }
    }
    // The card's damage halves are the block's.
    let t = lake
        .sql(
            "SELECT count(*) FROM support s JOIN players p USING (fight_id, guid) \
             WHERE s.given_damage <> p.support_given OR s.received_damage <> p.support_received",
        )
        .unwrap();
    assert_eq!(
        t.rows[0][0].as_u64(),
        Some(0),
        "card scalars vs rows-tier block"
    );

    // `support_targets`: the Evoker's table — the Mage with its pet's 90
    // folded on, the Warrior, the Evoker itself (the self-supported proc),
    // the Priest's heal share — and nobody else has targets.
    let t = lake
        .sql_with(
            "SELECT target, damage, healing, lines, name, class, spec \
             FROM support_targets WHERE guid = ? ORDER BY damage DESC, target",
            &[Json::str(EVOKER)],
        )
        .unwrap();
    assert_eq!(
        t.columns,
        [
            "target", "damage", "healing", "lines", "name", "class", "spec"
        ]
    );
    let got: Vec<(String, u64, u64, u64)> = t
        .rows
        .iter()
        .map(|r| {
            assert!(
                !cell_str(&r[4]).is_empty(),
                "a target row has a name: {r:?}"
            );
            (
                cell_str(&r[0]),
                r[1].as_u64().unwrap(),
                r[2].as_u64().unwrap(),
                r[3].as_u64().unwrap(),
            )
        })
        .collect();
    let mut want: Vec<(String, u64, u64, u64)> = EVOKER_TARGETS
        .iter()
        .map(|(g, d, h, l)| (g.to_string(), *d, *h, *l))
        .collect();
    want.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    assert_eq!(got, want, "the Evoker's targets");
    let t = lake
        .sql_with(
            "SELECT count(*) FROM support_targets WHERE guid <> ?",
            &[Json::str(EVOKER)],
        )
        .unwrap();
    assert_eq!(
        t.rows[0][0].as_u64(),
        Some(0),
        "only the supporter has targets"
    );
    assert_support_identities(&lake, "fixture");

    // The daemon's `stored_fight { player }` on the supporter carries the
    // same block, from the same rows file the views unnest.
    let fight = fetch_fight(
        &mut client,
        next_req(),
        &card.id,
        View::Damage,
        Some(EVOKER),
    )
    .expect("the daemon serves the stored fight");
    let block = fight
        .support
        .expect("a drilled supporter carries its block");
    assert_eq!(
        [
            block.given_damage,
            block.given_healing,
            block.received_damage,
            block.received_healing
        ],
        SUPPORT_EXPECTED[0].block
    );
    let mut daemon_targets: Vec<(String, u64, u64, u64)> = block
        .targets
        .iter()
        .map(|r| (r.key.clone(), r.amount, r.extra, r.count))
        .collect();
    daemon_targets.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    assert_eq!(
        daemon_targets, got,
        "stored_fight.support.targets vs support_targets"
    );
    let fight = fetch_fight(
        &mut client,
        next_req(),
        &card.id,
        View::Damage,
        Some(PRIEST),
    )
    .expect("stored fight");
    let block = fight
        .support
        .expect("a received-only player still has a block");
    assert_eq!(block.received_healing, 2_100);
    assert!(block.targets.is_empty(), "nothing given: no targets");

    // Grading: `role_ranks` is the grader — the DPS role by effective dps
    // (Mage 269 350 > Warrior 227 250 > Evoker 85 900; the raw order is the
    // same here, the median is not), the Priest by hps.
    assert_ranks_match_grader(&lake, &cards);
    let t = lake
        .sql(
            "SELECT guid, rank_measure, rank, count, excluded, measure, median \
             FROM role_ranks WHERE role = 'dps' ORDER BY rank",
        )
        .unwrap();
    let ranked: Vec<(String, String, u64, u64, u64)> = t
        .rows
        .iter()
        .map(|r| {
            (
                cell_str(&r[0]),
                cell_str(&r[1]),
                r[2].as_u64().unwrap(),
                r[3].as_u64().unwrap(),
                r[4].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ranked,
        [
            (MAGE.to_string(), "effective_dps".to_string(), 1, 3, 0),
            (WARRIOR.to_string(), "effective_dps".to_string(), 2, 3, 0),
            (EVOKER.to_string(), "effective_dps".to_string(), 3, 3, 0),
        ]
    );
    for (r, (guid, effective)) in
        t.rows
            .iter()
            .zip([(MAGE, 269_350.0), (WARRIOR, 227_250.0), (EVOKER, 85_900.0)])
    {
        let key = (card.id.clone(), guid.to_string());
        assert_eq!(
            bits(&r[5]),
            stored_bits[&key],
            "{guid}: measure is effective_dps"
        );
        assert_eq!(r[5].as_f64(), Some(effective / 60.0), "{guid} measure");
        assert_eq!(r[6].as_f64(), Some(227_250.0 / 60.0), "{guid} median");
    }
    let t = lake
        .sql("SELECT guid, rank_measure, rank FROM role_ranks WHERE role = 'healer'")
        .unwrap();
    assert_eq!(t.rows.len(), 1);
    assert_eq!(cell_str(&t.rows[0][0]), PRIEST);
    assert_eq!(cell_str(&t.rows[0][1]), "hps");

    // Trend by effective dps: the daemon's point per fight is the card's
    // `effective` and its stored rate — the same bits SQL holds.
    for want in &SUPPORT_EXPECTED {
        let guid = want.guid;
        let HistoryAnswer::Trend(points) = ask(
            &mut client,
            next_req(),
            HistoryQuery::Trend {
                guid: guid.to_string(),
                spec: None,
                encounter: None,
                difficulty: None,
                measure: TrendMeasure::EffectiveDps,
                bucket: TrendBucket::None,
                since_utc_ms: None,
                limit: 0,
                local_cutover_hour: None,
            },
        ) else {
            panic!("trend");
        };
        assert_eq!(points.len(), 1, "{guid}");
        let p = &points[0];
        assert_eq!(p.fight_id, card.id);
        let effective = wowdps_model::effective(want.damage, want.received, want.given);
        assert_eq!(p.amount, effective, "{guid}: trend amount is effective");
        let key = (card.id.clone(), guid.to_string());
        assert_eq!(
            p.per_sec.to_bits(),
            stored_bits[&key],
            "{guid}: trend per_sec {} vs SQL's bits",
            p.per_sec
        );
    }
    assert_eq!(
        lake.stats()
            .get("cards_without_overheal")
            .and_then(Json::as_u64),
        Some(0),
        "every card the daemon writes carries the split"
    );
    client.send(&ClientMsg::Shutdown);
}

/// An awkward duration so no rate is exact: 61.5 s.
const DURATION_3B: i64 = 61_500;

/// A post-3b card player with the model's own `dps` arithmetic and the two
/// support scalars; the rest default.
fn supported(guid: &str, spec: Spec, damage: u64, given: u64, received: u64) -> CardPlayer {
    let secs = DURATION_3B as f64 / 1000.0;
    CardPlayer {
        guid: guid.to_string(),
        name: guid.to_uppercase(),
        class: Some(spec.class()),
        spec: Some(spec),
        logged: true,
        damage,
        dps: damage as f64 / secs,
        support_given: given,
        support_received: received,
        ..CardPlayer::default()
    }
}

/// The hand-built post-3b fight: an Augmentation whose effective (85 000)
/// overtakes the Mage (80 000 raw, 60 000 effective) and the Warrior
/// (70 000 / 65 000) — the raw order Mage, Warrior, Aug becomes Aug,
/// Warrior, Mage — plus the clamp case the fixture lacks (1 000 raw, 3 000
/// received: effective 0, dropped by the floors either way) and a healer
/// with every scalar of the split set.
fn support_card(id: &str) -> FightCard {
    let secs = DURATION_3B as f64 / 1000.0;
    let mut c = card(
        id,
        vec![
            supported("aug", Spec::Augmentation, 60_000, 30_000, 5_000),
            supported("mage", Spec::Fire, 80_000, 0, 20_000),
            supported("warr", Spec::Arms, 70_000, 0, 5_000),
            supported("clamp", Spec::Marksmanship, 1_000, 0, 3_000),
            CardPlayer {
                healing: 50_000,
                hps: 50_000.0 / secs,
                overheal: 10_000,
                absorbed: 5_000,
                healed_received: 7_000,
                self_healed: 7_000,
                ..supported("heal", Spec::Discipline, 0, 0, 0)
            },
        ],
    );
    c.duration_ms = DURATION_3B;
    c
}

/// Its rows tier: the Augmentation's block with its target table, and the
/// received-only blocks of the two it buffed (empty targets). Σ targets
/// = the Aug's given on both sides; the clamp player is deliberately NOT
/// on the rows tier (its shares came from nobody here — a hand-built
/// card, not a partition).
fn support_rows(id: &str) -> FightRows {
    FightRows {
        id: id.to_string(),
        support: vec![
            PlayerSupport {
                guid: "aug".to_string(),
                given_damage: 30_000,
                given_healing: 1_500,
                received_damage: 5_000,
                received_healing: 0,
                targets: vec![
                    taken_row("mage", "MAGE", 20_000, 0, 4),
                    taken_row("warr", "WARR", 5_000, 1_500, 3),
                    taken_row("aug", "AUG", 5_000, 0, 1),
                ],
            },
            PlayerSupport {
                guid: "mage".to_string(),
                received_damage: 20_000,
                ..PlayerSupport::default()
            },
            PlayerSupport {
                guid: "warr".to_string(),
                received_damage: 5_000,
                received_healing: 1_500,
                ..PlayerSupport::default()
            },
        ],
        ..FightRows::default()
    }
}

/// `rows` re-identified as `id`; `support_key` false drops the key the way
/// a PR #19 rows file never had it (true keeps whatever it holds — the
/// all-empty `[]` of an Augmentation-less fight included).
fn rows_as(rows: &Json, id: &str, support_key: bool) -> Json {
    let drop: &[&str] = if support_key {
        &["id"]
    } else {
        &["id", "support"]
    };
    let mut out = match without(rows, drop) {
        Json::Obj(o) => o,
        _ => panic!("rows"),
    };
    out.push(("id".to_string(), Json::str(id)));
    Json::Obj(out)
}

/// The DPS-role order of `fight` in `role_ranks`, with each row's measure.
fn dps_order(lake: &Lake, fight: &str) -> Vec<(String, u64, f64)> {
    lake.sql_with(
        "SELECT guid, rank, measure FROM role_ranks WHERE fight_id = ? AND role = 'dps' \
         ORDER BY rank, guid",
        &[Json::str(fight)],
    )
    .unwrap()
    .rows
    .iter()
    .map(|r| {
        (
            cell_str(&r[0]),
            r[1].as_u64().unwrap(),
            r[2].as_f64().unwrap(),
        )
    })
    .collect()
}

#[test]
fn a_pre_3b_card_ranks_exactly_as_v22_did() {
    let new = support_card("new");
    let new_text = new.to_json().to_line();
    assert!(new_text.contains("\"effective_dps\":"), "{new_text}");
    let old_json = pre_3b_card(&new.to_json(), "old");
    let old_text = old_json.to_line();
    for key in PRE_3B_KEYS {
        assert!(!old_text.contains(key), "{key} survived the strip");
    }
    // The old card, read back: zeros, so its effective IS its damage and
    // the grader (ranking the DPS role by effective) ranks it by dps —
    // exactly what v22's grader did.
    let old = FightCard::from_json(&old_json).expect("a PR #19 card parses");
    for p in &old.players {
        assert_eq!(p.effective(), p.damage);
        assert_eq!(p.effective_dps(old.duration_ms).to_bits(), p.dps.to_bits());
    }
    // v22, spelled out: the DPS pool by raw dps — mage 80 000, warr 70 000,
    // aug 60 000, the 1 000 dropped by both floors.
    let secs = DURATION_3B as f64 / 1000.0;
    let v22 = vec![
        ("mage".to_string(), 1, 80_000.0 / secs),
        ("warr".to_string(), 2, 70_000.0 / secs),
        ("aug".to_string(), 3, 60_000.0 / secs),
    ];
    // Step 3b on the same fight with its scalars: by effective — aug
    // 85 000, warr 65 000, mage 60 000, the clamp's 0 dropped.
    let v23 = vec![
        ("aug".to_string(), 1, 85_000.0 / secs),
        ("warr".to_string(), 2, 65_000.0 / secs),
        ("mage".to_string(), 3, 60_000.0 / secs),
    ];

    // The pre-3b lake alone: no scalar column exists at all (as no `taken`
    // does on a pre-2b lake), `effective_dps` is there and NULL,
    // `effective_dps_sql` is `dps` bit for bit, and the ranks are v22's.
    let (_keep, lake) = lake_of("pre3b-alone", &[("old", old_text.clone())]);
    assert!(
        lake.sql("SELECT overheal FROM players").is_err(),
        "a pre-3b card cannot have an overheal column"
    );
    let t = lake
        .sql("SELECT guid, dps, effective_dps, effective_dps_sql, support FROM players ORDER BY 1")
        .unwrap();
    assert_eq!(t.rows.len(), 5);
    for r in &t.rows {
        assert_eq!(r[2], Json::Null, "{r:?}: nothing stored");
        assert_eq!(bits(&r[3]), bits(&r[1]), "{r:?}: effective_dps_sql is dps");
        assert_eq!(r[4].as_bool(), Some(cell_str(&r[0]) == "aug"), "{r:?}");
    }
    assert_eq!(dps_order(&lake, "old"), v22);
    assert_ranks_match_grader(&lake, std::slice::from_ref(&old));
    assert_eq!(
        lake.stats()
            .get("cards_without_overheal")
            .and_then(Json::as_u64),
        Some(1)
    );

    // The mixed lake: `union_by_name` gives the struct the scalars, the
    // old card reads NULL in them and still ranks as v22 did, the new one
    // by effective — with the clamp holding the model's 0.
    let (_keep, lake) = lake_of(
        "pre3b-mixed",
        &[("old", old_text.clone()), ("new", new_text.clone())],
    );
    let t = lake
        .sql(
            "SELECT fight_id, guid, dps, effective_dps, effective_dps_sql, overheal, \
                    support_received FROM players ORDER BY 1, 2",
        )
        .unwrap();
    assert_eq!(t.rows.len(), 10);
    for r in &t.rows {
        let is_new = cell_str(&r[0]) == "new";
        assert_eq!(r[3] != Json::Null, is_new, "{r:?}: stored effective_dps");
        assert_eq!(r[5] != Json::Null, is_new, "{r:?}: overheal");
        assert_eq!(r[6] != Json::Null, is_new, "{r:?}: support_received");
        if !is_new {
            assert_eq!(
                bits(&r[4]),
                bits(&r[2]),
                "{r:?}: the old card's sql rate is its dps"
            );
        }
    }
    assert_effective_agrees(&lake, std::slice::from_ref(&new), "mixed/new");
    let t = lake
        .sql("SELECT effective_dps_sql, effective_dps FROM players WHERE guid = 'clamp' AND fight_id = 'new'")
        .unwrap();
    assert_eq!(
        t.rows[0][0].as_f64(),
        Some(0.0),
        "the clamp: 1 000 − 3 000 is 0, never negative"
    );
    assert_eq!(t.rows[0][1].as_f64(), Some(0.0));
    assert_eq!(
        dps_order(&lake, "old"),
        v22,
        "the old card ranks as under v22"
    );
    assert_eq!(
        dps_order(&lake, "new"),
        v23,
        "the new card ranks by effective"
    );
    let t = lake
        .sql(
            "SELECT DISTINCT rank_measure FROM role_ranks WHERE role = 'dps' \
             UNION ALL SELECT DISTINCT rank_measure FROM role_ranks WHERE role = 'healer'",
        )
        .unwrap();
    let labels: Vec<String> = t.rows.iter().map(|r| cell_str(&r[0])).collect();
    assert_eq!(
        labels,
        ["effective_dps", "hps"],
        "one label per role, old and new alike"
    );
    assert_ranks_match_grader(&lake, &[old.clone(), new.clone()]);
    // The healer's split reads back on the new card.
    let t = lake
        .sql(
            "SELECT overheal, absorbed, healed_received, self_healed, support \
             FROM players WHERE fight_id = 'new' AND guid = 'heal'",
        )
        .unwrap();
    assert_eq!(
        Json::Arr(t.rows[0].clone()).to_line(),
        r#"[10000,5000,7000,7000,false]"#
    );
    assert_eq!(
        lake.stats()
            .get("cards_without_overheal")
            .and_then(Json::as_u64),
        Some(1),
        "only the old card"
    );

    // A fresh lake: nothing to regrade.
    let (_keep, lake) = lake_of("post3b-alone", &[("new", new_text)]);
    assert_eq!(dps_order(&lake, "new"), v23);
    assert_eq!(
        lake.stats()
            .get("cards_without_overheal")
            .and_then(Json::as_u64),
        Some(0)
    );
}

#[test]
fn a_mixed_lake_opens_and_says_which_support_views_exist() {
    let card = support_card("new").to_json();
    let rows = support_rows("new").to_json();
    assert!(
        rows.to_line().contains(r#""support":[{"guid":"aug""#),
        "{rows:?}"
    );
    // A pre-3b lake alone: no `support` key in any rows file, so neither
    // view can be defined — and neither can they when every file's list
    // is `[]` (an Augmentation-less night: DuckDB types the column JSON,
    // not a list of structs, the trap the probes exist for). The lake
    // opens and everything else answers.
    let empty_rows = support_rows("x");
    assert!(empty_rows.support.iter().all(|s| s.guid != "none"));
    let empty = FightRows {
        id: "x".to_string(),
        ..FightRows::default()
    }
    .to_json();
    assert!(empty.to_line().contains(r#""support":[]"#), "{empty:?}");
    for (tag, rows_json) in [
        ("pre3b-missing", rows_as(&rows, "old", false)),
        ("pre3b-empty", rows_as(&empty, "old", true)),
    ] {
        let tmp = Temp::new(tag);
        write_fight(&tmp.0, "old", &pre_3b_card(&card, "old"), &rows_json);
        let lake = Lake::open(&tmp.0).unwrap();
        assert_eq!(
            lake.views(),
            ["fights", "players", "role_ranks", "rows"],
            "{tag}"
        );
        assert!(lake.sql("SELECT * FROM support").is_err(), "{tag}");
        assert!(lake.sql("SELECT * FROM support_targets").is_err(), "{tag}");
        let t = lake
            .sql("SELECT count(*) FROM role_ranks WHERE role = 'dps'")
            .unwrap();
        assert_eq!(t.rows[0][0].as_u64(), Some(3), "{tag}: still graded");
    }

    // The mixed lake: one post-3b fight beside both older shapes.
    let tmp = Temp::new("support-mixed");
    write_fight(&tmp.0, "new", &card, &rows);
    write_fight(
        &tmp.0,
        "old",
        &pre_3b_card(&card, "old"),
        &rows_as(&rows, "old", false),
    );
    write_fight(
        &tmp.0,
        "empty",
        &pre_3b_card(&card, "empty"),
        &rows_as(&empty, "empty", true),
    );
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(
        lake.views(),
        [
            "fights",
            "players",
            "role_ranks",
            "rows",
            "support",
            "support_targets"
        ]
    );
    // Only the post-3b fight has any of it.
    let t = lake
        .sql("SELECT fight_id, count(*) FROM support GROUP BY 1")
        .unwrap();
    assert_eq!(t.rows.len(), 1, "{t:?}");
    assert_eq!(cell_str(&t.rows[0][0]), "new");
    assert_eq!(t.rows[0][1].as_u64(), Some(3));
    assert_support_identities(&lake, "mixed");
    // The shape, spelled out.
    let t = lake
        .sql(
            "SELECT guid, given_damage, given_healing, received_damage, received_healing \
             FROM support ORDER BY guid",
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
            r#"["aug",30000,1500,5000,0]"#,
            r#"["mage",0,0,20000,0]"#,
            r#"["warr",0,0,5000,1500]"#,
        ]
    );
    let t = lake
        .sql(
            "SELECT guid, target, name, damage, healing, lines \
             FROM support_targets ORDER BY damage DESC, target",
        )
        .unwrap();
    assert_eq!(
        t.columns,
        ["guid", "target", "name", "damage", "healing", "lines"]
    );
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    assert_eq!(
        got,
        [
            r#"["aug","mage","MAGE",20000,0,4]"#,
            r#"["aug","aug","AUG",5000,0,1]"#,
            r#"["aug","warr","WARR",5000,1500,3]"#,
        ]
    );
    // Two old cards, one new.
    assert_eq!(
        lake.stats()
            .get("cards_without_overheal")
            .and_then(Json::as_u64),
        Some(2)
    );
}

// ---------------------------------------------------------------------------
// R18 (step 4b): the span scalars on the card, the `uptime` / `coarse`
// views on the rows tier, `am_uptime` trend, and the recipe file.
// ---------------------------------------------------------------------------

/// R18's fixture: a Protection Warrior's Shield Block / Shield Wall spans,
/// a Priest's and a Mage's externals, an Evoker's support buffs
/// (`crates/core/fixtures/spans.expected.tsv`).
const SPANS_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/spans.txt");
const SPANS_WARRIOR: &str = "Player-1168-0A1B2C31";
const SPANS_PRIEST: &str = "Player-1168-0A1B2C32";
const SPANS_EVOKER: &str = "Player-1168-0A1B2C33";
const SPANS_MAGE: &str = "Player-1168-0A1B2C34";

/// The card scalars of `spans.expected.tsv` on the kill (60 000 ms):
/// (guid, am_uptime_ms, externals_given, externals_given_ms,
/// externals_received, externals_received_ms).
const SPANS_EXPECTED: [(&str, u64, u32, u64, u32, u64); 4] = [
    (SPANS_WARRIOR, 27_000, 0, 0, 3, 58_000),
    (SPANS_PRIEST, 0, 3, 38_000, 1, 40_000),
    (SPANS_EVOKER, 0, 0, 0, 0, 0),
    (SPANS_MAGE, 0, 3, 120_000, 2, 60_000),
];

/// The five span scalars, the stored pct and the recomputed one, per
/// player of `cards`' fights — stored = SQL = the model, bit for bit, and
/// the scalars are the card's. Returns the stored pct's bits per (fight,
/// guid) for the callers that hold them against the daemon.
fn assert_am_uptime_agrees(
    lake: &Lake,
    cards: &[FightCard],
    tag: &str,
) -> HashMap<(String, String), u64> {
    let ids: Vec<String> = cards
        .iter()
        .map(|c| format!("'{}'", c.id.replace('\'', "''")))
        .collect();
    let t = lake
        .sql(&format!(
            "SELECT fight_id, guid, am_uptime_pct, am_uptime_pct_sql, am_uptime_ms, \
                    externals_given, externals_given_ms, externals_received, \
                    externals_received_ms, duration_ms FROM players \
             WHERE fight_id IN ({}) ORDER BY 1, 2",
            ids.join(", ")
        ))
        .unwrap();
    let mut out = HashMap::new();
    for r in &t.rows {
        let key = (cell_str(&r[0]), cell_str(&r[1]));
        let card = cards
            .iter()
            .find(|c| c.id == key.0)
            .unwrap_or_else(|| panic!("{tag}: {key:?} is not a card of this lake"));
        let p = card
            .players
            .iter()
            .find(|p| p.guid == key.1)
            .expect("on card");
        let model = p.am_uptime_pct(card.duration_ms);
        assert_eq!(
            bits(&r[3]),
            model.to_bits(),
            "{tag} {key:?}: am_uptime_pct_sql {} vs the model's {model}",
            r[3].to_line()
        );
        let stored = bits(&r[2]);
        assert_eq!(
            stored,
            model.to_bits(),
            "{tag} {key:?}: stored am_uptime_pct {} vs the model's {model}",
            r[2].to_line()
        );
        assert_eq!(
            r[4].as_u64(),
            Some(p.am_uptime_ms),
            "{tag} {key:?} am_uptime_ms"
        );
        assert_eq!(
            r[5].as_u64(),
            Some(u64::from(p.externals_given)),
            "{tag} {key:?} externals_given"
        );
        assert_eq!(
            r[6].as_u64(),
            Some(p.externals_given_ms),
            "{tag} {key:?} externals_given_ms"
        );
        assert_eq!(
            r[7].as_u64(),
            Some(u64::from(p.externals_received)),
            "{tag} {key:?} externals_received"
        );
        assert_eq!(
            r[8].as_u64(),
            Some(p.externals_received_ms),
            "{tag} {key:?} externals_received_ms"
        );
        assert_eq!(
            r[9].as_i64(),
            Some(card.duration_ms),
            "{tag} {key:?} duration"
        );
        out.insert(key, stored);
    }
    assert_eq!(
        out.len(),
        cards.iter().map(|c| c.players.len()).sum::<usize>(),
        "{tag}: one players row per card player"
    );
    out
}

/// R18's identities between the rows tier and the card: per fight, Σ
/// `uptime.total_ms` of the `external` cells grouped by `src` is that
/// caster's card `externals_given_ms`, grouped by target it is the
/// target's `externals_received_ms` — for EVERY player, the ones with no
/// cell reading 0 on both sides — and the span counts match likewise.
fn assert_external_identities(lake: &Lake, tag: &str) {
    let t = lake
        .sql(
            "SELECT p.fight_id, p.guid, p.externals_given, p.externals_given_ms, \
                    p.externals_received, p.externals_received_ms, \
                    coalesce((SELECT sum(u.count) FROM uptime u \
                              WHERE u.fight_id = p.fight_id AND u.src = p.guid \
                                AND u.kind = 'external'), 0), \
                    coalesce((SELECT sum(u.total_ms) FROM uptime u \
                              WHERE u.fight_id = p.fight_id AND u.src = p.guid \
                                AND u.kind = 'external'), 0), \
                    coalesce((SELECT sum(u.count) FROM uptime u \
                              WHERE u.fight_id = p.fight_id AND u.guid = p.guid \
                                AND u.kind = 'external'), 0), \
                    coalesce((SELECT sum(u.total_ms) FROM uptime u \
                              WHERE u.fight_id = p.fight_id AND u.guid = p.guid \
                                AND u.kind = 'external'), 0) \
             FROM players p WHERE NOT p.enemy AND p.am_uptime_ms IS NOT NULL ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: no post-4b players at all");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        assert_eq!(
            r[2].as_u64(),
            r[6].as_u64(),
            "{who}: externals_given vs Σ count by src"
        );
        assert_eq!(
            r[3].as_u64(),
            r[7].as_u64(),
            "{who}: externals_given_ms vs Σ total_ms by src"
        );
        assert_eq!(
            r[4].as_u64(),
            r[8].as_u64(),
            "{who}: externals_received vs Σ count by target"
        );
        assert_eq!(
            r[5].as_u64(),
            r[9].as_u64(),
            "{who}: externals_received_ms vs Σ total_ms by target"
        );
    }
    // And over a fight, Σ given = Σ received on both sides.
    let t = lake
        .sql(
            "SELECT fight_id, sum(externals_given), sum(externals_received), \
                    sum(externals_given_ms), sum(externals_received_ms) \
             FROM players WHERE am_uptime_ms IS NOT NULL GROUP BY 1 ORDER BY 1",
        )
        .unwrap();
    for r in &t.rows {
        let fight = cell_str(&r[0]);
        assert_eq!(
            r[1].as_u64(),
            r[2].as_u64(),
            "{tag} {fight}: Σ given = Σ received"
        );
        assert_eq!(
            r[3].as_u64(),
            r[4].as_u64(),
            "{tag} {fight}: Σ given_ms = Σ received_ms"
        );
    }
}

/// The `coarse` view's identities: Σ `taken10` is the player's Taken row
/// (R17's amount + absorbed, the series the engine sums to the row), Σ
/// `heal10` their Healing row, and both lists are typed `BIGINT[]`.
fn assert_coarse_identities(lake: &Lake, tag: &str) {
    let t = lake.sql("DESCRIBE SELECT * FROM coarse").unwrap();
    for col in ["taken10", "heal10"] {
        let ty = t
            .rows
            .iter()
            .find(|r| r.first().and_then(Json::as_str) == Some(col))
            .map(|r| cell_str(&r[1]));
        assert_eq!(ty.as_deref(), Some("BIGINT[]"), "{tag}: coarse.{col}");
    }
    let t = lake
        .sql(
            "SELECT c.fight_id, c.guid, list_sum(c.taken10), \
                    coalesce((SELECT t.amount FROM taken t \
                              WHERE t.fight_id = c.fight_id AND t.guid = c.guid), 0), \
                    list_sum(c.heal10), \
                    coalesce((SELECT h.amount FROM rows r, unnest(r.views.healing) AS u(h) \
                              WHERE r.id = c.fight_id AND h.key = c.guid), 0) \
             FROM coarse c ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: no coarse rows at all");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        assert_eq!(
            r[2].as_u64().unwrap_or(0),
            r[3].as_u64().unwrap_or(0),
            "{who}: Σ taken10 vs the Taken row"
        );
        assert_eq!(
            r[4].as_u64().unwrap_or(0),
            r[5].as_u64().unwrap_or(0),
            "{who}: Σ heal10 vs the Healing row"
        );
    }
}

#[test]
fn the_span_views_answer_the_r18_fixture() {
    let tmp = Temp::new("spans");
    let (socket, hist, _done) = start_over(&tmp, SPANS_FIXTURE);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    // The trash tail is not stored (`store_trash` off): the lake is the kill.
    wait_for_store(&mut client, 1);

    let lake = Lake::open(&hist).expect("lake opens");
    let cards = stored_cards(&hist);
    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card.duration_ms, 60_000, "R4: the kill is 60.000 s");
    assert_eq!(card.players.len(), 4, "{:?}", card.players);

    // Whatever the daemon extracted: stored = SQL = the model, bit for
    // bit, and the scalars the view shows are the card's.
    let stored_bits = assert_am_uptime_agrees(&lake, &cards, "fixture");
    assert_eq!(
        lake.stats()
            .get("cards_without_am_uptime")
            .and_then(Json::as_u64),
        Some(0),
        "every card the daemon writes carries the span scalars"
    );
    assert_eq!(
        lake.stats()
            .get("rows_without_uptime")
            .and_then(Json::as_u64),
        Some(0),
        "every rows file the daemon writes carries the uptime key"
    );

    // `trend { measure: am_uptime }` — the daemon's point per fight is the
    // card's union and its percentage — the same bits SQL holds.
    for p in &card.players {
        let HistoryAnswer::Trend(points) = ask(
            &mut client,
            next_req(),
            HistoryQuery::Trend {
                guid: p.guid.clone(),
                spec: None,
                encounter: None,
                difficulty: None,
                measure: TrendMeasure::AmUptime,
                bucket: TrendBucket::None,
                since_utc_ms: None,
                limit: 0,
                local_cutover_hour: None,
            },
        ) else {
            panic!("trend");
        };
        assert_eq!(points.len(), 1, "{}", p.guid);
        let point = &points[0];
        assert_eq!(point.fight_id, card.id);
        assert_eq!(point.amount, p.am_uptime_ms, "{}: trend amount", p.guid);
        let key = (card.id.clone(), p.guid.clone());
        assert_eq!(
            point.per_sec.to_bits(),
            stored_bits[&key],
            "{}: trend per_sec {} vs SQL's bits",
            p.guid,
            point.per_sec
        );
    }

    // The rows-tier identities hold whenever the views could be typed —
    // and when they could not, the rows tier carries nothing and the card
    // must agree that nothing was given or received.
    let has_uptime = lake.views().contains(&"uptime");
    let has_coarse = lake.views().contains(&"coarse");
    if has_uptime {
        assert_external_identities(&lake, "fixture");
    } else {
        assert!(
            card.players
                .iter()
                .all(|p| p.externals_given_ms == 0 && p.externals_received_ms == 0),
            "no uptime view, yet the card says externals were exchanged: {:?}",
            card.players
        );
    }
    if has_coarse {
        assert_coarse_identities(&lake, "fixture");
    }

    // The daemon's own answers over the same file: the drilled Taken view
    // serves the coarse series with the marks, and `uptime` both halves.
    let fight = fetch_fight(
        &mut client,
        next_req(),
        &card.id,
        View::Taken,
        Some(SPANS_WARRIOR),
    )
    .expect("the daemon serves the stored fight");
    if has_coarse {
        let t = lake
            .sql_with(
                "SELECT taken10, len(marks) FROM coarse WHERE fight_id = ? AND guid = ?",
                &[Json::str(&card.id), Json::str(SPANS_WARRIOR)],
            )
            .unwrap();
        assert_eq!(t.rows.len(), 1, "{t:?}");
        let taken10: Vec<u64> = t.rows[0][0]
            .as_arr()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        let timeline = fight
            .breakdown
            .as_ref()
            .and_then(|b| b.timeline.as_ref())
            .expect("the stored Taken drill carries the coarse series");
        assert_eq!(timeline.bucket_ms, 10_000);
        assert_eq!(
            timeline.buckets, taken10,
            "stored_fight.timeline vs coarse.taken10"
        );
        assert_eq!(
            Some(timeline.marks.len() as u64),
            t.rows[0][1].as_u64(),
            "stored_fight.timeline.marks vs coarse.marks"
        );
    }
    if has_uptime {
        let t = lake
            .sql_with(
                "SELECT guid, spell_id, src, count, total_ms FROM uptime \
                 WHERE fight_id = ? AND (guid = ? OR src = ?) ORDER BY guid, spell_id, src",
                &[
                    Json::str(&card.id),
                    Json::str(SPANS_WARRIOR),
                    Json::str(SPANS_WARRIOR),
                ],
            )
            .unwrap();
        let sql: Vec<(String, u64, String, u64, i64)> = t
            .rows
            .iter()
            .map(|r| {
                (
                    cell_str(&r[0]),
                    r[1].as_u64().unwrap(),
                    cell_str(&r[2]),
                    r[3].as_u64().unwrap(),
                    r[4].as_i64().unwrap(),
                )
            })
            .collect();
        let mut wire: Vec<(String, u64, String, u64, i64)> = fight
            .uptime
            .iter()
            .map(|u| {
                (
                    u.target.clone(),
                    u64::from(u.cell.spell_id),
                    u.cell.src.clone(),
                    u64::from(u.cell.count),
                    u.cell.total_ms,
                )
            })
            .collect();
        wire.sort();
        assert_eq!(wire, sql, "stored_fight.uptime vs the uptime view");
    }

    // The fixture goldens (`spans.expected.tsv`) — the daemon's extraction
    // of the R18 scalars and the rows-tier blocks.
    for (guid, am, given, given_ms, received, received_ms) in SPANS_EXPECTED {
        let p = card
            .players
            .iter()
            .find(|p| p.guid == guid)
            .unwrap_or_else(|| panic!("{guid} on the card"));
        assert_eq!(
            (
                p.am_uptime_ms,
                p.externals_given,
                p.externals_given_ms,
                p.externals_received,
                p.externals_received_ms
            ),
            (am, given, given_ms, received, received_ms),
            "{guid}: card scalars"
        );
    }
    assert!(
        has_uptime,
        "the daemon's own rows file did not carry uptime: {:?}",
        lake.views()
    );
    assert!(
        has_coarse,
        "the daemon's own rows file did not carry coarse: {:?}",
        lake.views()
    );
    // The Warrior's union is 27 s of 60: 45 %, and the tank's first 10 s
    // bucket of taken is 22 000 (`taken10_0`); the Evoker's support-buff
    // uptime on its targets is 48 000 ms; the Mage's Time Warp is three
    // externals given.
    let t = lake
        .sql_with(
            "SELECT am_uptime_pct_sql FROM players WHERE guid = ?",
            &[Json::str(SPANS_WARRIOR)],
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_f64(), Some(45.0));
    let t = lake
        .sql_with(
            "SELECT taken10[1] FROM coarse WHERE guid = ?",
            &[Json::str(SPANS_WARRIOR)],
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_u64(), Some(22_000), "taken10_0");
    let t = lake
        .sql_with(
            "SELECT sum(total_ms) FROM uptime WHERE src = ? AND kind = 'support_buff'",
            &[Json::str(SPANS_EVOKER)],
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_u64(), Some(48_000), "support uptime");
    let t = lake
        .sql_with(
            "SELECT guid, count, total_ms FROM uptime WHERE src = ? AND kind = 'external' \
             ORDER BY guid",
            &[Json::str(SPANS_MAGE)],
        )
        .unwrap();
    assert_eq!(
        t.rows.iter().map(|r| r[1].as_u64().unwrap()).sum::<u64>(),
        3,
        "{t:?}"
    );
    assert_eq!(
        t.rows.iter().map(|r| r[2].as_u64().unwrap()).sum::<u64>(),
        120_000,
        "{t:?}"
    );
    // The Priest's externals given are a Priest's spells, on other people.
    let t = lake
        .sql_with(
            "SELECT count(*) FROM uptime WHERE src = ? AND kind = 'external' AND guid = ?",
            &[Json::str(SPANS_PRIEST), Json::str(SPANS_PRIEST)],
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_u64(), Some(0));
    // Grading is untouched by step 4b: tanks stay unranked.
    assert_ranks_match_grader(&lake, &cards);
    let t = lake
        .sql_with(
            "SELECT count(*) FROM role_ranks WHERE guid = ?",
            &[Json::str(SPANS_WARRIOR)],
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_u64(), Some(0), "the tank is unranked");
    client.send(&ClientMsg::Shutdown);
}

/// An awkward duration so no percentage is exact: 61.5 s.
const DURATION_4B: i64 = 61_500;
const TANK: &str = "tank";
const HEALER: &str = "heal";
const CASTER: &str = "mage";
const AUG: &str = "aug";

/// The hand-built post-4b card: a tank with a 24.6 s union who received
/// three externals (two from the healer, one from the mage), the two
/// casters with their given sides, and an Augmentation whose support buff
/// sits on the mage — every scalar consistent with `spans_rows`.
fn spans_card(id: &str) -> FightCard {
    let secs = DURATION_4B as f64 / 1000.0;
    let mut c = card(
        id,
        vec![
            CardPlayer {
                taken: 28_500,
                dtps: 28_500.0 / secs,
                am_uptime_ms: 24_600,
                externals_received: 3,
                externals_received_ms: 38_000,
                ..supported(TANK, Spec::ProtectionWarrior, 30_000, 0, 0)
            },
            CardPlayer {
                healing: 50_000,
                hps: 50_000.0 / secs,
                externals_given: 2,
                externals_given_ms: 30_000,
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
            CardPlayer {
                externals_given: 1,
                externals_given_ms: 8_000,
                ..supported(CASTER, Spec::Fire, 80_000, 0, 20_000)
            },
            supported(AUG, Spec::Augmentation, 60_000, 20_000, 0),
        ],
    );
    c.duration_ms = DURATION_4B;
    c
}

fn cell(
    spell_id: u32,
    label: &str,
    kind: MarkKind,
    src: &str,
    count: u32,
    total_ms: i64,
) -> UptimeCell {
    UptimeCell {
        spell_id,
        label: label.to_string(),
        kind,
        src: src.to_string(),
        count,
        total_ms,
    }
}

fn mark(at_ms: i64, kind: MarkKind, label: &str, spell_id: u32, dur_ms: i64, src: &str) -> Mark {
    Mark {
        at_ms,
        kind,
        label: label.to_string(),
        spell_id,
        dur_ms,
        src: src.to_string(),
    }
}

/// Its rows tier: the tank's Taken row (Σ `taken10`), the uptime cells
/// keyed by target, and the coarse blocks of the two with any series.
fn spans_rows(id: &str) -> FightRows {
    let mut rows = FightRows {
        id: id.to_string(),
        uptime: vec![
            PlayerUptime {
                guid: TANK.to_string(),
                cells: vec![
                    cell(
                        2565,
                        "Shield Block",
                        MarkKind::ActiveMitigation,
                        TANK,
                        4,
                        24_600,
                    ),
                    cell(
                        33206,
                        "Pain Suppression",
                        MarkKind::External,
                        HEALER,
                        1,
                        20_000,
                    ),
                    cell(
                        47788,
                        "Guardian Spirit",
                        MarkKind::External,
                        HEALER,
                        1,
                        10_000,
                    ),
                    cell(80353, "Time Warp", MarkKind::External, CASTER, 1, 8_000),
                ],
            },
            PlayerUptime {
                guid: CASTER.to_string(),
                cells: vec![cell(
                    395152,
                    "Ebon Might",
                    MarkKind::SupportBuff,
                    AUG,
                    3,
                    40_000,
                )],
            },
        ],
        coarse: vec![
            PlayerCoarse {
                guid: TANK.to_string(),
                taken10: vec![22_000, 0, 5_000, 0, 0, 0, 1_500],
                heal10: Vec::new(),
                marks: vec![
                    mark(
                        1_000,
                        MarkKind::ActiveMitigation,
                        "Shield Block",
                        2565,
                        6_000,
                        TANK,
                    ),
                    mark(
                        5_000,
                        MarkKind::External,
                        "Pain Suppression",
                        33206,
                        20_000,
                        HEALER,
                    ),
                ],
            },
            PlayerCoarse {
                guid: HEALER.to_string(),
                taken10: Vec::new(),
                heal10: vec![10_000, 40_000],
                marks: Vec::new(),
            },
        ],
        ..FightRows::default()
    };
    rows.views[View::Taken.index()] = vec![taken_row(TANK, "TANK", 28_500, 0, 7)];
    rows.views[View::Healing.index()] = vec![taken_row(HEALER, "HEAL", 50_000, 10_000, 9)];
    rows
}

/// The card as PR #23 wrote it: no span scalars and no derived
/// `am_uptime_pct` on any player line.
fn pre_4b_card(card: &Json, id: &str) -> Json {
    card_without(card, id, &PRE_4B_KEYS)
}

const PRE_4B_KEYS: [&str; 6] = [
    "am_uptime_ms",
    "externals_given",
    "externals_given_ms",
    "externals_received",
    "externals_received_ms",
    "am_uptime_pct",
];

/// The rows file as PR #23 wrote it: no `uptime` / `coarse` keys (`empty`
/// instead keeps both keys and writes `[]` into them — an aura-less fight
/// on a post-4b daemon, the all-empty JSON-typing trap the probes and the
/// `starts_with("JSON")` rule exist for).
fn pre_4b_rows(rows: &Json, id: &str, empty: bool) -> Json {
    let mut out = match without(rows, &["uptime", "coarse", "id"]) {
        Json::Obj(o) => o,
        _ => panic!("rows"),
    };
    out.push(("id".to_string(), Json::str(id)));
    if empty {
        out.push(("uptime".to_string(), Json::Arr(Vec::new())));
        out.push(("coarse".to_string(), Json::Arr(Vec::new())));
    }
    Json::Obj(out)
}

#[test]
fn the_span_identities_hold_in_sql() {
    let card = spans_card("new");
    let rows = spans_rows("new");
    let (card_json, rows_json) = (card.to_json(), rows.to_json());
    assert!(
        rows_json.to_line().contains(r#""uptime":[{"guid":"tank""#),
        "{rows_json:?}"
    );
    assert!(
        card_json.to_line().contains(r#""am_uptime_pct":40,"#),
        "24 600 of 61 500 is exactly 40 %: {card_json:?}"
    );
    let tmp = Temp::new("spans-hand");
    write_fight(&tmp.0, "new", &card_json, &rows_json);
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(
        lake.views(),
        [
            "fights",
            "players",
            "role_ranks",
            "rows",
            "taken",
            "uptime",
            "coarse"
        ]
    );
    let stored = assert_am_uptime_agrees(&lake, std::slice::from_ref(&card), "hand");
    assert_eq!(
        stored[&("new".to_string(), TANK.to_string())],
        40.0f64.to_bits()
    );
    assert_external_identities(&lake, "hand");
    assert_coarse_identities(&lake, "hand");
    // The shape, spelled out: `kind` is the NAME, `guid` the target.
    let t = lake
        .sql(
            "SELECT guid, spell_id, label, kind, src, count, total_ms FROM uptime \
             ORDER BY guid, spell_id",
        )
        .unwrap();
    assert_eq!(
        t.columns,
        [
            "guid", "spell_id", "label", "kind", "src", "count", "total_ms"
        ]
    );
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    assert_eq!(
        got,
        [
            r#"["mage",395152,"Ebon Might","support_buff","aug",3,40000]"#,
            r#"["tank",2565,"Shield Block","active_mitigation","tank",4,24600]"#,
            r#"["tank",33206,"Pain Suppression","external","heal",1,20000]"#,
            r#"["tank",47788,"Guardian Spirit","external","heal",1,10000]"#,
            r#"["tank",80353,"Time Warp","external","mage",1,8000]"#,
        ]
    );
    // The coarse block: typed lists, marks with the code, unnested per query.
    let t = lake
        .sql(
            "SELECT c.guid, m.at_ms, m.kind, m.label, m.src, m.dur_ms \
             FROM coarse c, unnest(c.marks) AS u(m) ORDER BY 1, 2",
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
            r#"["tank",1000,4,"Shield Block","tank",6000]"#,
            r#"["tank",5000,3,"Pain Suppression","heal",20000]"#,
        ]
    );
    let t = lake
        .sql("SELECT guid, taken10, heal10 FROM coarse ORDER BY guid")
        .unwrap();
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    assert_eq!(
        got,
        [
            r#"["heal",[],[10000,40000]]"#,
            r#"["tank",[22000,0,5000,0,0,0,1500],[]]"#,
        ]
    );
    assert_eq!(
        lake.stats()
            .get("cards_without_am_uptime")
            .and_then(Json::as_u64),
        Some(0)
    );
    assert_eq!(
        lake.stats()
            .get("rows_without_uptime")
            .and_then(Json::as_u64),
        Some(0)
    );
}

#[test]
fn a_mixed_lake_opens_and_says_which_span_views_exist() {
    let card = spans_card("new").to_json();
    let rows = spans_rows("new").to_json();
    let ranks_alone = {
        let tmp = Temp::new("spans-alone");
        write_fight(&tmp.0, "new", &card, &rows);
        dps_order(&Lake::open(&tmp.0).unwrap(), "new")
    };
    assert_eq!(
        ranks_alone.len(),
        2,
        "the mage and the aug: {ranks_alone:?}"
    );

    // A pre-4b lake alone: today's view list exactly, the two pct columns
    // 0 and no scalar column at all — and the same when every file's
    // lists are `[]` (an aura-less night on a post-4b daemon), except that
    // the rows then DO carry the key.
    for (tag, rows_json, empty) in [
        ("pre4b-missing", pre_4b_rows(&rows, "old", false), false),
        ("pre4b-empty", pre_4b_rows(&rows, "old", true), true),
    ] {
        let tmp = Temp::new(tag);
        write_fight(&tmp.0, "old", &pre_4b_card(&card, "old"), &rows_json);
        let lake = Lake::open(&tmp.0).unwrap();
        assert_eq!(
            lake.views(),
            ["fights", "players", "role_ranks", "rows", "taken"],
            "{tag}"
        );
        assert!(lake.sql("SELECT * FROM uptime").is_err(), "{tag}");
        assert!(lake.sql("SELECT * FROM coarse").is_err(), "{tag}");
        assert!(
            lake.sql("SELECT am_uptime_ms FROM players").is_err(),
            "{tag}: no scalar column on a lake with no such card"
        );
        let t = lake
            .sql("SELECT am_uptime_pct, am_uptime_pct_sql FROM players ORDER BY guid")
            .unwrap();
        assert_eq!(t.rows.len(), 4, "{tag}");
        for r in &t.rows {
            assert_eq!(r[0].as_f64(), Some(0.0), "{tag}: {r:?}");
            assert_eq!(r[1].as_f64(), Some(0.0), "{tag}: {r:?}");
        }
        assert_eq!(
            dps_order(&lake, "old"),
            ranks_alone,
            "{tag}: grading unchanged"
        );
        let stats = lake.stats();
        assert_eq!(
            stats.get("cards_without_am_uptime").and_then(Json::as_u64),
            Some(1),
            "{tag}"
        );
        assert_eq!(
            stats.get("rows_without_uptime").and_then(Json::as_u64),
            Some(u64::from(!empty)),
            "{tag}: an empty list is a stored answer, a missing key is not"
        );
    }

    // The all-empty LIST trap, on its own: a `coarse` block with empty
    // series types `taken10` as `JSON[]`, which the old `!= "JSON"` rule
    // waved through; the `::BIGINT[]` cast still types it, so `coarse`
    // defines (typed), while `uptime: []` everywhere does not define.
    {
        let tmp = Temp::new("spans-emptylists");
        let empty_block = FightRows {
            id: "old".to_string(),
            coarse: vec![PlayerCoarse {
                guid: "x".to_string(),
                ..PlayerCoarse::default()
            }],
            ..FightRows::default()
        }
        .to_json();
        assert!(
            empty_block.to_line().contains(
                r#""uptime":[],"coarse":[{"guid":"x","taken10":[],"heal10":[],"marks":[]}]"#
            ),
            "{empty_block:?}"
        );
        write_fight(&tmp.0, "old", &pre_4b_card(&card, "old"), &empty_block);
        let lake = Lake::open(&tmp.0).unwrap();
        assert_eq!(
            lake.views(),
            ["fights", "players", "role_ranks", "rows", "coarse"]
        );
        let t = lake.sql("DESCRIBE SELECT * FROM coarse").unwrap();
        let types: Vec<(String, String)> = t
            .rows
            .iter()
            .map(|r| (cell_str(&r[0]), cell_str(&r[1])))
            .collect();
        assert_eq!(
            types[2],
            ("taken10".to_string(), "BIGINT[]".to_string()),
            "{types:?}"
        );
        assert_eq!(
            types[3],
            ("heal10".to_string(), "BIGINT[]".to_string()),
            "{types:?}"
        );
        assert!(
            types[4].1.starts_with("JSON"),
            "an all-empty mark list is what the rule catches: {types:?}"
        );
        let t = lake
            .sql("SELECT guid, taken10, list_sum(taken10) FROM coarse")
            .unwrap();
        assert_eq!(t.rows.len(), 1);
        assert_eq!(cell_str(&t.rows[0][0]), "x");
        assert_eq!(t.rows[0][1], Json::Arr(Vec::new()));
        assert_eq!(t.rows[0][2], Json::Null, "list_sum of nothing");
    }

    // The mixed lake: one post-4b fight beside both older shapes.
    let tmp = Temp::new("spans-mixed");
    write_fight(&tmp.0, "new", &card, &rows);
    write_fight(
        &tmp.0,
        "old",
        &pre_4b_card(&card, "old"),
        &pre_4b_rows(&rows, "old", false),
    );
    write_fight(
        &tmp.0,
        "empty",
        &pre_4b_card(&card, "empty"),
        &pre_4b_rows(&rows, "empty", true),
    );
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(
        lake.views(),
        [
            "fights",
            "players",
            "role_ranks",
            "rows",
            "taken",
            "uptime",
            "coarse"
        ]
    );
    // Only the post-4b fight has any of it; the old cards' scalars are
    // NULL and their pct 0 — never an error.
    let t = lake
        .sql("SELECT fight_id, count(*) FROM uptime GROUP BY 1")
        .unwrap();
    assert_eq!(t.rows.len(), 1, "{t:?}");
    assert_eq!(cell_str(&t.rows[0][0]), "new");
    assert_eq!(t.rows[0][1].as_u64(), Some(5));
    let t = lake
        .sql(
            "SELECT fight_id, am_uptime_ms, externals_given_ms, am_uptime_pct, \
                    am_uptime_pct_sql FROM players WHERE guid = 'tank' ORDER BY fight_id",
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
            r#"["empty",null,null,null,0]"#,
            r#"["new",24600,0,40,40]"#,
            r#"["old",null,null,null,0]"#,
        ]
    );
    assert_am_uptime_agrees(&lake, &[spans_card("new")], "mixed");
    assert_external_identities(&lake, "mixed");
    assert_coarse_identities(&lake, "mixed");
    for fight in ["new", "old", "empty"] {
        assert_eq!(
            dps_order(&lake, fight),
            ranks_alone,
            "{fight}: grading unchanged"
        );
    }
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_am_uptime").and_then(Json::as_u64),
        Some(2)
    );
    assert_eq!(
        stats.get("rows_without_uptime").and_then(Json::as_u64),
        Some(1),
        "the missing key, not the empty list"
    );
}

/// Every ```sql block of `docs/history-queries.md`, with its heading.
fn doc_queries() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/history-queries.md"
    ))
    .unwrap();
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut block: Option<String> = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            heading = h.to_string();
        } else if line.starts_with("```sql") {
            block = Some(String::new());
        } else if line.starts_with("```") {
            if let Some(sql) = block.take() {
                out.push((heading.clone(), sql));
            }
        } else if let Some(b) = block.as_mut() {
            b.push_str(line);
            b.push('\n');
        }
    }
    out
}

/// Run the daemon over `fixture` until its store holds `fights`, then shut
/// it down and hand back the lake directory (inside `tmp`).
fn daemon_lake(tmp: &Temp, fixture: &str, fights: u32) -> PathBuf {
    let (socket, hist, _done) = start_over(tmp, fixture);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    wait_for_store(&mut client, fights);
    client.send(&ClientMsg::Shutdown);
    hist
}

/// `docs/history-queries.md` cannot rot: every recipe runs over a lake the
/// daemon wrote from the R18, R19 and R20 fixtures (one directory, the three
/// stores' files — every fight id is distinct) and answers at least one
/// row for a parameter that fits its question.
#[test]
fn every_documented_query_runs_over_the_fixture_lake() {
    let spans = Temp::new("doc-spans");
    let spans_hist = daemon_lake(&spans, SPANS_FIXTURE, 1);
    let support = Temp::new("doc-support");
    let support_hist = daemon_lake(&support, SUPPORT_FIXTURE, 1);
    let shields = Temp::new("doc-shields");
    let shields_hist = daemon_lake(&shields, SHIELDS_FIXTURE, 1);
    let merged = Temp::new("doc-merged");
    for src in [&spans_hist, &support_hist, &shields_hist] {
        for sub in wowdps_history::DIRS {
            let Ok(dir) = std::fs::read_dir(src.join(sub)) else {
                continue;
            };
            std::fs::create_dir_all(merged.0.join(sub)).unwrap();
            for e in dir.flatten() {
                std::fs::copy(e.path(), merged.0.join(sub).join(e.file_name())).unwrap();
            }
        }
    }
    let lake = Lake::open(&merged.0).unwrap();
    for view in [
        "uptime",
        "coarse",
        "support_targets",
        "mitigation",
        "taken_spells",
        "shields",
    ] {
        assert!(lake.views().contains(&view), "{view}: {:?}", lake.views());
    }
    let spans_id = stored_cards(&spans_hist)[0].id.clone();
    let queries = doc_queries();
    assert_eq!(queries.len(), 11, "{queries:?}");
    for (heading, sql) in &queries {
        let param = match heading.as_str() {
            "Healer rank trend across a tier" => Json::str(SPANS_PRIEST),
            "Externals given, to whom, how early (R18, step 4b)" if sql.contains("marks") => {
                Json::str(SPANS_WARRIOR)
            }
            "Externals given, to whom, how early (R18, step 4b)" => Json::str(SPANS_MAGE),
            "Tank swap points (10 s taken series per tank)" => Json::str(&spans_id),
            "Support uptime per target (Augmentation)" => Json::str(SPANS_EVOKER),
            "Augmentation contribution per target (R19)" => Json::str(EVOKER),
            "Damage taken by ability, avoidable share" => Json::str(SPANS_WARRIOR),
            "Absorb efficiency by boss (R20, step 5)" => Json::str(SHIELDS_PRIEST),
            "Shield ledger per spell (R20, step 5)" => Json::str(SHIELDS_PRIEST),
            _ => Json::Null,
        };
        let params: Vec<Json> = if sql.contains("$1") {
            assert_ne!(
                param,
                Json::Null,
                "{heading}: a recipe with $1 needs a value"
            );
            vec![param]
        } else {
            Vec::new()
        };
        let t = lake
            .sql_with(sql, &params)
            .unwrap_or_else(|e| panic!("{heading}:\n{sql}\n{e}"));
        assert!(!t.rows.is_empty(), "{heading}: answered nothing:\n{sql}");
    }
    // Two recipes' answers, pinned: the Mage's Time Warp reaches three
    // players, and the tank-swap series starts with the Warrior's 22 000.
    let (_, externals) = &queries[1];
    let t = lake.sql_with(externals, &[Json::str(SPANS_MAGE)]).unwrap();
    assert_eq!(t.rows.len(), 3, "{t:?}");
    assert!(
        t.rows.iter().all(|r| cell_str(&r[1]) == "Time Warp"),
        "{t:?}"
    );
    let (_, swaps) = &queries[4];
    let t = lake.sql_with(swaps, &[Json::str(&spans_id)]).unwrap();
    assert_eq!(t.rows[0][2].as_u64(), Some(0), "the first bucket: {t:?}");
    assert_eq!(t.rows[0][3].as_u64(), Some(22_000), "{t:?}");
}

// ---------------------------------------------------------------------------
// R20 (step 5): the shield ledger on the card and the rows tier, and the
// `RoleNight` fixed question.
// ---------------------------------------------------------------------------

/// R20's fixture (`crates/core/fixtures/shields.expected.md`): one kill
/// with a Discipline Priest shielding everyone (a pre-pull shield, a
/// refresh up and a refresh down, an over-absorb, one open at the kill), a
/// Mage's re-applied Ice Barrier, a Blood DK's running-total Blood Shield —
/// then a trash tail the store does not keep.
const SHIELDS_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/shields.txt");
const SHIELDS_PRIEST: &str = "Player-1168-0A1B2C41";
const SHIELDS_WARRIOR: &str = "Player-1168-0A1B2C42";
const SHIELDS_MAGE: &str = "Player-1168-0A1B2C43";
const SHIELDS_MONK: &str = "Player-1168-0A1B2C44";
const SHIELDS_DK: &str = "Player-1168-0A1B2C45";

/// The card scalars of `shields.expected.tsv` on the kill: (guid,
/// absorbed = absorbheal, absorb_wasted — `None` is the TSV's blank,
/// shields_unknown) and, from the per-spell rows, absorb_applied.
const SHIELDS_EXPECTED: [(&str, u64, Option<u64>, u32, u64); 5] = [
    (SHIELDS_PRIEST, 65_000, Some(19_000), 2, 75_000),
    (SHIELDS_MAGE, 11_000, Some(5_000), 0, 16_000),
    (SHIELDS_DK, 9_000, Some(5_000), 0, 14_000),
    (SHIELDS_WARRIOR, 0, None, 0, 0),
    (SHIELDS_MONK, 0, None, 0, 0),
];

/// The per-spell ledger rows of the kill: (absorber, spell_id, label,
/// applied, consumed, wasted, count, unknown).
type ShieldGolden = (&'static str, u64, &'static str, u64, u64, u64, u64, u64);
const SHIELDS_ROWS: [ShieldGolden; 3] = [
    (
        SHIELDS_PRIEST,
        17,
        "Power Word: Shield",
        75_000,
        65_000,
        19_000,
        7,
        2,
    ),
    (
        SHIELDS_MAGE,
        11426,
        "Ice Barrier",
        16_000,
        11_000,
        5_000,
        2,
        0,
    ),
    (
        SHIELDS_DK,
        77535,
        "Blood Shield",
        14_000,
        9_000,
        5_000,
        1,
        0,
    ),
];

/// The shield scalars, the stored efficiency and the recomputed one, per
/// player of `cards`' fights — stored = SQL = the model, bit for bit (or
/// all three NULL / `None`), and the scalars are the card's. Returns the
/// SQL efficiency per (fight, guid) for the callers that hold it against
/// the daemon.
fn assert_absorb_efficiency_agrees(
    lake: &Lake,
    cards: &[FightCard],
    tag: &str,
) -> HashMap<(String, String), Option<f64>> {
    let ids: Vec<String> = cards
        .iter()
        .map(|c| format!("'{}'", c.id.replace('\'', "''")))
        .collect();
    let t = lake
        .sql(&format!(
            "SELECT fight_id, guid, absorbed, absorb_wasted, shields_unknown, \
                    absorb_efficiency, absorb_efficiency_sql FROM players \
             WHERE fight_id IN ({}) ORDER BY 1, 2",
            ids.join(", ")
        ))
        .unwrap();
    let mut out = HashMap::new();
    for r in &t.rows {
        let key = (cell_str(&r[0]), cell_str(&r[1]));
        let card = cards
            .iter()
            .find(|c| c.id == key.0)
            .unwrap_or_else(|| panic!("{tag}: {key:?} is not a card of this lake"));
        let p = card
            .players
            .iter()
            .find(|p| p.guid == key.1)
            .expect("on card");
        assert_eq!(r[2].as_u64(), Some(p.absorbed), "{tag} {key:?} absorbed");
        assert_eq!(
            r[3].as_u64(),
            p.absorb_wasted,
            "{tag} {key:?} absorb_wasted {}",
            r[3].to_line()
        );
        assert_eq!(
            r[4].as_u64(),
            Some(u64::from(p.shields_unknown)),
            "{tag} {key:?} shields_unknown"
        );
        let model = p.absorb_efficiency();
        assert_eq!(
            r[6].as_f64().map(f64::to_bits),
            model.map(f64::to_bits),
            "{tag} {key:?}: absorb_efficiency_sql {} vs the model's {model:?}",
            r[6].to_line()
        );
        assert_eq!(
            r[5].as_f64().map(f64::to_bits),
            model.map(f64::to_bits),
            "{tag} {key:?}: stored absorb_efficiency {} vs the model's {model:?}",
            r[5].to_line()
        );
        out.insert(key, r[6].as_f64());
    }
    assert_eq!(
        out.len(),
        cards.iter().map(|c| c.players.len()).sum::<usize>(),
        "{tag}: one players row per card player"
    );
    out
}

/// R20's identities between the rows tier and the card: per fight, Σ
/// `shields.consumed` grouped by absorber is that player's card `absorbed`
/// — for EVERY post-5 friendly player, the ones with no row reading 0 on
/// both sides — `applied = consumed + wasted` on every row whose shields
/// were all known, and nothing is negative.
fn assert_shield_identities(lake: &Lake, tag: &str) {
    let t = lake
        .sql(
            "SELECT p.fight_id, p.guid, p.absorbed, \
                    coalesce((SELECT sum(s.consumed) FROM shields s \
                              WHERE s.fight_id = p.fight_id AND s.guid = p.guid), 0) \
             FROM players p WHERE NOT p.enemy AND p.shields_unknown IS NOT NULL ORDER BY 1, 2",
        )
        .unwrap();
    assert!(!t.rows.is_empty(), "{tag}: no post-5 players at all");
    for r in &t.rows {
        let who = format!("{tag} {}/{}", cell_str(&r[0]), cell_str(&r[1]));
        assert_eq!(
            r[2].as_u64(),
            r[3].as_u64(),
            "{who}: absorbed vs Σ shields.consumed"
        );
    }
    let t = lake
        .sql(
            "SELECT fight_id, guid, spell_id, applied, consumed, wasted, count, unknown \
             FROM shields ORDER BY 1, 2, 3",
        )
        .unwrap();
    for r in &t.rows {
        let who = format!(
            "{tag} {}/{}/{}",
            cell_str(&r[0]),
            cell_str(&r[1]),
            cell_str(&r[2])
        );
        let n = |i: usize| {
            r[i].as_u64()
                .unwrap_or_else(|| panic!("{who}: {} < 0", r[i].to_line()))
        };
        if n(7) == 0 {
            assert_eq!(n(3), n(4) + n(5), "{who}: applied = consumed + wasted");
        }
        assert!(n(6) >= n(7), "{who}: unknown ≤ count");
    }
}

/// `daemon` and `sql` must be the same roster, row by row: every field,
/// the f64s bit for bit (the daemon's fold is the meter's own arithmetic,
/// `x / (ms / 1000)` per pull and a plain mean; a formula that differs by
/// a rounding shows here and is reported, never loosened).
fn assert_role_night_matches(
    daemon: &[wowdps_model::RoleNightRow],
    sql: &[wowdps_model::RoleNightRow],
    tag: &str,
) {
    assert_eq!(
        daemon.len(),
        sql.len(),
        "{tag}: row counts\n daemon {daemon:?}\n sql {sql:?}"
    );
    for (d, s) in daemon.iter().zip(sql) {
        let who = format!("{tag} {}", d.guid);
        assert_eq!(d.guid, s.guid, "{who}: order");
        assert_eq!(d.name, s.name, "{who}: name");
        assert_eq!(d.spec, s.spec, "{who}: spec");
        assert_eq!(d.role, s.role, "{who}: role");
        assert_eq!(d.pulls, s.pulls, "{who}: pulls");
        assert_eq!(d.taken, s.taken, "{who}: taken");
        assert_eq!(
            d.externals_given, s.externals_given,
            "{who}: externals_given"
        );
        for (name, a, b) in [
            ("measure", d.measure, s.measure),
            ("best", d.best, s.best),
            ("dtps", d.dtps, s.dtps),
            ("am_uptime_pct", d.am_uptime_pct, s.am_uptime_pct),
            ("overheal_pct", d.overheal_pct, s.overheal_pct),
        ] {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "{who}: {name} daemon {a} vs sql {b}"
            );
        }
        assert_eq!(
            d.absorb_efficiency.map(f64::to_bits),
            s.absorb_efficiency.map(f64::to_bits),
            "{who}: absorb_efficiency daemon {:?} vs sql {:?}",
            d.absorb_efficiency,
            s.absorb_efficiency
        );
    }
}

#[test]
fn the_shield_views_answer_the_r20_fixture() {
    let tmp = Temp::new("shields");
    let (socket, hist, _done) = start_over(&tmp, SHIELDS_FIXTURE);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    // The trash tail is not stored (`store_trash` off): the lake is the kill.
    wait_for_store(&mut client, 1);

    let lake = Lake::open(&hist).expect("lake opens");
    let cards = stored_cards(&hist);
    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card.duration_ms, 60_000, "R4: the kill is 60.000 s");
    assert_eq!(card.players.len(), 5, "{:?}", card.players);
    let encounter = card.encounter.expect("a boss");
    assert_eq!((encounter.id, encounter.difficulty), (3148, 16));

    // Whatever the daemon extracted: stored = SQL = the model, bit for
    // bit, and the scalars the view shows are the card's.
    let sql_eff = assert_absorb_efficiency_agrees(&lake, &cards, "fixture");
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_shields").and_then(Json::as_u64),
        Some(0),
        "every card the daemon writes carries shields_unknown"
    );
    assert_eq!(
        stats.get("rows_without_shields").and_then(Json::as_u64),
        Some(0),
        "every rows file the daemon writes carries the shields key"
    );

    // `trend { measure: absorb_efficiency }` — a point per fight whose
    // waste is known, its value the efficiency as a PERCENTAGE — the same
    // bits SQL holds, scaled the same way; a player with no known waste
    // contributes no point.
    for p in &card.players {
        let HistoryAnswer::Trend(points) = ask(
            &mut client,
            next_req(),
            HistoryQuery::Trend {
                guid: p.guid.clone(),
                spec: None,
                encounter: None,
                difficulty: None,
                measure: TrendMeasure::AbsorbEfficiency,
                bucket: TrendBucket::None,
                since_utc_ms: None,
                limit: 0,
                local_cutover_hour: None,
            },
        ) else {
            panic!("trend");
        };
        let key = (card.id.clone(), p.guid.clone());
        match sql_eff[&key] {
            None => assert!(
                points.is_empty(),
                "{}: an unknown waste is no trend point, got {points:?}",
                p.guid
            ),
            Some(eff) => {
                assert_eq!(points.len(), 1, "{}: {points:?}", p.guid);
                let point = &points[0];
                assert_eq!(point.fight_id, card.id);
                assert_eq!(point.amount, p.absorbed, "{}: trend amount", p.guid);
                assert_eq!(
                    point.per_sec.to_bits(),
                    (eff * 100.0).to_bits(),
                    "{}: trend per_sec {} vs SQL's {eff} × 100",
                    p.guid,
                    point.per_sec
                );
            }
        }
    }

    // The rows-tier identities hold whenever the view could be typed —
    // and when it could not, the rows tier carries nothing and the card
    // must agree that nothing was absorbed.
    let has_shields = lake.views().contains(&"shields");
    if has_shields {
        assert_shield_identities(&lake, "fixture");
    } else {
        assert!(
            card.players.iter().all(|p| p.absorbed == 0),
            "no shields view, yet the card says absorbs happened: {:?}",
            card.players
        );
    }

    // The daemon's own drill over the same file: `stored_fight { player }`
    // serves the Priest's ledger rows, consumed desc — the `shields` view
    // says the same.
    let fight = fetch_fight(
        &mut client,
        next_req(),
        &card.id,
        View::Healing,
        Some(SHIELDS_PRIEST),
    )
    .expect("the daemon serves the stored fight");
    if has_shields {
        let t = lake
            .sql_with(
                "SELECT spell_id, label, applied, consumed, wasted, count, unknown FROM shields \
                 WHERE fight_id = ? AND guid = ? ORDER BY consumed DESC, spell_id",
                &[Json::str(&card.id), Json::str(SHIELDS_PRIEST)],
            )
            .unwrap();
        let sql: Vec<String> = t
            .rows
            .iter()
            .map(|r| Json::Arr(r.clone()).to_line())
            .collect();
        let wire: Vec<String> = fight
            .shields
            .iter()
            .map(|s| {
                Json::Arr(vec![
                    Json::num(s.spell_id),
                    Json::str(&*s.label),
                    Json::u64(s.applied),
                    Json::u64(s.consumed),
                    Json::u64(s.wasted),
                    Json::num(s.count),
                    Json::num(s.unknown),
                ])
                .to_line()
            })
            .collect();
        assert_eq!(wire, sql, "stored_fight.shields vs the shields view");
    }

    // The fixture goldens (`shields.expected.tsv`) — the daemon's
    // extraction of the R20 scalars and the rows-tier ledger.
    for (guid, absorbed, wasted, unknown, _) in SHIELDS_EXPECTED {
        let p = card
            .players
            .iter()
            .find(|p| p.guid == guid)
            .unwrap_or_else(|| panic!("{guid} on the card"));
        assert_eq!(
            (p.absorbed, p.absorb_wasted, p.shields_unknown),
            (absorbed, wasted, unknown),
            "{guid}: card scalars"
        );
    }
    assert!(
        has_shields,
        "the daemon's own rows file did not carry shields: {:?}",
        lake.views()
    );
    let t = lake
        .sql(
            "SELECT guid, spell_id, label, applied, consumed, wasted, count, unknown \
             FROM shields ORDER BY consumed DESC",
        )
        .unwrap();
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    let want: Vec<String> = SHIELDS_ROWS
        .iter()
        .map(
            |(g, id, label, applied, consumed, wasted, count, unknown)| {
                Json::Arr(vec![
                    Json::str(*g),
                    Json::u64(*id),
                    Json::str(*label),
                    Json::u64(*applied),
                    Json::u64(*consumed),
                    Json::u64(*wasted),
                    Json::u64(*count),
                    Json::u64(*unknown),
                ])
                .to_line()
            },
        )
        .collect();
    assert_eq!(got, want, "the per-spell rows");
    for (guid, _, _, _, applied) in SHIELDS_EXPECTED {
        let t = lake
            .sql_with(
                "SELECT coalesce(sum(applied), 0) FROM shields WHERE guid = ?",
                &[Json::str(guid)],
            )
            .unwrap();
        assert_eq!(
            t.rows[0][0].as_u64(),
            Some(applied),
            "{guid}: absorb_applied"
        );
    }
    // The Priest: 65 000 of 84 000.
    let t = lake
        .sql_with(
            "SELECT absorb_efficiency_sql FROM players WHERE guid = ?",
            &[Json::str(SHIELDS_PRIEST)],
        )
        .unwrap();
    assert_eq!(t.rows[0][0].as_f64(), Some(65_000.0 / 84_000.0));

    // `RoleNight` over the night `Progression` lists — the daemon's fold
    // against `Lake::role_night`, row by row.
    let HistoryAnswer::Progression { nights, .. } = ask(
        &mut client,
        next_req(),
        HistoryQuery::Progression {
            encounter: encounter.id,
            difficulty: encounter.difficulty,
            local_cutover_hour: None,
        },
    ) else {
        panic!("progression");
    };
    assert_eq!(nights.len(), 1, "{nights:?}");
    let night = nights[0].day_utc_ms;
    let sql_rows = lake
        .role_night(encounter.id, encounter.difficulty, night)
        .unwrap();
    assert_eq!(sql_rows.len(), 5, "{sql_rows:?}");
    // The roster's shape from SQL alone: the Warrior, the Monk and the
    // Blood DK tank (by mitigated_pct desc), the Priest heals (hps), the
    // Mage is the one DPS — each a single pull whose mean is its best.
    let order: Vec<(&str, Option<Role>)> =
        sql_rows.iter().map(|r| (r.guid.as_str(), r.role)).collect();
    let mut tanks: Vec<&str> = order[..3].iter().map(|(g, _)| *g).collect();
    tanks.sort_unstable();
    assert_eq!(
        tanks,
        [SHIELDS_WARRIOR, SHIELDS_MONK, SHIELDS_DK],
        "{order:?}"
    );
    assert!(
        order[..3].iter().all(|(_, r)| *r == Some(Role::Tank)),
        "{order:?}"
    );
    assert!(
        sql_rows[..3]
            .windows(2)
            .all(|w| w[0].measure >= w[1].measure),
        "tanks by measure desc: {order:?}"
    );
    assert_eq!(order[3], (SHIELDS_PRIEST, Some(Role::Healer)), "{order:?}");
    assert_eq!(order[4], (SHIELDS_MAGE, Some(Role::Dps)), "{order:?}");
    for r in &sql_rows {
        assert_eq!(r.pulls, 1, "{r:?}");
        assert_eq!(r.measure.to_bits(), r.best.to_bits(), "{r:?}");
        let p = card.players.iter().find(|p| p.guid == r.guid).unwrap();
        assert_eq!(r.taken, p.taken, "{r:?}");
        assert_eq!(r.dtps.to_bits(), p.dtps.to_bits(), "{r:?}");
        assert_eq!(
            r.absorb_efficiency.map(f64::to_bits),
            p.absorb_efficiency().map(f64::to_bits),
            "{r:?}"
        );
    }
    let priest = &sql_rows[3];
    let p = card
        .players
        .iter()
        .find(|p| p.guid == SHIELDS_PRIEST)
        .unwrap();
    assert_eq!(priest.measure.to_bits(), p.hps.to_bits());
    assert_eq!(priest.overheal_pct, 6_000.0 * 100.0 / 95_000.0);
    let HistoryAnswer::RoleNight { night: n, rows } = ask(
        &mut client,
        next_req(),
        HistoryQuery::RoleNight {
            encounter: encounter.id,
            difficulty: encounter.difficulty,
            night,
            local_cutover_hour: None,
        },
    ) else {
        panic!("role night");
    };
    assert_eq!(n.day_utc_ms, night);
    assert_eq!(n.pulls, 1, "{n:?}");
    assert!(n.kill, "{n:?}");
    assert_role_night_matches(&rows, &sql_rows, "fixture");

    // Grading is untouched by step 5.
    assert_ranks_match_grader(&lake, &cards);
    client.send(&ClientMsg::Shutdown);
}

/// The hand-built post-5 card: a Discipline Priest whose shields closed
/// with a known waste (20 000 absorbed, 5 000 wasted, two of them
/// unknown-applied), a Mage whose one Ice Barrier was fully consumed
/// (efficiency exactly 1), and a tank and a DPS who shielded nobody —
/// waste unknown, `null` on the card — every scalar consistent with
/// `shields_rows`.
fn shields_card(id: &str) -> FightCard {
    let secs = DURATION_4B as f64 / 1000.0;
    let mut c = card(
        id,
        vec![
            CardPlayer {
                taken: 28_500,
                dtps: 28_500.0 / secs,
                am_uptime_ms: 24_600,
                ..supported(TANK, Spec::ProtectionWarrior, 30_000, 0, 0)
            },
            CardPlayer {
                healing: 50_000,
                hps: 50_000.0 / secs,
                overheal: 10_000,
                absorbed: 20_000,
                absorb_wasted: Some(5_000),
                shields_unknown: 2,
                externals_given: 2,
                externals_given_ms: 30_000,
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
            CardPlayer {
                healing: 3_000,
                hps: 3_000.0 / secs,
                absorbed: 3_000,
                absorb_wasted: Some(0),
                ..supported(CASTER, Spec::Fire, 80_000, 0, 20_000)
            },
            supported(AUG, Spec::Augmentation, 60_000, 20_000, 0),
        ],
    );
    c.duration_ms = DURATION_4B;
    c.encounter = Some(wowdps_model::Encounter {
        id: 3148,
        difficulty: 16,
        group_size: 20,
    });
    c
}

fn shield_row(
    spell_id: u32,
    label: &str,
    applied: u64,
    consumed: u64,
    wasted: u64,
    count: u32,
    unknown: u32,
) -> wowdps_model::ShieldRow {
    wowdps_model::ShieldRow {
        spell_id,
        label: label.to_string(),
        applied,
        consumed,
        wasted,
        count,
        unknown,
    }
}

/// Its rows tier: the Priest's two spells (Σ consumed 20 000, Σ wasted
/// 5 000, Σ unknown 2 — the Divine Aegis rows were never seen applied,
/// so that row's identity does not hold and is not asserted) and the
/// Mage's one, plus the meter rows the other views need.
fn shields_rows(id: &str) -> FightRows {
    let mut rows = FightRows {
        id: id.to_string(),
        shields: vec![
            PlayerShields {
                guid: HEALER.to_string(),
                rows: vec![
                    shield_row(17, "Power Word: Shield", 18_000, 15_000, 3_000, 3, 0),
                    shield_row(47753, "Divine Aegis", 0, 5_000, 2_000, 2, 2),
                ],
            },
            PlayerShields {
                guid: CASTER.to_string(),
                rows: vec![shield_row(11426, "Ice Barrier", 3_000, 3_000, 0, 1, 0)],
            },
        ],
        ..FightRows::default()
    };
    rows.views[View::Taken.index()] = vec![taken_row(TANK, "TANK", 28_500, 0, 7)];
    rows.views[View::Healing.index()] = vec![
        taken_row(HEALER, "HEAL", 50_000, 10_000, 9),
        taken_row(CASTER, "MAGE", 3_000, 0, 1),
    ];
    rows
}

/// The card as PR #24 wrote it: no `absorb_wasted` / `shields_unknown`
/// and no derived `absorb_efficiency` on any player line — the keys
/// `crates/proto/tests/history.rs` strips.
fn pre_5_card(card: &Json, id: &str) -> Json {
    card_without(card, id, &PRE_5_KEYS)
}

const PRE_5_KEYS: [&str; 3] = ["absorb_wasted", "shields_unknown", "absorb_efficiency"];

/// The rows file as PR #24 wrote it: no `shields` key (`empty` instead
/// keeps the key and writes `[]` into it — a fight nobody shielded on a
/// post-5 daemon, the all-empty JSON-typing trap).
fn pre_5_rows(rows: &Json, id: &str, empty: bool) -> Json {
    let mut out = match without(rows, &["shields", "id"]) {
        Json::Obj(o) => o,
        _ => panic!("rows"),
    };
    out.push(("id".to_string(), Json::str(id)));
    if empty {
        out.push(("shields".to_string(), Json::Arr(Vec::new())));
    }
    Json::Obj(out)
}

#[test]
fn the_shield_identities_hold_in_sql() {
    let card = shields_card("new");
    let rows = shields_rows("new");
    let (card_json, rows_json) = (card.to_json(), rows.to_json());
    assert!(
        rows_json
            .to_line()
            .contains(r#""shields":[{"guid":"heal","rows":[{"spell_id":17,"#),
        "{rows_json:?}"
    );
    assert!(
        card_json.to_line().contains(r#""absorb_efficiency":0.8,"#),
        "20 000 of 25 000 is exactly 0.8: {card_json:?}"
    );
    assert!(
        card_json
            .to_line()
            .contains(r#""absorb_wasted":null,"shields_unknown":0"#),
        "the tank's waste is unknown: {card_json:?}"
    );
    let tmp = Temp::new("shields-hand");
    write_fight(&tmp.0, "new", &card_json, &rows_json);
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(
        lake.views(),
        [
            "fights",
            "players",
            "role_ranks",
            "rows",
            "taken",
            "shields"
        ]
    );
    let eff = assert_absorb_efficiency_agrees(&lake, std::slice::from_ref(&card), "hand");
    assert_eq!(eff[&("new".to_string(), HEALER.to_string())], Some(0.8));
    assert_eq!(eff[&("new".to_string(), CASTER.to_string())], Some(1.0));
    assert_eq!(eff[&("new".to_string(), TANK.to_string())], None);
    assert_eq!(eff[&("new".to_string(), AUG.to_string())], None);
    assert_shield_identities(&lake, "hand");
    // The columns are TYPED even though half the lake's `absorb_wasted`
    // values are null (the cast), and the shape, spelled out.
    let t = lake.sql("DESCRIBE SELECT * FROM players").unwrap();
    let ty = |col: &str| {
        t.rows
            .iter()
            .find(|r| r.first().and_then(Json::as_str) == Some(col))
            .map(|r| cell_str(&r[1]))
    };
    assert_eq!(ty("absorb_wasted").as_deref(), Some("BIGINT"));
    assert_eq!(ty("absorb_efficiency").as_deref(), Some("DOUBLE"));
    assert_eq!(ty("absorb_efficiency_sql").as_deref(), Some("DOUBLE"));
    assert_eq!(ty("shields_unknown").as_deref(), Some("BIGINT"));
    let t = lake
        .sql(
            "SELECT guid, spell_id, label, applied, consumed, wasted, count, unknown \
             FROM shields ORDER BY guid, consumed DESC",
        )
        .unwrap();
    assert_eq!(
        t.columns,
        [
            "guid", "spell_id", "label", "applied", "consumed", "wasted", "count", "unknown"
        ]
    );
    let got: Vec<String> = t
        .rows
        .iter()
        .map(|r| Json::Arr(r.clone()).to_line())
        .collect();
    assert_eq!(
        got,
        [
            r#"["heal",17,"Power Word: Shield",18000,15000,3000,3,0]"#,
            r#"["heal",47753,"Divine Aegis",0,5000,2000,2,2]"#,
            r#"["mage",11426,"Ice Barrier",3000,3000,0,1,0]"#,
        ]
    );
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_shields").and_then(Json::as_u64),
        Some(0)
    );
    assert_eq!(
        stats.get("rows_without_shields").and_then(Json::as_u64),
        Some(0)
    );

    // A lake whose EVERY `absorb_wasted` is null (a night nobody shielded)
    // — the key is there, DuckDB types it JSON, and `players` must still
    // hand out a typed NULL and count nothing as missing.
    {
        let mut nobody = shields_card("nobody");
        for p in &mut nobody.players {
            p.absorb_wasted = None;
            p.shields_unknown = 0;
            p.absorbed = 0;
        }
        let tmp = Temp::new("shields-allnull");
        write_fight(
            &tmp.0,
            "nobody",
            &nobody.to_json(),
            &pre_5_rows(&rows_json, "nobody", true),
        );
        let lake = Lake::open(&tmp.0).unwrap();
        assert_eq!(
            lake.views(),
            ["fights", "players", "role_ranks", "rows", "taken"]
        );
        let t = lake.sql("DESCRIBE SELECT * FROM players").unwrap();
        let ty = |col: &str| {
            t.rows
                .iter()
                .find(|r| r.first().and_then(Json::as_str) == Some(col))
                .map(|r| cell_str(&r[1]))
        };
        assert_eq!(ty("absorb_wasted").as_deref(), Some("BIGINT"));
        assert_eq!(ty("absorb_efficiency").as_deref(), Some("DOUBLE"));
        assert_eq!(ty("shields_unknown").as_deref(), Some("BIGINT"));
        let eff = assert_absorb_efficiency_agrees(&lake, &[nobody], "allnull");
        assert!(eff.values().all(Option::is_none), "{eff:?}");
        let stats = lake.stats();
        assert_eq!(
            stats.get("cards_without_shields").and_then(Json::as_u64),
            Some(0),
            "a null waste is a stored answer"
        );
        assert_eq!(
            stats.get("rows_without_shields").and_then(Json::as_u64),
            Some(0)
        );
    }
}

/// `role_night` over hand-built pulls: two nights, the second with two
/// pulls where the healer swapped spec (the mode picks the spec played
/// twice; the DPS's two pulls tie on spec, so the smaller id wins), one
/// aborted pull that never counts, and an enemy that is never rostered.
#[test]
fn role_night_folds_the_nights_pulls_in_sql() {
    const DAY: i64 = 86_400_000;
    let secs = DURATION_4B as f64 / 1000.0;
    let mut pulls: Vec<FightCard> = Vec::new();
    let mut at = |id: &str, start: i64, aborted: bool, players: Vec<CardPlayer>| {
        let mut c = shields_card(id);
        c.players = players;
        c.start_utc_ms = start;
        c.aborted = aborted;
        c.success = Some(!aborted);
        pulls.push(c);
    };
    // Night 1: one pull.
    at(
        "n1a",
        3 * DAY + 1_000,
        false,
        vec![
            CardPlayer {
                healing: 40_000,
                hps: 40_000.0 / secs,
                overheal: 10_000,
                absorbed: 10_000,
                absorb_wasted: Some(10_000),
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
            supported(CASTER, Spec::Fire, 80_000, 0, 20_000),
        ],
    );
    // Night 2: two pulls plus an aborted one.
    at(
        "n2a",
        4 * DAY + 5_000,
        false,
        vec![
            CardPlayer {
                taken: 30_000,
                dtps: 30_000.0 / secs,
                mitigated: 15_000,
                prevented: 5_000,
                am_uptime_ms: 30_750,
                ..supported(TANK, Spec::ProtectionWarrior, 30_000, 0, 0)
            },
            CardPlayer {
                healing: 50_000,
                hps: 50_000.0 / secs,
                overheal: 50_000,
                absorbed: 20_000,
                absorb_wasted: Some(5_000),
                externals_given: 2,
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
            supported(CASTER, Spec::Fire, 80_000, 0, 20_000),
            CardPlayer {
                enemy: true,
                ..supported("enemy", Spec::Arms, 90_000, 0, 0)
            },
        ],
    );
    at(
        "n2b",
        4 * DAY + 9_000,
        false,
        vec![
            CardPlayer {
                taken: 10_000,
                dtps: 10_000.0 / secs,
                mitigated: 2_000,
                prevented: 0,
                am_uptime_ms: 0,
                ..supported(TANK, Spec::ProtectionWarrior, 30_000, 0, 0)
            },
            CardPlayer {
                healing: 30_000,
                hps: 30_000.0 / secs,
                overheal: 0,
                absorbed: 6_000,
                absorb_wasted: None,
                shields_unknown: 1,
                externals_given: 1,
                ..supported(HEALER, Spec::HolyPriest, 0, 0, 0)
            },
            supported(CASTER, Spec::FrostMage, 40_000, 0, 0),
        ],
    );
    // The healer back on Discipline, alone (the tank and the caster sat
    // out): the mode is Discipline, 2 of 3.
    at(
        "n2c",
        4 * DAY + 12_000,
        false,
        vec![CardPlayer {
            healing: 20_000,
            hps: 20_000.0 / secs,
            ..supported(HEALER, Spec::Discipline, 0, 0, 0)
        }],
    );
    at(
        "n2x",
        4 * DAY + 20_000,
        true,
        vec![supported(CASTER, Spec::Fire, 999_000, 0, 0)],
    );
    let tmp = Temp::new("role-night");
    for c in &pulls {
        write_fight(&tmp.0, &c.id, &c.to_json(), &shields_rows(&c.id).to_json());
    }
    let lake = Lake::open(&tmp.0).unwrap();
    assert!(lake.role_night(3148, 16, 2 * DAY).unwrap().is_empty());
    assert!(lake.role_night(3149, 16, 4 * DAY).unwrap().is_empty());

    let n1 = lake.role_night(3148, 16, 3 * DAY).unwrap();
    assert_eq!(n1.len(), 2, "{n1:?}");
    assert_eq!(n1[0].guid, HEALER);
    assert_eq!(n1[0].role, Some(Role::Healer));
    assert_eq!(n1[0].spec, Some(Spec::Discipline.id() as u16));
    assert_eq!(n1[0].pulls, 1);
    assert_eq!(n1[0].measure.to_bits(), (40_000.0f64 / secs).to_bits());
    assert_eq!(n1[0].best.to_bits(), n1[0].measure.to_bits());
    assert_eq!(n1[0].overheal_pct, 20.0);
    assert_eq!(n1[0].absorb_efficiency, Some(0.5));
    assert_eq!(n1[1].guid, CASTER);
    assert_eq!(n1[1].role, Some(Role::Dps));
    assert_eq!(n1[1].measure.to_bits(), (60_000.0f64 / secs).to_bits());
    assert_eq!(n1[1].absorb_efficiency, None);

    let n2 = lake.role_night(3148, 16, 4 * DAY).unwrap();
    let guids: Vec<&str> = n2.iter().map(|r| r.guid.as_str()).collect();
    assert_eq!(
        guids,
        [TANK, HEALER, CASTER],
        "tank, healer, dps; no enemy, no aborted pull"
    );
    let tank = &n2[0];
    assert_eq!(tank.role, Some(Role::Tank));
    assert_eq!(tank.pulls, 2);
    // mitigated_pct per pull: 15 000 × 100 / 35 000 and 2 000 × 100 / 10 000.
    let pct_a: f64 = 15_000.0 * 100.0 / 35_000.0;
    let pct_b: f64 = 2_000.0 * 100.0 / 10_000.0;
    assert_eq!(tank.measure.to_bits(), ((pct_a + pct_b) / 2.0).to_bits());
    assert_eq!(tank.best.to_bits(), pct_a.to_bits());
    assert_eq!(tank.taken, 40_000);
    assert_eq!(
        tank.dtps.to_bits(),
        ((30_000.0 / secs + 10_000.0 / secs) / 2.0f64).to_bits()
    );
    assert_eq!(tank.am_uptime_pct, 25.0, "50 % and 0 %");
    let healer = &n2[1];
    assert_eq!(healer.role, Some(Role::Healer));
    assert_eq!(healer.pulls, 3, "the swap: n2a, n2b, n2c");
    assert_eq!(healer.spec, Some(Spec::Discipline.id() as u16), "the mode");
    assert_eq!(healer.externals_given, 3);
    assert_eq!(
        healer.measure.to_bits(),
        ((50_000.0 / secs + 30_000.0 / secs + 20_000.0 / secs) / 3.0f64).to_bits()
    );
    assert_eq!(healer.best.to_bits(), (50_000.0f64 / secs).to_bits());
    assert_eq!(healer.overheal_pct, 50.0 / 3.0, "50 %, 0 %, 0 %");
    // A ratio of sums over the pulls with a KNOWN waste: 20 000 of 25 000
    // — the Holy pull's 6 000 (waste unknown) is outside both sums.
    assert_eq!(healer.absorb_efficiency, Some(0.8));
    let dps = &n2[2];
    assert_eq!(dps.role, Some(Role::Dps));
    assert_eq!(dps.pulls, 2);
    assert_eq!(
        dps.spec,
        Some(Spec::Fire.id() as u16),
        "a tie: the smaller id"
    );
    assert!(Spec::Fire.id() < Spec::FrostMage.id());
    assert_eq!(
        dps.measure.to_bits(),
        ((60_000.0 / secs + 40_000.0 / secs) / 2.0f64).to_bits()
    );
    assert_eq!(dps.best.to_bits(), (60_000.0f64 / secs).to_bits());
}

/// `role_night` picks the mode spec FIRST and folds only the pulls played
/// in its role — in BOTH readers: a hand-built two-night lake written into
/// the store dir before a real daemon starts over it (beside the fixture
/// it tails), against `Lake::role_night` over the same files, every field
/// equal, f64 by bits. Night 2 has a spec-swap player (a tank pull, then
/// two dps pulls — the mode is dps and the tank pull is outside EVERY
/// column), a healer with one specless pull (outside the fold, waste and
/// all), a fully specless player (role `None`, measure 0, all their pulls),
/// an enemy (never rostered) and an aborted pull (never counted).
#[test]
fn role_night_folds_by_the_mode_role_in_both_readers() {
    const DAY: i64 = 86_400_000;
    const SWAP: &str = "swap";
    const NOSPEC: &str = "nospec";
    let secs = DURATION_4B as f64 / 1000.0;
    let tmp = Temp::new("role-night-both");
    let hist = tmp.0.join("history");
    let mut pulls: Vec<FightCard> = Vec::new();
    let mut at = |id: &str, start: i64, aborted: bool, players: Vec<CardPlayer>| {
        let mut c = shields_card(id);
        c.encounter = Some(wowdps_model::Encounter {
            id: 3150,
            difficulty: 16,
            group_size: 20,
        });
        c.players = players;
        c.start_utc_ms = start;
        c.aborted = aborted;
        c.success = Some(!aborted);
        pulls.push(c);
    };
    let specless = |guid: &str, damage: u64| {
        let mut p = supported(guid, Spec::Arms, damage, 0, 0);
        p.spec = None;
        p.class = None;
        p
    };
    // Night 1: the swap player tanks, the healer heals, nospec is specless.
    at(
        "r1a",
        3 * DAY + 1_000,
        false,
        vec![
            CardPlayer {
                taken: 30_000,
                dtps: 30_000.0 / secs,
                mitigated: 15_000,
                prevented: 5_000,
                am_uptime_ms: 30_750,
                ..supported(SWAP, Spec::ProtectionWarrior, 30_000, 0, 0)
            },
            CardPlayer {
                healing: 40_000,
                hps: 40_000.0 / secs,
                overheal: 10_000,
                absorbed: 10_000,
                absorb_wasted: Some(10_000),
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
            specless(NOSPEC, 20_000),
        ],
    );
    // Night 2: the swap player tanks once, then dps twice.
    at(
        "r2a",
        4 * DAY + 5_000,
        false,
        vec![
            CardPlayer {
                taken: 20_000,
                dtps: 20_000.0 / secs,
                mitigated: 5_000,
                prevented: 0,
                am_uptime_ms: 12_300,
                ..supported(SWAP, Spec::ProtectionWarrior, 25_000, 0, 0)
            },
            CardPlayer {
                healing: 50_000,
                hps: 50_000.0 / secs,
                overheal: 50_000,
                absorbed: 20_000,
                absorb_wasted: Some(5_000),
                externals_given: 2,
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
            CardPlayer {
                taken: 4_000,
                dtps: 4_000.0 / secs,
                ..specless(NOSPEC, 20_000)
            },
            CardPlayer {
                enemy: true,
                ..supported("enemy", Spec::Arms, 90_000, 0, 0)
            },
        ],
    );
    at(
        "r2b",
        4 * DAY + 9_000,
        false,
        vec![
            CardPlayer {
                taken: 7_000,
                dtps: 7_000.0 / secs,
                ..supported(SWAP, Spec::Arms, 80_000, 0, 20_000)
            },
            // The healer's specless pull: a healer by every number, no
            // spec — its waste is KNOWN, and still outside the fold.
            CardPlayer {
                healing: 30_000,
                hps: 30_000.0 / secs,
                absorbed: 6_000,
                absorb_wasted: Some(1_000),
                externals_given: 1,
                ..specless(HEALER, 0)
            },
            CardPlayer {
                taken: 5_000,
                dtps: 5_000.0 / secs,
                ..specless(NOSPEC, 30_000)
            },
        ],
    );
    at(
        "r2c",
        4 * DAY + 12_000,
        false,
        vec![
            CardPlayer {
                taken: 9_000,
                dtps: 9_000.0 / secs,
                am_uptime_ms: 6_150,
                ..supported(SWAP, Spec::Arms, 60_000, 10_000, 0)
            },
            CardPlayer {
                healing: 20_000,
                hps: 20_000.0 / secs,
                ..supported(HEALER, Spec::Discipline, 0, 0, 0)
            },
        ],
    );
    at(
        "r2x",
        4 * DAY + 20_000,
        true,
        vec![supported(SWAP, Spec::Arms, 999_000, 0, 0)],
    );
    for c in &pulls {
        write_fight(&hist, &c.id, &c.to_json(), &shields_rows(&c.id).to_json());
    }

    // The daemon opens its store over those files and imports the fixture's
    // kill beside them.
    let (socket, hist_used, _done) = start_over(&tmp, SHIELDS_FIXTURE);
    assert_eq!(hist_used, hist);
    let mut client =
        DaemonClient::over(UnixStream::connect(&socket).unwrap(), ClientKind::Mcp).unwrap();
    wait_for_store(&mut client, 1 + u32::try_from(pulls.len()).unwrap());
    let lake = Lake::open(&hist).expect("lake opens");

    let mut nights = Vec::new();
    for (night, tag) in [(3 * DAY, "night 1"), (4 * DAY, "night 2")] {
        let sql = lake.role_night(3150, 16, night).unwrap();
        let HistoryAnswer::RoleNight { night: n, rows } = ask(
            &mut client,
            next_req(),
            HistoryQuery::RoleNight {
                encounter: 3150,
                difficulty: 16,
                night,
                local_cutover_hour: None,
            },
        ) else {
            panic!("role night");
        };
        assert_eq!(n.day_utc_ms, night, "{tag}");
        assert_role_night_matches(&rows, &sql, tag);
        nights.push((n, rows));
    }
    client.send(&ClientMsg::Shutdown);

    // Night 1: one pull each — the swap player a tank, nospec unroled.
    let (n1, rows1) = &nights[0];
    assert_eq!(n1.pulls, 1);
    let guids: Vec<&str> = rows1.iter().map(|r| r.guid.as_str()).collect();
    assert_eq!(guids, [SWAP, HEALER, NOSPEC], "{rows1:?}");
    assert_eq!(rows1[0].role, Some(Role::Tank));
    assert_eq!(rows1[0].pulls, 1);
    assert_eq!(
        rows1[0].measure.to_bits(),
        (15_000.0f64 * 100.0 / 35_000.0).to_bits()
    );
    assert_eq!(rows1[2].role, None);
    assert_eq!(rows1[2].spec, None);
    assert_eq!(rows1[2].measure, 0.0);

    // Night 2: the aborted pull never counts, the enemy is never rostered.
    let (n2, rows2) = &nights[1];
    assert_eq!(n2.pulls, 3, "{n2:?}");
    let guids: Vec<&str> = rows2.iter().map(|r| r.guid.as_str()).collect();
    assert_eq!(
        guids,
        [HEALER, SWAP, NOSPEC],
        "healer, dps, unroled: {rows2:?}"
    );
    // The swap player: the mode is Arms (2 of 3), so a DPS with TWO pulls —
    // the tank pull's taken, dtps, am uptime and measure all outside.
    let swap = &rows2[1];
    assert_eq!(swap.spec, Some(Spec::Arms.id() as u16));
    assert_eq!(swap.role, Some(Role::Dps));
    assert_eq!(swap.pulls, 2);
    assert_eq!(
        swap.taken, 16_000,
        "7 000 + 9 000, never the tank pull's 20 000"
    );
    let eff_b: f64 = 60_000.0 / secs;
    let eff_c: f64 = 70_000.0 / secs;
    assert_eq!(swap.measure.to_bits(), ((eff_b + eff_c) / 2.0).to_bits());
    assert_eq!(swap.best.to_bits(), eff_c.to_bits());
    assert_eq!(
        swap.am_uptime_pct, 5.0,
        "0 % and 10 %; the tank pull's 20 % outside"
    );
    assert_eq!(
        swap.dtps.to_bits(),
        ((7_000.0 / secs + 9_000.0 / secs) / 2.0f64).to_bits()
    );
    // The healer: the specless pull is outside the fold — pulls 2, its
    // external and its KNOWN waste not counted (20 000 of 25 000, not
    // 26 000 of 27 000).
    let healer = &rows2[0];
    assert_eq!(healer.spec, Some(Spec::Discipline.id() as u16));
    assert_eq!(healer.pulls, 2);
    assert_eq!(healer.externals_given, 2);
    assert_eq!(
        healer.measure.to_bits(),
        ((50_000.0 / secs + 20_000.0 / secs) / 2.0f64).to_bits()
    );
    assert_eq!(healer.absorb_efficiency, Some(0.8));
    // Fully specless: role None, measure 0, ALL their pulls.
    let nospec = &rows2[2];
    assert_eq!(nospec.spec, None);
    assert_eq!(nospec.role, None);
    assert_eq!(nospec.pulls, 2);
    assert_eq!(nospec.measure, 0.0);
    assert_eq!(nospec.best, 0.0);
    assert_eq!(nospec.taken, 9_000);
}

#[test]
fn a_mixed_lake_opens_and_says_which_shield_views_exist() {
    let card = shields_card("new").to_json();
    let rows = shields_rows("new").to_json();
    let ranks_alone = {
        let tmp = Temp::new("shields-alone");
        write_fight(&tmp.0, "new", &card, &rows);
        dps_order(&Lake::open(&tmp.0).unwrap(), "new")
    };
    assert_eq!(
        ranks_alone.len(),
        2,
        "the mage and the aug: {ranks_alone:?}"
    );

    // A pre-5 lake alone: today's view list exactly, the three columns
    // synthesized (NULL / 0 / NULL), no `shields` view — and the same when
    // every file's list is `[]`, except that the rows then DO carry the key.
    for (tag, rows_json, empty) in [
        ("pre5-missing", pre_5_rows(&rows, "old", false), false),
        ("pre5-empty", pre_5_rows(&rows, "old", true), true),
    ] {
        let tmp = Temp::new(tag);
        write_fight(&tmp.0, "old", &pre_5_card(&card, "old"), &rows_json);
        let lake = Lake::open(&tmp.0).unwrap();
        assert_eq!(
            lake.views(),
            ["fights", "players", "role_ranks", "rows", "taken"],
            "{tag}"
        );
        assert!(lake.sql("SELECT * FROM shields").is_err(), "{tag}");
        let t = lake
            .sql(
                "SELECT absorb_wasted, shields_unknown, absorb_efficiency, \
                        absorb_efficiency_sql FROM players ORDER BY guid",
            )
            .unwrap();
        assert_eq!(t.rows.len(), 4, "{tag}");
        for r in &t.rows {
            assert_eq!(
                Json::Arr(r.clone()).to_line(),
                "[null,0,null,null]",
                "{tag}: NULL is the honest pre-5 value, never 0"
            );
        }
        assert_eq!(
            dps_order(&lake, "old"),
            ranks_alone,
            "{tag}: grading unchanged"
        );
        let stats = lake.stats();
        assert_eq!(
            stats.get("cards_without_shields").and_then(Json::as_u64),
            Some(1),
            "{tag}"
        );
        assert_eq!(
            stats.get("rows_without_shields").and_then(Json::as_u64),
            Some(u64::from(!empty)),
            "{tag}: an empty list is a stored answer, a missing key is not"
        );
        // The fixed question still answers, with nothing known.
        let night = lake.role_night(3148, 16, 0).unwrap();
        assert_eq!(night.len(), 4, "{tag}: {night:?}");
        assert!(night.iter().all(|r| r.absorb_efficiency.is_none()), "{tag}");
    }

    // The mixed lake: one post-5 fight beside both older shapes.
    let tmp = Temp::new("shields-mixed");
    write_fight(&tmp.0, "new", &card, &rows);
    write_fight(
        &tmp.0,
        "old",
        &pre_5_card(&card, "old"),
        &pre_5_rows(&rows, "old", false),
    );
    write_fight(
        &tmp.0,
        "empty",
        &pre_5_card(&card, "empty"),
        &pre_5_rows(&rows, "empty", true),
    );
    let lake = Lake::open(&tmp.0).unwrap();
    assert_eq!(
        lake.views(),
        [
            "fights",
            "players",
            "role_ranks",
            "rows",
            "taken",
            "shields"
        ]
    );
    // Only the post-5 fight has any of it; the old cards' scalars are NULL
    // — never an error, never 0.
    let t = lake
        .sql("SELECT fight_id, count(*) FROM shields GROUP BY 1")
        .unwrap();
    assert_eq!(t.rows.len(), 1, "{t:?}");
    assert_eq!(cell_str(&t.rows[0][0]), "new");
    assert_eq!(t.rows[0][1].as_u64(), Some(3));
    let t = lake
        .sql(
            "SELECT fight_id, absorb_wasted, shields_unknown, absorb_efficiency, \
                    absorb_efficiency_sql FROM players WHERE guid = 'heal' ORDER BY fight_id",
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
            r#"["empty",null,null,null,null]"#,
            r#"["new",5000,2,0.8,0.8]"#,
            r#"["old",null,null,null,null]"#,
        ]
    );
    assert_absorb_efficiency_agrees(&lake, &[shields_card("new")], "mixed");
    assert_shield_identities(&lake, "mixed");
    for fight in ["new", "old", "empty"] {
        assert_eq!(
            dps_order(&lake, fight),
            ranks_alone,
            "{fight}: grading unchanged"
        );
    }
    let stats = lake.stats();
    assert_eq!(
        stats.get("cards_without_shields").and_then(Json::as_u64),
        Some(2)
    );
    assert_eq!(
        stats.get("rows_without_shields").and_then(Json::as_u64),
        Some(1),
        "the missing key, not the empty list"
    );
    // The night over all three (they share a start of 0): the healer's
    // efficiency is the one known pull's — 20 000 of 25 000 — the two
    // unknown ones outside both sums; pulls still count all three.
    let night = lake.role_night(3148, 16, 0).unwrap();
    let healer = night.iter().find(|r| r.guid == HEALER).unwrap();
    assert_eq!(healer.pulls, 3);
    assert_eq!(healer.absorb_efficiency, Some(0.8));
}
