//! The wowdps daemon: one process owns tail → index → parse → meter →
//! snapshots for every client. Threads + channels, no async runtime.
//!
//! `run` is the whole daemon; `DaemonOptions` makes every path and grace
//! injectable so the integration suite can run real daemons on temp sockets
//! against the fixtures.

pub mod addon;
pub mod cache;
pub mod config;
pub mod engine;
pub mod game;
pub mod history;
pub mod hub;
pub mod loader;
pub mod mine;
pub mod mock;
pub mod overlay;
pub mod replay;
pub mod server;
pub mod session;
pub mod sources;

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
use wowdps_model::Edition;

use crate::hub::{HubMsg, HubOptions};
use crate::sources::SourceSetup;

/// v49: the editions a daemon follows beside its `source`, one source and
/// one engine each (`sources::Sources`). The default is `source` alone, as
/// `--file`, `--logs` and a folder outside an install are followed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Followed {
    /// The edition `source`'s own folder names: its install product
    /// folder's (`wowdps_core::cli::product_edition`), when it sits in an
    /// install.
    pub source: Option<Edition>,
    /// Each further edition's logs folder in the same install, tailed into
    /// its own engine from when it first holds a log
    /// (`wowdps_core::cli::sibling_logs` over `cli::FOLLOWED`).
    pub more: Vec<(Edition, PathBuf)>,
}

impl Followed {
    /// What a logs folder the config or install discovery names follows:
    /// its own product's edition and, inside an install, the followed
    /// editions' folders beside it.
    pub fn of_logs_dir(dir: &std::path::Path) -> Self {
        Self {
            source: wowdps_core::cli::product_edition(dir),
            more: wowdps_core::cli::sibling_logs(dir, wowdps_core::cli::FOLLOWED),
        }
    }
}

pub struct DaemonOptions {
    pub socket: PathBuf,
    pub lockfile: PathBuf,
    /// What `--file`, `--logs` or the config names: the first source, the
    /// one `Status::source` reports.
    pub source: SourceSpec,
    /// v49: the editions followed beside `source` ([`Followed`]).
    pub editions: Followed,
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
    /// Spawn `<gui_bin> --overlay` when the game appears.
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
        let (source, editions) = match source {
            // `--file` and `--logs`: one source, as they always were.
            Some(s) => (s, Followed::default()),
            // Config first, then install discovery; a daemon with no idea
            // what to tail says so instead of following a made-up path. A
            // folder inside an install names the install: the followed
            // editions' folders beside it are followed too.
            None => {
                let dir = cfg
                    .logs_dir
                    .clone()
                    .or_else(wowdps_core::cli::default_logs_dir)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::NotFound,
                            "no logs_dir configured and no WoW install found — set \
                             logs_dir in ~/.config/wowdps/config.toml, set \
                             WOWDPS_WOW_DIR, or pass --logs",
                        )
                    })?;
                let editions = Followed::of_logs_dir(&dir);
                (SourceSpec::Dir(dir), editions)
            }
        };
        Ok(Self {
            socket,
            lockfile,
            source: source.clone(),
            editions,
            linger,
            idle_grace: Duration::from_secs(10),
            tick: Duration::from_millis(100),
            version: env!("CARGO_PKG_VERSION").to_string(),
            cache_dir: cache::IndexCache::default_dir(),
            game_pattern: Some(cfg.game_process.clone()),
            loader_workers: 2,
            auto_overlay: cfg.auto_overlay,
            overlay_exit_grace: Duration::from_secs(cfg.overlay_exit_grace_secs),
            // `gui_binary` beside this binary, else on PATH, else a path.
            // Spawn failures are captured and reported through `--status`,
            // not guessed at here.
            gui_bin: Some(cfg.gui_bin(std::env::current_exe().ok().as_deref())),
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
                        keep_kills_whole: cfg.history_keep_kills_whole,
                        keep_progression_whole: cfg.history_keep_progression_whole,
                        replay_mb: cfg.history_replay_mb,
                        characters: cfg.history_characters.clone(),
                        cache_dir: cache::IndexCache::default_dir(),
                        // The addon belongs to the install the logs come
                        // from; a `--file` or a stray directory has none.
                        addon_dir: match &source {
                            SourceSpec::Dir(d) => addon::product_dir(d),
                            SourceSpec::File(_) => None,
                        },
                    })
            } else {
                None
            },
        })
    }
}

impl DaemonOptions {
    /// v49: every source followed, in order, with the edition its folder
    /// names: `source` first, then each of `editions.more` — a folder
    /// already followed once is not followed twice.
    pub fn followed(&self) -> Vec<(Option<Edition>, SourceSpec)> {
        let mut out = vec![(self.editions.source, self.source.clone())];
        for (edition, dir) in &self.editions.more {
            let spec = SourceSpec::Dir(dir.clone());
            if !out.iter().any(|(_, s)| same_spec(s, &spec)) {
                out.push((Some(*edition), spec));
            }
        }
        out
    }
}

/// Two specs naming the same file or folder, however spelled.
fn same_spec(a: &SourceSpec, b: &SourceSpec) -> bool {
    a == b || spec_display(a) == spec_display(b)
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

    // v49: one tailer per followed source, in order: the first is `source`.
    let specs = opts.followed();
    let setups: Vec<SourceSetup> = specs
        .iter()
        .map(|(edition, spec)| SourceSetup {
            spec: Some(spec_display(spec)),
            edition: *edition,
        })
        .collect();
    for (i, (_, spec)) in specs.iter().enumerate() {
        spawn_tail(i, spec.clone(), opts.cache_dir.clone(), hub_tx.clone());
    }
    let loader_tx = loader::spawn(hub_tx.clone(), opts.loader_workers);
    let sweeps: Vec<SourceSpec> = specs.iter().map(|(_, spec)| spec.clone()).collect();
    let history_link = match &opts.history {
        Some(h) => history::spawn(h.clone(), loader_tx.clone(), hub_tx.clone(), &sweeps),
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
            sources: setups,
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

/// The only thing in the whole system that opens the log: one per followed
/// source, its events tagged with the source's place (`HubMsg::Tail`).
fn spawn_tail(
    index: usize,
    source: SourceSpec,
    cache_dir: Option<PathBuf>,
    tx: mpsc::Sender<HubMsg>,
) {
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
                if tx.send(HubMsg::Tail(index, ev)).is_err() {
                    return; // hub gone: daemon is shutting down
                }
            }
            if !busy {
                thread::sleep(POLL_INTERVAL);
            }
        }
    });
}
