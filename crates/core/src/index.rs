//! Fast structural scan of a combat log: segment boundaries and byte ranges,
//! no per-event parsing. This is what lets the app show every encounter in a
//! 300 MB log in well under a second, then lazily parse only the segment the
//! user opens (`load_range` + `Meter::feed`).
//!
//! The scanner is a deliberate mirror of `Meter::feed`'s segmentation rules
//! (ENCOUNTER_START/END, R6 version boundaries, R7 trash gaps) so that
//! index-then-lazy-parse and a full replay agree on the segment list. That
//! parity is asserted against the fixtures in the tests below.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crate::meter::{
    CC_SPELLS, NON_HEALING_ABSORBS, SegmentKind, TRASH_GAP_MS, is_friendly_source,
    is_hostile_target, trash_name,
};
use crate::parser::{is_damage_event, is_guid, parse_timestamp};

/// One segment as seen by the scanner: everything the list screen shows, plus
/// the byte range to feed through the real parser when the user opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentMeta {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    /// `None` only on the trailing open segment (`Index::open`).
    pub end_ms: Option<i64>,
    pub success: Option<bool>,
    /// R7 semantics: Encounter = START..END, Trash = first..last combat event.
    pub duration_ms: i64,
    /// R10, keyed Overall metas only: the dungeon's (par, +2, +3) timers.
    pub pars_ms: Option<(i64, i64, i64)>,
    /// R11: worth a list row (mirrors `Segment::counts`) — false for Trash
    /// that closed with no enemy damage and no player death.
    pub counts: bool,
    /// `[start, end)` file offsets; replaying exactly these bytes through the
    /// meter reproduces this segment.
    pub byte_range: (u64, u64),
    /// Byte ranges of earlier state-carrying lines (SPELL_SUMMON,
    /// COMBATANT_INFO, COMBAT_LOG_VERSION, and R10's ZONE_CHANGE /
    /// CHALLENGE_MODE lines) that must be replayed BEFORE the slice so pet
    /// ownership, names, classes and visit context resolve exactly as they
    /// do in a full replay. These lines are rare, so this stays small.
    pub seeds: Vec<(u64, u64)>,
    /// R10: ordinal of the instance visit this segment belongs to. On an
    /// `Overall` meta: the visit it aggregates (its byte range spans the
    /// whole visit; replaying it and merging the members with this ordinal
    /// reproduces the Overall).
    pub visit: Option<u32>,
}

/// R10: the scanner's open-visit state — everything a resumed scan (or
/// `finish`) needs to keep tracking the visit and eventually emit its
/// Overall meta. Mirrors `Meter`'s visit rules exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisitScan {
    pub ordinal: u32,
    pub map_id: u32,
    pub difficulty: u32,
    pub name: String,
    pub key_level: Option<u32>,
    pub keyed: bool,
    pub completed: Option<bool>,
    /// CHALLENGE_MODE_END's totalMs — the official key time.
    pub official_ms: Option<i64>,
    /// The dungeon's (par, +2, +3) timers (generated MapChallengeMode table).
    pub pars_ms: Option<(i64, i64, i64)>,
    pub start_ms: i64,
    pub start_off: u64,
    /// Sum of closed member durations (R7 semantics per member).
    pub dur_ms: i64,
    pub members: u32,
    /// Seed count at visit open: the Overall meta's seeds are `seeds[..n]`.
    pub seed_n: usize,
    /// False while suspended (zoned out mid-visit).
    pub zoned_in: bool,
}

/// The product of one scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    /// Closed segments, oldest first.
    pub segments: Vec<SegmentMeta>,
    /// R10: Overall metas of closed visits, oldest first. Kept separate from
    /// `segments` (their byte ranges overlap their members'); the daemon
    /// interleaves them into the list at display time.
    pub overalls: Vec<SegmentMeta>,
    /// R10: the visit still in progress at end of scan, as an Overall meta
    /// covering `[visit start, live_offset)` — the prefix the live tail
    /// cannot see: closed members only, in bytes and in clock. Present only
    /// once the visit has a closed member. The visit's live side (the open
    /// member included, in full) continues in the meter; a consumer merging
    /// prefix + live gets exactly one copy of everything.
    pub open_visit: Option<SegmentMeta>,
    /// The trailing segment still open at end of scan, if any. Its lines are
    /// replayed by the live meter, not lazily loaded.
    pub open: Option<SegmentMeta>,
    /// Where the live tail should start emitting lines: the open segment's
    /// start, or the end of the scan when nothing is open.
    pub live_offset: u64,
    /// Bytes consumed (end of the last complete line).
    pub scanned: u64,
    /// Resumable scanner state at the last clean boundary (see [`ScanState`]).
    pub checkpoint: ScanState,
}

/// Scanner state at a clean boundary — the end of a line with no segment
/// open — from which a later `scan_from` reproduces exactly what a full scan
/// of the same bytes would have produced. This is what the daemon's index
/// cache persists so a 300 MB log costs one full scan per file, ever, not one
/// per daemon start.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanState {
    /// Closed segments as of `offset`, oldest first.
    pub segments: Vec<SegmentMeta>,
    /// R10: Overall metas of visits closed before `offset`.
    pub overalls: Vec<SegmentMeta>,
    /// State-carrying seed lines seen before `offset`.
    pub seeds: Vec<(u64, u64)>,
    /// Mirror of the meter's trash-gap clock as of `offset`.
    pub last_combat_ms: Option<i64>,
    /// R10: visits opened so far (assigns the next ordinal).
    pub visit_count: u32,
    /// R10: the visit in progress at `offset`, if any.
    pub visit: Option<VisitScan>,
    /// File offset the state describes; resume reading here.
    pub offset: u64,
}

/// Everything `Meter::feed` needs to reproduce one segment: the seed lines
/// (pet summons, combatant info, version seams) followed by the slice itself.
pub fn load_segment(path: &Path, meta: &SegmentMeta) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    for &range in &meta.seeds {
        lines.extend(load_range(path, range)?);
    }
    lines.extend(load_range(path, meta.byte_range)?);
    Ok(lines)
}

/// Read one segment's raw lines for lazy parsing. Plain seek + bounded read.
pub fn load_range(path: &Path, range: (u64, u64)) -> io::Result<Vec<String>> {
    let (start, end) = range;
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0u8; end.saturating_sub(start) as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let l = l.strip_suffix(b"\r").unwrap_or(l);
            String::from_utf8_lossy(l).into_owned()
        })
        .collect())
}

/// Scan a whole reader (normally a `File` positioned at 0). The caller keeps
/// the handle and can seek back to `live_offset` afterwards to start tailing.
pub fn scan<R: Read>(reader: &mut R) -> Index {
    scan_from(reader, ScanState::default())
}

/// Resume a scan from a [`ScanState`] checkpoint. The reader must be
/// positioned at `state.offset`; the bytes from there on are scanned as a
/// continuation, and the result is identical to a full scan of the whole
/// file. Gated by the checkpoint-parity fixture test below.
pub fn scan_from<R: Read>(reader: &mut R, state: ScanState) -> Index {
    let mut base: u64 = state.offset; // file offset of buf[0]
    let mut sc = Scanner {
        segments: state.segments,
        overalls: state.overalls,
        open: None,
        last_combat_ms: state.last_combat_ms,
        seeds: state.seeds,
        visit_count: state.visit_count,
        visit: state.visit,
        ckpt: Ckpt::default(),
    };
    sc.ckpt = Ckpt {
        seg_n: sc.segments.len(),
        overall_n: sc.overalls.len(),
        seed_n: sc.seeds.len(),
        last_combat_ms: sc.last_combat_ms,
        visit_count: sc.visit_count,
        visit: sc.visit.clone(),
        offset: base,
    };
    let mut buf: Vec<u8> = Vec::with_capacity(2 * CHUNK);
    let mut chunk = vec![0u8; CHUNK];

    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break, // an unreadable tail is simply not indexed
        };
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());

        let mut start = 0usize;
        while let Some(nl) = buf.get(start..).and_then(|tail| memchr(b'\n', tail)) {
            let (s, e) = (start, start + nl);
            let line = buf.get(s..e).unwrap_or_default();
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            sc.line(base + s as u64, base + e as u64 + 1, line);
            sc.mark(base + e as u64 + 1);
            start = e + 1;
        }
        buf.drain(..start);
        base += start as u64;
    }
    sc.finish(base)
}

const CHUNK: usize = 1024 * 1024;

/// Word-at-a-time byte search. The scan spends about a quarter of its time
/// here — 4.4M newlines over a 1.2 GB log — and a byte-at-a-time loop gave up
/// 4x against this: 325 ms vs 81 ms on that log.
///
/// The kernel is the classic zero-byte test `(x - 0x01..01) & !x & 0x80..80`
/// applied to `x = word ^ needle`, so a set high bit marks a matching byte.
/// Words are assembled little-endian on every target, which makes the lowest
/// set bit the lowest *address* — so `trailing_zeros` finds the first match
/// on a big-endian host too. Safe and stdlib-only; no `unsafe` needed to get
/// the win, since the bounds check LLVM cannot elide is the one this removes
/// by looking at eight bytes per iteration instead of one.
fn memchr(needle: u8, hay: &[u8]) -> Option<usize> {
    const N: usize = size_of::<usize>();
    const LO: usize = usize::from_ne_bytes([0x01; N]);
    const HI: usize = usize::from_ne_bytes([0x80; N]);
    let rep = usize::from_ne_bytes([needle; N]);

    let mut off = 0usize;
    let mut words = hay.chunks_exact(N);
    for c in &mut words {
        let Ok(bytes) = <[u8; N]>::try_from(c) else {
            break;
        };
        let x = usize::from_le_bytes(bytes) ^ rep;
        let m = x.wrapping_sub(LO) & !x & HI;
        if m != 0 {
            return Some(off + m.trailing_zeros() as usize / 8);
        }
        off += N;
    }
    words
        .remainder()
        .iter()
        .position(|&b| b == needle)
        .map(|i| off + i)
}

/// The open segment being tracked.
struct OpenSeg {
    kind: SegmentKind,
    name: String,
    start_ms: i64,
    start_off: u64,
    /// Timestamp of the last combat event inside (R7 trash duration).
    last_ms: i64,
    /// Snapshot of the scanner's seed lines taken when this segment opened.
    seeds: Vec<(u64, u64)>,
    /// Mirror of `Segment::enemies` — damage-event counts per hostile name,
    /// so scanned Trash segments get the same Details-style display name a
    /// live replay would compute.
    enemies: std::collections::HashMap<String, u64>,
    /// R10: the visit this segment opened inside, mirroring `Segment::visit`.
    visit: Option<u32>,
    /// R11: a player died inside (UNIT_DIED already passed `is_combat`'s
    /// player check) — with `enemies` and `pvp`, the meta's `counts` verdict.
    deaths: bool,
    /// R11 mirror of `Segment::pvp`: player-vs-player damage, self excluded.
    pvp: bool,
}

/// Scanner state at the latest clean boundary, materialized into
/// `Index::checkpoint` by `finish`. Counts index into the append-only vecs.
#[derive(Default, Clone)]
struct Ckpt {
    seg_n: usize,
    overall_n: usize,
    seed_n: usize,
    last_combat_ms: Option<i64>,
    visit_count: u32,
    visit: Option<VisitScan>,
    offset: u64,
}

#[derive(Default)]
struct Scanner {
    segments: Vec<SegmentMeta>,
    /// R10: Overall metas of closed visits.
    overalls: Vec<SegmentMeta>,
    open: Option<OpenSeg>,
    /// Mirrors `Meter::last_combat_ms`: drives the trash gap rule and the
    /// close timestamp of a gap-split segment.
    last_combat_ms: Option<i64>,
    /// State-carrying lines seen so far (see `SegmentMeta::seeds`).
    seeds: Vec<(u64, u64)>,
    /// R10: mirrors `Meter::visits`' length (assigns the next ordinal).
    visit_count: u32,
    /// R10: the visit in progress, mirroring the meter's visit rules.
    visit: Option<VisitScan>,
    ckpt: Ckpt,
}

impl Scanner {
    /// `off`/`end` are the file offsets of the line's first byte and one past
    /// its newline; `line` has the newline (and any `\r`) stripped.
    fn line(&mut self, off: u64, end: u64, line: &[u8]) {
        let Some((prefix, rest)) = split_prefix(line) else {
            return;
        };
        let token_len = memchr(b',', rest).unwrap_or(rest.len());
        let Ok(event) = std::str::from_utf8(rest.get(..token_len).unwrap_or_default()) else {
            return;
        };

        match event {
            "COMBAT_LOG_VERSION" => {
                // R6: hard boundary. The version line belongs to the segment it
                // closes so a lazy replay of that slice also sees the close; it
                // is also a seed so later slices replay the owner-map reset.
                // The visit only suspends (see `Meter`): a mid-run /reload
                // must not split the key.
                if let Some(ts) = ts_of(prefix) {
                    self.close(ts, None, end);
                    if let Some(v) = self.visit.as_mut() {
                        v.zoned_in = false;
                    }
                    self.last_combat_ms = None;
                    self.seeds.push((off, end));
                }
            }
            // Not combat, but they carry state later segments depend on:
            // pet ownership + names, and player classes.
            "SPELL_SUMMON" | "COMBATANT_INFO" => self.seeds.push((off, end)),
            // R10: visit boundaries, mirroring `Meter::feed`'s zone rules.
            // All three are seeds — replaying them is what gives lazy slices
            // and the live meter their visit context.
            "ZONE_CHANGE" => {
                let Some(ts) = ts_of(prefix) else { return };
                self.close_trash(ts, off);
                let f = split_fields(rest, 4);
                let map_id = f.get(1).map_or(0, |s| ascii_u32(s));
                let difficulty = f.get(3).map_or(0, |s| ascii_u32(s));
                let seed_n = self.seeds.len();
                self.seeds.push((off, end));
                if difficulty == 0 {
                    if let Some(v) = self.visit.as_mut() {
                        v.zoned_in = false;
                    }
                } else if let Some(v) = self
                    .visit
                    .as_mut()
                    // A keyed visit resumes on the map alone (see `Meter`).
                    .filter(|v| v.map_id == map_id && (v.keyed || v.difficulty == difficulty))
                {
                    v.zoned_in = true;
                } else {
                    self.close_visit(Some(ts), off);
                    let name =
                        String::from_utf8_lossy(f.get(2).copied().unwrap_or(b"?")).into_owned();
                    self.open_visit_state(map_id, difficulty, name, None, ts, off, seed_n);
                }
            }
            "CHALLENGE_MODE_START" => {
                let Some(ts) = ts_of(prefix) else { return };
                let f = split_fields(rest, 5);
                let map_id = f.get(2).map_or(0, |s| ascii_u32(s));
                let challenge_id = f.get(3).map_or(0, |s| ascii_u32(s));
                let key_level = f.get(4).map_or(0, |s| ascii_u32(s));
                let seed_n = self.seeds.len();
                self.seeds.push((off, end));
                let Some(v) = self.visit.as_ref().filter(|v| v.map_id == map_id) else {
                    return;
                };
                // The dungeon reset and the key's clock starts: a visit
                // boundary, mirroring `Meter` — pre-key activity stays in
                // the closed visit.
                let (difficulty, name) = (v.difficulty, v.name.clone());
                self.close_trash(ts, off);
                self.close_visit(Some(ts), off);
                self.open_visit_state(map_id, difficulty, name, Some(key_level), ts, off, seed_n);
                if let Some(v) = self.visit.as_mut() {
                    v.pars_ms = crate::keystone_timers::pars_ms(challenge_id);
                }
            }
            "CHALLENGE_MODE_END" => {
                self.seeds.push((off, end));
                let f = split_fields(rest, 5);
                let map_id = f.get(1).map_or(0, |s| ascii_u32(s));
                let success = f.get(2).is_some_and(|s| truthy_bytes(s));
                let total_ms = f.get(4).map_or(0, |s| ascii_u32(s)) as i64;
                if let Some(v) = self
                    .visit
                    .as_mut()
                    .filter(|v| v.map_id == map_id && v.keyed)
                {
                    v.completed = Some(success);
                    v.official_ms = (total_ms > 0).then_some(total_ms);
                }
            }
            "ENCOUNTER_START" => {
                let Some(ts) = ts_of(prefix) else { return };
                let f = split_fields(rest, 3);
                let name = String::from_utf8_lossy(f.get(2).copied().unwrap_or(b"?")).into_owned();
                self.close(ts, None, off);
                self.open = Some(OpenSeg {
                    kind: SegmentKind::Encounter,
                    name,
                    start_ms: ts,
                    start_off: off,
                    last_ms: ts,
                    seeds: self.seeds.clone(),
                    enemies: Default::default(),
                    visit: self.member_visit(),
                    deaths: false,
                    pvp: false,
                });
                self.last_combat_ms = Some(ts);
            }
            "ENCOUNTER_END" => {
                let Some(ts) = ts_of(prefix) else { return };
                let f = split_fields(rest, 6);
                let success = f.get(5).is_some_and(|s| truthy_bytes(s));
                self.close(ts, Some(success), end);
            }
            _ => {
                if !is_combat(event, rest) {
                    return;
                }
                let Some(ts) = ts_of(prefix) else { return };
                self.ensure_combat(ts, off);
                self.tally_enemy(event, rest);
                if event == "UNIT_DIED"
                    && let Some(o) = self.open.as_mut()
                {
                    o.deaths = true;
                }
            }
        }
    }

    /// Mirror of `Meter::name_trash`'s tally: count group damage events per
    /// hostile target so `meta` can name the pull, and flag player-vs-player
    /// damage (R11). Fields 1/5/6 of a damage line are srcGUID / dstGUID /
    /// dstName.
    fn tally_enemy(&mut self, event: &str, rest: &[u8]) {
        if !is_damage_event(event) {
            return;
        }
        let Some(o) = self.open.as_mut() else { return };
        if o.kind != SegmentKind::Trash {
            return;
        }
        let f = split_fields(rest, 7);
        let (Some(src), Some(dst), Some(dst_name)) = (f.get(1), f.get(5), f.get(6)) else {
            return;
        };
        let (Ok(src), Ok(dst)) = (std::str::from_utf8(src), std::str::from_utf8(dst)) else {
            return;
        };
        if is_friendly_source(src) && is_hostile_target(dst) {
            let name = String::from_utf8_lossy(dst_name).into_owned();
            *o.enemies.entry(name).or_insert(0) += 1;
        }
        if is_friendly_source(src) && dst.starts_with("Player-") && src != dst {
            o.pvp = true;
        }
    }

    /// Mirror of `Meter::ensure_combat`.
    fn ensure_combat(&mut self, ts: i64, off: u64) {
        let need_new = match &self.open {
            None => true,
            Some(o) => {
                o.kind == SegmentKind::Trash
                    && self
                        .last_combat_ms
                        .is_some_and(|last| ts - last > TRASH_GAP_MS)
            }
        };
        if need_new {
            let close_at = self.last_combat_ms.unwrap_or(ts);
            self.close(close_at, None, off);
            self.open = Some(OpenSeg {
                kind: SegmentKind::Trash,
                name: "Trash".to_string(),
                start_ms: ts,
                start_off: off,
                last_ms: ts,
                seeds: self.seeds.clone(),
                enemies: Default::default(),
                visit: self.member_visit(),
                deaths: false,
                pvp: false,
            });
        }
        self.last_combat_ms = Some(ts);
        if let Some(o) = self.open.as_mut() {
            o.last_ms = o.last_ms.max(ts);
        }
    }

    /// Mirror of `Meter::close`; `end_off` is where this segment's bytes end.
    /// A member's R7 duration accumulates into its visit's Overall clock.
    fn close(&mut self, ts: i64, success: Option<bool>, end_off: u64) {
        let Some(o) = self.open.take() else { return };
        let m = meta(&o, Some(ts), success, end_off);
        if let Some(v) = self.visit.as_mut()
            && o.visit == Some(v.ordinal)
        {
            v.dur_ms += m.duration_ms;
            v.members += 1;
        }
        self.segments.push(m);
    }

    /// R10 mirror of `Meter::close_trash`: a zone change closes open Trash.
    fn close_trash(&mut self, ts: i64, off: u64) {
        if self
            .open
            .as_ref()
            .is_some_and(|o| o.kind == SegmentKind::Trash)
        {
            self.close(ts, None, off);
        }
    }

    /// R10 mirror of `Meter::close_visit`: emit the visit's Overall meta
    /// (visits that never had a member leave nothing behind).
    fn close_visit(&mut self, end_ms: Option<i64>, end_off: u64) {
        let Some(v) = self.visit.take() else { return };
        if v.members == 0 {
            return;
        }
        self.overalls
            .push(overall_meta(&v, &self.seeds, end_ms, end_off));
    }

    #[allow(clippy::too_many_arguments)]
    fn open_visit_state(
        &mut self,
        map_id: u32,
        difficulty: u32,
        name: String,
        key_level: Option<u32>,
        ts: i64,
        off: u64,
        seed_n: usize,
    ) {
        self.visit = Some(VisitScan {
            ordinal: self.visit_count,
            map_id,
            difficulty,
            name,
            key_level,
            keyed: key_level.is_some(),
            completed: None,
            official_ms: None,
            pars_ms: None,
            start_ms: ts,
            start_off: off,
            dur_ms: 0,
            members: 0,
            seed_n,
            zoned_in: true,
        });
        self.visit_count += 1;
    }

    /// The visit a segment opening right now would belong to.
    fn member_visit(&self) -> Option<u32> {
        self.visit
            .as_ref()
            .filter(|v| v.zoned_in)
            .map(|v| v.ordinal)
    }

    /// Record a clean boundary after a fully processed line: with no segment
    /// open, the checkpoint carries everything a resumed scan needs (an open
    /// *visit* is fine — its state travels in the checkpoint).
    fn mark(&mut self, end: u64) {
        if self.open.is_none() {
            self.ckpt = Ckpt {
                seg_n: self.segments.len(),
                overall_n: self.overalls.len(),
                seed_n: self.seeds.len(),
                last_combat_ms: self.last_combat_ms,
                visit_count: self.visit_count,
                visit: self.visit.clone(),
                offset: end,
            };
        }
    }

    fn finish(self, scanned: u64) -> Index {
        let open = self.open.as_ref().map(|o| meta(o, None, None, scanned));
        let live_offset = self.open.as_ref().map_or(scanned, |o| o.start_off);
        // R10: the in-progress visit surfaces once it has a *closed* member —
        // as the prefix the live tail cannot see: closed members' durations
        // only, bytes cut at `live_offset`. The still-open member is rebuilt
        // in full by the live meter (the tail replays from `live_offset`), so
        // counting any of it here would double count in every prefix + live
        // composition the daemon serves.
        let open_visit = self
            .visit
            .as_ref()
            .filter(|v| v.members > 0)
            .map(|v| overall_meta(v, &self.seeds, None, live_offset));
        let checkpoint = ScanState {
            segments: self
                .segments
                .get(..self.ckpt.seg_n)
                .unwrap_or_default()
                .to_vec(),
            overalls: self
                .overalls
                .get(..self.ckpt.overall_n)
                .unwrap_or_default()
                .to_vec(),
            seeds: self
                .seeds
                .get(..self.ckpt.seed_n)
                .unwrap_or_default()
                .to_vec(),
            last_combat_ms: self.ckpt.last_combat_ms,
            visit_count: self.ckpt.visit_count,
            visit: self.ckpt.visit,
            offset: self.ckpt.offset,
        };
        Index {
            segments: self.segments,
            overalls: self.overalls,
            open_visit,
            open,
            live_offset,
            scanned,
            checkpoint,
        }
    }
}

/// Would this line reach `Meter::record` (and thus open/extend a segment)?
/// Must match `Meter::feed` exactly; the fixture parity tests gate this.
fn is_combat(event: &str, rest: &[u8]) -> bool {
    if is_damage_event(event) {
        return true;
    }
    match event {
        "SPELL_INTERRUPT" | "SPELL_DISPEL" | "SPELL_STOLEN" => true,
        // R2: the four self-absorb spells never record as healing.
        "SPELL_HEAL" | "SPELL_PERIODIC_HEAL" => {
            let f = split_fields(rest, 10);
            let id = f.get(9).map_or(0, |s| ascii_u32(s));
            !NON_HEALING_ABSORBS.contains(&id)
        }
        "SPELL_ABSORBED" => {
            // Variable arity, indexed from the end like the parser.
            let f = split_fields(rest, usize::MAX);
            if f.len() < 19 {
                return false;
            }
            let id = f.get(f.len() - 6).map_or(0, |s| ascii_u32(s));
            !NON_HEALING_ABSORBS.contains(&id)
        }
        // Only CC debuffs record (CrowdControl view).
        "SPELL_AURA_APPLIED" => {
            let f = split_fields(rest, 32);
            let id = f.get(9).map_or(0, |s| ascii_u32(s));
            if !CC_SPELLS.contains(&id) {
                return false;
            }
            // The aura type sits after the (optional) advanced block.
            let kind = match f.get(12) {
                Some(s) if is_guid_bytes(s) => f.get(31).copied().unwrap_or(b""),
                Some(s) => *s,
                None => b"",
            };
            kind.eq_ignore_ascii_case(b"DEBUFF")
        }
        // Only player deaths record (Deaths view).
        "UNIT_DIED" => {
            let f = split_fields(rest, 8);
            let guid = f.get(5).copied().unwrap_or(b"");
            if guid.is_empty() || guid == b"0000000000000000" {
                return false;
            }
            let flags = f.get(7).map_or(0, |s| ascii_u32_hex(s));
            flags & 0x0000_0400 != 0 || guid.starts_with(b"Player-")
        }
        _ => false,
    }
}

fn meta(o: &OpenSeg, end_ms: Option<i64>, success: Option<bool>, end_off: u64) -> SegmentMeta {
    // R7: Encounter duration runs to its close; Trash to its last combat
    // event. (The scanner never opens an Overall segment.)
    let end_for_duration = match o.kind {
        SegmentKind::Encounter => end_ms.unwrap_or(o.last_ms),
        SegmentKind::Trash | SegmentKind::Overall => o.last_ms,
    };
    // Trash earns its display name from the enemy tally, like a live replay.
    let name = match o.kind {
        SegmentKind::Trash => trash_name(&o.enemies).unwrap_or_else(|| o.name.clone()),
        SegmentKind::Encounter | SegmentKind::Overall => o.name.clone(),
    };
    SegmentMeta {
        kind: o.kind,
        name,
        start_ms: o.start_ms,
        end_ms,
        success,
        duration_ms: (end_for_duration - o.start_ms).max(0),
        pars_ms: None,
        // R11 mirror of `Segment::counts`.
        counts: o.kind != SegmentKind::Trash || !o.enemies.is_empty() || o.pvp || o.deaths,
        byte_range: (o.start_off, end_off),
        seeds: o.seeds.clone(),
        visit: o.visit,
    }
}

/// R10: the Overall meta for a visit — kind `Overall`, byte range spanning
/// the visit, duration = accumulated member durations, except a keystone
/// run whose clock is the key timer (mirrors `Segment::duration_ms`). Its
/// display name matches `Visit::display_name` ("Skyreach +10" for keys).
fn overall_meta(
    v: &VisitScan,
    seeds: &[(u64, u64)],
    end_ms: Option<i64>,
    end_off: u64,
) -> SegmentMeta {
    let name = match v.key_level {
        Some(l) => format!("{} +{l}", v.name),
        None => v.name.clone(),
    };
    // Clock and verdict via the one implementation in `meter` — the meta
    // must equal what a lazy replay's `Meter::overall` will report.
    let twin = crate::meter::Visit {
        map_id: v.map_id,
        difficulty: v.difficulty,
        name: String::new(),
        key_level: v.key_level,
        keyed: v.keyed,
        start_ms: v.start_ms,
        end_ms,
        completed: v.completed,
        official_ms: v.official_ms,
        pars_ms: v.pars_ms,
    };
    let at = end_ms.unwrap_or(v.start_ms);
    SegmentMeta {
        kind: SegmentKind::Overall,
        name,
        start_ms: v.start_ms,
        end_ms,
        success: twin.verdict(at),
        duration_ms: twin.key_clock(at).unwrap_or(v.dur_ms),
        pars_ms: v.pars_ms,
        counts: true,
        byte_range: (v.start_off, end_off),
        seeds: seeds.get(..v.seed_n).unwrap_or_default().to_vec(),
        visit: Some(v.ordinal),
    }
}

/// Timestamp and event CSV separated by two spaces (a tab on some clients) —
/// the same split as `parse_line`.
fn split_prefix(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let two = line.windows(2).position(|w| w == b"  ");
    let tab = memchr(b'\t', line);
    let (idx, skip) = match (two, tab) {
        (Some(a), Some(b)) if b < a => (b, 1),
        (Some(a), _) => (a, 2),
        (None, Some(b)) => (b, 1),
        (None, None) => return None,
    };
    let rest = line.get(idx + skip..)?;
    let trimmed = rest.iter().position(|&b| b != b' ').unwrap_or(rest.len());
    Some((line.get(..idx)?, rest.get(trimmed..)?))
}

fn ts_of(prefix: &[u8]) -> Option<i64> {
    parse_timestamp(std::str::from_utf8(prefix).ok()?)
}

/// Quote-aware split of the CSV after the timestamp; `f[0]` is the event name,
/// matching the parser's indexing. Stops after `max` fields. Quotes are
/// stripped, like `split_csv`.
fn split_fields(rest: &[u8], max: usize) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let mut i = 0;
    while let Some(&b) = rest.get(i) {
        match b {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                out.push(trim_quotes(rest.get(start..i).unwrap_or_default()));
                if out.len() >= max {
                    return out;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(trim_quotes(rest.get(start..).unwrap_or_default()));
    out
}

fn trim_quotes(f: &[u8]) -> &[u8] {
    let f = f.strip_prefix(b"\"").unwrap_or(f);
    f.strip_suffix(b"\"").unwrap_or(f)
}

fn ascii_u32(s: &[u8]) -> u32 {
    std::str::from_utf8(s)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Flags are hex (`0x511`) or decimal, like `parser::parse_u32`.
fn ascii_u32_hex(s: &[u8]) -> u32 {
    let s = std::str::from_utf8(s).unwrap_or("");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}

fn is_guid_bytes(s: &[u8]) -> bool {
    std::str::from_utf8(s).is_ok_and(is_guid)
}

/// Combat-log booleans are `1` / `nil` / `0`.
fn truthy_bytes(s: &[u8]) -> bool {
    !matches!(s, b"nil" | b"0" | b"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::{Meter, View};
    use crate::parser::parse_line;

    /// Minutes/seconds into a synthetic log, advanced format, one player.
    fn at(min: i64, sec: i64, body: &str) -> String {
        format!("7/27/2026 21:{min:02}:{sec:02}.000-7  {body}")
    }

    const HIT: &str = r#"SPELL_DAMAGE,Player-1-A,"Ana-Realm",0x511,0x0,Creature-0-9,"Boss",0xa48,0x0,1449,"Frostbolt",16,12345,12000,0,0,0,0,0,1,nil,nil"#;

    fn scan_str(lines: &[String]) -> Index {
        let joined = lines.join("\n") + "\n";
        scan(&mut joined.as_bytes())
    }

    fn replay(lines: &[String]) -> Meter {
        let mut m = Meter::new();
        for l in lines {
            if let Some(p) = parse_line(l) {
                m.feed(p);
            }
        }
        m
    }

    #[test]
    fn an_encounter_is_indexed_with_success_and_r7_duration() {
        let lines = vec![
            at(0, 0, "COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1"),
            at(0, 5, r#"ENCOUNTER_START,3183,"Midnight Falls",16,20,2913"#),
            at(0, 10, HIT),
            at(1, 5, r#"ENCOUNTER_END,3183,"Midnight Falls",16,20,1,60000"#),
        ];
        let idx = scan_str(&lines);
        assert!(idx.open.is_none());
        assert_eq!(idx.segments.len(), 1);
        let s = &idx.segments[0];
        assert_eq!(s.kind, SegmentKind::Encounter);
        assert_eq!(s.name, "Midnight Falls");
        assert_eq!(s.success, Some(true));
        assert_eq!(s.duration_ms, 60_000);
        assert_eq!(idx.live_offset, idx.scanned);
    }

    #[test]
    fn byte_ranges_replay_to_exactly_their_own_segment() {
        let lines = [
            at(0, 0, HIT),  // trash
            at(0, 30, HIT), // same trash
            at(1, 0, r#"ENCOUNTER_START,1,"Boss",16,20,1"#),
            at(1, 10, HIT),
            at(2, 0, r#"ENCOUNTER_END,1,"Boss",16,20,0,50000"#),
        ];
        let joined = lines.join("\n") + "\n";
        let idx = scan(&mut joined.as_bytes());
        assert_eq!(idx.segments.len(), 2);

        // Trash range must exclude the ENCOUNTER_START line...
        let (t0, t1) = idx.segments[0].byte_range;
        let trash = &joined.as_bytes()[t0 as usize..t1 as usize];
        let text = std::str::from_utf8(trash).unwrap();
        assert!(!text.contains("ENCOUNTER_START"), "{text}");

        // ...and the encounter range must include its END line.
        let (e0, e1) = idx.segments[1].byte_range;
        let enc = joined.get(e0 as usize..e1 as usize).unwrap_or_default();
        assert!(enc.starts_with("7/"), "range starts at a line: {enc}");
        assert!(enc.contains("ENCOUNTER_START") && enc.contains("ENCOUNTER_END"));
    }

    #[test]
    fn a_long_lull_splits_trash_like_the_meter_does() {
        let lines = vec![
            at(0, 0, HIT),
            at(0, 20, HIT),
            at(2, 0, HIT), // 100s after the last hit: new segment
        ];
        let idx = scan_str(&lines);
        assert_eq!(idx.segments.len(), 1, "first trash closed by the gap");
        let closed = &idx.segments[0];
        assert_eq!(closed.kind, SegmentKind::Trash);
        assert_eq!(closed.duration_ms, 20_000, "first..last combat event");
        let open = idx.open.expect("second trash still open");
        assert_eq!(open.end_ms, None);
        assert_eq!(idx.live_offset, open.byte_range.0);
    }

    #[test]
    fn a_mid_log_version_line_is_a_hard_boundary_kept_in_its_segment() {
        let lines = [
            at(0, 0, HIT),
            at(0, 30, "COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1"),
            at(0, 40, HIT),
        ];
        let joined = lines.join("\n") + "\n";
        let idx = scan(&mut joined.as_bytes());
        assert_eq!(idx.segments.len(), 1);
        let (s0, s1) = idx.segments[0].byte_range;
        let slice = joined.get(s0 as usize..s1 as usize).unwrap_or_default();
        assert!(
            slice.contains("COMBAT_LOG_VERSION"),
            "the closing version line belongs to the closed segment: {slice}"
        );
        assert!(idx.open.is_some(), "combat after the seam reopens");
    }

    #[test]
    fn r11_counts_mirrors_the_meter() {
        let lines = vec![
            // Heal-only trash, closed by the 60s lull before the duel.
            at(
                0,
                0,
                r#"SPELL_HEAL,Player-1-A,"Ana",0x511,0x0,Player-1-B,"Bo",0x512,0x0,2061,"Flash Heal",0x2,Player-1-B,0000000000000000,612000,905000,18420,0,9800,0,0,0,1,60,100,0,-810.12,2148.30,2287,3.1416,639,500,0,0,0,nil"#,
            ),
            // A duel: player-vs-player damage counts (R11).
            at(
                2,
                0,
                r#"SPELL_DAMAGE,Player-1-A,"Ana",0x511,0x0,Player-1-B,"Bo",0x512,0x0,1449,"Frostbolt",16,777,700,0,0,0,0,0,1,nil,nil"#,
            ),
            at(4, 0, HIT),
        ];
        let idx = scan_str(&lines);
        assert_eq!(idx.segments.len(), 2, "heal-only and duel trash closed");
        assert!(!idx.segments[0].counts, "no enemy damage, no death, no pvp");
        assert!(idx.segments[1].counts, "the duel counts");
        assert!(
            idx.open.as_ref().is_some_and(|o| o.counts),
            "the real pull counts"
        );

        // The live meter agrees, segment for segment.
        let meter = replay(&lines);
        let replayed: Vec<bool> = meter.segments().iter().map(|s| s.counts()).collect();
        assert_eq!(replayed, vec![false, true, true]);
    }

    #[test]
    fn only_recordable_events_open_segments() {
        // None of these reach Meter::record, so none may open a segment.
        let quiet = vec![
            at(
                0,
                0,
                r#"SPELL_CAST_SUCCESS,Player-1-A,"Ana",0x511,0x0,Creature-0-9,"Boss",0xa48,0x0,116,"Frostbolt",16"#,
            ),
            // A buff, and a debuff that is not on the CC list.
            at(
                0,
                1,
                r#"SPELL_AURA_APPLIED,Player-1-A,"Ana",0x511,0x0,Player-1-A,"Ana",0x511,0x0,1459,"Arcane Intellect",64,BUFF"#,
            ),
            at(
                0,
                2,
                r#"SPELL_AURA_APPLIED,Player-1-A,"Ana",0x511,0x0,Creature-0-9,"Boss",0xa48,0x0,589,"Shadow Word: Pain",32,DEBUFF"#,
            ),
            // Stagger self-absorb heal (R2: excluded from healing).
            at(
                0,
                3,
                r#"SPELL_PERIODIC_HEAL,Player-1-A,"Ana",0x511,0x0,Player-1-A,"Ana",0x511,0x0,114556,"Purgatory",1,500,500,0,0,nil"#,
            ),
            // An NPC death.
            at(
                0,
                4,
                r#"UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Creature-0-9,"Boss",0xa48,0x0"#,
            ),
        ];
        let idx = scan_str(&quiet);
        assert!(idx.segments.is_empty() && idx.open.is_none(), "{idx:?}");

        // And their recordable twins all do.
        for body in [
            r#"SPELL_AURA_APPLIED,Player-1-A,"Ana",0x511,0x0,Creature-0-9,"Boss",0xa48,0x0,118,"Polymorph",64,DEBUFF"#,
            r#"SPELL_HEAL,Player-1-A,"Ana",0x511,0x0,Player-1-A,"Ana",0x511,0x0,2061,"Flash Heal",2,900,900,0,0,nil"#,
            r#"UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1-A,"Ana",0x511,0x0"#,
        ] {
            let idx = scan_str(&[at(0, 0, body)]);
            assert!(idx.open.is_some(), "expected combat: {body}");
        }
    }

    #[test]
    fn a_trailing_partial_line_is_not_scanned() {
        let mut joined = at(0, 0, HIT) + "\n";
        let complete = joined.len() as u64;
        joined.push_str("7/27/2026 21:00:05.000-7  SPELL_DA"); // cut mid-line
        let idx = scan(&mut joined.as_bytes());
        assert_eq!(idx.scanned, complete);
    }

    #[test]
    fn load_range_returns_exactly_the_requested_lines() {
        let dir = std::env::temp_dir().join(format!("wowdps-idx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.txt");
        let lines = [at(0, 0, HIT), at(0, 1, HIT), at(0, 2, HIT)];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let start = (lines[0].len() + 1) as u64;
        let end = start + (lines[1].len() + 1) as u64;
        let got = load_range(&path, (start, end)).unwrap();
        assert_eq!(got, vec![lines[1].clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- fixture parity: index-then-lazy-parse == full replay --------------

    fn parity(path: &str) {
        let bytes = std::fs::read(path).expect("fixture exists");
        let idx = scan(&mut &bytes[..]);
        let lines: Vec<String> = String::from_utf8_lossy(&bytes)
            .lines()
            .map(str::to_string)
            .collect();
        let full = replay(&lines);
        let now = lines
            .iter()
            .filter_map(|l| parse_line(l))
            .map(|p| p.ts_ms)
            .max()
            .unwrap_or(0);

        let metas: Vec<SegmentMeta> = idx
            .segments
            .iter()
            .cloned()
            .chain(idx.open.clone())
            .collect();
        assert_eq!(metas.len(), full.segments().len(), "segment count");

        for (meta, seg) in metas.iter().zip(full.segments()) {
            assert_eq!(meta.kind, seg.kind, "{}", meta.name);
            assert_eq!(meta.name, seg.name);
            assert_eq!(meta.start_ms, seg.start_ms, "{}", meta.name);
            assert_eq!(meta.end_ms, seg.end_ms, "{}", meta.name);
            assert_eq!(meta.success, seg.success, "{}", meta.name);
            assert_eq!(meta.duration_ms, seg.duration_ms(now), "{}", meta.name);
        }

        // Lazily parsing each slice (seeds first, like load_segment) must
        // reproduce the same numbers.
        for (meta, seg) in metas.iter().zip(full.segments()) {
            let ranges = meta.seeds.iter().chain([&meta.byte_range]);
            let slice_lines: Vec<String> = ranges
                .flat_map(|&(a, b)| {
                    String::from_utf8_lossy(&bytes[a as usize..b as usize])
                        .lines()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect();
            let lazy = replay(&slice_lines);
            assert_eq!(
                lazy.segments().len(),
                1,
                "one segment per slice: {}",
                meta.name
            );
            let ls = &lazy.segments()[0];
            // The scan's display name and a live replay's must agree, or the
            // list row would not match the header after lazy loading.
            assert_eq!(ls.name, meta.name, "display-name parity");

            for view in [
                View::Damage,
                View::Healing,
                View::Interrupts,
                View::CrowdControl,
                View::Dispels,
                View::Deaths,
            ] {
                let want = seg.rows(view);
                let got = ls.rows(view);
                assert_eq!(got.len(), want.len(), "{:?} rows in {}", view, meta.name);
                for (g, w) in got.iter().zip(&want) {
                    assert_eq!(g.key, w.key, "{:?} in {}", view, meta.name);
                    assert_eq!(g.label, w.label, "{:?} in {}", view, meta.name);
                    assert_eq!(
                        g.amount, w.amount,
                        "{} {:?} in {}",
                        g.label, view, meta.name
                    );
                    assert_eq!(g.extra, w.extra, "{} {:?} in {}", g.label, view, meta.name);
                    assert!(
                        (g.per_sec - w.per_sec).abs() < 0.01,
                        "{}: {} vs {}",
                        g.label,
                        g.per_sec,
                        w.per_sec
                    );
                    // Seeded COMBATANT_INFO lines make class colors exact even
                    // in trash slices; R8 inference is segment-local, so it
                    // must agree too — spec included.
                    assert_eq!(g.class, w.class, "{} class in {}", g.label, meta.name);
                    assert_eq!(g.spec, w.spec, "{} spec in {}", g.label, meta.name);
                }
                // The drilldown numbers must survive lazy loading too.
                if let Some(top) = want.first() {
                    let (ws, wt) = seg.breakdown(&top.key, view);
                    let (gs, gt) = ls.breakdown(&top.key, view);
                    let flat = |rows: &[crate::meter::Row]| {
                        rows.iter()
                            .map(|r| (r.label.clone(), r.amount, r.extra, r.hp, r.gain))
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(flat(&gs), flat(&ws), "{:?} by-spell in {}", view, meta.name);
                    assert_eq!(
                        flat(&gt),
                        flat(&wt),
                        "{:?} by-target in {}",
                        view,
                        meta.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_sample_fixture_survives_index_then_lazy_parse() {
        parity(crate::testkit::FIXTURE);
    }

    /// Cutting the file anywhere and resuming from the prefix's checkpoint
    /// must agree with a full scan — the invariant the daemon's index cache
    /// stands on.
    #[test]
    fn a_resumed_scan_matches_a_full_scan_from_any_cut() {
        let bytes = std::fs::read(crate::testkit::FIXTURE).unwrap();
        let full = scan(&mut &bytes[..]);
        // Cut at every line boundary (plus mid-line for good measure).
        let cuts: Vec<usize> = bytes
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .map(|(i, _)| i + 1)
            .chain([bytes.len() / 2, bytes.len()])
            .collect();
        for cut in cuts {
            let prefix = scan(&mut &bytes[..cut]);
            let state = prefix.checkpoint.clone();
            let off = state.offset as usize;
            let resumed = scan_from(&mut &bytes[off..], state);
            assert_eq!(resumed.segments, full.segments, "cut at {cut}");
            assert_eq!(resumed.open, full.open, "cut at {cut}");
            assert_eq!(resumed.live_offset, full.live_offset, "cut at {cut}");
            assert_eq!(resumed.scanned, full.scanned, "cut at {cut}");
            assert_eq!(resumed.checkpoint, full.checkpoint, "cut at {cut}");
        }
    }

    /// Manual perf gate against a real log. Run with:
    /// `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -- --ignored real_log`
    #[test]
    #[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
    fn real_log_scans_at_speed() {
        let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
        let mut file = std::fs::File::open(&path).unwrap();
        let size = file.metadata().unwrap().len();

        let t = std::time::Instant::now();
        let idx = scan(&mut file);
        let scan_ms = t.elapsed().as_millis();

        let enc: Vec<&SegmentMeta> = idx
            .segments
            .iter()
            .filter(|m| m.kind == SegmentKind::Encounter)
            .collect();
        println!(
            "{} MB, {} segments ({} encounters), scanned in {scan_ms} ms",
            size / (1024 * 1024),
            idx.segments.len(),
            enc.len(),
        );
        assert!(!idx.segments.is_empty(), "a real log has segments");
        // The contract is "a 300 MB+ log lists its segments in under a second",
        // but a flat second is a budget that depends on which log you point
        // this at. Hold the second as a floor and scale past it at 500 MB/s —
        // under a third of the ~1.65 GB/s this scanner sustains, so the gate
        // catches a real regression without tripping on a slow disk.
        let size_mb = size / (1024 * 1024);
        let budget_ms = u128::from(size_mb * 2).max(1_000);
        assert!(
            scan_ms < budget_ms,
            "scan took {scan_ms} ms for {size_mb} MB (budget {budget_ms} ms)"
        );

        // Loading the biggest boss pull must also be sub-second.
        let biggest = enc
            .iter()
            .max_by_key(|m| m.byte_range.1 - m.byte_range.0)
            .expect("has encounters");
        let t = std::time::Instant::now();
        let lines = load_segment(Path::new(&path), biggest).unwrap();
        let meter = {
            let mut m = Meter::new();
            for l in &lines {
                if let Some(p) = parse_line(l) {
                    m.feed(p);
                }
            }
            m
        };
        let load_ms = t.elapsed().as_millis();
        let rows = meter.segments()[0].rows(View::Damage);
        println!(
            "biggest pull {:?}: {} MB, loaded+parsed in {load_ms} ms, {} damage rows",
            biggest.name,
            (biggest.byte_range.1 - biggest.byte_range.0) / (1024 * 1024),
            rows.len(),
        );
        assert!(load_ms < 1_000, "lazy load took {load_ms} ms");
    }

    /// The word-at-a-time kernel must agree with the byte-at-a-time one it
    /// replaced, especially where a match straddles a word boundary or lands
    /// in the sub-word remainder.
    #[test]
    fn memchr_matches_a_byte_at_a_time_search() {
        let naive = |n: u8, h: &[u8]| h.iter().position(|&b| b == n);
        // Lengths either side of the 8-byte word, and a match at every
        // position in each — plus the no-match case.
        for len in 0..40usize {
            let base = vec![b'x'; len];
            assert_eq!(memchr(b'\n', &base), naive(b'\n', &base), "none, len {len}");
            for at in 0..len {
                let mut h = base.clone();
                h[at] = b'\n';
                assert_eq!(memchr(b'\n', &h), Some(at), "len {len}, at {at}");
                // A second, later match must not win over the first.
                if at + 1 < len {
                    let mut h2 = h.clone();
                    h2[len - 1] = b'\n';
                    assert_eq!(memchr(b'\n', &h2), Some(at), "first of two, len {len}");
                }
            }
        }
        // High-bit bytes must not be mistaken for matches: the 0x80 test is
        // the classic trap for a SWAR search over non-ASCII input.
        let utf8 = "héllo\nwörld".as_bytes();
        assert_eq!(memchr(b'\n', utf8), naive(b'\n', utf8));
        for n in [0u8, 0x80, 0xff, b'|'] {
            let h = [0x00, 0x80, 0xff, b'|', 0x7f, 0x80, 0x01, 0xfe, 0x80, b'|'];
            assert_eq!(memchr(n, &h), naive(n, &h), "needle {n:#04x}");
        }
    }

    #[test]
    fn the_relog_fixture_survives_index_then_lazy_parse() {
        parity(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/relog.txt"));
    }
}
