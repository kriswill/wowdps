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
    /// `[start, end)` file offsets; replaying exactly these bytes through the
    /// meter reproduces this segment.
    pub byte_range: (u64, u64),
    /// Byte ranges of earlier state-carrying lines (SPELL_SUMMON,
    /// COMBATANT_INFO, COMBAT_LOG_VERSION) that must be replayed BEFORE the
    /// slice so pet ownership, names and classes resolve exactly as they do in
    /// a full replay. These lines are rare, so this stays small.
    pub seeds: Vec<(u64, u64)>,
}

/// The product of one scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    /// Closed segments, oldest first.
    pub segments: Vec<SegmentMeta>,
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
    /// State-carrying seed lines seen before `offset`.
    pub seeds: Vec<(u64, u64)>,
    /// Mirror of the meter's trash-gap clock as of `offset`.
    pub last_combat_ms: Option<i64>,
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
        .map(|mut l| {
            if l.last() == Some(&b'\r') {
                l = &l[..l.len() - 1];
            }
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
        open: None,
        last_combat_ms: state.last_combat_ms,
        seeds: state.seeds,
        ckpt: (0, 0, state.last_combat_ms, state.offset),
    };
    sc.ckpt = (sc.segments.len(), sc.seeds.len(), sc.last_combat_ms, base);
    let mut buf: Vec<u8> = Vec::with_capacity(2 * CHUNK);
    let mut chunk = vec![0u8; CHUNK];

    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break, // an unreadable tail is simply not indexed
        };
        buf.extend_from_slice(&chunk[..n]);

        let mut start = 0usize;
        while let Some(nl) = memchr(b'\n', &buf[start..]) {
            let (s, e) = (start, start + nl);
            let mut line = &buf[s..e];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
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

fn memchr(needle: u8, hay: &[u8]) -> Option<usize> {
    hay.iter().position(|&b| b == needle)
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
}

#[derive(Default)]
struct Scanner {
    segments: Vec<SegmentMeta>,
    open: Option<OpenSeg>,
    /// Mirrors `Meter::last_combat_ms`: drives the trash gap rule and the
    /// close timestamp of a gap-split segment.
    last_combat_ms: Option<i64>,
    /// State-carrying lines seen so far (see `SegmentMeta::seeds`).
    seeds: Vec<(u64, u64)>,
    /// Latest clean boundary: (segments seen, seeds seen, gap clock, offset).
    /// Materialized into `Index::checkpoint` by `finish`.
    ckpt: (usize, usize, Option<i64>, u64),
}

impl Scanner {
    /// `off`/`end` are the file offsets of the line's first byte and one past
    /// its newline; `line` has the newline (and any `\r`) stripped.
    fn line(&mut self, off: u64, end: u64, line: &[u8]) {
        let Some((prefix, rest)) = split_prefix(line) else {
            return;
        };
        let token_len = memchr(b',', rest).unwrap_or(rest.len());
        let Ok(event) = std::str::from_utf8(&rest[..token_len]) else {
            return;
        };

        match event {
            "COMBAT_LOG_VERSION" => {
                // R6: hard boundary. The version line belongs to the segment it
                // closes so a lazy replay of that slice also sees the close; it
                // is also a seed so later slices replay the owner-map reset.
                if let Some(ts) = ts_of(prefix) {
                    self.close(ts, None, end);
                    self.last_combat_ms = None;
                    self.seeds.push((off, end));
                }
            }
            // Not combat, but they carry state later segments depend on:
            // pet ownership + names, and player classes.
            "SPELL_SUMMON" | "COMBATANT_INFO" => self.seeds.push((off, end)),
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
            }
        }
    }

    /// Mirror of `Meter::name_trash`'s tally: count group damage events per
    /// hostile target so `meta` can name the pull. Fields 1/5/6 of a damage
    /// line are srcGUID / dstGUID / dstName.
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
            });
        }
        self.last_combat_ms = Some(ts);
        if let Some(o) = self.open.as_mut() {
            o.last_ms = o.last_ms.max(ts);
        }
    }

    /// Mirror of `Meter::close`; `end_off` is where this segment's bytes end.
    fn close(&mut self, ts: i64, success: Option<bool>, end_off: u64) {
        let Some(o) = self.open.take() else { return };
        self.segments.push(meta(&o, Some(ts), success, end_off));
    }

    /// Record a clean boundary after a fully processed line: with no segment
    /// open, (segments, seeds, gap clock, offset) is everything a resumed
    /// scan needs to carry on as if it had read the whole file.
    fn mark(&mut self, end: u64) {
        if self.open.is_none() {
            self.ckpt = (
                self.segments.len(),
                self.seeds.len(),
                self.last_combat_ms,
                end,
            );
        }
    }

    fn finish(self, scanned: u64) -> Index {
        let open = self.open.as_ref().map(|o| meta(o, None, None, scanned));
        let live_offset = self.open.as_ref().map_or(scanned, |o| o.start_off);
        let (seg_n, seed_n, gap, offset) = self.ckpt;
        let checkpoint = ScanState {
            segments: self.segments[..seg_n].to_vec(),
            seeds: self.seeds[..seed_n].to_vec(),
            last_combat_ms: gap,
            offset,
        };
        Index {
            segments: self.segments,
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
            let id = ascii_u32(f[f.len() - 6]);
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
    // R7: Encounter duration runs to its close; Trash to its last combat event.
    let end_for_duration = match o.kind {
        SegmentKind::Encounter => end_ms.unwrap_or(o.last_ms),
        SegmentKind::Trash => o.last_ms,
    };
    // Trash earns its display name from the enemy tally, like a live replay.
    let name = match o.kind {
        SegmentKind::Trash => trash_name(&o.enemies).unwrap_or_else(|| o.name.clone()),
        SegmentKind::Encounter => o.name.clone(),
    };
    SegmentMeta {
        kind: o.kind,
        name,
        start_ms: o.start_ms,
        end_ms,
        success,
        duration_ms: (end_for_duration - o.start_ms).max(0),
        byte_range: (o.start_off, end_off),
        seeds: o.seeds.clone(),
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
    let rest = &line[idx + skip..];
    let trimmed = rest.iter().position(|&b| b != b' ').unwrap_or(rest.len());
    Some((&line[..idx], &rest[trimmed..]))
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
    while i < rest.len() {
        match rest[i] {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                out.push(trim_quotes(&rest[start..i]));
                if out.len() >= max {
                    return out;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(trim_quotes(&rest[start..]));
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
        let lines = vec![
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
        let enc = std::str::from_utf8(&joined.as_bytes()[e0 as usize..e1 as usize]).unwrap();
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
        let lines = vec![
            at(0, 0, HIT),
            at(0, 30, "COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1"),
            at(0, 40, HIT),
        ];
        let joined = lines.join("\n") + "\n";
        let idx = scan(&mut joined.as_bytes());
        assert_eq!(idx.segments.len(), 1);
        let (s0, s1) = idx.segments[0].byte_range;
        let slice = std::str::from_utf8(&joined.as_bytes()[s0 as usize..s1 as usize]).unwrap();
        assert!(
            slice.contains("COMBAT_LOG_VERSION"),
            "the closing version line belongs to the closed segment: {slice}"
        );
        assert!(idx.open.is_some(), "combat after the seam reopens");
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
        let lines = vec![at(0, 0, HIT), at(0, 1, HIT), at(0, 2, HIT)];
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
    fn real_log_scans_in_under_a_second() {
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
        assert!(scan_ms < 1_000, "scan took {scan_ms} ms");

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

    #[test]
    fn the_relog_fixture_survives_index_then_lazy_parse() {
        parity(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/relog.txt"));
    }
}
