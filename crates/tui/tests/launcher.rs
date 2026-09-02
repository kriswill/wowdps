//! The `wowdps` dispatcher end to end: the built binary run as a subprocess
//! — usage, the git-style external dispatch, `stop`/`status` against nothing,
//! and a real daemon lifecycle on the fixture — every run sandboxed under a
//! temp dir (`XDG_RUNTIME_DIR` names the socket, so the user's real daemon is
//! never touched; `HOME`/`XDG_*` keep config, cache and state out of the
//! real home; `PATH` holds only what a test put there).

// In tests a panic IS the failure mechanism (clippy.toml's intent). The
// helper fns below sit outside #[test] items, which the allow-*-in-tests
// exemptions no longer cover, so this integration-test crate says it
// explicitly.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use wowdps_proto::{ClientMsg, DaemonMsg, PROTO_VERSION, wire};

const BIN: &str = env!("CARGO_BIN_EXE_wowdps");
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(15);

/// One isolated home for one test: runtime dir (socket + lock), state dir
/// (daemon.log), cache, config, a `bin` dir that is the whole `$PATH`.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        // Short: the socket path must fit a sockaddr_un.
        let root = std::env::temp_dir().join(format!("wdt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["rt", "state", "cache", "config/wowdps", "home", "bin"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        // The daemon must never spawn an overlay from a test, whatever is
        // running on the machine.
        std::fs::write(
            root.join("config/wowdps/config.toml"),
            "auto_overlay = false\ngame_process = \"wowdps-test-no-such-process\"\n",
        )
        .unwrap();
        Sandbox { root }
    }

    fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    fn socket(&self) -> PathBuf {
        self.root
            .join("rt/wowdps")
            .join(format!("wowdps-v{PROTO_VERSION}.sock"))
    }

    fn daemon_log(&self) -> String {
        std::fs::read_to_string(self.root.join("state/wowdps/daemon.log")).unwrap_or_default()
    }

    /// The daemon writes its last log line after it has already unlinked
    /// the socket, so a detached daemon's "clean exit" trails the socket's
    /// disappearance by a moment.
    fn wait_log_contains(&self, needle: &str) -> String {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let log = self.daemon_log();
            if log.contains(needle) {
                return log;
            }
            assert!(
                Instant::now() < deadline,
                "daemon.log never gained {needle:?}: {log:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        // Under `cargo llvm-cov` the children are instrumented too: keep
        // the profile sink so their coverage counts.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            cmd.env("LLVM_PROFILE_FILE", profile);
        }
        cmd.env("PATH", self.bin_dir())
            .env("HOME", self.root.join("home"))
            .env("XDG_RUNTIME_DIR", self.root.join("rt"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .stdin(Stdio::null());
        cmd
    }

    fn wowdps(&self, args: &[&str]) -> Command {
        let mut cmd = self.command(BIN);
        cmd.args(args);
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.wowdps(args).output().expect("run wowdps")
    }

    /// Drop an executable script into the sandbox's `$PATH`.
    fn script(&self, name: &str, body: &str, mode: u32) -> PathBuf {
        let path = self.bin_dir().join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn wait_socket(&self, present: bool) {
        let deadline = Instant::now() + DEADLINE;
        while self.socket().exists() != present {
            assert!(
                Instant::now() < deadline,
                "socket {:?} never became present={present}",
                self.socket()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn canonical_fixture() -> String {
    std::fs::canonicalize(FIXTURE)
        .unwrap()
        .display()
        .to_string()
}

#[test]
fn help_prints_the_usage_and_exits_clean() {
    let sb = Sandbox::new("help");
    for args in [&["--help"][..], &["-h"], &["help"]] {
        let out = sb.run(args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        let text = stdout(&out);
        assert!(text.contains("Usage:"), "{args:?}: {text}");
        assert!(text.contains("wowdps stop"), "{args:?}: {text}");
        assert!(stderr(&out).is_empty(), "{args:?} wrote to stderr");
    }
}

#[test]
fn a_bad_argument_prints_the_error_and_the_usage_with_status_2() {
    let sb = Sandbox::new("badarg");
    let out = sb.run(&["--nonsense"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("unknown argument \"--nonsense\""), "{err}");
    assert!(err.contains("Usage:"), "{err}");
    assert!(stdout(&out).is_empty());

    // The retired flag-mode spelling points at its subcommand.
    let out = sb.run(&["--stop"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("--stop is now a subcommand: wowdps stop"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn stop_and_status_without_a_daemon_say_so() {
    let sb = Sandbox::new("nodaemon");
    let out = sb.run(&["stop"]);
    assert!(out.status.success(), "stop is idempotent");
    assert!(
        stdout(&out).contains("no daemon running"),
        "{}",
        stdout(&out)
    );

    let out = sb.run(&["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("no daemon running"),
        "{}",
        stdout(&out)
    );
    assert!(!sb.socket().exists(), "status/stop never create a socket");
}

#[test]
fn an_unknown_word_runs_the_external_binary_with_its_arguments() {
    let sb = Sandbox::new("external");
    sb.script(
        "wowdps-gen-foo",
        "#!/bin/sh\necho \"gen-foo:$*\"\nexit 7\n",
        0o755,
    );
    let out = sb.run(&["gen-foo", "a", "--file", "b c"]);
    // exec() replaced the process: the script's own exit status comes back.
    assert_eq!(out.status.code(), Some(7), "{}", stderr(&out));
    assert_eq!(stdout(&out), "gen-foo:a --file b c\n");
}

#[test]
fn a_missing_external_binary_is_reported_with_the_usage() {
    let sb = Sandbox::new("noexternal");
    let out = sb.run(&["no-such-cmd-zz", "x"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(
        err.contains("'no-such-cmd-zz' is not a wowdps command (no wowdps-no-such-cmd-zz found)"),
        "{err}"
    );
    assert!(err.contains("Usage:"), "{err}");
}

#[test]
fn an_unrunnable_external_binary_reports_the_exec_error() {
    let sb = Sandbox::new("noexec");
    // Present on $PATH but not executable: exec fails with something other
    // than NotFound, which is a different message.
    sb.script("wowdps-gen-bar", "#!/bin/sh\nexit 0\n", 0o644);
    let out = sb.run(&["gen-bar"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("running wowdps-gen-bar failed"), "{err}");
    assert!(!err.contains("Usage:"), "not a usage error: {err}");
}

/// A socket that accepts and hangs up: the daemon socket exists but the
/// handshake cannot complete.
#[test]
fn status_reports_a_failed_handshake() {
    let sb = Sandbox::new("badhandshake");
    std::fs::create_dir_all(sb.socket().parent().unwrap()).unwrap();
    let listener = UnixListener::bind(sb.socket()).unwrap();
    let server = std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            drop(stream);
        }
    });
    let out = sb.run(&["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("daemon socket exists but the handshake failed"),
        "{}",
        stdout(&out)
    );
    server.join().unwrap();
}

/// A socket that completes the handshake and then goes silent: status is
/// asked and never answered.
#[test]
fn status_reports_a_daemon_that_never_answers() {
    let sb = Sandbox::new("noanswer");
    std::fs::create_dir_all(sb.socket().parent().unwrap()).unwrap();
    let listener = UnixListener::bind(sb.socket()).unwrap();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Read the Hello before answering, and the GetStatus before
            // hanging up, so the client never hits a closed pipe mid-write.
            let (tag, body) = wire::read_frame(&mut stream).unwrap();
            assert!(matches!(
                ClientMsg::decode(tag, &body),
                Ok(ClientMsg::Hello { .. })
            ));
            let ack = DaemonMsg::HelloAck {
                proto: PROTO_VERSION,
                version: "fake".to_string(),
            };
            stream.write_all(&ack.encode()).unwrap();
            let (tag, body) = wire::read_frame(&mut stream).unwrap();
            assert!(matches!(
                ClientMsg::decode(tag, &body),
                Ok(ClientMsg::GetStatus { .. })
            ));
            // Hang up without answering: the client's reader sees EOF and
            // gives up well before its 5 s deadline.
            drop(stream);
        }
    });
    let out = sb.run(&["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("daemon did not answer"),
        "{}",
        stdout(&out)
    );
    server.join().unwrap();
}

#[test]
fn a_daemon_with_nothing_to_tail_fails_setup_and_logs_it() {
    let sb = Sandbox::new("nosource");
    // No --file/--logs, config has no logs_dir, $HOME holds no Steam
    // install: the daemon says so instead of tailing a made-up path.
    let out = sb.run(&["daemon"]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("daemon setup failed"), "{err}");
    assert!(err.contains("no logs_dir configured"), "{err}");
    let log = sb.daemon_log();
    assert!(log.contains("setup failed"), "daemon.log: {log:?}");
    assert!(!sb.socket().exists());
}

/// The foreground daemon on the fixture, driven by the other subcommands:
/// status against it, a second daemon refusing to double up, the gui
/// launcher's source-conflict check, and a clean stop.
#[test]
fn the_foreground_daemon_serves_status_refuses_a_twin_and_stops_clean() {
    let sb = Sandbox::new("daemon");
    let mut daemon = sb
        .wowdps(&["daemon", "--linger", "--file", FIXTURE])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn daemon");
    sb.wait_socket(true);

    let out = sb.run(&["status"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("wowdps daemon: running"), "{text}");
    assert!(
        text.contains(&format!("source:  file:{}", canonical_fixture())),
        "{text}"
    );
    assert!(text.contains("clients: 1"), "{text}");
    assert!(text.contains("game:    not running"), "{text}");
    assert!(text.contains("linger:  yes"), "{text}");
    assert!(text.contains("overlay: Absent"), "{text}");

    // A second foreground daemon finds the lock taken: not an error.
    let twin = sb.run(&["daemon", "--file", FIXTURE]);
    assert!(twin.status.success(), "{}", stderr(&twin));
    assert!(
        stderr(&twin).contains("a daemon is already running"),
        "{}",
        stderr(&twin)
    );

    // The gui launcher checks source agreement before launching anything.
    let other = sb.root.join("home/other.txt");
    std::fs::write(&other, "").unwrap();
    let out = sb.run(&["gui", "--file", other.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(
        err.contains("a daemon is already running against file:"),
        "{err}"
    );
    assert!(err.contains("run `wowdps stop` first"), "{err}");
    // A directory source disagrees just the same.
    let out = sb.run(&["gui", "--logs", sb.root.join("home").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("not logs:"), "{}", stderr(&out));

    // Agreeing on the source, it launches wowdps-gui — which the sandbox's
    // $PATH does not hold. Only when no sibling build of the gui exists
    // beside the binary (that would be preferred, and would open a window).
    let sibling_gui = Path::new(BIN).parent().unwrap().join("wowdps-gui");
    if !sibling_gui.exists() {
        let out = sb.run(&["gui", "--file", FIXTURE]);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).contains("launching wowdps-gui failed"),
            "{}",
            stderr(&out)
        );
    }

    let out = sb.run(&["stop"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("daemon asked to stop"),
        "{}",
        stdout(&out)
    );
    let status = daemon.wait().expect("daemon exits");
    assert!(status.success(), "daemon exit: {status:?}");
    sb.wait_socket(false);
    let log = sb.daemon_log();
    assert!(
        log.contains(&format!("starting on file:{}", canonical_fixture())),
        "daemon.log: {log:?}"
    );
    assert!(log.contains("clean exit"), "daemon.log: {log:?}");
}

/// The TUI client with no daemon running spawns one itself, then fails to
/// take the terminal (there is none: `setsid` detaches the controlling tty
/// and stdin is /dev/null) — the error path every headless launch takes.
/// The daemon it spawned is real, idle-exits on its own, and is stopped here
/// explicitly anyway.
#[test]
fn the_tui_client_spawns_the_daemon_then_reports_a_missing_terminal() {
    let sb = Sandbox::new("tui");
    // The sandbox's $PATH is empty on purpose: resolve setsid on ours.
    let Some(setsid) = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("setsid"))
                .collect::<Vec<_>>()
        })
        .and_then(|cands| cands.into_iter().find(|p| p.is_file()))
    else {
        eprintln!("setsid not available: skipping");
        return;
    };
    let out = sb
        .command(setsid.to_str().unwrap())
        .args(["--wait", BIN, "--file", FIXTURE])
        .output()
        .expect("run under setsid");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.starts_with("wowdps: "), "{err}");
    assert!(
        !err.contains("starting the daemon failed"),
        "the daemon spawn itself must succeed: {err}"
    );
    assert!(sb.socket().exists(), "the client left its daemon running");

    // The self-spawned daemon is a real one: status answers, stop ends it.
    let status = sb.run(&["status"]);
    assert!(status.status.success(), "{}", stdout(&status));
    assert!(
        stdout(&status).contains("linger:  no"),
        "{}",
        stdout(&status)
    );
    let out = sb.run(&["stop"]);
    assert!(out.status.success());
    sb.wait_socket(false);
    sb.wait_log_contains("clean exit");
}
