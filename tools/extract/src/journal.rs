//! The Encounter Journal's sections and the spell tables as the journal's
//! words read them: the section tree (`sections`, a `Section` a row of
//! `JournalEncounterSection`), every spell's name (`spell_names`), and the
//! spell tables by spell id with every difficulty's rows kept (`Spells`),
//! read through the client's own fallback chain (`Difficulty`'s
//! `FallbackDifficultyID`). gen-spell-tips takes the journal's words for
//! spells from here (`journal_texts`, `spelltipgen`); the replay's
//! gen-rubric drafts an encounter's abilities, their text and its
//! `[spell]` table from the same reading (`rubricgen`, in
//! wowdps-replay-extract).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::spelltip;
use crate::table::{Csv, Tables, int, text};

/// What a journal section is (`JournalEncounterSection.Type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionType {
    /// A header (0): a stage at the tree's root, else a group of creatures.
    Header,
    /// A creature (1).
    Creature,
    /// An ability (2), by its spell.
    Spell,
    /// Anything else (an overview).
    Other,
}

impl SectionType {
    fn of(value: i64) -> SectionType {
        match value {
            0 => SectionType::Header,
            1 => SectionType::Creature,
            2 => SectionType::Spell,
            _ => SectionType::Other,
        }
    }
}

/// A row of `JournalEncounterSection`, as the tree reads it: what it is
/// (`is_stage`, `is_creature`, `is_ability`), where it hangs, its spell
/// and its words.
pub struct Section {
    pub id: i64,
    pub parent: i64,
    kind: SectionType,
    pub order: i64,
    pub spell: u32,
    pub icon_flags: u32,
    pub title: String,
    /// The words it adds to its spell's description.
    pub body: String,
    /// The difficulty ids it is limited to; `None`: every one.
    pub own: Option<BTreeSet<u32>>,
}

impl Section {
    /// A stage: a header at the tree's root.
    pub fn is_stage(&self) -> bool {
        self.kind == SectionType::Header && self.parent == 0
    }

    /// A creature, or a header nested in a stage, which names a group of
    /// creatures as a creature header does.
    pub fn is_creature(&self) -> bool {
        self.kind == SectionType::Creature || self.kind == SectionType::Header && self.parent != 0
    }

    /// An ability: a spell section that names its spell.
    pub fn is_ability(&self) -> bool {
        self.kind == SectionType::Spell && self.spell != 0
    }
}

/// The section tree's sections by JournalEncounter, in table order. A
/// section is limited to difficulties when its `Flags` has 0x2: then its
/// `JournalSectionXDifficulty` rows are the set.
pub fn sections(tables: &Tables) -> Result<HashMap<i64, Vec<Section>>, String> {
    let jx = tables.get("JournalSectionXDifficulty")?;
    let (d, s) = (
        jx.col("DifficultyID")?,
        jx.col("JournalEncounterSectionID")?,
    );
    let mut difficulties: HashMap<i64, BTreeSet<u32>> = HashMap::new();
    for r in &jx.rows {
        difficulties
            .entry(int(r, s))
            .or_default()
            .insert(int(r, d) as u32);
    }
    let js = tables.get("JournalEncounterSection")?;
    let c = |n: &str| js.col(n);
    let (id, enc, parent, kind, order, spell, icon, flags, title, body) = (
        c("ID")?,
        c("JournalEncounterID")?,
        c("ParentSectionID")?,
        c("Type")?,
        c("OrderIndex")?,
        c("SpellID")?,
        c("IconFlags")?,
        c("Flags")?,
        c("Title_lang")?,
        c("BodyText_lang")?,
    );
    let mut out: HashMap<i64, Vec<Section>> = HashMap::new();
    for r in &js.rows {
        let sid = int(r, id);
        let kind = SectionType::of(int(r, kind));
        let limited = int(r, flags) & 0x2 != 0;
        let rows = difficulties.get(&sid).cloned();
        // A header limited to nothing (King Dazar's "Heroic and Mythic
        // Difficulty Only", whose abilities are real) limits nothing; a
        // spell copy limited to nothing is shown on no difficulty.
        let own = match (limited, rows, kind) {
            (false, _, _) => None,
            (true, None, SectionType::Header) => None,
            (true, rows, _) => Some(rows.unwrap_or_default()),
        };
        out.entry(int(r, enc)).or_default().push(Section {
            id: sid,
            parent: int(r, parent),
            kind,
            order: int(r, order),
            spell: int(r, spell) as u32,
            icon_flags: int(r, icon) as u32,
            title: text(r, title).to_string(),
            body: text(r, body).to_string(),
            own,
        });
    }
    Ok(out)
}

/// Every spell's name (`SpellName`).
pub fn spell_names(tables: &Tables) -> Result<HashMap<u32, &str>, String> {
    let sn = tables.get("SpellName")?;
    let (id, name) = (sn.col("ID")?, sn.col("Name_lang")?);
    Ok(sn
        .rows
        .iter()
        .map(|r| (int(r, id) as u32, text(r, name)))
        .collect())
}

/// One SpellEffect row, as the facts and the text's numbers read it.
#[derive(Debug, Clone, Copy)]
struct EffectRow {
    difficulty: u32,
    index: u32,
    points: f64,
    /// Ms between an aura's ticks.
    period: u32,
    /// `EffectRadiusIndex_0` and `_1`.
    radius: (u32, u32),
    trigger: u32,
}

/// SpellEffect's columns, an `EffectRow`'s cells.
struct EffectCells {
    difficulty: usize,
    index: usize,
    points: usize,
    period: usize,
    radius: (usize, usize),
    trigger: usize,
}

impl EffectCells {
    fn read(&self, r: &[String]) -> EffectRow {
        EffectRow {
            difficulty: int(r, self.difficulty) as u32,
            index: int(r, self.index) as u32,
            points: text(r, self.points).parse().unwrap_or(0.0),
            period: int(r, self.period) as u32,
            radius: (int(r, self.radius.0) as u32, int(r, self.radius.1) as u32),
            trigger: int(r, self.trigger) as u32,
        }
    }
}

/// SpellMisc's columns: DifficultyID and DurationIndex.
struct MiscCells {
    difficulty: usize,
    duration: usize,
}

/// SpellAuraOptions as a text reads it: proc chance, most stacks, proc
/// cooldown (ms).
type AuraOptions = (f64, u32, u32);

/// SpellAuraOptions' columns: DifficultyID, then an `AuraOptions`' cells.
struct AuraCells {
    difficulty: usize,
    chance: usize,
    stacks: usize,
    cooldown: usize,
}

impl AuraCells {
    fn read(&self, r: &[String]) -> AuraOptions {
        (
            text(r, self.chance).parse().unwrap_or(0.0),
            int(r, self.stacks) as u32,
            int(r, self.cooldown) as u32,
        )
    }
}

/// A spell's rows as one difficulty reads them.
struct Rows {
    /// Ascending by index.
    effects: Vec<EffectRow>,
    /// Ms; -1 lasts until it is taken away.
    duration: Option<i64>,
    aura: Option<AuraOptions>,
}

/// A big spell table's rows by the spell they are of: each row's spell id
/// beside its place, sorted, so a spell's rows are one run a binary search
/// finds, in table order. Only the spell id is read up front, the rest of
/// a row when its spell is asked for: an encounter's few thousand spells
/// of the hundreds of thousands the table holds.
struct BySpell<'a> {
    csv: &'a Csv,
    rows: Vec<(u32, u32)>,
}

impl<'a> BySpell<'a> {
    fn new(csv: &'a Csv, spell: usize) -> BySpell<'a> {
        let mut rows: Vec<(u32, u32)> = csv
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| (int(r, spell) as u32, i as u32))
            .collect();
        rows.sort_unstable();
        BySpell { csv, rows }
    }

    /// A spell's rows, in table order.
    fn of(&self, spell: u32) -> impl Iterator<Item = &'a [String]> + '_ {
        let start = self.rows.partition_point(|&(s, _)| s < spell);
        self.rows
            .iter()
            .skip(start)
            .take_while(move |&&(s, _)| s == spell)
            .filter_map(|&(_, i)| self.csv.rows.get(i as usize).map(Vec::as_slice))
    }
}

/// What the spell tables say of a spell (`Spells::facts`), as one
/// difficulty's chain reads its rows: its name, its largest effect radius
/// (yards), how long an aura of it lasts and the seconds between its ticks,
/// the most stacks it holds, and the spells its effects set off. Plain
/// facts, no format's: gen-rubric (wowdps-replay-extract's `rubricgen`)
/// writes them as the encounter rubric's `[spell]` entries.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellFacts {
    pub name: String,
    /// Yards: its largest effect radius.
    pub radius: Option<f32>,
    /// Seconds an aura of it lasts; none for an instant one, or one that
    /// lasts until it is taken away.
    pub duration: Option<f32>,
    /// Seconds between its periodic effect's ticks.
    pub period: Option<f32>,
    /// The most stacks an aura of it holds.
    pub stacks: Option<u32>,
    /// The spells its effects set off.
    pub triggers: Vec<u32>,
}

/// The spell tables by spell id, every difficulty's rows kept.
pub struct Spells<'a> {
    names: &'a HashMap<u32, &'a str>,
    pub descs: HashMap<u32, String>,
    /// Each spell's description variables, unparsed: the table is read
    /// once here, and each text's spells picked from it (`ctx`).
    vars: HashMap<u32, &'a str>,
    effects: BySpell<'a>,
    effect_cells: EffectCells,
    misc: BySpell<'a>,
    misc_cells: MiscCells,
    aura: BySpell<'a>,
    aura_cells: AuraCells,
    /// SpellRadius → yards: `Radius`, else `RadiusMax`.
    radius: HashMap<u32, f64>,
    /// SpellDuration → ms.
    duration: HashMap<u32, i64>,
    /// Difficulty → `FallbackDifficultyID`.
    fallback: HashMap<u32, u32>,
}

impl<'a> Spells<'a> {
    pub fn new(tables: &'a Tables, names: &'a HashMap<u32, &'a str>) -> Result<Self, String> {
        let se = tables.get("SpellEffect")?;
        let effect_cells = EffectCells {
            difficulty: se.col("DifficultyID")?,
            index: se.col("EffectIndex")?,
            points: spelltip::col2(se, "EffectBasePointsF", "EffectBasePoints")?,
            period: se.col("EffectAuraPeriod")?,
            radius: (
                se.col("EffectRadiusIndex_0")?,
                se.col("EffectRadiusIndex_1")?,
            ),
            trigger: se.col("EffectTriggerSpell")?,
        };
        let sm = tables.get("SpellMisc")?;
        let misc_cells = MiscCells {
            difficulty: sm.col("DifficultyID")?,
            duration: sm.col("DurationIndex")?,
        };
        let sa = tables.get("SpellAuraOptions")?;
        let aura_cells = AuraCells {
            difficulty: sa.col("DifficultyID")?,
            chance: sa.col("ProcChance")?,
            stacks: sa.col("CumulativeAura")?,
            cooldown: sa.col("ProcCategoryRecovery")?,
        };
        let sr = tables.get("SpellRadius")?;
        let (id, yd, max) = (sr.col("ID")?, sr.col("Radius")?, sr.col("RadiusMax")?);
        let radius = sr
            .rows
            .iter()
            .map(|r| {
                let v: f64 = text(r, yd).parse().unwrap_or(0.0);
                let v = if v > 0.0 {
                    v
                } else {
                    text(r, max).parse().unwrap_or(0.0)
                };
                (int(r, id) as u32, v)
            })
            .collect();
        let sd = tables.get("SpellDuration")?;
        let (id, ms) = (sd.col("ID")?, sd.col("Duration")?);
        let duration = sd
            .rows
            .iter()
            .map(|r| (int(r, id) as u32, int(r, ms)))
            .collect();
        let df = tables.get("Difficulty")?;
        let (id, fb) = (df.col("ID")?, df.col("FallbackDifficultyID")?);
        let fallback = df
            .rows
            .iter()
            .map(|r| (int(r, id) as u32, int(r, fb) as u32))
            .collect();
        Ok(Spells {
            names,
            descs: spelltip::descriptions(tables)?,
            vars: spelltip::variable_texts(tables)?,
            effects: BySpell::new(se, se.col("SpellID")?),
            effect_cells,
            misc: BySpell::new(sm, sm.col("SpellID")?),
            misc_cells,
            aura: BySpell::new(sa, sa.col("SpellID")?),
            aura_cells,
            radius,
            duration,
            fallback,
        })
    }

    /// The difficulty ids a difficulty's rows are read through: it, the
    /// ones it falls back to, then the base rows (0).
    pub fn chain(&self, difficulty: u32) -> Vec<u32> {
        let mut out = vec![difficulty];
        let mut at = difficulty;
        while let Some(&f) = self.fallback.get(&at) {
            if f == 0 || out.contains(&f) {
                break;
            }
            out.push(f);
            at = f;
        }
        if difficulty != 0 {
            out.push(0);
        }
        out
    }

    /// A spell's rows as `chain` reads them: each effect index, its misc
    /// and its aura options from the first difficulty of the chain that
    /// has one.
    fn rows(&self, spell: u32, chain: &[u32]) -> Rows {
        let rank = |d: u32| chain.iter().position(|c| *c == d);
        let mut effects: BTreeMap<u32, (usize, EffectRow)> = BTreeMap::new();
        for r in self.effects.of(spell) {
            let e = self.effect_cells.read(r);
            let Some(r) = rank(e.difficulty) else {
                continue;
            };
            if effects.get(&e.index).is_none_or(|(best, _)| r < *best) {
                effects.insert(e.index, (r, e));
            }
        }
        let m = &self.misc_cells;
        let duration = self
            .misc
            .of(spell)
            .filter_map(|r| {
                Some((
                    rank(int(r, m.difficulty) as u32)?,
                    int(r, m.duration) as u32,
                ))
            })
            .min_by_key(|(r, _)| *r)
            .and_then(|(_, ix)| self.duration.get(&ix).copied());
        let a = &self.aura_cells;
        let aura = self
            .aura
            .of(spell)
            .filter_map(|r| Some((rank(int(r, a.difficulty) as u32)?, a.read(r))))
            .min_by_key(|(r, _)| *r)
            .map(|(_, a)| a);
        Rows {
            effects: effects.into_values().map(|(_, e)| e).collect(),
            duration,
            aura,
        }
    }

    fn yards(&self, index: u32) -> f64 {
        self.radius.get(&index).copied().unwrap_or(0.0)
    }

    /// The spells a spell's effects set off, as `chain` reads them.
    pub fn triggers(&self, spell: u32, chain: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        for e in self.rows(spell, chain).effects {
            if e.trigger != 0 && !out.contains(&e.trigger) {
                out.push(e.trigger);
            }
        }
        out
    }

    /// What the tables say of a spell, as `chain` reads them; none for a
    /// spell with no name.
    pub fn facts(&self, spell: u32, chain: &[u32]) -> Option<SpellFacts> {
        let name = self.names.get(&spell).filter(|n| !n.is_empty())?;
        let rows = self.rows(spell, chain);
        let radius = rows
            .effects
            .iter()
            .map(|e| self.yards(e.radius.0).max(self.yards(e.radius.1)))
            .fold(0.0, f64::max);
        Some(SpellFacts {
            name: name.to_string(),
            radius: (radius > 0.0).then_some(radius as f32),
            duration: rows
                .duration
                .filter(|ms| *ms > 0)
                .map(|ms| ms as f32 / 1000.0),
            period: rows
                .effects
                .iter()
                .map(|e| e.period)
                .find(|p| *p > 0)
                .map(|p| p as f32 / 1000.0),
            stacks: rows.aura.map(|(_, s, _)| s).filter(|s| *s > 0),
            triggers: self.triggers(spell, chain),
        })
    }

    /// The spell tables a text is read against at `chain`, for the spells
    /// in `needed`.
    pub fn ctx(&self, needed: &HashSet<u32>, chain: &[u32]) -> spelltip::Ctx {
        let mut ctx = spelltip::Ctx {
            effects: HashMap::new(),
            durations: HashMap::new(),
            names: HashMap::new(),
            descs: HashMap::new(),
            vars: spelltip::variables_of(&self.vars, needed),
            aura: HashMap::new(),
        };
        for &id in needed {
            let rows = self.rows(id, chain);
            if !rows.effects.is_empty() {
                // `$a` reads the first radius an effect has.
                let radius = |e: &EffectRow| match e.radius {
                    (0, ix) | (ix, _) => self.yards(ix),
                };
                let list = rows
                    .effects
                    .iter()
                    .map(|e| (e.index, e.points, e.period, radius(e)))
                    .collect();
                ctx.effects.insert(id, list);
            }
            if let Some(ms) = rows.duration {
                ctx.durations.insert(id, ms);
            }
            if let Some(a) = rows.aura {
                ctx.aura.insert(id, a);
            }
            if let Some(n) = self.names.get(&id) {
                ctx.names.insert(id, n.to_string());
            }
            if let Some(d) = self.descs.get(&id) {
                ctx.descs.insert(id, d.clone());
            }
        }
        ctx
    }
}

/// A journal listing's text: its spell's description, then what the
/// listing adds. A `$[…$]` block brings its own break; plain words run on.
pub fn join(desc: &str, body: &str) -> String {
    if desc.is_empty()
        || body.is_empty()
        || body.starts_with("$[")
        || desc.ends_with(char::is_whitespace)
    {
        format!("{desc}{body}")
    } else {
        format!("{desc} {body}")
    }
}

/// The tables `journal_texts` reads, beside the spell tables its texts'
/// tokens read (gen-spell-tips names them all).
pub const JOURNAL_TABLES: [&str; 3] = [
    "JournalEncounterSection",
    "JournalSectionXDifficulty",
    "Difficulty",
];

/// The journal's words for spells, for gen-spell-tips (`spelltipgen`):
/// for each of `wanted` an ability section of the journal names (`Type`
/// 2, its `SpellID`), its text as gen-rubric reads an ability's (its
/// spell's description, then what the section adds: `join`), its tokens
/// filled in from the base rows and no difficulty's branch taken
/// (`spelltip::journal` at 0). Of the sections naming one spell, the first
/// by section id that says anything gives it. Blizzard's words: the
/// caller keeps them on the machine.
pub(crate) fn journal_texts(
    tables: &Tables,
    wanted: &HashSet<u32>,
) -> Result<HashMap<u32, String>, String> {
    let names = spell_names(tables)?;
    let sp = Spells::new(tables, &names)?;
    let mut listed: Vec<Section> = sections(tables)?
        .into_values()
        .flatten()
        .filter(|s| s.is_ability() && wanted.contains(&s.spell))
        .collect();
    listed.sort_by_key(|s| s.id);
    let base = sp.chain(0);
    let mut out: HashMap<u32, String> = HashMap::new();
    for s in listed {
        if out.contains_key(&s.spell) {
            continue;
        }
        let desc = sp.descs.get(&s.spell).map_or("", String::as_str);
        let raw = join(desc, &s.body);
        // The spells its tokens touch, through the descriptions they embed
        // (`needed_spells`, for one text).
        let mut needed: HashSet<u32> = HashSet::from([s.spell]);
        let mut level: Vec<&str> = vec![raw.as_str()];
        for _ in 0..4 {
            let mut found = HashSet::new();
            for text in &level {
                spelltip::ids_in(text, &mut found);
            }
            level = found
                .iter()
                .filter(|id| !needed.contains(id))
                .filter_map(|id| sp.descs.get(id).map(String::as_str))
                .collect();
            needed.extend(found);
        }
        let read = spelltip::journal(&raw, s.spell, &sp.ctx(&needed, &base), 0);
        if !read.is_empty() {
            out.insert(s.spell, read);
        }
    }
    Ok(out)
}
