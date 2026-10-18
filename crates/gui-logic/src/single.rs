//! Single-instance guard for the overlay.
//!
//! Launching a new overlay must replace the running one — two surfaces
//! stacked on the game is never right, and an orphaned overlay is worse
//! than clutter: its reconnect loop respawns daemons. The claim is a unix
//! socket in the daemon's runtime dir: a newcomer connects to it (the
//! connection itself is the eviction notice — the incumbent exits on
//! accept), then binds the path and listens for the next taker. The path
//! carries no protocol version on purpose, so an overlay from an older
//! build is still evicted by a newer one.
//!
//! Nothing here writes through a panicking macro. An overlay the daemon
//! spawned has a pipe to that daemon for its stderr; once the daemon is
//! gone a write to it fails, `eprintln!` panics on the failure, and the
//! panic ends the acceptor thread before it exits the process. The evicted
//! overlay then lives on holding no claim, which is how four of them once
//! stacked on the game.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

/// Evict any running overlay and hold the claim for this process, which
/// exits when a newer overlay takes the claim from it. Failure to claim is
/// logged, never fatal — a meter without the guard beats no meter.
pub fn claim_overlay() {
    match wowdps_proto::client::prepare_socket_dir() {
        Ok(dir) => claim_at(&dir.join("overlay.sock"), leave),
        Err(e) => unavailable(&e),
    }
}

/// The eviction: say so, then exit. The notice is best effort, the exit is
/// not.
fn leave() {
    let _ = writeln!(
        std::io::stderr(),
        "wowdps-gui: replaced by a newer overlay, exiting"
    );
    // The workspace bans `exit` elsewhere; leaving at once is the whole
    // point of an eviction.
    #[allow(clippy::exit)]
    std::process::exit(0);
}

fn unavailable(e: &std::io::Error) {
    let _ = writeln!(
        std::io::stderr(),
        "wowdps-gui: overlay takeover claim unavailable: {e}"
    );
}

/// The claim at `path`. `on_evicted` runs on a background thread when a
/// newer overlay connects; for a real overlay it is `leave`.
fn claim_at(path: &Path, on_evicted: impl FnOnce() + Send + 'static) {
    // A live incumbent accepts and exits; a dead one left a stale path that
    // refuses the connection. Either way the path is ours to reclaim — the
    // incumbent never unlinks it (it exits straight from its acceptor), so
    // this cannot remove a newer claimant's socket.
    let _ = UnixStream::connect(path);
    let _ = std::fs::remove_file(path);
    match UnixListener::bind(path) {
        Ok(listener) => {
            std::thread::spawn(move || {
                if listener.accept().is_ok() {
                    on_evicted();
                }
            });
        }
        Err(e) => unavailable(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    /// A test's scratch directory, removed as it goes out of scope, a
    /// failed assertion's unwinding too.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("wowdps-single-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("test dir");
            Self(dir)
        }

        /// Where the test claims.
        fn sock(&self) -> PathBuf {
            self.0.join("overlay.sock")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn wait_for(flag: &AtomicBool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !flag.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "incumbent was never evicted");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_new_claim_evicts_the_incumbent() {
        let dir = Scratch::new("evict");
        let path = dir.sock();
        let evicted = Arc::new(AtomicBool::new(false));
        let flag = evicted.clone();
        claim_at(&path, move || flag.store(true, Ordering::SeqCst));
        claim_at(&path, || {});
        wait_for(&evicted);
    }

    #[test]
    fn a_stale_socket_file_does_not_block_the_claim() {
        let dir = Scratch::new("stale");
        let path = dir.sock();
        // A crashed overlay's leftover: the file exists, nobody listens.
        drop(UnixListener::bind(&path).expect("stale bind"));
        let evicted = Arc::new(AtomicBool::new(false));
        let flag = evicted.clone();
        claim_at(&path, move || flag.store(true, Ordering::SeqCst));
        // The claim is live: a plain connection is the eviction notice.
        UnixStream::connect(&path).expect("claim not listening");
        wait_for(&evicted);
    }

    /// Where the child overlay below claims; set only by the test that
    /// runs it.
    const CHILD_CLAIM: &str = "WOWDPS_SINGLE_TEST_CLAIM";

    /// The child's word that its claim is live. The parent waits for this
    /// file and never probes the socket: a connection IS the eviction.
    fn ready_marker(path: &Path) -> PathBuf {
        path.with_extension("ready")
    }

    /// Fail the test without leaving the child overlay running. The
    /// scratch directory goes with the unwinding.
    fn fail(child: &mut Child, why: &str) -> ! {
        let _ = child.kill();
        let _ = child.wait();
        panic!("{why}");
    }

    /// The real failure, in a real process: an overlay the daemon spawned
    /// writes its eviction notice into a pipe nobody reads once that daemon
    /// is gone. It must still exit, and cleanly.
    #[test]
    fn an_evicted_overlay_exits_with_no_reader_on_its_stderr() {
        // Removed on every way out, a failure's unwinding included; `fail`
        // kills and reaps the child before it panics.
        let dir = Scratch::new("orphan");
        let path = dir.sock();
        let ready = ready_marker(&path);
        let _ = std::fs::remove_file(&ready);
        let mut child = Command::new(std::env::current_exe().expect("the test binary"))
            // `--nocapture`: libtest's output capture would take the notice
            // before it reached the pipe, and the bug with it.
            .args([
                "--exact",
                "single::tests::an_evicted_overlay_child",
                "--ignored",
                "--nocapture",
            ])
            .env(CHILD_CLAIM, &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn the child overlay");
        // The daemon that spawned it is gone: nobody reads its stderr.
        drop(child.stderr.take());

        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            if let Ok(Some(status)) = child.try_wait() {
                panic!("the child overlay exited before it claimed: {status}");
            }
            if Instant::now() >= deadline {
                fail(&mut child, "the child overlay never claimed");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // The eviction.
        let _evict = match UnixStream::connect(&path) {
            Ok(stream) => stream,
            Err(e) => fail(&mut child, &format!("the child holds no claim: {e}")),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => fail(&mut child, "the evicted overlay is still running"),
                Err(e) => fail(&mut child, &format!("waiting on the child: {e}")),
            }
        };
        assert!(status.success(), "the evicted overlay exited with {status}");
    }

    /// The overlay half of the test above, in its own process: it claims
    /// with the real eviction and waits to be evicted. Ignored, so only that
    /// test runs it; run by hand it finds no claim path and returns.
    #[test]
    #[ignore = "the child half of an_evicted_overlay_exits_with_no_reader_on_its_stderr"]
    fn an_evicted_overlay_child() {
        let Some(path) = std::env::var_os(CHILD_CLAIM) else {
            return;
        };
        let path = PathBuf::from(path);
        claim_at(&path, leave);
        std::fs::write(ready_marker(&path), b"").expect("ready marker");
        // The eviction exits the process from the acceptor thread. Still
        // here after this long, it did not.
        std::thread::sleep(Duration::from_secs(30));
        panic!("still running 30 s after claiming");
    }
}
