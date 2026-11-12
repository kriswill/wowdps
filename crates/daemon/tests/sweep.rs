//! The start-up sweep reads only what is new: real daemons on temp sockets
//! over folders of synthetic logs, restarted. A log the history read whole
//! is passed over while its file (inode, length, modified time) and the
//! fights it left in the store are unchanged; any change reads it again.
//!
//! "Read" is told by the log's index checkpoint, which every scan of it
//! rewrites (a new file by rename, so a new inode), and by a pull moved in
//! place: a read stores it as a new fight.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use wowdps_core::tail::SourceSpec;
use wowdps_daemon::history::{DirBackend, HistoryOptions, Retention, Store};
use wowdps_daemon::{DaemonOptions, Followed, run, swept};
use wowdps_model::Edition;
use wowdps_proto::history::{FightCard, FightKind, fnv64};
use wowdps_proto::{ClientKind, ClientMsg, DaemonClient, DaemonMsg, HistoryAnswer, HistoryStatus};

const DEADLINE: Duration = Duration::from_secs(20);
const RETAIL: u8 = 1;
const FOREVER: u8 = 18;

// ---- scaffolding ------------------------------------------------------------

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("wowdps-sweep-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A log of one boss on `month/day` in edition `project_id`, so two logs
/// never share an identity: a zone-in, then each pull (seconds, kill) ten
/// minutes apart (the second at 20:10), one player hitting the boss once
/// per pull, then trash to `pad` bytes: a hit every hundredth of a second
/// from 23:00 on, one open trash segment the store keeps nothing of (trash
/// is off).
fn log_text(month: u32, day: u32, project_id: u8, pulls: &[(u32, bool)], pad: usize) -> String {
    let date = format!("{month}/{day}/2026");
    let mut out = format!(
        "{date} 20:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,{project_id}\n\
         {date} 20:00:01.000-4  ZONE_CHANGE,2769,\"Hall of Echoes\",15\n"
    );
    for (i, (dur_s, kill)) in pulls.iter().enumerate() {
        let h = 20 + (i / 6) as u32;
        let m = ((i % 6) * 10) as u32;
        let ts = |sec: u32| format!("{date} {h:02}:{:02}:{:02}.000-4", m + sec / 60, sec % 60);
        out.push_str(&format!(
            "{}  ENCOUNTER_START,3130,\"The Ashen Warden\",15,20,2769\n",
            ts(0)
        ));
        out.push_str(&format!(
            "{}  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"The Ashen Warden\",0xa48,0x0,116,\"Frostbolt\",16,1000,1000,0,0,0,0,0,nil,nil\n",
            ts(1),
        ));
        out.push_str(&format!(
            "{}  ENCOUNTER_END,3130,\"The Ashen Warden\",15,20,{},{}\n",
            ts(*dur_s),
            u8::from(*kill),
            dur_s * 1000
        ));
    }
    let mut hit = 0u32;
    while out.len() < pad {
        let (s, ms) = (hit / 100, (hit % 100) * 10);
        out.push_str(&format!(
            "{date} 23:{:02}:{:02}.{ms:03}-4  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-7,\"Trash\",0xa48,0x0,116,\"Frostbolt\",16,1000,1000,0,0,0,0,0,nil,nil\n",
            s / 60 % 60,
            s % 60,
        ));
        hit += 1;
    }
    out
}

/// Three pulls: two wipes and a kill.
const NIGHT: &[(u32, bool)] = &[(30, false), (40, false), (90, true)];

/// Write a log of `NIGHT` named for its day, its mtime `age` seconds before
/// now (the tailed log is the youngest).
fn write_log(dir: &Path, day: u32, project_id: u8, age: u64) -> PathBuf {
    let path = dir.join(format!("WoWCombatLog-07{day:02}26.txt"));
    std::fs::write(&path, log_text(7, day, project_id, NIGHT, 0)).unwrap();
    set_mtime(&path, SystemTime::now() - Duration::from_secs(age));
    path
}

fn options(
    tmp: &Temp,
    source: SourceSpec,
    history_dir: PathBuf,
    cache: Option<PathBuf>,
) -> DaemonOptions {
    DaemonOptions {
        socket: tmp.join("test.sock"),
        lockfile: tmp.join("test.lock"),
        source,
        editions: Default::default(),
        linger: true,
        idle_grace: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        version: "test".to_string(),
        cache_dir: cache.clone(),
        game_pattern: None,
        loader_workers: 2,
        auto_overlay: false,
        overlay_exit_grace: Duration::ZERO,
        gui_bin: None,
        history: Some(HistoryOptions {
            dir: history_dir,
            store_trash: false,
            keep_per_encounter: 200,
            keep_details_per_encounter: 10,
            details_min_wipe_secs: 60,
            keep_kills_whole: true,
            keep_progression_whole: true,
            replay_mb: 4096,
            characters: Vec::new(),
            cache_dir: cache,
            addon_dir: None,
        }),
    }
}

struct Daemon {
    socket: PathBuf,
    done: mpsc::Receiver<std::io::Result<()>>,
}

fn start(opts: DaemonOptions) -> Daemon {
    let socket = opts.socket.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(opts));
    });
    let deadline = Instant::now() + DEADLINE;
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "daemon never bound {socket:?}");
        thread::sleep(Duration::from_millis(5));
    }
    Daemon { socket, done: rx }
}

fn stop(d: Daemon) {
    if let Ok(stream) = UnixStream::connect(&d.socket)
        && let Ok(mut c) = DaemonClient::over(stream, ClientKind::Mcp)
    {
        c.send(&ClientMsg::Shutdown);
    }
    let _ = d.done.recv_timeout(DEADLINE);
}

/// Poll `Status` until nothing is importing and the fight count (`want`,
/// else whatever it settles at) has held for `quiet`: the status and when
/// the quiet began.
fn settle(
    socket: &Path,
    want: Option<u32>,
    quiet: Duration,
    deadline: Duration,
) -> (HistoryStatus, Instant) {
    let stream = UnixStream::connect(socket).unwrap();
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).unwrap();
    let until = Instant::now() + deadline;
    let mut since: Option<(Instant, u32)> = None;
    let mut last = None;
    let mut req_id = 1;
    while Instant::now() < until {
        client.send(&ClientMsg::GetStatus { req_id });
        req_id += 1;
        thread::sleep(Duration::from_millis(20));
        for msg in client.poll() {
            let DaemonMsg::Status { history, .. } = msg else {
                continue;
            };
            let calm = history.importing == 0 && want.is_none_or(|w| history.fights == w);
            since = match since {
                Some((at, n)) if calm && n == history.fights => Some((at, n)),
                _ if calm => Some((Instant::now(), history.fights)),
                _ => None,
            };
            if let Some((at, _)) = since
                && at.elapsed() >= quiet
            {
                return (history, at);
            }
            last = Some(history);
        }
    }
    panic!("the store never settled at {want:?}: {last:?}");
}

/// Start a daemon, wait for the store to hold `fights` with nothing
/// importing, and stop it.
fn run_until(opts: DaemonOptions, fights: u32) {
    let d = start(opts);
    settle(
        &d.socket,
        Some(fights),
        Duration::from_millis(300),
        DEADLINE,
    );
    stop(d);
}

/// Bytes this process has read through read(2) and its kin, socket reads
/// included (`/proc/self/io`'s `rchar`).
fn rchar() -> u64 {
    std::fs::read_to_string("/proc/self/io")
        .ok()
        .and_then(|io| {
            io.lines()
                .find_map(|l| l.strip_prefix("rchar: "))
                .and_then(|n| n.trim().parse().ok())
        })
        .unwrap_or(0)
}

fn set_mtime(path: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

/// The inode of `log`'s index checkpoint: every scan of the log writes a
/// new one (by rename), so an unchanged inode means the log went unread.
fn checkpoint(cache: &Path, log: &Path) -> u64 {
    let entry = cache.join(format!(
        "{:016x}.bin",
        fnv64(log.to_string_lossy().as_bytes())
    ));
    std::fs::metadata(&entry)
        .unwrap_or_else(|e| panic!("{entry:?}: {e}"))
        .ino()
}

/// Move the second pull (20:10) to minute `to` of the hour, in place, the
/// file's length kept: a read of the log stores it as a new fight.
fn move_second_pull(log: &Path, from: u32, to: u32) {
    let text = std::fs::read_to_string(log).unwrap();
    let moved = text.replace(&format!(" 20:{from}:"), &format!(" 20:{to}:"));
    assert_ne!(text, moved);
    assert_eq!(text.len(), moved.len());
    std::fs::write(log, moved).unwrap();
}

fn cards(hist: &Path) -> Vec<FightCard> {
    Store::open(DirBackend::new(hist.to_path_buf()), Retention::default())
        .cards()
        .to_vec()
}

fn client(socket: &Path) -> DaemonClient {
    DaemonClient::over(UnixStream::connect(socket).unwrap(), ClientKind::Mcp).unwrap()
}

/// Send one history request and wait for its answer.
fn ask(socket: &Path, msg: ClientMsg) -> HistoryAnswer {
    let mut c = client(socket);
    c.send(&msg);
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        for m in c.poll() {
            if let DaemonMsg::History { answer, .. } = m {
                return answer;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("no answer to {msg:?}");
}

fn regrade(socket: &Path, fight_id: &str) -> u32 {
    match ask(
        socket,
        ClientMsg::Regrade {
            req_id: 7,
            fight_id: Some(fight_id.to_string()),
            encounter: None,
            difficulty: None,
            kind: None,
            edition: None,
        },
    ) {
        HistoryAnswer::Regraded { queued } => queued,
        other => panic!("{other:?}"),
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A folder of three Retail logs: two older (days 1, 2), one tailed (day
/// 3), with a store and a cache of its own.
struct Folder {
    tmp: Temp,
    logs: PathBuf,
    hist: PathBuf,
    cache: PathBuf,
    older: [PathBuf; 2],
}

impl Folder {
    fn new(tag: &str) -> Self {
        let tmp = Temp::new(tag);
        let logs = tmp.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let older = [
            write_log(&logs, 1, RETAIL, 300),
            write_log(&logs, 2, RETAIL, 200),
        ];
        write_log(&logs, 3, RETAIL, 100);
        Self {
            hist: tmp.join("history"),
            cache: tmp.join("cache"),
            logs,
            tmp,
            older,
        }
    }

    fn options(&self) -> DaemonOptions {
        options(
            &self.tmp,
            SourceSpec::Dir(self.logs.clone()),
            self.hist.clone(),
            Some(self.cache.clone()),
        )
    }
}

/// Each older log: three pulls and its visit's Σ; the tailed one its pulls
/// alone (its open visit is live).
const FOLDER_FIGHTS: u32 = 4 + 4 + 3;

// ---- the rule ---------------------------------------------------------------

/// The whole rule on one older log, a restart per case: unchanged (its
/// bytes rewritten in place, its length and mtime as they were) it goes
/// unread; touched, replaced by another file, grown or truncated — each
/// alone, the rest kept — it is read again, and the moved pull is stored.
#[test]
fn an_older_log_is_read_again_only_when_its_file_changed() {
    let f = Folder::new("rule");
    run_until(f.options(), FOLDER_FIGHTS);
    let [a, b] = f.older.clone();
    let entries = swept::read(&f.cache).unwrap().1;
    let mut listed: Vec<PathBuf> = entries.iter().map(|(p, _)| p.clone()).collect();
    listed.sort();
    assert_eq!(
        listed,
        [swept::key_of(&a), swept::key_of(&b)],
        "the older logs read whole; the tailed one never"
    );
    assert!(entries.iter().all(|(_, e)| e.held.len() == 4));

    let mut fights = FOLDER_FIGHTS;
    let cases = ["unchanged", "touched", "replaced", "grown", "truncated"];
    for (minute, case) in (10..).zip(cases) {
        let at = mtime(&a);
        let (ckpt_a, ckpt_b) = (checkpoint(&f.cache, &a), checkpoint(&f.cache, &b));
        move_second_pull(&a, minute, minute + 1);
        match case {
            "unchanged" | "touched" => {}
            "replaced" => {
                let text = std::fs::read(&a).unwrap();
                let other = f.logs.join("other.tmp");
                std::fs::write(&other, text).unwrap();
                std::fs::rename(&other, &a).unwrap();
            }
            "grown" => {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new().append(true).open(&a).unwrap();
                // A cast: a line the index reads and stores nothing of.
                file.write_all(
                    b"7/1/2026 23:59:00.000-4  SPELL_CAST_SUCCESS,Player-1-A,\"Ana-Realm\",0x511,0x0,0000000000000000,nil,0x80000000,0x80000000,116,\"Frostbolt\",0x10\n",
                )
                .unwrap();
            }
            "truncated" => {
                let len = std::fs::metadata(&a).unwrap().len();
                std::fs::File::options()
                    .write(true)
                    .open(&a)
                    .unwrap()
                    .set_len(len - 10)
                    .unwrap();
            }
            other => panic!("{other}"),
        }
        set_mtime(
            &a,
            if case == "touched" {
                at + Duration::from_secs(1)
            } else {
                at
            },
        );
        let read = case != "unchanged";
        fights += u32::from(read);
        run_until(f.options(), fights);
        assert_eq!(
            checkpoint(&f.cache, &a) != ckpt_a,
            read,
            "{case}: the log read again or not"
        );
        assert_eq!(checkpoint(&f.cache, &b), ckpt_b, "{case}: the other never");
        assert_eq!(cards(&f.hist).len() as u32, fights, "{case}");
    }
}

/// The store is half the rule: a wiped store reads every log again, and a
/// card deleted by hand reads its log again and is stored back.
#[test]
fn a_fight_missing_from_the_store_reads_its_log_again() {
    let f = Folder::new("wiped");
    run_until(f.options(), FOLDER_FIGHTS);
    let [a, b] = f.older.clone();

    let gone = cards(&f.hist)
        .into_iter()
        .find(|c| c.log == wowdps_daemon::history::LogFacts::read(&b).id)
        .unwrap();
    std::fs::remove_file(f.hist.join("fights").join(format!("{}.json", gone.id))).unwrap();
    let (ckpt_a, ckpt_b) = (checkpoint(&f.cache, &a), checkpoint(&f.cache, &b));
    run_until(f.options(), FOLDER_FIGHTS);
    assert!(
        cards(&f.hist).iter().any(|c| c.id == gone.id),
        "stored back"
    );
    assert_ne!(checkpoint(&f.cache, &b), ckpt_b, "its log read");
    assert_eq!(checkpoint(&f.cache, &a), ckpt_a, "the other not");

    std::fs::remove_dir_all(&f.hist).unwrap();
    let ckpt_a = checkpoint(&f.cache, &a);
    run_until(f.options(), FOLDER_FIGHTS);
    assert_ne!(checkpoint(&f.cache, &a), ckpt_a, "a wiped store reads all");
    assert_eq!(cards(&f.hist).len() as u32, FOLDER_FIGHTS);
}

/// A fight retention evicted stays evicted: its log is not read again to
/// store it and evict it once more on every start. A cap raised in the
/// config moves the rules, and the next start reads every log again,
/// bringing back what now fits.
#[test]
fn an_evicted_fight_reads_no_log_until_the_rules_move() {
    let tmp = Temp::new("evicted");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    // Four wipes in an older log; the tailed log has no pull, so no owner
    // is inferred and nothing protects a wipe.
    let a = logs.join("WoWCombatLog-070126.txt");
    std::fs::write(
        &a,
        log_text(
            7,
            1,
            RETAIL,
            &[(30, false), (40, false), (50, false), (60, false)],
            0,
        ),
    )
    .unwrap();
    set_mtime(&a, SystemTime::now() - Duration::from_secs(200));
    let tailed = logs.join("WoWCombatLog-070226.txt");
    std::fs::write(&tailed, log_text(7, 2, RETAIL, &[], 0)).unwrap();
    let (hist, cache) = (tmp.join("history"), tmp.join("cache"));
    let opts = |cap: usize| {
        let mut o = options(
            &tmp,
            SourceSpec::Dir(logs.clone()),
            hist.clone(),
            Some(cache.clone()),
        );
        let h = o.history.as_mut().unwrap();
        h.keep_per_encounter = cap;
        h.keep_progression_whole = false;
        h.keep_kills_whole = false;
        o
    };
    // The newest wipe and the visit's Σ.
    run_until(opts(1), 2);
    let entry = swept::read(&cache).unwrap().1;
    assert_eq!(entry.len(), 1);
    assert_eq!(entry[0].1.held.len(), 2, "the evicted three held no more");

    let fights_dir = mtime(&hist.join("fights"));
    let ckpt = checkpoint(&cache, &a);
    run_until(opts(1), 2);
    assert_eq!(checkpoint(&cache, &a), ckpt, "the log went unread");
    assert_eq!(
        mtime(&hist.join("fights")),
        fights_dir,
        "no card written to be evicted again"
    );

    run_until(opts(10), 5);
    assert_ne!(checkpoint(&cache, &a), ckpt, "a raised cap reads it again");
}

/// A log the daemon tailed last run is read as an older one once a newer
/// log has appeared while it was down — its open visit's Σ stored — and is
/// passed over on the start after.
#[test]
fn the_log_tailed_last_run_is_read_whole_once_it_is_older() {
    let f = Folder::new("tailed");
    run_until(f.options(), FOLDER_FIGHTS);
    let was_tailed = f.logs.join("WoWCombatLog-070326.txt");
    assert!(
        swept::read(&f.cache)
            .unwrap()
            .1
            .iter()
            .all(|(p, _)| *p != swept::key_of(&was_tailed))
    );
    write_log(&f.logs, 4, RETAIL, 0);
    // Day 3's Σ and day 4's pulls.
    run_until(f.options(), FOLDER_FIGHTS + 1 + 3);
    let ckpt = checkpoint(&f.cache, &was_tailed);
    run_until(f.options(), FOLDER_FIGHTS + 1 + 3);
    assert_eq!(checkpoint(&f.cache, &was_tailed), ckpt);
}

/// A log deleted from disk leaves the manifest; the others stay passed
/// over.
#[test]
fn a_deleted_log_drops_out_of_the_manifest() {
    let f = Folder::new("deleted");
    run_until(f.options(), FOLDER_FIGHTS);
    let [a, b] = f.older.clone();
    std::fs::remove_file(&b).unwrap();
    let ckpt = checkpoint(&f.cache, &a);
    run_until(f.options(), FOLDER_FIGHTS);
    assert_eq!(checkpoint(&f.cache, &a), ckpt);
    let listed: Vec<PathBuf> = swept::read(&f.cache)
        .unwrap()
        .1
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(listed, [swept::key_of(&a)]);
}

// ---- what needs a log after a restart ---------------------------------------------

/// A restart passes over the older logs, and a regrade of an old fight
/// still finds its log and rewrites the card.
#[test]
fn a_regrade_after_a_restart_finds_a_log_the_sweep_passed_over() {
    let f = Folder::new("regrade");
    run_until(f.options(), FOLDER_FIGHTS);
    let a = &f.older[0];
    let log = wowdps_daemon::history::LogFacts::read(a).id;
    let old = cards(&f.hist)
        .into_iter()
        .find(|c| c.log == log && c.kind == FightKind::Encounter)
        .unwrap();
    let rows = f.hist.join("rows").join(format!("{}.json", old.id));
    std::fs::remove_file(&rows).unwrap();
    let ckpt = checkpoint(&f.cache, a);
    let d = start(f.options());
    settle(
        &d.socket,
        Some(FOLDER_FIGHTS),
        Duration::from_millis(300),
        DEADLINE,
    );
    assert_eq!(checkpoint(&f.cache, a), ckpt, "the sweep passed over it");
    assert_eq!(regrade(&d.socket, &old.id), 1, "its log is found");
    wait_for("the card was not rewritten", || rows.exists());
    stop(d);
}

/// A log imported by hand from outside the followed folders is found
/// again after a restart, by the manifest: a regrade of its fight reads it.
#[test]
fn an_imported_log_is_found_again_after_a_restart() {
    let f = Folder::new("imported");
    let elsewhere = f.tmp.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let x = elsewhere.join("WoWCombatLog-070926.txt");
    std::fs::write(&x, log_text(7, 9, RETAIL, NIGHT, 0)).unwrap();
    let d = start(f.options());
    settle(
        &d.socket,
        Some(FOLDER_FIGHTS),
        Duration::from_millis(300),
        DEADLINE,
    );
    let answer = ask(
        &d.socket,
        ClientMsg::ImportLog {
            req_id: 3,
            path: x.to_string_lossy().into_owned(),
        },
    );
    assert!(
        matches!(answer, HistoryAnswer::Imported { queued: 1 }),
        "{answer:?}"
    );
    settle(
        &d.socket,
        Some(FOLDER_FIGHTS + 4),
        Duration::from_millis(300),
        DEADLINE,
    );
    stop(d);

    let log = wowdps_daemon::history::LogFacts::read(&x).id;
    let fight = cards(&f.hist)
        .into_iter()
        .find(|c| c.log == log && c.kind == FightKind::Encounter)
        .unwrap();
    let rows = f.hist.join("rows").join(format!("{}.json", fight.id));
    std::fs::remove_file(&rows).unwrap();
    let d = start(f.options());
    settle(
        &d.socket,
        Some(FOLDER_FIGHTS + 4),
        Duration::from_millis(300),
        DEADLINE,
    );
    assert_eq!(
        regrade(&d.socket, &fight.id),
        1,
        "found outside the folders"
    );
    wait_for("the imported card was not rewritten", || rows.exists());
    stop(d);
}

// ---- two editions ---------------------------------------------------------------

/// Each edition's folder is swept apart and remembered in one manifest: a
/// restart reads neither's older log, and a change to one reads that one
/// alone.
#[test]
fn two_editions_each_read_only_what_is_new() {
    let tmp = Temp::new("editions");
    let (retail, forever) = (tmp.join("retail"), tmp.join("forever"));
    std::fs::create_dir_all(&retail).unwrap();
    std::fs::create_dir_all(&forever).unwrap();
    let ra = write_log(&retail, 1, RETAIL, 300);
    write_log(&retail, 2, RETAIL, 200);
    let fa = write_log(&forever, 11, FOREVER, 300);
    write_log(&forever, 12, FOREVER, 100);
    let (hist, cache) = (tmp.join("history"), tmp.join("cache"));
    let opts = || {
        let mut o = options(
            &tmp,
            SourceSpec::Dir(retail.clone()),
            hist.clone(),
            Some(cache.clone()),
        );
        o.editions = Followed {
            source: Some(Edition::Retail),
            more: vec![(Edition::Forever, forever.clone())],
        };
        o
    };
    // Each edition: the older log's pulls and Σ, the tailed one's pulls.
    let fights = 2 * (4 + 3);
    run_until(opts(), fights);
    let entries = swept::read(&cache).unwrap().1;
    let mut editions: Vec<u8> = entries.iter().map(|(_, e)| e.project_id).collect();
    editions.sort();
    assert_eq!(editions, [RETAIL, FOREVER]);

    let (cr, cf) = (checkpoint(&cache, &ra), checkpoint(&cache, &fa));
    for log in [&ra, &fa] {
        let at = mtime(log);
        move_second_pull(log, 10, 11);
        set_mtime(log, at);
    }
    run_until(opts(), fights);
    assert_eq!((checkpoint(&cache, &ra), checkpoint(&cache, &fa)), (cr, cf));

    let at = mtime(&fa);
    set_mtime(&fa, at + Duration::from_secs(1));
    run_until(opts(), fights + 1);
    assert_eq!(checkpoint(&cache, &ra), cr, "Retail's untouched log unread");
    assert_ne!(checkpoint(&cache, &fa), cf, "Forever's touched log read");
    let moved = cards(&hist)
        .into_iter()
        .filter(|c| c.kind == FightKind::Encounter && c.edition() == Edition::Forever)
        .count();
    assert_eq!(moved, 3 + 3 + 1, "the moved Forever pull, stored");
}

// ---- the measurement ------------------------------------------------------------

/// What three starts over one folder of forty logs cost: the wall time until
/// the store settles and the bytes the process read. Run alone (the bytes
/// are the process's):
/// `cargo test --release -p wowdps-daemon --test sweep -- --ignored --nocapture measured`
/// `WOWDPS_SWEEP_MB` sizes each log (5 MiB by default). With
/// `WOWDPS_SWEEP_COLD=1` each start begins with the logs out of the page
/// cache (GNU dd's `nocache`, which asks the kernel to drop them) and
/// reports how much of them the start brought back in (`fincore`).
#[test]
#[ignore = "a measurement: prints the time and bytes of three starts over forty logs"]
fn measured_three_starts_over_forty_logs() {
    let mb: usize = std::env::var("WOWDPS_SWEEP_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let tmp = Temp::new("measured");
    let logs = tmp.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let base = SystemTime::now() - Duration::from_secs(3600);
    let mut total = 0u64;
    for k in 0..40u32 {
        let (month, day) = (5 + k / 28, 1 + k % 28);
        let text = log_text(month, day, RETAIL, NIGHT, mb << 20);
        let path = logs.join(format!("WoWCombatLog-{month:02}{day:02}26.txt"));
        std::fs::write(&path, &text).unwrap();
        set_mtime(&path, base + Duration::from_secs(60 * u64::from(k)));
        total += text.len() as u64;
    }
    let hist = tmp.join("history");
    let cache = tmp.join("cache");
    let cold = std::env::var_os("WOWDPS_SWEEP_COLD").is_some();
    let paths: Vec<PathBuf> = std::fs::read_dir(&logs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    let mut fights = None;
    for run_no in 1..=3 {
        if cold {
            uncache(&paths);
        }
        let before = rchar();
        let started = Instant::now();
        let d = start(options(
            &tmp,
            SourceSpec::Dir(logs.clone()),
            hist.clone(),
            Some(cache.clone()),
        ));
        let (st, settled) = settle(
            &d.socket,
            fights,
            Duration::from_millis(if fights.is_none() { 1500 } else { 400 }),
            Duration::from_secs(600),
        );
        let wall = settled - started;
        stop(d);
        let read = rchar() - before;
        fights = Some(st.fights);
        let cached = if cold {
            format!(
                "; {} MiB of the logs brought into the page cache",
                resident(&paths) >> 20
            )
        } else {
            String::new()
        };
        eprintln!(
            "start {run_no}: settled in {wall:.2?} at {} fights; {} MiB read ({:.2} of the folder's {} MiB){cached}",
            st.fights,
            read >> 20,
            read as f64 / total as f64,
            total >> 20,
        );
    }
}

/// Drop `paths` from the page cache: written back first, then GNU dd's
/// `nocache` (`posix_fadvise` DONTNEED over each whole file).
fn uncache(paths: &[PathBuf]) {
    for p in paths {
        std::fs::File::open(p).unwrap().sync_all().unwrap();
        let ok = std::process::Command::new("dd")
            .arg(format!("if={}", p.display()))
            .args(["iflag=nocache", "count=0", "status=none"])
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "dd could not drop {p:?} from the page cache");
    }
    assert_eq!(resident(paths), 0, "the logs left the page cache");
}

/// The bytes of `paths` in the page cache (`fincore`).
fn resident(paths: &[PathBuf]) -> u64 {
    let out = std::process::Command::new("fincore")
        .args(["--bytes", "--raw", "--noheadings", "--output", "RES"])
        .args(paths)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u64>().ok())
        .sum()
}
