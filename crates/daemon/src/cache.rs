//! The one piece of persistence in scope: the structural index's resumable
//! checkpoint, serialized with the wire primitives (no new format), keyed by
//! file identity plus a checksum of the bytes just before the checkpoint.
//! On a warm start only the tail `[checkpoint.offset, EOF)` is rescanned.
//!
//! Deliberately NOT cached: parsed segment `Meter`s. They rebuild from the
//! log in milliseconds, and serializing per-actor hashmaps is how a cache
//! becomes an event store by accident.
//!
//! The entry does keep the edition the scan stood in at the checkpoint
//! (`ScanState::project_id`, which picks the tables a resumed scan reads):
//! a resumed scan starts past the version line that named it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use wowdps_core::index::{self, Index, ScanState, SegmentMeta};
use wowdps_model::Edition;
use wowdps_proto::wire::{self, Reader};

/// Cache files kept per edition (v49: the edition each entry's checkpoint
/// stood in, `ScanState::project_id`, reckoned as `Edition::counts_as`);
/// the entries of older logs are evicted (each entry wears its log's mtime,
/// [`IndexCache::scan_file`]) so "newest logs warm, ancient ones rescan"
/// falls out of the layout, and one edition's logs never push another's
/// out.
const KEEP: usize = 8;

/// Bytes hashed before the checkpoint to prove the prefix is unchanged.
const CHECK_WINDOW: u64 = 64 * 1024;

// \x02: R10 added visit tracking (meta.visit, overalls, open-visit state).
// Old entries fail the magic check and cost one full rescan, nothing more.
// \x0c: ScanState gained R13's `arena_over` (and \x0b before it: the arena
// verdict rule changed to faction-based home side, invalidating cached
// success flags).
// \x0d: SegmentMeta gained `encounter` (id / difficulty / group size).
// \x0e: R10 made a finished keystone terminal — `VisitScan.ended_ms`, and
// cached visits closed by the old rule would place the Overall wrongly.
// \x0f: R10's START became authoritative (a door logged at difficulty 0 no
// longer loses the key) — a checkpoint scanned by the old rule carries no
// keyed visit for such a run, and the sweep imports through this cache.
// \x10: R10 keys a run joined mid-way from its finished END — a checkpoint
// scanned by the old rule holds that run as a plain, unkeyed zone visit.
// \x11: R10's ENCOUNTER_START settles its visit (a door logged at 0 onto a
// raid, a delve, Timewalking) and is a seed when it moves it; ScanState
// gained `last_zone_map`. A checkpoint scanned by the old rule holds those
// pulls outside any visit and the visit before them open.
// \x12: R10 reads a door onto an open-world map (Map.db2) as zoned out — a
// checkpoint scanned by the old rule holds a city visit opened by a hearth
// stamped with the raid's difficulty, and the raid visit it closed.
// \x13: (v45, R29) every WORLD_MARKER line is a seed — a checkpoint scanned
// by the old rule holds none, and a pull cut over it would start with no
// markers standing.
// \x14: (R8) the scan reads its log's own edition's tables — a checkpoint
// scanned by the old rule read a Forever log's doors and keys by Retail's
// open-world and keystone tables, so its visits may differ. ScanState
// gained `project_id`, stored as a u8 after `arena_over`.
// \x15: R10's pull resumes the visit on its own instance whatever map the
// last door named (Forever can log a door into an instance at difficulty 0
// onto another map id) — a checkpoint scanned by the old rule holds those pulls
// outside the visit they rejoin.
const MAGIC: &[u8; 8] = b"WDPSIDX\x15";

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
        wire::put_opt(&mut buf, state.last_zone_map.as_ref(), |b, m| {
            wire::put_u32(b, *m)
        });
        wire::put_bool(&mut buf, state.arena_over);
        wire::put_u8(&mut buf, state.project_id);

        let entry = self.cache_path(path);
        let _ = write_atomic(&entry, &buf);
        // The prune ranks entries by their mtime: each wears its log's, so
        // the newest logs keep their checkpoints whatever order they were
        // scanned in — the history's start-up sweep scans newest first,
        // which would otherwise leave the oldest logs warm and rescan the
        // tailed one on every start.
        if let Ok(mtime) = meta.modified() {
            let _ = File::options()
                .write(true)
                .open(&entry)
                .and_then(|f| f.set_modified(mtime));
        }
        self.prune();
    }

    /// Keep the [`KEEP`] entries of each edition (v49) whose logs were
    /// written last, by the entry's mtime, which is its log's. An
    /// entry's edition is its checkpoint's, read from the entry decoded
    /// whole ([`entry_edition`]); one this build cannot decode (another
    /// magic, a short or torn file) counts as Retail's, where every entry
    /// counted before editions were told apart.
    fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut groups: std::collections::HashMap<Edition, Vec<(std::time::SystemTime, PathBuf)>> =
            std::collections::HashMap::new();
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "bin") {
                continue;
            }
            let Some(mtime) = e.metadata().ok().and_then(|m| m.modified().ok()) else {
                continue;
            };
            groups
                .entry(entry_edition(&path))
                .or_default()
                .push((mtime, path));
        }
        for mut files in groups.into_values() {
            if files.len() <= KEEP {
                continue;
            }
            files.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));
            for (_, path) in files.split_off(KEEP) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The edition a cache entry's checkpoint stood in, as it is reckoned: the
/// `ScanState::project_id` of an entry this build decodes whole, else —
/// another magic, a short or torn file — Retail's.
fn entry_edition(path: &Path) -> Edition {
    let project_id = std::fs::read(path)
        .ok()
        .and_then(|bytes| decode(&bytes))
        .map_or(0, |(.., state)| state.project_id);
    Edition::from_project_id(project_id).counts_as()
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
    let last_zone_map = rd.opt(|r| r.u32()).ok()?;
    let arena_over = rd.bool().ok()?;
    let project_id = rd.u8().ok()?;
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
            last_zone_map,
            arena_over,
            project_id,
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
    wire::put_opt(buf, v.ended_ms.as_ref(), |b, e| wire::put_i64(b, *e));
    wire::put_bool(buf, v.joined);
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
        ended_ms: rd.opt(|r| r.i64())?,
        joined: rd.bool()?,
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
    wire::put_opt(buf, m.encounter.as_ref(), |b, e| {
        wire::put_u32(b, e.id);
        wire::put_u32(b, e.difficulty);
        wire::put_u32(b, e.group_size);
    });
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
        encounter: rd.opt(|r| {
            Ok(wowdps_core::model::Encounter {
                id: r.u32()?,
                difficulty: r.u32()?,
                group_size: r.u32()?,
            })
        })?,
    })
}

/// Write `bytes` to `target` through a uniquely named sibling `.tmp` and a
/// rename, so a reader never sees a partial file and two writers of the
/// same target (the tail thread and the start-up sweep both checkpoint an
/// index) never share a temp file. Shared by the index cache and the
/// history store — the one durability primitive in the daemon.
pub(crate) fn write_atomic(target: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}-{seq}.tmp", std::process::id()));
    let tmp = target.with_file_name(name);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, target)
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

    /// The entry keeps the edition the checkpoint stood in, and a warm
    /// start reads the tail by its tables: a Forever log's door onto a map
    /// only Forever's table calls open world (2980) is zoned out after the
    /// resume as in a full scan. Read as Retail's (the id dropped or
    /// ignored), the same resume opens a second visit.
    #[test]
    fn a_warm_start_reads_the_tail_by_the_logs_own_edition() {
        let tmp = Temp::new("edition");
        let log = tmp.0.join("WoWCombatLog.txt");
        let head = "11/1/2026 19:00:00.000-8  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18\n\
                    11/1/2026 19:00:01.000-8  ZONE_CHANGE,48,\"Blackfathom Deeps\",1\n";
        std::fs::write(&log, head).unwrap();
        let cache = IndexCache::new(tmp.0.join("cache"));
        let cold = cache.scan_file(&log, &mut File::open(&log).unwrap());
        assert_eq!(cold.checkpoint.project_id, 18);
        assert_eq!(cold.checkpoint.offset, head.len() as u64);
        let stored = std::fs::read(cache.cache_path(&log)).unwrap();
        let (_, _, _, _, decoded) = decode(&stored).unwrap();
        assert_eq!(decoded.project_id, 18, "stored");
        assert_eq!(stored.last(), Some(&18), "the entry's last byte");

        let door = "11/1/2026 19:30:00.000-8  ZONE_CHANGE,2980,\"Dalaran City\",1\n";
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(door.as_bytes()).unwrap();
        drop(f);
        let state = cache
            .resume_state(&log, &mut File::open(&log).unwrap())
            .expect("the entry resumes");
        assert_eq!(state.project_id, 18);
        let warm = cache.scan_file(&log, &mut File::open(&log).unwrap());
        let full = index::scan(&mut File::open(&log).unwrap());
        assert_eq!(warm, full);
        assert_eq!(full.checkpoint.visit_count, 1, "the door zoned out");
        assert_eq!(warm.checkpoint.project_id, 18);

        // The same resume with the edition lost reads the door by Retail's
        // table, where 2980 is no open-world map: a second visit.
        let lost = ScanState {
            project_id: 0,
            ..state
        };
        let mut file = File::open(&log).unwrap();
        file.seek(SeekFrom::Start(lost.offset)).unwrap();
        let wrong = index::scan_from(&mut file, lost);
        assert_eq!(wrong.checkpoint.visit_count, 2);
        assert_ne!(wrong, full);
    }

    /// An entry written under the previous magic (`\x14`, scanned before a
    /// pull rejoined its visit behind a door onto another map) is refused
    /// whole, however well formed the rest of it is, and the log is scanned
    /// again from its start.
    #[test]
    fn an_entry_of_the_previous_magic_is_refused_and_rescanned() {
        let tmp = Temp::new("oldmagic");
        let log = tmp.0.join("WoWCombatLog.txt");
        std::fs::copy(FIXTURE, &log).unwrap();
        let cache = IndexCache::new(tmp.0.join("cache"));
        cache.scan_file(&log, &mut File::open(&log).unwrap());
        let entry = cache.cache_path(&log);
        let mut bytes = std::fs::read(&entry).unwrap();
        assert!(
            cache
                .resume_state(&log, &mut File::open(&log).unwrap())
                .is_some(),
            "the entry as written resumes"
        );
        assert_eq!(bytes[7], 0x15);
        bytes[7] = 0x14;
        std::fs::write(&entry, &bytes).unwrap();
        assert!(
            cache
                .resume_state(&log, &mut File::open(&log).unwrap())
                .is_none(),
            "the previous magic is refused"
        );
        let rescanned = cache.scan_file(&log, &mut File::open(&log).unwrap());
        let full = index::scan(&mut File::open(&log).unwrap());
        assert_eq!(rescanned, full);
        assert_eq!(
            std::fs::read(&entry).unwrap()[7],
            0x15,
            "rewritten under the new magic"
        );
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

    /// The newest logs keep their checkpoints whatever order they are
    /// scanned in: a sweep newest first (the history's on every start)
    /// leaves the eight newest warm, the tailed one among them, and the
    /// oldest to rescan — not the other way round.
    #[test]
    fn the_newest_logs_keep_their_checkpoints_however_scanned() {
        let tmp = Temp::new("prune-order");
        let cache = IndexCache::new(tmp.0.join("cache"));
        let now = std::time::SystemTime::now();
        let logs: Vec<PathBuf> = (0..(KEEP + 4))
            .map(|i| {
                let log = tmp.0.join(format!("WoWCombatLog-{i:02}.txt"));
                std::fs::copy(FIXTURE, &log).unwrap();
                let hours = (KEEP + 4 - i) as u64;
                File::options()
                    .write(true)
                    .open(&log)
                    .unwrap()
                    .set_modified(now - std::time::Duration::from_secs(3600 * hours))
                    .unwrap();
                log
            })
            .collect();
        for log in logs.iter().rev() {
            cache.scan_file(log, &mut File::open(log).unwrap());
        }
        let warm: Vec<usize> = (0..logs.len())
            .filter(|i| cache.cache_path(&logs[*i]).exists())
            .collect();
        assert_eq!(warm, (4..KEEP + 4).collect::<Vec<_>>());
        // Rescanning an old log evicts nothing newer: its own entry goes.
        cache.scan_file(&logs[0], &mut File::open(&logs[0]).unwrap());
        assert!(!cache.cache_path(&logs[0]).exists());
        assert!(cache.cache_path(&logs[KEEP + 3]).exists());
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

    /// v49: eight logs per edition, not eight in all — a night of Retail
    /// logs never evicts Forever's, and each edition keeps its own newest.
    #[test]
    fn each_edition_keeps_its_own_newest_entries() {
        let tmp = Temp::new("prune-editions");
        let cache = IndexCache::new(tmp.0.join("cache"));
        let forever_fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");
        let mut forever = Vec::new();
        for i in 0..3 {
            let log = tmp.0.join(format!("WoWCombatLog-forever-{i}.txt"));
            std::fs::copy(forever_fixture, &log).unwrap();
            cache.scan_file(&log, &mut File::open(&log).unwrap());
            forever.push(log);
        }
        for i in 0..(KEEP + 4) {
            let log = tmp.0.join(format!("WoWCombatLog-{i}.txt"));
            std::fs::copy(FIXTURE, &log).unwrap();
            cache.scan_file(&log, &mut File::open(&log).unwrap());
        }
        let mut by_edition: std::collections::HashMap<Edition, usize> = Default::default();
        for e in std::fs::read_dir(tmp.0.join("cache")).unwrap().flatten() {
            *by_edition.entry(entry_edition(&e.path())).or_default() += 1;
        }
        assert_eq!(by_edition.get(&Edition::Retail), Some(&KEEP));
        assert_eq!(by_edition.get(&Edition::Forever), Some(&3), "none evicted");
        for log in &forever {
            assert!(cache.cache_path(log).exists(), "{}", log.display());
        }
        // An entry this build cannot read counts as Retail's: another magic,
        // and one wearing this build's magic but too short to be an entry,
        // whatever its last byte says.
        let odd = tmp.0.join("cache").join("odd.bin");
        std::fs::write(&odd, b"WDPSIDX\x01\x12").unwrap();
        assert_eq!(entry_edition(&odd), Edition::Retail);
        let short = tmp.0.join("cache").join("short.bin");
        let mut bytes = MAGIC.to_vec();
        bytes.push(Edition::Forever.project_id());
        std::fs::write(&short, &bytes).unwrap();
        assert_eq!(entry_edition(&short), Edition::Retail);
        // A whole Forever entry, torn short of its last bytes, is unreadable.
        let whole = std::fs::read(cache.cache_path(&forever[0])).unwrap();
        assert_eq!(whole.last(), Some(&Edition::Forever.project_id()));
        let torn = tmp.0.join("cache").join("torn.bin");
        let mut cut = whole[..whole.len() - 3].to_vec();
        cut.push(Edition::Forever.project_id());
        std::fs::write(&torn, &cut).unwrap();
        assert_eq!(entry_edition(&torn), Edition::Retail);
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
