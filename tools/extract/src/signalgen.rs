//! The `crates/core/src/forever/role_signals.rs` generator: spell id → what
//! casting it (or, for a form, standing in it) says about a player's role
//! in a fight (CONTRACT.md R30), for an edition whose log carries no spec.
//! Retail has no such table: its role is its spec's.
//!
//! Like `rolegen` the membership is CURATED, but by NAME and kind, never by
//! id: an older class model gives every rank of a spell its own id
//! (Healing Wave is ten), and the client, not a hand list, says which ids
//! carry the name. An id joins its name's entry when the client's tables
//! prove all three of:
//!
//! - `SpellName` carries exactly the curated name;
//! - `SpellClassOptions` puts it in a class's spell family
//!   (`CLASS_FAMILIES`), so a creature's namesake (an NPC's Swipe or Heal)
//!   stays out;
//! - its kind's own mark: a taunt has an ATTACK_ME effect or a MOD_TAUNT
//!   aura (which keeps Earth Shock's plain ranks out and its taunting ones
//!   in); a shield cast a `SpellEquippedItems` row that requires a shield;
//!   a form a MOD_SHAPESHIFT aura into the `SpellShapeshiftForm` of the
//!   same name; a heal a HEAL effect or a PERIODIC_HEAL aura. Mitigation
//!   (Revenge, Last Stand) has no mark of its own beyond the family, but an
//!   entry may also name the forms it is cast in (`Curated::in_form`): a
//!   bear's own attacks join only where their `SpellShapeshift` row requires
//!   Bear Form or Dire Bear Form, which keeps the cat's Swipe, a creature
//!   bear's and a hunter pet's out.
//!
//! The meter reads a CAST line's id, and for a heal or a taunt that is often
//! not the id carrying the mark: an older heal is a parent cast whose effect
//! is a dummy or a trigger, and the heal its child (Holy Shock's rank 20473
//! heals as 25914; Penance's cast 402174 as 402289). So for those two kinds a
//! same-named id in the family also joins when the client ties it to an id
//! already in: one of its effects triggers that id (`EffectTriggerSpell`),
//! or its own text names it (`Spell.Description_lang`, `$25914s1`), read to
//! a fixpoint in id order. A rune's passive of the same name (an id with
//! the OVERRIDE_ACTIONBAR_SPELLS aura, which puts the spell on the bar) is
//! never cast and never joins this way. A cast whose client data names no such child (Earth Shield's,
//! Prayer of Mending's) stays out: what a cast line writes for it is known
//! only once a census holds one.
//!
//! An entry no id passes fails the build, naming it. The committed census
//! of real logs (`tools/role-signals-census.csv`, from
//! `tools/census-role-signals.sh`: every `SPELL_CAST_SUCCESS` a player
//! wrote, counted per spell) must show a player casting at least one of
//! the entry's ids under the same name, unless the entry carries a
//! `census_exempt` reason: the first edition this table serves has two
//! dungeon logs on hand, and a class's tanking or healing kit nobody in
//! them played still has to count. The reason is printed in the review
//! twin beside the counts it stands in for.
//!
//! Output is two files: the table (`role_signals.rs`) and its review twin
//! (`role_signals.expected.md`) listing every id with its proof and counts.

use crate::rolegen::Census;
use crate::spelltip::ids_in;
use crate::table::{Tables, cell, int, text};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use wowdps_core::roles::RoleSignal;

/// The tables the generator consumes (their FileDataIDs: `table::FILE_IDS`).
pub const TABLES: [&str; 7] = [
    "SpellName",
    "Spell",
    "SpellEffect",
    "SpellEquippedItems",
    "SpellShapeshift",
    "SpellShapeshiftForm",
    "SpellClassOptions",
];

/// `SpellClassOptions.SpellClassSet` of the nine classes of the older class
/// model: mage 3, warrior 4, warlock 5, priest 6, druid 7, rogue 8, hunter
/// 9, paladin 10, shaman 11. A hunter's pet abilities carry 9 too, which is
/// harmless: only a player's own casts are read.
pub(crate) const CLASS_FAMILIES: [i64; 9] = [3, 4, 5, 6, 7, 8, 9, 10, 11];

/// `SpellEffect.Effect`: HEAL and ATTACK_ME.
const EFFECT_HEAL: i64 = 10;
const EFFECT_ATTACK_ME: i64 = 114;
/// `SpellEffect.EffectAura`: PERIODIC_HEAL, MOD_TAUNT and MOD_SHAPESHIFT.
const AURA_PERIODIC_HEAL: i64 = 8;
const AURA_MOD_TAUNT: i64 = 11;
const AURA_MOD_SHAPESHIFT: i64 = 36;
/// `SpellEffect.EffectAura` OVERRIDE_ACTIONBAR_SPELLS: what a rune's passive
/// is, named after the spell it puts on the bar and never itself cast.
const AURA_OVERRIDE_ACTIONBAR: i64 = 332;
/// `SpellEffect.Effect` APPLY_AURA.
const EFFECT_APPLY_AURA: i64 = 6;
/// `SpellEquippedItems`: the armor item class, and the shield's bit in its
/// subclass mask (armor subclass 6).
const ITEM_CLASS_ARMOR: i64 = 4;
const SHIELD_SUBCLASS_BIT: i64 = 1 << 6;

/// One curated entry: the name the client and the log give every id of
/// it, its kind, and — for a kit no committed log shows a player casting —
/// the reason the census requirement is waived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Curated {
    pub name: &'static str,
    pub kind: RoleSignal,
    /// `Some(reason)` waives the census check (the client still proves
    /// every id); the reason lands in `role_signals.expected.md`.
    pub census_exempt: Option<&'static str>,
    /// The forms (`SpellShapeshiftForm` names) one of which an id's
    /// `SpellShapeshift` row must require; empty for none.
    pub in_form: &'static [&'static str],
}

impl Curated {
    /// An entry the census must show a player casting.
    pub const fn seen(name: &'static str, kind: RoleSignal) -> Self {
        Self {
            name,
            kind,
            census_exempt: None,
            in_form: &[],
        }
    }

    /// An entry the census is not required to hold, with the reason why.
    pub const fn exempt(name: &'static str, kind: RoleSignal, reason: &'static str) -> Self {
        Self {
            name,
            kind,
            census_exempt: Some(reason),
            in_form: &[],
        }
    }

    /// The entry with its ids held to the casts made in one of `forms`.
    pub const fn in_form(self, forms: &'static [&'static str]) -> Self {
        Self {
            in_form: forms,
            ..self
        }
    }
}

/// A bear's forms: what the druid's own tanking attacks are cast in.
const BEAR: &[&str] = &["Bear Form", "Dire Bear Form"];

/// The waiver on a kit no committed log has a player of: the client proves
/// its ids carry the name, the family and the kind (or tie to one that
/// does), not that a cast line writes them; the first log of such a tank or
/// healer joins the census and settles it.
const UNSEEN: &str = "no committed log holds a player casting it: the client proves these ids, not which one a cast line writes";

/// The table, by kind, then by class for review. Each name expands to
/// every id the client proves for it (the module's three proofs).
const CURATED: &[Curated] = &[
    // -- Taunt: makes the target attack the caster -------------------------
    Curated::seen("Taunt", RoleSignal::Taunt),
    Curated::exempt("Mocking Blow", RoleSignal::Taunt, UNSEEN),
    Curated::seen("Challenging Shout", RoleSignal::Taunt),
    Curated::seen("Growl", RoleSignal::Taunt),
    Curated::exempt("Challenging Roar", RoleSignal::Taunt, UNSEEN),
    // The paladin's, the warlock's (two, under Metamorphosis), the rogue's
    // and the shaman's taunts; Earth Shock's plain ranks fail the taunt
    // proof and stay out.
    Curated::exempt("Hand of Reckoning", RoleSignal::Taunt, UNSEEN),
    Curated::exempt("Menace", RoleSignal::Taunt, UNSEEN),
    Curated::exempt("Demonic Howl", RoleSignal::Taunt, UNSEEN),
    Curated::exempt("Tease", RoleSignal::Taunt, UNSEEN),
    Curated::exempt("Earth Shock", RoleSignal::Taunt, UNSEEN),
    // -- Shield: castable only with a shield equipped -----------------------
    Curated::seen("Shield Block", RoleSignal::Shield),
    Curated::seen("Shield Bash", RoleSignal::Shield),
    Curated::seen("Shield Wall", RoleSignal::Shield),
    Curated::exempt("Shield Slam", RoleSignal::Shield, UNSEEN),
    Curated::exempt("Shockwave", RoleSignal::Shield, UNSEEN),
    Curated::exempt("Holy Shield", RoleSignal::Shield, UNSEEN),
    Curated::exempt("Shield of Righteousness", RoleSignal::Shield, UNSEEN),
    // -- Mitigation: a tank's mitigation or threat cast, no shield needed ---
    Curated::seen("Revenge", RoleSignal::Mitigation),
    Curated::seen("Last Stand", RoleSignal::Mitigation),
    Curated::seen("Righteous Fury", RoleSignal::Mitigation),
    // A bear's own: only the ranks cast in Bear Form (the rune's Frenzied
    // Regeneration works in any form, the cat has a Swipe, a hunter's pet
    // another, and a creature's bear form a third).
    Curated::exempt("Frenzied Regeneration", RoleSignal::Mitigation, UNSEEN).in_form(BEAR),
    Curated::seen("Maul", RoleSignal::Mitigation).in_form(BEAR),
    Curated::seen("Swipe", RoleSignal::Mitigation).in_form(BEAR),
    Curated::seen("Demoralizing Roar", RoleSignal::Mitigation).in_form(BEAR),
    // -- Form: a tanking form or stance, the aura on the player -------------
    Curated::seen("Defensive Stance", RoleSignal::Form),
    Curated::seen("Bear Form", RoleSignal::Form),
    Curated::seen("Dire Bear Form", RoleSignal::Form),
    // The warlock's tanking form.
    Curated::exempt("Metamorphosis", RoleSignal::Form, UNSEEN),
    // -- Heal: a healing cast ----------------------------------------------
    Curated::seen("Healing Wave", RoleSignal::Heal),
    Curated::seen("Lesser Healing Wave", RoleSignal::Heal),
    Curated::exempt("Chain Heal", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Riptide", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Earth Shield", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Healing Rain", RoleSignal::Heal, UNSEEN),
    Curated::seen("Healing Touch", RoleSignal::Heal),
    Curated::seen("Regrowth", RoleSignal::Heal),
    Curated::seen("Rejuvenation", RoleSignal::Heal),
    Curated::exempt("Tranquility", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Lifebloom", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Wild Growth", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Nourish", RoleSignal::Heal, UNSEEN),
    Curated::seen("Lesser Heal", RoleSignal::Heal),
    Curated::exempt("Heal", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Greater Heal", RoleSignal::Heal, UNSEEN),
    Curated::seen("Flash Heal", RoleSignal::Heal),
    Curated::seen("Renew", RoleSignal::Heal),
    Curated::exempt("Prayer of Healing", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Circle of Healing", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Penance", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Prayer of Mending", RoleSignal::Heal, UNSEEN),
    Curated::seen("Holy Light", RoleSignal::Heal),
    Curated::seen("Flash of Light", RoleSignal::Heal),
    Curated::exempt("Holy Shock", RoleSignal::Heal, UNSEEN),
    Curated::exempt("Divine Light", RoleSignal::Heal, UNSEEN),
    // The mage's heal.
    Curated::exempt("Mass Regeneration", RoleSignal::Heal, UNSEEN),
];

#[derive(Debug)]
pub struct Generated {
    /// `role_signals.rs`.
    pub content: String,
    /// `role_signals.expected.md`.
    pub expected: String,
    /// The table's ids, and the curated entries they came from.
    pub spells: usize,
    pub entries: usize,
}

pub fn generate(tables: &Tables, census: &Census, build: &str) -> Result<Generated, String> {
    generate_curated(CURATED, tables, census, build)
}

/// What the client says of the ids the curated names reach.
#[derive(Default)]
struct Client {
    /// id → name, for every id carrying a curated name.
    names: HashMap<u32, String>,
    /// The ids in a class's spell family.
    family: HashSet<u32>,
    taunt: HashSet<u32>,
    shield: HashSet<u32>,
    heal: HashSet<u32>,
    /// id → the names of the forms it shapeshifts into.
    forms: HashMap<u32, Vec<String>>,
    /// id → the ids its effects trigger (`EffectTriggerSpell`).
    triggers: HashMap<u32, BTreeSet<u32>>,
    /// id → the spell ids its own description names (`$25914s1`).
    named: HashMap<u32, BTreeSet<u32>>,
    /// id → the form ids its `SpellShapeshift` row requires.
    cast_in: HashMap<u32, BTreeSet<i64>>,
    /// The ids with a rune's OVERRIDE_ACTIONBAR_SPELLS aura: passives.
    passive: HashSet<u32>,
    /// Form name → its `SpellShapeshiftForm` id.
    form_ids: HashMap<String, i64>,
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
        let reached = |id: i64| u32::try_from(id).ok().filter(|i| c.names.contains_key(i));

        let co = tables.get("SpellClassOptions")?;
        let (o_spell, o_set) = (co.col("SpellID")?, co.col("SpellClassSet")?);
        let mut family = HashSet::new();
        for row in &co.rows {
            if let Some(id) = reached(int(row, o_spell))
                && CLASS_FAMILIES.contains(&int(row, o_set))
            {
                family.insert(id);
            }
        }

        let forms_t = tables.get("SpellShapeshiftForm")?;
        let (f_id, f_name) = (forms_t.col("ID")?, forms_t.col("Name_lang")?);
        let mut form_names: HashMap<i64, String> = HashMap::new();
        for row in &forms_t.rows {
            form_names.insert(
                int(row, f_id),
                cell(row, f_name, "SpellShapeshiftForm")?.into(),
            );
        }
        let form_ids: HashMap<String, i64> =
            form_names.iter().map(|(id, n)| (n.clone(), *id)).collect();

        // A form id f is bit f − 1 of the two masks (1–32, then 33–64).
        let ss = tables.get("SpellShapeshift")?;
        let (h_spell, h_mask0, h_mask1) = (
            ss.col("SpellID")?,
            ss.col("ShapeshiftMask_0")?,
            ss.col("ShapeshiftMask_1")?,
        );
        let mut cast_in: HashMap<u32, BTreeSet<i64>> = HashMap::new();
        for row in &ss.rows {
            if let Some(id) = reached(int(row, h_spell)) {
                let (m0, m1) = (int(row, h_mask0), int(row, h_mask1));
                let forms = cast_in.entry(id).or_default();
                for bit in 0..32 {
                    if m0 >> bit & 1 == 1 {
                        forms.insert(bit + 1);
                    }
                    if m1 >> bit & 1 == 1 {
                        forms.insert(bit + 33);
                    }
                }
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
        let (e_spell, e_effect, e_aura, e_misc, e_trigger) = (
            se.col("SpellID")?,
            se.col("Effect")?,
            se.col("EffectAura")?,
            se.col("EffectMiscValue_0")?,
            se.col("EffectTriggerSpell")?,
        );
        let (mut taunt, mut heal) = (HashSet::new(), HashSet::new());
        let mut forms: HashMap<u32, Vec<String>> = HashMap::new();
        let mut triggers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
        let mut passive = HashSet::new();
        for row in &se.rows {
            let Some(id) = reached(int(row, e_spell)) else {
                continue;
            };
            if int(row, e_effect) == EFFECT_APPLY_AURA
                && int(row, e_aura) == AURA_OVERRIDE_ACTIONBAR
            {
                passive.insert(id);
            }
            if let Ok(t) = u32::try_from(int(row, e_trigger))
                && t != 0
                && t != id
            {
                triggers.entry(id).or_default().insert(t);
            }
            let (effect, aura) = (int(row, e_effect), int(row, e_aura));
            if effect == EFFECT_ATTACK_ME || aura == AURA_MOD_TAUNT {
                taunt.insert(id);
            }
            if effect == EFFECT_HEAL || aura == AURA_PERIODIC_HEAL {
                heal.insert(id);
            }
            if aura == AURA_MOD_SHAPESHIFT
                && let Some(form) = form_names.get(&int(row, e_misc))
            {
                forms.entry(id).or_default().push(form.clone());
            }
        }

        let eq = tables.get("SpellEquippedItems")?;
        let (q_spell, q_class, q_sub) = (
            eq.col("SpellID")?,
            eq.col("EquippedItemClass")?,
            eq.col("EquippedItemSubclass")?,
        );
        let mut shield = HashSet::new();
        for row in &eq.rows {
            if let Some(id) = reached(int(row, q_spell))
                && int(row, q_class) == ITEM_CLASS_ARMOR
                && int(row, q_sub) & SHIELD_SUBCLASS_BIT != 0
            {
                shield.insert(id);
            }
        }
        c.family = family;
        c.taunt = taunt;
        c.heal = heal;
        c.forms = forms;
        c.shield = shield;
        c.triggers = triggers;
        c.named = named;
        c.cast_in = cast_in;
        c.form_ids = form_ids;
        c.passive = passive;
        Ok(c)
    }

    /// Whether `id` is cast in one of `entry`'s forms (always, for an entry
    /// that names none).
    fn in_its_forms(&self, id: u32, entry: &Curated) -> bool {
        entry.in_form.is_empty()
            || self.cast_in.get(&id).is_some_and(|forms| {
                entry
                    .in_form
                    .iter()
                    .filter_map(|n| self.form_ids.get(*n))
                    .any(|f| forms.contains(f))
            })
    }

    /// `entry`'s ids and why each is in: the ones the kind's mark proves,
    /// then, for a taunt or a heal, every same-named id in the family the
    /// client ties to one already in (a trigger, else its text), to a
    /// fixpoint. Each pass reads the set as it began, in id order, so the
    /// answer is one.
    fn ids_of(&self, entry: &Curated) -> BTreeMap<u32, Proof> {
        let mut candidates: Vec<u32> = self
            .names
            .iter()
            .filter(|(id, name)| {
                name.as_str() == entry.name
                    && self.family.contains(id)
                    && self.in_its_forms(**id, entry)
            })
            .map(|(id, _)| *id)
            .collect();
        candidates.sort_unstable();
        let mut ids: BTreeMap<u32, Proof> = candidates
            .iter()
            .filter(|id| self.proves(**id, entry))
            .map(|id| (*id, Proof::Mark))
            .collect();
        if !matches!(entry.kind, RoleSignal::Taunt | RoleSignal::Heal) {
            return ids;
        }
        loop {
            let found: Vec<(u32, Proof)> = candidates
                .iter()
                .filter(|id| !ids.contains_key(id) && !self.passive.contains(id))
                .filter_map(|id| {
                    let hit = |set: Option<&BTreeSet<u32>>| {
                        set.and_then(|s| s.iter().find(|c| ids.contains_key(c)).copied())
                    };
                    hit(self.triggers.get(id))
                        .map(|c| (*id, Proof::Triggers(c)))
                        .or_else(|| hit(self.named.get(id)).map(|c| (*id, Proof::Names(c))))
                })
                .collect();
            if found.is_empty() {
                return ids;
            }
            ids.extend(found);
        }
    }

    /// Whether the client proves `id` of `entry`'s kind (the family and the
    /// kind's own mark; the name is already the entry's).
    fn proves(&self, id: u32, entry: &Curated) -> bool {
        self.family.contains(&id)
            && self.in_its_forms(id, entry)
            && match entry.kind {
                RoleSignal::Taunt => self.taunt.contains(&id),
                RoleSignal::Shield => self.shield.contains(&id),
                RoleSignal::Heal => self.heal.contains(&id),
                RoleSignal::Form => self
                    .forms
                    .get(&id)
                    .is_some_and(|f| f.iter().any(|n| n == entry.name)),
                RoleSignal::Mitigation => true,
            }
    }
}

/// Why an id is in its entry: the kind's own mark, or a tie to an id that
/// is in (one it triggers, or one its text names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proof {
    Mark,
    Triggers(u32),
    Names(u32),
}

impl Proof {
    /// The proof column of the review twin.
    fn text(self, entry: &Curated) -> String {
        match self {
            Proof::Mark if entry.in_form.is_empty() => proof(entry.kind).to_string(),
            Proof::Mark => format!("{}, in {}", proof(entry.kind), entry.in_form.join(" or ")),
            Proof::Triggers(c) => format!("triggers {c}"),
            Proof::Names(c) => format!("its text names {c}"),
        }
    }
}

/// The proof the review twin prints beside a kind's ids.
fn proof(kind: RoleSignal) -> &'static str {
    match kind {
        RoleSignal::Taunt => "ATTACK_ME or MOD_TAUNT",
        RoleSignal::Shield => "needs a shield",
        RoleSignal::Heal => "HEAL or PERIODIC_HEAL",
        RoleSignal::Form => "MOD_SHAPESHIFT, same form",
        RoleSignal::Mitigation => "class family",
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

    // Each entry's ids, sorted, with why each is in and the evidence printed
    // beside them.
    let mut entries: Vec<(&Curated, BTreeMap<u32, Proof>, Evidence<'_>)> = Vec::new();
    let mut table: BTreeMap<u32, RoleSignal> = BTreeMap::new();
    for entry in curated {
        let proofs = client.ids_of(entry);
        let ids: Vec<u32> = proofs.keys().copied().collect();
        if ids.is_empty() {
            return Err(format!(
                "role signal {:?} ({}): no id the client names so passes the class family \
                 and the {} proof",
                entry.name,
                entry.kind.name(),
                proof(entry.kind)
            ));
        }
        // The census: a row for one of these ids must agree on the name, and
        // one of them must have been cast, unless the entry is waived.
        let mut cast = false;
        for id in &ids {
            if let Some((log_name, per_log)) = census.counts.get(id) {
                if log_name != entry.name {
                    return Err(format!(
                        "role signal {id}: the census names it {log_name:?}, curated as {:?}",
                        entry.name
                    ));
                }
                cast |= per_log.iter().any(|&n| n > 0);
            }
        }
        let evidence = match (cast, entry.census_exempt) {
            (true, _) => Evidence::Counts,
            (false, Some(reason)) => Evidence::Exempt(reason),
            (false, None) => {
                return Err(format!(
                    "role signal {:?} ({}): no player cast any of {ids:?} in the census",
                    entry.name,
                    entry.kind.name()
                ));
            }
        };
        for id in &ids {
            if let Some(other) = table.insert(*id, entry.kind) {
                return Err(format!(
                    "role signal {id}: proven as {} and {}",
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
        entries: entries.len(),
        content: emit(&rows, build)?,
        expected: emit_expected(&entries, census, build)?,
    })
}

fn emit(table: &[(u32, u8)], build: &str) -> Result<String, String> {
    let mut o = String::new();
    let e = |e: std::fmt::Error| format!("emit: {e}");
    o.push_str("//! GENERATED by tools/gen-role-signals.sh — do not edit by hand.\n");
    // No timestamp: same build in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(e)?;
    writeln!(o, "//! {} role signals.", table.len()).map_err(e)?;
    o.push_str(
        "//!\n\
         //! Maps a combat-log spell id to what casting it (or, for a form, standing\n\
         //! in it) says about the player's role in a fight: a taunt, a tank's\n\
         //! mitigation, a cast that needs a shield, a tanking form or stance, or a\n\
         //! heal (CONTRACT.md R30). Membership is curated by name in\n\
         //! tools/extract/src/signalgen.rs; the client proves every id, and the\n\
         //! real-log census sits in role_signals.expected.md.\n\
         \n\
         use crate::roles::RoleSignal;\n\
         \n\
         /// What a spell says about a player's role, or `None` for one the\n\
         /// table does not curate.\n\
         pub(crate) fn signal(spell_id: u32) -> Option<RoleSignal> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, code) = TABLE.get(i)?;\n\
         \x20   RoleSignal::from_code(code)\n\
         }\n\
         \n\
         /// (spell id, `RoleSignal::code`), sorted by spell id.\n\
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
         \x20       assert!(super::TABLE.iter().all(|e| super::signal(e.0).is_some()));\n\
         \x20       assert_eq!(super::signal(0), None);\n\
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
    o.push_str("# role_signals.rs — the curated table and its evidence\n\n");
    o.push_str(
        "GENERATED by tools/gen-role-signals.sh beside `role_signals.rs` — do not edit by hand.\n",
    );
    writeln!(
        o,
        "Build {build}, {ids} role signals from {} curated names.",
        entries.len()
    )
    .map_err(e)?;
    o.push_str(
        "\n## Rules\n\n\
         - Membership is curated in `tools/extract/src/signalgen.rs` as `(name, kind)`; the\n\
         \x20 client expands each name to its ids. An id joins when `SpellName` carries the\n\
         \x20 name, `SpellClassOptions` puts it in a class's spell family, and the client\n\
         \x20 shows its kind's mark: a taunt an ATTACK_ME effect or a MOD_TAUNT aura, a shield\n\
         \x20 cast a `SpellEquippedItems` shield requirement, a form a MOD_SHAPESHIFT aura into\n\
         \x20 the form of the same name, a heal a HEAL effect or a PERIODIC_HEAL aura; a\n\
         \x20 mitigation cast the family alone, and a bear's own attack only where its\n\
         \x20 `SpellShapeshift` row requires Bear Form or Dire Bear Form (`in Bear Form or …`).\n\
         \x20 A name no id passes fails the build.\n\
         - A cast line writes the id the player cast, which for an older heal or taunt is\n\
         \x20 often a parent whose effect is a dummy or a trigger, the mark sitting on its\n\
         \x20 child. So a same-named id in the family also joins a taunt or a heal when the\n\
         \x20 client ties it to an id already in: an effect of it triggers that id\n\
         \x20 (`triggers N`), or its own description names it (`its text names N`), read to a\n\
         \x20 fixpoint; never a rune's passive (an id with the OVERRIDE_ACTIONBAR_SPELLS aura),\n\
         \x20 which carries the name and is never cast. A cast whose client data ties it to\n\
         \x20 no such child (Earth Shield's, Prayer of Mending's) is not in: only its heal's\n\
         \x20 id is, which a cast line never writes, so those casts count as no heal.\n\
         - The committed census `tools/role-signals-census.csv` (`tools/census-role-signals.sh\n\
         \x20 <log>...`: every `SPELL_CAST_SUCCESS` a `Player-` guid wrote, counted per (id,\n\
         \x20 name) per log) must show a player casting one of a name's ids, under that name,\n\
         \x20 unless the entry carries a `census_exempt` reason, printed below in place of the\n\
         \x20 counts (`exempt: …`).\n\
         - What a WAIVED entry's ids are known to be: each carries the name, the class family\n\
         \x20 and the kind's mark, or a tie the client records to an id that does. What they\n\
         \x20 are NOT known to be: the id a cast line writes. No committed log holds a player\n\
         \x20 casting one, so a cast the client ties to nothing would be missed, and the first\n\
         \x20 census to hold one settles it.\n\
         - The meter (CONTRACT.md R30) counts a player's casts of each kind and, for a form,\n\
         \x20 the time its aura is on them, in each fight alone; the role solver weighs them\n\
         \x20 with the fight's swings, healing and damage.\n\
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
    for kind in RoleSignal::ALL {
        for (entry, ids, evidence) in entries {
            if entry.kind != kind {
                continue;
            }
            for (id, why) in ids {
                let mut line = format!(
                    "| {id} | {} | {} | {} |",
                    entry.name,
                    kind.name(),
                    why.text(entry)
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

    fn tables() -> Tables {
        let mut t = Tables::default();
        // Two Taunt ids (a player's, and a creature's with no family), two
        // Earth Shock ranks (one taunts), a Shield Block, a Healing Wave
        // rank pair, Bear Form and an NPC's Bear Form namesake, Revenge.
        t.insert(
            "SpellName",
            parse_csv(
                "ID,Name_lang\n\
                 355,Taunt\n\
                 29060,Taunt\n\
                 8042,Earth Shock\n\
                 1219379,Earth Shock\n\
                 2565,Shield Block\n\
                 331,Healing Wave\n\
                 332,Healing Wave\n\
                 5487,Bear Form\n\
                 7090,Bear Form\n\
                 6572,Revenge\n\
                 71,Defensive Stance\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellClassOptions",
            parse_csv(
                "ID,SpellID,SpellClassSet\n\
                 1,355,4\n\
                 2,8042,11\n\
                 3,1219379,11\n\
                 4,2565,4\n\
                 5,331,11\n\
                 6,332,11\n\
                 7,5487,7\n\
                 8,6572,4\n\
                 9,71,4\n\
                 10,29060,0\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,Effect,EffectAura,EffectMiscValue_0,EffectTriggerSpell,SpellID\n\
                 1,114,0,0,0,355\n\
                 2,6,11,0,0,355\n\
                 3,114,0,0,0,29060\n\
                 4,2,0,0,0,8042\n\
                 5,114,0,0,0,1219379\n\
                 6,6,0,0,0,2565\n\
                 7,10,0,0,0,331\n\
                 8,10,0,0,0,332\n\
                 9,6,36,5,0,5487\n\
                 10,6,36,5,0,7090\n\
                 11,2,0,0,0,6572\n\
                 12,6,36,18,0,71\n",
            )
            .unwrap(),
        );
        // No text names anything here; `the_cast_a_heal_is_written_under` adds some.
        t.insert(
            "Spell",
            parse_csv("ID,NameSubtext_lang,Description_lang,AuraDescription_lang\n331,,Heals.,\n")
                .unwrap(),
        );
        t.insert(
            "SpellEquippedItems",
            parse_csv(
                "ID,SpellID,EquippedItemClass,EquippedItemInvTypes,EquippedItemSubclass\n\
                 1,2565,4,0,64\n\
                 2,6572,2,0,173555\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellShapeshiftForm",
            parse_csv("ID,Name_lang\n5,Bear Form\n18,Defensive Stance\n").unwrap(),
        );
        t.insert(
            "SpellShapeshift",
            parse_csv("ID,SpellID,ShapeshiftMask_0,ShapeshiftMask_1\n1,6572,131072,0\n").unwrap(),
        );
        t
    }

    fn census() -> Census {
        Census::parse(
            "id,name,log 1,log 2\n\
             355,\"Taunt\",33,67\n\
             331,\"Healing Wave\",114,0\n\
             2565,\"Shield Block\",82,277\n\
             6572,\"Revenge\",0,3\n\
             71,\"Defensive Stance\",0,22\n",
        )
        .unwrap()
    }

    const GOOD: &[Curated] = &[
        Curated::seen("Taunt", RoleSignal::Taunt),
        Curated::exempt("Earth Shock", RoleSignal::Taunt, "a waiver"),
        Curated::seen("Shield Block", RoleSignal::Shield),
        Curated::seen("Revenge", RoleSignal::Mitigation),
        Curated::seen("Defensive Stance", RoleSignal::Form),
        Curated::exempt("Bear Form", RoleSignal::Form, "a waiver"),
        Curated::seen("Healing Wave", RoleSignal::Heal),
    ];

    #[test]
    fn names_expand_to_the_ids_the_client_proves() {
        let g = generate_curated(GOOD, &tables(), &census(), "1.60.1.1").unwrap();
        assert_eq!(g.entries, 7);
        // 29060 (no family), 8042 (no taunt) and 7090 (no family) are out.
        assert!(
            g.content.contains(
                "(71,3), (331,4), (332,4), (355,0), (2565,2), (5487,3), (6572,1), (1219379,0),"
            ),
            "{}",
            g.content
        );
        assert_eq!(g.spells, 8);
        assert!(g.content.contains("//! 8 role signals."));
        assert!(g.content.contains("build 1.60.1.1."));
        assert!(
            g.expected
                .contains("| 355 | Taunt | taunt | ATTACK_ME or MOD_TAUNT | 33 | 67 |"),
            "{}",
            g.expected
        );
        // A rank the census never saw prints zeroes beside a cast sibling.
        assert!(
            g.expected
                .contains("| 332 | Healing Wave | heal | HEAL or PERIODIC_HEAL | 0 | 0 |")
        );
        assert!(
            g.expected.contains(
                "| 1219379 | Earth Shock | taunt | ATTACK_ME or MOD_TAUNT | exempt: a waiver | — |"
            ),
            "{}",
            g.expected
        );
        // Kind order in the review twin: taunts before heals.
        assert!(g.expected.find("| 355 |").unwrap() < g.expected.find("| 331 |").unwrap());
    }

    /// The id a cast line writes joins a heal through the client's own ties:
    /// Holy Shock's rank (a dummy) whose text names its heal, Penance's cast
    /// whose text names its heal and the channel that triggers it, the
    /// channel itself, and a second rank tied only to the first. An id tied
    /// to nothing proven (Prayer of Mending's dummy cast), one of another
    /// name naming a proven id, a tie outside the class family, and a
    /// shield cast whose text names a heal stay out.
    #[test]
    fn the_cast_a_heal_is_written_under_joins_through_the_clients_ties() {
        let mut t = Tables::default();
        t.insert(
            "SpellName",
            parse_csv(
                "ID,Name_lang\n\
                 20473,Holy Shock\n25914,Holy Shock\n25912,Holy Shock\n20930,Holy Shock\n\
                 30000,Holy Shock\n\
                 402174,Penance\n402277,Penance\n402289,Penance\n402284,Penance\n\
                 401859,Prayer of Mending\n401863,Prayer of Mending\n\
                 2050,Lesser Heal\n2565,Shield Block\n402844,Penance\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellClassOptions",
            parse_csv(
                "ID,SpellID,SpellClassSet\n\
                 1,20473,10\n2,25914,10\n3,25912,10\n4,20930,10\n5,30000,0\n\
                 6,402174,6\n7,402277,6\n8,402289,6\n9,402284,6\n\
                 10,401859,6\n11,401863,6\n12,2050,6\n13,2565,4\n14,402844,6\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,Effect,EffectAura,EffectMiscValue_0,EffectTriggerSpell,SpellID\n\
                 1,3,0,0,0,20473\n2,10,0,0,0,25914\n3,2,0,0,0,25912\n4,3,0,0,0,20930\n\
                 5,3,0,0,0,30000\n\
                 6,3,0,0,0,402174\n7,6,23,0,402289,402277\n8,10,0,0,0,402289\n\
                 9,2,0,0,0,402284\n\
                 10,3,0,0,0,401859\n11,10,0,0,0,401863\n\
                 12,10,0,0,0,2050\n13,6,0,0,0,2565\n14,6,332,0,0,402844\n15,6,107,0,0,402844\n",
            )
            .unwrap(),
        );
        t.insert(
            "Spell",
            parse_csv(
                "ID,NameSubtext_lang,Description_lang,AuraDescription_lang\n\
                 20473,,\"Causing $25912s1 damage, or $25914s1 healing.\",\n\
                 20930,,Like $20473s1.,\n\
                 30000,,Heals for $25914s1.,\n\
                 402174,,\"$402284s1 damage, or $402289s1 healing, every $402261t2 sec.\",\n\
                 401859,,Jumps within $401880a1 yards.,\n\
                 2050,,Like $25914s1.,\n\
                 2565,,Then $2050s1.,\n\
                 402844,,Engraves $402174s1.,\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellEquippedItems",
            parse_csv(
                "ID,SpellID,EquippedItemClass,EquippedItemInvTypes,EquippedItemSubclass\n\
                 1,2565,4,0,64\n",
            )
            .unwrap(),
        );
        t.insert("SpellShapeshiftForm", parse_csv("ID,Name_lang\n").unwrap());
        t.insert(
            "SpellShapeshift",
            parse_csv("ID,SpellID,ShapeshiftMask_0,ShapeshiftMask_1\n").unwrap(),
        );
        let list = [
            Curated::exempt("Holy Shock", RoleSignal::Heal, "w"),
            Curated::exempt("Penance", RoleSignal::Heal, "w"),
            Curated::exempt("Prayer of Mending", RoleSignal::Heal, "w"),
            Curated::exempt("Lesser Heal", RoleSignal::Heal, "w"),
            Curated::exempt("Shield Block", RoleSignal::Shield, "w"),
        ];
        let census = Census::parse("id,name,log 1\n1,\"x\",0\n").unwrap();
        let g = generate_curated(&list, &t, &census, "b").unwrap();
        let heals: Vec<u32> = [2050, 20473, 20930, 25914, 401863, 402174, 402277, 402289].into();
        for id in &heals {
            assert!(
                g.content.contains(&format!("({id},4)")),
                "{id}: {}",
                g.content
            );
        }
        // The damage children, Prayer of Mending's dummy cast, the namesake
        // outside the family: none is in; the shield cast is a shield.
        // The rune's passive that names the cast is out too.
        for id in [25912, 402284, 401859, 30000, 402844] {
            assert!(
                !g.content.contains(&format!("({id},")),
                "{id}: {}",
                g.content
            );
        }
        assert!(g.content.contains("(2565,2)"), "{}", g.content);
        assert_eq!(g.spells, heals.len() + 1);
        for line in [
            "| 20473 | Holy Shock | heal | its text names 25914 |",
            "| 20930 | Holy Shock | heal | its text names 20473 |",
            "| 25914 | Holy Shock | heal | HEAL or PERIODIC_HEAL |",
            "| 402174 | Penance | heal | its text names 402289 |",
            "| 402277 | Penance | heal | triggers 402289 |",
            "| 2565 | Shield Block | shield | needs a shield |",
        ] {
            assert!(g.expected.contains(line), "{line}\n{}", g.expected);
        }
        // Read again (another hash order inside), the same bytes.
        let again = generate_curated(&list, &t, &census, "b").unwrap();
        assert_eq!((g.content, g.expected), (again.content, again.expected));
    }

    /// A bear's attack joins only where its shapeshift row requires Bear
    /// Form or Dire Bear Form: the bear's Swipe (mask 144: forms 5 and 8) is
    /// in; the cat's (mask 1), a creature bear's (form 14) and a pet's (no
    /// row) are out, and all four (each in a class's family) are in without
    /// it.
    #[test]
    fn a_bear_attack_is_held_to_bear_form() {
        let mut t = Tables::default();
        t.insert(
            "SpellName",
            parse_csv("ID,Name_lang\n780,Swipe\n411128,Swipe\n27554,Swipe\n1264494,Swipe\n")
                .unwrap(),
        );
        t.insert(
            "SpellClassOptions",
            parse_csv("ID,SpellID,SpellClassSet\n1,780,7\n2,411128,7\n3,27554,7\n4,1264494,9\n")
                .unwrap(),
        );
        t.insert(
            "SpellShapeshift",
            parse_csv(
                "ID,SpellID,ShapeshiftMask_0,ShapeshiftMask_1\n\
                 1,780,144,0\n2,411128,1,0\n3,27554,8192,0\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellShapeshiftForm",
            parse_csv(
                "ID,Name_lang\n1,Cat Form\n5,Bear Form\n8,Dire Bear Form\n14,Creature - Bear\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellEffect",
            parse_csv("ID,Effect,EffectAura,EffectMiscValue_0,EffectTriggerSpell,SpellID\n")
                .unwrap(),
        );
        t.insert(
            "Spell",
            parse_csv("ID,NameSubtext_lang,Description_lang,AuraDescription_lang\n").unwrap(),
        );
        t.insert(
            "SpellEquippedItems",
            parse_csv("ID,SpellID,EquippedItemClass,EquippedItemInvTypes,EquippedItemSubclass\n")
                .unwrap(),
        );
        let census = Census::parse("id,name,log 1\n780,\"Swipe\",3\n").unwrap();
        let bear = [Curated::seen("Swipe", RoleSignal::Mitigation).in_form(BEAR)];
        let g = generate_curated(&bear, &t, &census, "b").unwrap();
        assert_eq!(g.spells, 1);
        assert!(g.content.contains("(780,1),"), "{}", g.content);
        let line =
            "| 780 | Swipe | mitigation | class family, in Bear Form or Dire Bear Form | 3 |";
        assert!(g.expected.contains(line), "{}", g.expected);
        // Hunter pets share family 9, a hunter's: in the family, so in too.
        let any = [Curated::seen("Swipe", RoleSignal::Mitigation)];
        assert_eq!(generate_curated(&any, &t, &census, "b").unwrap().spells, 4);
    }

    #[test]
    fn a_name_no_id_proves_fails_naming_it() {
        // Revenge as a shield cast: its equipment row is a weapon's.
        let list = [Curated::seen("Revenge", RoleSignal::Shield)];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(
            err.contains("Revenge") && err.contains("needs a shield"),
            "{err}"
        );
        // Healing Wave as a form; a name the client lacks.
        let list = [Curated::seen("Healing Wave", RoleSignal::Form)];
        assert!(generate_curated(&list, &tables(), &census(), "b").is_err());
        let list = [Curated::exempt("Nothing", RoleSignal::Heal, "w")];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("Nothing"), "{err}");
    }

    #[test]
    fn the_census_must_show_a_cast_or_a_waiver() {
        let list = [Curated::seen("Bear Form", RoleSignal::Form)];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("Bear Form") && err.contains("census"), "{err}");
        let renamed = Census::parse("id,name,log 1\n355,\"Provoke\",3\n").unwrap();
        let list = [Curated::seen("Taunt", RoleSignal::Taunt)];
        let err = generate_curated(&list, &tables(), &renamed, "b").unwrap_err();
        assert!(err.contains("355") && err.contains("Provoke"), "{err}");
        let zero = Census::parse("id,name,log 1\n355,\"Taunt\",0\n").unwrap();
        assert!(generate_curated(&list, &tables(), &zero, "b").is_err());
    }

    #[test]
    fn duplicates_fail_and_output_is_deterministic() {
        let list = [
            Curated::seen("Taunt", RoleSignal::Taunt),
            Curated::seen("Taunt", RoleSignal::Mitigation),
        ];
        let err = generate_curated(&list, &tables(), &census(), "b").unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
        let a = generate_curated(GOOD, &tables(), &census(), "b").unwrap();
        let b = generate_curated(GOOD, &tables(), &census(), "b").unwrap();
        assert_eq!((a.content, a.expected), (b.content, b.expected));
    }

    /// The real list is what ships: unique names, a reason on every waiver,
    /// and each kind's code its place in `RoleSignal::ALL`.
    #[test]
    fn curated_list_is_well_formed() {
        let names: HashSet<&str> = CURATED.iter().map(|e| e.name).collect();
        assert_eq!(names.len(), CURATED.len());
        assert!(
            CURATED
                .iter()
                .all(|e| e.census_exempt.is_none_or(|r| !r.is_empty()))
        );
        for (i, k) in RoleSignal::ALL.iter().enumerate() {
            assert_eq!(usize::from(k.code()), i);
            assert_eq!(RoleSignal::from_code(k.code()), Some(*k));
        }
        for kind in RoleSignal::ALL {
            assert!(CURATED.iter().any(|e| e.kind == kind), "{kind:?}");
        }
    }
}
