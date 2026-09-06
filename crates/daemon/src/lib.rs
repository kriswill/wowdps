//! The wowdps daemon: one process owns tail → index → parse → meter →
//! snapshots for every client. Threads + channels, no async runtime.
//!
//! `run` is the whole daemon; `DaemonOptions` makes every path and grace
//! injectable so the integration suite can run real daemons on temp sockets
//! against the fixtures.

pub mod cache;
pub mod config;
pub mod engine;
pub mod game;
pub mod history;
pub mod hub;
pub mod loader;
pub mod mock;
pub mod overlay;
pub mod server;
pub mod session;

use std::io::{self, Seek, SeekFrom};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use wowdps_core::index;
use wowdps_core::tail::{POLL_INTERVAL, ScanFn, SourceSpec, TailEvent, Tailer};

use crate::hub::{HubMsg, HubOptions};

pub struct DaemonOptions {
    pub socket: PathBuf,
    pub lockfile: PathBuf,
    pub source: SourceSpec,
    /// Never idle-exit (what the systemd unit uses).
    pub linger: bool,
    /// How long the daemon outlives its last watching session.
    pub idle_grace: Duration,
    /// Snapshot rebuild cadence — the push-rate cap.
    pub tick: Duration,
    /// Reported in `HelloAck` and `--status`.
    pub version: String,
    /// Where index checkpoints persist; `None` disables the cache.
    pub cache_dir: Option<PathBuf>,
    /// Process pattern for the game watcher; `None` disables it.
    pub game_pattern: Option<String>,
    pub loader_workers: usize,
    /// Spawn `wowdps-gui --overlay` when the game appears.
    pub auto_overlay: bool,
    /// How long a hidden overlay outlives the game before termination.
    pub overlay_exit_grace: Duration,
    /// The gui binary the supervisor spawns; `None` disables spawning (the
    /// supervisor still manages a user-launched overlay's visibility).
    pub gui_bin: Option<PathBuf>,
    /// The history store (roadmap item 1); `None` disables it.
    pub history: Option<history::HistoryOptions>,
}

impl DaemonOptions {
    /// Production defaults: versioned socket in the runtime dir, lockfile
    /// beside it, config-driven source unless overridden.
    pub fn production(
        cfg: &config::Config,
        source: Option<SourceSpec>,
        linger: bool,
    ) -> io::Result<Self> {
        let dir = wowdps_proto::client::prepare_socket_dir()?;
        let socket = wowdps_proto::client::socket_path();
        let lockfile = dir.join(format!("wowdps-v{}.lock", wowdps_proto::PROTO_VERSION));
        Ok(Self {
            socket,
            lockfile,
            source: match source {
                Some(s) => s,
                // Config first, then install discovery; a daemon with no
                // idea what to tail says so instead of following a
                // made-up path.
                None => SourceSpec::Dir(
                    cfg.logs_dir
                        .clone()
                        .or_else(wowdps_core::cli::default_logs_dir)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::NotFound,
                                "no logs_dir configured and no WoW install found — set \
                                 logs_dir in ~/.config/wowdps/config.toml, set \
                                 WOWDPS_WOW_DIR, or pass --logs",
                            )
                        })?,
                ),
            },
            linger,
            idle_grace: Duration::from_secs(10),
            tick: Duration::from_millis(100),
            version: env!("CARGO_PKG_VERSION").to_string(),
            cache_dir: cache::IndexCache::default_dir(),
            game_pattern: Some(cfg.game_process.clone()),
            loader_workers: 2,
            auto_overlay: cfg.auto_overlay,
            overlay_exit_grace: Duration::from_secs(cfg.overlay_exit_grace_secs),
            // Sibling from the same build; PATH otherwise. Spawn failures
            // are captured and reported through `--status`, not guessed at
            // here.
            gui_bin: Some(
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("wowdps-gui")))
                    .filter(|p| p.exists())
                    .unwrap_or_else(|| PathBuf::from("wowdps-gui")),
            ),
            history: if cfg.history_enabled {
                cfg.history_dir
                    .clone()
                    .or_else(history::HistoryOptions::default_dir)
                    .map(|dir| history::HistoryOptions {
                        dir,
                        store_trash: cfg.history_store_trash,
                        keep_per_encounter: cfg.history_keep_per_encounter as usize,
                        keep_details_per_encounter: cfg.history_keep_details_per_encounter as usize,
                        details_min_wipe_secs: cfg.history_details_min_wipe_secs,
                        characters: cfg.history_characters.clone(),
                        cache_dir: cache::IndexCache::default_dir(),
                    })
            } else {
                None
            },
        })
    }
}

/// Run the daemon to completion (`Shutdown`, idle-exit, or bind failure).
/// Holds the lockfile for its whole life; the lock is taken *before* the
/// stale socket is unlinked, which closes the two-racing-daemons TOCTOU.
pub fn run(opts: DaemonOptions) -> io::Result<()> {
    if let Some(parent) = opts.lockfile.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = std::fs::File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&opts.lockfile)?;
    if let Err(e) = lock.try_lock() {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("another daemon holds {}: {e}", opts.lockfile.display()),
        ));
    }

    // Ours now: a leftover socket is stale by definition.
    let _ = std::fs::remove_file(&opts.socket);
    let listener = UnixListener::bind(&opts.socket)?;

    let (hub_tx, hub_rx) = mpsc::channel::<HubMsg>();

    spawn_tail(opts.source.clone(), opts.cache_dir.clone(), hub_tx.clone());
    let loader_tx = loader::spawn(hub_tx.clone(), opts.loader_workers);
    let history_link = match &opts.history {
        Some(h) => history::spawn(
            h.clone(),
            loader_tx.clone(),
            hub_tx.clone(),
            Some(&opts.source),
        ),
        None => history::HistoryLink::disabled(
            "history disabled (history_enabled = false, or no data dir)",
        ),
    };
    if let Some(pattern) = opts.game_pattern.clone() {
        game::spawn_watcher(pattern, hub_tx.clone(), game::POLL);
    }

    let stop = Arc::new(AtomicBool::new(false));
    server::spawn_accept(listener, hub_tx, Arc::clone(&stop));

    let supervisor = overlay::Supervisor::new(
        opts.auto_overlay,
        opts.overlay_exit_grace,
        opts.gui_bin.clone().map(|gui_bin| {
            Box::new(overlay::GuiSpawner { gui_bin }) as Box<dyn overlay::OverlaySpawner>
        }),
    );

    hub::run(
        hub_rx,
        loader_tx,
        supervisor,
        HubOptions {
            linger: opts.linger,
            idle_grace: opts.idle_grace,
            tick: opts.tick,
            version: opts.version,
            source_spec: Some(spec_display(&opts.source)),
        },
        history_link,
    );

    // Wind down: wake the accept loop so it observes `stop`, then remove the
    // socket. The lock releases when `lock` drops.
    stop.store(true, Ordering::SeqCst);
    let _ = UnixStream::connect(&opts.socket);
    let _ = std::fs::remove_file(&opts.socket);
    drop(lock);
    Ok(())
}

/// Canonical, comparable rendering of a source spec. The client builds the
/// same string from its `--file`/`--logs` flags; inequality is the
/// "daemon is following something else" hard error.
pub fn spec_display(spec: &SourceSpec) -> String {
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    match spec {
        SourceSpec::File(p) => format!("file:{}", canon(p).display()),
        SourceSpec::Dir(d) => format!("logs:{}", canon(d).display()),
    }
}

/// The only thing in the whole system that opens the log.
fn spawn_tail(source: SourceSpec, cache_dir: Option<PathBuf>, tx: mpsc::Sender<HubMsg>) {
    thread::spawn(move || {
        let scan: ScanFn = match cache_dir.map(cache::IndexCache::new) {
            Some(cache) => Box::new(move |path, file| cache.scan_file(path, file)),
            None => Box::new(|_, file| {
                let _ = file.seek(SeekFrom::Start(0));
                index::scan(file)
            }),
        };
        let mut tailer = Tailer::with_scan(source, scan);
        loop {
            let events = tailer.poll();
            let busy = events
                .iter()
                .any(|e| matches!(e, TailEvent::Lines(l) if !l.is_empty()));
            for ev in events {
                if tx.send(HubMsg::Tail(ev)).is_err() {
                    return; // hub gone: daemon is shutting down
                }
            }
            if !busy {
                thread::sleep(POLL_INTERVAL);
            }
        }
    });
}
