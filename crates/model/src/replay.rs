//! v45 (R29): a fight as the replay draws it — the history store's REPLAY
//! tier. `core::replay::cut` reads a segment's parsed lines into a [`Cut`],
//! `proto::replay` keeps it as `replay/<fight id>.bin` and writes it back out
//! as the seven files a replay reads (`units.tsv`, `tracks.csv`,
//! `events.csv`, `placed.csv`, `markers.csv`, `raid.csv`, `pull.txt`). Each
//! type here is one of those files' rows, its fields named as the file's
//! columns are and documented against them.
//!
//! What the replay needs and nothing more: where every unit stood — on
//! whichever floor (UiMap) the log put it, so a keystone run follows the
//! party from level to level — which way it faced, how much health, shield
//! and power it had whenever the log said (a [`Post`]); the deaths, the
//! hostile casts, the hits on players, the debuffs on them, the buffs that
//! matter to the fight on them, the players' own debuffs on NPCs and where
//! each boss fight began and ended ([`Event`]); what players placed in the
//! room ([`Placed`]); the raid's world markers ([`Marker`]). The group's damage
//! each second (`raid.csv`) is NOT kept here: R25's raid timeline already
//! answers it from the details tier (`stored_fight`'s `raid`), so the
//! writer takes it from there rather than storing it twice.

use crate::{Class, Spec};

/// A unit's place in [`Cut::units`]: the number every file names it by.
pub type UnitId = u32;

/// `units.tsv`'s `kind`: a player, or an NPC — a boss (the encounter's
/// title names it, else its health is at least 30% of the strongest
/// hostile's), an add (any other hostile unit) or a friendly one (a totem,
/// a guardian, a pet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Player,
    Boss,
    Add,
    Friendly,
}

impl UnitKind {
    pub const ALL: [UnitKind; 4] = [
        UnitKind::Player,
        UnitKind::Boss,
        UnitKind::Add,
        UnitKind::Friendly,
    ];

    /// The file's word for it.
    pub fn word(self) -> &'static str {
        match self {
            UnitKind::Player => "player",
            UnitKind::Boss => "boss",
            UnitKind::Add => "add",
            UnitKind::Friendly => "friendly",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word() == word)
    }
}

/// One line of `units.tsv` (`unit, kind, name, class, spec, spec_name,
/// role, you, npc`) and the unit's track (`tracks.csv`). The unit's number
/// is its place in [`Cut::units`]: units are numbered in the order the cut
/// first sees them — posted, or named by an event — and the units only
/// `placed.csv` names come last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub kind: UnitKind,
    /// The name the log gives it (a player's `Name-Realm-Region`); "?" for
    /// an NPC no line names.
    pub name: String,
    /// A player's GUID — what names the pull's owner ([`Cut::set_owner`])
    /// and ties the unit to the card's players; empty for an NPC, which
    /// `npc` identifies instead. Never written to the files.
    pub guid: String,
    /// A player's class and spec: COMBATANT_INFO's spec, else R8's
    /// inference from their casts (a class alone where no cast names the
    /// spec). `class`/`spec`/`spec_name`/`role` in the file; `None` for an
    /// NPC.
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    /// `you`: the pull's owner, whose log it is.
    pub you: bool,
    /// `npc`: an NPC's creature id (its GUID's sixth field), 0 for none.
    pub npc: u32,
    /// `tracks.csv`'s rows for it, in time order.
    pub posts: Vec<Post>,
}

/// One row of `tracks.csv` (`unit, t_ms, x, y, facing, hp, power_type,
/// power, power_max, map_id, absorb, pools`): where the advanced block put
/// the unit at a time, in the log's own units and precision, kept exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// `t_ms`: milliseconds from the cut's start.
    pub t_ms: u32,
    /// `x`, `y`: the advanced block's position in hundredths of a yard (the
    /// log writes two decimals).
    pub x: i32,
    pub y: i32,
    /// `facing`: radians in ten-thousandths (the log writes four
    /// decimals; the file three).
    pub facing: i32,
    /// `hp`: the unit's health as a share of its most, in tenths of a
    /// percent — exactly the file's one decimal.
    pub hp: u16,
    /// `power_type`, `power`, `power_max`: the pool this line names first
    /// (the first of a cast's `a|b` lists); `None` where it names none.
    pub power: Option<Power>,
    /// `map_id`: the UiMap id the block put the unit on — the floor (a
    /// dungeon's level, a raid's room) `x` and `y` are on. Every post keeps
    /// its own, so a unit that changes floors does so in its track. A
    /// format-1 file kept the cut's [`Cut::floor`] alone, and reads with
    /// that on every post.
    pub map_id: u32,
    /// `absorb` (format 3): the shields on the unit in total, as the block
    /// says them after the line (its field 9); 0 for none, and on every post
    /// of an older file.
    pub absorb: u64,
    /// `pools` (format 3): the LAST reading of every pool kind the unit's
    /// posts have named so far, this one's included — so a class whose
    /// resource only sometimes leads the line (a warlock's shards, a
    /// rogue's combo points) still has it on every later post. By kind
    /// ascending, one reading per kind ([`set_pool`]), at most [`MAX_POOLS`]
    /// of them, each kind below [`POOL_KINDS`]; a kind never named is absent.
    /// An older file's read the same way from its posts' `power`.
    pub pools: Vec<Power>,
}

/// A unit's power reading: the game's power type (0 mana, 1 rage, 2
/// focus, 3 energy, … 17 fury), what it holds and its most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Power {
    pub kind: u32,
    pub current: u64,
    pub max: u64,
}

/// Every power type is below this (the game's run below 32): a type at or
/// above it is no pool — the parser reads none, [`set_pool`] keeps none and
/// the tier refuses one.
pub const POOL_KINDS: u32 = 64;

/// The most pool kinds a post carries (a druid's every form names six):
/// [`set_pool`] keeps no new kind past them, so a post's pools stay a few
/// readings whatever a log or a file names.
pub const MAX_POOLS: usize = 8;

/// `reading` into `pools` (by kind ascending, one per kind): the last
/// reading of its kind replaces the one before; a kind at or above
/// [`POOL_KINDS`], or a new kind once [`MAX_POOLS`] are held, is left out.
/// The one rule by which the cut keeps [`Post::pools`] and the tier reads
/// them back.
pub fn set_pool(pools: &mut Vec<Power>, reading: Power) {
    if reading.kind >= POOL_KINDS {
        return;
    }
    match pools.binary_search_by_key(&reading.kind, |p| p.kind) {
        Ok(at) => {
            if let Some(p) = pools.get_mut(at) {
                *p = reading;
            }
        }
        Err(at) if pools.len() < MAX_POOLS => pools.insert(at, reading),
        Err(_) => {}
    }
}

/// The pools a post carries, tidied as [`set_pool`] keeps them: its own
/// (sorted, one per kind, the later of a kind twice, bounded) — or, when it
/// lists none (a post built by hand, which never filled them; a cut's pools
/// never shrink, so an empty list after a full one is never the cut's),
/// `last`'s with its own pool set in. What the tier writes for any post.
pub fn next_pools(last: &[Power], post: &Post) -> Vec<Power> {
    let mut pools = Vec::new();
    if post.pools.is_empty() {
        pools.extend_from_slice(last);
        if let Some(w) = post.power {
            set_pool(&mut pools, w);
        }
    } else {
        for &w in &post.pools {
            set_pool(&mut pools, w);
        }
    }
    pools
}

/// Every post of `track` given [`next_pools`]: what a track built by hand
/// (an import that fills only `power`) carries once tidied, and what the
/// tier reads back from it.
pub fn fill_pools(track: &mut [Post]) {
    let mut last: Vec<Power> = Vec::new();
    for p in track {
        p.pools = next_pools(&last, p);
        last.clone_from(&p.pools);
    }
}

/// `events.csv`'s `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// A hostile hit on a player — or a miss (a hit a shield took whole,
    /// one they dodged or were immune to), which carries no position.
    Hit,
    /// A hostile unit's cast begun / landed.
    CastStart,
    CastSuccess,
    /// A player's cast begun / landed / failed.
    PcastStart,
    PcastSuccess,
    PcastFailed,
    /// A hostile unit's cast cut short by a player.
    Interrupt,
    /// A debuff going on, coming off, and its stacks changing: on a player
    /// from a hostile unit (or from no one) — `unit` the player, `src` the
    /// NPC — and (format 3) on an NPC from a player — `unit` the NPC, `src`
    /// the player, every such row kept.
    DebuffApplied,
    DebuffRemoved,
    DebuffDose,
    /// A player died; an NPC died (or went down unconscious).
    Death,
    NpcDied,
    /// A player raised.
    Rez,
    /// A boss fight begun (ENCOUNTER_START), and ended killed or wiped
    /// (ENCOUNTER_END): the encounter's id and name as the row's spell, no
    /// unit. A boss pull holds its own two, at t 0 and its end; a keystone
    /// run one pair per boss.
    BossEngaged,
    BossKilled,
    BossWiped,
    /// (Format 3) a buff going on a player, coming off, and its stacks
    /// changing — only a buff that matters to the fight: a defensive, a
    /// tank's mitigation, an external, a major cooldown or a healing
    /// cooldown (R18's curated table, its support buffs aside), or any buff
    /// an NPC or no one put on them. `unit` the player, `src` the caster (an
    /// external's giver). A removal or a dose whose application the cut never
    /// saw is no row: that buff was up before the cut began.
    BuffApplied,
    BuffRemoved,
    BuffDose,
}

impl EventKind {
    pub const ALL: [EventKind; 19] = [
        EventKind::Hit,
        EventKind::CastStart,
        EventKind::CastSuccess,
        EventKind::PcastStart,
        EventKind::PcastSuccess,
        EventKind::PcastFailed,
        EventKind::Interrupt,
        EventKind::DebuffApplied,
        EventKind::DebuffRemoved,
        EventKind::DebuffDose,
        EventKind::Death,
        EventKind::NpcDied,
        EventKind::Rez,
        EventKind::BossEngaged,
        EventKind::BossKilled,
        EventKind::BossWiped,
        EventKind::BuffApplied,
        EventKind::BuffRemoved,
        EventKind::BuffDose,
    ];

    /// The file's word for it.
    pub fn word(self) -> &'static str {
        match self {
            EventKind::Hit => "hit",
            EventKind::CastStart => "cast_start",
            EventKind::CastSuccess => "cast_success",
            EventKind::PcastStart => "pcast_start",
            EventKind::PcastSuccess => "pcast_success",
            EventKind::PcastFailed => "pcast_failed",
            EventKind::Interrupt => "interrupt",
            EventKind::DebuffApplied => "debuff_applied",
            EventKind::DebuffRemoved => "debuff_removed",
            EventKind::DebuffDose => "debuff_dose",
            EventKind::Death => "death",
            EventKind::NpcDied => "npc_died",
            EventKind::Rez => "rez",
            EventKind::BossEngaged => "boss_engaged",
            EventKind::BossKilled => "boss_killed",
            EventKind::BossWiped => "boss_wiped",
            EventKind::BuffApplied => "buff_applied",
            EventKind::BuffRemoved => "buff_removed",
            EventKind::BuffDose => "buff_dose",
        }
    }

    /// A boss fight's beginning or end: a row that names no unit.
    pub fn is_boss(self) -> bool {
        matches!(
            self,
            EventKind::BossEngaged | EventKind::BossKilled | EventKind::BossWiped
        )
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word() == word)
    }

    /// How many of `events.csv`'s eleven columns a line of this kind
    /// writes: up to its last field and no further, as the file always
    /// had it (a cast 11, through `target`; a hit 10, through `base`; a
    /// dose 9, through `stacks`; the rest 8, through `src` — a boss row's
    /// `unit` and `src` empty).
    pub fn columns(self) -> usize {
        match self {
            EventKind::CastStart
            | EventKind::CastSuccess
            | EventKind::PcastStart
            | EventKind::PcastSuccess
            | EventKind::PcastFailed => 11,
            EventKind::Hit => 10,
            EventKind::DebuffDose | EventKind::BuffDose => 9,
            _ => 8,
        }
    }

    /// Whether a line of this kind names a spell (`spell_id`, `spell`).
    pub fn has_spell(self) -> bool {
        !matches!(self, EventKind::Death | EventKind::NpcDied)
    }
}

/// One line of `events.csv` (`t_ms, kind, unit, spell_id, spell, x, y,
/// src, stacks, base, target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub t_ms: u32,
    pub kind: EventKind,
    /// `unit`: whom it is about — the victim of a hit or a death, the one
    /// wearing a debuff or a buff (a player's debuff on an NPC: the NPC),
    /// the caster of a cast, the unit whose cast was interrupted; `None` on
    /// a boss row alone (an encounter is no unit).
    pub unit: Option<UnitId>,
    /// `spell_id`, `spell`: the ability (an interrupt's: the cast it cut
    /// short); 0 and empty on a death; a boss row's encounter id and name.
    pub spell_id: u32,
    pub spell: String,
    /// `x`, `y`: where a hit found its victim (hundredths of a yard, the
    /// victim's own advanced block, on whatever floor it put them: the
    /// victim's post at that moment carries the floor); `None` for a miss
    /// and every other kind.
    pub at: Option<(i32, i32)>,
    /// `src`: who did it — a hit's attacker, a debuff's or a buff's source
    /// (an external's giver), an interrupter, a rezzer; `None` for no one.
    pub src: Option<UnitId>,
    /// `stacks`: a dose's new stack count.
    pub stacks: Option<u16>,
    /// `base`: a hit's amount before the victim's mitigation (the damage
    /// suffix's unmitigated amount); `None` on a miss.
    pub base: Option<u64>,
    /// `target`: whom a cast was aimed at.
    pub target: Option<UnitId>,
}

/// `placed.csv`'s `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlacedKind {
    /// A player's landed cast of a placing spell, where they stood, and
    /// the unit it was aimed at.
    Cast,
    /// A player's summon (or create) of one, and the unit it made.
    Summon,
    /// A spell that tells where one stands reaching a unit: an aura put on
    /// it, or a heal landing on it (where the heal found it).
    Touch,
    /// A unit a placing summon made dying or destroyed.
    Gone,
}

impl PlacedKind {
    pub const ALL: [PlacedKind; 4] = [
        PlacedKind::Cast,
        PlacedKind::Summon,
        PlacedKind::Touch,
        PlacedKind::Gone,
    ];

    pub fn word(self) -> &'static str {
        match self {
            PlacedKind::Cast => "cast",
            PlacedKind::Summon => "summon",
            PlacedKind::Touch => "touch",
            PlacedKind::Gone => "gone",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word() == word)
    }
}

/// One line of `placed.csv` (`t_ms, kind, unit, spell_id, spell, x, y,
/// src, target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub t_ms: u32,
    pub kind: PlacedKind,
    /// `unit`: the caster or summoner, the unit touched, the unit gone; a
    /// touch of a unit nothing else names has none.
    pub unit: Option<UnitId>,
    /// 0 and empty on `gone`.
    pub spell_id: u32,
    pub spell: String,
    /// `x`, `y`: where a cast's caster or a heal's target stood
    /// (hundredths of a yard).
    pub at: Option<(i32, i32)>,
    /// `src`: a touch's source.
    pub src: Option<UnitId>,
    /// `target`: a cast's aim, a summon's unit.
    pub target: Option<UnitId>,
}

/// `markers.csv`'s `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerKind {
    Placed,
    Removed,
}

impl MarkerKind {
    pub fn word(self) -> &'static str {
        match self {
            MarkerKind::Placed => "placed",
            MarkerKind::Removed => "removed",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        [MarkerKind::Placed, MarkerKind::Removed]
            .into_iter()
            .find(|k| k.word() == word)
    }
}

/// One line of `markers.csv` (`t_ms, kind, marker, x, y`): a raid world
/// marker standing at the cut's start (t 0, in marker order) or placed or
/// removed during it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub t_ms: u32,
    pub kind: MarkerKind,
    /// The log's marker number: square 0 to skull 7 (the game's world
    /// marker n + 1).
    pub marker: u8,
    /// Where it stands (hundredths of a yard); `None` on a removal.
    pub at: Option<(i32, i32)>,
}

/// The encounter a cut opens with: its ENCOUNTER_START's id, name,
/// difficulty and group size.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EncounterHead {
    pub id: u32,
    pub name: String,
    pub difficulty: u32,
    pub size: u32,
}

/// The keystone the cut is inside: its dungeon's name and level.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyHead {
    pub name: String,
    pub level: u32,
}

/// What `pull.txt` says of the cut, as the log said it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Head {
    /// The encounter the cut opens with; `None` for a keystone run's whole
    /// visit.
    pub encounter: Option<EncounterHead>,
    /// The keystone at the cut's start (or, for a run, the run's own).
    pub key: Option<KeyHead>,
    /// The instance's map id: ENCOUNTER_START's, else the run's.
    pub map: u32,
    /// The zone the log was in at the start (the last ZONE_CHANGE's name).
    pub zone: String,
    /// How it ended: ENCOUNTER_END's kill or wipe, a run's timed or not;
    /// `None` when the cut holds no end.
    pub success: Option<bool>,
    /// The end's own clock: ENCOUNTER_END's fight time, a run's official
    /// time.
    pub fight_ms: Option<u64>,
    /// The cut's first line, in UTC milliseconds.
    pub start_utc_ms: i64,
    /// The cut's first line's local date: year, month, day.
    pub date: (u16, u8, u8),
}

/// One floor the cut's posts stand on ([`Cut::maps`]): its UiMap id, how
/// many posts stand there and how many of those are players'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapPosts {
    pub map_id: u32,
    pub posts: usize,
    pub players: usize,
}

/// A fight as the replay draws it: its head, its main floor, and every row
/// of the files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cut {
    pub head: Head,
    /// The main floor (a UiMap id): where the players were posted most, a
    /// tie to the lower id — what a boss pull is drawn on. Every post keeps
    /// its own [`Post::map_id`] besides ([`Cut::maps`] lists them). 0 when
    /// no player was posted (a log written without the advanced block).
    pub floor: u32,
    pub units: Vec<Unit>,
    /// In time order (the log's).
    pub events: Vec<Event>,
    pub placed: Vec<Placed>,
    pub markers: Vec<Marker>,
}

impl Cut {
    /// Mark the pull's owner — the player whose GUID it is — as `you`, and
    /// nobody else.
    pub fn set_owner(&mut self, owner: Option<&str>) {
        for u in &mut self.units {
            u.you =
                u.kind == UnitKind::Player && owner.is_some_and(|o| !o.is_empty() && u.guid == o);
        }
    }

    /// Every post in the cut.
    pub fn posts(&self) -> usize {
        self.units.iter().map(|u| u.posts.len()).sum()
    }

    /// Every floor the posts stand on, by id, with its posts and its
    /// players' posts: the [`Cut::floor`] is the one with the most players'
    /// (a tie to the lower id).
    pub fn maps(&self) -> Vec<MapPosts> {
        let mut maps: Vec<MapPosts> = Vec::new();
        for u in &self.units {
            let player = u.kind == UnitKind::Player;
            for p in &u.posts {
                let at = match maps.binary_search_by_key(&p.map_id, |m| m.map_id) {
                    Ok(at) => at,
                    Err(at) => {
                        maps.insert(
                            at,
                            MapPosts {
                                map_id: p.map_id,
                                posts: 0,
                                players: 0,
                            },
                        );
                        at
                    }
                };
                if let Some(m) = maps.get_mut(at) {
                    m.posts += 1;
                    m.players += usize::from(player);
                }
            }
        }
        maps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_reads_back() {
        for k in UnitKind::ALL {
            assert_eq!(UnitKind::from_word(k.word()), Some(k));
        }
        for k in EventKind::ALL {
            assert_eq!(EventKind::from_word(k.word()), Some(k));
        }
        for k in PlacedKind::ALL {
            assert_eq!(PlacedKind::from_word(k.word()), Some(k));
        }
        for k in [MarkerKind::Placed, MarkerKind::Removed] {
            assert_eq!(MarkerKind::from_word(k.word()), Some(k));
        }
        assert_eq!(EventKind::from_word("nope"), None);
        let boss: Vec<&str> = EventKind::ALL
            .iter()
            .filter(|k| k.is_boss())
            .map(|k| k.word())
            .collect();
        assert_eq!(boss, ["boss_engaged", "boss_killed", "boss_wiped"]);
        let doses: Vec<&str> = EventKind::ALL
            .iter()
            .filter(|k| k.columns() == 9)
            .map(|k| k.word())
            .collect();
        assert_eq!(doses, ["debuff_dose", "buff_dose"]);
        assert_eq!(EventKind::BuffApplied.columns(), 8);
    }

    #[test]
    fn a_pool_reading_replaces_its_kinds_last() {
        let w = |kind, current, max| Power { kind, current, max };
        let mut pools = Vec::new();
        set_pool(&mut pools, w(0, 900, 1000));
        set_pool(&mut pools, w(7, 30, 50));
        set_pool(&mut pools, w(3, 40, 100));
        assert_eq!(pools, [w(0, 900, 1000), w(3, 40, 100), w(7, 30, 50)]);
        set_pool(&mut pools, w(7, 10, 50));
        set_pool(&mut pools, w(0, 800, 1200));
        assert_eq!(pools, [w(0, 800, 1200), w(3, 40, 100), w(7, 10, 50)]);
        // No kind at or above POOL_KINDS, and no new kind past MAX_POOLS.
        set_pool(&mut pools, w(POOL_KINDS, 1, 1));
        set_pool(&mut pools, w(u32::MAX, 1, 1));
        assert_eq!(pools.len(), 3);
        for kind in 10..100 {
            set_pool(&mut pools, w(kind, 1, 1));
        }
        assert_eq!(pools.len(), MAX_POOLS);
        assert!(pools.windows(2).all(|p| p[0].kind < p[1].kind));
        set_pool(&mut pools, w(3, 5, 100));
        assert!(pools.contains(&w(3, 5, 100)), "a held kind still updates");
    }

    /// A post built by hand is tidied as the cut keeps its pools: its own
    /// sorted, one per kind, bounded; none listed, its last's with its own
    /// pool set in.
    #[test]
    fn a_hand_built_track_fills_its_pools() {
        let w = |kind, current, max| Power { kind, current, max };
        let post = |power: Option<Power>, pools: Vec<Power>| Post {
            t_ms: 0,
            x: 0,
            y: 0,
            facing: 0,
            hp: 0,
            power,
            map_id: 1,
            absorb: 0,
            pools,
        };
        let mut track = vec![
            post(Some(w(0, 90, 100)), Vec::new()),
            post(Some(w(7, 2, 5)), Vec::new()),
            post(None, Vec::new()),
            post(
                Some(w(0, 80, 100)),
                vec![w(7, 2, 5), w(0, 70, 100), w(0, 80, 100), w(99, 1, 1)],
            ),
        ];
        fill_pools(&mut track);
        let pools: Vec<Vec<Power>> = track.iter().map(|p| p.pools.clone()).collect();
        assert_eq!(
            pools,
            [
                vec![w(0, 90, 100)],
                vec![w(0, 90, 100), w(7, 2, 5)],
                vec![w(0, 90, 100), w(7, 2, 5)],
                vec![w(0, 80, 100), w(7, 2, 5)],
            ]
        );
    }

    #[test]
    fn the_maps_count_every_floors_posts() {
        let post = |map_id| Post {
            t_ms: 0,
            x: 0,
            y: 0,
            facing: 0,
            hp: 0,
            power: None,
            map_id,
            absorb: 0,
            pools: Vec::new(),
        };
        let unit = |kind, posts| Unit {
            kind,
            name: "n".into(),
            guid: String::new(),
            class: None,
            spec: None,
            you: false,
            npc: 0,
            posts,
        };
        let cut = Cut {
            floor: 2094,
            units: vec![
                unit(UnitKind::Player, vec![post(2094), post(2095), post(2094)]),
                unit(UnitKind::Boss, vec![post(2095), post(2095)]),
                unit(UnitKind::Player, vec![post(2094)]),
            ],
            ..Cut::default()
        };
        let maps: Vec<(u32, usize, usize)> = cut
            .maps()
            .iter()
            .map(|m| (m.map_id, m.posts, m.players))
            .collect();
        assert_eq!(maps, [(2094, 3, 3), (2095, 3, 1)]);
        assert!(Cut::default().maps().is_empty());
    }

    #[test]
    fn the_owner_is_one_player_by_guid() {
        let unit = |kind, guid: &str| Unit {
            kind,
            name: "n".into(),
            guid: guid.into(),
            class: None,
            spec: None,
            you: false,
            npc: 0,
            posts: Vec::new(),
        };
        let mut cut = Cut {
            units: vec![
                unit(UnitKind::Player, "Player-1-A"),
                unit(UnitKind::Player, "Player-1-B"),
                unit(UnitKind::Boss, ""),
            ],
            ..Cut::default()
        };
        cut.set_owner(Some("Player-1-B"));
        let you: Vec<bool> = cut.units.iter().map(|u| u.you).collect();
        assert_eq!(you, [false, true, false]);
        cut.set_owner(Some(""));
        assert!(cut.units.iter().all(|u| !u.you), "an empty owner is nobody");
        cut.set_owner(None);
        assert!(cut.units.iter().all(|u| !u.you));
    }
}
