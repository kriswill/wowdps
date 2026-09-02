//! The one piece of persistence in scope: the structural index's resumable
//! checkpoint, serialized with the wire primitives (no new format), keyed by
//! file identity plus a checksum of the bytes just before the checkpoint.
//! On a warm start only the tail `[checkpoint.offset, EOF)` is rescanned.
//!
//! Deliberately NOT cached: parsed segment `Meter`s. They rebuild from the
//! log in milliseconds, and serializing per-actor hashmaps is how a cache
//! becomes an event store by accident.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use wowdps_core::index::{self, Index, ScanState, SegmentMeta};
use wowdps_proto::wire::{self, Reader};

/// Cache files kept per directory; older ones are evicted so "newest logs
/// warm, ancient ones rescan" falls out of the layout.
const KEEP: usize = 8;

/// Bytes hashed before the checkpoint to prove the prefix is unchanged.
const CHECK_WINDOW: u64 = 64 * 1024;

// \x02: R10 added visit tracking (meta.visit, overalls, open-visit state).
// Old entries fail the magic check and cost one full rescan, nothing more.
// \x0c: ScanState gained R13's `arena_over` (and \x0b before it: the arena
// verdict rule changed to faction-based home side, invalidating cached
// success flags).
const MAGIC: &[u8; 8] = b"WDPSIDX\x0c";

pub struct IndexCache {
    dir: PathBuf,
}

impl IndexCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The production location: `$XDG_CACHE_HOME/wowdps/index`.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
        Some(base.join("wowdps/index"))
    }

    /// Scan `file`, resuming from a cached checkpoint when the file's
    /// identity and prefix bytes still match; any mismatch (rotation,
    /// truncation, rewrite) is a full rescan. Stores the fresh checkpoint
    /// either way.
    pub fn scan_file(&self, path: &Path, file: &mut File) -> Index {
        let idx = match self.resume_state(path, file) {
            Some(state) if file.seek(SeekFrom::Start(state.offset)).is_ok() => {
                index::scan_from(file, state)
            }
            _ => {
                let _ = file.seek(SeekFrom::Start(0));
                index::scan(file)
            }
        };
        self.store(path, file, &idx.checkpoint);
        idx
    }

    fn cache_path(&self, log: &Path) -> PathBuf {
        self.dir.join(format!(
            "{:016x}.bin",
            fnv64(log.to_string_lossy().as_bytes())
        ))
    }

    fn resume_state(&self, path: &Path, file: &mut File) -> Option<ScanState> {
        let bytes = std::fs::read(self.cache_path(path)).ok()?;
        let (dev, ino, check_len, checksum, state) = decode(&bytes)?;

        let meta = file.metadata().ok()?;
        if meta.dev() != dev || meta.ino() != ino || meta.len() < state.offset {
            return None;
        }
        // `check_len` came off disk too: a corrupt entry must not underflow
        // the subtraction below or size an allocation. `store` never writes
        // more than CHECK_WINDOW.
        if check_len > state.offset || check_len > CHECK_WINDOW {
            return None;
        }
        // The cached prefix must still be the same bytes: hash the window
        // right before the checkpoint and compare.
        let start = state.offset - check_len;
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut window = vec![0u8; check_len as usize];
        file.read_exact(&mut window).ok()?;
        (fnv64(&window) == checksum).then_some(state)
    }

    fn store(&self, path: &Path, file: &mut File, state: &ScanState) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let check_len = state.offset.min(CHECK_WINDOW);
        let checksum = {
            let start = state.offset - check_len;
            if file.seek(SeekFrom::Start(start)).is_err() {
                return;
            }
            let mut window = vec![0u8; check_len as usize];
            if file.read_exact(&mut window).is_err() {
                return;
            }
            fnv64(&window)
        };
        let Ok(meta) = file.metadata() else { return };

        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        wire::put_u64(&mut buf, meta.dev());
        wire::put_u64(&mut buf, meta.ino());
        wire::put_u64(&mut buf, check_len);
        wire::put_u64(&mut buf, checksum);
        wire::put_u64(&mut buf, state.offset);
        wire::put_opt(&mut buf, state.last_combat_ms.as_ref(), |b, v| {
            wire::put_i64(b, *v)
        });
        wire::put_vec(&mut buf, &state.seeds, put_range);
        wire::put_vec(&mut buf, &state.segments, put_meta);
        wire::put_vec(&mut buf, &state.overalls, put_meta);
        wire::put_u32(&mut buf, state.visit_count);
        wire::put_opt(&mut buf, state.visit.as_ref(), put_visit);
        wire::put_opt(&mut buf, state.last_zone.as_ref(), |b, z| {
            wire::put_str(b, z)
        });
        wire::put_bool(&mut buf, state.arena_over);

        let target = self.cache_path(path);
        let tmp = target.with_extension("tmp");
        if std::fs::write(&tmp, &buf).is_ok() {
            let _ = std::fs::rename(&tmp, &target);
        }
        self.prune();
    }

    /// Keep the newest [`KEEP`] entries by mtime.
    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, e.path()))
            })
            .collect();
        if files.len() <= KEEP {
            return;
        }
        files.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));
        for (_, path) in files.split_off(KEEP) {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn decode(bytes: &[u8]) -> Option<(u64, u64, u64, u64, ScanState)> {
    let rest = bytes.strip_prefix(MAGIC.as_slice())?;
    let mut rd = Reader::new(rest);
    let dev = rd.u64().ok()?;
    let ino = rd.u64().ok()?;
    let check_len = rd.u64().ok()?;
    let checksum = rd.u64().ok()?;
    let offset = rd.u64().ok()?;
    let last_combat_ms = rd.opt(|r| r.i64()).ok()?;
    let seeds = rd.vec(get_range).ok()?;
    let segments = rd.vec(get_meta).ok()?;
    let overalls = rd.vec(get_meta).ok()?;
    let visit_count = rd.u32().ok()?;
    let visit = rd.opt(get_visit).ok()?;
    let last_zone = rd.opt(|r| r.string()).ok()?;
    let arena_over = rd.bool().ok()?;
    rd.finish().ok()?;
    Some((
        dev,
        ino,
        check_len,
        checksum,
        ScanState {
            segments,
            overalls,
            seeds,
            last_combat_ms,
            visit_count,
            visit,
            last_zone,
            arena_over,
            offset,
        },
    ))
}

fn put_visit(buf: &mut Vec<u8>, v: &wowdps_core::index::VisitScan) {
    wire::put_u32(buf, v.ordinal);
    wire::put_u32(buf, v.map_id);
    wire::put_u32(buf, v.difficulty);
    wire::put_str(buf, &v.name);
    wire::put_opt(buf, v.key_level.as_ref(), |b, l| wire::put_u32(b, *l));
    wire::put_bool(buf, v.keyed);
    wire::put_opt(buf, v.completed.as_ref(), |b, c| wire::put_bool(b, *c));
    wire::put_opt(buf, v.official_ms.as_ref(), |b, o| wire::put_i64(b, *o));
    wire::put_opt(buf, v.pars_ms.as_ref(), |b, p| {
        wire::put_i64(b, p.0);
        wire::put_i64(b, p.1);
        wire::put_i64(b, p.2);
    });
    wire::put_i64(buf, v.start_ms);
    wire::put_u64(buf, v.start_off);
    wire::put_i64(buf, v.dur_ms);
    wire::put_u32(buf, v.members);
    wire::put_u64(buf, v.seed_n as u64);
    wire::put_bool(buf, v.zoned_in);
}

fn get_visit(rd: &mut Reader) -> wire::Result<wowdps_core::index::VisitScan> {
    Ok(wowdps_core::index::VisitScan {
        ordinal: rd.u32()?,
        map_id: rd.u32()?,
        difficulty: rd.u32()?,
        name: rd.string()?,
        key_level: rd.opt(|r| r.u32())?,
        keyed: rd.bool()?,
        completed: rd.opt(|r| r.bool())?,
        official_ms: rd.opt(|r| r.i64())?,
        pars_ms: rd.opt(|r| Ok((r.i64()?, r.i64()?, r.i64()?)))?,
        start_ms: rd.i64()?,
        start_off: rd.u64()?,
        dur_ms: rd.i64()?,
        members: rd.u32()?,
        seed_n: rd.u64()? as usize,
        zoned_in: rd.bool()?,
    })
}

fn put_range(buf: &mut Vec<u8>, r: &(u64, u64)) {
    wire::put_u64(buf, r.0);
    wire::put_u64(buf, r.1);
}

fn get_range(rd: &mut Reader) -> wire::Result<(u64, u64)> {
    Ok((rd.u64()?, rd.u64()?))
}

fn put_meta(buf: &mut Vec<u8>, m: &SegmentMeta) {
    use wowdps_core::model::SegmentKind;
    wire::put_u8(
        buf,
        match m.kind {
            SegmentKind::Encounter => 0,
            SegmentKind::Trash => 1,
            SegmentKind::Overall => 2,
        },
    );
    wire::put_str(buf, &m.name);
    wire::put_i64(buf, m.start_ms);
    wire::put_opt(buf, m.end_ms.as_ref(), |b, v| wire::put_i64(b, *v));
    wire::put_opt(buf, m.success.as_ref(), |b, v| wire::put_bool(b, *v));
    wire::put_i64(buf, m.duration_ms);
    wire::put_bool(buf, m.counts);
    wire::put_opt(buf, m.pars_ms.as_ref(), |b, p| {
        wire::put_i64(b, p.0);
        wire::put_i64(b, p.1);
        wire::put_i64(b, p.2);
    });
    put_range(buf, &m.byte_range);
    wire::put_vec(buf, &m.seeds, put_range);
    wire::put_opt(buf, m.visit.as_ref(), |b, v| wire::put_u32(b, *v));
    wire::put_bool(buf, m.arena);
}

fn get_meta(rd: &mut Reader) -> wire::Result<SegmentMeta> {
    use wowdps_core::model::SegmentKind;
    Ok(SegmentMeta {
        kind: match rd.u8()? {
            1 => SegmentKind::Trash,
            2 => SegmentKind::Overall,
            _ => SegmentKind::Encounter,
        },
        name: rd.string()?,
        start_ms: rd.i64()?,
        end_ms: rd.opt(|r| r.i64())?,
        success: rd.opt(|r| r.bool())?,
        duration_ms: rd.i64()?,
        counts: rd.bool()?,
        pars_ms: rd.opt(|r| Ok((r.i64()?, r.i64()?, r.i64()?)))?,
        byte_range: get_range(rd)?,
        seeds: rd.vec(get_range)?,
        visit: rd.opt(|r| r.u32())?,
        arena: rd.bool()?,
    })
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");

    struct Temp(PathBuf);
    impl Temp {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("wowdps-cache-{tag}-{}", std::process::id()));
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

    #[test]
    fn cold_then_warm_then_append_all_agree_with_a_full_scan() {
        let tmp = Temp::new("warm");
        let log = tmp.0.join("WoWCombatLog.txt");
        std::fs::copy(FIXTURE, &log).unwrap();
        let cache = IndexCache::new(tmp.0.join("cache"));

        // Cold: full scan, checkpoint stored.
        let cold = cache.scan_file(&log, &mut File::open(&log).unwrap());
        assert!(cache.cache_path(&log).exists());

        // Warm: same bytes, resumed scan must agree completely.
        let warm = cache.scan_file(&log, &mut File::open(&log).unwrap());
        assert_eq!(cold, warm);

        // Append more combat: the tail-only rescan must match a full scan.
        let extra = "7/27/2026 23:59:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n";
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(extra.as_bytes()).unwrap();
        drop(f);

        let resumed = cache.scan_file(&log, &mut File::open(&log).unwrap());
        let full = index::scan(&mut File::open(&log).unwrap());
        assert_eq!(resumed, full);
    }

    #[test]
    fn a_rewritten_file_fails_identity_and_rescans_fully() {
        let tmp = Temp::new("ident");
        let log = tmp.0.join("WoWCombatLog.txt");
        std::fs::copy(FIXTURE, &log).unwrap();
        let cache = IndexCache::new(tmp.0.join("cache"));
        cache.scan_file(&log, &mut File::open(&log).unwrap());

        // Truncate-and-rewrite with different content of similar size: the
        // checksum window catches it even though dev/ino may survive.
        let text = std::fs::read_to_string(&log).unwrap();
        let doctored = text.replace("Verkath the Hollow", "Somebody Different");
        std::fs::write(&log, doctored).unwrap();

        let rescanned = cache.scan_file(&log, &mut File::open(&log).unwrap());
        let full = index::scan(&mut File::open(&log).unwrap());
        assert_eq!(rescanned, full);
        assert!(
            rescanned
                .segments
                .iter()
                .any(|m| m.name == "Somebody Different"),
            "the rescan saw the new content"
        );
    }

    #[test]
    fn truncation_forces_a_full_rescan() {
        let tmp = Temp::new("trunc");
        let log = tmp.0.join("WoWCombatLog.txt");
        std::fs::copy(FIXTURE, &log).unwrap();
        let cache = IndexCache::new(tmp.0.join("cache"));
        cache.scan_file(&log, &mut File::open(&log).unwrap());

        let bytes = std::fs::read(&log).unwrap();
        std::fs::write(&log, &bytes[..bytes.len() / 4]).unwrap();

        let rescanned = cache.scan_file(&log, &mut File::open(&log).unwrap());
        let full = index::scan(&mut File::open(&log).unwrap());
        assert_eq!(rescanned, full);
    }

    #[test]
    fn a_corrupt_cache_file_is_ignored() {
        let tmp = Temp::new("corrupt");
        let log = tmp.0.join("WoWCombatLog.txt");
        std::fs::copy(FIXTURE, &log).unwrap();
        let cache = IndexCache::new(tmp.0.join("cache"));
        cache.scan_file(&log, &mut File::open(&log).unwrap());

        // Stomp the cache file with garbage; the next scan must fall back.
        let entry = cache.cache_path(&log);
        std::fs::write(&entry, b"WDPSIDX\x01garbage").unwrap();
        let idx = cache.scan_file(&log, &mut File::open(&log).unwrap());
        let full = index::scan(&mut File::open(&log).unwrap());
        assert_eq!(idx, full);
    }

    #[test]
    fn old_entries_are_evicted_by_count() {
        let tmp = Temp::new("prune");
        let cache = IndexCache::new(tmp.0.join("cache"));
        for i in 0..(KEEP + 4) {
            let log = tmp.0.join(format!("WoWCombatLog-{i}.txt"));
            std::fs::copy(FIXTURE, &log).unwrap();
            cache.scan_file(&log, &mut File::open(&log).unwrap());
        }
        let count = std::fs::read_dir(tmp.0.join("cache")).unwrap().count();
        assert!(count <= KEEP, "kept {count}");
    }

    const INSTANCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/instance.txt");

    /// R10 state survives the cache: a keyed visit's par timers and its
    /// Overall meta come back from a warm start exactly as a cold scan
    /// produces them.
    #[test]
    fn keystone_pars_and_overalls_survive_a_warm_start() {
        use wowdps_core::model::SegmentKind;
        let tmp = Temp::new("keystone");
        let cache = IndexCache::new(tmp.0.join("idx"));
        let path = Path::new(INSTANCE);
        let mut file = std::fs::File::open(path).unwrap();
        let cold = cache.scan_file(path, &mut file);
        assert!(
            cold.overalls.iter().any(|m| m.pars_ms.is_some()),
            "the instance fixture holds a keyed visit"
        );
        assert!(cold.overalls.iter().all(|m| m.kind == SegmentKind::Overall));

        let mut file = std::fs::File::open(path).unwrap();
        let warm = cache.scan_file(path, &mut file);
        assert_eq!(warm.segments, cold.segments);
        assert_eq!(warm.overalls, cold.overalls);
        assert_eq!(warm.checkpoint.offset, cold.checkpoint.offset);
    }

    /// The production location follows `$XDG_CACHE_HOME`, then `$HOME`.
    #[test]
    fn the_default_dir_follows_the_xdg_environment() {
        // Env is process-global; this is the only test touching it.
        unsafe { std::env::set_var("XDG_CACHE_HOME", "/xdg-cache") };
        assert_eq!(
            IndexCache::default_dir(),
            Some(PathBuf::from("/xdg-cache/wowdps/index"))
        );
        unsafe { std::env::set_var("XDG_CACHE_HOME", "") };
        let fallback = IndexCache::default_dir();
        match std::env::var_os("HOME") {
            Some(home) => assert_eq!(
                fallback,
                Some(PathBuf::from(home).join(".cache/wowdps/index"))
            ),
            None => assert_eq!(fallback, None),
        }
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };
    }

    /// A checkpoint taken INSIDE a running key carries the open visit's
    /// par timers; the warm resume reproduces the cold scan exactly.
    #[test]
    fn an_open_keyed_visit_survives_a_warm_start() {
        let tmp = Temp::new("openkey");
        let text = std::fs::read_to_string(INSTANCE).unwrap();
        let cut = text
            .find("ZONE_CHANGE,2526,\"Algeth'ar Academy\",23")
            .expect("the key's zone-out line");
        let path = tmp.0.join("openkey.txt");
        std::fs::write(&path, text.as_bytes().get(..cut).unwrap()).unwrap();

        let cache = IndexCache::new(tmp.0.join("idx"));
        let mut file = std::fs::File::open(&path).unwrap();
        let cold = cache.scan_file(&path, &mut file);
        let open = cold
            .checkpoint
            .visit
            .as_ref()
            .expect("a visit is open at EOF");
        assert!(open.keyed);
        assert!(open.pars_ms.is_some(), "the key's timers are known");

        let mut file = std::fs::File::open(&path).unwrap();
        let warm = cache.scan_file(&path, &mut file);
        assert_eq!(warm.checkpoint.visit, cold.checkpoint.visit);
        assert_eq!(warm.segments, cold.segments);
        assert_eq!(warm.open_visit, cold.open_visit);
    }
}
