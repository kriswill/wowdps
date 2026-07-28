//! Log tailing: yields new lines from a single file or the newest
//! `WoWCombatLog*.txt` in a directory, following growth and rotating when a
//! newer file appears. Polling only — no `notify` dependency.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// How often the reader thread wakes when the log is idle.
pub const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Bytes read per poll, so a huge startup replay never stalls the first frame.
const CHUNK: usize = 256 * 1024;
/// A "line" longer than this is flushed anyway rather than buffered forever.
const MAX_LINE: usize = 4 * 1024 * 1024;

const LOG_PREFIX: &str = "WoWCombatLog";
const LOG_SUFFIX: &str = ".txt";

/// Where lines come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceSpec {
    /// Replay/follow one specific file.
    File(PathBuf),
    /// Follow the newest `WoWCombatLog*.txt` in this directory.
    Dir(PathBuf),
}

/// Something the tailer observed. `Switched` means "this is a different log
/// than what you were reading" — the consumer should reset its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailEvent {
    Lines(Vec<String>),
    Switched(PathBuf),
    /// No log file to read yet; emitted once until one shows up.
    Waiting,
    Error(String),
}

struct Open {
    path: PathBuf,
    file: File,
    offset: u64,
    dev: u64,
    ino: u64,
    /// Bytes after the last newline, kept until the rest of the line arrives.
    buf: Vec<u8>,
}

pub struct Tailer {
    spec: SourceSpec,
    open: Option<Open>,
    announced_waiting: bool,
    last_error: Option<String>,
}

impl Tailer {
    pub fn new(spec: SourceSpec) -> Self {
        Self {
            spec,
            open: None,
            announced_waiting: false,
            last_error: None,
        }
    }

    /// Path currently being followed, if any.
    pub fn current(&self) -> Option<&Path> {
        self.open.as_ref().map(|o| o.path.as_path())
    }

    /// Non-blocking: everything readable right now, up to [`CHUNK`] bytes.
    pub fn poll(&mut self) -> Vec<TailEvent> {
        let mut out = Vec::new();
        match self.spec.clone() {
            SourceSpec::File(p) => self.poll_file(&p, &mut out),
            SourceSpec::Dir(d) => self.poll_dir(&d, &mut out),
        }
        out
    }

    fn poll_file(&mut self, path: &Path, out: &mut Vec<TailEvent>) {
        if self.open.is_none() {
            if !path.exists() {
                if !self.announced_waiting {
                    self.announced_waiting = true;
                    out.push(TailEvent::Waiting);
                }
                return;
            }
            self.retarget(path, out);
        }
        self.read_open(out);
    }

    fn poll_dir(&mut self, dir: &Path, out: &mut Vec<TailEvent>) {
        let newest = newest_log(dir);
        match newest {
            None => {
                // Directory missing, empty, or holds no combat logs yet. Not an
                // error: WoW only creates one once /combatlog is enabled.
                if self.open.is_none() && !self.announced_waiting {
                    self.announced_waiting = true;
                    out.push(TailEvent::Waiting);
                }
                if self.open.is_some() {
                    self.read_open(out);
                }
            }
            Some(newest) => {
                let same = self.open.as_ref().is_some_and(|o| o.path == newest);
                if same {
                    self.read_open(out);
                } else if self.open.is_some() {
                    // Drain the old file before switching, or we drop its tail.
                    let before = out.len();
                    self.read_open(out);
                    let produced = out[before..]
                        .iter()
                        .any(|e| matches!(e, TailEvent::Lines(l) if !l.is_empty()));
                    if !produced {
                        self.retarget(&newest, out);
                        self.read_open(out);
                    }
                } else {
                    self.retarget(&newest, out);
                    self.read_open(out);
                }
            }
        }
    }

    /// Open `path` from byte 0 and tell the consumer to reset.
    fn retarget(&mut self, path: &Path, out: &mut Vec<TailEvent>) {
        match File::open(path) {
            Ok(file) => {
                let (dev, ino) = file
                    .metadata()
                    .map(|m| (m.dev(), m.ino()))
                    .unwrap_or((0, 0));
                self.open = Some(Open {
                    path: path.to_path_buf(),
                    file,
                    offset: 0,
                    dev,
                    ino,
                    buf: Vec::new(),
                });
                self.announced_waiting = false;
                self.last_error = None;
                out.push(TailEvent::Switched(path.to_path_buf()));
            }
            Err(e) => {
                self.open = None;
                self.report_error(format!("{}: {e}", path.display()), out);
            }
        }
    }

    fn report_error(&mut self, msg: String, out: &mut Vec<TailEvent>) {
        if self.last_error.as_deref() != Some(msg.as_str()) {
            self.last_error = Some(msg.clone());
            out.push(TailEvent::Error(msg));
        }
    }

    fn read_open(&mut self, out: &mut Vec<TailEvent>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };

        // Detect replace-in-place and truncation before reading.
        match fs::metadata(&open.path) {
            Ok(m) => {
                if m.dev() != open.dev || m.ino() != open.ino || m.len() < open.offset {
                    let path = open.path.clone();
                    self.retarget(&path, out);
                }
            }
            Err(_) => return, // vanished; a later poll picks up its replacement
        }

        let Some(open) = self.open.as_mut() else {
            return;
        };
        let mut chunk = vec![0u8; CHUNK];
        let n = match open.file.read(&mut chunk) {
            Ok(n) => n,
            Err(e) => {
                let msg = format!("{}: {e}", open.path.display());
                self.report_error(msg, out);
                return;
            }
        };
        if n == 0 {
            return;
        }
        open.offset += n as u64;
        open.buf.extend_from_slice(&chunk[..n]);

        let mut lines = Vec::new();
        drain_lines(&mut open.buf, &mut lines);
        if open.buf.len() > MAX_LINE {
            // Pathological: no newline in 4 MiB. Emit what we have rather than
            // grow without bound.
            let stuck = std::mem::take(&mut open.buf);
            lines.push(String::from_utf8_lossy(&stuck).into_owned());
        }
        if !lines.is_empty() {
            out.push(TailEvent::Lines(lines));
        }
    }
}

/// Split complete lines out of `buf`, leaving any trailing partial line behind.
/// Handles CRLF and non-UTF8 bytes (lossy) without failing.
fn drain_lines(buf: &mut Vec<u8>, out: &mut Vec<String>) {
    let mut start = 0;
    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let mut line = &buf[start..i];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        out.push(String::from_utf8_lossy(line).into_owned());
        start = i + 1;
    }
    buf.drain(..start);
}

/// Newest `WoWCombatLog*.txt` in `dir`, by mtime then filename (WoW's names
/// embed the date, so the name is a sane tiebreak when mtimes collide).
pub fn newest_log(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, String, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(LOG_PREFIX) || !name.ends_with(LOG_SUFFIX) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let cand = (mtime, name, entry.path());
        match &best {
            Some(b) if (b.0, &b.1) >= (cand.0, &cand.1) => {}
            _ => best = Some(cand),
        }
    }
    best.map(|b| b.2)
}

/// Run a [`Tailer`] on its own thread; the UI thread never touches the disk.
pub fn spawn(spec: SourceSpec) -> mpsc::Receiver<TailEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut tailer = Tailer::new(spec);
        loop {
            let events = tailer.poll();
            let busy = events
                .iter()
                .any(|e| matches!(e, TailEvent::Lines(l) if !l.is_empty()));
            for ev in events {
                if tx.send(ev).is_err() {
                    return; // UI dropped the receiver
                }
            }
            // Catching up on a replay: keep reading without the idle delay.
            if !busy {
                thread::sleep(POLL_INTERVAL);
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let mut p = std::env::temp_dir();
            p.push(format!(
                "wowdps-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn append(path: &Path, bytes: &[u8]) {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(bytes).unwrap();
    }

    fn lines_of(events: &[TailEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                TailEvent::Lines(l) => Some(l.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn file_reads_existing_content_then_follows_appends() {
        let dir = TempDir::new("follow");
        let p = dir.join("log.txt");
        append(&p, b"one\ntwo\n");

        let mut t = Tailer::new(SourceSpec::File(p.clone()));
        let first = t.poll();
        assert!(first.contains(&TailEvent::Switched(p.clone())));
        assert_eq!(lines_of(&first), vec!["one", "two"]);

        assert_eq!(lines_of(&t.poll()), Vec::<String>::new());

        append(&p, b"three\n");
        assert_eq!(lines_of(&t.poll()), vec!["three"]);
    }

    #[test]
    fn partial_line_is_buffered_until_its_newline_arrives() {
        let dir = TempDir::new("partial");
        let p = dir.join("log.txt");
        append(&p, b"complete\npar");

        let mut t = Tailer::new(SourceSpec::File(p.clone()));
        assert_eq!(lines_of(&t.poll()), vec!["complete"]);

        append(&p, b"tial");
        assert_eq!(lines_of(&t.poll()), Vec::<String>::new());

        append(&p, b"\n");
        assert_eq!(lines_of(&t.poll()), vec!["partial"]);
    }

    #[test]
    fn crlf_line_endings_are_stripped() {
        let dir = TempDir::new("crlf");
        let p = dir.join("log.txt");
        append(&p, b"a\r\nb\r\n");

        let mut t = Tailer::new(SourceSpec::File(p.clone()));
        assert_eq!(lines_of(&t.poll()), vec!["a", "b"]);
    }

    #[test]
    fn invalid_utf8_is_lossy_not_fatal() {
        let dir = TempDir::new("utf8");
        let p = dir.join("log.txt");
        append(&p, b"good\n\xff\xfe bad\nafter\n");

        let mut t = Tailer::new(SourceSpec::File(p.clone()));
        let got = lines_of(&t.poll());
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], "good");
        assert!(got[1].ends_with(" bad"), "got {:?}", got[1]);
        assert_eq!(got[2], "after");
    }

    #[test]
    fn multibyte_char_split_across_reads_is_not_corrupted() {
        let dir = TempDir::new("split");
        let p = dir.join("log.txt");
        let bytes = "Ünsteadý\n".as_bytes().to_vec();
        append(&p, &bytes[..3]);

        let mut t = Tailer::new(SourceSpec::File(p.clone()));
        assert_eq!(lines_of(&t.poll()), Vec::<String>::new());

        append(&p, &bytes[3..]);
        assert_eq!(lines_of(&t.poll()), vec!["Ünsteadý"]);
    }

    #[test]
    fn truncation_reopens_from_the_start() {
        let dir = TempDir::new("trunc");
        let p = dir.join("log.txt");
        append(&p, b"one\ntwo\n");

        let mut t = Tailer::new(SourceSpec::File(p.clone()));
        assert_eq!(lines_of(&t.poll()), vec!["one", "two"]);

        fs::write(&p, b"fresh\n").unwrap();
        let events = t.poll();
        assert!(
            events.contains(&TailEvent::Switched(p.clone())),
            "truncation must tell the consumer to reset: {events:?}"
        );
        assert_eq!(lines_of(&events), vec!["fresh"]);
    }

    #[test]
    fn dir_picks_newest_and_drains_old_file_before_rotating() {
        let dir = TempDir::new("rotate");
        let a = dir.join("WoWCombatLog-01.txt");
        append(&a, b"a1\n");

        let mut t = Tailer::new(SourceSpec::Dir(dir.path().to_path_buf()));
        let first = t.poll();
        assert!(first.contains(&TailEvent::Switched(a.clone())));
        assert_eq!(lines_of(&first), vec!["a1"]);

        // New log appears while the old one still has unread bytes.
        append(&a, b"a2\n");
        let b = dir.join("WoWCombatLog-02.txt");
        append(&b, b"b1\n");

        let drain = t.poll();
        assert_eq!(lines_of(&drain), vec!["a2"], "old file must drain first");
        assert!(
            !drain.iter().any(|e| matches!(e, TailEvent::Switched(_))),
            "must not rotate while the old file still has data: {drain:?}"
        );

        let rotated = t.poll();
        assert!(rotated.contains(&TailEvent::Switched(b.clone())));
        assert_eq!(lines_of(&rotated), vec!["b1"]);
    }

    #[test]
    fn dir_without_log_files_waits_instead_of_erroring() {
        let dir = TempDir::new("empty");
        fs::write(dir.join("Client.log"), b"noise\n").unwrap();

        let mut t = Tailer::new(SourceSpec::Dir(dir.path().to_path_buf()));
        assert_eq!(t.poll(), vec![TailEvent::Waiting]);
        assert_eq!(
            t.poll(),
            Vec::<TailEvent>::new(),
            "waiting is announced once"
        );

        let p = dir.join("WoWCombatLog.txt");
        append(&p, b"here\n");
        let events = t.poll();
        assert!(events.contains(&TailEvent::Switched(p)));
        assert_eq!(lines_of(&events), vec!["here"]);
    }

    #[test]
    fn missing_dir_is_not_an_error() {
        let dir = TempDir::new("gone");
        let missing = dir.join("nope");
        let mut t = Tailer::new(SourceSpec::Dir(missing));
        assert_eq!(t.poll(), vec![TailEvent::Waiting]);
        assert!(!t.poll().iter().any(|e| matches!(e, TailEvent::Error(_))));
    }

    #[test]
    fn newest_log_ignores_unrelated_files() {
        let dir = TempDir::new("select");
        fs::write(dir.join("Client.log"), b"").unwrap();
        fs::write(dir.join("WoWCombatLog.txt.bak"), b"").unwrap();
        fs::create_dir(dir.join("WoWCombatLog-dir.txt")).unwrap();
        let real = dir.join("WoWCombatLog-070124.txt");
        fs::write(&real, b"").unwrap();

        assert_eq!(newest_log(dir.path()), Some(real));
    }

    #[test]
    fn drain_lines_keeps_the_tail() {
        let mut buf = b"a\nb\nc".to_vec();
        let mut out = Vec::new();
        drain_lines(&mut buf, &mut out);
        assert_eq!(out, vec!["a", "b"]);
        assert_eq!(buf, b"c");
    }
}
