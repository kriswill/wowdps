//! The wowdps domain vocabulary: what a meter shows, said without reference to
//! how it is computed. Everything here is plain data — no I/O, no parser, no
//! dependencies — so frontends (and the wire protocol) can bind to these types
//! while the engine that produces them stays out of their build.
//!
//! `wowdps-core` re-exports everything here, so engine code and the fixture
//! contract keep their existing paths.

pub mod fmt;

/// A meter view: what the rows are counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    Damage,
    Healing,
    Interrupts,
    CrowdControl,
    Dispels,
    Deaths,
}

impl View {
    /// Number of views, for per-view storage.
    pub const COUNT: usize = 6;

    /// Dense 0-based index, stable across releases only as far as the wire
    /// protocol's `PROTO_VERSION` promises.
    pub fn index(self) -> usize {
        match self {
            View::Damage => 0,
            View::Healing => 1,
            View::Interrupts => 2,
            View::CrowdControl => 3,
            View::Dispels => 4,
            View::Deaths => 5,
        }
    }

    /// Count views report occurrences, not a rate.
    pub fn is_rate(self) -> bool {
        matches!(self, View::Damage | View::Healing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentKind {
    Encounter,
    Trash,
}

/// Player class, derived from COMBATANT_INFO's currentSpecID. Carries the
/// standard Blizzard class color so every UI agrees on the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Warrior,
    Paladin,
    Hunter,
    Rogue,
    Priest,
    DeathKnight,
    Shaman,
    Mage,
    Warlock,
    Monk,
    Druid,
    DemonHunter,
    Evoker,
}

impl Class {
    pub fn from_spec(spec_id: u32) -> Option<Self> {
        Some(match spec_id {
            71..=73 => Class::Warrior,
            65 | 66 | 70 => Class::Paladin,
            253..=255 => Class::Hunter,
            259..=261 => Class::Rogue,
            256..=258 => Class::Priest,
            250..=252 => Class::DeathKnight,
            262..=264 => Class::Shaman,
            62..=64 => Class::Mage,
            265..=267 => Class::Warlock,
            268..=270 => Class::Monk,
            102..=105 => Class::Druid,
            577 | 581 => Class::DemonHunter,
            1467 | 1468 | 1473 => Class::Evoker,
            _ => return None,
        })
    }

    /// Blizzard's standard class colors.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Class::Warrior => (0xC6, 0x9B, 0x6D),
            Class::Paladin => (0xF4, 0x8C, 0xBA),
            Class::Hunter => (0xAA, 0xD3, 0x72),
            Class::Rogue => (0xFF, 0xF4, 0x68),
            Class::Priest => (0xFF, 0xFF, 0xFF),
            Class::DeathKnight => (0xC4, 0x1E, 0x3A),
            Class::Shaman => (0x00, 0x70, 0xDD),
            Class::Mage => (0x3F, 0xC7, 0xEB),
            Class::Warlock => (0x87, 0x88, 0xEE),
            Class::Monk => (0x00, 0xFF, 0x98),
            Class::Druid => (0xFF, 0x7C, 0x0A),
            Class::DemonHunter => (0xA3, 0x30, 0xC9),
            Class::Evoker => (0x33, 0x93, 0x7F),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// Player GUID for meter rows; spell or target name for breakdown rows.
    pub key: String,
    pub label: String,
    /// Damage done, healing done, or an event count.
    pub amount: u64,
    /// Overheal for Healing, overkill for Damage, else 0.
    pub extra: u64,
    pub per_sec: f64,
    /// 0..100 of the view total.
    pub pct: f64,
    /// The owning player's class (meter rows and drilldown rows alike);
    /// `None` until a COMBATANT_INFO for that player has been seen.
    pub class: Option<Class>,
}

/// A key press translated into intent. Keeps the keymap testable on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SetView(View),
    OlderSegment,
    NewerSegment,
    Up,
    Down,
    Open,
    Back,
    SwapPane,
    Quit,
}

/// Which half of the drilldown has the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Spell,
    Target,
}

/// An open drilldown. Keyed by row key (player guid), never by index, so a
/// re-sort between frames can't silently switch which player you're inspecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drill {
    pub key: String,
    pub label: String,
    pub pane: Pane,
    pub spell_sel: usize,
    pub target_sel: usize,
}

/// Which screen the app is on. It starts on the list; an in-progress fight
/// detected at startup jumps straight to its meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    List,
    Meter,
}

/// One row of the segment list: an indexed historical segment or a segment of
/// the live meter, presented uniformly for the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    pub success: Option<bool>,
    pub duration_ms: i64,
    pub live: bool,
}

/// Stable identity of one segment for the daemon's lifetime: assigned at scan
/// or open, monotonic, and never reused — not even across log rotation, so a
/// stale id can only fail to resolve, never resolve to another file's fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SegmentId(pub u64);

/// Everything the meter header says about the segment being watched.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentInfo {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    /// R7 semantics, computed by the engine: the live clock never stretches a
    /// closed segment.
    pub duration_ms: i64,
    pub success: Option<bool>,
    /// Still accumulating right now.
    pub live: bool,
}
