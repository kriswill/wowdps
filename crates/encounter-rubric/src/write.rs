//! Writing rubric files: what `gen-rubric` drafts. The extractor names no
//! serde or toml itself; it hands a typed `Encounter` here.
//!
//! The text is laid out for review: keys in the schema's own order (`ORDER`),
//! each named entry (`[npc.slug]`, `[ability.slug]` …) under its own header,
//! and every list and every table inside an entry (a shape, a trigger) on one
//! line. Scalars are written by toml itself, so its quoting and escaping hold.

use std::collections::BTreeMap;

use serde::Serialize;
use toml::{Table, Value};

use crate::Encounter;

/// The schema's keys in the order a file lists them: every key it writes
/// (a test holds it). A key not here would come after, alphabetically.
const ORDER: &[&str] = &[
    "schema",
    "encounter",
    "enabled",
    "name",
    "instance",
    "order",
    "kind",
    "tint",
    "journal",
    "map",
    "ui_map",
    "floor",
    "level",
    "ceiling",
    "light",
    "scale",
    "exclude_groups",
    "exclude_models",
    "exclude_textures",
    "layer",
    "wmo",
    "placement",
    "breaks",
    "stencil",
    "points",
    "ppy",
    "glow",
    "color",
    "hot",
    "area",
    "except",
    "below",
    "rise",
    "hold",
    "fade",
    "view",
    "turn",
    "zoom",
    "zoom_max",
    "center",
    "arena",
    "npc",
    "creature",
    "role",
    "icon",
    "owner",
    "shares",
    "still",
    "ability",
    "spell",
    "by",
    "shape",
    "inner",
    "outer",
    "angle",
    "lifetime",
    "alert",
    "marks",
    "phase",
    "under",
    "about",
    "text",
    "place",
    "at",
    "radius",
    "duration",
    "period",
    "stacks",
    "triggers",
    "liquid_height",
    "from",
    "until",
    "ground",
    "delay",
    "ahead",
    "ends",
    "grow",
    "spread",
    "preview",
    "mark",
    "aura",
    "count",
    "tick",
    "each",
    "goal",
    "ring",
    "label",
    "link",
    "soak",
    "dot",
    "harm",
    "line",
    "to",
    "hits",
    "volley",
    "toward",
    "aim",
    "spots",
    "side",
    "speed",
    "width",
    "front",
    "form",
    "start",
    "length",
    "every",
    "times",
    "alternate",
    "holes",
    "faint",
    "destroys",
    "flash",
    "range",
    "sides",
    "apart",
    "within",
    "allow",
    "grace",
    "streak",
    "most",
    "room",
    "through",
    "reach",
    "orbit",
    "around",
    "seen",
    "event",
    "on",
    "after",
    "cast",
    "aura_applied",
    "aura_removed",
    "health_below",
    "pct",
    "plateau",
    "secs",
    "since",
    "nth",
    "appears",
    "death",
    "hit",
    "aura_gone",
    "enter",
    "again",
    "leave",
    "only",
    "difficulty",
];

/// Tables of named entries: each entry under them gets a header.
const COLLECTIONS: &[&str] = &[
    "npc", "ability", "phase", "place", "ground", "mark", "line", "volley", "flash", "range",
    "apart", "streak", "room", "reach", "event", "layer", "glow", "spell",
];

/// An encounter as a draft file's text: `header` as `#` comment lines,
/// then the encounter as TOML. An empty table or list is left out, so a
/// draft says only what it knows; the text reads back to the same
/// `Encounter`.
///
/// `difficulty` is the encounter as each difficulty (by name) reads it.
/// Each is written as `[difficulty.<name>]`, holding only what differs from
/// what the draft gives it through the difficulties it falls back to, so
/// the text reads back to that `Encounter` on that difficulty too. An
/// overlay can set a value, never unset one: what a difficulty lacks that
/// its fallback has, it reads as the fallback's.
pub fn draft_text(
    e: &Encounter,
    difficulty: &BTreeMap<String, Encounter>,
    header: &str,
) -> Result<String, String> {
    let mut t = table(e)?;
    let named = difficulty
        .iter()
        .map(|(n, d)| Ok((n.clone(), table(d)?)))
        .collect::<Result<Vec<_>, String>>()?;
    overlay(&mut t, named)?;
    let mut out = comment(header);
    emit(&mut out, &[], &t);
    Ok(out)
}

/// An encounter's journal text as its per-machine sidecar (`crate::text`):
/// `header` as `#` comment lines, then `[ability.<slug>] text = …` for each
/// ability's text where it first appears, and `[difficulty.<name>.ability.
/// <slug>]` where a difficulty reads otherwise, as a draft's overlays are
/// written. It reads back (`text::lay`) to each difficulty's reading.
pub fn text_file(t: &crate::text::Texts, header: &str) -> Result<String, String> {
    let abilities = |texts: &BTreeMap<String, String>| {
        let mut entries = Table::new();
        for (key, text) in texts {
            let mut entry = Table::new();
            entry.insert("text".into(), Value::String(text.clone()));
            entries.insert(key.clone(), Value::Table(entry));
        }
        let mut file = Table::new();
        file.insert("ability".into(), Value::Table(entries));
        file
    };
    let mut file = Table::new();
    file.insert("schema".into(), Value::Integer(i64::from(crate::SCHEMA)));
    file.extend(abilities(&t.ability));
    let named = t
        .difficulty
        .iter()
        .map(|(n, d)| (n.clone(), abilities(d)))
        .collect();
    overlay(&mut file, named)?;
    let mut out = comment(header);
    emit(&mut out, &[], &file);
    Ok(out)
}

/// Lay `[difficulty.<name>]` overlays into `t`, the base: each difficulty's
/// whole reading (by name) written as what differs from what `t` gives it
/// through the difficulties it falls back to. An overlay can set a value,
/// never unset one.
fn overlay(t: &mut Table, difficulty: Vec<(String, Table)>) -> Result<(), String> {
    let mut overlays = Table::new();
    // The most general first, so each difficulty is read over the overlays
    // of the ones it falls back to.
    let mut named = difficulty;
    named.sort_by_key(|(n, _)| crate::difficulty::fallbacks(n).len());
    for (name, d) in named {
        if !crate::difficulty::NAMES.contains(&name.as_str()) {
            return Err(format!("[difficulty.{name}]: not a difficulty"));
        }
        // What the text says so far, as this difficulty would read it:
        // through the ones it falls back to, as a reader lays them.
        let mut read = t.clone();
        read.insert("difficulty".into(), Value::Table(overlays.clone()));
        crate::lay_difficulties(&mut read, crate::difficulty::fallbacks(&name))?;
        let own = changes(&read, &d);
        if !own.values().all(empty) {
            overlays.insert(name, Value::Table(own));
        }
    }
    if !overlays.is_empty() {
        t.insert("difficulty".into(), Value::Table(overlays));
    }
    Ok(())
}

fn table(e: &Encounter) -> Result<Table, String> {
    match Value::try_from(e).map_err(|e| e.to_string())? {
        Value::Table(t) => Ok(t),
        _ => Err("an encounter is a table".into()),
    }
}

/// What `want` sets that `read` does not already say: tables compared key
/// by key, anything else whole. A key `read` has and `want` lacks is left
/// alone.
fn changes(read: &Table, want: &Table) -> Table {
    let mut out = Table::new();
    for (k, w) in want {
        match (read.get(k), w) {
            (Some(r), w) if r == w => {}
            (Some(Value::Table(r)), Value::Table(w)) => {
                let c = changes(r, w);
                if !c.is_empty() {
                    out.insert(k.clone(), Value::Table(c));
                }
            }
            (_, w) => {
                out.insert(k.clone(), w.clone());
            }
        }
    }
    out
}

/// An instance file's text, written once for a raid or dungeon and tuned by
/// hand after.
pub fn instance_text(header: &str, name: &str, kind: &str, journal: u32, map: u32) -> String {
    #[derive(Serialize)]
    struct InstanceFile<'a> {
        schema: u32,
        name: &'a str,
        kind: &'a str,
        journal: u32,
        map: u32,
    }
    let f = InstanceFile {
        schema: crate::SCHEMA,
        name,
        kind,
        journal,
        map,
    };
    format!(
        "{}{}",
        comment(header),
        toml::to_string(&f).unwrap_or_default()
    )
}

fn comment(header: &str) -> String {
    header
        .lines()
        .map(|l| {
            if l.is_empty() {
                "#\n".to_string()
            } else {
                format!("# {l}\n")
            }
        })
        .collect()
}

/// A table's keys in the schema's order.
fn ordered(t: &Table) -> Vec<(&String, &Value)> {
    let mut v: Vec<(&String, &Value)> = t.iter().collect();
    v.sort_by_cached_key(|(k, _)| {
        let rank = ORDER.iter().position(|o| o == k).unwrap_or(ORDER.len());
        (rank, *k)
    });
    v
}

/// Whether the table at `path` gets a header of its own (and its tables
/// may too), rather than being written on one line: the top level's
/// tables, the tables of named entries and each entry in them, and the
/// same again under a difficulty.
fn sectioned(path: &[&str]) -> bool {
    match path {
        [] | [_] => true,
        ["difficulty", _] => true,
        ["difficulty", _, rest @ ..] => sectioned(rest),
        [.., parent, last] => COLLECTIONS.contains(parent) || COLLECTIONS.contains(last),
    }
}

fn empty(v: &Value) -> bool {
    match v {
        Value::Table(t) => t.values().all(empty),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn key(k: &str) -> String {
    if !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        k.to_string()
    } else {
        Value::String(k.to_string()).to_string()
    }
}

/// A value on one line.
fn inline(v: &Value) -> String {
    match v {
        Value::Table(t) => {
            let parts: Vec<String> = ordered(t)
                .into_iter()
                .filter(|(_, v)| !empty(v))
                .map(|(k, v)| format!("{} = {}", key(k), inline(v)))
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(inline).collect::<Vec<_>>().join(", ")),
        // The schema's numbers are f32: one is written as the f32 it came
        // from (0.3, not 0.30000001192092896).
        Value::Float(f) if f.is_finite() && f64::from(*f as f32) == *f => {
            format!("{:?}", *f as f32)
        }
        scalar => scalar.to_string(),
    }
}

fn emit(out: &mut String, path: &[&str], t: &Table) {
    let mut sections = Vec::new();
    let mut lines = Vec::new();
    for (k, v) in ordered(t) {
        if empty(v) {
            continue;
        }
        let mut sub: Vec<&str> = path.to_vec();
        sub.push(k);
        match v {
            Value::Table(inner) if sectioned(&sub) => sections.push((k.as_str(), inner)),
            v => lines.push(format!("{} = {}", key(k), inline(v))),
        }
    }
    if !lines.is_empty() {
        if !path.is_empty() {
            let name: Vec<String> = path.iter().map(|p| key(p)).collect();
            out.push_str(&format!("\n[{}]\n", name.join(".")));
        }
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    for (k, inner) in sections {
        let mut sub: Vec<&str> = path.to_vec();
        sub.push(k);
        emit(out, &sub, inner);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::*;
    use crate::{Ability, Npc, Phase, Role, Rubric, Shape, Trigger};

    /// Every committed draft, written again from what it says: its header
    /// from its comment lines, the encounter it gives, and the encounter
    /// as each difficulty reads it through its fallback chain. What
    /// `gen-rubric` hands the writer, recovered from its own output, so the
    /// writer and the chains are held to the committed files with no game
    /// install.
    #[test]
    fn every_committed_draft_is_written_back_byte_for_byte() {
        let read = |path: &str, t: Table| {
            Encounter::deserialize(Value::Table(t)).unwrap_or_else(|e| panic!("{path}: {e}"))
        };
        let mut drafts = 0;
        for (path, text) in crate::embedded::FILES
            .iter()
            .filter(|(p, _)| p.ends_with(".draft.toml"))
        {
            let header: Vec<&str> = text
                .lines()
                .map_while(|l| l.strip_prefix('#'))
                .map(|l| l.strip_prefix(' ').unwrap_or(l))
                .collect();
            let mut t: Table = text.parse().unwrap_or_else(|e| panic!("{path}: {e}"));
            let overlays = match t.remove("difficulty") {
                Some(Value::Table(o)) => o,
                None => Table::new(),
                Some(v) => panic!("{path}: difficulty = {v}"),
            };
            let by: BTreeMap<String, Encounter> = crate::difficulty::NAMES
                .iter()
                .map(|&name| {
                    let mut d = t.clone();
                    for f in crate::difficulty::chain_named(name).iter().rev() {
                        if let Some(Value::Table(o)) = overlays.get(*f) {
                            crate::merge(&mut d, o.clone());
                        }
                    }
                    (name.to_string(), read(path, d))
                })
                .collect();
            let again = draft_text(&read(path, t), &by, &header.join("\n"))
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            if again != *text {
                let (want, got): (Vec<&str>, Vec<&str>) =
                    (text.lines().collect(), again.lines().collect());
                match (0..want.len().max(got.len())).find(|&i| want.get(i) != got.get(i)) {
                    Some(i) => panic!(
                        "{path}: line {}: {:?} is written {:?}",
                        i + 1,
                        want.get(i),
                        got.get(i)
                    ),
                    None => panic!("{path}: its line endings are written otherwise"),
                }
            }
            drafts += 1;
        }
        assert!(drafts >= 75, "{drafts} drafts");
    }

    /// Every key the schema writes has a place in `ORDER`, so a file lists
    /// it where the schema means it to rather than after the rest, by
    /// name; the slugs and spell ids naming entries are not keys.
    #[test]
    fn every_key_the_schema_writes_has_a_place_in_the_order() {
        let s = |v: &str| v.to_string();
        fn keys(entries: bool, v: &Value, out: &mut BTreeSet<String>) {
            match v {
                Value::Table(t) => {
                    for (k, v) in t {
                        if !entries {
                            out.insert(k.clone());
                        }
                        keys(!entries && named(k), v, out);
                    }
                }
                Value::Array(a) => a.iter().for_each(|v| keys(false, v, out)),
                _ => {}
            }
        }
        // The tables whose keys are entries' names.
        fn named(k: &str) -> bool {
            COLLECTIONS.contains(&k) || k == "difficulty"
        }
        let mut seen = BTreeSet::new();
        keys(
            false,
            &Value::Table(table(&crate::tests::every_key()).unwrap()),
            &mut seen,
        );
        // An instance file's and a text sidecar's, overlays and all.
        let texts = crate::text::Texts {
            ability: [(s("a"), s("Words."))].into(),
            difficulty: [(s("mythic"), [(s("a"), s("More words."))].into())].into(),
        };
        for file in [
            instance_text("", "Raid", "raid", 1, 2),
            text_file(&texts, "").unwrap(),
        ] {
            keys(false, &Value::Table(file.parse().unwrap()), &mut seen);
        }
        let unranked: Vec<&String> = seen
            .iter()
            .filter(|k| !ORDER.contains(&k.as_str()))
            .collect();
        assert!(unranked.is_empty(), "not in ORDER: {unranked:?}");
        for (i, k) in ORDER.iter().enumerate() {
            assert!(!ORDER[..i].contains(k), "{k} is in ORDER twice");
            assert!(seen.contains(*k), "{k} is in ORDER but no schema key");
        }
        // And all of it is written as text that reads back to it.
        let e = crate::tests::every_key();
        let text = draft_text(&e, &BTreeMap::new(), "every key").unwrap();
        let back: Table = text.parse().unwrap_or_else(|err| panic!("{err}\n{text}"));
        assert_eq!(Encounter::deserialize(Value::Table(back)).unwrap(), e);
        assert!(text.contains("\n[map.glow.g]\n"), "{text}");
    }

    #[test]
    fn a_draft_reads_back_to_the_encounter_it_was_written_from() {
        let mut e = Encounter {
            encounter: 3492,
            name: "Ula'tek".into(),
            instance: "raid".into(),
            order: 8,
            ..Encounter::default()
        };
        e.map.ui_map = Some(2610);
        e.map.floor = Some("ulatek".into());
        e.npc.insert(
            "ulatek".into(),
            Npc {
                enabled: true,
                name: "Ula'tek".into(),
                creature: vec![248711],
                role: Role::Boss,
                icon: Some(7_000_001),
                owner: None,
                shares: None,
                still: false,
                only: vec![],
            },
        );
        e.ability.insert(
            "serpents-bite".into(),
            Ability {
                enabled: true,
                name: "Serpent's \"Bite\"".into(),
                spell: vec![1, 2],
                by: Some("ulatek".into()),
                shape: Some(Shape::Cone {
                    angle: 60.0,
                    length: 35.0,
                }),
                lifetime: None,
                alert: Some(2),
                marks: vec!["deadly".into(), "tank".into()],
                phase: Some("stage-one".into()),
                under: None,
                text: None,
                only: vec!["heroic".into(), "mythic".into()],
            },
        );
        e.phase.insert(
            "stage-one".into(),
            Phase {
                enabled: true,
                name: "Stage One: Serpent's Bargain".into(),
                order: 0,
                enter: vec![Trigger::Start],
                again: false,
                leave: Vec::new(),
                only: vec![],
            },
        );
        e.phase.insert(
            "stage-two".into(),
            Phase {
                enabled: true,
                name: "Stage Two".into(),
                order: 1,
                enter: vec![
                    Trigger::Phase("stage-one".into()),
                    Trigger::HealthBelow {
                        npc: "ulatek".into(),
                        pct: 50.0,
                    },
                    Trigger::Cast {
                        spell: 77,
                        by: None,
                    },
                ],
                again: false,
                leave: Vec::new(),
                only: vec![],
            },
        );
        let text = draft_text(&e, &BTreeMap::new(), "generated\n\ndo not edit").unwrap();
        assert!(
            text.starts_with("# generated\n#\n# do not edit\nschema = 1\n"),
            "{text}"
        );
        assert!(
            text.contains("\n[npc.ulatek]\nname = \"Ula'tek\"\n"),
            "{text}"
        );
        assert!(
            text.contains("shape = { kind = \"cone\", angle = 60.0, length = 35.0 }"),
            "{text}"
        );
        assert!(
            text.contains(
                "enter = [{ phase = \"stage-one\" }, { health_below = { npc = \"ulatek\", pct = 50.0 } }, { cast = { spell = 77 } }]"
            ),
            "{text}"
        );
        assert!(
            !text.contains("[view]") && !text.contains("[place]"),
            "{text}"
        );
        let season = "schema = 1\nname = \"S\"\norder = 1\n".to_string();
        let instance = instance_text("once", "The Raid", "raid", 1320, 3004);
        let files = vec![
            ("s/season.toml".to_string(), season),
            ("s/raid/instance.toml".to_string(), instance),
            ("s/raid/3492-ulatek.draft.toml".to_string(), text),
        ];
        let r = Rubric::from_files(files).unwrap_or_else(|e| panic!("{e:?}"));
        let back = r.encounter(3492, None).unwrap().unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn each_difficulty_reads_back_to_its_own_encounter() {
        let mut e: Encounter = toml::from_str(
            "schema = 1\nencounter = 3445\nname = \"Entombed Sentinels\"\ninstance = \"raid\"\n",
        )
        .unwrap();
        e.ability.insert(
            "mark-of-blood".into(),
            Ability {
                enabled: true,
                name: "Mark of Blood".into(),
                spell: vec![1284503],
                by: None,
                shape: None,
                lifetime: None,
                alert: Some(1),
                marks: vec![],
                phase: None,
                under: None,
                text: None,
                only: vec![],
            },
        );
        let spell = |name: &str, radius: f32| crate::Spell {
            name: name.into(),
            radius: Some(radius),
            ..Default::default()
        };
        e.spell.insert(
            "1284506".into(),
            crate::Spell {
                duration: Some(40.0),
                period: Some(2.0),
                stacks: Some(99),
                ..spell("Mark of Blood", 40.0)
            },
        );
        e.spell.insert(
            "1288282".into(),
            crate::Spell {
                period: Some(0.3),
                triggers: vec![1288283],
                ..spell("Unstable Miasma", 7.5)
            },
        );
        // Heroic widens the burst; Mythic, falling back to Heroic, has the
        // base's again and alerts on the mark louder; LFR's marks reach less
        // far and stack less.
        let mut by = BTreeMap::new();
        by.insert("normal".to_string(), e.clone());
        let mut heroic = e.clone();
        if let Some(s) = heroic.spell.get_mut("1288282") {
            s.radius = Some(10.0);
        }
        by.insert("heroic".to_string(), heroic);
        let mut mythic = e.clone();
        if let Some(a) = mythic.ability.get_mut("mark-of-blood") {
            a.alert = Some(2);
        }
        by.insert("mythic".to_string(), mythic);
        let mut lfr = e.clone();
        if let Some(s) = lfr.spell.get_mut("1284506") {
            (s.radius, s.stacks) = (Some(30.0), Some(10));
        }
        by.insert("lfr".to_string(), lfr);

        let text = draft_text(&e, &by, "generated").unwrap();
        assert!(
            text.contains("\n[spell.1288282]\nname = \"Unstable Miasma\"\nradius = 7.5\nperiod = 0.3\ntriggers = [1288283]\n"),
            "{text}"
        );
        assert!(!text.contains("[difficulty.normal"), "{text}");
        assert!(
            text.contains("\n[difficulty.heroic.spell.1288282]\nradius = 10.0\n"),
            "{text}"
        );
        assert!(
            text.contains("\n[difficulty.mythic.spell.1288282]\nradius = 7.5\n"),
            "{text}"
        );
        assert!(
            text.contains("\n[difficulty.lfr.spell.1284506]\nradius = 30.0\nstacks = 10\n"),
            "{text}"
        );
        let files = vec![
            (
                "s/season.toml".to_string(),
                "schema = 1\nname = \"S\"\norder = 1\n".to_string(),
            ),
            (
                "s/raid/instance.toml".to_string(),
                instance_text("once", "The Raid", "raid", 1, 2),
            ),
            (
                "s/raid/3445-entombed-sentinels.draft.toml".to_string(),
                text,
            ),
        ];
        let r = Rubric::from_files(files).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(r.encounter(3445, None).unwrap().unwrap(), e);
        for (id, name) in [(14, "normal"), (15, "heroic"), (16, "mythic"), (17, "lfr")] {
            let back = r.encounter(3445, Some(id)).unwrap().unwrap();
            assert_eq!(&back, by.get(name).unwrap(), "{name}");
        }
        assert!(draft_text(&e, &[("hard".to_string(), e.clone())].into(), "x").is_err());
    }

    #[test]
    fn a_text_sidecar_reads_back_on_each_difficulty_and_its_absence_is_silent() {
        let ability = |text: &str| (String::from("mark-of-blood"), text.to_string());
        let base = "Marks players within 40 yards.";
        let texts = crate::text::Texts {
            ability: [ability(base)].into(),
            difficulty: [
                ("normal".to_string(), [ability(base)].into()),
                ("heroic".to_string(), [ability(base)].into()),
                ("mythic".to_string(), [ability("Marks them twice.")].into()),
                (
                    "lfr".to_string(),
                    [ability("Marks players within 30 yards.")].into(),
                ),
            ]
            .into(),
        };
        let file = text_file(&texts, "this machine's").unwrap();
        assert_eq!(
            file,
            "# this machine's\nschema = 1\n\
             \n[ability.mark-of-blood]\ntext = \"Marks players within 40 yards.\"\n\
             \n[difficulty.lfr.ability.mark-of-blood]\ntext = \"Marks players within 30 yards.\"\n\
             \n[difficulty.mythic.ability.mark-of-blood]\ntext = \"Marks them twice.\"\n"
        );
        // Read back, it holds each difficulty that reads otherwise (a
        // keystone, through Mythic, too), and is written as it was.
        let read = crate::text::read(&file).unwrap();
        assert_eq!(read.ability, texts.ability);
        let names: Vec<&str> = read.difficulty.keys().map(String::as_str).collect();
        assert_eq!(names, ["keystone", "lfr", "mythic"]);
        assert_eq!(read.reading(Some(8)), &texts.difficulty["mythic"]);
        assert_eq!(read.reading(Some(15)), &texts.ability);
        assert_eq!(text_file(&read, "this machine's").unwrap(), file);

        // A rubric of one encounter, the sidecar beside it on this machine.
        let draft = "schema = 1\nencounter = 3445\nname = \"Entombed Sentinels\"\n\
                     [ability.mark-of-blood]\nname = \"Mark of Blood\"\n\
                     [ability.berserk]\nname = \"Berserk\"\n";
        let files = vec![
            (
                "s/season.toml".to_string(),
                "schema = 1\nname = \"S\"\norder = 1\n".to_string(),
            ),
            (
                "s/raid/instance.toml".to_string(),
                instance_text("once", "The Raid", "raid", 1, 2),
            ),
            (
                "s/raid/3445-entombed-sentinels.draft.toml".to_string(),
                draft.to_string(),
            ),
        ];
        let root = std::env::temp_dir().join(format!(
            "wowdps-encounter-rubric-text-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("s/raid")).unwrap();
        std::fs::write(root.join("s/raid/3445-entombed-sentinels.toml"), &file).unwrap();
        let r = Rubric::from_files(files).unwrap_or_else(|e| panic!("{e:?}"));
        let text = |r: &Rubric, d: Option<u32>, key: &str| {
            r.encounter(3445, d)
                .unwrap()
                .unwrap()
                .ability
                .get(key)
                .unwrap()
                .text
                .clone()
        };
        assert_eq!(text(&r, None, "mark-of-blood"), None);
        let with = r.clone().with_texts(&root);
        for (d, want) in [
            (None, base),
            (Some(15), base),
            (Some(16), "Marks them twice."),
            (Some(17), "Marks players within 30 yards."),
        ] {
            assert_eq!(
                text(&with, d, "mark-of-blood").as_deref(),
                Some(want),
                "{d:?}"
            );
        }
        assert_eq!(text(&with, Some(16), "berserk"), None);
        // Read once: the rubric keeps what it read after the file is gone.
        std::fs::remove_file(root.join("s/raid/3445-entombed-sentinels.toml")).unwrap();
        assert_eq!(
            text(&with, Some(8), "mark-of-blood").as_deref(),
            Some("Marks them twice.")
        );
        assert!(with.text_errors().is_empty(), "{:?}", with.text_errors());
        // No sidecar for it on this machine: the text stays unsaid.
        let elsewhere = r.clone().with_texts(root.join("nowhere"));
        assert_eq!(text(&elsewhere, Some(16), "mark-of-blood"), None);
        assert!(elsewhere.text_errors().is_empty());
        // One that cannot be read leaves it unsaid too, and says why.
        std::fs::write(
            root.join("s/raid/3445-entombed-sentinels.toml"),
            "schema = 1\n[difficulty.mythc.ability.berserk]\ntext = \"x\"\n",
        )
        .unwrap();
        let broken = r.with_texts(&root);
        assert_eq!(text(&broken, Some(16), "mark-of-blood"), None);
        assert!(
            broken.text_errors().len() == 1
                && broken.text_errors()[0].contains("3445-entombed-sentinels.toml")
                && broken.text_errors()[0].contains("mythc"),
            "{:?}",
            broken.text_errors()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
