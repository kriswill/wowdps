//! The overlay supervisor: the daemon owns the overlay's whole life.
//!
//! ```text
//!                  game appears                    game appears
//!    Absent ──────────────────────► Visible ◄──────────────── Hidden
//!       ▲                              │                        │
//!       │      exit_grace elapsed      │ game exits             │
//!       └──────────────────────────────┴───────────────────────►┘
//! ```
//!
//! Layer-shell has no unmap, so "hidden" is a 1×1 click-through surface the
//! overlay process maintains — the daemon cannot hide it, only ask via
//! `SetVisible`. A manual hide by the user sticks until the next *game
//! transition*, so the daemon never fights the user mid-session. Spawn
//! failures are retained and reported through `Status`: a Wayland client
//! launched from a systemd unit with null stdio fails silently otherwise.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wowdps_proto::OverlayState;

/// What the supervisor wants said to the overlay session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    SetVisible(bool),
}

/// A spawned overlay, abstract so the state machine tests never fork.
pub trait OverlayProcess: Send {
    /// Still running? (Reaps on the transition to dead.)
    fn is_alive(&mut self) -> bool;
    fn terminate(&mut self);
    /// Whatever the child said on stderr, for failure reports.
    fn stderr_tail(&mut self) -> String;
}

pub trait OverlaySpawner: Send {
    fn spawn(&mut self) -> Result<Box<dyn OverlayProcess>, String>;
}

/// The real thing: `wowdps-gui --overlay`, stderr retained.
pub struct GuiSpawner {
    pub gui_bin: PathBuf,
}

impl OverlaySpawner for GuiSpawner {
    fn spawn(&mut self) -> Result<Box<dyn OverlayProcess>, String> {
        let mut child = Command::new(&self.gui_bin)
            .arg("--overlay")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawning {}: {e}", self.gui_bin.display()))?;
        let sink = Arc::new(Mutex::new(String::new()));
        let reader = child.stderr.take().map(|mut stderr| {
            let sink = Arc::clone(&sink);
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = stderr.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
                    s.push_str(&String::from_utf8_lossy(buf.get(..n).unwrap_or(&buf)));
                    // Keep the tail; the head of a spewing child is noise.
                    if s.len() > 4096 {
                        let cut = s.len() - 4096;
                        s.drain(..cut);
                    }
                }
            })
        });
        Ok(Box::new(GuiProcess {
            child,
            sink,
            reader,
        }))
    }
}

struct GuiProcess {
    child: Child,
    sink: Arc<Mutex<String>>,
    /// The stderr drain; joined once the child is dead so the failure
    /// report carries everything it said, not just what had landed by the
    /// tick that noticed the exit (that race lost under CI's sandbox).
    reader: Option<JoinHandle<()>>,
}

impl GuiProcess {
    /// The child is gone: wait for its stderr to reach EOF and be sunk.
    /// A dead child's pipe closes at once — nothing else holds the write end.
    fn drain_stderr(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl OverlayProcess for GuiProcess {
    fn is_alive(&mut self) -> bool {
        let alive = matches!(self.child.try_wait(), Ok(None));
        if !alive {
            self.drain_stderr();
        }
        alive
    }

    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.drain_stderr();
    }

    fn stderr_tail(&mut self) -> String {
        self.sink.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

pub struct Supervisor {
    auto: bool,
    exit_grace: Duration,
    spawner: Option<Box<dyn OverlaySpawner>>,
    child: Option<Box<dyn OverlayProcess>>,
    /// The connected overlay session (`Hello.client == Overlay`) — which is
    /// exactly what that field exists for. A user-launched overlay counts
    /// the same as a spawned one: never spawn over it.
    session: Option<u64>,
    game: bool,
    /// The overlay is being hidden pending game return; elapse ⇒ terminate.
    grace_until: Option<Instant>,
    /// The user hid it themselves; holds until the next game transition.
    manual_hidden: bool,
    /// What the supervisor currently wants shown.
    want_visible: bool,
    failure: Option<String>,
}

impl Supervisor {
    pub fn new(auto: bool, exit_grace: Duration, spawner: Option<Box<dyn OverlaySpawner>>) -> Self {
        Self {
            auto,
            exit_grace,
            spawner,
            child: None,
            session: None,
            game: false,
            grace_until: None,
            manual_hidden: false,
            want_visible: true,
            failure: None,
        }
    }

    /// A disabled supervisor (no auto_overlay, nothing to spawn).
    pub fn disabled() -> Self {
        Self::new(false, Duration::ZERO, None)
    }

    pub fn on_game(&mut self, running: bool) -> Vec<Cmd> {
        if self.game == running {
            return Vec::new();
        }
        self.game = running;
        // A game transition is the reset point for a manual hide.
        self.manual_hidden = false;
        if running {
            self.grace_until = None;
            self.want_visible = true;
            if self.session.is_some() {
                // Never spawn a second overlay over an existing one — reveal.
                vec![Cmd::SetVisible(true)]
            } else if self.auto && self.child.is_none() {
                self.spawn();
                Vec::new()
            } else {
                Vec::new()
            }
        } else {
            self.want_visible = false;
            if self.session.is_some() || self.child.is_some() {
                self.grace_until = Some(Instant::now() + self.exit_grace);
            }
            if self.session.is_some() {
                vec![Cmd::SetVisible(false)]
            } else {
                Vec::new()
            }
        }
    }

    fn spawn(&mut self) {
        let Some(spawner) = self.spawner.as_mut() else {
            return;
        };
        match spawner.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.failure = None;
            }
            Err(e) => self.failure = Some(e),
        }
    }

    pub fn on_overlay_connected(&mut self, id: u64) -> Vec<Cmd> {
        self.session = Some(id);
        self.failure = None;
        // Tell a late-connecting overlay the current wish, so one that
        // connects mid-grace (or into a game-less session) starts hidden.
        if !self.want_visible {
            vec![Cmd::SetVisible(false)]
        } else {
            Vec::new()
        }
    }

    pub fn on_overlay_disconnected(&mut self, id: u64) {
        if self.session == Some(id) {
            self.session = None;
        }
    }

    pub fn on_visibility_changed(&mut self, visible: bool) {
        self.manual_hidden = !visible;
    }

    /// Timers and child health. Call at the hub's tick cadence.
    pub fn on_tick(&mut self) -> Vec<Cmd> {
        // A child that died on its own is a failure worth reporting — the
        // classic case being a Wayland client with no WAYLAND_DISPLAY.
        if let Some(child) = self.child.as_mut()
            && !child.is_alive()
        {
            let tail = child.stderr_tail();
            // Only a death without a connected session (or mid-game) is
            // suspicious; a terminate-on-grace already cleared `child`.
            if self.game || self.session.is_none() {
                self.failure = Some(if tail.is_empty() {
                    "overlay exited unexpectedly".to_string()
                } else {
                    tail
                });
            }
            self.child = None;
        }
        if let Some(t) = self.grace_until
            && Instant::now() >= t
        {
            self.grace_until = None;
            if let Some(mut child) = self.child.take() {
                child.terminate();
            }
        }
        Vec::new()
    }

    /// The supervisor holds the daemon open while an overlay child lives or
    /// its exit grace is still counting down.
    pub fn holds_daemon_open(&self) -> bool {
        self.child.is_some() || self.grace_until.is_some()
    }

    /// For `Status`.
    pub fn state(&self) -> OverlayState {
        if let Some(f) = &self.failure {
            return OverlayState::Failed(f.clone());
        }
        if self.session.is_none() && self.child.is_none() {
            return OverlayState::Absent;
        }
        if self.want_visible && !self.manual_hidden {
            OverlayState::Visible
        } else {
            OverlayState::Hidden
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubProcess {
        alive: Arc<Mutex<bool>>,
        terminated: Arc<AtomicUsize>,
        stderr: String,
    }

    impl OverlayProcess for StubProcess {
        fn is_alive(&mut self) -> bool {
            *self.alive.lock().unwrap()
        }
        fn terminate(&mut self) {
            *self.alive.lock().unwrap() = false;
            self.terminated.fetch_add(1, Ordering::SeqCst);
        }
        fn stderr_tail(&mut self) -> String {
            self.stderr.clone()
        }
    }

    struct StubSpawner {
        spawns: Arc<AtomicUsize>,
        terminated: Arc<AtomicUsize>,
        alive: Arc<Mutex<bool>>,
        fail_with: Option<String>,
    }

    impl OverlaySpawner for StubSpawner {
        fn spawn(&mut self) -> Result<Box<dyn OverlayProcess>, String> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            if let Some(e) = &self.fail_with {
                return Err(e.clone());
            }
            *self.alive.lock().unwrap() = true;
            Ok(Box::new(StubProcess {
                alive: Arc::clone(&self.alive),
                terminated: Arc::clone(&self.terminated),
                stderr: String::new(),
            }))
        }
    }

    struct Probes {
        spawns: Arc<AtomicUsize>,
        terminated: Arc<AtomicUsize>,
        alive: Arc<Mutex<bool>>,
    }

    fn supervisor(grace: Duration, fail_with: Option<String>) -> (Supervisor, Probes) {
        let probes = Probes {
            spawns: Arc::new(AtomicUsize::new(0)),
            terminated: Arc::new(AtomicUsize::new(0)),
            alive: Arc::new(Mutex::new(false)),
        };
        let sup = Supervisor::new(
            true,
            grace,
            Some(Box::new(StubSpawner {
                spawns: Arc::clone(&probes.spawns),
                terminated: Arc::clone(&probes.terminated),
                alive: Arc::clone(&probes.alive),
                fail_with,
            })),
        );
        (sup, probes)
    }

    const GRACE: Duration = Duration::from_secs(3600); // effectively "never" in a test

    #[test]
    fn the_game_appearing_spawns_exactly_one_overlay() {
        let (mut sup, probes) = supervisor(GRACE, None);
        assert_eq!(sup.state(), OverlayState::Absent);
        sup.on_game(true);
        sup.on_game(true); // duplicate transition is a no-op
        assert_eq!(probes.spawns.load(Ordering::SeqCst), 1);
        assert_eq!(sup.state(), OverlayState::Visible);
        assert!(sup.holds_daemon_open());
    }

    #[test]
    fn never_spawn_over_a_user_launched_overlay() {
        let (mut sup, probes) = supervisor(GRACE, None);
        let cmds = sup.on_overlay_connected(7);
        assert_eq!(cmds, vec![], "visible wish: nothing to say");
        let cmds = sup.on_game(true);
        assert_eq!(cmds, vec![Cmd::SetVisible(true)], "reveal, don't spawn");
        assert_eq!(probes.spawns.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn game_exit_hides_then_grace_reprieve_reveals() {
        let (mut sup, probes) = supervisor(GRACE, None);
        sup.on_game(true);
        sup.on_overlay_connected(1); // the spawned overlay connects
        let cmds = sup.on_game(false);
        assert_eq!(cmds, vec![Cmd::SetVisible(false)]);
        assert_eq!(sup.state(), OverlayState::Hidden);
        assert!(sup.holds_daemon_open(), "mid-grace holds the daemon open");

        // The game returns before the grace elapses: reveal, no respawn.
        let cmds = sup.on_game(true);
        assert_eq!(cmds, vec![Cmd::SetVisible(true)]);
        assert_eq!(probes.spawns.load(Ordering::SeqCst), 1);
        assert_eq!(probes.terminated.load(Ordering::SeqCst), 0);
        assert_eq!(sup.state(), OverlayState::Visible);
    }

    #[test]
    fn an_elapsed_grace_terminates_the_child() {
        let (mut sup, probes) = supervisor(Duration::ZERO, None);
        sup.on_game(true);
        sup.on_game(false);
        sup.on_tick();
        assert_eq!(probes.terminated.load(Ordering::SeqCst), 1);
        assert!(!sup.holds_daemon_open(), "grace over: free to idle-exit");
    }

    #[test]
    fn a_manual_hide_sticks_until_the_next_game_transition() {
        let (mut sup, _probes) = supervisor(GRACE, None);
        sup.on_game(true);
        sup.on_overlay_connected(1);
        sup.on_visibility_changed(false);
        assert_eq!(sup.state(), OverlayState::Hidden);
        assert_eq!(sup.on_tick(), vec![], "the daemon never fights the user");

        // The next game session resets the preference.
        sup.on_game(false);
        let cmds = sup.on_game(true);
        assert_eq!(cmds, vec![Cmd::SetVisible(true)]);
        assert_eq!(sup.state(), OverlayState::Visible);
    }

    #[test]
    fn spawn_failure_is_reported_not_silent() {
        let (mut sup, probes) = supervisor(GRACE, Some("no WAYLAND_DISPLAY".to_string()));
        sup.on_game(true);
        assert_eq!(probes.spawns.load(Ordering::SeqCst), 1);
        assert_eq!(
            sup.state(),
            OverlayState::Failed("no WAYLAND_DISPLAY".to_string())
        );
        assert!(!sup.holds_daemon_open());
    }

    #[test]
    fn a_child_that_dies_on_its_own_surfaces_its_stderr() {
        let (mut sup, probes) = supervisor(GRACE, None);
        sup.on_game(true);
        *probes.alive.lock().unwrap() = false; // crash
        sup.on_tick();
        assert!(
            matches!(sup.state(), OverlayState::Failed(_)),
            "got {:?}",
            sup.state()
        );
    }

    #[test]
    fn a_late_connecting_overlay_is_told_to_start_hidden() {
        let (mut sup, _probes) = supervisor(GRACE, None);
        sup.on_game(true);
        sup.on_game(false); // grace running, wish = hidden
        let cmds = sup.on_overlay_connected(4);
        assert_eq!(cmds, vec![Cmd::SetVisible(false)]);
    }

    /// The real spawner against real processes: a child's stderr is kept
    /// for the failure report, a living child can be terminated, and a
    /// binary that is not there is a spawn error.
    #[test]
    fn the_gui_spawner_runs_the_binary_keeps_stderr_and_terminates() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("wowdps-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };

        // Dies at once, complaining: the classic no-display failure.
        let dying = script(
            "dying",
            "#!/bin/sh\n[ \"$1\" = --overlay ] || exit 9\necho 'no WAYLAND_DISPLAY' >&2\nexit 3\n",
        );
        let mut spawner = GuiSpawner { gui_bin: dying };
        let mut child = spawner.spawn().expect("spawns");
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.is_alive() || child.stderr_tail().is_empty() {
            assert!(Instant::now() < deadline, "child never exited with output");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            child.stderr_tail().contains("no WAYLAND_DISPLAY"),
            "{:?}",
            child.stderr_tail()
        );

        // A stayer: alive until terminated.
        let staying = script("staying", "#!/bin/sh\nsleep 30\n");
        let mut spawner = GuiSpawner { gui_bin: staying };
        let mut child = spawner.spawn().expect("spawns");
        assert!(child.is_alive());
        child.terminate();
        assert!(!child.is_alive());
        assert_eq!(child.stderr_tail(), "");

        // Nothing there at all.
        let mut spawner = GuiSpawner {
            gui_bin: dir.join("missing"),
        };
        let err = spawner.spawn().err().expect("spawn fails");
        assert!(err.starts_with("spawning "), "{err}");
        assert!(err.contains("missing"), "{err}");

        // The supervisor wires the same thing up through `Status`.
        let mut sup = Supervisor::new(
            true,
            GRACE,
            Some(Box::new(GuiSpawner {
                gui_bin: dir.join("missing"),
            })),
        );
        sup.on_game(true);
        assert!(matches!(sup.state(), OverlayState::Failed(e) if e.contains("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The edges of the state machine the happy paths skip: no spawner at
    /// all, a game returning while the child still lives, and a real
    /// child's stderr surfacing through the supervisor — trimmed to its
    /// tail when it spews.
    #[test]
    fn supervisor_edges_no_spawner_returning_game_and_real_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let mut sup = Supervisor::new(true, GRACE, None);
        assert!(sup.on_game(true).is_empty(), "nothing to spawn with");
        assert_eq!(sup.state(), OverlayState::Absent);

        let (mut sup, probes) = supervisor(GRACE, None);
        sup.on_game(true);
        sup.on_game(false);
        assert!(sup.on_game(true).is_empty());
        assert_eq!(
            probes.spawns.load(Ordering::SeqCst),
            1,
            "the child was still alive"
        );
        assert!(!sup.holds_daemon_open() || sup.state() == OverlayState::Visible);

        let dir = std::env::temp_dir().join(format!("wowdps-overlay-spew-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("spew");
        std::fs::write(
            &script,
            "#!/bin/sh\ni=0\nwhile [ $i -lt 200 ]; do echo 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' >&2; i=$((i+1)); done\necho TAIL-END >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut sup = Supervisor::new(true, GRACE, Some(Box::new(GuiSpawner { gui_bin: script })));
        sup.on_game(true);
        let deadline = Instant::now() + Duration::from_secs(5);
        let failure = loop {
            assert!(
                Instant::now() < deadline,
                "the child never died: {:?}",
                sup.state()
            );
            sup.on_tick();
            if let OverlayState::Failed(f) = sup.state()
                && f.contains("TAIL-END")
            {
                break f;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(failure.len() <= 4096, "kept to the tail: {}", failure.len());
        assert!(!sup.holds_daemon_open());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
