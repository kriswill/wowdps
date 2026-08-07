//! Client-side plumbing: the socket path (single source of truth, embeds
//! `PROTO_VERSION` so a new client never sees an old daemon's socket),
//! daemon self-spawning, and `DaemonClient` — one connection, a reader
//! thread, and the snapshot-coalescing inbox.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::msg::{ClientKind, ClientMsg, Cursor, DaemonMsg, PROTO_VERSION};
use crate::wire;

/// The directory the socket (and the daemon's lockfile) live in:
/// `$XDG_RUNTIME_DIR/wowdps`, else `/tmp/wowdps-<uid>`.
pub fn socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("wowdps"),
        _ => PathBuf::from(format!("/tmp/wowdps-{}", uid())),
    }
}

/// `<socket_dir>/wowdps-v<PROTO_VERSION>.sock`.
pub fn socket_path() -> PathBuf {
    socket_dir().join(format!("wowdps-v{PROTO_VERSION}.sock"))
}

/// Create the socket dir 0700 and verify it is really ours — a pre-existing
/// dir owned by someone else (a squatted `/tmp` name) is refused, not used.
pub fn prepare_socket_dir() -> io::Result<PathBuf> {
    let dir = socket_dir();
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::metadata(&dir)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} is not a directory", dir.display()),
        ));
    }
    if meta.uid() != uid() {
        return Err(io::Error::other(format!(
            "{} is owned by uid {}, not us",
            dir.display(),
            meta.uid()
        )));
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

/// Our uid, without libc: `/proc/self` is owned by the process's uid.
/// Linux-only, like the rest of the daemon.
fn uid() -> u32 {
    std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0)
}

/// The source a client forwards when it has to spawn the daemon itself.
/// (Proto cannot name core's `SourceSpec`; the daemon re-parses the flags.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceArg {
    File(PathBuf),
    Logs(PathBuf),
}

/// Connect to the daemon, spawning one if none is listening. Failure is
/// fatal to the caller — there is no embedded fallback.
pub fn ensure_daemon(daemon_bin: &Path, source: Option<&SourceArg>) -> io::Result<UnixStream> {
    let path = socket_path();
    if let Ok(stream) = UnixStream::connect(&path) {
        return Ok(stream);
    }
    prepare_socket_dir()?;

    let mut cmd = std::process::Command::new(daemon_bin);
    cmd.arg("--daemon");
    match source {
        Some(SourceArg::File(p)) => {
            cmd.arg("--file").arg(p);
        }
        Some(SourceArg::Logs(d)) => {
            cmd.arg("--logs").arg(d);
        }
        None => {}
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    cmd.spawn()?;

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match UnixStream::connect(&path) {
            Ok(stream) => return Ok(stream),
            Err(e) if Instant::now() >= deadline => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "spawned {} but {} never answered: {e}",
                        daemon_bin.display(),
                        path.display()
                    ),
                ));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[derive(Default)]
struct Inbox {
    /// Ordered, never dropped (acks, SegmentOpened, LoadFailed, …).
    control: VecDeque<DaemonMsg>,
    /// Coalesced: only the newest Snapshot per (segment, view) and the
    /// newest SegmentList survive — a client that missed ticks catches up
    /// by skipping, never by replaying a backlog.
    snapshots: Vec<DaemonMsg>,
    disconnected: bool,
}

impl Inbox {
    fn push(&mut self, msg: DaemonMsg) {
        match &msg {
            DaemonMsg::Snapshot { segment, view, .. } => {
                let (seg, v) = (*segment, *view);
                self.snapshots.retain(|m| {
                    !matches!(m, DaemonMsg::Snapshot { segment, view, .. }
                        if *segment == seg && *view == v)
                });
                self.snapshots.push(msg);
            }
            DaemonMsg::SegmentList { .. } => {
                self.snapshots
                    .retain(|m| !matches!(m, DaemonMsg::SegmentList { .. }));
                self.snapshots.push(msg);
            }
            _ => self.control.push_back(msg),
        }
    }
}

/// One connection to the daemon: writes on the caller's thread, reads on a
/// background thread into the coalescing inbox.
pub struct DaemonClient {
    stream: UnixStream,
    inbox: Arc<Mutex<Inbox>>,
    daemon_bin: PathBuf,
    source: Option<SourceArg>,
    kind: ClientKind,
    /// Re-declared automatically after a reconnect.
    last_watch: Option<Cursor>,
    dead: bool,
}

impl DaemonClient {
    /// `ensure_daemon` + handshake. Blocks (bounded) until `HelloAck`.
    pub fn connect(
        daemon_bin: &Path,
        source: Option<SourceArg>,
        kind: ClientKind,
    ) -> io::Result<Self> {
        let stream = ensure_daemon(daemon_bin, source.as_ref())?;
        let inbox = handshake(&stream, kind)?;
        Ok(Self {
            stream,
            inbox,
            daemon_bin: daemon_bin.to_path_buf(),
            source,
            kind,
            last_watch: None,
            dead: false,
        })
    }

    /// Handshake over an already-connected stream (tests, `--status`).
    pub fn over(stream: UnixStream, kind: ClientKind) -> io::Result<Self> {
        let inbox = handshake(&stream, kind)?;
        Ok(Self {
            stream,
            inbox,
            daemon_bin: PathBuf::new(),
            source: None,
            kind,
            last_watch: None,
            dead: false,
        })
    }

    /// Declare the cursor; replaces any prior Watch.
    pub fn watch(&mut self, cursor: Cursor) {
        self.send(&ClientMsg::Watch(cursor));
    }

    pub fn send(&mut self, msg: &ClientMsg) {
        if let ClientMsg::Watch(cursor) = msg {
            self.last_watch = Some(cursor.clone());
        }
        if self.stream.write_all(&msg.encode()).is_err() {
            self.dead = true;
        }
    }

    /// Non-blocking drain: control messages in arrival order, then the
    /// coalesced snapshots.
    pub fn poll(&mut self) -> Vec<DaemonMsg> {
        let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
        if inbox.disconnected {
            self.dead = true;
        }
        let mut out: Vec<DaemonMsg> = inbox.control.drain(..).collect();
        out.append(&mut inbox.snapshots);
        out
    }

    pub fn is_dead(&mut self) -> bool {
        if self
            .inbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .disconnected
        {
            self.dead = true;
        }
        self.dead
    }

    /// If the daemon went away, respawn/reconnect and re-declare the cursor.
    /// Returns true when a reconnect actually happened.
    pub fn reconnect_if_dead(&mut self) -> bool {
        if !self.is_dead() || self.daemon_bin.as_os_str().is_empty() {
            return false;
        }
        let Ok(stream) = ensure_daemon(self.daemon_bin.as_path(), self.source.as_ref()) else {
            return false;
        };
        let Ok(inbox) = handshake(&stream, self.kind) else {
            return false;
        };
        self.stream = stream;
        self.inbox = inbox;
        self.dead = false;
        if let Some(cursor) = self.last_watch.clone() {
            self.send(&ClientMsg::Watch(cursor));
        }
        true
    }
}

/// Hello → HelloAck (bounded wait), then the reader thread takes the stream.
fn handshake(stream: &UnixStream, kind: ClientKind) -> io::Result<Arc<Mutex<Inbox>>> {
    let mut writer = stream.try_clone()?;
    writer.write_all(
        &ClientMsg::Hello {
            proto: PROTO_VERSION,
            client: kind,
            pid: std::process::id(),
        }
        .encode(),
    )?;

    let mut reader = stream.try_clone()?;
    reader.set_read_timeout(Some(Duration::from_secs(5)))?;
    let (tag, body) = wire::read_frame(&mut reader)?;
    match DaemonMsg::decode(tag, &body) {
        Ok(DaemonMsg::HelloAck { proto, .. }) if proto == PROTO_VERSION => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad handshake reply: {other:?}"),
            ));
        }
    }
    reader.set_read_timeout(None)?;

    let inbox = Arc::new(Mutex::new(Inbox::default()));
    let inbox_for_reader = Arc::clone(&inbox);
    std::thread::spawn(move || {
        while let Ok((tag, body)) = wire::read_frame(&mut reader) {
            let Ok(msg) = DaemonMsg::decode(tag, &body) else {
                break;
            };
            inbox_for_reader
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(msg);
        }
        inbox_for_reader
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .disconnected = true;
    });
    Ok(inbox)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_name_embeds_the_protocol_version() {
        let name = socket_path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(name, format!("wowdps-v{PROTO_VERSION}.sock"));
    }

    #[test]
    fn preparing_the_dir_makes_it_private_to_us() {
        // Point the dir somewhere disposable for the duration of this test.
        // Env is process-global, but this is the only test that touches it.
        let tmp = std::env::temp_dir().join(format!("wowdps-proto-test-{}", std::process::id()));
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &tmp) };
        std::fs::create_dir_all(&tmp).unwrap();
        let dir = prepare_socket_dir().unwrap();
        assert_eq!(dir, tmp.join("wowdps"));
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
