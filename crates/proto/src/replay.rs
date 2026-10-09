//! v45 (R29): the history store's REPLAY tier — `replay/<fight id>.bin`, a
//! fight as the replay draws it (the model's [`Cut`]: its units and where
//! each stood, its events, what players placed, the world markers), written
//! for every boss pull and keystone run, wipes included. [`csv`] writes it
//! back out as the seven files a replay reads.
//!
//! Binary like the series tier, and for the same reason: it is sized by the
//! log's own lines. A 10-minute, 26-player raid pull posts its units some
//! 330 000 times (15 MB of `tracks.csv`); here a post costs a few bytes,
//! because each is coded against the same unit's last one and most of a
//! unit's fields hold still between two posts.
//!
//! ```text
//! file    := "WDRP" | u8 format | u32 LE index_len | index | section*
//! index   := varint n | (u8 tag | varint offset | varint len)*
//!            (offset from the first section; every tag below, once)
//! strs  1 := varint n | str*                      (every string, once)
//! head  2 := varint floor | varint map | varint zone | u8 success
//!            | opt(varint fight_ms) | zz start_utc_ms
//!            | varint year | u8 month | u8 day
//!            | opt(varint id | varint name | varint difficulty | varint size)
//!            | opt(varint name | varint level)               (the key)
//! units 3 := varint n | (u8 kind | varint name | varint guid | u8 class
//!            | varint spec | u8 you | varint npc | varint posts)*
//! posts 4 := every unit's posts in unit order, each against the unit's
//!            last (the first against zeros):
//!            u8 head | [zz dt] | [zz dx | zz dy] | [zz dfacing] | [zz dhp]
//!            | [u8 has | varint kind | varint max] | [zz dcurrent]
//!            head: bits 0–2 dt when below 7 (7: zz dt follows), 3 x/y
//!            moved, 4 facing, 5 health, 6 the pool's kind/max/presence,
//!            7 its current amount
//! events 5 := varint n | (zz dt | u8 kind | varint unit | varint spell
//!            | u8 has | [zz x | zz y] | [varint src] | [varint stacks]
//!            | [varint base] | [varint target])*
//!            (has: bits 0 at, 1 src, 2 stacks, 3 base, 4 target)
//! placed 6 := varint n | (zz dt | u8 kind | u8 has | [varint unit]
//!            | varint spell | [zz x | zz y] | [varint src] | [varint target])*
//!            (has: bits 0 unit, 1 at, 2 src, 3 target)
//! marks 7 := varint n | (zz dt | u8 kind | u8 marker | [zz x | zz y])*
//! spells  := an index into `spells` 8: varint n | (varint id | varint name)*
//! str     := varint len | utf-8
//! opt(x)  := u8 0 | u8 1 x;  zz := zigzag varint;  name := index into strs
//! ```
//!
//! Times count from the cut's start; an event's, a placed row's and a
//! marker's against the one before it. Decoding never panics and never
//! allocates more than the bytes in hand can fill: a lying count, an index
//! past its table, a unit number past the units, all are `None`.

use std::collections::HashMap;

use wowdps_model::replay::{
    Cut, EncounterHead, Event, EventKind, Head, KeyHead, Marker, MarkerKind, Placed, PlacedKind,
    Post, Power, Unit, UnitKind,
};
use wowdps_model::{Class, Spec};

pub mod csv;

/// The file's first four bytes.
pub const MAGIC: &[u8; 4] = b"WDRP";
/// The layout above, as this build writes it.
pub const FORMAT: u8 = 1;
/// The oldest layout a reader takes.
pub const OLDEST: u8 = 1;
/// The fixed head: magic, format, the index's length.
pub const HEAD_LEN: usize = 9;

const STRS: u8 = 1;
const HEAD: u8 = 2;
const UNITS: u8 = 3;
const POSTS: u8 = 4;
const EVENTS: u8 = 5;
const PLACED: u8 = 6;
const MARKS: u8 = 7;
const SPELLS: u8 = 8;
const SECTIONS: [u8; 8] = [STRS, HEAD, UNITS, POSTS, EVENTS, PLACED, MARKS, SPELLS];

/// The classes in the order the file numbers them (1-based; 0 is none).
const CLASSES: [Class; 13] = [
    Class::Warrior,
    Class::Paladin,
    Class::Hunter,
    Class::Rogue,
    Class::Priest,
    Class::DeathKnight,
    Class::Shaman,
    Class::Mage,
    Class::Warlock,
    Class::Monk,
    Class::Druid,
    Class::DemonHunter,
    Class::Evoker,
];

/// The layout a file's head names, whether or not this build reads it —
/// what tells the store a file older than [`FORMAT`] wants rewriting from
/// its log, and one NEWER is left alone. `None` for no replay head.
pub fn format_of(head: &[u8]) -> Option<u8> {
    (head.len() >= HEAD_LEN && head.get(..4)? == MAGIC)
        .then(|| head.get(4).copied())
        .flatten()
}

/// The cut as the tier's bytes.
pub fn encode(cut: &Cut) -> Vec<u8> {
    let mut w = Writer::default();
    let head = w.head(cut);
    let units = w.units(cut);
    let posts = w.posts(cut);
    let events = w.events(cut);
    let placed = w.placed(cut);
    let marks = w.marks(cut);
    let spells = w.spell_table();
    let strs = w.str_table();
    let mut index = Vec::new();
    let mut body = Vec::new();
    put_varint(&mut index, SECTIONS.len() as u64);
    for (tag, bytes) in [
        (STRS, strs),
        (HEAD, head),
        (UNITS, units),
        (POSTS, posts),
        (EVENTS, events),
        (PLACED, placed),
        (MARKS, marks),
        (SPELLS, spells),
    ] {
        index.push(tag);
        put_varint(&mut index, body.len() as u64);
        put_varint(&mut index, bytes.len() as u64);
        body.extend_from_slice(&bytes);
    }
    let mut out = Vec::with_capacity(HEAD_LEN + index.len() + body.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT);
    out.extend_from_slice(&(index.len() as u32).to_le_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&body);
    out
}

/// The tier's bytes as a cut. `None` for anything this format does not
/// describe.
pub fn decode(bytes: &[u8]) -> Option<Cut> {
    let format = *bytes.get(4)?;
    if bytes.get(..4)? != MAGIC || !(OLDEST..=FORMAT).contains(&format) {
        return None;
    }
    let len: [u8; 4] = bytes.get(5..HEAD_LEN)?.try_into().ok()?;
    let index_len = usize::try_from(u32::from_le_bytes(len)).ok()?;
    let mut c = Cur::new(bytes.get(HEAD_LEN..HEAD_LEN.checked_add(index_len)?)?);
    let base = HEAD_LEN + index_len;
    let n = c.count(3)?;
    let mut sections: HashMap<u8, &[u8]> = HashMap::new();
    for _ in 0..n {
        let tag = c.u8()?;
        let from = base.checked_add(c.usize()?)?;
        let to = from.checked_add(c.usize()?)?;
        sections.insert(tag, bytes.get(from..to)?);
    }
    if !c.done() {
        return None;
    }
    let section = |tag: u8| sections.get(&tag).map(|b| Cur::new(b));
    let strs = read_strs(&mut section(STRS)?)?;
    let spells = read_spells(&mut section(SPELLS)?, &strs)?;
    let (head, floor) = read_head(&mut section(HEAD)?, &strs)?;
    let (mut units, counts) = read_units(&mut section(UNITS)?, &strs)?;
    read_posts(&mut section(POSTS)?, &mut units, &counts)?;
    let n_units = units.len();
    let events = read_events(&mut section(EVENTS)?, &spells, n_units)?;
    let placed = read_placed(&mut section(PLACED)?, &spells, n_units)?;
    let markers = read_marks(&mut section(MARKS)?)?;
    Some(Cut {
        head,
        floor,
        units,
        events,
        placed,
        markers,
    })
}

// ---- writing ----------------------------------------------------------------------

#[derive(Default)]
struct Writer {
    strs: Vec<String>,
    str_ix: HashMap<String, u64>,
    spells: Vec<(u32, u64)>,
    spell_ix: HashMap<(u32, u64), u64>,
}

impl Writer {
    fn s(&mut self, s: &str) -> u64 {
        if let Some(&i) = self.str_ix.get(s) {
            return i;
        }
        let i = self.strs.len() as u64;
        self.strs.push(s.to_string());
        self.str_ix.insert(s.to_string(), i);
        i
    }

    fn spell(&mut self, id: u32, name: &str) -> u64 {
        let key = (id, self.s(name));
        if let Some(&i) = self.spell_ix.get(&key) {
            return i;
        }
        let i = self.spells.len() as u64;
        self.spells.push(key);
        self.spell_ix.insert(key, i);
        i
    }

    fn str_table(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, self.strs.len() as u64);
        for s in &self.strs {
            put_varint(&mut out, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        out
    }

    fn spell_table(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, self.spells.len() as u64);
        for &(id, name) in &self.spells {
            put_varint(&mut out, u64::from(id));
            put_varint(&mut out, name);
        }
        out
    }

    fn head(&mut self, cut: &Cut) -> Vec<u8> {
        let h = &cut.head;
        let mut out = Vec::new();
        put_varint(&mut out, u64::from(cut.floor));
        put_varint(&mut out, u64::from(h.map));
        let zone = self.s(&h.zone);
        put_varint(&mut out, zone);
        out.push(match h.success {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
        put_opt(&mut out, h.fight_ms);
        put_zz(&mut out, h.start_utc_ms);
        put_varint(&mut out, u64::from(h.date.0));
        out.push(h.date.1);
        out.push(h.date.2);
        match &h.encounter {
            Some(e) => {
                out.push(1);
                put_varint(&mut out, u64::from(e.id));
                let name = self.s(&e.name);
                put_varint(&mut out, name);
                put_varint(&mut out, u64::from(e.difficulty));
                put_varint(&mut out, u64::from(e.size));
            }
            None => out.push(0),
        }
        match &h.key {
            Some(k) => {
                out.push(1);
                let name = self.s(&k.name);
                put_varint(&mut out, name);
                put_varint(&mut out, u64::from(k.level));
            }
            None => out.push(0),
        }
        out
    }

    fn units(&mut self, cut: &Cut) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, cut.units.len() as u64);
        for u in &cut.units {
            out.push(match u.kind {
                UnitKind::Player => 0,
                UnitKind::Boss => 1,
                UnitKind::Add => 2,
                UnitKind::Friendly => 3,
            });
            let name = self.s(&u.name);
            put_varint(&mut out, name);
            let guid = self.s(&u.guid);
            put_varint(&mut out, guid);
            out.push(
                u.class
                    .and_then(|c| CLASSES.iter().position(|x| *x == c))
                    .map_or(0, |i| i as u8 + 1),
            );
            put_varint(&mut out, u.spec.map_or(0, |s| u64::from(s.id())));
            out.push(u8::from(u.you));
            put_varint(&mut out, u64::from(u.npc));
            put_varint(&mut out, u.posts.len() as u64);
        }
        out
    }

    fn posts(&mut self, cut: &Cut) -> Vec<u8> {
        let mut out = Vec::new();
        for u in &cut.units {
            let mut last = Post {
                t_ms: 0,
                x: 0,
                y: 0,
                facing: 0,
                hp: 0,
                power: None,
            };
            for p in &u.posts {
                put_post(&mut out, &last, p);
                last = *p;
            }
        }
        out
    }

    fn events(&mut self, cut: &Cut) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, cut.events.len() as u64);
        let mut t = 0i64;
        for e in &cut.events {
            put_zz(&mut out, i64::from(e.t_ms) - t);
            t = i64::from(e.t_ms);
            out.push(event_code(e.kind));
            put_varint(&mut out, u64::from(e.unit));
            let spell = self.spell(e.spell_id, &e.spell);
            put_varint(&mut out, spell);
            let has = u8::from(e.at.is_some())
                | u8::from(e.src.is_some()) << 1
                | u8::from(e.stacks.is_some()) << 2
                | u8::from(e.base.is_some()) << 3
                | u8::from(e.target.is_some()) << 4;
            out.push(has);
            if let Some((x, y)) = e.at {
                put_zz(&mut out, i64::from(x));
                put_zz(&mut out, i64::from(y));
            }
            if let Some(src) = e.src {
                put_varint(&mut out, u64::from(src));
            }
            if let Some(stacks) = e.stacks {
                put_varint(&mut out, u64::from(stacks));
            }
            if let Some(base) = e.base {
                put_varint(&mut out, base);
            }
            if let Some(target) = e.target {
                put_varint(&mut out, u64::from(target));
            }
        }
        out
    }

    fn placed(&mut self, cut: &Cut) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, cut.placed.len() as u64);
        let mut t = 0i64;
        for p in &cut.placed {
            put_zz(&mut out, i64::from(p.t_ms) - t);
            t = i64::from(p.t_ms);
            out.push(placed_code(p.kind));
            let has = u8::from(p.unit.is_some())
                | u8::from(p.at.is_some()) << 1
                | u8::from(p.src.is_some()) << 2
                | u8::from(p.target.is_some()) << 3;
            out.push(has);
            if let Some(unit) = p.unit {
                put_varint(&mut out, u64::from(unit));
            }
            let spell = self.spell(p.spell_id, &p.spell);
            put_varint(&mut out, spell);
            if let Some((x, y)) = p.at {
                put_zz(&mut out, i64::from(x));
                put_zz(&mut out, i64::from(y));
            }
            if let Some(src) = p.src {
                put_varint(&mut out, u64::from(src));
            }
            if let Some(target) = p.target {
                put_varint(&mut out, u64::from(target));
            }
        }
        out
    }

    fn marks(&mut self, cut: &Cut) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, cut.markers.len() as u64);
        let mut t = 0i64;
        for m in &cut.markers {
            put_zz(&mut out, i64::from(m.t_ms) - t);
            t = i64::from(m.t_ms);
            out.push(match (m.kind, m.at.is_some()) {
                (MarkerKind::Placed, true) => 1,
                (MarkerKind::Placed, false) => 2,
                (MarkerKind::Removed, true) => 3,
                (MarkerKind::Removed, false) => 0,
            });
            out.push(m.marker);
            if let Some((x, y)) = m.at {
                put_zz(&mut out, i64::from(x));
                put_zz(&mut out, i64::from(y));
            }
        }
        out
    }
}

/// One post against the unit's last.
fn put_post(out: &mut Vec<u8>, last: &Post, p: &Post) {
    let dt = i64::from(p.t_ms) - i64::from(last.t_ms);
    let moved = (p.x, p.y) != (last.x, last.y);
    let turned = p.facing != last.facing;
    let hurt = p.hp != last.hp;
    let pool = match (p.power, last.power) {
        (Some(a), Some(b)) => a.kind != b.kind || a.max != b.max,
        (a, b) => a.is_some() != b.is_some(),
    };
    let last_current = last.power.map_or(0, |w| w.current);
    let spent = p.power.is_some_and(|w| w.current != last_current);
    let small = (0..7).contains(&dt);
    let head = (if small { dt as u8 } else { 7 })
        | u8::from(moved) << 3
        | u8::from(turned) << 4
        | u8::from(hurt) << 5
        | u8::from(pool) << 6
        | u8::from(spent) << 7;
    out.push(head);
    if !small {
        put_zz(out, dt);
    }
    if moved {
        put_zz(out, i64::from(p.x) - i64::from(last.x));
        put_zz(out, i64::from(p.y) - i64::from(last.y));
    }
    if turned {
        put_zz(out, i64::from(p.facing) - i64::from(last.facing));
    }
    if hurt {
        put_zz(out, i64::from(p.hp) - i64::from(last.hp));
    }
    if pool {
        match p.power {
            Some(w) => {
                out.push(1);
                put_varint(out, u64::from(w.kind));
                put_varint(out, w.max);
            }
            None => out.push(0),
        }
    }
    if spent && let Some(w) = p.power {
        put_zz(out, w.current as i64 - last_current as i64);
    }
}

fn event_code(k: EventKind) -> u8 {
    EventKind::ALL
        .iter()
        .position(|x| *x == k)
        .map_or(0, |i| i as u8)
}

fn placed_code(k: PlacedKind) -> u8 {
    PlacedKind::ALL
        .iter()
        .position(|x| *x == k)
        .map_or(0, |i| i as u8)
}

// ---- reading ----------------------------------------------------------------------

fn read_strs(c: &mut Cur) -> Option<Vec<String>> {
    let n = c.count(1)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(c.str()?);
    }
    c.done().then_some(out)
}

fn read_spells(c: &mut Cur, strs: &[String]) -> Option<Vec<(u32, String)>> {
    let n = c.count(2)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = c.u32()?;
        out.push((id, c.name(strs)?));
    }
    c.done().then_some(out)
}

fn read_head(c: &mut Cur, strs: &[String]) -> Option<(Head, u32)> {
    let floor = c.u32()?;
    let map = c.u32()?;
    let zone = c.name(strs)?;
    let success = match c.u8()? {
        0 => None,
        1 => Some(false),
        2 => Some(true),
        _ => return None,
    };
    let fight_ms = match c.u8()? {
        0 => None,
        1 => Some(c.varint()?),
        _ => return None,
    };
    let start_utc_ms = c.zz()?;
    let year = u16::try_from(c.varint()?).ok()?;
    let date = (year, c.u8()?, c.u8()?);
    let encounter = match c.u8()? {
        0 => None,
        1 => Some(EncounterHead {
            id: c.u32()?,
            name: c.name(strs)?,
            difficulty: c.u32()?,
            size: c.u32()?,
        }),
        _ => return None,
    };
    let key = match c.u8()? {
        0 => None,
        1 => Some(KeyHead {
            name: c.name(strs)?,
            level: c.u32()?,
        }),
        _ => return None,
    };
    c.done().then_some((
        Head {
            encounter,
            key,
            map,
            zone,
            success,
            fight_ms,
            start_utc_ms,
            date,
        },
        floor,
    ))
}

fn read_units(c: &mut Cur, strs: &[String]) -> Option<(Vec<Unit>, Vec<usize>)> {
    // Seven fields of a byte at least.
    let n = c.count(7)?;
    let mut units = Vec::with_capacity(n);
    let mut counts = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = match c.u8()? {
            0 => UnitKind::Player,
            1 => UnitKind::Boss,
            2 => UnitKind::Add,
            3 => UnitKind::Friendly,
            _ => return None,
        };
        let name = c.name(strs)?;
        let guid = c.name(strs)?;
        let class = match c.u8()? {
            0 => None,
            k => Some(*CLASSES.get(usize::from(k) - 1)?),
        };
        // A spec a later build knows and this one does not reads as none.
        let spec = Spec::from_id(c.u32()?);
        let you = match c.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let npc = c.u32()?;
        counts.push(c.usize()?);
        units.push(Unit {
            kind,
            name,
            guid,
            class,
            spec,
            you,
            npc,
            posts: Vec::new(),
        });
    }
    c.done().then_some((units, counts))
}

fn read_posts(c: &mut Cur, units: &mut [Unit], counts: &[usize]) -> Option<()> {
    for (u, &n) in units.iter_mut().zip(counts) {
        // A post is a byte at least: more than the bytes left is a lie.
        if n > c.left() {
            return None;
        }
        u.posts.reserve_exact(n);
        let mut last = Post {
            t_ms: 0,
            x: 0,
            y: 0,
            facing: 0,
            hp: 0,
            power: None,
        };
        for _ in 0..n {
            let p = read_post(c, &last)?;
            u.posts.push(p);
            last = p;
        }
    }
    c.done().then_some(())
}

fn read_post(c: &mut Cur, last: &Post) -> Option<Post> {
    let head = c.u8()?;
    let dt = match head & 7 {
        7 => c.zz()?,
        small => i64::from(small),
    };
    let i32_of = |base: i32, d: i64| i32::try_from(i64::from(base).checked_add(d)?).ok();
    let t_ms = u32::try_from(i64::from(last.t_ms).checked_add(dt)?).ok()?;
    let (x, y) = if head & 1 << 3 != 0 {
        (i32_of(last.x, c.zz()?)?, i32_of(last.y, c.zz()?)?)
    } else {
        (last.x, last.y)
    };
    let facing = if head & 1 << 4 != 0 {
        i32_of(last.facing, c.zz()?)?
    } else {
        last.facing
    };
    let hp = if head & 1 << 5 != 0 {
        u16::try_from(i64::from(last.hp).checked_add(c.zz()?)?).ok()?
    } else {
        last.hp
    };
    let mut power = last.power;
    if head & 1 << 6 != 0 {
        power = match c.u8()? {
            0 => None,
            1 => {
                let kind = c.u32()?;
                let max = c.varint()?;
                let current = last.power.map_or(0, |w| w.current);
                Some(Power { kind, current, max })
            }
            _ => return None,
        };
    }
    if head & 1 << 7 != 0 {
        let w = power.as_mut()?;
        let last_current = last.power.map_or(0, |l| l.current);
        w.current = u64::try_from(i64::try_from(last_current).ok()?.checked_add(c.zz()?)?).ok()?;
    } else if let Some(w) = power.as_mut() {
        // Unchanged since the last post (none before it: 0).
        w.current = last.power.map_or(0, |l| l.current);
    }
    Some(Post {
        t_ms,
        x,
        y,
        facing,
        hp,
        power,
    })
}

fn read_events(c: &mut Cur, spells: &[(u32, String)], units: usize) -> Option<Vec<Event>> {
    // A time, a kind, a unit, a spell and a flags byte: five bytes.
    let n = c.count(5)?;
    let mut out = Vec::with_capacity(n);
    let mut t = 0i64;
    for _ in 0..n {
        t = t.checked_add(c.zz()?)?;
        let kind = *EventKind::ALL.get(usize::from(c.u8()?))?;
        let unit = c.unit(units)?;
        let (spell_id, spell) = spells.get(c.usize()?)?.clone();
        let has = c.u8()?;
        if has >> 5 != 0 {
            return None;
        }
        let at = if has & 1 != 0 {
            Some((c.i32()?, c.i32()?))
        } else {
            None
        };
        let src = if has & 2 != 0 {
            Some(c.unit(units)?)
        } else {
            None
        };
        let stacks = if has & 4 != 0 {
            Some(u16::try_from(c.varint()?).ok()?)
        } else {
            None
        };
        let base = if has & 8 != 0 {
            Some(c.varint()?)
        } else {
            None
        };
        let target = if has & 16 != 0 {
            Some(c.unit(units)?)
        } else {
            None
        };
        out.push(Event {
            t_ms: u32::try_from(t).ok()?,
            kind,
            unit,
            spell_id,
            spell,
            at,
            src,
            stacks,
            base,
            target,
        });
    }
    c.done().then_some(out)
}

fn read_placed(c: &mut Cur, spells: &[(u32, String)], units: usize) -> Option<Vec<Placed>> {
    // A time, a kind, a flags byte and a spell: four bytes.
    let n = c.count(4)?;
    let mut out = Vec::with_capacity(n);
    let mut t = 0i64;
    for _ in 0..n {
        t = t.checked_add(c.zz()?)?;
        let kind = *PlacedKind::ALL.get(usize::from(c.u8()?))?;
        let has = c.u8()?;
        if has >> 4 != 0 {
            return None;
        }
        let unit = if has & 1 != 0 {
            Some(c.unit(units)?)
        } else {
            None
        };
        let (spell_id, spell) = spells.get(c.usize()?)?.clone();
        let at = if has & 2 != 0 {
            Some((c.i32()?, c.i32()?))
        } else {
            None
        };
        let src = if has & 4 != 0 {
            Some(c.unit(units)?)
        } else {
            None
        };
        let target = if has & 8 != 0 {
            Some(c.unit(units)?)
        } else {
            None
        };
        out.push(Placed {
            t_ms: u32::try_from(t).ok()?,
            kind,
            unit,
            spell_id,
            spell,
            at,
            src,
            target,
        });
    }
    c.done().then_some(out)
}

fn read_marks(c: &mut Cur) -> Option<Vec<Marker>> {
    // A time, a kind and a marker: three bytes.
    let n = c.count(3)?;
    let mut out = Vec::with_capacity(n);
    let mut t = 0i64;
    for _ in 0..n {
        t = t.checked_add(c.zz()?)?;
        let (kind, at) = match c.u8()? {
            0 => (MarkerKind::Removed, false),
            1 => (MarkerKind::Placed, true),
            2 => (MarkerKind::Placed, false),
            3 => (MarkerKind::Removed, true),
            _ => return None,
        };
        let marker = c.u8()?;
        let at = if at { Some((c.i32()?, c.i32()?)) } else { None };
        out.push(Marker {
            t_ms: u32::try_from(t).ok()?,
            kind,
            marker,
            at,
        });
    }
    c.done().then_some(out)
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

fn put_zz(out: &mut Vec<u8>, v: i64) {
    put_varint(out, ((v << 1) ^ (v >> 63)) as u64);
}

fn put_opt(out: &mut Vec<u8>, v: Option<u64>) {
    match v {
        Some(v) => {
            out.push(1);
            put_varint(out, v);
        }
        None => out.push(0),
    }
}

/// A bounds-checked cursor: every read is `None` past the end.
struct Cur<'a> {
    b: &'a [u8],
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b }
    }

    fn left(&self) -> usize {
        self.b.len()
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

    fn zz(&mut self) -> Option<i64> {
        let v = self.varint()?;
        Some((v >> 1) as i64 ^ -((v & 1) as i64))
    }

    fn u32(&mut self) -> Option<u32> {
        u32::try_from(self.varint()?).ok()
    }

    fn i32(&mut self) -> Option<i32> {
        i32::try_from(self.zz()?).ok()
    }

    fn usize(&mut self) -> Option<usize> {
        usize::try_from(self.varint()?).ok()
    }

    /// A unit number, which must name one of `n` units.
    fn unit(&mut self, n: usize) -> Option<u32> {
        let u = self.u32()?;
        ((u as usize) < n).then_some(u)
    }

    /// A string by its index into the table.
    fn name(&mut self, strs: &[String]) -> Option<String> {
        strs.get(self.usize()?).cloned()
    }

    /// A count of items each at least `min` bytes long: more than the
    /// bytes left could hold is a lie, refused before any allocation.
    fn count(&mut self, min: usize) -> Option<usize> {
        let n = self.usize()?;
        (n.saturating_mul(min) <= self.b.len()).then_some(n)
    }

    fn str(&mut self) -> Option<String> {
        let len = self.usize()?;
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
mod tests;
