//! The spell tips cache reader: what a spell's tooltip says (its name,
//! school, cast time, range, cooldown, cost, description, the encounter
//! journal's words) and what a cast lane asks of a spell (whether it is
//! passive or channeled, a class spell, the duration of the aura it
//! applies, and the spell whose effect it is), from the per-machine cache
//! `$XDG_DATA_HOME/wowdps/spell-tips.bin` beside `spell-icons.bin`,
//! written by `tools/gen-spell-tips.sh` (`tools/extract/src/spelltipgen.rs`,
//! which documents how each fact is read from the game's tables).
//!
//! Entirely optional, as the icons are: no file (or one of a future format
//! version) answers `None`, and a reader says the spell's name as the log
//! gives it and nothing more. The file is a few MiB and read whole once;
//! a record is decoded when it is asked for.
//!
//! File layout (all LE):
//!
//! ```text
//! "WDPT" | u32 version=1 | u64 stamp | u32 n
//! n × (u32 spell_id, u32 offset)          sorted by spell_id; offset into the body
//! body, a record each:
//!   u32 parent | u32 cast_ms | u32 cooldown_ms | i32 duration_ms | f32 range_yd
//!   u8 school | u8 flags | u8 charges | u8 0
//!   4 × (u32 len, UTF-8): name, cost, desc, journal
//! ```
//!
//! `flags`: 1 passive, 2 the parent is passive, 4 channeled, 8 a class
//! spell (R8's table), 16 on the global cooldown, 32 empowered.

use std::path::{Path, PathBuf};

use crate::drill::school_name;
use crate::lazy_tiles::le_u32;

/// The file's name beside the other per-machine caches.
pub const FILE: &str = "spell-tips.bin";

const VERSION: u32 = 1;
const HEAD: usize = 20;
/// A record's fixed part, before its four strings.
const FIXED: usize = 24;

/// The spell whose effect a spell is: the root of its trigger chain (or
/// the spell its description embeds), and whether that root is passive (a
/// proc's, a hero talent's, a rune's: no press made it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent {
    pub id: u32,
    pub passive: bool,
}

/// One spell's tip.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellTip {
    pub id: u32,
    pub name: String,
    /// The game's school bitmask (`drill::school_name` names it).
    pub school: u32,
    pub passive: bool,
    pub channel: bool,
    /// In a class's spellbook, specialization or talents (R8).
    pub class: bool,
    /// It starts the global cooldown (a press does; a proc does not).
    pub gcd: bool,
    /// Held to empower it (never `channel`: the log says when its hold
    /// began, not how long it was held).
    pub empower: bool,
    pub parent: Option<Parent>,
    pub cast_ms: u32,
    pub range_yd: f32,
    pub cooldown_ms: u32,
    /// Charges where it holds more than one.
    pub charges: u8,
    /// The aura it applies lasts this long: -1 until cancelled, 0 none.
    pub duration_ms: i32,
    pub cost: String,
    pub desc: String,
    /// The encounter journal's words, where its own description says
    /// little and the journal names it (or a spell it is an effect of).
    pub journal: String,
}

/// The tips, read whole.
pub struct SpellTips {
    /// (spell id, offset into `body`), sorted by spell id.
    index: Vec<(u32, u32)>,
    body: Vec<u8>,
}

fn cache_path() -> Option<PathBuf> {
    wowdps_proto::talents::data_path(FILE)
}

/// A length-prefixed string at `at` in `b`, and where it ends.
fn string_at(b: &[u8], at: usize) -> Option<(String, usize)> {
    let len = u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize;
    let s = b.get(at + 4..at.checked_add(4 + len)?)?;
    Some((String::from_utf8_lossy(s).into_owned(), at + 4 + len))
}

impl SpellTips {
    /// The per-machine cache, or `None` without one (or with one this build
    /// cannot read).
    pub fn open() -> Option<SpellTips> {
        SpellTips::open_at(&cache_path()?)
    }

    /// The reader behind a path, so tests can point it at absent, truncated
    /// or garbage files without the real per-machine cache.
    pub fn open_at(path: &Path) -> Option<SpellTips> {
        SpellTips::from_bytes(std::fs::read(path).ok()?)
    }

    /// The tips a file's bytes hold; `None` for anything but a whole index
    /// of this format.
    pub fn from_bytes(bytes: Vec<u8>) -> Option<SpellTips> {
        if bytes.get(..4)? != b"WDPT" || bytes.len() < HEAD {
            return None;
        }
        if le_u32(&bytes, 4) != VERSION {
            return None;
        }
        let n = le_u32(&bytes, 16) as usize;
        let body_at = HEAD.checked_add(n.checked_mul(8)?)?;
        if n > 4_000_000 || body_at > bytes.len() {
            return None;
        }
        let index: Vec<(u32, u32)> = bytes
            .get(HEAD..body_at)?
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&[a0, a1, a2, a3, b0, b1, b2, b3]| {
                (
                    u32::from_le_bytes([a0, a1, a2, a3]),
                    u32::from_le_bytes([b0, b1, b2, b3]),
                )
            })
            .collect();
        if index.windows(2).any(|w| matches!(w, [a, b] if a.0 >= b.0)) {
            return None;
        }
        Some(SpellTips {
            index,
            body: bytes.get(body_at..)?.to_vec(),
        })
    }

    /// How many spells it holds.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// A spell's tip; `None` for a spell the file lacks or a record it
    /// cannot read whole.
    pub fn get(&self, id: u32) -> Option<SpellTip> {
        let k = self.index.binary_search_by_key(&id, |&(s, _)| s).ok()?;
        let at = self.index.get(k)?.1 as usize;
        let b = &self.body;
        let fixed = b.get(at..at.checked_add(FIXED)?)?;
        let word = |i: usize| -> Option<[u8; 4]> { fixed.get(i..i + 4)?.try_into().ok() };
        let parent = u32::from_le_bytes(word(0)?);
        let flags = *fixed.get(21)?;
        let (name, next) = string_at(b, at + FIXED)?;
        let (cost, next) = string_at(b, next)?;
        let (desc, next) = string_at(b, next)?;
        let (journal, _) = string_at(b, next)?;
        Some(SpellTip {
            id,
            name,
            school: u32::from(*fixed.get(20)?),
            passive: flags & 1 != 0,
            channel: flags & 4 != 0,
            class: flags & 8 != 0,
            gcd: flags & 16 != 0,
            empower: flags & 32 != 0,
            parent: (parent != 0).then_some(Parent {
                id: parent,
                passive: flags & 2 != 0,
            }),
            cast_ms: u32::from_le_bytes(word(4)?),
            cooldown_ms: u32::from_le_bytes(word(8)?),
            duration_ms: i32::from_le_bytes(word(12)?),
            range_yd: f32::from_le_bytes(word(16)?),
            charges: *fixed.get(22)?,
            cost,
            desc,
            journal,
        })
    }
}

/// "12 sec", "1.5 sec", "1.5 min", "2 min", "1 hr": a time as the game's
/// tooltips word it.
pub fn time_words(ms: u32) -> String {
    let trim = |v: f64| {
        if (v - v.round()).abs() < 0.05 {
            format!("{}", v.round() as i64)
        } else {
            format!("{v:.1}")
        }
    };
    let s = f64::from(ms) / 1000.0;
    if s < 60.0 {
        format!("{} sec", trim(s))
    } else if s < 3600.0 {
        format!("{} min", trim(s / 60.0))
    } else {
        format!("{} hr", trim(s / 3600.0))
    }
}

impl SpellTip {
    /// The school's name ("Physical", "Shadowflame"); none for no school.
    pub fn school_name(&self) -> Option<String> {
        school_name(self.school)
    }

    /// How it is cast: "Passive", "Empowered", "Channeled", "1.5 sec cast"
    /// or "Instant".
    pub fn cast_words(&self) -> String {
        if self.passive {
            "Passive".into()
        } else if self.empower {
            "Empowered".into()
        } else if self.channel {
            "Channeled".into()
        } else if self.cast_ms > 0 {
            format!("{} cast", time_words(self.cast_ms))
        } else {
            "Instant".into()
        }
    }

    /// Its reach: "Melee range", "40 yd range", "Unlimited range"; none
    /// for a spell on its caster alone.
    pub fn range_words(&self) -> Option<String> {
        let r = self.range_yd;
        if r <= 0.0 {
            None
        } else if r <= 6.0 {
            Some("Melee range".into())
        } else if r >= 50_000.0 {
            Some("Unlimited range".into())
        } else if (r - r.round()).abs() < 0.05 {
            Some(format!("{} yd range", r.round() as i64))
        } else {
            Some(format!("{r:.1} yd range"))
        }
    }

    /// Its cooldown: "45 sec cooldown", or "2 charges, 30 sec recharge";
    /// none without one.
    pub fn cooldown_words(&self) -> Option<String> {
        (self.cooldown_ms > 0).then(|| {
            let t = time_words(self.cooldown_ms);
            if self.charges > 1 {
                format!("{} charges, {t} recharge", self.charges)
            } else {
                format!("{t} cooldown")
            }
        })
    }

    /// The tooltip's one line of facts: its cost, how it is cast, its reach
    /// and its cooldown, those it has, `·` between.
    pub fn facts_line(&self) -> String {
        let cost = (!self.cost.is_empty()).then(|| self.cost.clone());
        [
            cost,
            Some(self.cast_words()),
            self.range_words(),
            self.cooldown_words(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }

    /// How long the aura it applies lasts (ms), where it has a length.
    pub fn duration(&self) -> Option<u32> {
        u32::try_from(self.duration_ms).ok().filter(|d| *d > 0)
    }

    /// Whether a press makes it: a class spell, or one that starts the
    /// global cooldown, has a cooldown of its own or costs something.
    /// Thunder Blast, an Execute a talent swaps in, a trinket's use: each
    /// is pressed though a passive's chain reaches it; a proc is none of
    /// these.
    pub fn pressed(&self) -> bool {
        self.class || self.gcd || self.cooldown_ms > 0 || !self.cost.is_empty()
    }

    /// Whether a cast success of it is no press: a passive's own, or an
    /// effect of a passive no press makes (a hero talent's or a talent's
    /// proc, Crusading Strikes' swing). What the replay's cast lane leaves
    /// out of its instants.
    pub fn passive_success(&self) -> bool {
        self.passive || self.parent.is_some_and(|p| p.passive) && !self.pressed()
    }
}

#[cfg(test)]
mod tests;
