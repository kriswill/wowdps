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
    /// R10: the per-instance-visit aggregate — every counter accumulated
    /// while that visit was in progress, merged across its member segments.
    Overall,
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

/// What kind of item granted a spell (R12). Produced by the generated
/// `core::item_spells` table (spell id → kind, built from the client's
/// Item/ItemEffect tables), and consumed by the meter to label the vertical
/// markers on a comparison graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// Equipped trinket (inventory type 12) — on-use *and* proc effects.
    Trinket,
    Potion,
    /// Flask or elixir.
    Flask,
    Food,
    /// Any other consumable (healthstones, augment runes, bandages…).
    Consumable,
}

impl ItemKind {
    /// Dense 0-based code, as the generated table and the wire encode it.
    pub fn code(self) -> u8 {
        match self {
            ItemKind::Trinket => 0,
            ItemKind::Potion => 1,
            ItemKind::Flask => 2,
            ItemKind::Food => 3,
            ItemKind::Consumable => 4,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => ItemKind::Trinket,
            1 => ItemKind::Potion,
            2 => ItemKind::Flask,
            3 => ItemKind::Food,
            4 => ItemKind::Consumable,
            _ => return None,
        })
    }
}

/// What a timeline marker records (R12): the same item spell reads as a *use*
/// when the player cast it and a *proc* when it merely landed on them, which
/// is the distinction a trinket comparison is actually about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkKind {
    /// An on-use trinket the player actively cast.
    TrinketUse,
    /// A trinket effect that fired on its own (no cast preceded it).
    TrinketProc,
    /// A potion, flask, food or other consumable the player used.
    Consumable,
}

impl MarkKind {
    pub fn code(self) -> u8 {
        match self {
            MarkKind::TrinketUse => 0,
            MarkKind::TrinketProc => 1,
            MarkKind::Consumable => 2,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => MarkKind::TrinketUse,
            1 => MarkKind::TrinketProc,
            2 => MarkKind::Consumable,
            _ => return None,
        })
    }
}

/// One vertical bar on a player's timeline graph (R12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// Milliseconds since the segment's `start_ms` — already relative, so a
    /// renderer never needs the absolute clock to place the bar.
    pub at_ms: i64,
    pub kind: MarkKind,
    /// The spell name as the combat log wrote it ("Potion of Unwavering Focus").
    pub label: String,
}

/// One player's fight timeline (R12): damage bucketed on a fixed grid, plus
/// the markers drawn over it. `buckets[i]` covers
/// `[i * bucket_ms, (i+1) * bucket_ms)` from the segment start, so a renderer
/// can integrate it into a cumulative curve or smooth it into rolling DPS
/// without any further knowledge of the fight.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    pub bucket_ms: u32,
    pub buckets: Vec<u64>,
    pub marks: Vec<Mark>,
}

impl Timeline {
    /// Damage per second over a centred rolling window of `window_ms`.
    /// The window is clamped at the ends, so the curve starts and finishes at
    /// a real rate instead of ramping out of zero.
    pub fn rolling_dps(&self, window_ms: u32) -> Vec<f64> {
        if self.bucket_ms == 0 || self.buckets.is_empty() {
            return Vec::new();
        }
        let half = ((window_ms / self.bucket_ms).max(1) / 2) as usize;
        (0..self.buckets.len())
            .map(|i| {
                let lo = i.saturating_sub(half);
                let hi = (i + half + 1).min(self.buckets.len());
                let sum: u64 = self
                    .buckets
                    .get(lo..hi)
                    .map(|w| w.iter().sum())
                    .unwrap_or(0);
                let span = (hi - lo) as f64 * self.bucket_ms as f64 / 1000.0;
                if span > 0.0 { sum as f64 / span } else { 0.0 }
            })
            .collect()
    }

    /// Running total of damage done, one point per bucket.
    pub fn cumulative(&self) -> Vec<u64> {
        let mut acc = 0;
        self.buckets
            .iter()
            .map(|b| {
                acc += b;
                acc
            })
            .collect()
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
    /// By-spell breakdown rows: the spell id behind the label (first-seen id
    /// when ranks share a name), for client-side icon lookup. 0 everywhere a
    /// label has no spell — meter rows, targets, Melee, deaths.
    pub spell_id: u32,
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
    /// R12: add/remove the selected player from the comparison pair.
    PickCompare,
    /// R12: swap the comparison graph between rolling DPS and cumulative.
    ToggleGraph,
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
    /// R12: two picked players side by side. Only reachable once BOTH have
    /// been picked — a half-made comparison has nothing to show, so the
    /// meter stays up while the second pick is outstanding.
    Compare,
}

/// R12: how a comparison graph draws the fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphMode {
    /// Rolling DPS — burst windows line up with the trinket and pot markers,
    /// which is what a comparison is usually asking about.
    #[default]
    Dps,
    /// Cumulative damage — who pulled ahead, and when.
    Total,
}

impl GraphMode {
    pub fn toggled(self) -> Self {
        match self {
            GraphMode::Dps => GraphMode::Total,
            GraphMode::Total => GraphMode::Dps,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GraphMode::Dps => "dps",
            GraphMode::Total => "total",
        }
    }
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
    /// R10: the instance visit these counters belong to — a file-level
    /// ordinal shared by every segment recorded inside that visit, and by
    /// the visit's Overall row. `None` outside instanced content.
    pub instance: Option<u32>,
    /// R10, keyed Overall rows only: the dungeon's (par, +2, +3) timers —
    /// what `duration_ms` (the key clock) is judged against.
    pub pars_ms: Option<(i64, i64, i64)>,
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
    /// closed segment. For Overall (R10): the sum of member durations.
    pub duration_ms: i64,
    pub success: Option<bool>,
    /// Still accumulating right now.
    pub live: bool,
    /// R10: the instance visit ordinal (see [`ListRow::instance`]).
    pub instance: Option<u32>,
    /// R10, keyed Overall segments only: the dungeon's (par, +2, +3)
    /// timers (see [`ListRow::pars_ms`]).
    pub pars_ms: Option<(i64, i64, i64)>,
}
