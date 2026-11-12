//! The `crates/core/src/forever/role_spells.rs` generator: Forever's aura
//! id → the role its buff plays (CONTRACT.md R18), the table Retail's
//! `rolegen` writes for Retail, for an edition whose class model gives
//! every rank of a spell its own id.
//!
//! As `signalgen` does for R30, the membership is CURATED by NAME and kind,
//! never by id: the client, not a hand list, says which ids carry the name.
//! An id joins its name's entry when the client's tables prove:
//!
//! - `SpellName` carries exactly the curated name;
//! - `SpellEffect` gives it an aura the log can apply to a player: an
//!   APPLY_AURA row, or an area aura on the caster's party or raid
//!   (APPLY_AREA_AURA_PARTY, _RAID: Tranquility's player ranks), and no
//!   OVERRIDE_ACTIONBAR_SPELLS aura (a rune's passive, named after the spell
//!   it puts on the bar and never applied as the buff);
//! - `SpellMisc` does not mark it PASSIVE (`Attributes_0` 0x40): a set
//!   bonus or a talent of the same name is never the cooldown's buff
//!   (28752 "Adrenaline Rush", a hunter's set bonus, beside the rogue's);
//! - and either `SpellClassOptions` puts it in the entry's own class's
//!   spell family (`Curated::families`: Divine Protection is the paladin's,
//!   so the priest's absorb of that name, 27779, stays out, and so does a
//!   creature's namesake), or a same-named id in no class's family is
//!   triggered (`EffectTriggerSpell`) or named in its own text
//!   (`Spell.Description_lang`) by a same-named id of those families: Last
//!   Stand's cast (12975) is a dummy whose text names its buff (12976),
//!   which the log applies and no family holds.
//!
//! An entry no id passes fails the build, naming it. The committed census
//! of Forever's logs (`tools/forever/role-spells-census.csv`, from
//! `tools/census-role-spells.sh`: every `SPELL_AURA_APPLIED` BUFF on a
//! `Player-` guid, counted per (id, name) per log) must show one of the
//! entry's ids applied under the same name, unless the entry carries a
//! `census_exempt` reason, printed in the review twin beside the counts it
//! stands in for.
//!
//! Output is two files, as `rolegen`'s: the table (`role_spells.rs`, the
//! same `role_kind` the meter reads) and its review twin
//! (`role_spells.expected.md`) listing every id with its proof and counts.

use crate::rolegen::{Census, Generated, KIND_ORDER};
use crate::spelltip::ids_in;
use crate::table::{Tables, cell, int, text};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use wowdps_core::model::RoleSpellKind;

/// The tables the generator consumes (their FileDataIDs: `table::FILE_IDS`).
pub const TABLES: [&str; 5] = [
    "SpellName",
    "Spell",
    "SpellEffect",
    "SpellClassOptions",
    "SpellMisc",
];

/// `SpellEffect.Effect`: APPLY_AURA, and the area auras a caster puts on
/// its party (APPLY_AREA_AURA_PARTY) or its raid (_RAID).
const EFFECT_APPLY_AURA: i64 = 6;
const EFFECT_AREA_AURA_PARTY: i64 = 35;
const EFFECT_AREA_AURA_RAID: i64 = 65;
/// `SpellEffect.EffectAura` OVERRIDE_ACTIONBAR_SPELLS: a rune's passive.
const AURA_OVERRIDE_ACTIONBAR: i64 = 332;
/// `SpellMisc.Attributes_0` PASSIVE: always on, never a buff a cast gives.
const ATTR0_PASSIVE: i64 = 0x40;

/// `SpellClassOptions.SpellClassSet` of the older class model's classes:
/// mage 3 to shaman 11 (warlock 5 has no entry here).
const MAGE: i64 = 3;
const WARRIOR: i64 = 4;
const PRIEST: i64 = 6;
const DRUID: i64 = 7;
const ROGUE: i64 = 8;
const HUNTER: i64 = 9;
const PALADIN: i64 = 10;
const SHAMAN: i64 = 11;

/// One curated entry: the name the client and the log give every id of
/// it, its kind, the class families it is a spell of, and — for a buff no
/// committed log shows on a player — the reason the census requirement is
/// waived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Curated {
    pub name: &'static str,
    pub kind: RoleSpellKind,
    /// The `SpellClassSet`s whose ids of the name are this entry's.
    pub families: &'static [i64],
    /// `Some(reason)` waives the census check (the client still proves
    /// every id); the reason lands in `role_spells.expected.md`.
    pub census_exempt: Option<&'static str>,
}

impl Curated {
    /// An entry the census must show on a player.
    pub const fn seen(name: &'static str, kind: RoleSpellKind, families: &'static [i64]) -> Self {
        Self {
            name,
            kind,
            families,
            census_exempt: None,
        }
    }

    /// An entry the census is not required to hold, with the reason why.
    pub const fn exempt(
        name: &'static str,
        kind: RoleSpellKind,
        families: &'static [i64],
        reason: &'static str,
    ) -> Self {
        Self {
            name,
            kind,
            families,
            census_exempt: Some(reason),
        }
    }
}

/// The waiver on a buff no committed Forever log shows on a player: the
/// client proves its ids carry the name and an aura in a class's family (or
/// a tie to one that does), not which one a log writes.
const UNSEEN: &str = "no committed log shows it on a player: the client proves these ids, not which one the log applies";

use RoleSpellKind::{ActiveMitigation, Cooldown, Defensive, External, HealingCooldown};

/// The table, by kind, then by class for review. Each name expands to every
/// id the client proves for it. Kinds as Retail's (`rolegen`): a tank's
/// rotational mitigation, a personal defensive, a buff cast on someone else,
/// a major offensive cooldown's own buff, a major healing cooldown's. Left
/// out: Bloodlust (no player's in this client: every rank is a creature's),
/// Berserk (one id in the family is a forced-stance debuff), Metamorphosis
/// (this edition's warlock tanking form, R30's), and the racials, as
/// Retail's table leaves them.
const CURATED: &[Curated] = &[
    // -- ActiveMitigation --------------------------------------------------
    Curated::seen("Shield Block", ActiveMitigation, &[WARRIOR]),
    Curated::seen("Holy Shield", ActiveMitigation, &[PALADIN]),
    // -- Defensive ---------------------------------------------------------
    Curated::seen("Shield Wall", Defensive, &[WARRIOR]),
    Curated::seen("Last Stand", Defensive, &[WARRIOR]),
    Curated::seen("Evasion", Defensive, &[ROGUE]),
    Curated::seen("Ice Block", Defensive, &[MAGE]),
    Curated::seen("Ice Barrier", Defensive, &[MAGE]),
    Curated::seen("Divine Shield", Defensive, &[PALADIN]),
    Curated::seen("Divine Protection", Defensive, &[PALADIN]),
    Curated::exempt("Barkskin", Defensive, &[DRUID], UNSEEN),
    Curated::exempt("Frenzied Regeneration", Defensive, &[DRUID], UNSEEN),
    Curated::exempt("Survival Instincts", Defensive, &[DRUID], UNSEEN),
    Curated::exempt("Dispersion", Defensive, &[PRIEST], UNSEEN),
    // -- External ----------------------------------------------------------
    Curated::seen("Blessing of Protection", External, &[PALADIN]),
    Curated::seen("Power Infusion", External, &[PRIEST]),
    Curated::exempt("Blessing of Sacrifice", External, &[PALADIN], UNSEEN),
    Curated::exempt("Innervate", External, &[DRUID], UNSEEN),
    Curated::exempt("Pain Suppression", External, &[PRIEST], UNSEEN),
    // -- Cooldown ----------------------------------------------------------
    Curated::seen("Sweeping Strikes", Cooldown, &[WARRIOR]),
    Curated::exempt("Recklessness", Cooldown, &[WARRIOR], UNSEEN),
    Curated::exempt("Death Wish", Cooldown, &[WARRIOR], UNSEEN),
    Curated::seen("Blade Flurry", Cooldown, &[ROGUE]),
    Curated::seen("Cold Blood", Cooldown, &[ROGUE]),
    Curated::exempt("Adrenaline Rush", Cooldown, &[ROGUE], UNSEEN),
    Curated::seen("Rapid Fire", Cooldown, &[HUNTER]),
    Curated::exempt("Bestial Wrath", Cooldown, &[HUNTER], UNSEEN),
    Curated::seen("Presence of Mind", Cooldown, &[MAGE]),
    Curated::exempt("Arcane Power", Cooldown, &[MAGE], UNSEEN),
    Curated::exempt("Combustion", Cooldown, &[MAGE], UNSEEN),
    Curated::exempt("Icy Veins", Cooldown, &[MAGE], UNSEEN),
    Curated::exempt("Avenging Wrath", Cooldown, &[PALADIN], UNSEEN),
    // -- HealingCooldown ---------------------------------------------------
    Curated::seen("Divine Favor", HealingCooldown, &[PALADIN]),
    Curated::seen("Inner Focus", HealingCooldown, &[PRIEST]),
    Curated::seen("Nature's Swiftness", HealingCooldown, &[DRUID, SHAMAN]),
    Curated::exempt("Tranquility", HealingCooldown, &[DRUID], UNSEEN),
];

pub fn generate(tables: &Tables, census: &Census, build: &str) -> Result<Generated, String> {
    generate_curated(CURATED, tables, census, build)
}

/// Why an id is in its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proof {
    /// In the entry's class's family, with an aura.
    Family,
    /// Out of the families, triggered by the named family id.
    TriggeredBy(u32),
    /// Out of the families, named in the named family id's text.
    NamedBy(u32),
}

impl Proof {
    fn text(self) -> String {
        match self {
            Proof::Family => "its class's family, an aura".to_string(),
            Proof::TriggeredBy(c) => format!("an aura, triggered by {c}"),
            Proof::NamedBy(c) => format!("an aura, named by {c}"),
        }
    }
}

/// What the client says of the ids the curated names reach.
#[derive(Default)]
struct Client {
    /// id → name, for every id carrying a curated name.
    names: HashMap<u32, String>,
    /// id → its `SpellClassSet`.
    family: HashMap<u32, i64>,
    /// The ids with an aura a player can wear (APPLY_AURA, a party or raid
    /// area aura), and those that are passive (a rune's action-bar
    /// override, or SpellMisc's PASSIVE).
    aura: HashSet<u32>,
    passive: HashSet<u32>,
    /// id → the ids its effects trigger, and the ids its own text names.
    triggers: HashMap<u32, BTreeSet<u32>>,
    named: HashMap<u32, BTreeSet<u32>>,
}

impl Client {
    fn read(tables: &Tables, wanted: &HashSet<&str>) -> Result<Self, String> {
        let mut c = Client::default();
        let sn = tables.get("SpellName")?;
        let (n_id, n_name) = (sn.col("ID")?, sn.col("Name_lang")?);
        for row in &sn.rows {
            let name = cell(row, n_name, "SpellName")?;
            if wanted.contains(name) {
                let id = u32::try_from(int(row, n_id)).unwrap_or(0);
                c.names.insert(id, name.to_string());
            }
        }
        let names = &c.names;
        let reached = |id: i64| u32::try_from(id).ok().filter(|i| names.contains_key(i));

        let co = tables.get("SpellClassOptions")?;
        let (o_spell, o_set) = (co.col("SpellID")?, co.col("SpellClassSet")?);
        let mut family = HashMap::new();
        for row in &co.rows {
            if let Some(id) = reached(int(row, o_spell)) {
                family.insert(id, int(row, o_set));
            }
        }

        let sm = tables.get("SpellMisc")?;
        let (m_spell, m_attr) = (sm.col("SpellID")?, sm.col("Attributes_0")?);
        let mut passive = HashSet::new();
        for row in &sm.rows {
            if let Some(id) = reached(int(row, m_spell))
                && int(row, m_attr) & ATTR0_PASSIVE != 0
            {
                passive.insert(id);
            }
        }

        let sp = tables.get("Spell")?;
        let (s_id, s_desc) = (sp.col("ID")?, sp.col("Description_lang")?);
        let mut named: HashMap<u32, BTreeSet<u32>> = HashMap::new();
        for row in &sp.rows {
            if let Some(id) = reached(int(row, s_id)) {
                let mut refs = HashSet::new();
                ids_in(text(row, s_desc), &mut refs);
                refs.remove(&id);
                if !refs.is_empty() {
                    named.entry(id).or_default().extend(refs);
                }
            }
        }

        let se = tables.get("SpellEffect")?;
        let (e_spell, e_effect, e_aura, e_trigger) = (
            se.col("SpellID")?,
            se.col("Effect")?,
            se.col("EffectAura")?,
            se.col("EffectTriggerSpell")?,
        );
        let mut aura = HashSet::new();
        let mut triggers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
        for row in &se.rows {
            let Some(id) = reached(int(row, e_spell)) else {
                continue;
            };
            let effect = int(row, e_effect);
            if matches!(
                effect,
                EFFECT_APPLY_AURA | EFFECT_AREA_AURA_PARTY | EFFECT_AREA_AURA_RAID
            ) {
                aura.insert(id);
            }
            if effect == EFFECT_APPLY_AURA && int(row, e_aura) == AURA_OVERRIDE_ACTIONBAR {
                passive.insert(id);
            }
            if let Ok(t) = u32::try_from(int(row, e_trigger))
                && t != 0
                && t != id
            {
                triggers.entry(id).or_default().insert(t);
            }
        }
        c.family = family;
        c.aura = aura;
        c.passive = passive;
        c.triggers = triggers;
        c.named = named;
        Ok(c)
    }

    /// `entry`'s ids and why each is in: every same-named buff in one of
    /// its families, then every same-named buff in no class's family that a
    /// same-named id of its families triggers or names. Never a passive.
    fn ids_of(&self, entry: &Curated) -> BTreeMap<u32, Proof> {
        let mut same: Vec<u32> = self
            .names
            .iter()
            .filter(|(_, name)| name.as_str() == entry.name)
            .map(|(id, _)| *id)
            .collect();
        same.sort_unstable();
        let buff = |id: &u32| self.aura.contains(id) && !self.passive.contains(id);
        let own = |id: &u32| {
            self.family
                .get(id)
                .is_some_and(|f| entry.families.contains(f))
        };
        // In no class's family: another class's namesake is never this
        // entry's, however a cast of its own names it.
        let classless = |id: &u32| {
            self.family
                .get(id)
                .is_none_or(|f| !(MAGE..=SHAMAN).contains(f))
        };
        let anchors: Vec<u32> = same.iter().copied().filter(|id| own(id)).collect();
        let mut ids: BTreeMap<u32, Proof> = anchors
            .iter()
            .filter(|id| buff(id))
            .map(|id| (*id, Proof::Family))
            .collect();
        for id in same.iter().filter(|id| classless(id) && buff(id)) {
            let tie = anchors.iter().find_map(|a| {
                if self.triggers.get(a).is_some_and(|t| t.contains(id)) {
                    Some(Proof::TriggeredBy(*a))
                } else if self.named.get(a).is_some_and(|t| t.contains(id)) {
                    Some(Proof::NamedBy(*a))
                } else {
                    None
                }
            });
            if let Some(proof) = tie {
                ids.insert(*id, proof);
            }
        }
        ids
    }
}

/// What the review twin prints beside an entry: the counts of its ids, or
/// the reason it ships without any.
enum Evidence<'a> {
    Counts,
    Exempt(&'a str),
}

/// The generator over an explicit list, so tests can run it on a synthetic
/// handful instead of the real list.
pub fn generate_curated(
    curated: &[Curated],
    tables: &Tables,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    let wanted: HashSet<&str> = curated.iter().map(|e| e.name).collect();
    if wanted.len() != curated.len() {
        return Err("curated list holds a duplicate name".into());
    }
    let client = Client::read(tables, &wanted)?;
    let mut entries: Vec<(&Curated, BTreeMap<u32, Proof>, Evidence<'_>)> = Vec::new();
    let mut table: BTreeMap<u32, RoleSpellKind> = BTreeMap::new();
    for entry in curated {
        let proofs = client.ids_of(entry);
        let ids: Vec<u32> = proofs.keys().copied().collect();
        if ids.is_empty() {
            return Err(format!(
                "role spell {:?} ({}): no id the client names so gives an aura in its class's \
                 family, or is tied to one that does",
                entry.name,
                entry.kind.name()
            ));
        }
        // The census: a row for one of these ids must agree on the name, and
        // one of them must have been applied, unless the entry is waived.
        let mut applied = false;
        for id in &ids {
            if let Some((log_name, per_log)) = census.counts.get(id) {
                if log_name != entry.name {
                    return Err(format!(
                        "role spell {id}: the census names it {log_name:?}, curated as {:?}",
                        entry.name
                    ));
                }
                applied |= per_log.iter().any(|&n| n > 0);
            }
        }
        let evidence = match (applied, entry.census_exempt) {
            (true, _) => Evidence::Counts,
            (false, Some(reason)) => Evidence::Exempt(reason),
            (false, None) => {
                return Err(format!(
                    "role spell {:?} ({}): none of {ids:?} was applied to a player in the census",
                    entry.name,
                    entry.kind.name()
                ));
            }
        };
        for id in &ids {
            if let Some(other) = table.insert(*id, entry.kind) {
                return Err(format!(
                    "role spell {id}: proven as {} and {}",
                    other.name(),
                    entry.kind.name()
                ));
            }
        }
        entries.push((entry, proofs, evidence));
    }
    let rows: Vec<(u32, u8)> = table.iter().map(|(id, k)| (*id, k.code())).collect();
    Ok(Generated {
        spells: rows.len(),
        content: emit(&rows, build)?,
        expected: emit_expected(&entries, census, build)?,
    })
}

fn emit(table: &[(u32, u8)], build: &str) -> Result<String, String> {
    let mut o = String::new();
    let e = |e: std::fmt::Error| format!("emit: {e}");
    o.push_str("//! GENERATED by tools/gen-role-spells.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(e)?;
    writeln!(o, "//! {} role spells.", table.len()).map_err(e)?;
    o.push_str(
        "//!\n\
         //! Maps a combat-log aura id to the role its buff plays — active mitigation,\n\
         //! a defensive, an external, an offensive cooldown or a healing cooldown — so\n\
         //! the meter can open a span on the buff's target with its caster\n\
         //! (CONTRACT.md R18). Membership is curated by name in\n\
         //! tools/extract/src/spangen.rs; the client proves every id, and the\n\
         //! real-log census sits in role_spells.expected.md.\n\
         \n\
         use wowdps_model::RoleSpellKind;\n\
         \n\
         /// The role a buff plays, or `None` for a spell the table does not curate.\n\
         pub(crate) fn role_kind(spell_id: u32) -> Option<RoleSpellKind> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, code) = TABLE.get(i)?;\n\
         \x20   KINDS.get(code as usize).copied()\n\
         }\n\
         \n\
         const KINDS: [RoleSpellKind; 6] = [\n",
    );
    for kind in KIND_ORDER {
        writeln!(o, "    RoleSpellKind::{kind:?},").map_err(e)?;
    }
    o.push_str(
        "];\n\
         \n\
         /// (aura id, kind code), sorted by aura id.\n\
         #[rustfmt::skip]\n\
         static TABLE: &[(u32, u8)] = &[\n",
    );
    for chunk in table.chunks(8) {
        let cells: Vec<String> = chunk.iter().map(|(s, k)| format!("({s},{k}),")).collect();
        writeln!(o, "    {}", cells.join(" ")).map_err(e)?;
    }
    o.push_str(
        "];\n\
         \n\
         #[cfg(test)]\n\
         mod tests {\n\
         \x20   /// Strictly ascending: binary search demands it, and it doubles as\n\
         \x20   /// a dedup check.\n\
         \x20   #[test]\n\
         \x20   fn table_is_sorted_by_spell_id() {\n\
         \x20       assert!(super::TABLE.windows(2).all(|w| w[0].0 < w[1].0));\n\
         \x20   }\n\
         \n\
         \x20   /// Every code in the table names a kind, and a stranger is `None`.\n\
         \x20   #[test]\n\
         \x20   fn every_entry_resolves() {\n\
         \x20       assert!(super::TABLE.iter().all(|e| super::role_kind(e.0).is_some()));\n\
         \x20       assert_eq!(super::role_kind(0), None);\n\
         \x20   }\n\
         }\n",
    );
    Ok(o)
}

fn emit_expected(
    entries: &[(&Curated, BTreeMap<u32, Proof>, Evidence<'_>)],
    census: &Census,
    build: &str,
) -> Result<String, String> {
    let mut o = String::new();
    let e = |e: std::fmt::Error| format!("emit: {e}");
    let ids: usize = entries.iter().map(|(_, ids, _)| ids.len()).sum();
    o.push_str("# role_spells.rs — the curated table and its evidence\n\n");
    o.push_str(
        "GENERATED by tools/gen-role-spells.sh beside `role_spells.rs` — do not edit by hand.\n",
    );
    writeln!(
        o,
        "Build {build}, {ids} role spells from {} curated names.",
        entries.len()
    )
    .map_err(e)?;
    o.push_str(
        "\n## Rules\n\n\
         - Membership is curated in `tools/extract/src/spangen.rs` as `(name, kind, class)`;\n\
         \x20 the client expands each name to its ids. An id joins when `SpellName` carries\n\
         \x20 the name, `SpellEffect` gives it an aura a player can wear (APPLY_AURA, or an\n\
         \x20 area aura on the caster's party or raid) and no rune passive's\n\
         \x20 OVERRIDE_ACTIONBAR_SPELLS aura, `SpellMisc` does not mark it PASSIVE (a set\n\
         \x20 bonus or a talent of the name), and `SpellClassOptions` puts it in the entry's\n\
         \x20 own class's spell family (`its class's family, an aura`), or it is in no class's\n\
         \x20 family and a same-named id of the entry's triggers it (`triggered by N`) or names\n\
         \x20 it in its own text (`named by N`): a cast that is a dummy and the buff it gives.\n\
         \x20 Another class's spell of the name never joins. A name no id passes fails the\n\
         \x20 build.\n\
         - The committed census `tools/forever/role-spells-census.csv`\n\
         \x20 (`tools/census-role-spells.sh <log>...`: every `SPELL_AURA_APPLIED` BUFF on a\n\
         \x20 `Player-` guid, counted per (id, name) per log) must show one of a name's ids\n\
         \x20 applied, under that name, unless the entry carries a `census_exempt` reason,\n\
         \x20 printed below in place of the counts (`exempt: …`).\n\
         - What a WAIVED entry's ids are known to be: each carries the name and an aura in\n\
         \x20 a class's family, or a tie the client records to an id that does. What they\n\
         \x20 are NOT known to be: the id the log applies. No committed log shows one on a\n\
         \x20 player, so a buff the client ties to nothing would be missed, and the first\n\
         \x20 census to hold one settles it.\n\
         - The engine (CONTRACT.md R18) opens a span keyed by the target with the caster as\n\
         \x20 `src` on a Buff apply/refresh of a curated id, and closes it on the removal.\n\
         \n## Entries\n\n",
    );
    let mut header = String::from("| id | name | kind | proof |");
    let mut rule = String::from("| ---: | --- | --- | --- |");
    for log in &census.logs {
        write!(header, " observed ({log}) |").map_err(e)?;
        rule.push_str(" ---: |");
    }
    writeln!(o, "{header}").map_err(e)?;
    writeln!(o, "{rule}").map_err(e)?;
    for kind in KIND_ORDER {
        for (entry, ids, evidence) in entries {
            if entry.kind != kind {
                continue;
            }
            for (id, why) in ids {
                let mut line = format!(
                    "| {id} | {} | {} | {} |",
                    entry.name,
                    kind.name(),
                    why.text()
                );
                let counts = census.counts.get(id).map(|(_, c)| c.as_slice());
                match (evidence, counts) {
                    (Evidence::Exempt(reason), None) => {
                        write!(line, " exempt: {reason} |").map_err(e)?;
                        for _ in 1..census.logs.len() {
                            line.push_str(" — |");
                        }
                    }
                    (_, counts) => {
                        for i in 0..census.logs.len() {
                            let n = counts.and_then(|c| c.get(i)).copied().unwrap_or(0);
                            write!(line, " {n} |").map_err(e)?;
                        }
                    }
                }
                writeln!(o, "{line}").map_err(e)?;
            }
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::parse_csv;

    /// A handful of a client: Last Stand's dummy cast (in the warrior's
    /// family) naming its buff (in no family), Shield Wall in two ranks and
    /// a creature's namesake, a rune's passive, the paladin's Divine
    /// Protection beside a priest's absorb of that name, the rogue's
    /// Adrenaline Rush beside a hunter's passive set bonus of that name, and
    /// Tranquility's party-aura rank beside a creature's.
    fn tables() -> Tables {
        let mut t = Tables::default();
        t.insert(
            "SpellName",
            parse_csv(
                "ID,Name_lang\n\
                 12975,Last Stand\n\
                 12976,Last Stand\n\
                 871,Shield Wall\n\
                 15062,Shield Wall\n\
                 29061,Shield Wall\n\
                 9000,Shield Wall\n\
                 2565,Shield Block\n\
                 498,Divine Protection\n\
                 27779,Divine Protection\n\
                 13750,Adrenaline Rush\n\
                 28752,Adrenaline Rush\n\
                 740,Tranquility\n\
                 9001,Tranquility\n",
            )
            .unwrap(),
        );
        t.insert(
            "Spell",
            parse_csv(
                "ID,Description_lang\n\
                 12975,\"grants $12976s1% of your maximum health for $12976d\"\n\
                 871,\"\"\n",
            )
            .unwrap(),
        );
        // 29061 is a rune's passive; 9000 a creature's (no family).
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,SpellID,Effect,EffectAura,EffectTriggerSpell\n\
                 1,12975,3,0,0\n\
                 2,12976,6,34,0\n\
                 3,871,6,87,0\n\
                 4,15062,6,87,0\n\
                 5,29061,6,332,0\n\
                 6,9000,6,87,0\n\
                 7,2565,6,51,0\n\
                 8,498,6,87,0\n\
                 9,27779,6,69,0\n\
                 10,13750,6,128,0\n\
                 11,28752,6,42,0\n\
                 12,740,35,8,0\n\
                 13,9001,35,8,0\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellClassOptions",
            parse_csv(
                "ID,SpellID,SpellClassSet\n\
                 1,12975,4\n\
                 2,871,4\n\
                 3,15062,4\n\
                 4,29061,4\n\
                 5,2565,4\n\
                 6,498,10\n\
                 7,27779,6\n\
                 8,13750,8\n\
                 9,28752,9\n\
                 10,740,7\n",
            )
            .unwrap(),
        );
        // 28752 is a passive set bonus.
        t.insert(
            "SpellMisc",
            parse_csv(
                "ID,SpellID,Attributes_0\n\
                 1,28752,64\n\
                 2,13750,0\n\
                 3,871,0\n",
            )
            .unwrap(),
        );
        t
    }

    fn census() -> Census {
        Census::parse(
            "id,name,log 1,log 2\n\
             12976,\"Last Stand\",1,0\n\
             871,\"Shield Wall\",0,2\n",
        )
        .unwrap()
    }

    const LIST: &[Curated] = &[
        Curated::seen("Last Stand", Defensive, &[WARRIOR]),
        Curated::seen("Shield Wall", Defensive, &[WARRIOR]),
        Curated::exempt("Shield Block", ActiveMitigation, &[WARRIOR], UNSEEN),
    ];

    #[test]
    fn names_expand_to_the_buffs_the_client_proves() {
        let g = generate_curated(LIST, &tables(), &census(), "1.60.1.70888").unwrap();
        // Last Stand's buff by its cast's text, not the dummy cast itself;
        // Shield Wall's two ranks, not the passive nor the creature's.
        assert_eq!(g.spells, 4);
        assert!(
            g.content
                .contains("    (871,1), (2565,0), (12976,1), (15062,1),\n"),
            "{}",
            g.content
        );
        assert!(g.content.contains("pub(crate) fn role_kind"));
        assert!(
            g.expected
                .contains("| 12976 | Last Stand | defensive | an aura, named by 12975 | 1 | 0 |")
        );
        assert!(
            g.expected.contains(
                "| 15062 | Shield Wall | defensive | its class's family, an aura | 0 | 0 |"
            )
        );
        assert!(g.expected.contains(
            "| 2565 | Shield Block | active_mitigation | its class's family, an aura | exempt: "
        ));
        assert!(!g.expected.contains("| 29061 |") && !g.expected.contains("| 9000 |"));
        assert!(!g.expected.contains("| 12975 |"));
    }

    #[test]
    fn an_entry_the_census_never_saw_fails_unless_waived() {
        let list = [Curated::seen("Shield Block", ActiveMitigation, &[WARRIOR])];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("Shield Block") && err.contains("census"),
            "{err}"
        );
        let list = [Curated::seen("Nothing Like It", Defensive, &[WARRIOR])];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("Nothing Like It"), "{err}");
    }

    #[test]
    fn a_census_row_of_another_name_fails() {
        let census = Census::parse("id,name,log 1\n871,\"Shield Bash\",3\n").unwrap();
        let err = generate_curated(&LIST[1..2], &tables(), &census, "b").unwrap_err();
        assert!(err.contains("871") && err.contains("Shield Bash"), "{err}");
    }

    /// A passive of the curated name (a hunter's set bonus called
    /// Adrenaline Rush) never joins, nor does another class's spell of it
    /// (a priest's absorb called Divine Protection): each name is its own
    /// class's.
    #[test]
    fn a_passive_or_another_classs_namesake_stays_out() {
        let list = [
            Curated::exempt("Adrenaline Rush", Cooldown, &[ROGUE], UNSEEN),
            Curated::exempt("Divine Protection", Defensive, &[PALADIN], UNSEEN),
        ];
        let g = generate_curated(&list, &tables(), &census(), "b").unwrap();
        assert_eq!(g.spells, 2, "{}", g.expected);
        assert!(g.expected.contains("| 13750 | Adrenaline Rush |"));
        assert!(g.expected.contains("| 498 | Divine Protection |"));
        assert!(!g.expected.contains("| 28752 |"), "a passive set bonus");
        assert!(!g.expected.contains("| 27779 |"), "a priest's absorb");
        // Named for the hunter, the set bonus is still passive.
        let list = [Curated::exempt(
            "Adrenaline Rush",
            Cooldown,
            &[HUNTER],
            UNSEEN,
        )];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("Adrenaline Rush"), "{err}");
    }

    /// A rank that puts its aura on the caster's party (Tranquility's)
    /// joins as a buff does; a creature's of the name does not.
    #[test]
    fn a_party_aura_rank_joins() {
        let list = [Curated::exempt(
            "Tranquility",
            HealingCooldown,
            &[DRUID],
            UNSEEN,
        )];
        let g = generate_curated(&list, &tables(), &census(), "b").unwrap();
        assert_eq!(g.spells, 1);
        assert!(
            g.expected
                .contains("| 740 | Tranquility | healing_cooldown | its class's family, an aura |")
        );
        assert!(!g.expected.contains("| 9001 |"));
    }

    /// The kinds' codes are Retail's: one `KINDS` order reads both tables.
    #[test]
    fn kind_codes_are_retails() {
        let g = generate_curated(LIST, &tables(), &census(), "b").unwrap();
        for kind in KIND_ORDER {
            assert!(
                g.content
                    .contains(&format!("    RoleSpellKind::{kind:?},\n"))
            );
        }
    }

    /// Every curated name is unique, names a class, and every waiver says
    /// why.
    #[test]
    fn the_list_is_whole() {
        let names: HashSet<&str> = CURATED.iter().map(|e| e.name).collect();
        assert_eq!(names.len(), CURATED.len());
        assert!(CURATED.iter().all(|e| {
            !e.families.is_empty() && e.families.iter().all(|f| (MAGE..=SHAMAN).contains(f))
        }));
        assert!(
            CURATED
                .iter()
                .all(|e| e.census_exempt.is_none_or(|r| !r.is_empty()))
        );
    }
}
