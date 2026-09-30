//! The `crates/core/src/proc_spells.rs` generator: a talent proc's spell id
//! → the spell that drives it (CONTRACT.md R26), so the ability tree hangs
//! Blackened Soul under Wither and Expurgation under Blade of Justice.
//!
//! The client does not link a proc to its driver in data a generator can
//! read: the proc hangs off the talent's aura, whose trigger conditions are
//! proc flags and script, not a class mask pointing at the driver (the
//! masks of these talents select nothing). So, like `rolegen`, this table
//! is CURATED — `(proc id, proc name, driver id, driver name, spec)` — and
//! every entry must be proven twice, once from each side:
//!
//! - the install: `SpellName` must carry both names exactly, and the game's
//!   own text must tie them — the proc's description (followed through
//!   `$@spelldesc`), or the description of a spell it references, names the
//!   driver ("Your Blade of Justice causes the target to burn…"); or the
//!   text references the driver's id (an Erupt's description IS its
//!   plague's); or a `SpellEffect` of the driver triggers the proc, directly
//!   or through one spell (Wake of Ashes → Truth's Wake). None of the three
//!   fails the build, naming the entry;
//! - the logs: the committed census (`tools/proc-spells-census.csv`,
//!   written by `tools/census-proc-spells.sh`) must show the pair in real
//!   play — at least `THRESHOLD` of the proc's direct onsets within 1.5 s
//!   after the same player's driver, or of its hits landing on a target
//!   carrying the player's driver aura. The better of the two decides; a
//!   pair neither holds is a build failure, unless the entry carries a
//!   `census_exempt` reason, printed beside the counts.
//!
//! A proc with two drivers (Soulburst: Consume, or Devour in Void
//! Metamorphosis; Atonement: every damage spell) has no entry — the tree
//! nests a row under ONE spell. The driver is named as the log writes it:
//! the tree finds the driver's row by name, and takes the group's icon
//! from that row.
//!
//! Output is two files: the table (`proc_spells.rs`) and its review twin
//! (`proc_spells.expected.md`) with every entry's evidence.

use crate::spelltip::ids_in;
use crate::table::{Csv, parse_csv};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

/// The tables the generator consumes, with their FileDataIDs
/// (from wowdev/wow-listfile; stable per file, forever).
pub const TABLES: [(&str, u32); 3] = [
    ("SpellName", 1990283),
    ("Spell", 1140089),
    ("SpellEffect", 1140088),
];

/// The share of a pair's census the better metric must reach.
pub const THRESHOLD: f64 = 0.9;

/// One curated entry: the proc's spell id and name as the log writes them,
/// the driver's (the id only proves the name — the tree matches by name),
/// the spec it belongs to (for review), and — for a pair the committed
/// census cannot show — the reason the census requirement is waived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Curated {
    pub id: u32,
    pub name: &'static str,
    pub driver_id: u32,
    pub driver: &'static str,
    pub spec: &'static str,
    pub census_exempt: Option<&'static str>,
}

impl Curated {
    /// An entry the census must show.
    pub const fn seen(
        id: u32,
        name: &'static str,
        driver_id: u32,
        driver: &'static str,
        spec: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            driver_id,
            driver,
            spec,
            census_exempt: None,
        }
    }
}

/// The table, by spec. The specs are the ones most often in the owner's own
/// keys and raids (the history store, 2026-09-30), plus Wither's Hellcaller
/// procs, R26's first example.
const CURATED: &[Curated] = &[
    // -- Demonology: the Dreadstalkers' bile rides their bite ---------------
    Curated::seen(1276960, "Blighted Maw", 271971, "Dreadbite", "Demonology"),
    // -- Elemental: each Overload under the spell it echoes -----------------
    Curated::seen(
        285466,
        "Lava Burst Overload",
        285452,
        "Lava Burst",
        "Elemental",
    ),
    Curated::seen(
        45284,
        "Lightning Bolt Overload",
        188196,
        "Lightning Bolt",
        "Elemental",
    ),
    Curated::seen(
        45297,
        "Chain Lightning Overload",
        188443,
        "Chain Lightning",
        "Elemental",
    ),
    Curated::seen(
        120588,
        "Elemental Blast Overload",
        117014,
        "Elemental Blast",
        "Elemental",
    ),
    // -- Arms ----------------------------------------------------------------
    Curated::seen(383706, "Fatal Mark", 260798, "Execute", "Arms"),
    // -- Unholy: a plague's eruption as its host dies ------------------------
    Curated::seen(
        1241167,
        "Virulent Plague (Erupt)",
        191587,
        "Virulent Plague",
        "Unholy",
    ),
    Curated::seen(
        1241171,
        "Dread Plague (Erupt)",
        1240996,
        "Dread Plague",
        "Unholy",
    ),
    // -- Shadow ----------------------------------------------------------------
    Curated::seen(
        1231479,
        "Shadeburst",
        413231,
        "Shadowy Apparition",
        "Shadow",
    ),
    Curated::seen(
        1264176,
        "Void Apparition",
        335467,
        "Shadow Word: Madness",
        "Shadow",
    ),
    // -- Retribution -----------------------------------------------------------
    Curated::seen(
        383346,
        "Expurgation",
        184575,
        "Blade of Justice",
        "Retribution",
    ),
    Curated::seen(
        1261160,
        "Light Within",
        184575,
        "Blade of Justice",
        "Retribution",
    ),
    Curated::seen(
        403695,
        "Truth's Wake",
        255937,
        "Wake of Ashes",
        "Retribution",
    ),
    // -- Holy Paladin ------------------------------------------------------------
    Curated::seen(
        461499,
        "Overflowing Light",
        25914,
        "Holy Shock",
        "Holy Paladin",
    ),
    // -- Balance -----------------------------------------------------------------
    Curated::seen(
        428682,
        "Boundless Moonlight",
        211545,
        "Fury of Elune",
        "Balance",
    ),
    // -- Hellcaller (Destruction, Affliction) -------------------------------------
    Curated::seen(
        445736,
        "Blackened Soul",
        445468,
        "Wither",
        "Destruction, Affliction",
    ),
];

/// `proc id \t driver name` per curated entry — what
/// `tools/census-proc-spells.sh` counts, so the census and the table never
/// disagree on what is curated.
pub fn pairs() -> String {
    pairs_of(CURATED)
}

fn pairs_of(curated: &[Curated]) -> String {
    curated
        .iter()
        .map(|c| format!("{}\t{}\n", c.id, c.driver))
        .collect()
}

/// One log's counts for one pair (see `tools/census-proc-spells.sh`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub onsets: u64,
    pub f50: u64,
    pub f250: u64,
    pub f1500: u64,
    pub lands: u64,
    pub on: u64,
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        self.onsets += o.onsets;
        self.f50 += o.f50;
        self.f250 += o.f250;
        self.f1500 += o.f1500;
        self.lands += o.lands;
        self.on += o.on;
    }

    fn rate(n: u64, of: u64) -> f64 {
        if of == 0 { 0.0 } else { n as f64 / of as f64 }
    }

    /// The better of the two metrics.
    fn best(&self) -> f64 {
        Self::rate(self.f1500, self.onsets).max(Self::rate(self.on, self.lands))
    }
}

/// The committed real-log census: per (proc id, driver) and per log, the
/// name the log wrote the proc under and its counts.
#[derive(Debug, Default)]
pub struct Census {
    /// Log basenames, first-seen order.
    pub logs: Vec<String>,
    /// (proc id, driver) → (proc names seen, counts per log in `logs` order).
    pub pairs: HashMap<(u32, String), (BTreeSet<String>, Vec<Counts>)>,
}

impl Census {
    /// Parse `tools/census-proc-spells.sh`'s CSV:
    /// `proc_id,proc_name,driver,log,onsets,f50,f250,f1500,lands,on`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let csv = parse_csv(text).map_err(|e| format!("census: {e}"))?;
        let col = |n: &str| csv.col(n);
        let (id_c, name_c, drv_c, log_c) = (
            col("proc_id")?,
            col("proc_name")?,
            col("driver")?,
            col("log")?,
        );
        let count_cols = [
            col("onsets")?,
            col("f50")?,
            col("f250")?,
            col("f1500")?,
            col("lands")?,
            col("on")?,
        ];
        let mut census = Census::default();
        for row in &csv.rows {
            let id: u32 = cell(row, id_c, "census")?
                .parse()
                .map_err(|_| format!("census: bad proc id {:?}", row.first()))?;
            let log = cell(row, log_c, "census")?.to_string();
            let li = match census.logs.iter().position(|l| *l == log) {
                Some(i) => i,
                None => {
                    census.logs.push(log);
                    census.logs.len() - 1
                }
            };
            let mut n = [0u64; 6];
            for (slot, &c) in n.iter_mut().zip(&count_cols) {
                let v = cell(row, c, "census")?;
                *slot = v
                    .parse()
                    .map_err(|_| format!("census: bad count {v:?} for {id}"))?;
            }
            let counts = Counts {
                onsets: n[0],
                f50: n[1],
                f250: n[2],
                f1500: n[3],
                lands: n[4],
                on: n[5],
            };
            let key = (id, cell(row, drv_c, "census")?.to_string());
            let entry = census.pairs.entry(key).or_default();
            let name = cell(row, name_c, "census")?;
            if !name.is_empty() {
                entry.0.insert(name.to_string());
            }
            if entry.1.len() <= li {
                entry.1.resize(li + 1, Counts::default());
            }
            if let Some(slot) = entry.1.get_mut(li) {
                slot.add(&counts);
            }
        }
        let logs = census.logs.len();
        for (_, per_log) in census.pairs.values_mut() {
            per_log.resize(logs, Counts::default());
        }
        Ok(census)
    }
}

#[derive(Debug)]
pub struct Generated {
    /// `proc_spells.rs`.
    pub content: String,
    /// `proc_spells.expected.md`.
    pub expected: String,
    pub procs: usize,
}

/// How the install ties an entry's proc to its driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Text {
    /// The description of this spell — the proc's own, reached through
    /// `$@spelldesc`, or one it references — names the driver.
    Names(u32),
    /// The proc's text references the driver's id.
    Refers,
    /// The driver's `SpellEffect` triggers the proc (through `via` when
    /// the chain has a middle spell).
    Triggers(Option<u32>),
}

/// The waiver, or the counts: what the review twin prints per entry.
#[derive(Debug, Clone, Copy)]
enum Evidence {
    Counts(Counts, usize),
    Exempt(&'static str, Counts),
}

/// One cell of a CSV row. The column index comes from `Csv::col`, so a miss
/// means the row itself is short — a malformed table, not a bug here.
fn cell<'a>(row: &'a [String], c: usize, what: &str) -> Result<&'a str, String> {
    row.get(c)
        .map(String::as_str)
        .ok_or_else(|| format!("{what}: row has no column {c}"))
}

pub fn generate(
    tables: &HashMap<&str, Csv>,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    generate_curated(CURATED, tables, census, build)
}

pub fn generate_curated(
    curated: &[Curated],
    tables: &HashMap<&str, Csv>,
    census: &Census,
    build: &str,
) -> Result<Generated, String> {
    let get = |name: &str| {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };
    let ids: HashSet<u32> = curated.iter().map(|c| c.id).collect();
    if ids.len() != curated.len() {
        return Err("curated list holds a duplicate proc id".into());
    }

    let sn = get("SpellName")?;
    let (n_id, n_name) = (sn.col("ID")?, sn.col("Name_lang")?);
    let mut names: HashMap<u32, &str> = HashMap::new();
    for row in &sn.rows {
        let id: u32 = cell(row, n_id, "SpellName")?.parse().unwrap_or(0);
        names.insert(id, cell(row, n_name, "SpellName")?);
    }

    let sp = get("Spell")?;
    let (d_id, d_text) = (sp.col("ID")?, sp.col("Description_lang")?);
    let mut descs: HashMap<u32, &str> = HashMap::new();
    for row in &sp.rows {
        let id: u32 = cell(row, d_id, "Spell")?.parse().unwrap_or(0);
        descs.insert(id, cell(row, d_text, "Spell")?);
    }

    // SpellEffect: spell → the spells its effects trigger.
    let se = get("SpellEffect")?;
    let (e_spell, e_trig) = (se.col("SpellID")?, se.col("EffectTriggerSpell")?);
    let mut triggers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for row in &se.rows {
        let t: u32 = cell(row, e_trig, "SpellEffect")?.parse().unwrap_or(0);
        if t == 0 {
            continue;
        }
        let s: u32 = cell(row, e_spell, "SpellEffect")?.parse().unwrap_or(0);
        triggers.entry(s).or_default().insert(t);
    }

    let mut table: BTreeMap<u32, (&Curated, Text, Evidence)> = BTreeMap::new();
    for c in curated {
        for (id, want) in [(c.id, c.name), (c.driver_id, c.driver)] {
            match names.get(&id) {
                None => {
                    return Err(format!(
                        "proc {} ({}): no SpellName row for {id}",
                        c.id, c.name
                    ));
                }
                Some(actual) if *actual != want => {
                    return Err(format!(
                        "proc {} ({}): SpellName says {id} is {actual:?}, curated as {want:?}",
                        c.id, c.name
                    ));
                }
                Some(_) => {}
            }
        }
        let text = text_evidence(c, &descs, &triggers).ok_or_else(|| {
            format!(
                "proc {} ({}): nothing in the client's text or effects ties it to {} ({}) — \
                 a guess, not a curation",
                c.id, c.name, c.driver_id, c.driver
            )
        })?;

        let (seen_names, per_log) = census
            .pairs
            .get(&(c.id, c.driver.to_string()))
            .cloned()
            .unwrap_or_default();
        if let Some(other) = seen_names.iter().find(|n| *n != c.name) {
            return Err(format!(
                "proc {}: the census names it {other:?}, curated as {:?}",
                c.id, c.name
            ));
        }
        let mut total = Counts::default();
        for n in &per_log {
            total.add(n);
        }
        let logs = per_log.iter().filter(|n| n.lands > 0).count();
        let evidence = match c.census_exempt {
            Some(reason) => Evidence::Exempt(reason, total),
            None if total.lands == 0 => {
                return Err(format!(
                    "proc {} ({}): the census never saw it — run tools/census-proc-spells.sh \
                     over logs that hold it",
                    c.id, c.name
                ));
            }
            None if total.best() < THRESHOLD => {
                return Err(format!(
                    "proc {} ({}) under {}: the census shows {:.1}% within 1.5 s and {:.1}% \
                     on the driver's aura — neither reaches {:.0}%",
                    c.id,
                    c.name,
                    c.driver,
                    100.0 * Counts::rate(total.f1500, total.onsets),
                    100.0 * Counts::rate(total.on, total.lands),
                    100.0 * THRESHOLD
                ));
            }
            None => Evidence::Counts(total, logs),
        };
        table.insert(c.id, (c, text, evidence));
    }

    Ok(Generated {
        procs: table.len(),
        content: emit(&table, build)?,
        expected: emit_expected(&table, census.logs.len(), &names, build)?,
    })
}

/// A description with its leading `$@spelldesc<id>` references followed.
fn resolve<'a>(id: u32, descs: &HashMap<u32, &'a str>, seen: &mut Vec<u32>) -> Option<&'a str> {
    let mut at = id;
    for _ in 0..6 {
        seen.push(at);
        let text = descs.get(&at)?;
        match text.strip_prefix("$@spelldesc") {
            Some(rest) => {
                at = rest
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()?;
            }
            None => return Some(text),
        }
    }
    None
}

fn text_evidence(
    c: &Curated,
    descs: &HashMap<u32, &str>,
    triggers: &HashMap<u32, BTreeSet<u32>>,
) -> Option<Text> {
    let mut chain = Vec::new();
    let own = resolve(c.id, descs, &mut chain);
    let mut refs: HashSet<u32> = chain.iter().copied().collect();
    if let Some(t) = own {
        ids_in(t, &mut refs);
    }
    if let Some(t) = own
        && t.contains(c.driver)
    {
        return Some(Text::Names(*chain.last()?));
    }
    // One hop: a spell the proc's text references names the driver (an
    // Overload's text reads Mastery: Elemental Overload's values).
    let mut hops: Vec<u32> = refs.iter().copied().filter(|r| *r >= 100).collect();
    hops.sort_unstable();
    for r in hops {
        let mut sub = Vec::new();
        if let Some(t) = resolve(r, descs, &mut sub)
            && t.contains(c.driver)
        {
            return Some(Text::Names(*sub.last()?));
        }
    }
    if refs.contains(&c.driver_id) {
        return Some(Text::Refers);
    }
    let direct = triggers.get(&c.driver_id)?;
    if direct.contains(&c.id) {
        return Some(Text::Triggers(None));
    }
    direct
        .iter()
        .find(|m| triggers.get(m).is_some_and(|t| t.contains(&c.id)))
        .map(|m| Text::Triggers(Some(*m)))
}

fn emit(table: &BTreeMap<u32, (&Curated, Text, Evidence)>, build: &str) -> Result<String, String> {
    let drivers: BTreeSet<&str> = table.values().map(|(c, _, _)| c.driver).collect();
    let drivers: Vec<&str> = drivers.into_iter().collect();
    let mut o = String::new();
    let e = |e: std::fmt::Error| format!("emit: {e}");
    o.push_str("//! GENERATED by tools/gen-proc-spells.sh — do not edit by hand.\n");
    // No timestamp: same build and census in, same bytes out.
    writeln!(
        o,
        "//! Source: local client DB2s via wowdps-extract, build {build}."
    )
    .map_err(e)?;
    writeln!(
        o,
        "//! {} procs under {} drivers.",
        table.len(),
        drivers.len()
    )
    .map_err(e)?;
    o.push_str(
        "//!\n\
         //! Maps a talent proc's combat-log spell id to the spell that drives it, by\n\
         //! the driver's name as the log writes it, so the ability tree hangs the proc\n\
         //! under its driver (CONTRACT.md R26). Membership is curated in\n\
         //! tools/extract/src/procgen.rs; every entry is tied to its driver by the\n\
         //! client's own text or effects and shown in real logs, and that evidence\n\
         //! sits in proc_spells.expected.md.\n\
         \n\
         /// The name of the spell that drives a proc, or `None` for a spell the table\n\
         /// does not curate.\n\
         pub(crate) fn driver_of(spell_id: u32) -> Option<&'static str> {\n\
         \x20   let i = TABLE.binary_search_by_key(&spell_id, |e| e.0).ok()?;\n\
         \x20   let &(_, d) = TABLE.get(i)?;\n\
         \x20   DRIVERS.get(d as usize).copied()\n\
         }\n\
         \n\
         /// The drivers, sorted.\n\
         static DRIVERS: &[&str] = &[\n",
    );
    for d in &drivers {
        writeln!(o, "    {d:?},").map_err(e)?;
    }
    o.push_str(
        "];\n\
         \n\
         /// (proc id, index into DRIVERS), sorted by proc id.\n\
         #[rustfmt::skip]\n\
         static TABLE: &[(u32, u16)] = &[\n",
    );
    let cells: Vec<String> = table
        .iter()
        .map(|(id, (c, _, _))| {
            let d = drivers.iter().position(|d| *d == c.driver).unwrap_or(0);
            format!("({id},{d}),")
        })
        .collect();
    for chunk in cells.chunks(6) {
        writeln!(o, "    {}", chunk.join(" ")).map_err(e)?;
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
         \x20   /// Every entry names a driver, and a stranger is `None`.\n\
         \x20   #[test]\n\
         \x20   fn every_entry_resolves() {\n\
         \x20       assert!(super::TABLE.iter().all(|e| super::driver_of(e.0).is_some()));\n\
         \x20       assert_eq!(super::driver_of(0), None);\n\
         \x20   }\n\
         }\n",
    );
    Ok(o)
}

fn pct(n: u64, of: u64) -> String {
    if of == 0 {
        "—".to_string()
    } else {
        format!("{:.1}%", 100.0 * n as f64 / of as f64)
    }
}

fn emit_expected(
    table: &BTreeMap<u32, (&Curated, Text, Evidence)>,
    logs: usize,
    names: &HashMap<u32, &str>,
    build: &str,
) -> Result<String, String> {
    let mut o = String::new();
    let e = |e: std::fmt::Error| format!("emit: {e}");
    o.push_str("# proc_spells.rs — the curated table and its evidence\n\n");
    o.push_str(
        "GENERATED by tools/gen-proc-spells.sh beside `proc_spells.rs` — do not edit by hand.\n",
    );
    writeln!(
        o,
        "Build {build}, {} procs; census over {logs} log(s).\n",
        table.len()
    )
    .map_err(e)?;
    o.push_str(
        "## Rules\n\n\
         - Membership is curated in `tools/extract/src/procgen.rs` as `(proc id, proc name,\n\
         \x20 driver id, driver name, spec)`; the tree matches the driver's row by name.\n\
         - The install proves each entry: `SpellName` carries both names, and the client's\n\
         \x20 own text ties them — the proc's description (through `$@spelldesc`), or one it\n\
         \x20 references, names the driver; or the text references the driver's id; or a\n\
         \x20 `SpellEffect` of the driver triggers the proc (directly or through one spell).\n\
         - The logs prove it too: `tools/proc-spells-census.csv`\n\
         \x20 (`tools/census-proc-spells.sh <log>...`) counts the proc's direct onsets within\n\
         \x20 1.5 s after the same player's driver, and its hits on a target carrying the\n\
         \x20 player's driver aura; the better share must reach 90%, unless the entry\n\
         \x20 carries a `census_exempt` reason, printed in place of the verdict.\n\
         - A proc with two drivers has no entry: a row nests under one spell.\n\n\
         ## Entries\n\n\
         | proc id | proc | driver | spec | the client's text | onsets | ≤ 50 ms | ≤ 250 ms | ≤ 1.5 s | hits | on the driver's aura | logs |\n\
         | ---: | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    let mut rows: Vec<_> = table.iter().collect();
    rows.sort_by_key(|(id, (c, _, _))| (c.spec, c.driver, **id));
    for (id, (c, text, evidence)) in rows {
        let text = match text {
            Text::Names(s) if *s == *id => "its description names it".to_string(),
            Text::Names(s) => format!("{s} ({}) names it", names.get(s).copied().unwrap_or("?")),
            Text::Refers => "its description is the driver's".to_string(),
            Text::Triggers(None) => format!("{} triggers it", c.driver_id),
            Text::Triggers(Some(m)) => format!("{} triggers it through {m}", c.driver_id),
        };
        let (n, logs) = match evidence {
            Evidence::Counts(n, logs) => (n, logs.to_string()),
            Evidence::Exempt(reason, n) => (n, format!("exempt: {reason}")),
        };
        writeln!(
            o,
            "| {id} | {} | {} ({}) | {} | {text} | {} | {} | {} | {} | {} | {} | {logs} |",
            c.name,
            c.driver,
            c.driver_id,
            c.spec,
            n.onsets,
            pct(n.f50, n.onsets),
            pct(n.f250, n.onsets),
            pct(n.f1500, n.onsets),
            n.lands,
            pct(n.on, n.lands),
        )
        .map_err(e)?;
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[Curated] = &[
        Curated::seen(
            383346,
            "Expurgation",
            184575,
            "Blade of Justice",
            "Retribution",
        ),
        Curated::seen(
            403695,
            "Truth's Wake",
            255937,
            "Wake of Ashes",
            "Retribution",
        ),
        Curated::seen(
            1241167,
            "Virulent Plague (Erupt)",
            191587,
            "Virulent Plague",
            "Unholy",
        ),
    ];

    fn tables() -> HashMap<&'static str, Csv> {
        let mut t = HashMap::new();
        t.insert(
            "SpellName",
            parse_csv(
                "ID,Name_lang\n\
                 184575,Blade of Justice\n\
                 191587,Virulent Plague\n\
                 255937,Wake of Ashes\n\
                 383344,Expurgation\n\
                 383346,Expurgation\n\
                 403695,Truth's Wake\n\
                 1241167,Virulent Plague (Erupt)\n",
            )
            .unwrap(),
        );
        t.insert(
            "Spell",
            parse_csv(
                "ID,NameSubtext_lang,Description_lang,AuraDescription_lang\n\
                 191587,,\"An infectious plague that spreads to all nearby enemies.\",\n\
                 383344,,\"Your Blade of Justice causes the target to burn for $383346o1.\",\n\
                 383346,,$@spelldesc383344,\n\
                 403695,,\"Burns the targets for an additional $o2 Radiant damage.\",\n\
                 1241167,,$@spelldesc191587,\n",
            )
            .unwrap(),
        );
        t.insert(
            "SpellEffect",
            parse_csv(
                "ID,SpellID,EffectIndex,Effect,EffectTriggerSpell\n\
                 1,255937,0,64,403695\n\
                 2,184575,0,2,0\n",
            )
            .unwrap(),
        );
        t
    }

    fn census(rows: &str) -> Census {
        Census::parse(&format!(
            "proc_id,proc_name,driver,log,onsets,f50,f250,f1500,lands,on\n{rows}"
        ))
        .unwrap()
    }

    fn good_census() -> Census {
        census(
            "383346,Expurgation,Blade of Justice,a.txt,100,80,99,100,400,0\n\
             403695,Truth's Wake,Wake of Ashes,a.txt,10,10,10,10,60,0\n\
             1241167,Virulent Plague (Erupt),Virulent Plague,a.txt,50,1,2,5,50,49\n\
             1241167,Virulent Plague (Erupt),Virulent Plague,b.txt,0,0,0,0,0,0\n",
        )
    }

    #[test]
    fn each_kind_of_text_proves_its_entry() {
        let g = generate_curated(GOOD, &tables(), &good_census(), "12.1.0").unwrap();
        assert_eq!(g.procs, 3);
        // Named through the $@spelldesc chain; triggered; the Erupt's text
        // is its plague's.
        assert!(
            g.expected.contains("383344 (Expurgation) names it"),
            "{}",
            g.expected
        );
        assert!(g.expected.contains("255937 triggers it"), "{}", g.expected);
        assert!(
            g.expected.contains("its description is the driver's"),
            "{}",
            g.expected
        );
        // The Erupt passes on the aura metric alone, over the log holding it.
        assert!(g.expected.contains("| 50 | 98.0% | 1 |"), "{}", g.expected);
        assert!(g.content.contains("\"Blade of Justice\","));
        assert!(g.content.contains("(383346,0),"), "{}", g.content);
    }

    #[test]
    fn same_inputs_same_bytes() {
        let a = generate_curated(GOOD, &tables(), &good_census(), "12.1.0").unwrap();
        let b = generate_curated(GOOD, &tables(), &good_census(), "12.1.0").unwrap();
        assert_eq!((a.content, a.expected), (b.content, b.expected));
    }

    #[test]
    fn a_pair_nothing_in_the_client_ties_is_refused() {
        let guess = [Curated::seen(
            403695,
            "Truth's Wake",
            184575,
            "Blade of Justice",
            "Retribution",
        )];
        let c = census("403695,Truth's Wake,Blade of Justice,a.txt,10,10,10,10,60,0\n");
        let err = generate_curated(&guess, &tables(), &c, "x").unwrap_err();
        assert!(err.contains("a guess, not a curation"), "{err}");
    }

    #[test]
    fn a_pair_the_logs_do_not_show_is_refused_unless_exempt() {
        let c = census(
            "383346,Expurgation,Blade of Justice,a.txt,100,1,2,30,400,0\n\
             403695,Truth's Wake,Wake of Ashes,a.txt,10,10,10,10,60,0\n\
             1241167,Virulent Plague (Erupt),Virulent Plague,a.txt,50,1,2,5,50,49\n",
        );
        let err = generate_curated(GOOD, &tables(), &c, "x").unwrap_err();
        assert!(err.contains("30.0% within 1.5 s"), "{err}");
        let mut waived = GOOD.to_vec();
        waived[0].census_exempt = Some("a talent no committed log exercises");
        let g = generate_curated(&waived, &tables(), &c, "x").unwrap();
        assert!(
            g.expected
                .contains("exempt: a talent no committed log exercises")
        );
    }

    #[test]
    fn a_renamed_spell_or_an_unseen_proc_is_refused() {
        let mut renamed = GOOD.to_vec();
        renamed[1].driver = "Wake of Light";
        let err = generate_curated(&renamed, &tables(), &good_census(), "x").unwrap_err();
        assert!(err.contains("curated as \"Wake of Light\""), "{err}");
        let c = census("403695,Truth's Wake,Wake of Ashes,a.txt,10,10,10,10,60,0\n");
        let err = generate_curated(GOOD, &tables(), &c, "x").unwrap_err();
        assert!(err.contains("the census never saw it"), "{err}");
    }

    #[test]
    fn the_census_parses_pairs_across_logs() {
        let c = good_census();
        assert_eq!(c.logs, ["a.txt", "b.txt"]);
        let (names, per_log) = &c.pairs[&(1241167, "Virulent Plague".to_string())];
        assert_eq!(names.len(), 1);
        assert_eq!(per_log.len(), 2);
        assert_eq!(per_log[0].on, 49);
        assert_eq!(
            pairs_of(GOOD).lines().next(),
            Some("383346\tBlade of Justice")
        );
    }
}
