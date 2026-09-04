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
    /// R17: damage taken by friendly players (pets folded), `extra` = absorbed.
    Taken,
}

impl View {
    /// Number of views, for per-view storage.
    pub const COUNT: usize = 7;

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
            View::Taken => 6,
        }
    }

    /// Count views report occurrences, not a rate.
    pub fn is_rate(self) -> bool {
        matches!(self, View::Damage | View::Healing | View::Taken)
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

/// Encounter identity from ENCOUNTER_START: the encounter id, the
/// difficulty id and the group size. Heroic and Mythic share a name, so
/// this — not the name — is what a fight's history is keyed on. `None`
/// on Trash, Overall and arena segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Encounter {
    pub id: u32,
    pub difficulty: u32,
    pub group_size: u32,
}

/// Difficulty.db2's name for the ids a combat log carries (ENCOUNTER_START,
/// ZONE_CHANGE). Raids and dungeons reuse names under different ids, so a
/// consumer must keep the id; `None` for an id this table does not know.
pub fn difficulty_name(id: u32) -> Option<&'static str> {
    Some(match id {
        1 => "Normal",
        2 => "Heroic",
        3 => "10 Player",
        4 => "25 Player",
        5 => "10 Player (Heroic)",
        6 => "25 Player (Heroic)",
        7 => "Looking For Raid",
        8 => "Mythic Keystone",
        9 => "40 Player",
        14 => "Normal",
        15 => "Heroic",
        16 => "Mythic",
        17 => "Looking For Raid",
        23 => "Mythic",
        24 => "Timewalking",
        33 => "Timewalking",
        150 => "Normal",
        151 => "Looking For Raid",
        205 => "Follower",
        208 => "Delve",
        220 => "Story",
        _ => return None,
    })
}

/// The inverse for the names a person types: raid difficulties by their
/// plain names, keystone / delve / timewalking by theirs. Case-insensitive;
/// digits parse as an id.
pub fn difficulty_from_str(s: &str) -> Option<u32> {
    let t = s.trim().to_ascii_lowercase();
    if let Ok(n) = t.parse::<u32>() {
        return Some(n);
    }
    Some(match t.as_str() {
        "normal" => 14,
        "heroic" => 15,
        "mythic" => 16,
        "lfr" | "looking for raid" | "raid finder" => 17,
        "mythic keystone" | "keystone" | "mythic+" | "m+" => 8,
        "mythic dungeon" | "mythic 0" | "m0" => 23,
        "heroic dungeon" => 2,
        "normal dungeon" => 1,
        "timewalking" => 24,
        "follower" => 205,
        "delve" => 208,
        "story" => 220,
        _ => return None,
    })
}

/// A spec's role, as the game's group finder classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Tank,
    Healer,
    Dps,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Tank => "tank",
            Role::Healer => "healer",
            Role::Dps => "dps",
        }
    }
}

/// R17: why a hit did not land, as the combat log's `missType` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MissKind {
    Dodge,
    Parry,
    /// A FULL block; a partial block rides the damage event's `blocked`.
    Block,
    Miss,
    /// A FULLY absorbed hit; a partial absorb rides the damage event.
    Absorb,
    Immune,
    Deflect,
    Evade,
    Reflect,
    /// Never seen in a modern log; modeled so it can never be `Other`.
    Resist,
}

impl MissKind {
    pub const COUNT: usize = 10;

    /// Every kind, in `index` order.
    pub const ALL: [MissKind; MissKind::COUNT] = [
        MissKind::Dodge,
        MissKind::Parry,
        MissKind::Block,
        MissKind::Miss,
        MissKind::Absorb,
        MissKind::Immune,
        MissKind::Deflect,
        MissKind::Evade,
        MissKind::Reflect,
        MissKind::Resist,
    ];

    /// The log's `missType` token; unknown tokens are `None` (the parser
    /// yields `Event::Other`, never an error).
    pub fn parse(s: &str) -> Option<MissKind> {
        Some(match s {
            "DODGE" => MissKind::Dodge,
            "PARRY" => MissKind::Parry,
            "BLOCK" => MissKind::Block,
            "MISS" => MissKind::Miss,
            "ABSORB" => MissKind::Absorb,
            "IMMUNE" => MissKind::Immune,
            "DEFLECT" => MissKind::Deflect,
            "EVADE" => MissKind::Evade,
            "REFLECT" => MissKind::Reflect,
            "RESIST" => MissKind::Resist,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            MissKind::Dodge => "dodge",
            MissKind::Parry => "parry",
            MissKind::Block => "block",
            MissKind::Miss => "miss",
            MissKind::Absorb => "absorb",
            MissKind::Immune => "immune",
            MissKind::Deflect => "deflect",
            MissKind::Evade => "evade",
            MissKind::Reflect => "reflect",
            MissKind::Resist => "resist",
        }
    }

    /// Dense index into `Mitigation::misses`.
    pub fn index(self) -> usize {
        match self {
            MissKind::Dodge => 0,
            MissKind::Parry => 1,
            MissKind::Block => 2,
            MissKind::Miss => 3,
            MissKind::Absorb => 4,
            MissKind::Immune => 5,
            MissKind::Deflect => 6,
            MissKind::Evade => 7,
            MissKind::Reflect => 8,
            MissKind::Resist => 9,
        }
    }
}

/// R17: one player's mitigation over a segment — what was swung at them
/// and did not land on health. The Taken row itself (amount = R1's
/// `amount + absorbed`, `extra` = absorbed, `count` incl. misses) carries
/// the totals; this record carries the split. Every field is additive
/// under the R10 merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mitigation {
    /// Partial absorbs on damage events (the Taken row's `extra`); the
    /// stagger family below is a subset of it.
    pub absorbed: u64,
    /// Partial blocks on damage events (the log's `amount` is post-block).
    pub blocked: u64,
    /// ABSORB misses' `amountMissed` — prevented outright, never Taken.
    pub absorbed_full: u64,
    /// BLOCK misses' amount — prevented outright, never Taken.
    pub blocked_full: u64,
    pub overkill: u64,
    /// `NON_HEALING_ABSORBS` (Stagger, cheat-death …) consumed on the
    /// player. Already inside `absorbed`; reported, never added again.
    pub stagger: u64,
    /// Self-sourced Stagger ticks (124255) re-dealing the staggered amount;
    /// excluded from Taken so a hit is never counted twice.
    pub stagger_ticked: u64,
    /// Miss counts by `MissKind::index`.
    pub misses: [u32; MissKind::COUNT],
}

impl Mitigation {
    /// Damage that was swung with an amount and did not land:
    /// partial absorbs and blocks plus full absorbs and blocks. Dodges,
    /// parries and misses carry no amount and are counts only.
    pub fn mitigated(&self) -> u64 {
        self.absorbed + self.blocked + self.absorbed_full + self.blocked_full
    }

    /// `mitigated` over everything swung with an amount: `taken` (the
    /// Taken row amount, absorbs included) plus the full-miss amounts.
    /// 0..100; 0 when nothing was swung.
    pub fn mitigated_pct(&self, taken: u64) -> f64 {
        let swung = taken + self.absorbed_full + self.blocked_full;
        if swung == 0 {
            0.0
        } else {
            self.mitigated() as f64 * 100.0 / swung as f64
        }
    }

    pub fn miss(&mut self, kind: MissKind) {
        if let Some(n) = self.misses.get_mut(kind.index()) {
            *n += 1;
        }
    }

    pub fn misses_of(&self, kind: MissKind) -> u32 {
        self.misses.get(kind.index()).copied().unwrap_or(0)
    }

    /// Every miss of every kind.
    pub fn misses(&self) -> u32 {
        self.misses.iter().sum()
    }

    pub fn merge(&mut self, other: &Mitigation) {
        self.absorbed += other.absorbed;
        self.blocked += other.blocked;
        self.absorbed_full += other.absorbed_full;
        self.blocked_full += other.blocked_full;
        self.overkill += other.overkill;
        self.stagger += other.stagger;
        self.stagger_ticked += other.stagger_ticked;
        for (a, b) in self.misses.iter_mut().zip(other.misses.iter()) {
            *a += *b;
        }
    }
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
    /// 12.1: the Demon Hunter's third spec (ranged Int caster).
    Devourer,
    Devastation,
    Preservation,
    Augmentation,
}

impl Spec {
    /// Every spec, once — the iteration source for exhaustive checks and
    /// generated tables (the SQL `players.role` CASE). A spec added to the
    /// enum without a row here fails `spec_ids_roundtrip_exhaustively`.
    pub const ALL: [Spec; 40] = [
        Spec::Arms,
        Spec::Fury,
        Spec::ProtectionWarrior,
        Spec::HolyPaladin,
        Spec::ProtectionPaladin,
        Spec::Retribution,
        Spec::BeastMastery,
        Spec::Marksmanship,
        Spec::Survival,
        Spec::Assassination,
        Spec::Outlaw,
        Spec::Subtlety,
        Spec::Discipline,
        Spec::HolyPriest,
        Spec::Shadow,
        Spec::Blood,
        Spec::FrostDeathKnight,
        Spec::Unholy,
        Spec::Elemental,
        Spec::Enhancement,
        Spec::RestorationShaman,
        Spec::Arcane,
        Spec::Fire,
        Spec::FrostMage,
        Spec::Affliction,
        Spec::Demonology,
        Spec::Destruction,
        Spec::Brewmaster,
        Spec::Mistweaver,
        Spec::Windwalker,
        Spec::Balance,
        Spec::Feral,
        Spec::Guardian,
        Spec::RestorationDruid,
        Spec::Havoc,
        Spec::Vengeance,
        Spec::Devourer,
        Spec::Devastation,
        Spec::Preservation,
        Spec::Augmentation,
    ];

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
            1480 => Spec::Devourer,
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
            Spec::Devourer => 1480,
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
            Spec::Havoc | Spec::Vengeance | Spec::Devourer => Class::DemonHunter,
            Spec::Devastation | Spec::Preservation | Spec::Augmentation => Class::Evoker,
        }
    }

    /// The role the game assigns the spec — what "DPS specs only" means in
    /// a ranking, and the median a pull is graded against.
    pub fn role(self) -> Role {
        match self {
            Spec::ProtectionWarrior
            | Spec::ProtectionPaladin
            | Spec::Blood
            | Spec::Brewmaster
            | Spec::Guardian
            | Spec::Vengeance => Role::Tank,
            Spec::HolyPaladin
            | Spec::Discipline
            | Spec::HolyPriest
            | Spec::RestorationShaman
            | Spec::Mistweaver
            | Spec::RestorationDruid
            | Spec::Preservation => Role::Healer,
            _ => Role::Dps,
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
            Spec::Devourer => "Devourer",
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
    /// v13: a temporary buff cast ON the player by someone else — Bloodlust
    /// and its cousins, Power Infusion. Curated to burst externals only;
    /// persistent raid buffs (Arcane Intellect, Mark of the Wild) never mark.
    External,
}

impl MarkKind {
    pub fn code(self) -> u8 {
        match self {
            MarkKind::TrinketUse => 0,
            MarkKind::TrinketProc => 1,
            MarkKind::Consumable => 2,
            MarkKind::External => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => MarkKind::TrinketUse,
            1 => MarkKind::TrinketProc,
            2 => MarkKind::Consumable,
            3 => MarkKind::External,
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
    /// The item spell behind the marker, for client-side icon lookup (v12).
    pub spell_id: u32,
    /// v13: how long the buff behind the marker lasted (aura applied →
    /// removed), so a renderer can fill the active span. 0 = unknown — the
    /// aura never came off inside the segment, or predates duration tracking.
    pub dur_ms: i64,
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
    /// Overheal for Healing, overkill for Damage, absorbed for Taken (R17),
    /// else 0.
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
    /// R13, meter rows only: the player fought on the hostile side of an
    /// arena match (unit-flags reaction bit, only in `arena` segments — never
    /// in world PvP). Sorted views group the friendly team ahead of the enemy
    /// team, so a renderer can split the chart at the first `enemy` row.
    /// Always false on breakdown rows.
    pub enemy: bool,
    /// v15, by-spell breakdown rows: the spell's school bitmask exactly as
    /// the combat log wrote it (1 Physical, 2 Holy, 4 Fire, 8 Nature,
    /// 16 Frost, 32 Shadow, 64 Arcane; combos OR together — Shadowflame is
    /// 0x24). 0 = unknown/none — meter rows and by-target rows stay 0, so a
    /// renderer can color spell bars by school without touching the rest.
    /// First-seen wins per label, like `spell_id`.
    pub school: u32,
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

/// One talent choice from COMBATANT_INFO's talent bracket: the trait node,
/// which of its entries was taken, and the purchased rank. `rank` 0 means the
/// node was selected without a purchased rank — a granted/free node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TalentPick {
    pub node_id: u32,
    pub entry_id: u32,
    pub rank: u32,
}

/// One equipped item from COMBATANT_INFO's gear bracket. Ids only — the log
/// carries no names, and resolving them needs game data a client may not have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GearItem {
    pub item_id: u32,
    pub ilvl: u32,
    pub enchants: Vec<u32>,
    pub bonus_ids: Vec<u32>,
    pub gems: Vec<u32>,
}

/// A player's build as COMBATANT_INFO reported it: talents plus equipped gear
/// in the log's own inventory-slot order. `spec_id` repeats the line's
/// currentSpecID so a consumer can open the right tree without a second lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loadout {
    pub spec_id: Option<u32>,
    pub talents: Vec<TalentPick>,
    pub gear: Vec<GearItem>,
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
    /// v16: a second drill level — one ability of the drilled player, as
    /// (by-spell row key, display label). The key is the breakdown row's own
    /// (`"spell"` or `"spell\0pet"`), never an index, so a re-sort between
    /// frames can't switch which ability is open.
    pub spell: Option<(String, String)>,
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
    /// R13: an arena match — `success` reads WIN/LOSS, not KILL/WIPE.
    pub arena: bool,
    /// v20: ENCOUNTER_START identity (id, difficulty, group size); `None`
    /// off raid-boss Encounter rows.
    pub encounter: Option<Encounter>,
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
    /// R13: an arena match — `success` reads WIN/LOSS, not KILL/WIPE.
    pub arena: bool,
    /// v20: ENCOUNTER_START identity (see [`ListRow::encounter`]).
    pub encounter: Option<Encounter>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `id` and `from_id` document themselves as inverses; hold them to it
    /// in both directions, and pin that ids are unique so two specs can
    /// never claim one COMBATANT_INFO specID.
    #[test]
    fn spec_ids_roundtrip_exhaustively() {
        let mut seen = std::collections::HashSet::new();
        for spec in Spec::ALL {
            assert_eq!(Spec::from_id(spec.id()), Some(spec));
            assert!(seen.insert(spec.id()), "duplicate spec id {}", spec.id());
            // The class route agrees with the direct route.
            assert_eq!(Class::from_spec(spec.id()), Some(spec.class()));
        }
        assert_eq!(seen.len(), Spec::ALL.len());
        assert_eq!(Spec::from_id(0), None);
        assert_eq!(Spec::from_id(9999), None);
        assert_eq!(Class::from_spec(9999), None);
    }

    /// Every class fields exactly three specs — except Druid's four and
    /// Demon Hunter's two — and every class is somebody's class, so the
    /// crest lookup can never meet a class no spec produces.
    #[test]
    fn every_class_is_reachable_and_spec_counts_match_the_game() {
        let mut by_class = std::collections::HashMap::new();
        for spec in Spec::ALL {
            *by_class.entry(spec.class()).or_insert(0u32) += 1;
        }
        assert_eq!(by_class.len(), 13, "all thirteen classes are reachable");
        for (class, n) in by_class {
            let want = match class {
                Class::Druid => 4,
                Class::DemonHunter => 3,
                _ => 3,
            };
            assert_eq!(n, want, "{class:?} has the game's spec count");
        }
    }

    /// Spec names are the in-game, unqualified strings: the four shared
    /// names (Protection, Holy, Frost, Restoration) collapse across classes,
    /// everything else is unique.
    #[test]
    fn shared_spec_names_collapse_and_the_rest_are_unique() {
        for (a, b) in [
            (Spec::ProtectionWarrior, Spec::ProtectionPaladin),
            (Spec::HolyPaladin, Spec::HolyPriest),
            (Spec::FrostDeathKnight, Spec::FrostMage),
            (Spec::RestorationShaman, Spec::RestorationDruid),
        ] {
            assert_eq!(a.name(), b.name());
        }
        let names: std::collections::HashSet<&str> = Spec::ALL.iter().map(|s| s.name()).collect();
        // 40 specs, 4 pairwise-shared names.
        assert_eq!(names.len(), 36);
        assert!(names.iter().all(|n| !n.is_empty()));
    }

    /// Blizzard's class palette: thirteen distinct colors, spot-checked
    /// against the published values a raider would recognize on sight.
    #[test]
    fn class_colors_are_distinct_and_match_the_published_palette() {
        let colors: std::collections::HashSet<(u8, u8, u8)> =
            Spec::ALL.iter().map(|s| s.class().rgb()).collect();
        assert_eq!(colors.len(), 13);
        assert_eq!(Class::Mage.rgb(), (0x3F, 0xC7, 0xEB));
        assert_eq!(Class::DeathKnight.rgb(), (0xC4, 0x1E, 0x3A));
        assert_eq!(Class::Priest.rgb(), (0xFF, 0xFF, 0xFF));
    }

    /// `View::index` is the wire's per-view array key: dense, unique, and
    /// bounded by `COUNT`; only the two throughput views read as rates.
    #[test]
    fn view_indices_are_dense_and_only_throughput_views_are_rates() {
        let all = [
            View::Damage,
            View::Healing,
            View::Interrupts,
            View::CrowdControl,
            View::Dispels,
            View::Deaths,
            View::Taken,
        ];
        assert_eq!(all.len(), View::COUNT);
        let mut seen = [false; View::COUNT];
        for v in all {
            let i = v.index();
            assert!(i < View::COUNT);
            assert!(!std::mem::replace(&mut seen[i], true), "index {i} reused");
            assert_eq!(
                v.is_rate(),
                matches!(v, View::Damage | View::Healing | View::Taken)
            );
        }
    }

    /// The one-byte item/mark codes are wire surface: roundtrip every
    /// variant, reject every byte no variant claims.
    #[test]
    fn item_and_mark_codes_roundtrip_and_reject_strangers() {
        let kinds = [
            ItemKind::Trinket,
            ItemKind::Potion,
            ItemKind::Flask,
            ItemKind::Food,
            ItemKind::Consumable,
        ];
        for k in kinds {
            assert_eq!(ItemKind::from_code(k.code()), Some(k));
        }
        for code in kinds.len() as u8..=u8::MAX {
            assert_eq!(ItemKind::from_code(code), None);
        }
        let marks = [
            MarkKind::TrinketUse,
            MarkKind::TrinketProc,
            MarkKind::Consumable,
            MarkKind::External,
        ];
        for m in marks {
            assert_eq!(MarkKind::from_code(m.code()), Some(m));
        }
        for code in marks.len() as u8..=u8::MAX {
            assert_eq!(MarkKind::from_code(code), None);
        }
    }

    /// Hand-computed rolling DPS: 1s buckets of 1000/2000/3000/4000 damage
    /// under a 3s centred window. The ends clamp — the first and last points
    /// average over the buckets that exist, not over zero-padding.
    #[test]
    fn rolling_dps_clamps_the_window_at_both_ends() {
        let t = Timeline {
            bucket_ms: 1000,
            buckets: vec![1000, 2000, 3000, 4000],
            marks: Vec::new(),
        };
        assert_eq!(t.rolling_dps(3000), vec![1500.0, 2000.0, 3000.0, 3500.0]);
        // A window narrower than one bucket degrades to per-bucket rates.
        assert_eq!(t.rolling_dps(500), vec![1000.0, 2000.0, 3000.0, 4000.0]);
    }

    /// The degenerate timelines a renderer can actually receive: no buckets
    /// (an empty side) and a zero grid (a malformed wire value) both answer
    /// with an empty curve instead of dividing by zero.
    #[test]
    fn degenerate_timelines_answer_empty_not_nan() {
        assert!(Timeline::default().rolling_dps(3000).is_empty());
        let zero_grid = Timeline {
            bucket_ms: 0,
            buckets: vec![1, 2, 3],
            marks: Vec::new(),
        };
        assert!(zero_grid.rolling_dps(3000).is_empty());
        assert!(Timeline::default().cumulative().is_empty());
    }

    /// The cumulative curve is a running total, one point per bucket.
    #[test]
    fn cumulative_is_a_running_total() {
        let t = Timeline {
            bucket_ms: 1000,
            buckets: vec![5, 0, 10, 1],
            marks: Vec::new(),
        };
        assert_eq!(t.cumulative(), vec![5, 5, 15, 16]);
    }

    /// Crit rate guards its own divide: no events is 0%, not NaN — and the
    /// absorb-credit rule (counted, never critting) stays representable.
    #[test]
    fn crit_pct_is_zero_when_nothing_hit() {
        assert_eq!(Row::default().crit_pct(), 0.0);
        let row = Row {
            count: 8,
            crits: 2,
            ..Row::default()
        };
        assert_eq!(row.crit_pct(), 25.0);
    }

    /// The graph-mode toggle is an involution with stable footer labels.
    #[test]
    fn graph_mode_toggles_there_and_back() {
        assert_eq!(GraphMode::default(), GraphMode::Dps);
        assert_eq!(GraphMode::Dps.toggled(), GraphMode::Total);
        assert_eq!(GraphMode::Dps.toggled().toggled(), GraphMode::Dps);
        assert_eq!(GraphMode::Dps.label(), "dps");
        assert_eq!(GraphMode::Total.label(), "total");
    }

    /// The tank and healer sets, listed: a spec added by a patch shows up
    /// here in review instead of silently landing in the DPS bucket.
    #[test]
    fn role_sets_are_exactly_these() {
        let of = |role: Role| -> Vec<Spec> {
            Spec::ALL
                .iter()
                .copied()
                .filter(|s| s.role() == role)
                .collect()
        };
        assert_eq!(
            of(Role::Tank),
            [
                Spec::ProtectionWarrior,
                Spec::ProtectionPaladin,
                Spec::Blood,
                Spec::Brewmaster,
                Spec::Guardian,
                Spec::Vengeance
            ]
        );
        assert_eq!(
            of(Role::Healer),
            [
                Spec::HolyPaladin,
                Spec::Discipline,
                Spec::HolyPriest,
                Spec::RestorationShaman,
                Spec::Mistweaver,
                Spec::RestorationDruid,
                Spec::Preservation
            ]
        );
        assert_eq!(of(Role::Dps).len(), Spec::ALL.len() - 13);
        for r in [Role::Tank, Role::Healer, Role::Dps] {
            assert!(!r.name().is_empty());
        }
    }

    /// `MissKind` is wire and record surface: every kind parses its own
    /// token, has a dense index, and `Mitigation` sums exactly.
    #[test]
    fn miss_kinds_parse_index_and_merge() {
        let mut seen = [false; MissKind::COUNT];
        for k in MissKind::ALL {
            assert_eq!(MissKind::parse(&k.name().to_uppercase()), Some(k));
            assert!(!std::mem::replace(&mut seen[k.index()], true));
        }
        assert_eq!(MissKind::parse("0x1"), None);
        let mut a = Mitigation {
            absorbed: 10,
            blocked: 5,
            absorbed_full: 20,
            blocked_full: 3,
            ..Mitigation::default()
        };
        a.miss(MissKind::Parry);
        let mut b = Mitigation::default();
        b.miss(MissKind::Parry);
        b.miss(MissKind::Dodge);
        b.stagger = 7;
        a.merge(&b);
        assert_eq!(a.misses_of(MissKind::Parry), 2);
        assert_eq!(a.misses(), 3);
        assert_eq!(a.mitigated(), 38, "stagger is inside absorbed, never added");
        // taken 62 + full 23 = 85 swung; 38 / 85.
        assert!((a.mitigated_pct(62) - 38.0 * 100.0 / 85.0).abs() < 1e-9);
        assert_eq!(Mitigation::default().mitigated_pct(0), 0.0);
    }
}
