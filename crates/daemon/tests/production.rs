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
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::tail::SourceSpec;
use wowdps_daemon::config::Config;
use wowdps_daemon::{DaemonOptions, run, spec_display};
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
        for sub in ["rt", "cache", "home"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", root.join("rt"));
            std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
            std::env::set_var("HOME", root.join("home"));
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
    let cfg = Config::default();

    // Nothing configured, nothing installed: a clear error, no made-up path.
    let err = DaemonOptions::production(&cfg, None, false)
        .err()
        .expect("no source");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(err.to_string().contains("no logs_dir configured"), "{err}");

    // The config's logs_dir.
    let configured = Config {
        logs_dir: Some(root.join("logs")),
        game_process: "custom.exe".to_string(),
        auto_overlay: false,
        overlay_exit_grace_secs: 5,
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
    assert!(opts.gui_bin.is_some(), "a spawner is always configured");
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
    while !socket.exists() {
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
