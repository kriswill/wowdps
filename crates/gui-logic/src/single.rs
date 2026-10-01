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

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

/// Evict any running overlay and hold the claim for this process.
/// `on_evicted` runs on a background thread when a newer overlay takes the
/// claim from us; it should exit the process. Failure to claim is logged,
/// never fatal — a meter without the guard beats no meter.
pub fn claim_overlay(on_evicted: impl FnOnce() + Send + 'static) {
    match wowdps_proto::client::prepare_socket_dir() {
        Ok(dir) => claim_at(&dir.join("overlay.sock"), on_evicted),
        Err(e) => eprintln!("wowdps-gui: overlay takeover claim unavailable: {e}"),
    }
}

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
        Err(e) => eprintln!("wowdps-gui: overlay takeover claim unavailable: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wowdps-single-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        dir.join("overlay.sock")
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
        let path = scratch("evict");
        let evicted = Arc::new(AtomicBool::new(false));
        let flag = evicted.clone();
        claim_at(&path, move || flag.store(true, Ordering::SeqCst));
        claim_at(&path, || {});
        wait_for(&evicted);
    }

    #[test]
    fn a_stale_socket_file_does_not_block_the_claim() {
        let path = scratch("stale");
        // A crashed overlay's leftover: the file exists, nobody listens.
        drop(UnixListener::bind(&path).expect("stale bind"));
        let evicted = Arc::new(AtomicBool::new(false));
        let flag = evicted.clone();
        claim_at(&path, move || flag.store(true, Ordering::SeqCst));
        // The claim is live: a plain connection is the eviction notice.
        UnixStream::connect(&path).expect("claim not listening");
        wait_for(&evicted);
    }
}
