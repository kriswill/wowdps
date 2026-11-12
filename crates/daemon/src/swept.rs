//! The sweep's manifest: which logs the history has read whole, so a start
//! reads only what is new. It sits beside the index checkpoints
//! (`<cache dir>/swept`), a cache like them: deleting it costs one read of
//! every log, nothing more.
//!
//! An entry is a log read to its end as an older log — never the tailed
//! one, whose open tail is live — with every import that read queued
//! landed: what the file was before the read (inode, length, modified
//! time), its identity and timezone (`LogFacts`), its edition, and the
//! fights of it the store then held. The start-up sweep passes over a log
//! whose entry still describes the file and whose held fights are all still
//! cards; anything else is read again ([`Manifest::unchanged`]). A fight
//! retention evicts later is dropped from its entry ([`Manifest::forget`]),
//! so it stays evicted rather than being parsed again on every start.
//!
//! The whole manifest answers to one set of rules ([`rules`]): the card
//! schema, the index cache's format (the scanner's rules), and the history
//! config that decides what a read stores and keeps. Any of them moving
//! voids every entry, so each log is read once more under the new rules.
//!
//! Serialized with the wire primitives, as the checkpoints are.

use std::collections::{BTreeMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use wowdps_proto::history::{HISTORY_SCHEMA, fight_id, fnv64};
use wowdps_proto::wire::{self, Reader};

use crate::cache::write_atomic;
use crate::history::{LogFacts, Retention};

/// The manifest's file name in the index cache's directory. No `.bin`
/// extension: the checkpoints' prune counts only those.
pub const FILE: &str = "swept";

/// `\x01`: the first layout.
const MAGIC: &[u8; 8] = b"WDPSSWP\x01";

/// Entries kept at most: past it, the logs written longest ago go first.
/// A folder of logs is a few hundred; this bounds a store fed by hand-
/// imported folders too.
const MAX_ENTRIES: usize = 4096;

/// What a file is, told without reading it: its inode, its length and its
/// modified time to the nanosecond. A log that grew, was truncated, was
/// rewritten or was replaced by another file of the same name differs in
/// one of them. The device number is left out: some filesystems renumber
/// it on every mount, and a file at the same path with the same inode,
/// length and nanosecond mtime is all but certainly the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub ino: u64,
    pub len: u64,
    pub mtime_s: i64,
    pub mtime_ns: i64,
}

impl Stat {
    /// The file's, through symlinks (as the scan opens it); `None` for
    /// anything that is not a readable regular file's metadata.
    pub fn of(path: &Path) -> Option<Self> {
        let m = std::fs::metadata(path).ok()?;
        m.is_file().then(|| Self {
            ino: m.ino(),
            len: m.len(),
            mtime_s: m.mtime(),
            mtime_ns: m.mtime_nsec(),
        })
    }
}

/// One log read whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file before the read began: a byte the read may have missed
    /// leaves the file differing from it.
    pub stat: Stat,
    /// `LogFacts::id` and `LogFacts::tz_min` (always complete).
    pub log: u64,
    pub tz_min: Option<i16>,
    /// The edition the read stood in at its end (`ScanState::project_id`).
    pub project_id: u8,
    /// The fights of the log the store held when the read finished, by
    /// [`fight_id`]'s parts: the start (log-local ms) and the Σ mark.
    pub held: Vec<(i64, bool)>,
}

impl Entry {
    pub fn facts(&self) -> LogFacts {
        LogFacts {
            id: self.log,
            tz_min: self.tz_min,
            complete: true,
        }
    }

    /// The held fights' ids.
    pub fn held_ids(&self) -> impl Iterator<Item = String> + '_ {
        self.held
            .iter()
            .map(|(start, sigma)| fight_id(self.log, *start, *sigma))
    }
}

/// The fingerprint every entry answers to: the card schema, the index
/// cache's format and the history config that decides what a read of a
/// log stores (`store_trash`) and what retention keeps of it (the cap, the
/// kept-whole switches, the configured characters, whose bests are kept).
/// Detail and replay caps are left out: what a kept fight is short of, the
/// rewrite queue fills from its log without the sweep.
pub fn rules(cfg: &Retention) -> u64 {
    let mut chars: Vec<String> = cfg
        .characters
        .iter()
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    chars.sort();
    let mut key = Vec::new();
    key.extend_from_slice(MAGIC);
    key.extend_from_slice(crate::cache::MAGIC);
    key.extend_from_slice(
        format!(
            "|{HISTORY_SCHEMA}|{}|{}|{}|{}|{}",
            cfg.store_trash,
            cfg.keep_per_encounter,
            cfg.keep_whole,
            cfg.keep_progression,
            chars.join(",")
        )
        .as_bytes(),
    );
    fnv64(&key)
}

/// The key an entry is filed under: the log's canonical path, so two
/// spellings of one folder share their entries.
pub fn key_of(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub struct Manifest {
    file: PathBuf,
    rules: u64,
    entries: BTreeMap<PathBuf, Entry>,
    /// Changed since the file was last written.
    dirty: bool,
}

impl Manifest {
    /// The manifest in `dir` (the index cache's directory) for `rules`.
    /// A missing, torn or foreign file, or one written under other rules,
    /// is an empty manifest; an entry whose file is gone or no longer what
    /// it describes is dropped (it can never match again).
    pub fn load(dir: &Path, rules: u64) -> Self {
        let file = dir.join(FILE);
        let entries = std::fs::read(&file)
            .ok()
            .and_then(|bytes| decode(&bytes))
            .filter(|(r, _)| *r == rules)
            .map(|(_, entries)| entries)
            .unwrap_or_default();
        let mut m = Self {
            file,
            rules,
            entries: entries.into_iter().collect(),
            dirty: false,
        };
        m.drop_stale();
        m
    }

    /// The entry of a log (by [`key_of`]) when it is one the sweep may pass
    /// over: the file is still what the entry describes and every fight it
    /// held is still one of `cards`.
    pub fn unchanged(&self, key: &Path, cards: &HashSet<&str>) -> Option<&Entry> {
        let entry = self.entries.get(key)?;
        (Stat::of(key) == Some(entry.stat)
            && entry.held_ids().all(|id| cards.contains(id.as_str())))
        .then_some(entry)
    }

    /// A finished read: replaces whatever the key held.
    pub fn record(&mut self, key: PathBuf, entry: Entry) {
        self.entries.insert(key, entry);
        self.dirty = true;
    }

    /// Retention evicted fight `id` of log `log`: no entry holds it now, so
    /// its absence reads no log again.
    pub fn forget(&mut self, log: u64, id: &str) {
        for entry in self.entries.values_mut().filter(|e| e.log == log) {
            let before = entry.held.len();
            entry
                .held
                .retain(|(start, sigma)| fight_id(log, *start, *sigma) != id);
            self.dirty |= entry.held.len() != before;
        }
    }

    /// The path and facts of the log whose identity is `log`, when an entry
    /// knows one that is still that file: a log read once is found again
    /// without reading a header — one imported from outside the followed
    /// folders too.
    pub fn path_of(&self, log: u64) -> Option<(PathBuf, LogFacts)> {
        self.entries
            .iter()
            .find(|(path, e)| e.log == log && Stat::of(path) == Some(e.stat))
            .map(|(path, e)| (path.clone(), e.facts()))
    }

    pub fn entries(&self) -> impl Iterator<Item = (&Path, &Entry)> {
        self.entries.iter().map(|(p, e)| (p.as_path(), e))
    }

    /// Write the manifest when it changed, its stale entries dropped and
    /// its size bounded first; unchanged, a flush costs nothing (the idle
    /// loop calls it on every turn).
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.drop_stale();
        if self.entries.len() > MAX_ENTRIES {
            let mut by_age: Vec<(i64, i64, PathBuf)> = self
                .entries
                .iter()
                .map(|(p, e)| (e.stat.mtime_s, e.stat.mtime_ns, p.clone()))
                .collect();
            by_age.sort();
            let over = by_age.len() - MAX_ENTRIES;
            for (.., path) in by_age.into_iter().take(over) {
                self.entries.remove(&path);
            }
        }
        let bytes = encode(self.rules, &self.entries);
        // Written or not, it is clean until it changes again: a cache that
        // refuses writes is not tried on every turn, and a lost write costs
        // a read on the next start, never a fight.
        let _ = self
            .file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| write_atomic(&self.file, &bytes));
        self.dirty = false;
    }

    /// Entries whose file is gone or changed drop out: a log deleted from
    /// disk leaves the manifest at the next load or write.
    fn drop_stale(&mut self) {
        let before = self.entries.len();
        self.entries
            .retain(|path, e| Stat::of(path) == Some(e.stat));
        self.dirty |= self.entries.len() != before;
    }
}

/// The entries a manifest file holds, whatever its rules — for a reader
/// that only looks (a test, a diagnostic).
pub fn read(dir: &Path) -> Option<(u64, Vec<(PathBuf, Entry)>)> {
    decode(&std::fs::read(dir.join(FILE)).ok()?)
}

fn encode(rules: u64, entries: &BTreeMap<PathBuf, Entry>) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    wire::put_u64(&mut buf, rules);
    let entries: Vec<(&PathBuf, &Entry)> = entries.iter().collect();
    wire::put_vec(&mut buf, &entries, |b, (path, e)| {
        wire::put_bytes(b, path.as_os_str().as_bytes());
        wire::put_u64(b, e.stat.ino);
        wire::put_u64(b, e.stat.len);
        wire::put_i64(b, e.stat.mtime_s);
        wire::put_i64(b, e.stat.mtime_ns);
        wire::put_u64(b, e.log);
        wire::put_opt(b, e.tz_min.as_ref(), |b, tz| {
            wire::put_u16(b, u16::from_le_bytes(tz.to_le_bytes()))
        });
        wire::put_u8(b, e.project_id);
        wire::put_vec(b, &e.held, |b, (start, sigma)| {
            wire::put_i64(b, *start);
            wire::put_bool(b, *sigma);
        });
    });
    buf
}

fn decode(bytes: &[u8]) -> Option<(u64, Vec<(PathBuf, Entry)>)> {
    let rest = bytes.strip_prefix(MAGIC.as_slice())?;
    let mut rd = Reader::new(rest);
    let rules = rd.u64().ok()?;
    let entries = rd
        .vec(|r| {
            let path = PathBuf::from(std::ffi::OsStr::from_bytes(&r.bytes()?));
            let stat = Stat {
                ino: r.u64()?,
                len: r.u64()?,
                mtime_s: r.i64()?,
                mtime_ns: r.i64()?,
            };
            let entry = Entry {
                stat,
                log: r.u64()?,
                tz_min: r.opt(|r| Ok(i16::from_le_bytes(r.u16()?.to_le_bytes())))?,
                project_id: r.u8()?,
                held: r.vec(|r| Ok((r.i64()?, r.bool()?)))?,
            };
            Ok((path, entry))
        })
        .ok()?;
    rd.finish().ok()?;
    Some((rules, entries))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let p =
                std::env::temp_dir().join(format!("wowdps-swept-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry_for(path: &Path, held: &[(i64, bool)]) -> Entry {
        Entry {
            stat: Stat::of(path).unwrap(),
            log: 0xabc,
            tz_min: Some(-240),
            project_id: 1,
            held: held.to_vec(),
        }
    }

    fn cards(ids: &[String]) -> HashSet<&str> {
        ids.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_flushed_manifest_loads_back_whole_under_its_rules() {
        let tmp = Temp::new("roundtrip");
        let log = tmp.0.join("WoWCombatLog-1.txt");
        std::fs::write(&log, "a log\n").unwrap();
        let mut m = Manifest::load(&tmp.0, 7);
        m.record(
            key_of(&log),
            entry_for(&log, &[(1000, false), (2000, true)]),
        );
        m.flush();
        let back = Manifest::load(&tmp.0, 7);
        let got: Vec<(&Path, &Entry)> = back.entries().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, key_of(&log));
        assert_eq!(got[0].1, &entry_for(&log, &[(1000, false), (2000, true)]));
        // Another rule set reads none of it.
        assert_eq!(Manifest::load(&tmp.0, 8).entries().count(), 0);
        // A torn file is no manifest, never a panic.
        let bytes = std::fs::read(tmp.0.join(FILE)).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode(&bytes[..cut]).is_none(), "cut at {cut}");
        }
    }

    #[test]
    fn a_log_is_unchanged_only_while_its_file_and_its_fights_are() {
        let tmp = Temp::new("unchanged");
        let log = tmp.0.join("WoWCombatLog-1.txt");
        std::fs::write(&log, "a log\n").unwrap();
        let mut m = Manifest::load(&tmp.0, 1);
        let key = key_of(&log);
        m.record(key.clone(), entry_for(&log, &[(1000, false), (2000, true)]));
        let both = [fight_id(0xabc, 1000, false), fight_id(0xabc, 2000, true)];
        assert!(m.unchanged(&key, &cards(&both)).is_some());
        // A held fight missing from the store (a store wiped, a card
        // deleted): read again.
        assert!(m.unchanged(&key, &cards(&both[..1])).is_none());
        assert!(m.unchanged(&key, &HashSet::new()).is_none());
        // Evicted by retention: no longer held, so its absence is no change.
        m.forget(0xabc, &both[1]);
        assert!(m.unchanged(&key, &cards(&both[..1])).is_some());
        // Any change to the file: read again.
        let at = std::fs::metadata(&log).unwrap().modified().unwrap();
        std::fs::write(&log, "a log!\n").unwrap();
        assert!(m.unchanged(&key, &cards(&both)).is_none(), "grown");
        std::fs::write(&log, "a lo\n").unwrap();
        set_mtime(&log, at);
        assert!(m.unchanged(&key, &cards(&both)).is_none(), "truncated");
        std::fs::write(&log, "a log\n").unwrap();
        set_mtime(&log, at);
        assert!(m.unchanged(&key, &cards(&both)).is_some(), "as it was");
        set_mtime(&log, at + std::time::Duration::from_nanos(1));
        assert!(m.unchanged(&key, &cards(&both)).is_none(), "touched");
        // Replaced by another file of the same name, length and mtime.
        let other = tmp.0.join("other.txt");
        std::fs::write(&other, "b log\n").unwrap();
        set_mtime(&other, at);
        std::fs::rename(&other, &log).unwrap();
        assert!(m.unchanged(&key, &cards(&both)).is_none(), "replaced");
        // Unknown logs are never unchanged.
        assert!(
            m.unchanged(&tmp.0.join("nope.txt"), &cards(&both))
                .is_none()
        );
    }

    #[test]
    fn a_deleted_log_drops_out_and_its_path_is_no_answer() {
        let tmp = Temp::new("deleted");
        let a = tmp.0.join("WoWCombatLog-a.txt");
        let b = tmp.0.join("WoWCombatLog-b.txt");
        std::fs::write(&a, "a\n").unwrap();
        std::fs::write(&b, "b\n").unwrap();
        let mut m = Manifest::load(&tmp.0, 1);
        m.record(key_of(&a), entry_for(&a, &[]));
        let mut eb = entry_for(&b, &[]);
        eb.log = 0xb;
        m.record(key_of(&b), eb);
        m.flush();
        assert_eq!(m.path_of(0xb).map(|(p, _)| p), Some(key_of(&b)));
        std::fs::remove_file(&b).unwrap();
        assert!(m.path_of(0xb).is_none(), "a gone file is found nowhere");
        // The next write leaves it out.
        m.record(key_of(&a), entry_for(&a, &[(5, false)]));
        m.flush();
        let (_, entries) = read(&tmp.0).unwrap();
        assert_eq!(
            entries.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
            [key_of(&a)]
        );
        // And a load drops what no write got to, writing at its first flush.
        std::fs::remove_file(&a).unwrap();
        let mut m = Manifest::load(&tmp.0, 1);
        assert_eq!(m.entries().count(), 0);
        m.flush();
        assert_eq!(read(&tmp.0).unwrap().1.len(), 0);
    }

    #[test]
    fn the_rules_move_with_what_a_read_stores_and_keeps() {
        let base = Retention::default();
        let r = rules(&base);
        assert_eq!(r, rules(&base.clone()));
        for changed in [
            Retention {
                store_trash: true,
                ..base.clone()
            },
            Retention {
                keep_per_encounter: 201,
                ..base.clone()
            },
            Retention {
                keep_whole: false,
                ..base.clone()
            },
            Retention {
                keep_progression: false,
                ..base.clone()
            },
            Retention {
                characters: vec!["Ana-Realm".into()],
                ..base.clone()
            },
        ] {
            assert_ne!(rules(&changed), r, "{changed:?}");
        }
        // What the rewrite queue fills without a sweep moves nothing.
        for same in [
            Retention {
                keep_details_per_encounter: 3,
                ..base.clone()
            },
            Retention {
                details_min_wipe_secs: 5,
                ..base.clone()
            },
            Retention {
                replay_bytes: 1,
                ..base.clone()
            },
        ] {
            assert_eq!(rules(&same), r, "{same:?}");
        }
        // The characters as a set, however spelled.
        let a = Retention {
            characters: vec!["Ana-Realm".into(), " bex-realm ".into()],
            ..base.clone()
        };
        let b = Retention {
            characters: vec!["Bex-Realm".into(), "ana-realm".into()],
            ..base
        };
        assert_eq!(rules(&a), rules(&b));
    }

    fn set_mtime(path: &Path, at: std::time::SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }
}
