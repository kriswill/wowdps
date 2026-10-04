//! v39: the history store's SERIES tier — `series/<fight id>.bin`, a
//! fight's abilities and targets second by second (the model's
//! [`SeriesRow`]s) for every player of a kill, a key or a pinned fight: what
//! lets a stored pull's drill answer a zoom window as the live one does.
//!
//! Binary, unlike every other tier, because it is the one tier sized by
//! seconds × abilities × players (a 7-minute, 25-player pull is ~75 000
//! cells: 1.3 MB as JSON, ~0.4 MB here), and because nothing reads it but
//! a window: SQL keeps the coarse series, and no fixed answer needs these.
//! Laid out for a reader that wants ONE player:
//!
//! ```text
//! file  := "WDSR" | u8 format | u32 LE index_len | index | block*
//! index := varint players | (str guid | varint offset | varint len)*
//!          (offset from the first block)
//! block := rows(damage) | rows(heal) | rows(targets)          (format 1)
//!        | spells | tallies(damage) | tallies(heal) | taken   (format 2)
//! spells  := varint n | (str ability key | rows(its targets))*
//! tallies := varint n | (str ability key | varint school | varint m
//!            | (str target | varint amount | varint extra
//!               | varint count | varint crits)*)*
//! taken := varint seconds | varint*                            (dense)
//! rows  := varint n | row*
//! row   := str key | varint spell_id | varint school | varint cells
//!        | varint runs | (varint gap | varint len)*   (seconds, as runs)
//!        | (varint amount | packed)*                  (one per second)
//!        | u8 extra mode | extra
//! packed := (crits << 4 | count) when 1 ≤ count ≤ 15 and crits ≤ 15,
//!           else 0x00 | varint count | varint crits
//! extra := mode 0 none | 1 a varint per second
//!        | 2 varint k | (varint index gap | varint extra)*  (sparse)
//! str   := varint len | utf-8
//! ```
//!
//! A run's gap is from the end of the run before it (the first's, from
//! second 0). Overkill is all but always zero, so a damage row stores it
//! sparse; overheal is everywhere, so a heal row stores it dense — each row
//! takes whichever is smaller. Decoding never panics and never allocates
//! more than the bytes in hand can fill: a lying count is `None`.
//!
//! v42 (format 2) adds what opens an ability on a stored pull as the live
//! meter does — each Damage ability's targets second by second (an opened
//! ability's windowed list and its stack by target), every ability's
//! whole-fight targets on Damage and Healing (its list over the whole
//! fight) — and the player's damage taken on the 1 s grid (the Taken
//! drill's and a Taken comparison's curve, where the rows tier keeps 10 s).
//! A format-1 file still reads, those parts empty, and the store rewrites
//! it from its log ([`format_of`]).

use wowdps_model::series::{SeriesCell, SeriesRow, SpellTallies, SpellTargets, TargetTally};

/// The file's first four bytes.
pub const MAGIC: &[u8; 4] = b"WDSR";
/// The layout above, as this build writes it. A reader takes it and every
/// older one ([`OLDEST`]); the store reads any other as absent (the pull
/// answers its whole lists).
pub const FORMAT: u8 = 2;
/// The oldest layout a reader takes: v39's, without format 2's parts.
pub const OLDEST: u8 = 1;
/// The fixed head: magic, format, the index's length.
pub const HEAD_LEN: usize = 9;

/// One player's seconds: their abilities on Damage and on Healing, the
/// enemies their damage landed on, and (format 2) each ability's targets
/// and what they took.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerSeries {
    pub guid: String,
    pub damage: Vec<SeriesRow>,
    pub heal: Vec<SeriesRow>,
    pub targets: Vec<SeriesRow>,
    /// v42: each Damage ability's enemy targets second by second
    /// (`Segment::spell_targets_all`).
    pub spells: Vec<SpellTargets>,
    /// v42: each Damage ability's whole-fight targets
    /// (`Segment::spell_tallies_all`).
    pub damage_tallies: Vec<SpellTallies>,
    /// v42: each Healing ability's whole-fight targets.
    pub heal_tallies: Vec<SpellTallies>,
    /// v42: damage taken on the 1 s grid (`Segment::taken_timeline`'s
    /// buckets), trailing seconds and all.
    pub taken: Vec<u64>,
}

impl PlayerSeries {
    /// Nothing to keep: a player who dealt, healed and took nothing.
    pub fn is_empty(&self) -> bool {
        self.damage.is_empty()
            && self.heal.is_empty()
            && self.targets.is_empty()
            && self.spells.is_empty()
            && self.damage_tallies.is_empty()
            && self.heal_tallies.is_empty()
            && self.taken.is_empty()
    }
}

/// A fight's series tier, every player's block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FightSeries {
    pub players: Vec<PlayerSeries>,
}

impl FightSeries {
    pub fn encode(&self) -> Vec<u8> {
        self.encode_as(FORMAT)
    }

    /// The file in `format`: an older one leaves out the parts it lacks —
    /// how a test writes the files older builds left on disk.
    pub fn encode_as(&self, format: u8) -> Vec<u8> {
        let mut blocks: Vec<u8> = Vec::new();
        let mut index: Vec<u8> = Vec::new();
        put_varint(&mut index, self.players.len() as u64);
        for p in &self.players {
            let at = blocks.len();
            for rows in [&p.damage, &p.heal, &p.targets] {
                put_rows(&mut blocks, rows);
            }
            if format >= 2 {
                put_varint(&mut blocks, p.spells.len() as u64);
                for s in &p.spells {
                    put_str(&mut blocks, &s.key);
                    put_rows(&mut blocks, &s.targets);
                }
                for tallies in [&p.damage_tallies, &p.heal_tallies] {
                    put_tallies(&mut blocks, tallies);
                }
                put_varint(&mut blocks, p.taken.len() as u64);
                for v in &p.taken {
                    put_varint(&mut blocks, *v);
                }
            }
            put_str(&mut index, &p.guid);
            put_varint(&mut index, at as u64);
            put_varint(&mut index, (blocks.len() - at) as u64);
        }
        let mut out = Vec::with_capacity(HEAD_LEN + index.len() + blocks.len());
        out.extend_from_slice(MAGIC);
        out.push(format);
        out.extend_from_slice(&(index.len() as u32).to_le_bytes());
        out.extend_from_slice(&index);
        out.extend_from_slice(&blocks);
        out
    }

    /// The whole file. `None` for anything this format does not describe.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (format, index_len) = head(bytes.get(..HEAD_LEN)?)?;
        let index = Index::decode(bytes.get(HEAD_LEN..HEAD_LEN.checked_add(index_len)?)?)?;
        let base = HEAD_LEN + index_len;
        let mut players = Vec::new();
        for e in &index.entries {
            let from = base.checked_add(usize::try_from(e.offset).ok()?)?;
            let to = from.checked_add(usize::try_from(e.len).ok()?)?;
            players.push(decode_block(&e.guid, bytes.get(from..to)?, format)?);
        }
        Some(Self { players })
    }
}

/// v42: the layout a file's head names, whether or not this build reads it
/// — what tells the store a file older than [`FORMAT`] wants rewriting
/// from its log, and one NEWER (a later build's) is left alone. `None`
/// for bytes that are no series head at all.
pub fn format_of(head_bytes: &[u8]) -> Option<u8> {
    (head_bytes.len() >= HEAD_LEN && head_bytes.get(..4)? == MAGIC)
        .then(|| head_bytes.get(4).copied())
        .flatten()
}

/// One player's block through `read(offset, len)` — the head, the index,
/// then that block alone, so a window on one player of a raid reads a few
/// kilobytes of the file. `None` when the file is absent or refused; a
/// player the file does not hold had no seconds (an empty block — a
/// window on them answers empty lists, not the whole fight).
pub fn read_player(
    read: impl Fn(u64, usize) -> Option<Vec<u8>>,
    guid: &str,
) -> Option<PlayerSeries> {
    let (format, index_len) = head(&read(0, HEAD_LEN)?)?;
    let index = Index::decode(&read(HEAD_LEN as u64, index_len)?)?;
    let Some(e) = index.entries.iter().find(|e| e.guid == guid) else {
        return Some(PlayerSeries {
            guid: guid.to_string(),
            ..PlayerSeries::default()
        });
    };
    let at = (HEAD_LEN + index_len) as u64 + e.offset;
    let block = read(at, usize::try_from(e.len).ok()?)?;
    decode_block(guid, &block, format)
}

/// The format and the index's length from the fixed head, checking the
/// magic and that a reader takes the format.
fn head(head: &[u8]) -> Option<(u8, usize)> {
    let format = *head.get(4)?;
    if head.len() < HEAD_LEN || head.get(..4)? != MAGIC || !(OLDEST..=FORMAT).contains(&format) {
        return None;
    }
    let len: [u8; 4] = head.get(5..9)?.try_into().ok()?;
    Some((format, usize::try_from(u32::from_le_bytes(len)).ok()?))
}

struct Entry {
    guid: String,
    offset: u64,
    len: u64,
}

struct Index {
    entries: Vec<Entry>,
}

impl Index {
    fn decode(bytes: &[u8]) -> Option<Self> {
        let mut c = Cur::new(bytes);
        let n = c.count(3)?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            entries.push(Entry {
                guid: c.str()?,
                offset: c.varint()?,
                len: c.varint()?,
            });
        }
        c.done().then_some(Self { entries })
    }
}

fn decode_block(guid: &str, bytes: &[u8], format: u8) -> Option<PlayerSeries> {
    let mut c = Cur::new(bytes);
    let mut p = PlayerSeries {
        guid: guid.to_string(),
        damage: rows(&mut c)?,
        heal: rows(&mut c)?,
        targets: rows(&mut c)?,
        ..PlayerSeries::default()
    };
    if format >= 2 {
        // A key and a row count: two bytes at least.
        let n = c.count(2)?;
        p.spells = Vec::with_capacity(n);
        for _ in 0..n {
            p.spells.push(SpellTargets {
                key: c.str()?,
                targets: rows(&mut c)?,
            });
        }
        p.damage_tallies = tallies(&mut c)?;
        p.heal_tallies = tallies(&mut c)?;
        let n = c.count(1)?;
        p.taken = Vec::with_capacity(n);
        for _ in 0..n {
            p.taken.push(c.varint()?);
        }
    }
    c.done().then_some(p)
}

// ---- tallies ----------------------------------------------------------------------

fn put_tallies(out: &mut Vec<u8>, tallies: &[SpellTallies]) {
    put_varint(out, tallies.len() as u64);
    for s in tallies {
        put_str(out, &s.key);
        put_varint(out, u64::from(s.school));
        put_varint(out, s.targets.len() as u64);
        for t in &s.targets {
            put_str(out, &t.target);
            put_varint(out, t.amount);
            put_varint(out, t.extra);
            put_varint(out, t.count);
            put_varint(out, t.crits);
        }
    }
}

fn tallies(c: &mut Cur) -> Option<Vec<SpellTallies>> {
    // A key, a school and a count: three bytes at least.
    let n = c.count(3)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let key = c.str()?;
        let school = u32::try_from(c.varint()?).ok()?;
        // A name and four numbers: five bytes at least.
        let m = c.count(5)?;
        let mut targets = Vec::with_capacity(m);
        for _ in 0..m {
            targets.push(TargetTally {
                target: c.str()?,
                amount: c.varint()?,
                extra: c.varint()?,
                count: c.varint()?,
                crits: c.varint()?,
            });
        }
        out.push(SpellTallies {
            key,
            school,
            targets,
        });
    }
    Some(out)
}

// ---- rows -------------------------------------------------------------------------

fn put_rows(out: &mut Vec<u8>, rows: &[SeriesRow]) {
    put_varint(out, rows.len() as u64);
    for r in rows {
        put_row(out, r);
    }
}

fn put_row(out: &mut Vec<u8>, r: &SeriesRow) {
    put_str(out, &r.key);
    put_varint(out, u64::from(r.spell_id));
    put_varint(out, u64::from(r.school));
    put_varint(out, r.cells.len() as u64);
    // The seconds as maximal runs.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for c in &r.cells {
        match runs.last_mut() {
            Some((start, len)) if start.saturating_add(*len) == c.bucket => *len += 1,
            _ => runs.push((c.bucket, 1)),
        }
    }
    put_varint(out, runs.len() as u64);
    let mut end = 0u32;
    for (start, len) in &runs {
        put_varint(out, u64::from(start.saturating_sub(end)));
        put_varint(out, u64::from(*len));
        end = start.saturating_add(*len);
    }
    for c in &r.cells {
        put_varint(out, c.amount);
        if (1..=15).contains(&c.count) && c.crits <= 15 {
            out.push(((c.crits as u8) << 4) | c.count as u8);
        } else {
            out.push(0);
            put_varint(out, c.count);
            put_varint(out, c.crits);
        }
    }
    // Extra, dense or sparse — whichever is smaller; none at all when zero.
    let set: Vec<(usize, u64)> = r
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.extra > 0)
        .map(|(i, c)| (i, c.extra))
        .collect();
    if set.is_empty() {
        out.push(0);
        return;
    }
    let mut dense = Vec::new();
    for c in &r.cells {
        put_varint(&mut dense, c.extra);
    }
    let mut sparse = Vec::new();
    put_varint(&mut sparse, set.len() as u64);
    let mut last = 0usize;
    for (i, v) in &set {
        put_varint(&mut sparse, (i - last) as u64);
        put_varint(&mut sparse, *v);
        last = *i;
    }
    if dense.len() <= sparse.len() {
        out.push(1);
        out.extend_from_slice(&dense);
    } else {
        out.push(2);
        out.extend_from_slice(&sparse);
    }
}

fn rows(c: &mut Cur) -> Option<Vec<SeriesRow>> {
    let n = c.count(6)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(row(c)?);
    }
    Some(out)
}

fn row(c: &mut Cur) -> Option<SeriesRow> {
    let key = c.str()?;
    let spell_id = u32::try_from(c.varint()?).ok()?;
    let school = u32::try_from(c.varint()?).ok()?;
    // Every second is at least two bytes on disk.
    let n = c.count(2)?;
    let runs = c.count(2)?;
    let mut buckets: Vec<u32> = Vec::with_capacity(n);
    let mut end = 0u32;
    for _ in 0..runs {
        let start = end.checked_add(u32::try_from(c.varint()?).ok()?)?;
        let len = u32::try_from(c.varint()?).ok()?;
        if buckets.len() + len as usize > n {
            return None;
        }
        buckets.extend(start..start.checked_add(len)?);
        end = start + len;
    }
    if buckets.len() != n {
        return None;
    }
    let mut cells = Vec::with_capacity(n);
    for bucket in buckets {
        let amount = c.varint()?;
        let (count, crits) = match c.u8()? {
            0 => (c.varint()?, c.varint()?),
            b => (u64::from(b & 0x0f), u64::from(b >> 4)),
        };
        if count == 0 {
            return None;
        }
        cells.push(SeriesCell {
            bucket,
            amount,
            extra: 0,
            count,
            crits,
        });
    }
    match c.u8()? {
        0 => {}
        1 => {
            for cell in &mut cells {
                cell.extra = c.varint()?;
            }
        }
        2 => {
            let k = c.count(2)?;
            let mut at = 0usize;
            for _ in 0..k {
                at = at.checked_add(usize::try_from(c.varint()?).ok()?)?;
                cells.get_mut(at)?.extra = c.varint()?;
            }
        }
        _ => return None,
    }
    Some(SeriesRow {
        key,
        spell_id,
        school,
        cells,
    })
}

// ---- varints ----------------------------------------------------------------------

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// A bounds-checked cursor: every read is `None` past the end.
struct Cur<'a> {
    b: &'a [u8],
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b }
    }

    fn u8(&mut self) -> Option<u8> {
        let (&first, rest) = self.b.split_first()?;
        self.b = rest;
        Some(first)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            v |= u64::from(byte & 0x7f).checked_shl(shift)?;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    /// A count of items each at least `min` bytes long: more than the
    /// bytes left could hold is a lie, and refused before any allocation.
    fn count(&mut self, min: usize) -> Option<usize> {
        let n = usize::try_from(self.varint()?).ok()?;
        (n.saturating_mul(min) <= self.b.len()).then_some(n)
    }

    fn str(&mut self) -> Option<String> {
        let len = usize::try_from(self.varint()?).ok()?;
        if len > self.b.len() {
            return None;
        }
        let (s, rest) = self.b.split_at(len);
        self.b = rest;
        String::from_utf8(s.to_vec()).ok()
    }

    fn done(&self) -> bool {
        self.b.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(bucket: u32, amount: u64, extra: u64, count: u64, crits: u64) -> SeriesCell {
        SeriesCell {
            bucket,
            amount,
            extra,
            count,
            crits,
        }
    }

    fn sample() -> FightSeries {
        FightSeries {
            players: vec![
                PlayerSeries {
                    guid: "Player-1-A".into(),
                    damage: vec![SeriesRow {
                        key: "Firebolt\u{0}Imp".into(),
                        spell_id: 3110,
                        school: 4,
                        // Two runs, a big amount, a killing blow's overkill,
                        // and a second too busy to pack.
                        cells: vec![
                            cell(0, 100, 0, 1, 0),
                            cell(1, 7_000_000_000, 0, 3, 2),
                            cell(9, 60, 1_554, 1, 1),
                            cell(10, 5, 0, 40, 17),
                        ],
                    }],
                    heal: vec![SeriesRow {
                        key: "Flash Heal".into(),
                        spell_id: 2061,
                        school: 2,
                        cells: vec![cell(3, 250, 50, 1, 0), cell(4, 200, 9, 1, 1)],
                    }],
                    targets: vec![SeriesRow {
                        key: "Ulgrax".into(),
                        spell_id: 0,
                        school: 0,
                        cells: vec![cell(0, 100, 0, 1, 0)],
                    }],
                    spells: vec![SpellTargets {
                        key: "Firebolt\u{0}Imp".into(),
                        targets: vec![
                            SeriesRow {
                                key: "Ulgrax".into(),
                                spell_id: 0,
                                school: 4,
                                cells: vec![cell(0, 100, 0, 1, 0), cell(1, 9, 0, 1, 1)],
                            },
                            SeriesRow {
                                key: "Spitting Larva".into(),
                                spell_id: 0,
                                school: 4,
                                cells: vec![cell(10, 5, 0, 40, 17)],
                            },
                        ],
                    }],
                    damage_tallies: vec![SpellTallies {
                        key: "Firebolt\u{0}Imp".into(),
                        school: 4,
                        targets: vec![
                            TargetTally {
                                target: "Spitting Larva".into(),
                                amount: 5,
                                extra: 0,
                                count: 40,
                                crits: 17,
                            },
                            TargetTally {
                                target: "Ulgrax".into(),
                                amount: 7_000_000_160,
                                extra: 1_554,
                                count: 5,
                                crits: 3,
                            },
                        ],
                    }],
                    heal_tallies: vec![SpellTallies {
                        key: "Flash Heal".into(),
                        school: 2,
                        targets: vec![TargetTally {
                            target: "Ana".into(),
                            amount: 450,
                            extra: 59,
                            count: 2,
                            crits: 1,
                        }],
                    }],
                    // A trailing quiet second is kept: the live curve has it.
                    taken: vec![0, 0, 1_200, 0, 98_000_000, 0],
                },
                PlayerSeries {
                    guid: "Player-1-B".into(),
                    ..PlayerSeries::default()
                },
            ],
        }
    }

    /// Every field survives the trip, whole or one player at a time.
    #[test]
    fn a_fight_round_trips_and_reads_one_player() {
        let s = sample();
        let bytes = s.encode();
        assert_eq!(FightSeries::decode(&bytes), Some(s.clone()));
        let read = |at: u64, len: usize| {
            let at = at as usize;
            bytes.get(at..at + len).map(<[u8]>::to_vec)
        };
        assert_eq!(read_player(read, "Player-1-A"), s.players.first().cloned());
        assert_eq!(read_player(read, "Player-1-B"), s.players.get(1).cloned());
        let nobody = read_player(read, "Player-1-C").expect("a readable file");
        assert!(nobody.is_empty(), "no seconds, not no answer");
        assert_eq!(read_player(|_, _| None, "Player-1-A"), None, "no file");
    }

    /// v42: a format-1 file — every series v39 wrote — still reads, with
    /// format 2's parts empty, and says which format it is so the store
    /// can rewrite it.
    #[test]
    fn a_format_one_file_reads_without_the_new_parts() {
        let s = sample();
        let old = s.encode_as(1);
        assert_eq!(format_of(&old), Some(1));
        assert_eq!(format_of(&s.encode()), Some(FORMAT));
        let mut want = s.clone();
        for p in &mut want.players {
            p.spells.clear();
            p.damage_tallies.clear();
            p.heal_tallies.clear();
            p.taken.clear();
        }
        assert_eq!(FightSeries::decode(&old), Some(want.clone()));
        let read = |at: u64, len: usize| {
            let at = at as usize;
            old.get(at..at + len).map(<[u8]>::to_vec)
        };
        assert_eq!(
            read_player(read, "Player-1-A"),
            want.players.first().cloned()
        );
        let mut none = old.clone();
        none[4] = 0;
        assert_eq!(
            format_of(&none),
            Some(0),
            "named, though no reader takes it"
        );
        let mut newer = old.clone();
        newer[4] = FORMAT + 1;
        assert_eq!(format_of(&newer), Some(FORMAT + 1), "a later build's");
        assert_eq!(
            FightSeries::decode(&newer),
            None,
            "which this one cannot read"
        );
        assert_eq!(format_of(b"WDSX\x02\0\0\0\0"), None, "no series head");
        assert_eq!(FightSeries::decode(&none), None);
    }

    /// Truncation, a wrong format, a flipped byte: `None`, never a panic.
    #[test]
    fn bad_bytes_are_refused_never_a_panic() {
        let bytes = sample().encode();
        for n in 0..bytes.len() {
            assert_eq!(FightSeries::decode(&bytes[..n]), None, "cut at {n}");
        }
        let mut other = bytes.clone();
        other[4] = FORMAT + 1;
        assert_eq!(FightSeries::decode(&other), None);
        for i in 0..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0xff;
            let _ = FightSeries::decode(&b);
        }
        // A count that lies about what follows is refused before allocating.
        let mut liar = Vec::from(&MAGIC[..]);
        liar.push(FORMAT);
        liar.extend_from_slice(&3u32.to_le_bytes());
        liar.extend_from_slice(&[0xff, 0xff, 0x7f]);
        assert_eq!(FightSeries::decode(&liar), None);
    }

    /// Overkill rides sparse, overheal dense: each row the smaller.
    #[test]
    fn extra_is_stored_the_cheaper_way() {
        let row = |extra: [u64; 6]| SeriesRow {
            key: "k".into(),
            spell_id: 1,
            school: 1,
            cells: (0..6)
                .map(|i| cell(i, 1_000, extra[i as usize], 1, 0))
                .collect(),
        };
        let size = |r: SeriesRow| {
            let mut out = Vec::new();
            put_row(&mut out, &r);
            out.len()
        };
        let none = size(row([0; 6]));
        let one = size(row([0, 0, 0, 0, 0, 900]));
        let all = size(row([900; 6]));
        assert!(
            one <= none + 4,
            "a lone overkill costs a few bytes: {none} → {one}"
        );
        assert!(all <= none + 1 + 6 * 2, "dense: {none} → {all}");
    }
}
