//! Socket plumbing: the accept loop and the per-connection reader/writer
//! threads. The reader handshakes and forwards decoded `ClientMsg`s to the
//! hub; the writer drains a bounded outbox. Neither ever touches the engine.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, sync_channel};
use std::thread;

use wowdps_proto::wire;
use wowdps_proto::{ClientMsg, DaemonMsg, PROTO_VERSION};

use crate::hub::HubMsg;
use crate::session::OUTBOX;

/// Accept until `stop` is set (wake a blocked accept by connecting once).
pub fn spawn_accept(
    listener: UnixListener,
    hub: Sender<HubMsg>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let ids = AtomicU64::new(1);
        for conn in listener.incoming() {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = conn else { continue };
            let id = ids.fetch_add(1, Ordering::SeqCst);
            let hub = hub.clone();
            thread::spawn(move || connection(id, stream, hub));
        }
    })
}

fn connection(id: u64, stream: UnixStream, hub: Sender<HubMsg>) {
    let mut reader = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Handshake: the first frame must be Hello (right version) — except
    // Shutdown, which always works, or `--stop` could not stop a daemon
    // nobody can talk to.
    let hello = match read_client_msg(&mut reader) {
        Some(ClientMsg::Shutdown) => {
            let _ = hub.send(HubMsg::Client {
                id,
                msg: ClientMsg::Shutdown,
            });
            return;
        }
        Some(ClientMsg::Hello { proto, client, pid }) if proto == PROTO_VERSION => (client, pid),
        Some(_) | None => {
            // The socket path embeds the version, so a mismatch here means a
            // hand-rolled client got the path wrong. Fatal + close is enough.
            let mut w = stream;
            let _ = w.write_all(&DaemonMsg::Fatal("bad handshake".to_string()).encode());
            return;
        }
    };

    let (tx, rx) = sync_channel::<DaemonMsg>(OUTBOX);
    let writer_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    thread::spawn(move || {
        let mut w = writer_stream;
        for msg in rx {
            if w.write_all(&msg.encode()).is_err() {
                break;
            }
        }
        // Outbox closed (session reaped or hub gone): shut the stream so the
        // blocked reader thread wakes with EOF.
        let _ = w.shutdown(std::net::Shutdown::Both);
    });

    if hub
        .send(HubMsg::Connected {
            id,
            kind: hello.0,
            pid: hello.1,
            tx,
        })
        .is_err()
    {
        return;
    }

    while let Some(msg) = read_client_msg(&mut reader) {
        if hub.send(HubMsg::Client { id, msg }).is_err() {
            return;
        }
    }
    let _ = hub.send(HubMsg::Disconnected { id });
}

/// One decoded frame, or `None` on EOF/garbage (either way the connection is
/// done for).
fn read_client_msg(stream: &mut UnixStream) -> Option<ClientMsg> {
    let (tag, body) = wire::read_frame(stream).ok()?;
    ClientMsg::decode(tag, &body).ok()
}
