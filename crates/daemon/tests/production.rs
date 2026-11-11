//! `DaemonOptions::production` and a production-shaped `run`: the socket
//! and lockfile land in `$XDG_RUNTIME_DIR`, the source comes from the
//! override, the config, or install discovery (in that order), the index
//! cache goes under `$XDG_CACHE_HOME`, and the game watcher + gui spawner
//! are wired — all under a sandboxed environment, never the user's.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::tail::SourceSpec;
use wowdps_daemon::config::Config;
use wowdps_daemon::{DaemonOptions, run, spec_display};
use wowdps_model::Edition;
use wowdps_proto::{ClientKind, ClientMsg, DaemonClient, DaemonMsg, socket_path};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(15);

/// One process, one sandbox: every test here shares it, so the env is set
/// once and the tests are serialized through this lock.
fn sandbox() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("wdp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["rt", "cache", "home", "data"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", root.join("rt"));
            std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
            std::env::set_var("HOME", root.join("home"));
            // The history store resolves its lake from here: without it the
            // start-up sweep would import the fixture into the user's own.
            std::env::set_var("XDG_DATA_HOME", root.join("data"));
            std::env::remove_var("WOWDPS_WOW_DIR");
        }
        root
    })
    .clone()
}

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_source_comes_from_the_override_the_config_or_nowhere() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = sandbox();

    // Nothing configured, nothing installed: a clear error, no made-up
    // path. Off macOS only: discovery also scans /Applications, which a
    // machine with the game installed fills (as core's cli tests note),
    // making the error path unreachable there.
    #[cfg(not(target_os = "macos"))]
    {
        let cfg = Config::default();
        let err = DaemonOptions::production(&cfg, None, false)
            .err()
            .expect("no source");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("no logs_dir configured"), "{err}");
    }

    // The config's logs_dir.
    let configured = Config {
        logs_dir: Some(root.join("logs")),
        game_process: "custom.exe".to_string(),
        auto_overlay: false,
        overlay_exit_grace_secs: 5,
        gui_binary: "/opt/wowdps/bin/wowdps-gui-dev".to_string(),
        ..Config::default()
    };
    let opts = DaemonOptions::production(&configured, None, true).expect("configured");
    assert_eq!(opts.source, SourceSpec::Dir(root.join("logs")));
    assert!(opts.linger);
    assert_eq!(opts.socket, socket_path());
    assert_eq!(opts.socket.parent(), Some(root.join("rt/wowdps").as_path()));
    assert_eq!(opts.lockfile.parent(), opts.socket.parent());
    assert_eq!(opts.cache_dir, Some(root.join("cache/wowdps/index")));
    assert_eq!(opts.game_pattern.as_deref(), Some("custom.exe"));
    assert!(!opts.auto_overlay);
    assert_eq!(opts.overlay_exit_grace, Duration::from_secs(5));
    assert_eq!(
        opts.gui_bin,
        Some(PathBuf::from("/opt/wowdps/bin/wowdps-gui-dev")),
        "a spawner is always configured, from gui_binary"
    );
    assert_eq!(opts.version, env!("CARGO_PKG_VERSION"));

    // An explicit override beats the config.
    let opts = DaemonOptions::production(
        &configured,
        Some(SourceSpec::File(PathBuf::from(FIXTURE))),
        false,
    )
    .expect("override");
    assert_eq!(opts.source, SourceSpec::File(PathBuf::from(FIXTURE)));
    assert!(!opts.linger);
    assert_eq!(opts.editions, Default::default(), "--file: one source");
    // A folder outside an install: followed alone.
    let opts = DaemonOptions::production(&configured, None, true).expect("configured");
    assert_eq!(opts.editions, Default::default());
    assert_eq!(opts.followed().len(), 1);
}

const FOREVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");

/// A WoW install at `wow` holding the logs folder of each of `products`.
fn install(wow: &Path, products: &[&str]) {
    let _ = std::fs::remove_dir_all(wow);
    std::fs::create_dir_all(wow.join("Data/data")).unwrap();
    std::fs::write(wow.join(".build.info"), "x").unwrap();
    for product in products {
        std::fs::create_dir_all(wow.join(product).join("Logs")).unwrap();
    }
}

/// `$WOWDPS_WOW_DIR` naming `wow` while it lives: install discovery's
/// override, cleared again however the test ends.
struct WowDir;

impl WowDir {
    fn set(wow: &Path) -> Self {
        unsafe { std::env::set_var("WOWDPS_WOW_DIR", wow) };
        WowDir
    }
}

impl Drop for WowDir {
    fn drop(&mut self) {
        unsafe { std::env::remove_var("WOWDPS_WOW_DIR") };
    }
}

/// v49: a `logs_dir` inside an install names the install — its product
/// folder names the source's edition — and the other followed product's
/// folder is followed beside it: Retail's and Forever's, whichever the
/// config names, and with no config the install discovery finds, Retail's
/// first. A Retail-only install follows Forever's folder too, though it
/// does not exist yet. `--logs` is one source whatever it names.
#[test]
fn an_install_is_followed_edition_by_edition() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = sandbox();
    let wow = root.join("home/Games/World of Warcraft");
    install(&wow, &["_retail_", "_forever_"]);
    let retail = wow.join("_retail_/Logs");
    let forever = wow.join("_forever_/Logs");
    for (logs, own, beside) in [
        (&retail, Edition::Retail, (Edition::Forever, &forever)),
        (&forever, Edition::Forever, (Edition::Retail, &retail)),
    ] {
        let cfg = Config {
            logs_dir: Some(logs.clone()),
            ..Config::default()
        };
        let opts = DaemonOptions::production(&cfg, None, true).expect("configured");
        assert_eq!(opts.source, SourceSpec::Dir(logs.clone()));
        assert_eq!(opts.editions.source, Some(own));
        assert_eq!(opts.editions.more, vec![(beside.0, beside.1.clone())]);
        assert_eq!(opts.followed().len(), 2, "{own:?}");
    }

    // No logs_dir: discovery names the install, and Retail's folder leads.
    let opts = {
        let _wow = WowDir::set(&wow);
        DaemonOptions::production(&Config::default(), None, true).expect("discovered")
    };
    assert_eq!(opts.source, SourceSpec::Dir(retail.clone()));
    assert_eq!(
        opts.followed(),
        vec![
            (Some(Edition::Retail), SourceSpec::Dir(retail.clone())),
            (Some(Edition::Forever), SourceSpec::Dir(forever.clone())),
        ]
    );

    // A Retail-only install: Forever's folder is followed from when it
    // first holds a log.
    let only = root.join("home/Retail Only/World of Warcraft");
    install(&only, &["_retail_"]);
    let opts = {
        let _wow = WowDir::set(&only);
        DaemonOptions::production(&Config::default(), None, true).expect("discovered")
    };
    assert!(!only.join("_forever_").exists());
    assert_eq!(
        opts.editions.more,
        vec![(Edition::Forever, only.join("_forever_/Logs"))]
    );

    let opts = DaemonOptions::production(
        &Config::default(),
        Some(SourceSpec::Dir(retail.clone())),
        true,
    )
    .unwrap();
    assert_eq!(opts.editions, Default::default(), "--logs: one source");
    let _ = std::fs::remove_dir_all(&wow);
    let _ = std::fs::remove_dir_all(root.join("home/Retail Only"));
}

/// Stops the daemon on `.0` when dropped, so a failing test never leaves
/// one holding the sandbox's socket for the next.
struct Stop(PathBuf);

impl Drop for Stop {
    fn drop(&mut self) {
        if let Ok(mut s) = UnixStream::connect(&self.0) {
            let _ = std::io::Write::write_all(&mut s, &ClientMsg::Shutdown.encode());
        }
    }
}

/// A production-shaped daemon over `opts` with its own store and index
/// cache under `dir`, the watcher looking for no process; the socket.
fn start(
    mut opts: DaemonOptions,
    dir: &Path,
) -> (thread::JoinHandle<std::io::Result<()>>, PathBuf, Stop) {
    opts.idle_grace = Duration::from_secs(30);
    opts.tick = Duration::from_millis(20);
    opts.cache_dir = Some(dir.join("cache"));
    if let Some(h) = opts.history.as_mut() {
        h.dir = dir.join("history");
        h.cache_dir = Some(dir.join("cache"));
    }
    let socket = opts.socket.clone();
    let _ = std::fs::remove_file(&socket);
    let daemon = thread::spawn(move || run(opts));
    let deadline = Instant::now() + DEADLINE;
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "daemon never bound {socket:?}");
        thread::sleep(Duration::from_millis(5));
    }
    let stop = Stop(socket.clone());
    (daemon, socket, stop)
}

/// Status, asked until `done` holds of it.
fn status_until(
    client: &mut DaemonClient,
    what: &str,
    done: impl Fn(&DaemonMsg) -> bool,
) -> DaemonMsg {
    let deadline = Instant::now() + DEADLINE;
    let mut last = None;
    let mut req_id = 1;
    while Instant::now() < deadline {
        client.send(&ClientMsg::GetStatus { req_id });
        req_id += 1;
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until {
            for msg in client.poll() {
                if matches!(msg, DaemonMsg::Status { .. }) {
                    if done(&msg) {
                        return msg;
                    }
                    last = Some(msg);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    panic!("{what}: {last:#?}");
}

/// Each listed source's (edition, log), in `Status` order.
fn sources_of(msg: &DaemonMsg) -> Vec<(Option<Edition>, Option<String>)> {
    match msg {
        DaemonMsg::Status { editions, .. } => editions
            .iter()
            .map(|e| (e.edition, e.log.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

fn fights_of(msg: &DaemonMsg) -> Option<(u32, u32)> {
    match msg {
        DaemonMsg::Status { history, .. } => Some((history.fights, history.importing)),
        _ => None,
    }
}

/// Index checkpoints in a cache folder.
fn checkpoints(cache: &Path) -> usize {
    std::fs::read_dir(cache).map_or(0, |d| {
        d.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
            .count()
    })
}

/// v49 end to end: a daemon of the default config over an install that
/// holds both editions' logs — found by install discovery, no logs_dir —
/// follows both: `Status` lists each edition's source with its log, the
/// store files each log's fights by its edition, and the index cache keeps
/// its checkpoints per edition: ten Retail logs leave eight of Retail's,
/// and Forever's one is never evicted by them.
#[test]
fn a_default_daemon_follows_both_editions_of_an_install() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = sandbox();
    let wow = root.join("home/Both/World of Warcraft");
    install(&wow, &["_retail_", "_forever_"]);
    let retail = wow.join("_retail_/Logs");
    let forever = wow.join("_forever_/Logs");
    // Ten copies of one Retail log, older than Forever's: one log's fights
    // to the store, ten checkpoints offered to the cache.
    let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    for i in 0..10 {
        let log = retail.join(format!("WoWCombatLog-0727260{i}_200000.txt"));
        std::fs::copy(FIXTURE, &log).unwrap();
        let at = hour_ago + Duration::from_secs(i);
        std::fs::File::options()
            .write(true)
            .open(&log)
            .and_then(|f| f.set_modified(at))
            .unwrap();
    }
    std::fs::copy(FOREVER, forever.join("WoWCombatLog-110126_200000.txt")).unwrap();
    let cfg = Config {
        game_process: "wowdps-no-such-process-5e1c".to_string(),
        auto_overlay: false,
        ..Config::default()
    };
    let opts = {
        let _wow = WowDir::set(&wow);
        DaemonOptions::production(&cfg, None, true).expect("discovered")
    };
    let dir = root.join("both");
    let _ = std::fs::remove_dir_all(&dir);
    let (daemon, socket, _stop) = start(opts, &dir);

    let stream = UnixStream::connect(&socket).expect("connect");
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).expect("handshake");
    let status = status_until(&mut client, "both sources listed", |m| {
        sources_of(m).len() == 2 && sources_of(m).iter().all(|(e, _)| e.is_some())
    });
    assert_eq!(
        sources_of(&status),
        [
            (
                Some(Edition::Retail),
                Some("WoWCombatLog-07272609_200000.txt".to_string())
            ),
            (
                Some(Edition::Forever),
                Some("WoWCombatLog-110126_200000.txt".to_string())
            ),
        ]
    );
    // Sample's two bosses, once however many copies, and its visit's Σ,
    // closed in the nine copies not tailed; Forever's one boss.
    status_until(&mut client, "every log stored", |m| {
        fights_of(m) == Some((4, 0))
    });
    let cache = dir.join("cache");
    let deadline = Instant::now() + DEADLINE;
    while checkpoints(&cache) < 9 {
        assert!(
            Instant::now() < deadline,
            "checkpoints: {}",
            checkpoints(&cache)
        );
        thread::sleep(Duration::from_millis(10));
    }
    client.send(&ClientMsg::Shutdown);
    assert!(daemon.join().expect("daemon thread").is_ok());
    assert_eq!(
        checkpoints(&cache),
        9,
        "eight of Retail's and Forever's one"
    );
    let store = wowdps_daemon::history::Store::open(
        wowdps_daemon::history::DirBackend::new(dir.join("history")),
        Default::default(),
    );
    let mut editions: Vec<Edition> = store.cards().iter().map(|c| c.edition()).collect();
    editions.sort();
    assert_eq!(
        editions,
        [
            Edition::Retail,
            Edition::Retail,
            Edition::Retail,
            Edition::Forever
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&wow);
}

/// v49: a Retail-only install's daemon lists Retail's source alone, as a
/// daemon of one source did — Forever's folder missing, then there but
/// empty — and once Forever's first log appears in it, it is followed and
/// stored with no restart.
#[test]
fn forevers_folder_is_followed_from_its_first_log() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = sandbox();
    let wow = root.join("home/Later/World of Warcraft");
    install(&wow, &["_retail_"]);
    std::fs::copy(
        FIXTURE,
        wow.join("_retail_/Logs/WoWCombatLog-072726_200000.txt"),
    )
    .unwrap();
    let cfg = Config {
        logs_dir: Some(wow.join("_retail_/Logs")),
        game_process: "wowdps-no-such-process-5e1c".to_string(),
        auto_overlay: false,
        ..Config::default()
    };
    let opts = DaemonOptions::production(&cfg, None, true).expect("configured");
    assert_eq!(opts.followed().len(), 2);
    let dir = root.join("later");
    let _ = std::fs::remove_dir_all(&dir);
    let (daemon, socket, _stop) = start(opts, &dir);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).expect("handshake");
    let retail_only = [(
        Some(Edition::Retail),
        Some("WoWCombatLog-072726_200000.txt".to_string()),
    )];
    let status = status_until(&mut client, "Retail stored", |m| {
        fights_of(m) == Some((2, 0)) && sources_of(m).first().is_some_and(|s| s.0.is_some())
    });
    assert_eq!(sources_of(&status), retail_only, "Forever's folder missing");

    // The folder appears, empty: still Retail's alone.
    let forever = wow.join("_forever_/Logs");
    std::fs::create_dir_all(&forever).unwrap();
    thread::sleep(Duration::from_millis(500));
    let status = status_until(&mut client, "Retail's alone", |_| true);
    assert_eq!(sources_of(&status), retail_only, "Forever's folder empty");

    // Its first log: followed beside Retail's, its fight stored.
    std::fs::copy(FOREVER, forever.join("WoWCombatLog-110126_200000.txt")).unwrap();
    let status = status_until(&mut client, "Forever followed", |m| {
        sources_of(m).len() == 2 && fights_of(m) == Some((3, 0))
    });
    assert_eq!(
        sources_of(&status).get(1),
        Some(&(
            Some(Edition::Forever),
            Some("WoWCombatLog-110126_200000.txt".to_string())
        ))
    );
    client.send(&ClientMsg::Shutdown);
    assert!(daemon.join().expect("daemon thread").is_ok());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&wow);
}

/// A production-shaped daemon — index cache on, game watcher on, a gui
/// spawner that can never be asked to spawn — answers status and stops.
#[test]
fn a_production_daemon_runs_with_the_cache_and_watcher_wired() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = sandbox();
    let cfg = Config {
        logs_dir: None,
        game_process: "wowdps-no-such-process-5e1c".to_string(),
        auto_overlay: false,
        overlay_exit_grace_secs: 1,
        ..Config::default()
    };
    let mut opts =
        DaemonOptions::production(&cfg, Some(SourceSpec::File(PathBuf::from(FIXTURE))), false)
            .expect("options");
    opts.idle_grace = Duration::from_secs(30);
    opts.tick = Duration::from_millis(20);
    let socket = opts.socket.clone();
    let cache_dir = opts.cache_dir.clone().expect("cache dir");
    let want_source = spec_display(&opts.source);
    let _ = std::fs::remove_file(&socket);

    let daemon = thread::spawn(move || run(opts));
    let deadline = Instant::now() + DEADLINE;
    loop {
        // The socket's vnode appears at bind(); listen() follows. A connect
        // in that gap is refused, so readiness is a connect, not exists().
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "daemon never bound {socket:?}");
        thread::sleep(Duration::from_millis(5));
    }

    let stream = UnixStream::connect(&socket).expect("connect");
    let mut client = DaemonClient::over(stream, ClientKind::Mcp).expect("handshake");
    client.send(&ClientMsg::GetStatus { req_id: 7 });
    let status = loop {
        assert!(Instant::now() < deadline, "no status");
        if let Some(s) = client
            .poll()
            .into_iter()
            .find(|m| matches!(m, DaemonMsg::Status { .. }))
        {
            break s;
        }
        thread::sleep(Duration::from_millis(5));
    };
    match status {
        DaemonMsg::Status {
            req_id,
            source,
            clients,
            linger,
            ..
        } => {
            assert_eq!(req_id, 7);
            assert_eq!(source.as_deref(), Some(want_source.as_str()));
            assert_eq!(clients, 1);
            assert!(!linger);
        }
        other => panic!("{other:?}"),
    }

    // The tail thread scanned through the cache: a checkpoint was written.
    let has_checkpoint = || {
        std::fs::read_dir(&cache_dir).is_ok_and(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "bin"))
        })
    };
    while !has_checkpoint() {
        assert!(
            Instant::now() < deadline,
            "no index checkpoint under {cache_dir:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }

    client.send(&ClientMsg::Shutdown);
    let result = daemon.join().expect("daemon thread");
    assert!(result.is_ok(), "{result:?}");
    assert!(!socket.exists(), "the socket is removed on the way out");
    assert!(root.join("rt/wowdps").exists());
}
