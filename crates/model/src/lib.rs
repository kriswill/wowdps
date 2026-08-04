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

/// Player class, from COMBATANT_INFO's currentSpecID when available, else
/// inferred from class-identifying spell casts (R8). Carries the standard
/// Blizzard class color so every UI agrees on the palette.
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
        Spec::from_id(spec_id).map(Spec::class)
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

/// Player specialization, from COMBATANT_INFO's currentSpecID when available,
/// else inferred from spec-unique spell casts (R8). Variants carry the class
/// name only where the in-game spec name is shared between classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Spec {
    Arms,
    Fury,
    ProtectionWarrior,
    HolyPaladin,
    ProtectionPaladin,
    Retribution,
    BeastMastery,
    Marksmanship,
    Survival,
    Assassination,
    Outlaw,
    Subtlety,
    Discipline,
    HolyPriest,
    Shadow,
    Blood,
    FrostDeathKnight,
    Unholy,
    Elemental,
    Enhancement,
    RestorationShaman,
    Arcane,
    Fire,
    FrostMage,
    Affliction,
    Demonology,
    Destruction,
    Brewmaster,
    Mistweaver,
    Windwalker,
    Balance,
    Feral,
    Guardian,
    RestorationDruid,
    Havoc,
    Vengeance,
    Devastation,
    Preservation,
    Augmentation,
}

impl Spec {
    /// Blizzard chrSpecialization id -> spec. The inverse of `id`.
    pub fn from_id(spec_id: u32) -> Option<Self> {
        Some(match spec_id {
            71 => Spec::Arms,
            72 => Spec::Fury,
            73 => Spec::ProtectionWarrior,
            65 => Spec::HolyPaladin,
            66 => Spec::ProtectionPaladin,
            70 => Spec::Retribution,
            253 => Spec::BeastMastery,
            254 => Spec::Marksmanship,
            255 => Spec::Survival,
            259 => Spec::Assassination,
            260 => Spec::Outlaw,
            261 => Spec::Subtlety,
            256 => Spec::Discipline,
            257 => Spec::HolyPriest,
            258 => Spec::Shadow,
            250 => Spec::Blood,
            251 => Spec::FrostDeathKnight,
            252 => Spec::Unholy,
            262 => Spec::Elemental,
            263 => Spec::Enhancement,
            264 => Spec::RestorationShaman,
            62 => Spec::Arcane,
            63 => Spec::Fire,
            64 => Spec::FrostMage,
            265 => Spec::Affliction,
            266 => Spec::Demonology,
            267 => Spec::Destruction,
            268 => Spec::Brewmaster,
            270 => Spec::Mistweaver,
            269 => Spec::Windwalker,
            102 => Spec::Balance,
            103 => Spec::Feral,
            104 => Spec::Guardian,
            105 => Spec::RestorationDruid,
            577 => Spec::Havoc,
            581 => Spec::Vengeance,
            1467 => Spec::Devastation,
            1468 => Spec::Preservation,
            1473 => Spec::Augmentation,
            _ => return None,
        })
    }

    /// Blizzard chrSpecialization id. The inverse of `from_id`.
    pub fn id(self) -> u32 {
        match self {
            Spec::Arms => 71,
            Spec::Fury => 72,
            Spec::ProtectionWarrior => 73,
            Spec::HolyPaladin => 65,
            Spec::ProtectionPaladin => 66,
            Spec::Retribution => 70,
            Spec::BeastMastery => 253,
            Spec::Marksmanship => 254,
            Spec::Survival => 255,
            Spec::Assassination => 259,
            Spec::Outlaw => 260,
            Spec::Subtlety => 261,
            Spec::Discipline => 256,
            Spec::HolyPriest => 257,
            Spec::Shadow => 258,
            Spec::Blood => 250,
            Spec::FrostDeathKnight => 251,
            Spec::Unholy => 252,
            Spec::Elemental => 262,
            Spec::Enhancement => 263,
            Spec::RestorationShaman => 264,
            Spec::Arcane => 62,
            Spec::Fire => 63,
            Spec::FrostMage => 64,
            Spec::Affliction => 265,
            Spec::Demonology => 266,
            Spec::Destruction => 267,
            Spec::Brewmaster => 268,
            Spec::Mistweaver => 270,
            Spec::Windwalker => 269,
            Spec::Balance => 102,
            Spec::Feral => 103,
            Spec::Guardian => 104,
            Spec::RestorationDruid => 105,
            Spec::Havoc => 577,
            Spec::Vengeance => 581,
            Spec::Devastation => 1467,
            Spec::Preservation => 1468,
            Spec::Augmentation => 1473,
        }
    }

    pub fn class(self) -> Class {
        match self {
            Spec::Arms | Spec::Fury | Spec::ProtectionWarrior => Class::Warrior,
            Spec::HolyPaladin | Spec::ProtectionPaladin | Spec::Retribution => Class::Paladin,
            Spec::BeastMastery | Spec::Marksmanship | Spec::Survival => Class::Hunter,
            Spec::Assassination | Spec::Outlaw | Spec::Subtlety => Class::Rogue,
            Spec::Discipline | Spec::HolyPriest | Spec::Shadow => Class::Priest,
            Spec::Blood | Spec::FrostDeathKnight | Spec::Unholy => Class::DeathKnight,
            Spec::Elemental | Spec::Enhancement | Spec::RestorationShaman => Class::Shaman,
            Spec::Arcane | Spec::Fire | Spec::FrostMage => Class::Mage,
            Spec::Affliction | Spec::Demonology | Spec::Destruction => Class::Warlock,
            Spec::Brewmaster | Spec::Mistweaver | Spec::Windwalker => Class::Monk,
            Spec::Balance | Spec::Feral | Spec::Guardian | Spec::RestorationDruid => Class::Druid,
            Spec::Havoc | Spec::Vengeance => Class::DemonHunter,
            Spec::Devastation | Spec::Preservation | Spec::Augmentation => Class::Evoker,
        }
    }

    /// The in-game spec name, unqualified ("Holy", not "Holy Paladin").
    pub fn name(self) -> &'static str {
        match self {
            Spec::Arms => "Arms",
            Spec::Fury => "Fury",
            Spec::ProtectionWarrior | Spec::ProtectionPaladin => "Protection",
            Spec::HolyPaladin | Spec::HolyPriest => "Holy",
            Spec::Retribution => "Retribution",
            Spec::BeastMastery => "Beast Mastery",
            Spec::Marksmanship => "Marksmanship",
            Spec::Survival => "Survival",
            Spec::Assassination => "Assassination",
            Spec::Outlaw => "Outlaw",
            Spec::Subtlety => "Subtlety",
            Spec::Discipline => "Discipline",
            Spec::Shadow => "Shadow",
            Spec::Blood => "Blood",
            Spec::FrostDeathKnight | Spec::FrostMage => "Frost",
            Spec::Unholy => "Unholy",
            Spec::Elemental => "Elemental",
            Spec::Enhancement => "Enhancement",
            Spec::RestorationShaman | Spec::RestorationDruid => "Restoration",
            Spec::Arcane => "Arcane",
            Spec::Fire => "Fire",
            Spec::Affliction => "Affliction",
            Spec::Demonology => "Demonology",
            Spec::Destruction => "Destruction",
            Spec::Brewmaster => "Brewmaster",
            Spec::Mistweaver => "Mistweaver",
            Spec::Windwalker => "Windwalker",
            Spec::Balance => "Balance",
            Spec::Feral => "Feral",
            Spec::Guardian => "Guardian",
            Spec::Havoc => "Havoc",
            Spec::Vengeance => "Vengeance",
            Spec::Devastation => "Devastation",
            Spec::Preservation => "Preservation",
            Spec::Augmentation => "Augmentation",
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
    /// Contributing events: hits/ticks for Damage, heal events for Healing,
    /// the recorded count for count views. Absorb credits count too (their
    /// crit flag is unknowable, so they can never crit).
    pub count: u64,
    /// How many of `count` were critical.
    pub crits: u64,
    pub per_sec: f64,
    /// 0..100 of the view total.
    pub pct: f64,
    /// The owning player's class (meter rows and drilldown rows alike);
    /// from COMBATANT_INFO when one has been seen, else inferred within the
    /// segment from class-identifying spell casts (R8), else `None`.
    pub class: Option<Class>,
    /// The owning player's specialization; from COMBATANT_INFO's specID, else
    /// inferred within the segment from spec-unique spell casts (R8), else
    /// `None` (class-wide casts identify a class but not a spec).
    pub spec: Option<Spec>,
    /// Death-recap rows only (R9): the victim's (current, max) health right
    /// after this event, from the advanced block. `None` everywhere else, and
    /// on recap entries whose line carried no health report.
    pub hp: Option<(u64, u64)>,
    /// Death-recap rows only (R9): true when the entry restored health
    /// (a heal or a consumed absorb) rather than removed it.
    pub gain: bool,
}

impl Row {
    /// Crit rate over the contributing events, 0..100. 0.0 when nothing hit.
    pub fn crit_pct(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.crits as f64 / self.count as f64 * 100.0
        }
    }
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
