use super::*;

const SEASON: &str = r#"
schema = 1
name = "Test Season"
order = 2
instances = [100]

[defaults.view]
zoom = 1.15

[defaults.map]
ceiling = 25
"#;

const INSTANCE: &str = r#"
schema = 1
name = "The Test Raid"
kind = "raid"
map = 3004
"#;

const DRAFT: &str = r#"
schema = 1
encounter = 3492
name = "Ula'tek"
order = 8

[map]
ui_map = 2610
floor = "ulatek"

[npc.ulatek]
name = "Ula'tek"
role = "boss"
creature = [248711]

[npc.coagulation]
name = "Venom Coagulation"
owner = "ulatek"

[ability.bite]
name = "Serpent's Bite"
spell = [1, 2]
by = "ulatek"
shape = { kind = "cone", angle = 60, length = 35 }

[ability.mythic-only]
name = "Only On Mythic"
only = ["mythic"]

[phase.one]
name = "Stage One"
enter = ["start"]

[phase.two]
name = "Stage Two"
order = 1
enter = [{ plateau = { npc = "ulatek", secs = 30 } }]
"#;

const TUNED: &str = r#"
schema = 1

[view]
turn = 90

[npc.coagulation]
name = "Venom Coagulation"
role = "pet"

[map.layer.platform]
wmo = 7549724
breaks = { phase = "two" }

[event.shattering]
on = { cast = { spell = 77 } }

[place.pool]
kind = "pool"
at = [1575, 0]
liquid_height = 388
from = { event = "shattering" }

[difficulty.heroic.ability.bite]
shape = { length = 40 }

[difficulty.mythic.ability.bite]
shape = { length = 45 }

[difficulty.keystone.npc.coagulation]
enabled = false
"#;

fn files(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v = vec![
        ("s/season.toml".to_string(), SEASON.to_string()),
        ("s/raid/instance.toml".to_string(), INSTANCE.to_string()),
        (
            "s/raid/3492-ulatek.draft.toml".to_string(),
            DRAFT.to_string(),
        ),
        ("s/raid/3492-ulatek.toml".to_string(), TUNED.to_string()),
    ];
    v.extend(extra.iter().map(|(p, t)| (p.to_string(), t.to_string())));
    v
}

fn ulatek(difficulty: Option<u32>) -> Encounter {
    let r = Rubric::from_files(files(&[])).unwrap();
    r.encounter(3492, difficulty).unwrap().unwrap()
}

#[test]
fn an_encounter_lays_its_season_draft_and_tuned_file_in_order() {
    let e = ulatek(None);
    assert_eq!(e.name, "Ula'tek");
    assert_eq!(e.instance, "raid");
    // The season's defaults, the draft's map, the tuned file's view.
    assert_eq!(e.view.zoom, Some(1.15));
    assert_eq!(e.view.turn, Some(90.0));
    assert_eq!(e.map.ceiling, Some(25.0));
    assert_eq!(e.map.floor.as_deref(), Some("ulatek"));
    // The tuned file changes one key of an entry and keeps the rest.
    let c = &e.npc["coagulation"];
    assert_eq!((c.role, c.owner.as_deref()), (Role::Pet, Some("ulatek")));
    assert_eq!(e.map.layer["platform"].breaks, Breaks::Phase("two".into()));
    assert_eq!(
        e.place["pool"].from,
        Some(Trigger::Event("shattering".into()))
    );
    assert!(check(&e).is_empty(), "{:?}", check(&e));
}

#[test]
fn a_difficulty_reads_what_it_falls_back_to_then_its_own() {
    let length = |e: &Encounter| match &e.ability["bite"].shape {
        Some(Shape::Cone { length, angle }) => (*length, *angle),
        s => panic!("{s:?}"),
    };
    // No difficulty: the draft's cone.
    assert_eq!(length(&ulatek(None)), (35.0, 60.0));
    // Normal has no overrides; Heroic its own; Mythic over Heroic's.
    assert_eq!(length(&ulatek(Some(14))), (35.0, 60.0));
    assert_eq!(length(&ulatek(Some(15))), (40.0, 60.0));
    assert_eq!(length(&ulatek(Some(16))), (45.0, 60.0));
    // A keystone reads Mythic's and drops what it switches off.
    let k = ulatek(Some(8));
    assert_eq!(length(&k), (45.0, 60.0));
    assert!(!k.npc.contains_key("coagulation"));
}

#[test]
fn an_entry_limited_to_a_difficulty_appears_only_there() {
    assert!(ulatek(None).ability.contains_key("mythic-only"));
    assert!(!ulatek(Some(15)).ability.contains_key("mythic-only"));
    assert!(ulatek(Some(16)).ability.contains_key("mythic-only"));
    // Presence is exact, not inherited: a keystone has what names it (the
    // journal lists `[8, 23]` where both have it).
    assert!(!ulatek(Some(8)).ability.contains_key("mythic-only"));
}

#[test]
fn a_typo_is_an_error_that_names_its_file_and_key() {
    let tuned = format!("{TUNED}\n[view.extra]\nzom = 1\n");
    let mut f = files(&[]);
    f[3].1 = tuned;
    let r = Rubric::from_files(f).unwrap();
    let e = r.encounter(3492, None).unwrap().unwrap_err();
    assert!(
        e.contains("s/raid/3492-ulatek") && e.contains("extra"),
        "{e}"
    );
    let mut f = files(&[]);
    f[3].1 = "schema = 1\n[difficulty.mythc]\n".to_string();
    let r = Rubric::from_files(f).unwrap();
    let e = r.encounter(3492, Some(16)).unwrap().unwrap_err();
    assert!(e.contains("mythc"), "{e}");
}

/// A key a trigger's table does not know is an error that names it and
/// where it is, as a struct's is: `{ cast = { spell = 1, bye = "boss" } }`
/// is no cast by anyone. In an event, in a ground's `at` list, in a clock's
/// `since`.
#[test]
fn a_typo_inside_a_triggers_table_is_an_error_that_names_it() {
    for (entry, key, path) in [
        (
            "[event.typo]\non = { cast = { spell = 1, bye = \"boss\" } }",
            "bye",
            "event.typo.on",
        ),
        (
            "[ground.typo]\nkind = \"pool\"\nshape = { kind = \"circle\", radius = 3 }\n\
             at = [{ aura_applied = { spell = 1, onn = \"boss\" } }]",
            "onn",
            "ground.typo.at",
        ),
        (
            "[event.typo]\non = { after = { secs = 1, since = { hit = { spell = 1, by = \"boss\" } } } }",
            "by",
            "event.typo.on.since",
        ),
        (
            "[event.typo]\non = { health_below = { npc = \"boss\", pc = 50 } }",
            "pc",
            "event.typo.on",
        ),
    ] {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n{entry}\n"
        );
        let err = Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
            .unwrap_err();
        assert!(
            err.contains(&format!("unknown field `{key}`"))
                && err.contains("7-x")
                && err.contains(&format!("`{path}`")),
            "{err}"
        );
    }
}

#[test]
fn a_newer_or_unversioned_file_is_refused() {
    for n in [SCHEMA + 1, 99] {
        let newer = files(&[("s/raid/9-x.toml", &format!("schema = {n}\n"))]);
        let errs = Rubric::from_files(newer).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.contains("9-x") && e.contains("newer")),
            "{errs:?}"
        );
    }
    let none = files(&[("s/raid/9-x.toml", "name = \"x\"\n")]);
    let errs = Rubric::from_files(none).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("schema")), "{errs:?}");
    let older = files(&[("s/raid/9-x.toml", "schema = 0\n")]);
    let errs = Rubric::from_files(older).unwrap_err();
    assert!(
        errs.iter().any(|e| e.contains("schema 0 has no migration")),
        "{errs:?}"
    );
}

#[test]
fn an_older_file_is_brought_to_this_builds_format() {
    // The test files are written in 1: they read, and as the format this
    // build writes. A file in 2 reads alike.
    assert_eq!(SCHEMA, 2);
    let e = ulatek(None);
    assert_eq!(e.schema, SCHEMA);
    let mut f = files(&[]);
    for (_, text) in &mut f {
        *text = text.replace("schema = 1\n", "schema = 2\n");
    }
    let r = Rubric::from_files(f).unwrap();
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), e);
    let t = migrate("schema = 1\nname = \"x\"\n".parse().unwrap()).unwrap();
    assert_eq!(t, "schema = 2\nname = \"x\"\n".parse::<Table>().unwrap());
}

#[test]
fn a_dangling_name_is_found() {
    let dangling = files(&[(
        "s/raid/7-x.toml",
        "schema = 1\nencounter = 7\nname = \"X\"\n[ability.a]\nname = \"A\"\nby = \"nobody\"\n[phase.p]\nname = \"P\"\nenter = [{ event = \"never\" }]\n",
    )]);
    let r = Rubric::from_files(dangling).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    let problems = check(&e);
    assert_eq!(problems.len(), 2, "{problems:?}");
}

#[test]
fn a_clock_counts_from_the_pull_or_from_the_nth_time_a_trigger_fired() {
    let tuned = format!(
        "{TUNED}
[event.cracks]
on = {{ after = 353 }}

[event.flung]
on = {{ after = {{ secs = 68.5, since = {{ cast = {{ spell = 9, by = \"ulatek\" }} }}, nth = 2 }} }}

[event.after-it]
on = {{ after = {{ secs = 3, since = {{ event = \"shattering\" }} }} }}

[difficulty.mythic.event.flung]
on = {{ after = {{ secs = 78.5 }} }}

[difficulty.mythic.event.cracks]
on = {{ after = {{ secs = 1, since = \"start\" }} }}
"
    );
    let mut f = files(&[]);
    f[3].1 = tuned;
    let r = Rubric::from_files(f).unwrap();
    let at = |d: Option<u32>| r.encounter(3492, d).unwrap().unwrap();
    let rage = Some(Box::new(Trigger::Cast {
        spell: 9,
        by: Some("ulatek".into()),
    }));
    // Seconds into the pull, as a number; seconds after a trigger's nth
    // firing, as a table, the first when it says no `nth`.
    let e = at(None);
    assert_eq!(e.event["cracks"].on, Trigger::After(Clock::pull(353.0)));
    assert_eq!(
        e.event["flung"].on,
        Trigger::After(Clock {
            secs: 68.5,
            since: rage.clone(),
            nth: Some(2),
            each: false,
        })
    );
    assert_eq!(
        e.event["after-it"].on,
        Trigger::After(Clock {
            secs: 3.0,
            since: Some(Box::new(Trigger::Event("shattering".into()))),
            nth: None,
            each: false,
        })
    );
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // A later layer's `secs` retimes a clock and keeps what it counts from;
    // its table replaces a number whole.
    let m = at(Some(16));
    assert_eq!(
        m.event["flung"].on,
        Trigger::After(Clock {
            secs: 78.5,
            since: rage,
            nth: Some(2),
            each: false,
        })
    );
    assert_eq!(
        m.event["cracks"].on,
        Trigger::After(Clock {
            secs: 1.0,
            since: Some(Box::new(Trigger::Start)),
            nth: None,
            each: false,
        })
    );
    // Written back as it was read: a number from the pull, else the table
    // with `nth` only where it is not the first.
    for (t, text) in [
        (&e.event["cracks"].on, "after = 353.0"),
        (
            &e.event["after-it"].on,
            "after = { secs = 3.0, since = { event = \"shattering\" } }",
        ),
    ] {
        let v = toml::Value::try_from(t).unwrap();
        assert_eq!(v, text.parse::<toml::Table>().unwrap().into());
    }
    // A key the table does not know is an error that names it.
    let mut f = files(&[]);
    f[3].1 = format!(
        "{TUNED}\n[event.typo]\non = {{ after = {{ secs = 1, since = \"start\", nht = 2 }} }}\n"
    );
    let r = Rubric::from_files(f).unwrap();
    let err = r.encounter(3492, None).unwrap().unwrap_err();
    assert!(err.contains("nht"), "{err}");
    // A clock counting from an NPC the encounter lacks, from its 0th
    // firing, or to a time under 0, is found.
    let mut f = files(&[]);
    f[3].1 = format!(
        "{TUNED}
[event.astray]
on = {{ after = {{ secs = -1, since = {{ cast = {{ spell = 9, by = \"nobody\" }} }}, nth = 0 }} }}
"
    );
    let r = Rubric::from_files(f).unwrap();
    let problems = check(&r.encounter(3492, None).unwrap().unwrap());
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.contains("event.astray")),
        "{problems:?}"
    );
}

#[test]
fn the_base_leaves_the_tuned_file_and_its_difficulty_overrides_out() {
    let r = Rubric::from_files(files(&[])).unwrap();
    let base = |d: Option<u32>| r.encounter_at(3492, d, Tier::Base).unwrap().unwrap();
    let e = base(None);
    // The season's defaults and the draft's map stand; the tuned file's
    // view, layer, event and place do not.
    assert_eq!(e.view.zoom, Some(1.15));
    assert_eq!(e.map.floor.as_deref(), Some("ulatek"));
    assert_eq!(e.view.turn, None);
    assert_ne!(e.npc["coagulation"].role, Role::Pet);
    assert!(e.map.layer.is_empty() && e.event.is_empty() && e.place.is_empty());
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // Its difficulties read the draft's cone, and a keystone keeps what the
    // tuned file switches off there.
    let cone = |e: &Encounter| match &e.ability["bite"].shape {
        Some(Shape::Cone { length, .. }) => *length,
        s => panic!("{s:?}"),
    };
    assert_eq!(cone(&base(Some(16))), 35.0);
    assert!(base(Some(8)).npc.contains_key("coagulation"));
    // The draft's own difficulty limits still hold.
    assert!(!base(Some(15)).ability.contains_key("mythic-only"));
    assert!(base(Some(16)).ability.contains_key("mythic-only"));
}

#[test]
fn the_rubric_tier_is_the_most_it_answers() {
    let r = Rubric::from_files(files(&[])).unwrap();
    // Everything the machine has, unless lowered.
    assert_eq!(r.tier(), Tier::User);
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), ulatek(None));
    let r = r.with_tier(Tier::Base);
    let base = r.encounter_at(3492, None, Tier::Base).unwrap().unwrap();
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), base);
    for tier in [Tier::Curated, Tier::User] {
        assert_eq!(r.encounter_at(3492, None, tier).unwrap().unwrap(), base);
    }
}

#[test]
fn a_tier_is_named_and_ordered() {
    for tier in Tier::ALL {
        assert_eq!(Tier::from_name(tier.name()), Some(tier));
    }
    assert_eq!(
        Tier::ALL.map(Tier::name),
        ["base", "curated", "user"],
        "the lesser first"
    );
    assert!(Tier::Base < Tier::Curated && Tier::Curated < Tier::User);
    assert_eq!(Tier::default(), Tier::User);
    assert_eq!(Tier::from_name("tuned"), None);
}

#[test]
fn an_encounter_without_a_tuned_file_has_no_curated_layer() {
    let only_tuned = r#"
schema = 1
encounter = 3420
name = "Tuned Alone"
"#;
    let r = Rubric::from_files(files(&[
        (
            "s/raid/3421-draft-alone.draft.toml",
            "schema = 1\nencounter = 3421\nname = \"Draft Alone\"\n",
        ),
        ("s/raid/3420-tuned-alone.toml", only_tuned),
    ]))
    .unwrap();
    assert!(r.has_curated(3492));
    assert!(!r.has_curated(3421));
    // The base and the curated answers of a draft alone are the same.
    assert_eq!(
        r.encounter_at(3421, None, Tier::Base).unwrap().unwrap(),
        r.encounter_at(3421, None, Tier::Curated).unwrap().unwrap()
    );
    // A tuned file with no draft under it has no base.
    assert!(r.encounter_at(3420, None, Tier::Base).is_none());
    assert!(r.encounter_at(3420, None, Tier::Curated).is_some());
}

#[test]
fn the_version_changes_with_any_file() {
    let a = Rubric::from_files(files(&[])).unwrap();
    let mut f = files(&[]);
    f[3].1.push_str("\n# a comment\n");
    let b = Rubric::from_files(f).unwrap();
    assert_ne!(a.version(), b.version());
    assert!(a.version().starts_with("s+"));
}

/// The embedded files are the crate's `seasons/` as it stands, each by
/// its path under it (`<season>/season.toml`, `<season>/<instance>/…`), in
/// path order.
#[test]
fn the_embedded_files_are_the_seasons_on_disk() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("seasons");
    let files = embedded_files();
    assert!(!files.is_empty());
    assert!(files.windows(2).all(|w| w[0].0 < w[1].0));
    for (rel, text) in files {
        let depth = rel.split('/').count();
        assert!(
            (depth == 2 && rel.ends_with("/season.toml")) || depth == 3,
            "{rel}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(rel)).unwrap(),
            *text,
            "{rel}"
        );
    }
}

/// The embedded tree is the base alone: no file in it is one the
/// classifier calls tuned or trash (`FileRole::of`), whose place is the
/// repository's `curated/seasons/`. A stray one (an editor on an old pin
/// saving where the set lay before it moved) would be embedded as the
/// base's own curated tier; this refuses it, naming it and where it goes.
#[test]
fn the_embedded_base_holds_no_tuned_or_trash_file() {
    let stray: Vec<String> = embedded_files()
        .iter()
        .filter(|(rel, _)| {
            matches!(
                FileRole::of(rel),
                Ok(Some(
                    FileRole::Trash { .. } | FileRole::Encounter { draft: false, .. }
                ))
            )
        })
        .map(|(rel, _)| {
            format!("seasons/{rel}: a tuned or trash file belongs under curated/seasons/{rel}")
        })
        .collect();
    assert!(stray.is_empty(), "{}", stray.join("\n"));
}

#[test]
fn the_embedded_rubric_resolves_every_encounter_on_every_difficulty() {
    let r = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let mut problems = Vec::new();
    for (id, _, _) in r.encounters() {
        for d in [
            None,
            Some(1),
            Some(2),
            Some(23),
            Some(8),
            Some(14),
            Some(15),
            Some(16),
            Some(17),
        ] {
            // The base alone must stand too: nothing in it may lean on a
            // tuned file. (This repository carries none, so every tier
            // reads the base; a tree that holds tuned files checks them.)
            for tier in Tier::ALL {
                match r.encounter_at(id, d, tier) {
                    Some(Ok(e)) => problems.extend(
                        check(&e)
                            .into_iter()
                            .map(|p| format!("{id} {d:?} {}: {p}", tier.name())),
                    ),
                    Some(Err(e)) => problems.push(format!("{}: {e}", tier.name())),
                    None => problems.push(format!("{id} {d:?}: no {} layer", tier.name())),
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every instance's own creatures check clean, and no season gives one map
/// id twice: `instance_by_map` finds one instance by it.
#[test]
fn the_embedded_rubric_checks_every_instances_creatures() {
    let r = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let mut problems = Vec::new();
    for s in r.seasons() {
        let mut maps = BTreeMap::new();
        for i in s.instances.values() {
            problems.extend(
                check_instance(i)
                    .into_iter()
                    .map(|p| format!("{}/{}: {p}", s.id, i.slug)),
            );
            if let Some(m) = i.map
                && let Some(other) = maps.insert(m, &i.slug)
            {
                problems.push(format!("{}: map {m} is {other}'s and {}'s", s.id, i.slug));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // Ruby Life Pools, the first dungeon given its creatures: found by the
    // map id its runs' logs write, its whole forces, a creature by id, a
    // summon's owner; its bosses are its encounters'.
    let (_, ruby) = r.instance_by_map(2521).unwrap();
    assert_eq!(
        (ruby.slug.as_str(), ruby.forces()),
        ("ruby-life-pools", Some(551))
    );
    let (key, n) = ruby.npc_of(188244).unwrap();
    assert_eq!((key, n.forces), ("primal-juggernaut", Some(25)));
    let (key, n) = ruby.npc_of(194622).unwrap();
    assert_eq!(
        (key, n.owner.as_deref()),
        ("summoned-scorchling", Some("blazebound-destroyer"))
    );
    assert!(ruby.npc_of(188252).is_none());
}

#[test]
fn a_ground_lands_where_its_unit_stood_and_one_that_names_none_is_found() {
    let grounds = files(&[(
        "s/raid/7-x.toml",
        r#"schema = 1
encounter = 7
name = "X"

[npc.boss]
name = "Boss"

[ground.pool]
kind = "pool"
tint = "red"
shape = { kind = "circle", radius = 6 }
at = [{ aura_removed = { spell = 11 } }, { aura_removed = { spell = 12 } }]
delay = 1
grow = "stacks"

[ground.droplet]
kind = "soak"
shape = { kind = "circle", radius = 1.5 }
at = [{ hit = { spell = 21 } }]
from = { cast = { spell = 20, by = "boss" } }
ends = "at"

[ground.fading]
kind = "puddle"
shape = { kind = "circle", radius = 3 }
at = [{ death = { npc = "boss" } }]
ends = { after = 30 }

[ground.nowhere]
kind = "pool"
shape = { kind = "circle", radius = 3 }
at = [{ after = 30 }]
"#,
    )]);
    let r = Rubric::from_files(grounds).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    let pool = &e.ground["pool"];
    assert_eq!(pool.at.len(), 2);
    assert_eq!((pool.delay, pool.grow), (Some(1.0), Some(Grow::Stacks)));
    assert_eq!(pool.ends, Ends::Pull);
    assert_eq!(e.ground["droplet"].ends, Ends::At);
    assert_eq!(
        e.ground["droplet"].from,
        Some(Trigger::Cast {
            spell: 20,
            by: Some("boss".into())
        })
    );
    assert_eq!(e.ground["fading"].ends, Ends::After(30.0));
    // A clock names no one to stand on.
    let problems = check(&e);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("ground.nowhere"), "{problems:?}");
}

/// A cast's start (`SPELL_CAST_START`) is a trigger like a cast's landing:
/// spelled `cast_start`, its `by` an NPC the encounter has, about its
/// caster so a ground lands there, and written back as it was read.
#[test]
fn a_cast_start_is_a_trigger_about_its_caster() {
    let text = |by: &str| {
        format!(
            r#"schema = 1
encounter = 7
name = "X"

[npc.boss]
name = "Boss"

[phase.one]
name = "One"
enter = ["start"]

[phase.two]
name = "Two"
order = 1
enter = [{{ cast_start = {{ spell = 30, by = "{by}" }} }}]

[event.winds-up]
on = {{ cast_start = {{ spell = 31 }} }}

[event.lands]
on = {{ after = {{ secs = 3, since = {{ cast_start = {{ spell = 31, by = "{by}" }} }} }} }}

[ground.frontal]
kind = "soak"
shape = {{ kind = "cone", angle = 75, length = 30 }}
at = [{{ cast_start = {{ spell = 32, by = "{by}" }} }}]
ends = {{ hit = {{ spell = 33 }} }}
"#
        )
    };
    let read = |t: &str| {
        Rubric::from_files(files(&[("s/raid/7-x.toml", t)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let e = read(&text("boss")).unwrap();
    let start = |spell: u32, by: Option<&str>| Trigger::CastStart {
        spell,
        by: by.map(str::to_string),
    };
    assert_eq!(e.phase["two"].enter, [start(30, Some("boss"))]);
    assert_eq!(e.event["winds-up"].on, start(31, None));
    assert_eq!(
        e.event["lands"].on,
        Trigger::After(Clock {
            secs: 3.0,
            since: Some(Box::new(start(31, Some("boss")))),
            nth: None,
            each: false,
        })
    );
    // It names its caster, so a ground can stand where the caster stood.
    assert_eq!(e.ground["frontal"].at, [start(32, Some("boss"))]);
    assert!(start(32, None).marks_a_spot());
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // Written back as it reads, and read again to the same encounter.
    let written = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    for line in [
        "\nenter = [{ cast_start = { spell = 30, by = \"boss\" } }]\n",
        "\non = { cast_start = { spell = 31 } }\n",
        "\non = { after = { secs = 3.0, since = { cast_start = { spell = 31, by = \"boss\" } } } }\n",
        "\nat = [{ cast_start = { spell = 32, by = \"boss\" } }]\n",
    ] {
        assert!(written.contains(line), "{line}\n{written}");
    }
    assert_eq!(read(&written).unwrap(), e);
    // A `by` naming an NPC the encounter lacks is found, wherever the
    // trigger stands, as a cast's is.
    let e = read(&text("nobody")).unwrap();
    let problems = check(&e);
    assert_eq!(
        problems,
        [
            "phase.two names npc \"nobody\", which the encounter does not have",
            "ground.frontal names npc \"nobody\", which the encounter does not have",
            "event.lands names npc \"nobody\", which the encounter does not have",
        ],
        "{problems:?}"
    );
}

#[test]
fn a_ground_can_end_at_the_first_hit_inside_it() {
    let burst = |ends: &str| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [ground.burst]\nkind = \"pool\"\nshape = {{ kind = \"circle\", radius = 3 }}\n\
             at = [{{ death = {{ npc = \"boss\" }} }}]\nends = {ends}\n"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    // Spelled as a `hit` trigger is, and written back so.
    let e = burst("{ hit = { spell = 384024 } }").unwrap();
    assert_eq!(
        e.ground["burst"].ends,
        Ends::Hit {
            spell: 384024,
            lands: false
        }
    );
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nends = { hit = { spell = 384024 } }\n"),
        "{text}"
    );
    // Spell 0 is none; a key a hit ending lacks is a typo.
    let e = burst("{ hit = { spell = 0 } }").unwrap();
    assert_eq!(
        check(&e),
        ["ground.burst ends at a hit of spell 0, which is none"]
    );
    let err = burst("{ hit = { spell = 1, by = \"boss\" } }").unwrap_err();
    assert!(err.contains("by"), "{err}");
    // Tables merge key by key, so a layer cannot switch a timed ending to a
    // hit, nor a hit to a timed one: the two forms merge into a table that
    // names both, which is refused, on a difficulty as anywhere. A word
    // replaces either whole, and either replaces a word.
    let layered = |draft: &str, tuned: &str, difficulty: Option<u32>| {
        let ground = |ends: &str| {
            format!(
                "[ground.burst]\nkind = \"pool\"\nshape = {{ kind = \"circle\", radius = 3 }}\n\
                 at = [{{ death = {{ npc = \"boss\" }} }}]\nends = {ends}\n"
            )
        };
        let draft = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n{}",
            ground(draft)
        );
        let tuned = format!("schema = 1\n{tuned}");
        Rubric::from_files(files(&[
            ("s/raid/7-x.draft.toml", &draft),
            ("s/raid/7-x.toml", &tuned),
        ]))
        .unwrap()
        .encounter(7, difficulty)
        .unwrap()
        .map(|e| e.ground["burst"].ends.clone())
    };
    let over = |ends: &str| format!("[ground.burst]\nends = {ends}\n");
    let mythic = |ends: &str| format!("[difficulty.mythic.ground.burst]\nends = {ends}\n");
    let hit = "{ hit = { spell = 9 } }";
    assert!(layered("{ after = 30 }", &over(hit), None).is_err());
    assert!(layered(hit, &over("{ after = 30 }"), None).is_err());
    assert!(layered("{ after = 30 }", &mythic(hit), Some(16)).is_err());
    assert_eq!(
        layered("{ after = 30 }", &mythic(hit), None),
        Ok(Ends::After(30.0))
    );
    assert_eq!(
        layered(hit, &over("{ hit = { spell = 10 } }"), None),
        Ok(Ends::Hit {
            spell: 10,
            lands: false
        })
    );
    assert_eq!(layered(hit, &over("\"pull\""), None), Ok(Ends::Pull));
    assert_eq!(
        layered("\"at\"", &over(hit), None),
        Ok(Ends::Hit {
            spell: 9,
            lands: false
        })
    );
    // `lands`: its landing hits alone, read and written back where it is
    // said, and laid by a later layer onto the spell an earlier one gave.
    assert_eq!(
        layered(hit, &over("{ hit = { lands = true } }"), None),
        Ok(Ends::Hit {
            spell: 9,
            lands: true
        })
    );
    let e = burst("{ hit = { spell = 384024, lands = true } }").unwrap();
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nends = { hit = { spell = 384024, lands = true } }\n"),
        "{text}"
    );
}

/// `ends = { first = [...] }`: gone at the first of its hits and times,
/// read and written back on one line, its `after` the earliest of its
/// times and its hits every `hit`; refused empty, nested, or holding a
/// word; laid whole by a later layer, and never laid onto another form.
#[test]
fn a_ground_can_end_at_the_first_of_a_hit_and_a_time() {
    let orb = |ends: &str, tuned: Option<&str>| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[ground.orb]\nkind = \"soak\"\n\
             shape = {{ kind = \"circle\", radius = 2 }}\nat = [{{ hit = {{ spell = 1 }} }}]\n\
             ends = {ends}\n"
        );
        let mut f = vec![("s/raid/7-x.draft.toml", text)];
        if let Some(t) = tuned {
            f.push((
                "s/raid/7-x.toml",
                format!("schema = 1\n[ground.orb]\nends = {t}\n"),
            ));
        }
        let f: Vec<(&str, &str)> = f.iter().map(|(p, t)| (*p, t.as_str())).collect();
        Rubric::from_files(files(&f))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let first =
        "{ first = [{ hit = { spell = 1313448, lands = true } }, { after = 40 }, { after = 30 }] }";
    let e = orb(first, None).unwrap();
    let ends = &e.ground["orb"].ends;
    assert_eq!(ends.after(), Some(30.0));
    assert_eq!(ends.hits(), [(1313448, true)]);
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nends = { first = [{ hit = { spell = 1313448, lands = true } }, { after = 40.0 }, { after = 30.0 }] }\n"),
        "{text}"
    );
    let problems = |ends: &str| check(&orb(ends, None).unwrap());
    assert_eq!(
        problems("{ first = [] }"),
        ["ground.orb ends at the first of nothing"]
    );
    assert_eq!(
        problems(
            "{ first = [{ first = [{ after = 3 }] }, \"at\", \"pull\", { hit = { spell = 0 } }] }"
        ),
        [
            "ground.orb ends at a hit of spell 0, which is none",
            "ground.orb ends at the first of a `first`; give its endings in one list",
            "ground.orb ends at the first of \"at\", which is no ending to race: give a `hit` or an `after`",
            "ground.orb ends at the first of \"pull\", which is no ending to race: give a `hit` or an `after`",
        ]
    );
    // A later layer gives the whole list again; laid over a hit, the two
    // forms name both and are refused, as an `after` over a hit is.
    let e = orb(first, Some("{ first = [{ after = 12 }] }")).unwrap();
    assert_eq!(e.ground["orb"].ends, Ends::First(vec![Ends::After(12.0)]));
    assert!(orb("{ hit = { spell = 9 } }", Some(first)).is_err());
}

/// A `hit` trigger's `lands`: unsaid, every hit; said, its landings alone,
/// read and written back on the trigger's one line.
#[test]
fn a_hit_trigger_can_keep_to_its_landings() {
    let text = "schema = 1\nencounter = 7\nname = \"X\"\n\n[ground.frost]\nkind = \"pool\"\n\
                shape = { kind = \"circle\", radius = 8 }\n\
                at = [{ hit = { spell = 1313448, lands = true } }, { hit = { spell = 5 } }]\n";
    let e = Rubric::from_files(files(&[("s/raid/7-x.toml", text)]))
        .unwrap()
        .encounter(7, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        e.ground["frost"].at,
        [
            Trigger::Hit {
                spell: 1313448,
                lands: true
            },
            Trigger::Hit {
                spell: 5,
                lands: false
            }
        ]
    );
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains(
            "\nat = [{ hit = { spell = 1313448, lands = true } }, { hit = { spell = 5 } }]\n"
        ),
        "{text}"
    );
}

/// `renew`: a firing inside one standing renews it rather than placing
/// another. Unsaid, false and not written; said, read and written back,
/// and refused beside a `from`, an `"at"` ending and a `seen` spell that
/// also places it; a push beside it is allowed.
#[test]
fn a_ground_a_firing_inside_renews_is_read_written_and_checked() {
    let residue = |more: &str| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [ground.residue]\nkind = \"pool\"\nshape = {{ kind = \"circle\", radius = 6 }}\n\
             at = [{{ hit = {{ spell = 1 }} }}, {{ hit = {{ spell = 2 }} }}]\n{more}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let e = residue("").unwrap();
    assert!(!e.ground["residue"].renew);
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(!text.contains("renew"), "{text}");
    let e = residue("ends = { after = 5 }\nrenew = true\n").unwrap();
    assert!(e.ground["residue"].renew);
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nends = { after = 5.0 }\nrenew = true\n"),
        "{text}"
    );
    let back = Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
        .unwrap()
        .encounter(7, None)
        .unwrap()
        .unwrap();
    assert_eq!(back.ground["residue"], e.ground["residue"]);
    // It is a flag: anything else is refused.
    let err = residue("renew = 1\n").unwrap_err();
    assert!(err.contains("renew") || err.contains("bool"), "{err}");
    // A `from` stands one before its firing, an `"at"` ending leaves none
    // standing.
    let e =
        residue("renew = true\nends = \"at\"\nfrom = { cast = { spell = 3, by = \"boss\" } }\n")
            .unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.residue both renews and stands `from` a trigger; it does one",
            "ground.residue renews but ends as `at` fires, so none stands to renew",
        ]
    );
    // A push is read with it: each firing finds the grounds where the
    // pushes have taken them.
    let e = residue(
        "renew = true\npush = { on = { hit = { spell = 1 } }, toward = [0], speed = 1, secs = 1 }\n",
    )
    .unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // A hit that renews it and finds it again would drag it to each victim;
    // a `seen` spell it is not placed by finds it as ever.
    let e = residue("renew = true\nseen = [2, 3]\n").unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.residue renews on hits of spell 2 that its `seen` also finds it by, so it would follow its victims; it does one"
        ]
    );
    let e = residue("renew = true\nseen = [3]\n").unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // An orbit is fixed as each forms, so it may renew.
    let e = residue("renew = true\norbit = { around = \"boss\", turn = 3 }\n").unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
}

/// `clears`: the grounds a ground sweeps away as it forms; `sweeps`, those
/// it sweeps away all the while it stands. Unsaid, empty and not written;
/// said, read and written back after `seen`, and refused where it names no
/// ground (an NPC's key among them) or the ground itself, and where two
/// clear each other; a ground that renews or has `seen` hits may be
/// cleared.
#[test]
fn a_ground_that_clears_others_is_read_written_and_checked() {
    let sever = |clears: &str, orb: &str| {
        let text = format!(
            "schema = 2\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [ground.orb]\nkind = \"soak\"\nshape = {{ kind = \"circle\", radius = 2 }}\n\
             at = [{{ appears = {{ npc = \"boss\" }} }}]\n{orb}\n\
             [ground.sever]\nkind = \"pool\"\nshape = {{ kind = \"cone\", angle = 60, length = 20 }}\n\
             at = [{{ cast = {{ spell = 1, by = \"boss\" }} }}]\nseen = [2]\n{clears}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let e = sever("", "").unwrap();
    assert!(e.ground["sever"].clears.is_empty());
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(!text.contains("clears"), "{text}");
    let e = sever("clears = [\"orb\"]\n", "").unwrap();
    assert_eq!(e.ground["sever"].clears, ["orb"]);
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nseen = [2]\nclears = [\"orb\"]\n"),
        "{text}"
    );
    let back = Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
        .unwrap()
        .encounter(7, None)
        .unwrap()
        .unwrap();
    assert_eq!(back.ground["sever"], e.ground["sever"]);
    // A list of keys: anything else is refused.
    let err = sever("clears = \"orb\"\n", "").unwrap_err();
    assert!(err.contains("clears") || err.contains("sequence"), "{err}");
    // No such ground, an NPC, itself.
    let e = sever("clears = [\"orbs\", \"boss\", \"sever\"]\n", "").unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.sever clears ground \"orbs\", which the encounter does not have",
            "ground.sever clears \"boss\", an NPC: it clears grounds alone (a volley's `destroys` takes NPCs)",
            "ground.sever clears itself",
        ]
    );
    // A ground that renews, or that its `seen` hits find again, is read
    // after what clears it, each clear a moment in its own reading.
    let e = sever("clears = [\"orb\"]\n", "renew = true\nseen = [3]").unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // `sweeps`, the same keys swept all the while it stands: read and
    // written back after `clears`, refused as `clears` is.
    let e = sever("clears = [\"orb\"]\nsweeps = [\"orb\"]\n", "").unwrap();
    assert_eq!(e.ground["sever"].sweeps, ["orb"]);
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nclears = [\"orb\"]\nsweeps = [\"orb\"]\n"),
        "{text}"
    );
    let e = sever("sweeps = [\"orbs\", \"boss\", \"sever\"]\n", "").unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.sever sweeps ground \"orbs\", which the encounter does not have",
            "ground.sever sweeps \"boss\", an NPC: it sweeps grounds alone (a volley's `destroys` takes NPCs)",
            "ground.sever sweeps itself",
        ]
    );
    // Each is read after those that clear it, so two that clear each other,
    // by any way round, are refused, each of them.
    let e = sever("sweeps = [\"orb\"]\n", "clears = [\"sever\"]\n").unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.orb clears or sweeps a ground that clears or sweeps it in turn (orb → sever → orb): each is read after those that clear it",
            "ground.sever clears or sweeps a ground that clears or sweeps it in turn (sever → orb → sever): each is read after those that clear it",
        ]
    );
}

/// `sweeps` names a ground the difficulty lacks as `clears` does: dropped
/// as it resolves, and still refused where it is no ground at all.
#[test]
fn a_ground_sweeps_one_its_difficulty_does_not_have_as_nothing() {
    let text = "schema = 2\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
                [ground.frost]\nkind = \"pool\"\nshape = { kind = \"circle\", radius = 8 }\n\
                at = [{ appears = { npc = \"boss\" } }]\nonly = [\"mythic\"]\n\n\
                [ground.jet]\nkind = \"pool\"\nshape = { kind = \"line\", width = 6, length = 60 }\n\
                at = [{ cast = { spell = 1, by = \"boss\" } }]\nsweeps = [\"frost\", \"frosts\"]\n";
    let r = Rubric::from_files(files(&[("s/raid/7-x.toml", text)])).unwrap();
    for (d, sweeps) in [
        (Some(16), &["frost", "frosts"][..]),
        (Some(15), &["frosts"]),
    ] {
        let e = r.encounter(7, d).unwrap().unwrap();
        assert_eq!(e.ground["jet"].sweeps, sweeps, "{d:?}");
        assert_eq!(
            check(&e),
            ["ground.jet sweeps ground \"frosts\", which the encounter does not have"],
            "{d:?}"
        );
    }
}

/// `clears` may name any ground of the encounter's file: one a difficulty
/// does not have (kept to others, or switched off) clears nothing there,
/// its name gone as the encounter resolves. Mythic Keystone (8) lays the
/// Mythic overlays (its chain runs through Mythic) but keeps a Mythic-only
/// ground out. A name that is no ground on any difficulty is still refused.
#[test]
fn a_ground_clears_one_its_difficulty_does_not_have_as_nothing() {
    let sever = |mutation: &str, own: &str, mythic: &str, d: Option<u32>| {
        let text = format!(
            "schema = 2\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [ground.orb]\nkind = \"soak\"\nshape = {{ kind = \"circle\", radius = 2 }}\n\
             at = [{{ appears = {{ npc = \"boss\" }} }}]\n\n\
             [ground.mutation]\nkind = \"soak\"\nshape = {{ kind = \"circle\", radius = 2 }}\n\
             at = [{{ appears = {{ npc = \"boss\" }} }}]\nonly = [\"mythic\"]\n{mutation}\n\
             [ground.sever]\nkind = \"pool\"\nshape = {{ kind = \"cone\", angle = 60, length = 20 }}\n\
             at = [{{ cast = {{ spell = 1, by = \"boss\" }} }}]\nclears = {own}\n\n\
             [difficulty.mythic.ground.sever]\nclears = {mythic}\n"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, d)
            .unwrap()
            .unwrap()
    };
    let (orb, both) = ("[\"orb\"]", "[\"orb\", \"mutation\"]");
    // The mutation named in a Mythic overlay alone: Mythic (16) has it and
    // clears it; Heroic (15) and the encounter's own lay no overlay, and
    // Mythic Keystone (8) lays it but lacks the ground: the orbs alone.
    for (d, clears) in [
        (Some(16), &["orb", "mutation"][..]),
        (Some(15), &["orb"]),
        (None, &["orb"]),
        (Some(8), &["orb"]),
    ] {
        let e = sever("", orb, both, d);
        let has = matches!(d, Some(16) | None);
        assert_eq!(e.ground.contains_key("mutation"), has, "{d:?}");
        assert_eq!(e.ground["sever"].clears, clears, "{d:?}");
        assert!(check(&e).is_empty(), "{d:?}: {:?}", check(&e));
    }
    // Named in the ground's own `clears`, so on every difficulty: Heroic
    // and Mythic Keystone lack it; the encounter's own has every ground.
    for (d, clears) in [
        (Some(16), &["orb", "mutation"][..]),
        (Some(15), &["orb"]),
        (None, &["orb", "mutation"]),
        (Some(8), &["orb"]),
    ] {
        let e = sever("", both, both, d);
        assert_eq!(e.ground["sever"].clears, clears, "{d:?}");
        assert!(check(&e).is_empty(), "{d:?}: {:?}", check(&e));
    }
    // Switched off, it is cleared by none, on Mythic too.
    for d in [Some(16), None] {
        let e = sever("enabled = false\n", both, both, d);
        assert_eq!(e.ground["sever"].clears, ["orb"], "{d:?}");
        assert!(check(&e).is_empty(), "{d:?}: {:?}", check(&e));
    }
    // A name that is no ground on any difficulty stays, and is refused on
    // each.
    for d in [Some(16), Some(15), None, Some(8)] {
        let e = sever(
            "",
            "[\"orb\", \"orbs\"]",
            "[\"orb\", \"mutation\", \"orbs\"]",
            d,
        );
        assert!(
            e.ground["sever"].clears.iter().any(|c| c == "orbs"),
            "{d:?}"
        );
        assert_eq!(
            check(&e),
            ["ground.sever clears ground \"orbs\", which the encounter does not have"],
            "{d:?}"
        );
    }
}

/// A line or a cone `follows` its cast's target, a player carrying an
/// aura or its unit's facing, written as it reads; a word or a key it
/// lacks is a typo, and `check` refuses it on a circle, beside an orbit or
/// an `ahead`, `"target"` by a trigger that is no cast (an aura's carrier
/// and a hit's victim are the unit it lands on), and an aura of spell 0.
#[test]
fn a_line_follows_its_unit_and_a_bad_follows_is_found() {
    let follows = |shape: &str, at: &str, more: &str| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [place.well]\nkind = \"pool\"\nat = [0, 0]\n\n\
             [ground.jet]\nkind = \"pool\"\nshape = {shape}\nat = [{at}]\n{more}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let line = "{ kind = \"line\", width = 6, length = 60 }";
    let cast = "{ cast = { spell = 5, by = \"boss\" } }";
    let death = "{ death = { npc = \"boss\" } }";
    for (word, want) in [
        ("\"target\"", Follows::Target),
        ("\"facing\"", Follows::Facing),
        ("{ aura = 9 }", Follows::Aura(9)),
    ] {
        let e = follows(line, cast, &format!("follows = {word}\n")).unwrap();
        assert!(check(&e).is_empty(), "{:?}", check(&e));
        assert_eq!(e.ground["jet"].follows, Some(want));
        let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
        assert!(text.contains(&format!("\nfollows = {word}\n")), "{text}");
    }
    // A cone follows as a line does; an NPC dying has a facing to follow.
    let cone = "{ kind = \"cone\", angle = 60, length = 20 }";
    let e = follows(cone, death, "follows = \"facing\"\n").unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // A word it lacks, and a key an aura's table lacks.
    let err = follows(line, cast, "follows = \"tank\"\n").unwrap_err();
    assert!(err.contains("tank"), "{err}");
    assert!(follows(line, cast, "follows = { aura = 9, on = 1 }\n").is_err());
    // On a circle, beside an orbit and an `ahead`, its target by an NPC's
    // death, a hit and an aura (a cast's start names one).
    let e = follows(
        "{ kind = \"circle\", radius = 3 }",
        &format!(
            "{death}, {{ hit = {{ spell = 4 }} }}, {{ aura_applied = {{ spell = 6 }} }}, {{ cast_start = {{ spell = 5 }} }}"
        ),
        "follows = \"target\"\norbit = { around = \"well\", turn = 3 }\nahead = 5\n",
    )
    .unwrap();
    let none = "names none apart from the unit it lands on, which it would follow from under it: only a cast names a target (`{ aura = N }` follows a player carrying an aura)";
    assert_eq!(
        check(&e),
        [
            "ground.jet follows, but a circle has no way to point: `follows` turns a line or a cone".to_string(),
            "ground.jet both follows and circles; it turns one way".to_string(),
            "ground.jet both follows and lands ahead of its unit; it would turn about the spot ahead, not the unit".to_string(),
            format!("ground.jet follows its target, but Death {{ npc: \"boss\" }} {none}"),
            format!("ground.jet follows its target, but Hit {{ spell: 4, lands: false }} {none}"),
            format!("ground.jet follows its target, but AuraApplied {{ spell: 6, on: None }} {none}"),
        ]
    );
    let e = follows(line, cast, "follows = { aura = 0 }\n").unwrap();
    assert_eq!(
        check(&e),
        ["ground.jet follows the carrier of spell 0, which is none"]
    );
}

#[test]
fn a_ground_is_pushed_along_its_bearings_in_turn_and_a_bad_push_is_found() {
    let pushed = |push: &str, more: &str| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [place.well]\nkind = \"pool\"\nat = [0, 0]\n\n\
             [ground.slick]\nkind = \"pool\"\nshape = {{ kind = \"circle\", radius = 3 }}\n\
             at = [{{ death = {{ npc = \"boss\" }} }}]\npush = {push}\n{more}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let e = pushed(
        "{ on = { cast = { spell = 5, by = \"boss\" } }, toward = [0, 90, 180, 270], speed = 4, secs = 3 }",
        "",
    )
    .unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    assert_eq!(
        e.ground["slick"].push,
        Some(Push {
            on: vec![Trigger::Cast {
                spell: 5,
                by: Some("boss".into())
            }],
            toward: Toward::Bearings(vec![0.0, 90.0, 180.0, 270.0]),
            speed: 4.0,
            secs: 3.0,
        })
    );
    // A lone trigger is read as a list of one, and written as one.
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains(
            "\npush = { toward = [0.0, 90.0, 180.0, 270.0], speed = 4.0, on = [{ cast = { spell = 5, by = \"boss\" } }], secs = 3.0 }\n"
        ),
        "{text}"
    );
    // No bearing, or one that is no number; no speed or time; an orbit
    // beside it; a trigger naming an NPC the fight lacks.
    let e = pushed(
        "{ on = { death = { npc = \"nobody\" } }, toward = [], speed = 0, secs = -1 }",
        "orbit = { around = \"well\", turn = 3 }\n",
    )
    .unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.slick names npc \"nobody\", which the encounter does not have",
            "ground.slick is pushed along no bearing, or one that is not a number",
            "ground.slick is pushed at a speed or for a time not above 0",
            "ground.slick both circles and is pushed; it does one",
        ]
    );
    let e = pushed(
        "{ on = { hit = { spell = 1 } }, toward = [nan], speed = 1, secs = 1 }",
        "",
    )
    .unwrap();
    assert_eq!(
        check(&e),
        ["ground.slick is pushed along no bearing, or one that is not a number"]
    );
    // A key a push lacks is a typo.
    let err = pushed(
        "{ on = { hit = { spell = 1 } }, toward = [0], speed = 1, secs = 1, turn = 2 }",
        "",
    )
    .unwrap_err();
    assert!(err.contains("turn"), "{err}");
    // An empty list pushes on nothing.
    let e = pushed("{ on = [], toward = [0], speed = 1, secs = 1 }", "").unwrap();
    assert_eq!(
        check(&e),
        ["ground.slick is pushed on nothing (`on` is empty)"]
    );
}

/// A knock throws its trigger's unit out from a place or an NPC, landing
/// within a place with a radius where it says one; each name it uses is
/// one the encounter has, and its trigger names a unit to throw.
#[test]
fn a_knock_throws_from_a_place_or_an_npc_and_a_bad_one_is_found() {
    let knock = |body: &str| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
             [place.bubble]\nkind = \"pool\"\nat = [100, -200]\nradius = 8\n\n\
             [place.platform]\nkind = \"spawn\"\nat = [100, -200]\nradius = 40\n\n\
             [place.spot]\nkind = \"spawn\"\nat = [0, 0]\n\n\
             [knock.throw]\n{body}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let e = knock(
        "name = \"Throw\"\ntint = \"teal\"\non = { hit = { spell = 900051 } }\n\
         from = \"bubble\"\nwithin = \"platform\"\n",
    )
    .unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    assert_eq!(
        e.knock["throw"],
        Knock {
            enabled: true,
            name: Some("Throw".into()),
            tint: Some("teal".into()),
            on: Trigger::Hit {
                spell: 900051,
                lands: false,
            },
            from: "bubble".into(),
            within: Some("platform".into()),
            only: Vec::new(),
        }
    );
    // It reads back as it was written.
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    let back = Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
        .unwrap()
        .encounter(7, None)
        .unwrap()
        .unwrap();
    assert_eq!(back.knock, e.knock);
    // Thrown from an NPC, landing anywhere.
    let e = knock("on = { aura_applied = { spell = 5 } }\nfrom = \"boss\"\n").unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // A trigger about no one, a middle neither a place nor an NPC, a place
    // to land in that the fight lacks, a color the rubric does not name.
    let e = knock(
        "tint = \"Blue\"\non = { event = \"burst\" }\nfrom = \"nowhere\"\nwithin = \"lagoon\"\n",
    )
    .unwrap();
    assert_eq!(
        check(&e),
        [
            "knock.throw names event \"burst\", which the encounter does not have",
            "knock.throw throws on Event(\"burst\"), which names no unit to throw",
            "knock.throw throws from \"nowhere\", which is neither a place nor an NPC",
            "knock.throw lands within place \"lagoon\", which the encounter does not have",
            "knock.throw.tint is \"Blue\", which is not a tint (green, red, blue, orange, teal, cyan, purple, pink, gold, yellow, amber, white, ice)",
        ]
    );
    // A place with no radius has no circle to land in.
    let e = knock("on = { hit = { spell = 1 } }\nfrom = \"bubble\"\nwithin = \"spot\"\n").unwrap();
    assert_eq!(
        check(&e),
        ["knock.throw: place \"spot\" has no radius to land within"]
    );
    // A place kept to other difficulties is none on this one: a knock
    // landing within it, or thrown from it, is refused where it is not.
    let text = "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.boss]\nname = \"Boss\"\n\n\
         [place.bubble]\nkind = \"pool\"\nat = [0, 0]\nradius = 9\nonly = [\"mythic\"]\n\n\
         [place.platform]\nkind = \"spawn\"\nat = [0, 0]\nradius = 40\nonly = [\"mythic\"]\n\n\
         [knock.throw]\non = { hit = { spell = 1 } }\nfrom = \"bubble\"\nwithin = \"platform\"\n";
    let r = Rubric::from_files(files(&[("s/raid/7-x.toml", text)])).unwrap();
    let heroic = r.encounter(7, Some(15)).unwrap().unwrap();
    assert_eq!(
        check(&heroic),
        [
            "knock.throw throws from \"bubble\", which is neither a place nor an NPC",
            "knock.throw lands within place \"platform\", which the encounter does not have",
        ]
    );
    let mythic = r.encounter(7, Some(16)).unwrap().unwrap();
    assert!(check(&mythic).is_empty(), "{:?}", check(&mythic));
    // A key a knock lacks is a typo, and one it needs is missing.
    let err = knock("on = { hit = { spell = 1 } }\nfrom = \"bubble\"\nspeed = 20\n").unwrap_err();
    assert!(err.contains("speed"), "{err}");
    let err = knock("on = { hit = { spell = 1 } }\n").unwrap_err();
    assert!(err.contains("from"), "{err}");
}

/// A reach whose place takes in the NPC that gets there says so, and
/// reads back as it was written; unsaid, it does not.
#[test]
fn a_reach_may_say_its_place_absorbs() {
    let reach = |more: &str| {
        let text = format!(
            "schema = 1\nencounter = 7\nname = \"X\"\n\n[npc.murloc]\nname = \"Murloc\"\n\n\
             [place.bubble]\nkind = \"pool\"\nat = [0, 0]\nradius = 9\n\n\
             [reach.murloc-at-bubble]\nnpc = \"murloc\"\nplace = \"bubble\"\n{more}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
            .unwrap()
    };
    assert!(!reach("").reach["murloc-at-bubble"].absorbs);
    let e = reach("absorbs = true\n");
    assert!(e.reach["murloc-at-bubble"].absorbs);
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains(
            "[reach.murloc-at-bubble]\nnpc = \"murloc\"\nplace = \"bubble\"\nabsorbs = true\n"
        ),
        "{text}"
    );
    // Unsaid, it is not written.
    let text = write::draft_text(&reach(""), &BTreeMap::new(), "x").unwrap();
    assert!(!text.contains("absorbs"), "{text}");
}

/// An invented slick laid out as a tuned file would write it, an inline
/// table whose list spans lines: blown by the three gales of each storm
/// the boss casts, each an `after` counting from every cast (`each`), the
/// way the players drifted, renewed where the wind has taken it and gone
/// as it leaves the platform. Its ids are made up.
const RESIDUE: &str = r#"
[ground.acid-slick]
name = "Acid Slick"
kind = "pool"
tint = "teal"
shape = { kind = "circle", radius = 5 }
at = [
    { hit = { spell = 900101 } },
    { hit = { spell = 900102 } },
]
renew = true
falls = true
push = { on = [
    { after = { secs = 2, since = { cast = { spell = 900100, by = "windlord" } }, each = true } },
    { after = { secs = 7, since = { cast = { spell = 900100, by = "windlord" } }, each = true } },
    { after = { secs = 12.5, since = { cast = { spell = 900100, by = "windlord" } }, each = true } },
], toward = "drift", speed = 3, secs = 6 }
only = ["mythic"]
"#;

/// The Windlord with `ground` (one or more grounds' text) and a platform for a
/// stencil where `stencil` says so, read on Mythic.
fn windlord(ground: &str, stencil: bool) -> Result<Encounter, String> {
    let map = if stencil {
        "[map]\nstencil = [{ center = [0, 0], radius = 50 }]\n\n"
    } else {
        ""
    };
    let text = format!(
        "schema = 2\nencounter = 7\nname = \"The Windlord\"\n\n{map}[npc.windlord]\nname = \"The Windlord\"\nrole = \"boss\"\n{ground}"
    );
    Rubric::from_files(files(&[("s/raid/7-windlord.toml", &text)]))
        .map_err(|e| format!("{e:?}"))?
        .encounter(7, Some(16))
        .ok_or("no encounter")?
}

/// A push fires on a list of triggers (still read as one alone, written
/// as a list either way), each `after` with `each` counting from every
/// firing of its `since`; `toward` is a list of bearings or the word
/// `"drift"`; `falls` is a flag written after `renew`. Read from the tune's
/// own layout, written back in the schema's order, and read again alike.
#[test]
fn a_tunes_drifting_falling_residue_is_read_written_and_read_again() {
    let e = windlord(RESIDUE, true).unwrap();
    let g = &e.ground["acid-slick"];
    assert!(g.renew && g.falls);
    let cast = Trigger::Cast {
        spell: 900100,
        by: Some("windlord".into()),
    };
    let gale = |secs: f32| {
        Trigger::After(Clock {
            secs,
            since: Some(Box::new(cast.clone())),
            nth: None,
            each: true,
        })
    };
    assert_eq!(
        g.push,
        Some(Push {
            on: vec![gale(2.0), gale(7.0), gale(12.5)],
            toward: Toward::Drift,
            speed: 3.0,
            secs: 6.0,
        })
    );
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    for line in [
        "\nrenew = true\nfalls = true\n",
        "\npush = { toward = \"drift\", speed = 3.0, on = [{ after = { secs = 2.0, since = { cast = { spell = 900100, by = \"windlord\" } }, each = true } }, ",
        "{ after = { secs = 12.5, since = { cast = { spell = 900100, by = \"windlord\" } }, each = true } }], secs = 6.0 }\n",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    let back = Rubric::from_files(files(&[("s/raid/7-windlord.toml", &text)]))
        .unwrap()
        .encounter(7, Some(16))
        .unwrap()
        .unwrap();
    assert_eq!(back.ground["acid-slick"], *g);
    // `falls` unsaid is false and not written; a word for `toward` other
    // than "drift" is refused as it is read.
    let plain = RESIDUE.replace("falls = true\n", "");
    let e = windlord(&plain, true).unwrap();
    assert!(!e.ground["acid-slick"].falls);
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(!text.contains("falls"), "{text}");
    let err = windlord(&RESIDUE.replace("\"drift\"", "\"wind\""), true).unwrap_err();
    assert!(err.contains("wind") && err.contains("drift"), "{err}");
    let err = windlord(&RESIDUE.replace("falls = true", "falls = 1"), true).unwrap_err();
    assert!(err.contains("falls") || err.contains("bool"), "{err}");
}

/// `falls` needs something to move the ground (a push or an orbit) and a
/// stencil to fall off: the encounter's map's. A trash file gives no map,
/// so its grounds never fall.
#[test]
fn a_ground_that_falls_needs_a_way_to_move_and_a_stencil() {
    let still = "\n[ground.pool]\nkind = \"pool\"\nshape = { kind = \"circle\", radius = 3 }\n\
                 at = [{ death = { npc = \"windlord\" } }]\nfalls = true\n";
    let e = windlord(still, false).unwrap();
    assert_eq!(
        check(&e),
        [
            "ground.pool falls, but nothing moves it (no `push`, no `orbit`), so it never leaves where it formed",
            "ground.pool falls, but the encounter's map has no stencil to fall off",
        ]
    );
    let e = windlord(&RESIDUE.replace("[\"mythic\"]", "[]"), false).unwrap();
    assert_eq!(
        check(&e),
        ["ground.acid-slick falls, but the encounter's map has no stencil to fall off"]
    );
    // Circling moves it as a push does.
    let circling = format!("{still}orbit = {{ around = \"windlord\", turn = 3 }}\n");
    assert!(check(&windlord(&circling, true).unwrap()).is_empty());
    assert_eq!(
        check_trash(&windlord(&circling, false).unwrap()),
        ["ground.pool falls, but the trash's map has no stencil to fall off"]
    );
}

/// An `after` with `each` strikes after every time its `since` fired: read,
/// written only where true (after `since`), and refused beside an `nth`,
/// one said as 1 among them, or one a later layer lays beside it.
#[test]
fn a_clock_counting_from_each_firing_is_read_written_and_kept_from_nth() {
    let event = |on: &str| format!("\n[event.gale]\non = {on}\n");
    let e = windlord(
        &event("{ after = { secs = 9, since = { cast = { spell = 5 } }, each = true } }"),
        false,
    )
    .unwrap();
    assert_eq!(
        e.event["gale"].on,
        Trigger::After(Clock {
            secs: 9.0,
            since: Some(Box::new(Trigger::Cast { spell: 5, by: None })),
            nth: None,
            each: true,
        })
    );
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains(
            "\non = { after = { secs = 9.0, since = { cast = { spell = 5 } }, each = true } }\n"
        ),
        "{text}"
    );
    // Said false, it is the first firing alone, and not written.
    let e = windlord(
        &event("{ after = { secs = 9, since = { cast = { spell = 5 } }, each = false } }"),
        false,
    )
    .unwrap();
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\non = { after = { secs = 9.0, since = { cast = { spell = 5 } } } }\n"),
        "{text}"
    );
    // An `nth` written beside it, even the first, is refused.
    for nth in [1, 2] {
        let e = windlord(
            &event(&format!(
                "{{ after = {{ secs = 9, since = {{ cast = {{ spell = 5 }} }}, nth = {nth}, each = true }} }}"
            )),
            false,
        )
        .unwrap();
        assert_eq!(
            check(&e),
            ["event.gale counts from each time its `since` fired and from its `nth`; it does one"]
        );
    }
    // A difficulty's `nth` laid over it key by key is refused alike.
    let e = windlord(
        &(event("{ after = { secs = 9, since = { cast = { spell = 5 } }, each = true } }")
            + "\n[difficulty.mythic.event.gale]\non = { after = { nth = 2 } }\n"),
        false,
    )
    .unwrap();
    assert_eq!(
        check(&e),
        ["event.gale counts from each time its `since` fired and from its `nth`; it does one"]
    );
}

#[test]
fn every_tint_is_one_the_rubric_names() {
    for name in tint::NAMES {
        assert!(tint::known(name), "{name}");
    }
    assert_eq!(tint::NAMES.len(), 13);
    for name in ["Green", "grey", "venom", ""] {
        assert!(!tint::known(name), "{name:?}");
    }
    // Every field a color is written in, each a name the rubric lacks but
    // one of the range's.
    let colors = r#"
[npc.boss]
name = "Boss"

[place.pool]
kind = "pool"
tint = "venom"
at = [0, 0]

[ground.puddle]
kind = "puddle"
tint = "Green"
shape = { kind = "circle", radius = 3 }
at = [{ death = { npc = "boss" } }]

[mark.bomb]
aura = 1
ring = "grey"
dot = "scarlet"

[line.beam]
tint = "lime"
from = { hit = { spell = 1 } }
to = "boss"

[volley.wave]
tint = "sea"
from = { hit = { spell = 1 } }
toward = [0]
speed = 1
width = 1

[flash.blast]
spell = [1]
tint = "white "

[range.reach]
npc = ["boss", "boss"]
tint = ["ice", "frost"]
radius = 5
"#;
    let named = |t: &str| format!("{t}, which is not a tint ({})", tint::NAMES.join(", "));
    let want = [
        named("place.pool.tint is \"venom\""),
        named("ground.puddle.tint is \"Green\""),
        named("mark.bomb.ring is \"grey\""),
        named("mark.bomb.dot is \"scarlet\""),
        named("line.beam.tint is \"lime\""),
        named("volley.wave.tint is \"sea\""),
        named("flash.blast.tint is \"white \""),
        named("range.reach.tint[1] is \"frost\""),
    ];
    let encounter = format!("schema = 1\nencounter = 7\nname = \"X\"\n{colors}");
    let r = Rubric::from_files(files(&[("s/raid/7-x.toml", &encounter)])).unwrap();
    assert_eq!(check(&r.encounter(7, None).unwrap().unwrap()), want);
    // A trash file's colors are held the same, its creatures the
    // instance's.
    let trash = format!(
        "schema = 1\n{}",
        colors
            .replace("[npc.boss]\nname = \"Boss\"\n", "")
            .replace("\"boss\"", "\"juggernaut\"")
    );
    let r = Rubric::from_files(trashed(&trash)).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(check_trash(&r.trash(3004).unwrap().unwrap()), want);
}

#[test]
fn a_places_swirl_is_read_checked_layered_and_written() {
    let places = r##"schema = 1
encounter = 7
name = "X"

[place.well]
kind = "pool"
at = [206.9, 0]
radius = 11.3
swirl = { texture = 5207558, colors = ["#03201a", "#7af0b8"], twist = 330, spin = -12 }

[place.spot]
kind = "spawn"
at = [0, 0]
swirl = { texture = 0, colors = ["03201a", "#fff"], twist = nan, spin = 1 }
"##;
    let r = Rubric::from_files(files(&[("s/raid/7-x.toml", places)])).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    let well = e.place["well"].swirl.as_ref().unwrap();
    assert_eq!(
        (well.texture, well.twist, well.spin),
        (5207558, 330.0, -12.0)
    );
    assert_eq!(well.rgb(), Some([[0x03, 0x20, 0x1a], [0x7a, 0xf0, 0xb8]]));
    assert_eq!(
        check(&e),
        [
            "place.spot: swirl color \"03201a\" is not #rrggbb",
            "place.spot: swirl color \"#fff\" is not #rrggbb",
            "place.spot has a swirl but no radius: it has no circle to fill",
            "place.spot: swirl twist or spin is not a number",
            "place.spot: swirl texture 0, which is none",
        ]
    );
    // A later layer retunes one key of it and keeps the rest.
    let r = r.with_user(owned(&[(
        "s/raid/7-x.toml",
        "schema = 1\n[place.well]\nswirl = { spin = -20 }\n",
    )]));
    let e = r.encounter(7, None).unwrap().unwrap();
    let well = e.place["well"].swirl.as_ref().unwrap();
    assert_eq!((well.texture, well.spin), (5207558, -20.0));
    // Written back on one line, its keys in the schema's order.
    let text = write::draft_text(&e, &BTreeMap::new(), "").unwrap();
    assert!(
        text.contains(
            "\nswirl = { texture = 5207558, colors = [\"#03201a\", \"#7af0b8\"], twist = 330.0, spin = -20.0 }\n"
        ),
        "{text}"
    );
}

/// A place opens each time its `from` fires and closes at the first
/// `until` after: a `from` while it is open is passed over, a burst of
/// `until`s closes it at its first, an unsaid `from` opens it at the
/// start, an unsaid `until` (or none after) keeps it to the end.
#[test]
fn a_place_opens_at_each_from_and_closes_at_the_next_until() {
    let place = |from: bool, until: bool| Place {
        enabled: true,
        name: None,
        kind: "pool".into(),
        tint: None,
        at: [0.0, 0.0],
        radius: Some(9.0),
        liquid_height: None,
        swirl: None,
        from: from.then_some(Trigger::Cast { spell: 1, by: None }),
        until: until.then_some(Trigger::Hit {
            spell: 2,
            lands: false,
        }),
        only: Vec::new(),
    };
    let both = place(true, true);
    assert!(both.comes_and_goes() && !place(false, false).comes_and_goes());
    // The bubble at 18 s and again at 50 s while it stands, its pops' hits
    // at 120.0, 120.4 and 230.0; then cast at 128 s and 238 s, never
    // popped after.
    let casts = [18.0, 50.0, 128.0, 238.0];
    let pops = [120.4, 120.0, 230.0];
    assert_eq!(
        both.windows(&casts, &pops, 0.0, 400.0),
        [(18.0, 120.0), (128.0, 230.0), (238.0, 400.0)]
    );
    // From the start to the first pop; from each cast to the end.
    assert_eq!(
        place(false, true).windows(&casts, &pops, 0.0, 400.0),
        [(0.0, 120.0)]
    );
    assert_eq!(
        place(true, false).windows(&casts, &pops, 0.0, 400.0),
        [(18.0, 400.0)]
    );
    assert_eq!(place(false, false).windows::<u32>(&[], &[], 5, 9), [(5, 9)]);
    // A `from` that never fires leaves it off the whole fight; one at the
    // end opens nothing; one before the start (a keystone fight's clock)
    // opens it at the start where it is open then, and not where it
    // closed before.
    assert_eq!(both.windows::<u32>(&[], &[3], 0, 10), []);
    assert_eq!(both.windows::<u32>(&[10, 12], &[], 2, 10), []);
    assert_eq!(both.windows::<u32>(&[1, 4], &[5], 2, 10), [(2, 5)]);
    assert_eq!(both.windows::<u32>(&[0, 4], &[1, 6], 2, 10), [(4, 6)]);
}

#[test]
fn a_mark_reads_its_count_from_its_stacks_or_its_ticks() {
    let marks = files(&[(
        "s/raid/7-x.toml",
        r#"schema = 1
encounter = 7
name = "X"

[mark.venom]
name = "Venom"
aura = 30
count = { tick = { spell = 31 } }
goal = 4

[mark.stacks]
aura = 40

[difficulty.mythic.mark.venom]
count = { tick = { spell = 31, each = 15867 } }
"#,
    )]);
    let r = Rubric::from_files(marks).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    assert_eq!(e.mark["venom"].goal, Some(4));
    assert_eq!(
        e.mark["venom"].count,
        Some(Count::Tick {
            spell: 31,
            each: None
        })
    );
    // Unsaid, a mark counts its aura's stacks.
    assert_eq!(
        (e.mark["stacks"].count.clone(), e.mark["stacks"].goal),
        (None, None)
    );
    let m = r.encounter(7, Some(16)).unwrap().unwrap();
    assert_eq!(
        m.mark["venom"].count,
        Some(Count::Tick {
            spell: 31,
            each: Some(15867)
        })
    );
}

#[test]
fn a_line_runs_from_where_its_unit_stood_to_an_npc_it_has() {
    let lines = files(&[(
        "s/raid/7-x.toml",
        r#"schema = 1
encounter = 7
name = "X"

[npc.boss]
name = "Boss"

[line.dart]
name = "Dart"
tint = "green"
from = { hit = { spell = 21 } }
to = "boss"
delay = 4
hits = [22]

[line.astray]
from = { after = 3 }
to = "nobody"

[flash.blast]
name = "Blast"
spell = [23]
tint = "green"
"#,
    )]);
    let r = Rubric::from_files(lines).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    let dart = &e.line["dart"];
    assert_eq!(
        (dart.to.as_str(), dart.delay, dart.hits.as_slice()),
        ("boss", Some(4.0), &[22][..])
    );
    assert_eq!(e.flash["blast"].spell, vec![23]);
    // A clock names no one to start from, and nobody is no NPC.
    let problems = check(&e);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.contains("line.astray")),
        "{problems:?}"
    );
}

/// A line may run to the unit that put its trigger's aura on (`to =
/// "@source"`), which only an aura going on or coming off names; any other
/// name starting with `@` is no keyword, never looked for among the NPCs.
/// It is written as it reads.
#[test]
fn a_line_to_its_auras_source_is_read_written_and_checked() {
    let spit = |from: &str, to: &str| {
        let text = format!(
            "schema = 2\nencounter = 7\nname = \"X\"\n\n[npc.spawn]\nname = \"Spawn\"\n\n\
             [line.spit]\nfrom = {from}\nto = \"{to}\"\nhits = [22]\n"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
            .unwrap()
    };
    let removed = "{ aura_removed = { spell = 21 } }";
    let e = spit(removed, "@source");
    assert!(e.line["spit"].runs_to_source());
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let applied = spit(
        "{ aura_applied = { spell = 21, on = \"spawn\" } }",
        "@source",
    );
    assert!(check(&applied).is_empty(), "{:?}", check(&applied));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains(
            "[line.spit]\nfrom = { aura_removed = { spell = 21 } }\nto = \"@source\"\nhits = [22]\n"
        ),
        "{text}"
    );
    let back = Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
        .unwrap()
        .encounter(7, None)
        .unwrap()
        .unwrap();
    assert_eq!(back.line["spit"], e.line["spit"]);
    // An NPC's key runs to that NPC, as ever.
    let npc = spit(removed, "spawn");
    assert!(!npc.line["spit"].runs_to_source());
    assert!(check(&npc).is_empty(), "{:?}", check(&npc));
    // A hit or a cast puts no aura on.
    for from in ["{ hit = { spell = 21 } }", "{ cast = { spell = 21 } }"] {
        assert_eq!(
            check(&spit(from, "@source")),
            ["line.spit runs to \"@source\", but its `from` names no aura"],
            "{from}"
        );
    }
    // A misspelled keyword is refused as one.
    assert_eq!(
        check(&spit(removed, "@sorce")),
        ["line.spit runs to \"@sorce\", which is no keyword (\"@source\" is the one)"]
    );
}

/// `hold`: a line held while its aura stays on. Unsaid, false and not
/// written; said, read and written back where the order puts it, and
/// refused beside a `from` that is no aura going on, a `delay` or `hits`.
#[test]
fn a_held_line_is_read_written_and_checked() {
    let fixation = |from: &str, more: &str| {
        let text = format!(
            "schema = 2\nencounter = 7\nname = \"X\"\n\n[npc.ghost]\nname = \"Ghost\"\n\n\
             [line.fixation]\nfrom = {from}\nto = \"@source\"\n{more}"
        );
        Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
            .unwrap()
            .encounter(7, None)
            .unwrap()
    };
    let applied = "{ aura_applied = { spell = 21 } }";
    let e = fixation(applied, "").unwrap();
    assert!(!e.line["fixation"].hold);
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(!text.contains("hold"), "{text}");
    let e = fixation(applied, "hold = true\n").unwrap();
    assert!(e.line["fixation"].hold);
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains(
            "[line.fixation]\nhold = true\nfrom = { aura_applied = { spell = 21 } }\nto = \"@source\"\n"
        ),
        "{text}"
    );
    let back = Rubric::from_files(files(&[("s/raid/7-x.toml", &text)]))
        .unwrap()
        .encounter(7, None)
        .unwrap()
        .unwrap();
    assert_eq!(back.line["fixation"], e.line["fixation"]);
    // To an NPC's key too.
    let e = fixation(applied, "hold = true\n")
        .map(|mut e| {
            e.line.get_mut("fixation").unwrap().to = "ghost".into();
            e
        })
        .unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // It is a flag.
    let err = fixation(applied, "hold = 1\n").unwrap_err();
    assert!(err.contains("hold") || err.contains("bool"), "{err}");
    // An aura coming off stays on no longer; a held line never strikes.
    let e = fixation(
        "{ aura_removed = { spell = 21 } }",
        "hold = true\ndelay = 2\nhits = [22]\n",
    )
    .unwrap();
    assert_eq!(
        check(&e),
        [
            "line.fixation holds, but its `from` is no `aura_applied`: only an aura going on stays on",
            "line.fixation holds, and a held line has no `delay`",
            "line.fixation holds, and a held line never strikes, so has no `hits`",
        ]
    );
}

#[test]
fn a_stencil_is_polygons_and_circles_joined() {
    let r = Rubric::from_files(drawn("")).unwrap();
    let m = r.encounter(3492, None).unwrap().unwrap().map;
    assert_eq!(m.stencil_bounds(), Some([0.0, -5.0, 25.0, 10.0]));
    // Inside the triangle, 1 yd from its nearest edge; inside the circle,
    // 2 yd from its rim; outside both, nearest the triangle's long edge;
    // and on that edge.
    assert!((m.stencil_depth([9.0, 2.0]) - 1.0).abs() < 1e-4);
    assert!((m.stencil_depth([23.0, 0.0]) - 2.0).abs() < 1e-4);
    assert!((m.stencil_depth([0.0, 9.0]) + 4.5 * 2f32.sqrt()).abs() < 1e-4);
    assert!(m.stencil_depth([5.0, 5.0]).abs() < 1e-4);
    assert_eq!(Map::default().stencil_depth([1.0, 1.0]), f32::MAX);
    // Neither a polygon nor a circle: two corners, a circle without a
    // radius, a shape that is both.
    for bad in [
        "{ points = [[0, 0], [1, 1]] }",
        "{ center = [0, 0] }",
        "{ points = [[0, 0], [1, 0], [1, 1]], center = [0, 0], radius = 1 }",
        "{ center = [0, 0], radius = 0 }",
    ] {
        let mut f = files(&[]);
        f[1].1 = format!("{INSTANCE}\n[encounter.3492.map]\nstencil = [{bad}]\n");
        let e = Rubric::from_files(f)
            .unwrap()
            .encounter(3492, None)
            .unwrap()
            .unwrap();
        let problems = check(&e);
        assert!(
            problems.len() == 1 && problems[0].contains("map.stencil[0]"),
            "{bad}: {problems:?}"
        );
    }
}

/// A map from its TOML, as a level's `[level.<ui_map>.map]` or an arena's
/// `[map]` reads.
fn map_of(text: &str) -> Map {
    toml::from_str(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

#[test]
fn a_segments_distance_is_to_its_nearest_point() {
    let near = |p, a, b| schema::segment_distance(p, a, b);
    // Beside it, past either end, and from a segment that is one point.
    assert!((near([0.0, 5.0], [-10.0, 0.0], [10.0, 0.0]) - 5.0).abs() < 1e-4);
    assert!((near([13.0, 4.0], [-10.0, 0.0], [10.0, 0.0]) - 5.0).abs() < 1e-4);
    assert!((near([-13.0, -4.0], [-10.0, 0.0], [10.0, 0.0]) - 5.0).abs() < 1e-4);
    assert!((near([3.0, 4.0], [0.0, 0.0], [0.0, 0.0]) - 5.0).abs() < 1e-4);
    assert_eq!(near([2.0, 0.0], [-10.0, 0.0], [10.0, 0.0]), 0.0);
}

/// A level cut to its areas: a hall's square and a court's circle, a
/// landing with no stencil, an area switched off; a bent path of its own
/// width, a straight one unsaid, one switched off, one to the area
/// switched off and one to no area.
const STENCILED: &str = r#"
[area.hall]
shape = [{ center = [0, 0], radius = 30 }]
stencil = [{ points = [[-10, -10], [10, -10], [10, 10], [-10, 10]] }]

[area.court]
shape = [{ center = [100, 0], radius = 20 }]
stencil = [{ center = [100, 0], radius = 15 }]

[area.landing]
shape = [{ points = [[0, 80], [20, 80], [20, 120], [0, 120]] }]

[area.off]
enabled = false
shape = [{ center = [0, -100], radius = 5 }]
stencil = [{ center = [0, -100], radius = 50 }]

[path.hall-to-court]
from = "hall"
to = "court"
width = 4
points = [[0, 0], [50, 20], [100, 0]]

[path.hall-to-landing]
from = "hall"
to = "landing"

[path.closed]
enabled = false
from = "hall"
to = "court"
width = 40
points = [[0, 0], [0, -60]]

[path.to-off]
from = "hall"
to = "off"

[path.astray]
from = "hall"
to = "nowhere"
"#;

#[test]
fn a_levels_stencil_is_its_areas_and_their_corridors() {
    let m = map_of(STENCILED);
    let depth = |p: [f32; 2]| m.level_stencil_depth(p).unwrap();
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    // Inside the hall's square, 10 yd from its edges; inside the court's
    // circle, 15 yd from its rim: each deeper than the corridors there.
    assert!(close(depth([0.0, 0.0]), 10.0), "{}", depth([0.0, 0.0]));
    assert!(close(depth([100.0, 0.0]), 15.0));
    // On the bent path's bend, half its 4 yd in; 3 yd off it, 1 yd out.
    assert!(close(depth([50.0, 20.0]), 2.0));
    assert!(close(depth([50.0, 23.0]), -1.0));
    // The straight one, unsaid: from the hall's middle to the landing's
    // (its bounds', (10, 100)), 8 yd across; it stops at the middle.
    assert!(close(depth([5.0, 50.0]), Path::CORRIDOR_YD / 2.0));
    assert!(close(depth([10.0, 104.0]), 0.0));
    assert!(close(depth([10.0, 110.0]), -6.0));
    // The area and the path switched off cut nothing, nor do the path to
    // the area switched off and the path to none: the hall's square is
    // nearest, 90 yd off.
    assert!(
        close(depth([0.0, -100.0]), -90.0),
        "{}",
        depth([0.0, -100.0])
    );
    // Kept by the arena's rule: whole inside, half on the edge, none a
    // half-yard out.
    assert_eq!(m.level_stencil_keep([0.0, 0.0]), 1.0);
    assert!(close(m.level_stencil_keep([10.0, 104.0]), 0.5));
    assert_eq!(m.level_stencil_keep([50.0, 23.0]), 0.0);
    // Bounds: the square, the circle and each corridor's way padded by
    // half its width; unfeathered.
    assert_eq!(m.level_stencil_bounds(), Some([-10.0, -15.0, 115.0, 104.0]));
    // Cut to two shapes and two corridors.
    assert_eq!(m.level_stencil_count(), 4);
    // The ways: said, unsaid, and none.
    let way = |k: &str| m.path_way(&m.path[k]);
    assert_eq!(
        way("hall-to-court"),
        Some(m.path["hall-to-court"].points.clone())
    );
    assert_eq!(
        way("hall-to-landing"),
        Some(vec![[0.0, 0.0], [10.0, 100.0]])
    );
    assert_eq!(way("to-off"), None);
    assert_eq!(way("astray"), None);
    assert_eq!(m.path["hall-to-court"].corridor_width(), 4.0);
    assert_eq!(
        m.path["hall-to-landing"].corridor_width(),
        Path::CORRIDOR_YD
    );
    // No stencil on an enabled area: none, the corridors cutting nothing
    // and the render whole; an area switched off is no stencil.
    let mut bare = m.clone();
    for k in ["hall", "court"] {
        bare.area.get_mut(k).unwrap().stencil.clear();
    }
    assert_eq!(bare.level_stencil_depth([0.0, 0.0]), None);
    assert_eq!(bare.level_stencil_bounds(), None);
    assert_eq!(bare.level_stencil_keep([500.0, 500.0]), 1.0);
    assert_eq!(bare.level_stencil_count(), 0);
    assert_eq!(Map::default().level_stencil_depth([0.0, 0.0]), None);
    // One area's stencil alone is the stencil, its corridors with it.
    bare.area.get_mut("court").unwrap().stencil = m.area["court"].stencil.clone();
    assert_eq!(bare.level_stencil_count(), 3);
    assert!(close(bare.level_stencil_depth([5.0, 50.0]).unwrap(), 4.0));
}

#[test]
fn an_areas_ceiling_is_the_lowest_over_a_point() {
    let m = map_of(
        r#"
ceiling = 25

[area.street]
shape = [{ points = [[0, 0], [100, 0], [100, 100], [0, 100]] }]
ceiling = 40

[area.hall]
shape = [{ center = [50, 50], radius = 10 }]
ceiling = 8

[area.camera]
shape = [{ center = [20, 20], radius = 5 }]

[area.off]
enabled = false
shape = [{ center = [50, 50], radius = 3 }]
ceiling = 2
"#,
    );
    // Where the street and the hall overlap, the hall's is lower; the area
    // switched off says nothing.
    assert_eq!(m.ceiling_at([50.0, 50.0]), Some(8.0));
    // On the hall's rim it is the hall's.
    assert_eq!(m.ceiling_at([60.0, 50.0]), Some(8.0));
    // The street's stands over the map's; an area saying none leaves it.
    assert_eq!(m.ceiling_at([20.0, 20.0]), Some(40.0));
    // Outside every area, the map's; with none, none.
    assert_eq!(m.ceiling_at([200.0, 200.0]), Some(25.0));
    let unsaid = Map { ceiling: None, ..m };
    assert_eq!(unsaid.ceiling_at([200.0, 200.0]), None);
    assert_eq!(unsaid.ceiling_at([50.0, 50.0]), Some(8.0));
}

#[test]
fn an_areas_hang_is_the_lowest_over_a_point() {
    let m = map_of(
        r#"
hang = 6

[area.street]
shape = [{ points = [[0, 0], [100, 0], [100, 100], [0, 100]] }]
hang = 12

[area.hall]
shape = [{ center = [50, 50], radius = 10 }]
hang = 4

[area.yard]
shape = [{ points = [[200, 200], [300, 200], [300, 300], [200, 300]] }]
hang = 3

[area.booth]
shape = [{ center = [250, 250], radius = 10 }]
hang = 9

[area.camera]
shape = [{ center = [20, 20], radius = 50 }]

[area.off]
enabled = false
shape = [{ center = [50, 50], radius = 80 }]
hang = 1
"#,
    );
    // The hall lies inside the street and says the lower: its own at its
    // middle, though the street lies deeper there, and on its rim; the
    // area switched off says nothing.
    assert_eq!(m.hang_at([50.0, 50.0]), Some(4.0));
    assert_eq!(m.hang_at([60.0, 50.0]), Some(4.0));
    // The yard holds the booth and says the lower: the yard's inside it.
    assert_eq!(m.hang_at([250.0, 250.0]), Some(3.0));
    // The street's stands over the map's; the camera's area says none and
    // leaves it.
    assert_eq!(m.hang_at([20.0, 20.0]), Some(12.0));
    // Outside every area saying one, the map's; with none, none.
    assert_eq!(m.hang_at([-20.0, -20.0]), Some(6.0));
    assert_eq!(m.hang_at([400.0, 400.0]), Some(6.0));
    let unsaid = Map { hang: None, ..m };
    assert_eq!(unsaid.hang_at([400.0, 400.0]), None);
    assert_eq!(unsaid.hang_at([50.0, 50.0]), Some(4.0));
}

/// The test raid's instance file, giving Ula'tek's map: a stencil, a
/// surface left out, the platform layer's building and the view's zoom.
fn drawn(extra: &str) -> Vec<(String, String)> {
    let mut f = files(&[]);
    f[1].1 = format!(
        "{INSTANCE}\n[encounter.3492.view]\nzoom = 2\n\n[encounter.3492.map]\nexclude_textures = [4516750]\nstencil = [\n  {{ points = [[0, 0], [10, 0], [10, 10]] }},\n  {{ center = [20, 0], radius = 5 }},\n]\n\n[encounter.3492.map.layer.platform]\nwmo = 7549724\n{extra}"
    );
    f
}

#[test]
fn an_instance_gives_its_encounters_maps_at_both_tiers() {
    let r = Rubric::from_files(drawn("")).unwrap();
    for tier in [Tier::Base, Tier::Curated] {
        let e = r.encounter_at(3492, None, tier).unwrap().unwrap();
        // Over the draft's map: its floor stays, the drawing joins it, and
        // the view's zoom is the instance's over the season's.
        assert_eq!(e.map.floor.as_deref(), Some("ulatek"));
        assert_eq!(e.map.exclude_textures, [4516750]);
        assert_eq!(e.map.stencil.len(), 2);
        assert_eq!(e.map.layer["platform"].wmo, 7549724);
        assert_eq!(e.view.zoom, Some(2.0));
        assert!(check(&e).is_empty(), "{:?}", check(&e));
    }
    // The base's layer breaks as the players leave it; the tuned file's
    // `breaks` joins the instance's building at the curated tier, and its
    // turn the instance's zoom.
    let base = r.encounter_at(3492, None, Tier::Base).unwrap().unwrap();
    assert_eq!(base.map.layer["platform"].breaks, Breaks::Occupancy);
    assert_eq!(base.view.turn, None);
    let curated = r.encounter_at(3492, None, Tier::Curated).unwrap().unwrap();
    assert_eq!(
        curated.map.layer["platform"].breaks,
        Breaks::Phase("two".into())
    );
    assert_eq!(curated.view.turn, Some(90.0));
}

#[test]
fn an_instance_gives_only_maps_of_encounters_it_has() {
    let errs =
        Rubric::from_files(drawn("\n[encounter.3492.ability.x]\nname = \"X\"\n")).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("instance.toml") && e.contains("only an encounter's map and NPCs")),
        "{errs:?}"
    );
    let errs = Rubric::from_files(drawn("\n[encounter.9.map]\nlevel = 1\n")).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("[encounter.9]") && e.contains("no files")),
        "{errs:?}"
    );
    let errs = Rubric::from_files(drawn("\n[encounter.ulatek.map]\nlevel = 1\n")).unwrap_err();
    assert!(
        errs.iter().any(|e| e.contains("DungeonEncounterID")),
        "{errs:?}"
    );
    // A typo in the map is the encounter's error, naming the key.
    let r = Rubric::from_files(drawn("stencl = []\n")).unwrap();
    let e = r.encounter(3492, None).unwrap().unwrap_err();
    assert!(e.contains("stencl"), "{e}");
}

#[test]
fn an_instance_gives_what_changes_its_map_at_the_base() {
    // A platform that breaks on an event of the instance's, and a room
    // entered through a place of the instance's: Ula'tek's and Nek'zali's.
    let mut f = files(&[]);
    f[1].1 = format!(
        "{INSTANCE}\n[encounter.3492.map.layer.quarter]\nwmo = 7754174\nbreaks = {{ event = \"shatter\" }}\n\n[encounter.3492.event.shatter]\nname = \"Shatter\"\non = {{ after = 353 }}\n\n[encounter.3492.place.well]\nkind = \"pool\"\nat = [1575, 0]\nradius = 11\n\n[encounter.3492.room.inside]\nname = \"Inside\"\nthrough = \"well\"\nenter = [{{ aura_removed = {{ spell = 1 }} }}]\nradius = 21\n"
    );
    let r = Rubric::from_files(f).unwrap();
    for tier in [Tier::Base, Tier::Curated] {
        let e = r.encounter_at(3492, None, tier).unwrap().unwrap();
        assert_eq!(
            e.map.layer["quarter"].breaks,
            Breaks::Event("shatter".into())
        );
        assert!(e.room.contains_key("inside") && e.place.contains_key("well"));
        assert!(check(&e).is_empty(), "{:?}", check(&e));
    }
    // Tables merge key by key, so a tuned file that would make a layer
    // break on another kind of thing than the instance says is refused,
    // never silently half-merged.
    let mut f = files(&[]);
    f[1].1 = format!(
        "{INSTANCE}\n[encounter.3492.map.layer.platform]\nwmo = 7549724\nbreaks = {{ event = \"shatter\" }}\n\n[encounter.3492.event.shatter]\non = {{ after = 353 }}\n"
    );
    let r = Rubric::from_files(f).unwrap();
    assert!(r.encounter_at(3492, None, Tier::Base).unwrap().is_ok());
    let e = r
        .encounter_at(3492, None, Tier::Curated)
        .unwrap()
        .unwrap_err();
    assert!(e.contains("map.layer.platform.breaks"), "{e}");
}

#[test]
fn an_instance_gives_an_npc_the_draft_lacks_or_has_wrong_at_the_base() {
    // Ula'tek's heart, which the journal lists no creature for, sharing
    // Ula'tek's health; and the coagulation, which the draft calls an add.
    let r = Rubric::from_files(drawn(
        "\n[encounter.3492.npc.heart]\nname = \"Venomous Heart\"\nrole = \"boss\"\nshares = \"ulatek\"\n\n[encounter.3492.npc.coagulation]\nrole = \"pet\"\n",
    ))
    .unwrap();
    for tier in [Tier::Base, Tier::Curated] {
        let e = r.encounter_at(3492, None, tier).unwrap().unwrap();
        let heart = &e.npc["heart"];
        assert_eq!(
            (heart.role, heart.shares.as_deref()),
            (Role::Boss, Some("ulatek"))
        );
        // Laid key by key over the draft's: its name and owner kept.
        let c = &e.npc["coagulation"];
        assert_eq!(
            (c.name.as_str(), c.role, c.owner.as_deref()),
            ("Venom Coagulation", Role::Pet, Some("ulatek"))
        );
        assert!(check(&e).is_empty(), "{:?}", check(&e));
    }
    // The NPC whose health it shares must be the encounter's.
    let r = Rubric::from_files(drawn(
        "\n[encounter.3492.npc.heart]\nname = \"Heart\"\nshares = \"nobody\"\n",
    ))
    .unwrap();
    let e = r.encounter_at(3492, None, Tier::Base).unwrap().unwrap();
    let found = check(&e);
    assert!(
        found
            .iter()
            .any(|m| m.contains("npc.heart") && m.contains("nobody")),
        "{found:?}"
    );
}

/// The test raid's instance file giving its own creatures: two of the
/// dungeon's trash with their forces, a pet of one, one switched off, and
/// the dungeon's whole forces.
const CREATURES: &str = r#"
forces = 300

[npc.juggernaut]
name = "Primal Juggernaut"
creature = [188244]
role = "add"
forces = 25

[npc.channeler]
name = "Tempest Channeler"
creature = [198047, 198048]
forces = 25

[npc.thundercloud]
name = "Primal Thundercloud"
creature = [197509]
role = "pet"
owner = "channeler"

[npc.gone]
enabled = false
name = "Gone"
creature = [1]
"#;

#[test]
fn an_instance_gives_its_creatures_and_their_forces() {
    let mut f = files(&[]);
    f[1].1 = format!("{INSTANCE}{CREATURES}");
    let r = Rubric::from_files(f).unwrap();
    let (s, i) = r.instance_by_map(3004).unwrap();
    assert_eq!((s.id.as_str(), i.slug.as_str()), ("s", "raid"));
    assert!(r.instance_by_map(3005).is_none());
    assert_eq!(i.forces(), Some(300));
    let keys: Vec<&str> = i.npcs().keys().map(String::as_str).collect();
    assert_eq!(keys, ["channeler", "juggernaut", "thundercloud"]);
    let (k, n) = i.npc_of(198048).unwrap();
    assert_eq!(
        (k, n.name.as_str(), n.forces),
        ("channeler", "Tempest Channeler", Some(25))
    );
    assert_eq!(i.npc_of(197509).map(|(k, _)| k), Some("thundercloud"));
    assert_eq!(
        i.npc_of(197509).unwrap().1.owner.as_deref(),
        Some("channeler")
    );
    // Switched off, or not listed (a boss its encounter names): none.
    assert!(i.npc_of(1).is_none() && i.npc_of(248711).is_none());
    assert!(check_instance(i).is_empty(), "{:?}", check_instance(i));
    // The creatures are the instance's alone: no encounter reads them.
    assert!(!ulatek(None).npc.contains_key("juggernaut"));
    // A typo in one is the instance file's error.
    let mut f = files(&[]);
    f[1].1 = format!(
        "{INSTANCE}{}",
        CREATURES.replace("forces = 25\n", "forcs = 25\n")
    );
    let errs = Rubric::from_files(f).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("s/raid/instance.toml") && e.contains("forcs")),
        "{errs:?}"
    );
}

#[test]
fn a_creature_says_how_it_can_be_controlled() {
    let controlled = |juggernaut: &str| {
        let mut f = files(&[]);
        f[1].1 = format!(
            "{INSTANCE}{}",
            CREATURES
                .replace(
                    "role = \"add\"\nforces = 25\n",
                    &format!("role = \"add\"\ncontrol = {juggernaut}\nforces = 25\n"),
                )
                .replace(
                    "creature = [198047, 198048]\n",
                    "creature = [198047, 198048]\ncontrol = \"stun\"\n",
                )
        );
        Rubric::from_files(f)
    };
    let r = controlled("\"immune\"").unwrap_or_else(|e| panic!("{e:?}"));
    let (_, i) = r.instance_by_map(3004).unwrap();
    // Unsaid, every control lands.
    let control = |k: &str| i.npcs()[k].control;
    assert_eq!(
        [
            control("juggernaut"),
            control("channeler"),
            control("thundercloud")
        ],
        [Control::Immune, Control::Stun, Control::All]
    );
    assert!(check_instance(i).is_empty(), "{:?}", check_instance(i));
    // Written only where it is not `all`, after the role.
    let written = |k: &str| toml::to_string(&i.npcs()[k]).unwrap();
    assert!(
        written("juggernaut").contains("role = \"add\"\ncontrol = \"immune\"\nforces = 25\n"),
        "{}",
        written("juggernaut")
    );
    assert!(!written("thundercloud").contains("control"));
    // A word it does not know is the instance file's error.
    let errs = controlled("\"stunned\"").unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("s/raid/instance.toml") && e.contains("stunned")),
        "{errs:?}"
    );
}

#[test]
fn an_instances_creatures_are_checked_as_an_encounters_are() {
    let bad = r#"
[npc.juggernaut]
name = "Primal Juggernaut"
creature = [188244]
forces = 25
aloft = ["stage-two"]
only = ["mythc"]

[npc.twin]
name = "Primal Juggernaut"
creature = [188244]

[npc.thundercloud]
name = "Primal Thundercloud"
role = "pet"
owner = "nobody"
shares = "noone"
forces = 3
"#;
    let mut f = files(&[]);
    f[1].1 = format!("{INSTANCE}{bad}");
    let r = Rubric::from_files(f).unwrap();
    let (_, i) = r.instance_by_map(3004).unwrap();
    let problems = check_instance(i);
    for want in [
        "npc.juggernaut is aloft in phase \"stage-two\", which the instance does not have",
        "npc.twin is creature 188244, which npc.juggernaut is too",
        "npc.thundercloud names npc \"nobody\", which the instance does not have",
        "npc.thundercloud names npc \"noone\", which the instance does not have",
        "npc.thundercloud gives enemy forces but names no creature",
        "npc.juggernaut is kept to \"mythc\"",
        "its creatures give enemy forces, but the instance gives no total",
    ] {
        assert!(
            problems.iter().any(|p| p.starts_with(want)),
            "{want}: {problems:#?}"
        );
    }
    assert_eq!(problems.len(), 7, "{problems:#?}");
    // An encounter's NPCs are held the same: a phase it lacks, a creature
    // claimed twice.
    let r = Rubric::from_files(drawn(
        "\n[encounter.3492.npc.ulatek]\naloft = [\"one\", \"three\"]\n\n[encounter.3492.npc.spectral]\nname = \"Spectral Ula'tek\"\ncreature = [248711]\n",
    ))
    .unwrap();
    let problems = check(&r.encounter(3492, None).unwrap().unwrap());
    assert_eq!(
        problems,
        [
            "npc.ulatek is aloft in phase \"three\", which the encounter does not have",
            "npc.ulatek is creature 248711, which npc.spectral is too: a unit is one NPC",
        ],
    );
}

#[test]
fn an_instances_creatures_are_the_bases_alone() {
    // A layer's instance file giving creatures or the dungeon's forces is
    // refused: what an instance is stays the base's.
    for given in [
        "schema = 1\nforces = 9\n",
        "schema = 1\n[npc.x]\nname = \"X\"\n",
    ] {
        let file = || owned(&[("s/raid/instance.toml", given)]);
        let errs = Rubric::from_files(base_files())
            .unwrap()
            .with_curated("pack", file())
            .unwrap_err();
        assert!(errs[0].contains("stays the base's"), "{errs:?}");
        let user = Rubric::from_files(base_files()).unwrap().with_user(file());
        assert_eq!(user.user_errors().len(), 1, "{:?}", user.user_errors());
    }
}

#[test]
fn a_glow_brightens_and_fades_and_a_bad_one_is_found() {
    let mut f = files(&[]);
    f[1].1 = format!(
        "{INSTANCE}\n[encounter.3492.map]\nppy = 16\n\n[encounter.3492.map.glow.flame]\nname = \"Flame\"\ncolor = [200, 40, 30]\narea = [{{ center = [0, 0], radius = 5 }}]\nexcept = [{{ center = [-3, 0], radius = 1 }}]\nbelow = 1\non = {{ event = \"erupts\" }}\n\n[encounter.3492.event.erupts]\non = {{ cast = {{ spell = 1 }} }}\n"
    );
    let r = Rubric::from_files(f).unwrap();
    let e = r.encounter_at(3492, None, Tier::Base).unwrap().unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    assert_eq!(e.map.ppy, Some(16.0));
    let g = &e.map.glow["flame"];
    assert!(g.covers([3.0, 0.0]) && !g.covers([6.0, 0.0]));
    // Its except is cut out of it.
    assert!(!g.covers([-3.0, 0.0]) && g.covers([-1.5, 0.0]));
    // Unsaid, it brightens to its own color at full strength.
    assert_eq!(g.hot(), [255, 51, 38]);
    // Up over 0.6 s, held 2 s, down over 2.5 s.
    assert_eq!(g.brightness(-0.1), 0.0);
    assert!((g.brightness(0.3) - 0.5).abs() < 1e-6);
    assert_eq!(g.brightness(1.0), 1.0);
    assert_eq!(g.brightness(2.6), 1.0);
    assert!((g.brightness(3.85) - 0.5).abs() < 1e-6);
    assert_eq!(g.brightness(5.1), 0.0);
    // No area, a cut-out that is no shape, a dangling event, a time under
    // 0, a density off the scale.
    let mut f = files(&[]);
    f[1].1 = format!(
        "{INSTANCE}\n[encounter.3492.map]\nppy = 100\n\n[encounter.3492.map.glow.bad]\ncolor = [1, 2, 3]\narea = []\nexcept = [{{ radius = 1 }}]\non = {{ event = \"nowhere\" }}\nfade = -1\n"
    );
    let r = Rubric::from_files(f).unwrap();
    let problems = check(&r.encounter(3492, None).unwrap().unwrap());
    assert_eq!(problems.len(), 5, "{problems:?}");
}

#[test]
fn a_volley_sets_out_on_its_bearings_and_a_bad_one_is_found() {
    let volleys = files(&[(
        "s/raid/7-x.toml",
        r#"schema = 1
encounter = 7
name = "X"

[volley.wave]
name = "Wave"
tint = "green"
from = { aura_removed = { spell = 31 } }
toward = [0, 90, 180, 270]
speed = 12
width = 5
start = 2.5
length = 30
hits = [32]
destroys = ["tumor"]
place_kills = true
preview = true

[npc.tumor]
name = "Tumor"
still = true

[volley.astray]
from = { after = 3 }
destroys = ["nobody"]
toward = []
speed = 0
width = 5
start = -1
preview = true
front = 0
spread = 200
times = 3
alternate = true
holes = true
faint = true

[volley.turned]
from = { cast = { spell = 33 } }
toward = [0, 90]
aim = "facing"
form = "crescent"
spots = [[10, 4]]
side = "tumor"
spread = 25
speed = 9.8
width = 4
front = 80
delay = 0.5
every = 4.5
times = 3
alternate = true
holes = true
faint = true
"#,
    )]);
    let r = Rubric::from_files(volleys).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    let w = &e.volley["wave"];
    assert_eq!(w.hits, vec![32]);
    // Its middle sets out 2.5 yd off and goes 12 yd a second, for 30 yd.
    assert_eq!(w.out(-0.1), None);
    assert_eq!(w.out(0.0), Some(2.5));
    assert_eq!(w.out(2.0), Some(26.5));
    assert_eq!(w.out(2.6), None);
    // Bearings along the log's X, then its Y, then back.
    let steps = w.steps();
    assert!((steps[0][0] - 1.0).abs() < 1e-6 && steps[0][1].abs() < 1e-6);
    assert!(steps[1][0].abs() < 1e-6 && (steps[1][1] - 1.0).abs() < 1e-6);
    assert!((steps[2][0] + 1.0).abs() < 1e-6 && (steps[3][1] + 1.0).abs() < 1e-6);
    assert_eq!(w.destroys, vec!["tumor"]);
    // Its kills drawn where it cleared them; unsaid, none.
    assert!(w.place_kills);
    assert!(!e.volley["turned"].place_kills);
    assert!(e.npc["tumor"].still);
    // A wave turned with its caster: straight ahead, then to its left,
    // whichever way the caster faced; the world's bearings never turn.
    let t = &e.volley["turned"];
    assert_eq!(
        (t.aim, t.front, t.spread),
        (Aim::Facing, Some(80.0), Some(25.0))
    );
    assert_eq!((t.form, w.form), (Form::Crescent, Form::Disc));
    let south = t.steps_facing(std::f32::consts::PI);
    assert!((south[0][0] + 1.0).abs() < 1e-6 && south[0][1].abs() < 1e-6);
    assert!(south[1][0].abs() < 1e-6 && (south[1][1] + 1.0).abs() < 1e-6);
    assert_eq!(w.steps_facing(std::f32::consts::PI), w.steps());
    // Sent three times, half a second in and 4.5 s apart, from one side
    // then the other; the wave sent once, at once.
    assert_eq!(t.sendings(), vec![0.5, 5.0, 9.5]);
    assert_eq!(
        t.sendings_sided(false),
        vec![(0.5, false), (5.0, true), (9.5, false)]
    );
    // Its spot 10 yd ahead of a unit facing south and 4 to its left: the
    // tumor's side, else mirrored to the other.
    let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
    assert!(near(
        t.origins(std::f32::consts::PI, true)[0],
        [-10.0, -4.0]
    ));
    assert!(near(
        t.origins(std::f32::consts::PI, false)[0],
        [-10.0, 4.0]
    ));
    assert_eq!(w.origins(1.0, true), vec![[0.0, 0.0]]);
    assert_eq!(w.sendings(), vec![0.0]);
    // A clock names no one, no bearing, no speed, a start under 0, an NPC
    // it does not have, a preview with no aura to wait on, a front of 0,
    // a spread past a half turn, three sendings with no time between,
    // sides to alternate with no spots to mirror and holes with no
    // projectiles to leave out.
    let problems = check(&e);
    assert_eq!(problems.len(), 11, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.contains("volley.astray")),
        "{problems:?}"
    );
}

/// A volley round a middle of its own (`around`), coming `inward`: each
/// way begins `start` out on its bearing and comes in, its middle along
/// it from 0 to its `length` (unsaid, all the way in); held still at its
/// start from `from` through its `delay` where it is previewed so. Its
/// trigger need name no one; a middle that is neither a place nor an NPC,
/// an inward one with no start, and a preview with neither an aura to
/// wait on nor a delay to hold it in are refused.
#[test]
fn a_volley_can_close_on_a_middle_of_its_own() {
    let text = r#"schema = 2
encounter = 7
name = "X"

[npc.boss]
name = "Boss"

[place.bubble]
kind = "pool"
at = [100.0, -200.0]
radius = 8

[volley.whirlpools]
from = { cast = { spell = 900401 } }
around = "bubble"
inward = true
toward = [0, 90]
speed = 12
width = 8
start = 60
length = 50
delay = 3
preview = true
holes = true
hits = [900402]

[volley.all-in]
from = { after = { secs = 1, since = { cast = { spell = 1 } }, each = true } }
around = "boss"
inward = true
toward = [0]
speed = 10
width = 4
start = 30

[volley.astray]
from = { after = 3 }
around = "nowhere"
inward = true
toward = [0]
aim = "facing"
side = "boss"
spread = 10
speed = 10
width = 4
preview = true
"#;
    let r = Rubric::from_files(files(&[("s/raid/7-x.toml", text)])).unwrap();
    let e = r.encounter(7, None).unwrap().unwrap();
    let w = &e.volley["whirlpools"];
    assert_eq!((w.around.as_deref(), w.inward), (Some("bubble"), true));
    assert!(w.holds() && !e.volley["all-in"].holds());
    // Along its way: from 0, 12 yd a second, gone past 50 (10 yd off the
    // middle, past 4.1 s on); all the way in where no length is said.
    assert_eq!((w.sets_out(), w.reach()), (0.0, Some(50.0)));
    assert_eq!(w.out(0.0), Some(0.0));
    assert_eq!(w.out(4.0), Some(48.0));
    assert_eq!(w.out(4.2), None);
    assert_eq!(e.volley["all-in"].reach(), Some(30.0));
    // Each way 60 yd out on its bearing, turned round.
    let ways = w.ways(1.0, true);
    let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
    assert_eq!(ways.len(), 2);
    assert!(near(ways[0].from, [60.0, 0.0]) && near(ways[0].step, [-1.0, 0.0]));
    assert!(near(ways[1].from, [0.0, 60.0]) && near(ways[1].step, [0.0, -1.0]));
    assert_eq!((ways[1].spot, ways[1].bearing), (None, 1));
    // Outward, the spot and the bearing: as `origins` and `steps_facing`.
    let out = Volley {
        inward: false,
        ..w.clone()
    };
    assert_eq!((out.sets_out(), out.reach()), (60.0, Some(110.0)));
    assert!(near(out.ways(0.0, true)[1].from, [0.0, 0.0]));
    assert!(near(out.ways(0.0, true)[1].step, [0.0, 1.0]));
    assert_eq!(
        check(&e),
        [
            "volley.astray sets out round \"nowhere\", which is neither a place nor an NPC",
            "volley.astray comes inward from no start: give the yards out it sets out at (`start`)",
            "volley.astray comes inward and spreads: a spread turns each way about its start on the rim, so it would no longer close on its middle",
            "volley.astray turns with its unit's facing, but sets out round \"nowhere\" at After(Clock { secs: 3.0, since: None, nth: None, each: false }), which names no unit",
            "volley.astray keeps its spots to a side of its unit, but sets out round \"nowhere\" at After(Clock { secs: 3.0, since: None, nth: None, each: false }), which names no unit",
            "volley.astray is previewed, but sets out at no aura coming off to wait on, nor after a delay to hold it still in",
        ]
    );
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(text.contains("\nlength = 50.0\ninward = true\n"), "{text}");
    assert!(text.contains("\naround = \"bubble\"\n"), "{text}");
}

/// A volley's holes are each slot crossed unhit (`holes_by` unsaid, or
/// "slot"), or one a sending where the unhit players cluster ("cluster");
/// a cluster with no holes to find is refused, a word it does not know is
/// no rubric, and a file writes the key after `holes`, only where it is
/// not the slot's.
#[test]
fn a_volleys_holes_are_each_slots_or_one_where_the_raid_clusters() {
    let file = |keys: &str| {
        files(&[(
            "s/raid/7-x.toml",
            &format!(
                "schema = 2\nencounter = 7\nname = \"X\"\n\
                 [volley.ring]\nfrom = {{ cast = {{ spell = 31 }} }}\n\
                 toward = [0, 90, 180, 270]\nspeed = 12\nwidth = 3.5\n{keys}"
            ),
        )])
    };
    let read = |keys: &str| {
        let r = Rubric::from_files(file(keys)).map_err(|e| format!("{e:?}"))?;
        let e = r
            .encounter(7, None)
            .ok_or("no encounter")?
            .map_err(|e| format!("{e:?}"))?;
        Ok::<_, String>((e.volley["ring"].holes_by, check(&e), e))
    };
    let (by, problems, _) = read("holes = true\n").unwrap();
    assert_eq!((by, problems), (HolesBy::Slot, Vec::<String>::new()));
    let (by, problems, _) = read("holes = true\nholes_by = \"slot\"\n").unwrap();
    assert_eq!((by, problems), (HolesBy::Slot, Vec::<String>::new()));
    let (by, problems, e) = read("holes = true\nholes_by = \"cluster\"\n").unwrap();
    assert_eq!((by, problems), (HolesBy::Cluster, Vec::<String>::new()));
    let text = write::draft_text(&e, &BTreeMap::new(), "x").unwrap();
    assert!(
        text.contains("\nholes = true\nholes_by = \"cluster\"\n"),
        "{text}"
    );
    let slot = read("holes = true\n").unwrap().2;
    let text = write::draft_text(&slot, &BTreeMap::new(), "x").unwrap();
    assert!(!text.contains("holes_by"), "{text}");
    let (_, problems, _) = read("holes_by = \"cluster\"\n").unwrap();
    assert_eq!(
        problems,
        ["volley.ring finds its holes by cluster, with no holes to find"]
    );
    assert!(read("holes = true\nholes_by = \"one\"\n").is_err());
}

#[test]
fn a_volley_places_only_the_kills_it_destroys() {
    // A volley that places its kills names an NPC it destroys; one that
    // destroys none has no kill to place.
    let file = |destroys: &str| {
        files(&[(
            "s/raid/7-x.toml",
            &format!(
                "schema = 2\nencounter = 7\nname = \"X\"\n\
                 [npc.tumor]\nname = \"Tumor\"\nstill = true\n\
                 [volley.wave]\nfrom = {{ aura_removed = {{ spell = 31 }} }}\n\
                 toward = [0, 90]\nspeed = 12\nwidth = 3.5\n{destroys}place_kills = true\n"
            ),
        )])
    };
    let read = |destroys: &str| {
        let r = Rubric::from_files(file(destroys)).unwrap();
        check(&r.encounter(7, None).unwrap().unwrap())
    };
    assert_eq!(read("destroys = [\"tumor\"]\n"), Vec::<String>::new());
    let problems = read("");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("volley.wave") && problems[0].contains("places its kills"),
        "{problems:?}"
    );
}

#[test]
fn a_roles_name_is_its_toml_word() {
    for r in Role::ALL {
        let v = toml::Value::try_from(r).unwrap();
        assert_eq!(v.as_str(), Some(r.name()));
        assert_eq!(Role::from_name(r.name()), Some(r));
    }
    assert_eq!(Role::from_name("Boss"), None);
}

/// An encounter with every key of the schema written: each optional
/// value set, each list and table filled, every trigger and shape. Its
/// structs are spelled out whole, so a field added to the schema does
/// not build here until it is given a value.
pub(crate) fn every_key() -> Encounter {
    let s = |v: &str| v.to_string();
    let names = || vec![s("normal")];
    let triggers = vec![
        Trigger::Start,
        Trigger::After(Clock::pull(1.0)),
        Trigger::After(Clock {
            secs: 1.5,
            since: Some(Box::new(Trigger::Cast {
                spell: 1,
                by: Some(s("boss")),
            })),
            nth: Some(2),
            each: false,
        }),
        Trigger::After(Clock {
            secs: 2.5,
            since: Some(Box::new(Trigger::Cast {
                spell: 1,
                by: Some(s("boss")),
            })),
            nth: None,
            each: true,
        }),
        Trigger::Cast {
            spell: 1,
            by: Some(s("boss")),
        },
        Trigger::CastStart {
            spell: 1,
            by: Some(s("boss")),
        },
        Trigger::AuraApplied {
            spell: 1,
            on: Some(s("boss")),
        },
        Trigger::AuraRemoved {
            spell: 1,
            on: Some(s("boss")),
        },
        Trigger::HealthBelow {
            npc: s("boss"),
            pct: 50.0,
        },
        Trigger::Plateau {
            npc: s("boss"),
            secs: 5.0,
        },
        Trigger::Appears { npc: s("boss") },
        Trigger::Death { npc: s("boss") },
        Trigger::Hit {
            spell: 1,
            lands: true,
        },
        Trigger::AuraGone { spell: 1 },
        Trigger::Event(s("e")),
        Trigger::Phase(s("p")),
    ];
    // A trigger added to the schema does not build here until it joins
    // the list above.
    for t in &triggers {
        match t {
            Trigger::Start
            | Trigger::After(_)
            | Trigger::Cast { .. }
            | Trigger::CastStart { .. }
            | Trigger::AuraApplied { .. }
            | Trigger::AuraRemoved { .. }
            | Trigger::HealthBelow { .. }
            | Trigger::Plateau { .. }
            | Trigger::Appears { .. }
            | Trigger::Death { .. }
            | Trigger::Hit { .. }
            | Trigger::AuraGone { .. }
            | Trigger::Event(_)
            | Trigger::Phase(_) => {}
        }
    }
    let shapes = [
        Shape::Circle { radius: 1.0 },
        Shape::Ring {
            inner: 1.0,
            outer: 2.0,
        },
        Shape::Cone {
            angle: 60.0,
            length: 2.0,
        },
        Shape::Line {
            width: 1.0,
            length: 2.0,
        },
    ];
    let circle = StencilShape {
        points: Vec::new(),
        center: Some([0.0, 0.0]),
        radius: Some(1.0),
    };
    let polygon = StencilShape {
        points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
        center: None,
        radius: None,
    };
    let circle_area = circle.clone();
    let mut e = Encounter {
        schema: crate::SCHEMA,
        encounter: 1,
        name: s("Boss"),
        instance: s("raid"),
        order: 1,
        map: Map {
            ui_map: Some(1),
            floor: Some(s("boss")),
            level: Some(1.0),
            ceiling: Some(1.0),
            hang: Some(1.0),
            light: Some(Light::Graded),
            exclude_groups: vec![1],
            exclude_models: vec![1],
            exclude_textures: vec![1],
            layer: [Breaks::Never, Breaks::Phase(s("p")), Breaks::Event(s("e"))]
                .into_iter()
                .enumerate()
                .map(|(i, breaks)| {
                    let layer = Layer {
                        enabled: false,
                        wmo: 1,
                        placement: Some(1),
                        breaks,
                    };
                    (i.to_string(), layer)
                })
                .collect(),
            stencil: vec![polygon, circle.clone()],
            ppy: Some(16.0),
            glow: [(
                s("g"),
                Glow {
                    enabled: false,
                    name: Some(s("Glow")),
                    color: [1, 2, 3],
                    hot: Some([4, 5, 6]),
                    area: vec![circle.clone()],
                    except: vec![circle],
                    below: Some(1.0),
                    on: Some(Trigger::Start),
                    rise: Some(1.0),
                    hold: Some(1.0),
                    fade: Some(1.0),
                },
            )]
            .into(),
            crop: Some([0.0, 0.0, 1.0, 1.0]),
            include_groups: vec![2],
            include_models: vec![2],
            fade: [(s("group"), Some(1), None), (s("model"), None, Some(1))]
                .into_iter()
                .map(|(k, group, model)| {
                    let fade = Fade {
                        enabled: false,
                        name: Some(s("Fade")),
                        group,
                        model,
                        alpha: 0.5,
                    };
                    (k, fade)
                })
                .collect(),
            ground: Some(Datum::Height(1.0)),
            exposure: Some(1.0),
            area: [s("a"), s("b")]
                .into_iter()
                .map(|k| {
                    let area = Area {
                        enabled: false,
                        name: Some(s("Area")),
                        shape: vec![circle_area.clone()],
                        far: Some(1.0),
                        home: Some(1.0),
                        near: Some(1.0),
                        turn: Some(1.0),
                        ceiling: Some(1.0),
                        hang: Some(1.0),
                        stencil: vec![circle_area.clone()],
                        encounter: Some(1),
                    };
                    (k, area)
                })
                .collect(),
            path: [Mode::Run, Mode::Mount, Mode::Vehicle, Mode::Leap]
                .into_iter()
                .enumerate()
                .map(|(i, mode)| {
                    let path = Path {
                        enabled: false,
                        name: Some(s("Path")),
                        from: s("a"),
                        to: s("b"),
                        mode,
                        width: Some(1.0),
                        speed: Some(1.0),
                        spell: (mode == Mode::Leap).then_some(1),
                        points: vec![[0.0, 0.0], [1.0, 1.0]],
                    };
                    (i.to_string(), path)
                })
                .collect(),
            transition: Some(Transition {
                after: Some(1.0),
                unit: Some(UnitQuiet::Travel),
                port: Some(Port::Flash),
                port_yd: Some(1.0),
                camera: Some(CameraQuiet::Glide),
            }),
            water: Some(Water {
                close: [1, 2, 3],
                far: [4, 5, 6],
                shallow: 0.5,
                deep: 0.75,
                depth: 1.0,
                bed: Some([7, 8, 9]),
                types: Some(vec![1, 2]),
                shore: Some(0.5),
            }),
        },
        view: View {
            turn: Some(1.0),
            zoom: Some(1.0),
            zoom_max: Some(1.0),
            center: Some([0.0, 0.0]),
            arena: Some([0.0, 0.0, 1.0, 1.0]),
        },
        npc: [(
            s("boss"),
            Npc {
                enabled: false,
                name: s("Boss"),
                creature: vec![1],
                role: Role::Boss,
                control: Control::Stun,
                icon: Some(1),
                owner: Some(s("boss")),
                shares: Some(s("boss")),
                still: true,
                forces: Some(1),
                aloft: vec![s("p")],
                only: names(),
            },
        )]
        .into(),
        ability: shapes
            .into_iter()
            .enumerate()
            .map(|(i, shape)| {
                let ability = Ability {
                    enabled: false,
                    name: s("Ability"),
                    spell: vec![1],
                    by: Some(s("boss")),
                    shape: Some(shape),
                    lifetime: Some(1.0),
                    alert: Some(1),
                    marks: vec![s("tank")],
                    phase: Some(s("p")),
                    under: Some(s("0")),
                    text: Some(s("Words.")),
                    only: names(),
                };
                (i.to_string(), ability)
            })
            .collect(),
        phase: [(
            s("p"),
            Phase {
                enabled: false,
                name: s("Phase"),
                order: 1,
                enter: triggers.clone(),
                again: true,
                leave: triggers.clone(),
                only: names(),
            },
        )]
        .into(),
        place: [(
            s("place"),
            Place {
                enabled: false,
                name: Some(s("Place")),
                kind: s("pool"),
                tint: Some(s("green")),
                at: [0.0, 0.0],
                radius: Some(1.0),
                liquid_height: Some(1.0),
                swirl: Some(Swirl {
                    texture: 1,
                    colors: [s("#000000"), s("#ffffff")],
                    twist: 1.0,
                    spin: 1.0,
                }),
                from: Some(Trigger::Start),
                until: Some(Trigger::Start),
                only: names(),
            },
        )]
        .into(),
        ground: [(
            s("ground"),
            Ground {
                enabled: false,
                name: Some(s("Ground")),
                kind: s("pool"),
                tint: Some(s("green")),
                shape: Shape::Circle { radius: 1.0 },
                at: vec![Trigger::Hit {
                    spell: 1,
                    lands: false,
                }],
                delay: Some(1.0),
                ahead: Some(1.0),
                from: Some(Trigger::Start),
                ends: Ends::After(1.0),
                renew: false,
                falls: false,
                grow: Some(Grow::Stacks),
                spread: Some(1.0),
                preview: true,
                follows: None,
                orbit: Some(Orbit {
                    around: s("place"),
                    turn: 1.0,
                }),
                push: None,
                seen: vec![1],
                clears: Vec::new(),
                sweeps: Vec::new(),
                only: names(),
            },
        )]
        .into(),
        mark: [(
            s("mark"),
            Mark {
                enabled: false,
                name: Some(s("Mark")),
                aura: 1,
                count: Some(Count::Tick {
                    spell: 1,
                    each: Some(1),
                }),
                goal: Some(4),
                ring: Some(s("red")),
                label: Some(MarkLabel::Left),
                link: true,
                soak: vec![1],
                dot: Some(s("red")),
                harm: vec![1],
                only: names(),
            },
        )]
        .into(),
        line: [(
            s("line"),
            Line {
                enabled: false,
                name: Some(s("Line")),
                tint: Some(s("green")),
                from: Trigger::Hit {
                    spell: 1,
                    lands: false,
                },
                to: s("boss"),
                delay: Some(1.0),
                hits: vec![1],
                hold: true,
                only: names(),
            },
        )]
        .into(),
        volley: [(
            s("volley"),
            Volley {
                enabled: false,
                name: Some(s("Volley")),
                tint: Some(s("green")),
                from: Trigger::Hit {
                    spell: 1,
                    lands: false,
                },
                around: Some(s("place")),
                toward: vec![0.0, 90.0],
                aim: Aim::Facing,
                spread: Some(1.0),
                spots: vec![[1.0, 2.0]],
                side: Some(s("boss")),
                speed: 1.0,
                width: 1.0,
                front: Some(1.0),
                form: Form::Crescent,
                start: Some(1.0),
                length: Some(1.0),
                inward: true,
                delay: Some(1.0),
                every: Some(1.0),
                times: Some(2),
                alternate: true,
                holes: true,
                holes_by: HolesBy::Cluster,
                faint: true,
                hits: vec![1],
                destroys: vec![s("boss")],
                place_kills: true,
                preview: true,
                only: names(),
            },
        )]
        .into(),
        flash: [(
            s("flash"),
            Flash {
                enabled: false,
                name: Some(s("Flash")),
                spell: vec![1],
                tint: Some(s("green")),
                only: names(),
            },
        )]
        .into(),
        knock: [(
            s("knock"),
            Knock {
                enabled: false,
                name: Some(s("Knock")),
                tint: Some(s("green")),
                on: Trigger::Hit {
                    spell: 1,
                    lands: false,
                },
                from: s("place"),
                within: Some(s("place")),
                only: names(),
            },
        )]
        .into(),
        range: [(
            s("range"),
            Range {
                enabled: false,
                name: Some(s("Range")),
                npc: vec![s("boss")],
                tint: vec![s("green")],
                radius: Some(1.0),
                spell: Some(1),
                sides: true,
                only: names(),
            },
        )]
        .into(),
        apart: [(
            s("apart"),
            Apart {
                enabled: false,
                name: Some(s("Apart")),
                npc: vec![s("boss"), s("boss")],
                within: 1.0,
                allow: vec![s("p")],
                grace: Some(1.0),
                only: names(),
            },
        )]
        .into(),
        streak: [(
            s("streak"),
            Streak {
                enabled: false,
                name: Some(s("Streak")),
                spell: vec![1],
                by: Some(s("boss")),
                most: Some(1),
                only: names(),
            },
        )]
        .into(),
        room: [(
            s("room"),
            Room {
                enabled: false,
                name: Some(s("Room")),
                about: Some(s("0")),
                through: Some(s("place")),
                enter: vec![Trigger::Hit {
                    spell: 1,
                    lands: false,
                }],
                leave: vec![Trigger::Hit {
                    spell: 1,
                    lands: false,
                }],
                npc: vec![s("boss")],
                center: Some([0.0, 0.0]),
                radius: Some(1.0),
                ui_map: Some(1),
                level: Some(1.0),
                ceiling: Some(1.0),
                light: Some(Light::Baked),
                scale: Some(1.0),
                only: names(),
            },
        )]
        .into(),
        reach: [(
            s("reach"),
            Reach {
                enabled: false,
                name: Some(s("Reach")),
                npc: s("boss"),
                place: s("place"),
                absorbs: true,
                only: names(),
            },
        )]
        .into(),
        event: [(
            s("e"),
            Event {
                enabled: false,
                name: Some(s("Event")),
                on: Trigger::Start,
                only: names(),
            },
        )]
        .into(),
        spell: [(
            s("1"),
            Spell {
                name: s("Spell"),
                radius: Some(1.0),
                duration: Some(1.0),
                period: Some(1.0),
                stacks: Some(1),
                triggers: vec![1],
            },
        )]
        .into(),
        travel: Travel {
            run: Some(1.0),
            mount: Some(1.0),
            vehicle: Some(1.0),
        },
    };
    e.event.insert(
        s("e2"),
        Event {
            on: Trigger::After(Clock::pull(1.0)),
            ..e.event[&s("e")].clone()
        },
    );
    // A ground that ends at the first of a hit and a time.
    e.ground.insert(
        s("shatter"),
        Ground {
            ends: Ends::First(vec![
                Ends::Hit {
                    spell: 1,
                    lands: true,
                },
                Ends::After(40.0),
            ]),
            ..e.ground[&s("ground")].clone()
        },
    );
    // A ground that ends on a hit, is pushed (never orbiting too) and
    // clears another as it forms, so their keys are written too.
    e.ground.insert(
        s("burst"),
        Ground {
            ends: Ends::Hit {
                spell: 1,
                lands: true,
            },
            clears: vec![s("ground")],
            sweeps: vec![s("shatter")],
            orbit: None,
            push: Some(Push {
                on: vec![Trigger::Hit {
                    spell: 1,
                    lands: false,
                }],
                toward: Toward::Bearings(vec![0.0, 90.0]),
                speed: 1.0,
                secs: 1.0,
            }),
            ..e.ground[&s("ground")].clone()
        },
    );
    // A line that follows whoever carries an aura and one that follows
    // its target (never orbiting too), so the key and both its forms are
    // written too.
    e.ground.insert(
        s("jet"),
        Ground {
            shape: Shape::Line {
                width: 1.0,
                length: 1.0,
            },
            follows: Some(Follows::Aura(1)),
            orbit: None,
            ..e.ground[&s("ground")].clone()
        },
    );
    e.ground.insert(
        s("beam"),
        Ground {
            shape: Shape::Cone {
                angle: 1.0,
                length: 1.0,
            },
            follows: Some(Follows::Target),
            orbit: None,
            ..e.ground[&s("ground")].clone()
        },
    );
    // A ground a firing inside renews (never standing `from` before its
    // firing), and one blown off the stencil by the way the players
    // drifted, so their keys and the drift's word are written too.
    e.ground.insert(
        s("residue"),
        Ground {
            from: None,
            renew: true,
            ..e.ground[&s("ground")].clone()
        },
    );
    e.ground.insert(
        s("blown"),
        Ground {
            from: None,
            renew: true,
            falls: true,
            orbit: None,
            push: Some(Push {
                on: vec![
                    Trigger::Hit {
                        spell: 1,
                        lands: false,
                    },
                    Trigger::Start,
                ],
                toward: Toward::Drift,
                speed: 1.0,
                secs: 1.0,
            }),
            ..e.ground[&s("ground")].clone()
        },
    );
    e
}

/// A level with every key of its own written: a pull with each optional
/// value set. Its structs are spelled out whole, so a field added to
/// `Level` or `Pull` does not build here until it is given a value.
pub(crate) fn every_level_key() -> Level {
    let s = |v: &str| v.to_string();
    Level {
        ui_map: 1,
        instance: s("dungeon"),
        map: Map::default(),
        view: View::default(),
        travel: Travel::default(),
        pull: [(
            s("first"),
            Pull {
                enabled: false,
                name: Some(s("Pull")),
                order: 1,
                optional: true,
                area: s("a"),
                npc: vec![s("boss"), s("boss")],
                spots: vec![[0.0, 0.0], [1.0, 1.0]],
            },
        )]
        .into(),
        npc: BTreeMap::new(),
    }
}

/// How many entries each kind of named entry holds, the map's layers,
/// glows, fades, areas and paths last. Every field is named, so a kind
/// added to the schema does not build here until it is counted or said not
/// to be.
fn entries(e: &Encounter) -> [usize; 21] {
    let Encounter {
        npc,
        ability,
        phase,
        place,
        ground,
        mark,
        line,
        volley,
        flash,
        knock,
        range,
        apart,
        streak,
        room,
        reach,
        event,
        map,
        schema: _,
        encounter: _,
        name: _,
        instance: _,
        order: _,
        view: _,
        spell: _,
        travel: _,
    } = e;
    [
        npc.len(),
        ability.len(),
        phase.len(),
        place.len(),
        ground.len(),
        mark.len(),
        line.len(),
        volley.len(),
        flash.len(),
        knock.len(),
        range.len(),
        apart.len(),
        streak.len(),
        room.len(),
        reach.len(),
        event.len(),
        map.layer.len(),
        map.glow.len(),
        map.fade.len(),
        map.area.len(),
        map.path.len(),
    ]
}

#[test]
fn every_kind_of_entry_is_switched_off_and_kept_to_its_difficulties() {
    // Every entry of every kind, switched off, and switched on and kept to
    // Normal (the map's entries have no `only`).
    let off = write::draft_text(&every_key(), &BTreeMap::new(), "every key").unwrap();
    let on = off.replace("enabled = false\n", "");
    assert_eq!(on.matches("enabled").count(), 0, "{on}");
    let resolve = |draft: &str, d: Option<u32>| {
        let r = Rubric::from_files(files(&[("s/raid/1-boss.draft.toml", draft)])).unwrap();
        entries(&r.encounter(1, d).unwrap().unwrap())
    };
    let all = entries(&every_key());
    assert!(all.iter().all(|n| *n > 0), "{all:?}");
    for d in [None, Some(14), Some(15)] {
        assert_eq!(resolve(&off, d), [0; 21], "{d:?}");
    }
    assert_eq!(resolve(&on, None), all);
    assert_eq!(resolve(&on, Some(14)), all);
    let mut heroic = [0; 21];
    heroic[16..].copy_from_slice(&all[16..]);
    assert_eq!(resolve(&on, Some(15)), heroic);
}

#[test]
fn an_only_naming_no_difficulty_is_found() {
    let typo = files(&[(
        "s/raid/7-x.toml",
        "schema = 1\nencounter = 7\nname = \"X\"\n\
         [ability.a]\nname = \"A\"\nonly = [\"mythic\", \"mythc\"]\n\
         [flash.f]\nspell = [1]\nonly = [\"Heroic\"]\n",
    )]);
    let r = Rubric::from_files(typo).unwrap();
    let problems = check(&r.encounter(7, None).unwrap().unwrap());
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].starts_with(
            "ability.a is kept to \"mythc\", which is not a difficulty (normal, heroic"
        ),
        "{problems:?}"
    );
    assert!(
        problems[1].starts_with("flash.f is kept to \"Heroic\""),
        "{problems:?}"
    );
    // On a difficulty it does name, the entry is there.
    let mythic = r.encounter(7, Some(16)).unwrap().unwrap();
    assert!(mythic.ability.contains_key("a") && !mythic.flash.contains_key("f"));
}

#[test]
fn an_encounter_is_given_once_by_the_newest_season_holding_it() {
    let draft = |name: &str| format!("schema = 1\nencounter = 3492\nname = \"{name}\"\n");
    // A second instance of the season giving Ula'tek again.
    let twice = files(&[
        ("s/other/instance.toml", INSTANCE),
        ("s/other/3492-ulatek.draft.toml", &draft("Again")),
    ]);
    let errs = Rubric::from_files(twice).unwrap_err();
    assert_eq!(
        errs,
        [
            "s/raid/3492-ulatek: encounter 3492 is s/other/3492-ulatek too; a season gives an encounter once"
        ],
    );
    // An older season giving it too: a returning dungeon. The newest
    // describes it, and it is listed once, there.
    let older = "schema = 1\nname = \"Older\"\norder = 1\n";
    let returning = files(&[
        ("old/season.toml", older),
        ("old/raid/instance.toml", INSTANCE),
        ("old/raid/3492-ulatek.draft.toml", &draft("Old Ula'tek")),
    ]);
    let r = Rubric::from_files(returning).unwrap();
    assert_eq!(r.encounter(3492, None).unwrap().unwrap().name, "Ula'tek");
    assert!(r.has_curated(3492));
    let listed: Vec<(u32, &str)> = r
        .encounters()
        .into_iter()
        .map(|(id, s, _)| (id, s.id.as_str()))
        .collect();
    assert_eq!(listed, [(3492, "s")]);
    // Two seasons of one order cannot say which is newer.
    let same = files(&[
        ("t/season.toml", &SEASON.replace("Test Season", "Twin")),
        ("t/raid/instance.toml", INSTANCE),
        ("t/raid/3492-ulatek.draft.toml", &draft("Twin")),
    ]);
    let errs = Rubric::from_files(same).unwrap_err();
    assert!(
        errs.len() == 1 && errs[0].contains("encounter 3492 is s/raid/3492-ulatek too"),
        "{errs:?}"
    );
}

/// A season's keystones' death penalty by level (`[rules]`): a key's
/// level takes the pair with the highest from-level at or under it, none
/// under the lowest, none where the season says nothing; a reader reaches
/// it by a run's map. It is the base's: a set's `season.toml` giving
/// `rules` is refused, and a misspelled rule is an error.
#[test]
fn a_season_gives_its_keys_death_penalty_by_level() {
    let mut f = files(&[]);
    f[0].1 = format!("{SEASON}\n[rules]\ndeath_penalty = [[7, 5], [2, 0], [12, 15]]\n");
    let r = Rubric::from_files(f).unwrap();
    let rules = &r.seasons()[0].rules;
    assert_eq!(rules.death_penalty, [(7, 5.0), (2, 0.0), (12, 15.0)]);
    let by_level: Vec<Option<f32>> = [1, 2, 5, 7, 11, 12, 20]
        .into_iter()
        .map(|l| rules.penalty_at(l))
        .collect();
    assert_eq!(
        by_level,
        [
            None,
            Some(0.0),
            Some(0.0),
            Some(5.0),
            Some(5.0),
            Some(15.0),
            Some(15.0)
        ]
    );
    let penalty = r
        .instance_by_map(3004)
        .and_then(|(s, _)| s.rules.penalty_at(14));
    assert_eq!(penalty, Some(15.0));
    // A season that says nothing knows no penalty.
    let bare = Rubric::from_files(files(&[])).unwrap();
    assert_eq!(bare.seasons()[0].rules, SeasonRules::default());
    assert_eq!(bare.seasons()[0].rules.penalty_at(14), None);
    // A from-level given twice, or a cost below 0 or not a number, is
    // refused, naming the file and the key.
    for (rules, says) in [
        ("[[2, 0], [7, 5], [7, 6]]", "+7 given twice"),
        ("[[2, -1]]", "+2 costs -1 s"),
        ("[[2, nan]]", "+2 costs NaN s"),
        ("[[2, inf]]", "+2 costs inf s"),
    ] {
        let mut f = files(&[]);
        f[0].1 = format!("{SEASON}\n[rules]\ndeath_penalty = {rules}\n");
        let errs = Rubric::from_files(f).unwrap_err();
        assert!(
            errs.iter().any(|e| e.starts_with("s/season.toml")
                && e.contains("`rules.death_penalty`")
                && e.contains(says)),
            "{rules}: {errs:?}"
        );
    }
    // A rule the schema lacks is refused, naming the file.
    let mut f = files(&[]);
    f[0].1 = format!("{SEASON}\n[rules]\ndeath_penalties = [[2, 0]]\n");
    let errs = Rubric::from_files(f).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.starts_with("s/season.toml") && e.contains("death_penalties")),
        "{errs:?}"
    );
    // A set's season file gives its defaults alone.
    let r = Rubric::from_files(files(&[])).unwrap().with_user(owned(&[(
        "s/season.toml",
        "schema = 2\n[rules]\ndeath_penalty = [[2, 30]]\n",
    )]));
    assert!(
        r.user_errors()
            .iter()
            .any(|e| e.starts_with("s/season.toml") && e.contains("`rules`")),
        "{:?}",
        r.user_errors()
    );
    assert_eq!(r.seasons()[0].rules.penalty_at(14), None);
}

#[test]
fn a_season_and_an_instance_are_read_as_they_are_written() {
    let r = Rubric::from_files(drawn("")).unwrap();
    let s = &r.seasons()[0];
    assert_eq!(
        (
            s.id.as_str(),
            s.name.as_str(),
            s.order,
            s.instances_from.as_slice()
        ),
        ("s", "Test Season", 2, &[100][..])
    );
    let i = &s.instances["raid"];
    assert_eq!(
        (
            i.slug.as_str(),
            i.name.as_str(),
            i.kind.as_str(),
            i.journal,
            i.map
        ),
        ("raid", "The Test Raid", "raid", None, Some(3004))
    );
    // A key neither knows is refused, naming the file.
    for (n, extra) in [(0, "id = \"s\"\n"), (1, "slug = \"raid\"\n")] {
        let mut f = files(&[]);
        f[n].1 = format!("{extra}{}", f[n].1);
        let errs = Rubric::from_files(f).unwrap_err();
        assert!(errs.iter().any(|e| e.contains(&f_path(n))), "{errs:?}");
    }
    let mut f = files(&[]);
    f[1].1 = format!("encounter = 3\n{}", f[1].1);
    let errs = Rubric::from_files(f).unwrap_err();
    assert!(
        errs.iter().any(|e| e.contains("`encounter` is a table")),
        "{errs:?}"
    );
}

fn f_path(n: usize) -> String {
    files(&[]).remove(n).0
}

// ---- the layer stack -------------------------------------------------------

/// The test raid's base alone: the season, the instance and the draft, no
/// tuned file.
fn base_files() -> Vec<(String, String)> {
    files(&[])
        .into_iter()
        .filter(|(p, _)| p != "s/raid/3492-ulatek.toml")
        .collect()
}

fn owned(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect()
}

/// The tuned file embedded beside the draft, for the stack: a scalar, a
/// list and one key of the bite's shape, and a Heroic override.
const STACK_TUNED: &str = r#"
schema = 1

[view]
turn = 90

[ability.bite]
spell = [3]
shape = { length = 40 }

[difficulty.heroic.ability.bite]
shape = { length = 42 }
"#;

/// Two bundles, laid in this order: the second's name sorts first, so the
/// call order, not the name, is what lays it over the first.
const ZETA: &str = r#"
schema = 1

[view]
turn = 45

[ability.bite]
spell = [4, 5]
shape = { angle = 70 }
"#;

const ALPHA: &str = r#"
schema = 1

[view]
turn = 30
"#;

/// The user's: the turn again, one more key of the shape, and a Mythic
/// override of the shape's length.
const MINE: &str = r#"
schema = 1

[view]
turn = 10

[ability.bite.shape]
length = 50

[difficulty.mythic.ability.bite.shape]
length = 60
"#;

/// The whole stack over the test raid: the base (with the instance's
/// drawing and a default of the instance's), the tuned file embedded,
/// two bundles and the user's file.
fn stacked() -> Rubric {
    let mut f = drawn("");
    f[1].1 = f[1]
        .1
        .replace("map = 3004\n", "map = 3004\n\n[defaults.map]\nlevel = 5\n");
    f[3].1 = STACK_TUNED.to_string();
    Rubric::from_files(f)
        .unwrap()
        .with_curated("zeta", owned(&[("s/raid/3492-ulatek.toml", ZETA)]))
        .unwrap()
        .with_curated("alpha", owned(&[("s/raid/3492-ulatek.toml", ALPHA)]))
        .unwrap()
        .with_user(owned(&[("s/raid/3492-ulatek.toml", MINE)]))
}

fn bite(e: &Encounter) -> (Vec<u32>, f32, f32) {
    match &e.ability["bite"].shape {
        Some(Shape::Cone { length, angle }) => (e.ability["bite"].spell.clone(), *length, *angle),
        s => panic!("{s:?}"),
    }
}

fn bundle(name: &str) -> Source {
    Source::Curated(Origin::Bundle(name.into()))
}

#[test]
fn the_stack_lays_base_curated_bundles_then_the_user() {
    let r = stacked();
    assert!(r.user_errors().is_empty(), "{:?}", r.user_errors());
    let at = |tier| r.encounter_at(3492, None, tier).unwrap().unwrap();
    // The base: the draft's list and cone; no turn but the drawing's zoom.
    let base = at(Tier::Base);
    assert_eq!(bite(&base), (vec![1, 2], 35.0, 60.0));
    assert_eq!(base.view.turn, None);
    // Curated: the embedded file, then the bundles in the order laid. A
    // scalar is the last one's (alpha's turn over zeta's), a list is
    // replaced whole (zeta's spells, none of the draft's or the tuned
    // file's), a table merges key by key (the tuned length, zeta's angle).
    let curated = at(Tier::Curated);
    assert_eq!(curated.view.turn, Some(30.0));
    assert_eq!(bite(&curated), (vec![4, 5], 40.0, 70.0));
    // The user's over all of it.
    let user = at(Tier::User);
    assert_eq!(user.view.turn, Some(10.0));
    assert_eq!(bite(&user), (vec![4, 5], 50.0, 70.0));
    // What none of them names stays the base's.
    assert_eq!(user.view.zoom, Some(2.0));
    assert_eq!(user.map.floor.as_deref(), Some("ulatek"));
    assert_eq!(user.map.stencil.len(), 2);
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), user);
}

#[test]
fn provenance_names_the_source_that_last_set_each_key() {
    let r = stacked();
    let traced = |tier, d| r.encounter_traced(3492, d, tier).unwrap().unwrap().1;
    let p = traced(Tier::User, None);
    let src = |p: &Provenance, path: &str| {
        let by = p.source_of(path).unwrap_or_else(|| panic!("{path}"));
        (by.source.clone(), by.difficulty)
    };
    assert_eq!(src(&p, "view.zoom"), (Source::Drawing, None));
    assert_eq!(src(&p, "map.ceiling"), (Source::Season, None));
    assert_eq!(src(&p, "map.level"), (Source::Instance, None));
    assert_eq!(src(&p, "map.floor"), (Source::Draft, None));
    assert_eq!(src(&p, "map.stencil"), (Source::Drawing, None));
    assert_eq!(src(&p, "map.layer.platform.wmo"), (Source::Drawing, None));
    assert_eq!(src(&p, "instance"), (Source::Instance, None));
    assert_eq!(src(&p, "ability.bite.shape.kind"), (Source::Draft, None));
    assert_eq!(src(&p, "ability.bite.shape.angle"), (bundle("zeta"), None));
    assert_eq!(src(&p, "ability.bite.spell"), (bundle("zeta"), None));
    assert_eq!(src(&p, "ability.bite.shape.length"), (Source::User, None));
    assert_eq!(src(&p, "view.turn"), (Source::User, None));
    // A list is laid whole: a key inside it answers for the list.
    assert_eq!(src(&p, "map.stencil.1"), (Source::Drawing, None));
    // A table merged key by key is no path of its own.
    assert!(p.source_of("ability.bite").is_none());
    assert_eq!(p.under("ability.bite.shape").count(), 3);
    // At the curated tier the embedded file and alpha speak where the user
    // did; at the base, the draft.
    let p = traced(Tier::Curated, None);
    assert_eq!(src(&p, "view.turn"), (bundle("alpha"), None));
    assert_eq!(
        src(&p, "ability.bite.shape.length"),
        (Source::Curated(Origin::Embedded), None)
    );
    let p = traced(Tier::Base, None);
    assert_eq!(src(&p, "ability.bite.shape.length"), (Source::Draft, None));
    assert!(p.source_of("view.turn").is_none());
    // A difficulty's overrides come last, each naming its difficulty: the
    // embedded Heroic length on Heroic and, through the chain, on Mythic
    // until the user's own Mythic override.
    let p = traced(Tier::User, Some(15));
    assert_eq!(
        src(&p, "ability.bite.shape.length"),
        (Source::Curated(Origin::Embedded), Some("heroic"))
    );
    let p = traced(Tier::User, Some(16));
    assert_eq!(
        src(&p, "ability.bite.shape.length"),
        (Source::User, Some("mythic"))
    );
    let p = traced(Tier::Curated, Some(16));
    assert_eq!(
        src(&p, "ability.bite.shape.length"),
        (Source::Curated(Origin::Embedded), Some("heroic"))
    );
    // The draft's own Mythic-only entry, and its switch.
    let p = traced(Tier::User, None);
    assert_eq!(src(&p, "ability.mythic-only.only"), (Source::Draft, None));
}

#[test]
fn a_user_difficulty_override_applies_only_where_asked() {
    let r = stacked();
    let length = |d| bite(&r.encounter(3492, d).unwrap().unwrap()).1;
    // No difficulty, and Normal: the user's own length.
    assert_eq!(length(None), 50.0);
    assert_eq!(length(Some(14)), 50.0);
    // Heroic: the tuned file's Heroic override, laid after every source's
    // own values.
    assert_eq!(length(Some(15)), 42.0);
    // Mythic and a keystone, which reads Mythic's: the user's Mythic one.
    assert_eq!(length(Some(16)), 60.0);
    assert_eq!(length(Some(8)), 60.0);
}

#[test]
fn the_tier_caps_the_stack_at_each_tier() {
    let r = stacked();
    let turns = |r: &Rubric| -> Vec<Option<f32>> {
        Tier::ALL
            .iter()
            .map(|&t| r.encounter_at(3492, None, t).unwrap().unwrap().view.turn)
            .collect()
    };
    assert_eq!(turns(&r), [None, Some(30.0), Some(10.0)]);
    let capped = r.clone().with_tier(Tier::Curated);
    assert_eq!(capped.tier(), Tier::Curated);
    assert_eq!(turns(&capped), [None, Some(30.0), Some(30.0)]);
    assert_eq!(
        capped.encounter(3492, None).unwrap().unwrap(),
        r.encounter_at(3492, None, Tier::Curated).unwrap().unwrap()
    );
    let base = r.clone().with_tier(Tier::Base);
    assert_eq!(turns(&base), [None, None, None]);
    let user = base.with_tier(Tier::User);
    assert_eq!(turns(&user), turns(&r));
}

#[test]
fn a_tuned_file_embedded_and_laid_in_resolve_alike() {
    let embedded = Rubric::from_files(files(&[])).unwrap();
    let laid = Rubric::from_files(base_files())
        .unwrap()
        .with_curated("pack", owned(&[("s/raid/3492-ulatek.toml", TUNED)]))
        .unwrap();
    for tier in Tier::ALL {
        for d in [None, Some(14), Some(15), Some(16), Some(8)] {
            assert_eq!(
                embedded.encounter_at(3492, d, tier).unwrap().unwrap(),
                laid.encounter_at(3492, d, tier).unwrap().unwrap(),
                "{} {d:?}",
                tier.name()
            );
        }
    }
    // The same keys, set by the curated layer under either name.
    let (_, a) = embedded
        .encounter_traced(3492, None, Tier::Curated)
        .unwrap()
        .unwrap();
    let (_, b) = laid
        .encounter_traced(3492, None, Tier::Curated)
        .unwrap()
        .unwrap();
    let rename = |s: &Source| match s {
        Source::Curated(_) => Source::Curated(Origin::Embedded),
        s => s.clone(),
    };
    let a: Vec<(&str, Source)> = a.iter().map(|(k, by)| (k, rename(&by.source))).collect();
    let b: Vec<(&str, Source)> = b.iter().map(|(k, by)| (k, rename(&by.source))).collect();
    assert_eq!(a, b);
    assert_eq!(
        embedded.layers_of(3492).last(),
        Some(&Source::Curated(Origin::Embedded))
    );
    assert_eq!(laid.layers_of(3492).last(), Some(&bundle("pack")));
}

#[test]
fn a_traced_encounter_is_the_one_answered_untraced() {
    let r = stacked();
    for tier in Tier::ALL {
        for d in [None, Some(14), Some(15), Some(16), Some(8)] {
            let (traced, p) = r.encounter_traced(3492, d, tier).unwrap().unwrap();
            assert_eq!(traced, r.encounter_at(3492, d, tier).unwrap().unwrap());
            assert!(!p.is_empty() && p.len() == p.iter().count());
        }
    }
    // A failing encounter fails the same traced, and an absent one is absent.
    let mut f = files(&[]);
    f[3].1 = format!("{TUNED}\n[view.extra]\nzom = 1\n");
    let bad = Rubric::from_files(f).unwrap();
    assert_eq!(
        bad.encounter_traced(3492, None, Tier::User)
            .unwrap()
            .unwrap_err(),
        bad.encounter_at(3492, None, Tier::User)
            .unwrap()
            .unwrap_err()
    );
    assert!(r.encounter_traced(9, None, Tier::User).is_none());
}

#[test]
fn every_key_of_a_draft_is_the_drafts_until_a_layer_sets_it() {
    let draft = write::draft_text(&every_key(), &BTreeMap::new(), "every key").unwrap();
    let r = Rubric::from_files(files(&[("s/raid/1-boss.draft.toml", &draft)])).unwrap();
    let (e, p) = r.encounter_traced(1, None, Tier::User).unwrap().unwrap();
    assert_eq!(e, r.encounter(1, None).unwrap().unwrap());
    // Every key the draft wrote is the draft's: the season's defaults it
    // also sets are its own now.
    let not_draft: Vec<(&str, &SetBy)> = p
        .iter()
        .filter(|(k, by)| by.source != Source::Draft && *k != "instance")
        .collect();
    assert!(not_draft.is_empty(), "{not_draft:?}");
    // A user file setting one key moves that key alone (and the format
    // number every file carries).
    let r = r.with_user(owned(&[(
        "s/raid/1-boss.toml",
        "schema = 1\n[view]\nturn = 12\n",
    )]));
    let (_, q) = r.encounter_traced(1, None, Tier::User).unwrap().unwrap();
    let moved: Vec<&str> = q
        .iter()
        .filter(|(k, by)| p.source_of(k) != Some(by))
        .map(|(k, _)| k)
        .collect();
    assert_eq!(moved, ["schema", "view.turn"]);
    assert_eq!(q.source_of("view.turn").unwrap().source, Source::User);
}

#[test]
fn a_bad_user_file_is_named_and_the_rest_are_laid() {
    let r = Rubric::from_files(files(&[])).unwrap().with_user(owned(&[
        // A key the schema lacks: it would make Ula'tek unreadable.
        ("s/raid/3492-ulatek.toml", "schema = 1\n[view]\nzom = 1\n"),
        // Not TOML.
        ("s/raid/7-x.toml", "schema = 1\n[view\n"),
        // A draft is the base's.
        ("s/raid/3492-ulatek.draft.toml", "schema = 1\n"),
        // No such instance in the base.
        ("s/nowhere/instance.toml", "schema = 1\n"),
        // What a season is stays the base's.
        ("s/season.toml", "schema = 1\nname = \"Mine\"\n"),
        // And one that is fine: a season-wide default.
        ("s/other.txt", "not a rubric file"),
    ]));
    let errs = r.user_errors();
    for (file, says) in [
        ("s/raid/3492-ulatek.toml", "zom"),
        ("s/raid/7-x.toml", "7-x"),
        ("s/raid/3492-ulatek.draft.toml", "a draft is the base's"),
        ("s/nowhere/instance.toml", "no s/nowhere"),
        ("s/season.toml", "`name`"),
        ("s/other.txt", "not where a rubric file goes"),
    ] {
        assert!(
            errs.iter().any(|e| e.starts_with(file) && e.contains(says)),
            "{file}: {errs:?}"
        );
    }
    assert_eq!(errs.len(), 6, "{errs:?}");
    // Nothing of them was laid: Ula'tek reads as it did, and the user
    // touches nothing.
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), ulatek(None));
    assert!(!r.has_user(3492));
    // A good file beside a bad one is laid.
    let r = Rubric::from_files(files(&[])).unwrap().with_user(owned(&[
        ("s/raid/3492-ulatek.toml", "schema = 1\n[view]\nzom = 1\n"),
        ("s/season.toml", "schema = 1\n[defaults.view]\nzoom = 3\n"),
    ]));
    assert_eq!(r.user_errors().len(), 1, "{:?}", r.user_errors());
    let e = r.encounter(3492, None).unwrap().unwrap();
    assert_eq!(e.view.zoom, Some(3.0));
    assert!(r.has_user(3492));
    assert_ne!(
        r.version(),
        Rubric::from_files(files(&[])).unwrap().version()
    );
}

#[test]
fn a_bad_curated_bundle_is_refused_whole() {
    let base = || Rubric::from_files(base_files()).unwrap();
    let errs = base()
        .with_curated(
            "broken",
            owned(&[
                ("s/raid/3492-ulatek.toml", "schema = 1\n[view]\nturn = 5\n"),
                ("s/raid/3492-ulatek.draft.toml", DRAFT),
            ]),
        )
        .unwrap_err();
    assert!(
        errs.len() == 1 && errs[0].contains("a draft is the base's"),
        "{errs:?}"
    );
    let errs = base()
        .with_curated(
            "typo",
            owned(&[("s/raid/3492-ulatek.toml", "schema = 1\n[view]\nzom = 1\n")]),
        )
        .unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("curated \"typo\"") && e.contains("zom")),
        "{errs:?}"
    );
    // A slug the encounter's other files do not have.
    let errs = base()
        .with_curated("slug", owned(&[("s/raid/3492-other.toml", "schema = 1\n")]))
        .unwrap_err();
    assert!(
        errs[0].contains("already has files named 3492-ulatek"),
        "{errs:?}"
    );
}

#[test]
fn a_layer_lays_a_season_an_instance_and_a_new_entry() {
    // A bundle's season and instance defaults reach every encounter of
    // theirs, its drawing reaches its encounter, and an entry the base
    // lacks is added.
    let r = Rubric::from_files(base_files())
        .unwrap()
        .with_curated(
            "pack",
            owned(&[
                ("s/season.toml", "schema = 1\n[defaults.map]\nceiling = 40\n"),
                (
                    "s/raid/instance.toml",
                    "schema = 1\n[defaults.view]\nzoom = 1.5\n[encounter.3492.npc.heart]\nname = \"Venomous Heart\"\nshares = \"ulatek\"\n",
                ),
                (
                    "s/raid/3492-ulatek.toml",
                    "schema = 1\n[ability.extra]\nname = \"Extra\"\nby = \"ulatek\"\n",
                ),
            ]),
        )
        .unwrap();
    let (e, p) = r
        .encounter_traced(3492, None, Tier::Curated)
        .unwrap()
        .unwrap();
    assert_eq!(e.map.ceiling, Some(40.0));
    assert_eq!(e.view.zoom, Some(1.5));
    assert_eq!(e.npc["heart"].shares.as_deref(), Some("ulatek"));
    assert!(e.ability.contains_key("extra"));
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    for path in [
        "map.ceiling",
        "view.zoom",
        "npc.heart.shares",
        "ability.extra.name",
    ] {
        assert_eq!(p.source_of(path).unwrap().source, bundle("pack"), "{path}");
    }
    // The base under it is untouched.
    let base = r.encounter_at(3492, None, Tier::Base).unwrap().unwrap();
    assert_eq!(base.map.ceiling, Some(25.0));
    assert!(!base.ability.contains_key("extra"));
    // A drawing for an encounter the instance has no files for is refused.
    let errs = Rubric::from_files(base_files())
        .unwrap()
        .with_curated(
            "pack",
            owned(&[(
                "s/raid/instance.toml",
                "schema = 1\n[encounter.9.map]\nlevel = 1\n",
            )]),
        )
        .unwrap_err();
    assert!(errs[0].contains("no files for encounter 9"), "{errs:?}");
}

#[test]
fn a_layer_may_bring_an_encounter_the_base_lacks_from_its_tier_up() {
    let r = Rubric::from_files(base_files())
        .unwrap()
        .with_user(owned(&[(
            "s/raid/3500-new-boss.toml",
            "schema = 1\nencounter = 3500\nname = \"New Boss\"\n",
        )]));
    assert!(r.user_errors().is_empty(), "{:?}", r.user_errors());
    assert!(r.encounter_at(3500, None, Tier::Base).is_none());
    assert!(r.encounter_at(3500, None, Tier::Curated).is_none());
    assert_eq!(r.encounter(3500, None).unwrap().unwrap().name, "New Boss");
    assert!(r.encounters().iter().any(|(id, _, _)| *id == 3500));
    assert_eq!(
        r.layers_of(3500),
        [Source::Season, Source::Instance, Source::User]
    );
}

#[test]
fn layers_of_lists_the_sources_with_a_file_an_encounter_reads() {
    let r = stacked();
    assert_eq!(
        r.layers_of(3492),
        [
            Source::Season,
            Source::Instance,
            Source::Draft,
            Source::Drawing,
            Source::Curated(Origin::Embedded),
            bundle("zeta"),
            bundle("alpha"),
            Source::User,
        ]
    );
    assert!(r.has_curated(3492) && r.has_user(3492));
    let base = Rubric::from_files(base_files()).unwrap();
    assert_eq!(
        base.layers_of(3492),
        [Source::Season, Source::Instance, Source::Draft]
    );
    assert!(!base.has_curated(3492) && !base.has_user(3492));
    assert!(base.layers_of(9).is_empty());
    // A user season file touches every encounter of its season.
    let r = base.with_user(owned(&[(
        "s/season.toml",
        "schema = 1\n[defaults.view]\nzoom = 3\n",
    )]));
    assert_eq!(r.layers_of(3492).last(), Some(&Source::User));
}

#[test]
fn the_user_directory_is_read_whole_and_its_bad_files_named_by_their_place() {
    let root = std::env::temp_dir().join(format!(
        "wowdps-encounter-rubric-user-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("s/raid")).unwrap();
    std::fs::write(root.join("README.md"), "my notes\n").unwrap();
    std::fs::write(
        root.join("s/season.toml"),
        "schema = 1\n[defaults.view]\nzoom = 3\n",
    )
    .unwrap();
    std::fs::write(root.join("s/raid/3492-ulatek.toml"), "schema = 1\n[view\n").unwrap();
    let r = Rubric::from_files(base_files())
        .unwrap()
        .with_user_dir(Some(root.clone()));
    let errs = r.user_errors().to_vec();
    assert_eq!(errs.len(), 1, "{errs:?}");
    let shown = root.join("s/raid/3492-ulatek.toml").display().to_string();
    assert!(errs[0].starts_with(&shown), "{errs:?}");
    assert_eq!(
        r.encounter(3492, None).unwrap().unwrap().view.zoom,
        Some(3.0)
    );
    // A directory that is not there lays nothing and says nothing.
    let none = Rubric::from_files(base_files())
        .unwrap()
        .with_user_dir(Some(root.join("missing")));
    assert!(none.user_errors().is_empty() && !none.has_user(3492));
    assert_eq!(
        none.version(),
        Rubric::from_files(base_files()).unwrap().version(),
        "laying nothing leaves the version"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A `seasons/` tree on disk under the system's temp directory, by `name`
/// and this process: each file at its path under it, the tree made anew.
fn tree_on_disk(name: &str, files: &[(String, String)]) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "wowdps-encounter-rubric-tree-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (rel, text) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

fn dir_source(name: &str) -> Source {
    Source::Curated(Origin::Dir(name.into()))
}

/// The tree's own tuned file: the turn again and a Heroic override, none
/// of the embedded file's entries.
const TREE_TUNED: &str = r#"
schema = 1

[view]
turn = 45

[difficulty.heroic.ability.bite]
shape = { length = 41 }
"#;

#[test]
fn a_tree_on_disk_lays_its_tuned_files_in_the_embedded_ones_place() {
    // The tree's base files say otherwise than the build's and are not
    // read: a season and an instance file a set could not give, a draft.
    let tree = tree_on_disk(
        "lays",
        &owned(&[
            ("s/season.toml", &SEASON.replace("zoom = 1.15", "zoom = 9")),
            (
                "s/raid/instance.toml",
                &format!("{INSTANCE}\n[encounter.3492.view]\nzoom = 8\n"),
            ),
            (
                "s/raid/3492-ulatek.draft.toml",
                &DRAFT.replace("name = \"Ula'tek\"\norder", "name = \"Not Read\"\norder"),
            ),
            ("s/raid/3492-ulatek.toml", TREE_TUNED),
            ("s/raid/notes.txt", "not a rubric file\n"),
        ]),
    );
    let embedded = || Rubric::from_files(files(&[])).unwrap();
    let r = embedded().with_curated_dir("tree", &tree).unwrap();
    // The same as a build whose tuned file was the tree's.
    let mut built = base_files();
    built.push(("s/raid/3492-ulatek.toml".into(), TREE_TUNED.into()));
    let built = Rubric::from_files(built).unwrap();
    for tier in Tier::ALL {
        for d in [None, Some(14), Some(15), Some(16), Some(8)] {
            assert_eq!(
                r.encounter_at(3492, d, tier),
                built.encounter_at(3492, d, tier),
                "{} {d:?}",
                tier.name()
            );
        }
    }
    let e = r
        .encounter_at(3492, Some(15), Tier::Curated)
        .unwrap()
        .unwrap();
    assert_eq!(e.name, "Ula'tek");
    assert_eq!(e.view.zoom, Some(1.15));
    assert_eq!(e.view.turn, Some(45.0));
    assert_eq!(bite(&e).1, 41.0);
    // The embedded file's entries are gone with it.
    assert!(e.place.is_empty() && e.map.layer.is_empty(), "{e:?}");
    assert_eq!(
        r.layers_of(3492),
        [
            Source::Season,
            Source::Instance,
            Source::Draft,
            dir_source("tree")
        ]
    );
    assert!(r.has_curated(3492));
    let (_, p) = r
        .encounter_traced(3492, None, Tier::Curated)
        .unwrap()
        .unwrap();
    assert_eq!(p.source_of("view.turn").unwrap().source, dir_source("tree"));
    // Read again unchanged, the same version; a file changed, another.
    let again = embedded().with_curated_dir("tree", &tree).unwrap();
    assert_eq!(again.version(), r.version());
    assert_ne!(r.version(), embedded().version());
    std::fs::write(
        tree.join("s/raid/3492-ulatek.toml"),
        TREE_TUNED.replace("45", "46"),
    )
    .unwrap();
    let changed = embedded().with_curated_dir("tree", &tree).unwrap();
    assert_ne!(changed.version(), r.version());
    assert_eq!(
        changed
            .encounter_at(3492, None, Tier::Curated)
            .unwrap()
            .unwrap()
            .view
            .turn,
        Some(46.0)
    );
    let _ = std::fs::remove_dir_all(&tree);
}

/// A curated tree a walk read lays as its directory does, and lays the
/// files the walk read: a file saved after the walk waits for the next
/// walk. A walk of a root that is not there is refused as no directory.
#[test]
fn a_curated_tree_a_walk_read_lays_as_its_directory_does() {
    let tree = tree_on_disk("walked", &owned(&[("s/raid/3492-ulatek.toml", TREE_TUNED)]));
    let base = || Rubric::from_files(files(&[])).unwrap();
    let walked = TreeFiles::read(&tree);
    let by_dir = base().with_curated_dir("tree", &tree).unwrap();
    // Saved after the walk: the walk's text is the one laid.
    std::fs::write(
        tree.join("s/raid/3492-ulatek.toml"),
        TREE_TUNED.replace("45", "46"),
    )
    .unwrap();
    let by_walk = base().with_curated_tree("tree", walked).unwrap();
    assert_eq!(by_walk.version(), by_dir.version());
    for tier in Tier::ALL {
        for d in [None, Some(15), Some(16)] {
            assert_eq!(
                by_walk.encounter_at(3492, d, tier),
                by_dir.encounter_at(3492, d, tier),
                "{} {d:?}",
                tier.name()
            );
        }
    }
    assert_eq!(by_walk.layers_of(3492), by_dir.layers_of(3492));
    let again = base()
        .with_curated_tree("tree", TreeFiles::read(&tree))
        .unwrap();
    assert_ne!(again.version(), by_walk.version());
    let missing = tree.join("not-there");
    let errs = base()
        .with_curated_tree("tree", TreeFiles::read(&missing))
        .unwrap_err();
    assert_eq!(errs, [format!("{}: not a directory", missing.display())]);
    let _ = std::fs::remove_dir_all(&tree);
}

#[test]
fn a_tree_on_disk_sits_under_the_bundles_and_a_second_takes_its_place() {
    let tree = tree_on_disk("order", &owned(&[("s/raid/3492-ulatek.toml", TREE_TUNED)]));
    let empty = tree_on_disk("order-empty", &[]);
    let r = Rubric::from_files(files(&[]))
        .unwrap()
        .with_curated("alpha", owned(&[("s/raid/3492-ulatek.toml", ALPHA)]))
        .unwrap()
        .with_curated_dir("first", &empty)
        .unwrap()
        .with_curated_dir("tree", &tree)
        .unwrap()
        .with_user(owned(&[("s/raid/3492-ulatek.toml", MINE)]));
    assert_eq!(
        r.layers_of(3492),
        [
            Source::Season,
            Source::Instance,
            Source::Draft,
            dir_source("tree"),
            bundle("alpha"),
            Source::User
        ]
    );
    // The bundle's turn over the tree's.
    let curated = r.encounter_at(3492, None, Tier::Curated).unwrap().unwrap();
    assert_eq!(curated.view.turn, Some(30.0));
    // A tree with no tuned file leaves the curated tier reading the base;
    // over a base with none, the version as it was.
    let none = Rubric::from_files(files(&[]))
        .unwrap()
        .with_curated_dir("empty", &empty)
        .unwrap();
    assert!(!none.has_curated(3492));
    assert_eq!(
        none.encounter_at(3492, None, Tier::Curated),
        none.encounter_at(3492, None, Tier::Base)
    );
    let base = Rubric::from_files(base_files()).unwrap();
    assert_eq!(
        base.clone()
            .with_curated_dir("empty", &empty)
            .unwrap()
            .version(),
        base.version()
    );
    // An encounter only the embedded files brought goes with them, and
    // stays where the tree holds its file too.
    const NEW: &str = "schema = 1\nencounter = 3501\nname = \"New\"\n";
    let brought = || Rubric::from_files(files(&[("s/raid/3501-new.toml", NEW)])).unwrap();
    assert!(brought().encounters().iter().any(|(id, _, _)| *id == 3501));
    let gone = brought().with_curated_dir("empty", &empty).unwrap();
    assert!(gone.encounters().iter().all(|(id, _, _)| *id != 3501));
    assert!(gone.layers_of(3501).is_empty());
    let kept_tree = tree_on_disk("order-kept", &owned(&[("s/raid/3501-new.toml", NEW)]));
    let kept = brought().with_curated_dir("kept", &kept_tree).unwrap();
    assert_eq!(
        kept.encounter_at(3501, None, Tier::Curated)
            .unwrap()
            .unwrap()
            .name,
        "New"
    );
    assert!(kept.encounter_at(3501, None, Tier::Base).is_none());
    let _ = std::fs::remove_dir_all(&tree);
    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&kept_tree);
}

#[test]
fn a_tree_on_disk_that_takes_away_what_a_bundle_leans_on_is_refused() {
    // The embedded tuned file gives the place "pool" its kind and spot, and
    // a bundle over it only sizes the place. A tree with no file for the
    // encounter takes the kind and spot away under the bundle, and a place
    // with neither does not read: refused, naming the tree, though the tree
    // itself holds nothing. One holding the file is laid.
    const LEANS: &str = "schema = 1\n\n[place.pool]\nradius = 5\n";
    let leaning = || {
        Rubric::from_files(files(&[]))
            .unwrap()
            .with_curated("leans", owned(&[("s/raid/3492-ulatek.toml", LEANS)]))
            .unwrap()
    };
    let empty = tree_on_disk("leans-empty", &[]);
    let errs = leaning().with_curated_dir("bare", &empty).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("curated \"bare\"") && e.contains("3492") && e.contains("pool")),
        "{errs:?}"
    );
    let whole = tree_on_disk("leans-whole", &owned(&[("s/raid/3492-ulatek.toml", TUNED)]));
    let r = leaning().with_curated_dir("whole", &whole).unwrap();
    assert_eq!(
        r.layers_of(3492),
        [
            Source::Season,
            Source::Instance,
            Source::Draft,
            dir_source("whole"),
            bundle("leans"),
        ]
    );
}

#[test]
fn a_users_file_laid_before_a_tree_is_read_again_over_it() {
    // The user's file only sizes the place the embedded tuned file gives,
    // and it reads as laid. A tree with no file for the encounter is laid
    // after it and takes the place's kind and spot away: the tree is fine
    // (the curated tier reads), the user's file is not, so it is left out
    // and named, as it would be had it come last. A bundle laid after the
    // user's files is read the same way.
    const LEANS: &str = "schema = 1\n\n[place.pool]\nradius = 7\n";
    let empty = tree_on_disk("user-first-empty", &[]);
    let user_first = || {
        Rubric::from_files(files(&[]))
            .unwrap()
            .with_user(owned(&[("s/raid/3492-ulatek.toml", LEANS)]))
    };
    assert!(user_first().user_errors().is_empty());
    let r = user_first().with_curated_dir("bare", &empty).unwrap();
    assert!(
        r.user_errors()
            .iter()
            .any(|e| e.contains("s/raid/3492-ulatek.toml") && e.contains("curated \"bare\"")),
        "{:?}",
        r.user_errors()
    );
    let e = r.encounter_at(3492, None, Tier::User).unwrap().unwrap();
    assert!(!e.place.contains_key("pool"));
    assert!(!r.layers_of(3492).contains(&Source::User));
    // Laid last, lay_user leaves the same file out itself.
    let last = Rubric::from_files(files(&[]))
        .unwrap()
        .with_curated_dir("bare", &empty)
        .unwrap()
        .with_user(owned(&[("s/raid/3492-ulatek.toml", LEANS)]));
    assert_eq!(last.user_errors().len(), 1, "{:?}", last.user_errors());
    // A bundle that takes nothing away leaves the user's file laid.
    let kept = user_first()
        .with_curated("alpha", owned(&[("s/raid/3492-ulatek.toml", ALPHA)]))
        .unwrap();
    assert!(kept.user_errors().is_empty(), "{:?}", kept.user_errors());
    assert!(kept.layers_of(3492).contains(&Source::User));
}

#[test]
fn a_bad_tree_on_disk_is_refused_whole_naming_its_files_by_their_place() {
    let base = || Rubric::from_files(files(&[])).unwrap();
    let missing = std::env::temp_dir().join("wowdps-encounter-rubric-tree-not-there");
    let errs = base().with_curated_dir("gone", &missing).unwrap_err();
    assert!(errs[0].contains("not a directory"), "{errs:?}");
    // A typo, named by the file's place on disk.
    let typo = tree_on_disk(
        "typo",
        &owned(&[("s/raid/3492-ulatek.toml", "schema = 1\n[view]\nzom = 1\n")]),
    );
    let errs = base().with_curated_dir("typo", &typo).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("curated \"typo\"") && e.contains("zom")),
        "{errs:?}"
    );
    // A file that cannot be laid, by its place on disk; and the rest of
    // the set is not laid either.
    let tree = tree_on_disk(
        "slug",
        &owned(&[
            ("s/raid/3492-other.toml", "schema = 1\n"),
            (
                "s/raid/3501-new.toml",
                "schema = 1\nencounter = 3501\nname = \"New\"\n",
            ),
        ]),
    );
    let errs = base().with_curated_dir("slug", &tree).unwrap_err();
    let shown = tree.join("s/raid/3492-other.toml").display().to_string();
    assert!(
        errs.len() == 1 && errs[0].starts_with(&shown) && errs[0].contains("already has files"),
        "{errs:?}"
    );
    let _ = std::fs::remove_dir_all(&typo);
    let _ = std::fs::remove_dir_all(&tree);
}

/// The checkout's own `seasons/`, laid from disk in place of the files the
/// crate embedded from it, resolves every encounter as the embedded rubric
/// does, its curated layer named for the tree. The tree holds no tuned file
/// now (the curated set lives at the repository's `curated/seasons/`), so
/// this holds a tree of the base alone; the replay's tests lay that set
/// from disk and hold it so.
#[test]
fn the_checkouts_seasons_laid_from_disk_resolve_as_the_embedded() {
    let seasons = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("seasons");
    let embedded = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let disk = Rubric::embedded()
        .unwrap()
        .with_curated_dir("seasons", &seasons)
        .unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let rename = |s: &Source| match s {
        Source::Curated(Origin::Embedded) => dir_source("seasons"),
        s => s.clone(),
    };
    let mut curated = 0;
    for (id, _, _) in embedded.encounters() {
        for d in [
            None,
            Some(1),
            Some(2),
            Some(23),
            Some(8),
            Some(14),
            Some(15),
            Some(16),
            Some(17),
        ] {
            for tier in Tier::ALL {
                assert_eq!(
                    disk.encounter_at(id, d, tier),
                    embedded.encounter_at(id, d, tier),
                    "{id} {d:?} {}",
                    tier.name()
                );
            }
        }
        let want: Vec<Source> = embedded.layers_of(id).iter().map(rename).collect();
        assert_eq!(disk.layers_of(id), want, "{id}");
        if embedded.has_curated(id) {
            curated += 1;
            assert!(disk.layers_of(id).contains(&dir_source("seasons")), "{id}");
            assert!(
                !disk
                    .layers_of(id)
                    .contains(&Source::Curated(Origin::Embedded))
            );
        }
        // Every key set by the same layer, the curated one under its name.
        let (_, a) = embedded
            .encounter_traced(id, None, Tier::Curated)
            .unwrap()
            .unwrap();
        let (_, b) = disk
            .encounter_traced(id, None, Tier::Curated)
            .unwrap()
            .unwrap();
        let a: Vec<(&str, Source)> = a.iter().map(|(k, by)| (k, rename(&by.source))).collect();
        let b: Vec<(&str, Source)> = b.iter().map(|(k, by)| (k, by.source.clone())).collect();
        assert_eq!(a, b, "{id}");
    }
    // Every tuned file the crate embeds is the tree's, laid.
    let tuned = embedded_files()
        .iter()
        .filter(|(p, _)| {
            matches!(
                FileRole::of(p),
                Ok(Some(FileRole::Encounter { draft: false, .. }))
            )
        })
        .count();
    assert_eq!(curated, tuned);
    assert_eq!(disk.encounters().len(), embedded.encounters().len());
    // And every instance's trash, its trash file the tree's.
    for s in embedded.seasons() {
        for map in s.instances.values().filter_map(|i| i.map) {
            for tier in Tier::ALL {
                assert_eq!(
                    disk.trash_at(map, tier),
                    embedded.trash_at(map, tier),
                    "{map} {}",
                    tier.name()
                );
            }
        }
    }
}

/// The build's own files written out as a tree and read whole from it
/// (`from_dir`) resolve every encounter, trash, level and instance's
/// creatures as the embedded rubric does, the tuned and trash files named
/// for the tree. The version is not the build's: every file hashes alike,
/// then the tree's path is folded in, so what laid the rubric (its
/// provenance names the tree) is part of what it says.
#[test]
fn a_tree_read_whole_resolves_as_the_embedded_rubric() {
    let embedded = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let written: Vec<(String, String)> = embedded_files()
        .iter()
        .map(|&(p, t)| (p.to_string(), t.to_string()))
        .collect();
    let tree = tree_on_disk("whole", &written);
    let disk = Rubric::from_dir(&tree).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let name = tree.display().to_string();
    assert_eq!(*embedded.base_origin(), Origin::Embedded);
    assert_eq!(*disk.base_origin(), Origin::Dir(name.clone()));
    let rename = |s: &Source| match s {
        Source::Curated(Origin::Embedded) => dir_source(&name),
        s => s.clone(),
    };
    assert_eq!(disk.encounters().len(), embedded.encounters().len());
    for (id, _, _) in embedded.encounters() {
        for d in [
            None,
            Some(1),
            Some(2),
            Some(23),
            Some(8),
            Some(14),
            Some(15),
            Some(16),
            Some(17),
        ] {
            for tier in Tier::ALL {
                assert_eq!(
                    disk.encounter_at(id, d, tier),
                    embedded.encounter_at(id, d, tier),
                    "{id} {d:?} {}",
                    tier.name()
                );
            }
        }
        let want: Vec<Source> = embedded.layers_of(id).iter().map(rename).collect();
        assert_eq!(disk.layers_of(id), want, "{id}");
        let (_, a) = embedded
            .encounter_traced(id, None, Tier::Curated)
            .unwrap()
            .unwrap();
        let (_, b) = disk
            .encounter_traced(id, None, Tier::Curated)
            .unwrap()
            .unwrap();
        let a: Vec<(&str, Source)> = a.iter().map(|(k, by)| (k, rename(&by.source))).collect();
        let b: Vec<(&str, Source)> = b.iter().map(|(k, by)| (k, by.source.clone())).collect();
        assert_eq!(a, b, "{id}");
    }
    // Every level, from the tree's instance files.
    let levels: Vec<u32> = embedded.levels().iter().map(|&(u, _, _)| u).collect();
    assert!(!levels.is_empty());
    let read: Vec<u32> = disk.levels().iter().map(|&(u, _, _)| u).collect();
    assert_eq!(read, levels);
    for u in levels {
        assert_eq!(disk.level(u), embedded.level(u), "level {u}");
    }
    // Every season and instance, its creatures and forces, and its trash.
    assert_eq!(disk.seasons().len(), embedded.seasons().len());
    for (a, b) in embedded.seasons().iter().zip(disk.seasons()) {
        assert_eq!((&a.id, &a.name, a.order), (&b.id, &b.name, b.order));
        let slugs = |s: &Season| s.instances.keys().cloned().collect::<Vec<_>>();
        assert_eq!(slugs(a), slugs(b), "{}", a.id);
        for (slug, i) in &a.instances {
            let j = &b.instances[slug];
            assert_eq!(
                (&i.name, i.map, i.forces(), i.npcs()),
                (&j.name, j.map, j.forces(), j.npcs()),
                "{}/{slug}",
                a.id
            );
        }
        for (slug, i) in &a.instances {
            // Every difficulty's reading, not the keystone's alone.
            for d in std::iter::once(None).chain(difficulty::probes().map(Some)) {
                for tier in Tier::ALL {
                    assert_eq!(
                        disk.answer_trash(&b.id, slug, d, tier),
                        embedded.answer_trash(&a.id, slug, d, tier),
                        "{}/{slug} {d:?} {}",
                        a.id,
                        tier.name()
                    );
                }
            }
            let Some(map) = i.map else { continue };
            for tier in Tier::ALL {
                assert_eq!(
                    disk.trash_at(map, tier),
                    embedded.trash_at(map, tier),
                    "{map} {}",
                    tier.name()
                );
            }
            let want: Vec<Source> = embedded.trash_layers_of(map).iter().map(rename).collect();
            assert_eq!(disk.trash_layers_of(map), want, "{map}");
        }
    }
    // The version: the build's files, then the tree's path.
    assert_ne!(disk.version(), embedded.version());
    assert_eq!(
        disk.hash,
        hash_files(embedded.hash, [("\0dir\0", name.as_str())])
    );
    // Read again unchanged, the same; a file changed, another.
    let again = Rubric::from_dir(&tree).unwrap();
    assert_eq!(again.version(), disk.version());
    let (season, text) = written
        .iter()
        .find(|(p, _)| p.ends_with("/season.toml"))
        .unwrap();
    std::fs::write(tree.join(season), format!("{text}\n# a comment\n")).unwrap();
    let changed = Rubric::from_dir(&tree).unwrap();
    assert_ne!(changed.version(), disk.version());
    let _ = std::fs::remove_dir_all(&tree);
}

/// A tree read whole is refused, as the build's tree is, on any file that
/// cannot be read or placed, each named by its place on disk; and, as a
/// curated set is, where an encounter, an instance's trash or a level it
/// gives does not read: a tree on disk has no test to hold it so.
#[test]
fn a_tree_read_whole_is_refused_saying_why() {
    let missing = std::env::temp_dir().join("wowdps-encounter-rubric-tree-not-there");
    let errs = Rubric::from_dir(&missing).unwrap_err();
    assert!(
        errs.len() == 1 && errs[0].contains("not a directory"),
        "{errs:?}"
    );
    // No season: an instance and its encounter's files, no season.toml.
    let f = files(&[]);
    let seasonless = tree_on_disk("seasonless", &f[1..]);
    let errs = Rubric::from_dir(&seasonless).unwrap_err();
    assert_eq!(
        errs,
        [format!(
            "{}: no season: a seasons tree holds a <season>/season.toml",
            seasonless.display()
        )]
    );
    // A second season with no season.toml: its instance file is named.
    let half = tree_on_disk(
        "half",
        &files(&[(
            "t/other/instance.toml",
            &INSTANCE.replace("map = 3004", "map = 3005"),
        )]),
    );
    let errs = Rubric::from_dir(&half).unwrap_err();
    assert_eq!(
        errs,
        [format!(
            "{}: no t/season.toml",
            half.join("t/other/instance.toml").display()
        )]
    );
    // A file that does not parse, by its place on disk.
    let mut f = files(&[]);
    f[3].1 = "schema = 1\n[view\n".into();
    let unparsed = tree_on_disk("unparsed", &f);
    let errs = Rubric::from_dir(&unparsed).unwrap_err();
    let shown = unparsed
        .join("s/raid/3492-ulatek.toml")
        .display()
        .to_string();
    assert!(errs.len() == 1 && errs[0].starts_with(&shown), "{errs:?}");
    // A typo the file reads with but its encounter does not: the build's
    // reader takes it (its test holds the build's tree readable), the
    // tree's refuses it, naming the encounter and its files under the tree.
    let mut f = files(&[]);
    f[3].1 = "schema = 1\n[view]\nzom = 1\n".into();
    assert!(Rubric::from_files(f.clone()).is_ok());
    let typo = tree_on_disk("typo", &f);
    let errs = Rubric::from_dir(&typo).unwrap_err();
    assert!(
        errs.len() == 1
            && errs[0].starts_with(&format!(
                "{}: encounter 3492 (on every difficulty): ",
                typo.display()
            ))
            && errs[0].contains("s/raid/3492-ulatek")
            && errs[0].contains("zom"),
        "{errs:?}"
    );
    // A level that does not read.
    let f = leveled("[level.2097.map]\nground = \"terrain\"\nzom = 1\n");
    assert!(Rubric::from_files(f.clone()).is_ok());
    let level = tree_on_disk("level", &f);
    let errs = Rubric::from_dir(&level).unwrap_err();
    assert!(
        errs.len() == 1
            && errs[0].starts_with(&format!("{}: s/raid/level.2097: ", level.display()))
            && errs[0].contains("zom"),
        "{errs:?}"
    );
    for t in [seasonless, half, unparsed, typo, level] {
        let _ = std::fs::remove_dir_all(&t);
    }
}

/// A tree's stray entries are passed over, never a refusal: an editor's
/// lock file (a dot-file linking to nothing), a dot-file or directory that
/// would not parse, a link to nothing, a name that is not UTF-8 on a file
/// that is no `.toml`. A `.toml` whose name is not UTF-8 is still named.
#[cfg(unix)]
#[test]
fn a_trees_strays_are_passed_over_and_a_bad_toml_name_is_named() {
    use std::os::unix::ffi::OsStrExt;
    let f = files(&[]);
    let tree = tree_on_disk("strays", &f);
    std::os::unix::fs::symlink(
        "k@host.1234:1700000000",
        tree.join("s/raid/.#instance.toml"),
    )
    .unwrap();
    std::fs::write(tree.join("s/raid/.3492-ulatek.toml.swp"), "\0\0").unwrap();
    std::fs::write(tree.join("s/raid/.hidden.toml"), "not [ toml").unwrap();
    std::fs::create_dir_all(tree.join("s/.cache")).unwrap();
    std::fs::write(tree.join("s/.cache/x.toml"), "not [ toml").unwrap();
    std::os::unix::fs::symlink(tree.join("nowhere.toml"), tree.join("s/raid/gone.toml")).unwrap();
    let odd = std::ffi::OsStr::from_bytes(b"notes-\xff.txt");
    std::fs::write(tree.join("s/raid").join(odd), "notes").unwrap();
    let r = Rubric::from_dir(&tree).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let built = Rubric::from_files(f).unwrap();
    assert_eq!(r.encounter(3492, Some(16)), built.encounter(3492, Some(16)));
    // The walk's fingerprint is the clean tree's: the strays are not read.
    let clean = tree_on_disk("strays-clean", &files(&[]));
    let (a, b) = (TreeFiles::read(&tree), TreeFiles::read(&clean));
    assert_eq!(a.files, b.files);
    assert!(a.errors.is_empty(), "{:?}", a.errors);
    // A rubric file whose name is not UTF-8 is named, by its place.
    let bad = std::ffi::OsStr::from_bytes(b"3493-bad\xff.toml");
    std::fs::write(tree.join("s/raid").join(bad), "schema = 1\n").unwrap();
    let errs = Rubric::from_dir(&tree).unwrap_err();
    assert!(
        errs.len() == 1 && errs[0].ends_with("not a UTF-8 name") && errs[0].contains("3493-bad"),
        "{errs:?}"
    );
    let _ = std::fs::remove_dir_all(&tree);
    let _ = std::fs::remove_dir_all(&clean);
}

/// A tree's fingerprint follows its files and parses none: the same walk
/// twice gives the same one, an edit (a file that will not parse among
/// them) another, and a tree that is not there one of its own.
#[test]
fn a_trees_fingerprint_follows_its_files_without_parsing() {
    let tree = tree_on_disk("print", &files(&[]));
    let first = TreeFiles::read(&tree).fingerprint();
    assert_eq!(TreeFiles::read(&tree).fingerprint(), first);
    std::fs::write(tree.join("s/raid/3492-ulatek.toml"), "schema = 1\n[view\n").unwrap();
    let broken = TreeFiles::read(&tree);
    assert_ne!(broken.fingerprint(), first);
    assert_eq!(TreeFiles::read(&tree).fingerprint(), broken.fingerprint());
    // The walk read the file it could not parse; the rubric refuses it.
    assert!(
        broken
            .files
            .iter()
            .any(|(p, _)| p == "s/raid/3492-ulatek.toml")
    );
    assert!(Rubric::from_tree(broken).is_err());
    let gone = tree.join("gone");
    let none = TreeFiles::read(&gone);
    assert!(!none.present && none.files.is_empty() && none.errors.is_empty());
    assert_eq!(
        Rubric::from_tree(none).unwrap_err(),
        [format!("{}: not a directory", gone.display())]
    );
    let _ = std::fs::remove_dir_all(&tree);
}

/// A trash file that reads as a file but whose trash does not resolve
/// refuses the tree, naming the instance's trash; the build's reader takes
/// it (its test holds the build's trash readable).
#[test]
fn a_tree_whose_trash_does_not_read_is_refused() {
    let f = trashed("schema = 2\n[ability.slam]\nname = \"Slam\"\nzom = 1\n");
    let built = Rubric::from_files(f.clone()).unwrap();
    assert!(matches!(built.trash(3004), Some(Err(_))));
    let tree = tree_on_disk("trash-unread", &f);
    let errs = Rubric::from_dir(&tree).unwrap_err();
    assert!(
        errs.len() == 1
            && errs[0].starts_with(&format!(
                "{}: s/raid's trash (on every difficulty): ",
                tree.display()
            ))
            && errs[0].contains("zom"),
        "{errs:?}"
    );
    let _ = std::fs::remove_dir_all(&tree);
}

/// A trash file where no reader looks, in a tree, is named by its place on
/// disk.
#[test]
fn a_trees_trash_file_where_no_reader_looks_is_named_by_its_place() {
    let older = "schema = 1\nname = \"Older\"\norder = 1\n";
    let mut f = trashed("");
    f.extend(owned(&[
        ("old/season.toml", older),
        ("old/raid/instance.toml", INSTANCE),
        ("old/raid/3492-ulatek.draft.toml", DRAFT),
        ("old/raid/trash.toml", "schema = 1\n"),
    ]));
    let tree = tree_on_disk("trash-elsewhere", &f);
    let errs = Rubric::from_dir(&tree).unwrap_err();
    assert_eq!(
        errs,
        [format!(
            "{}: map 3004's trash is answered from s/raid; a layer's trash file goes there",
            tree.join("old/raid/trash.toml").display()
        )]
    );
    let _ = std::fs::remove_dir_all(&tree);
}

/// The user's files lay over a tree read whole as over the build's, above
/// its curated set (`Curated(Origin::Dir)`); a tree laid over that later
/// (`with_curated_dir`) reads the user's files again over it, and one it
/// leaves unreadable is taken out, named.
#[test]
fn the_users_files_lay_over_a_tree_and_are_read_again_over_its_replacement() {
    let extra = format!("{TUNED}\n[npc.extra]\nname = \"Extra\"\n");
    let tree = tree_on_disk(
        "user-over",
        &files(&[])
            .into_iter()
            .map(|(p, t)| {
                if p == "s/raid/3492-ulatek.toml" {
                    (p, extra.clone())
                } else {
                    (p, t)
                }
            })
            .collect::<Vec<_>>(),
    );
    let user = owned(&[(
        "s/raid/3492-ulatek.toml",
        "schema = 1\n[view]\nturn = 30\n[npc.extra]\nrole = \"pet\"\n",
    )]);
    let r = Rubric::from_dir(&tree).unwrap().with_user(user);
    assert!(r.user_errors().is_empty(), "{:?}", r.user_errors());
    let name = tree.display().to_string();
    let layers = r.layers_of(3492);
    assert_eq!(layers.last(), Some(&Source::User));
    assert!(layers.contains(&dir_source(&name)), "{layers:?}");
    let at = |r: &Rubric, tier| r.encounter_at(3492, None, tier).unwrap().unwrap();
    assert_eq!(at(&r, Tier::Curated).view.turn, Some(90.0));
    assert_eq!(at(&r, Tier::User).view.turn, Some(30.0));
    assert_eq!(at(&r, Tier::User).npc["extra"].role, Role::Pet);
    // The tree's set replaced by one without the extra NPC: the user's
    // file, whose `extra` has no name of its own, no longer reads.
    let plain = tree_on_disk("user-over-plain", &files(&[]));
    let again = r.with_curated_dir("plain", &plain).unwrap();
    assert!(!again.layers_of(3492).contains(&dir_source(&name)));
    assert!(
        again
            .user_errors()
            .iter()
            .any(|e| e.starts_with("s/raid/3492-ulatek.toml: left out over curated \"plain\"")),
        "{:?}",
        again.user_errors()
    );
    assert_eq!(at(&again, Tier::User).view.turn, Some(90.0));
    let _ = std::fs::remove_dir_all(&tree);
    let _ = std::fs::remove_dir_all(&plain);
}

#[test]
fn a_layer_file_where_no_reader_looks_is_refused() {
    // A returning dungeon: an older season holds the raid too, and the
    // newer season answers Ula'tek. A layer's file under the older copy
    // would be laid and never read.
    let older = "schema = 1\nname = \"Older\"\norder = 1\n";
    let mut f = base_files();
    f.extend(owned(&[
        ("old/season.toml", older),
        ("old/raid/instance.toml", INSTANCE),
        ("old/raid/3492-ulatek.draft.toml", DRAFT),
    ]));
    let base = || Rubric::from_files(f.clone()).unwrap();
    let r = base().with_user(owned(&[
        (
            "old/raid/3492-ulatek.toml",
            "schema = 1\n[view]\nturn = 7\n",
        ),
        (
            "old/raid/instance.toml",
            "schema = 1\n[encounter.3492.view]\nturn = 8\n",
        ),
        // An id the newer season has no files for is the older season's.
        (
            "old/raid/3501-old-only.toml",
            "schema = 1\nencounter = 3501\nname = \"Old\"\n",
        ),
    ]));
    let errs = r.user_errors();
    assert_eq!(errs.len(), 2, "{errs:?}");
    for file in ["old/raid/3492-ulatek.toml", "old/raid/instance.toml"] {
        assert!(
            errs.iter()
                .any(|e| e.starts_with(file) && e.contains("answered from s/raid")),
            "{file}: {errs:?}"
        );
    }
    assert_eq!(r.encounter(3492, None).unwrap().unwrap().view.turn, None);
    assert!(!r.has_user(3492));
    assert_eq!(r.encounter(3501, None).unwrap().unwrap().name, "Old");
    let errs = base()
        .with_curated(
            "old",
            owned(&[(
                "old/raid/3492-ulatek.toml",
                "schema = 1\n[view]\nturn = 7\n",
            )]),
        )
        .unwrap_err();
    assert!(errs[0].contains("answered from s/raid"), "{errs:?}");
}

// ---- the map's vocabulary: levels, areas, paths ------------------------------

/// Every encounter's map and view, on every difficulty at every tier, as
/// they resolved before the map vocabulary (crop, include, fade, ground,
/// exposure, area, path, transition, water, a level) was added: the fixture is
/// their Debug then, one line per encounter (`*`: every reading alike).
/// The map is written here in its shape then, and its new keys are held
/// apart: default, but for the areas, paths and crops an instance file
/// has since given. An instance file's later edit to a key the map had
/// then (a stencil the every-room pass gave Melidrussa Chillworn) is
/// written into the fixture with it, so it shows in review.
#[test]
fn every_map_and_view_resolves_as_before_the_map_vocabulary() {
    use std::fmt;
    /// A map as its Debug read before: its keys then, in their order.
    struct Before<'a>(&'a Map);
    impl fmt::Debug for Before<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let m = self.0;
            f.debug_struct("Map")
                .field("ui_map", &m.ui_map)
                .field("floor", &m.floor)
                .field("level", &m.level)
                .field("ceiling", &m.ceiling)
                .field("light", &m.light)
                .field("exclude_groups", &m.exclude_groups)
                .field("exclude_models", &m.exclude_models)
                .field("exclude_textures", &m.exclude_textures)
                .field("layer", &m.layer)
                .field("stencil", &m.stencil)
                .field("ppy", &m.ppy)
                .field("glow", &m.glow)
                .finish()
        }
    }
    // The encounters whose maps gained areas since, by their keys; those
    // that gained paths between them; those framed by a crop since; those
    // whose render took an exposure since; and those that say their water.
    let gained: BTreeMap<u32, Vec<&str>> = [
        (3455, vec!["hall"]),
        (
            3492,
            vec!["court", "east-terrace", "platform", "west-terrace"],
        ),
    ]
    .into();
    let pathed: BTreeMap<u32, Vec<&str>> =
        [(3492, vec!["platform-to-east", "platform-to-west"])].into();
    let cropped = [3178, 3182, 3306];
    let exposed = [3455, 3470];
    let watered = [3183, 3379];
    let r = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let mut out = String::new();
    for (id, _, _) in r.encounters() {
        let mut seen: Vec<(String, Vec<String>)> = Vec::new();
        for tier in Tier::ALL {
            for d in std::iter::once(None).chain(difficulty::probes().map(Some)) {
                let e = r.encounter_at(id, d, tier).unwrap().unwrap();
                let areas: Vec<&str> = e.map.area.keys().map(String::as_str).collect();
                assert_eq!(areas, gained.get(&id).cloned().unwrap_or_default(), "{id}");
                let paths: Vec<&str> = e.map.path.keys().map(String::as_str).collect();
                assert_eq!(paths, pathed.get(&id).cloned().unwrap_or_default(), "{id}");
                assert_eq!(e.map.crop.is_some(), cropped.contains(&id), "{id}");
                assert_eq!(e.map.exposure.is_some(), exposed.contains(&id), "{id}");
                assert_eq!(e.map.water.is_some(), watered.contains(&id), "{id}");
                let m = &e.map;
                let then = Map {
                    ui_map: m.ui_map,
                    floor: m.floor.clone(),
                    level: m.level,
                    ceiling: m.ceiling,
                    light: m.light,
                    exclude_groups: m.exclude_groups.clone(),
                    exclude_models: m.exclude_models.clone(),
                    exclude_textures: m.exclude_textures.clone(),
                    layer: m.layer.clone(),
                    stencil: m.stencil.clone(),
                    ppy: m.ppy,
                    glow: m.glow.clone(),
                    ..Map::default()
                };
                let no_areas = Map {
                    area: BTreeMap::new(),
                    path: BTreeMap::new(),
                    crop: None,
                    exposure: None,
                    water: None,
                    ..m.clone()
                };
                assert_eq!(
                    no_areas, then,
                    "{id}: a new key besides its areas, paths, crop, exposure and water"
                );
                let dbg = format!("{:?} {:?}", Before(m), e.view);
                let label = format!(
                    "{}/{}",
                    tier.name(),
                    d.map_or("-".to_string(), |d| d.to_string())
                );
                match seen.iter_mut().find(|(k, _)| *k == dbg) {
                    Some((_, v)) => v.push(label),
                    None => seen.push((dbg, vec![label])),
                }
            }
        }
        let every = seen.len() == 1;
        for (dbg, labels) in seen {
            let labels = if every {
                "*".to_string()
            } else {
                labels.join(",")
            };
            out.push_str(&format!("{id} {labels} {dbg}\n"));
        }
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/resolved-maps.txt");
    if std::env::var_os("WOWDPS_BLESS").is_some() {
        std::fs::write(path, &out).unwrap();
        return;
    }
    let want = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{path}: {e}; run with WOWDPS_BLESS=1"));
    for (n, (w, g)) in want.lines().zip(out.lines()).enumerate() {
        assert_eq!(
            w,
            g,
            "fixtures/resolved-maps.txt line {}; if the change is meant, run with WOWDPS_BLESS=1",
            n + 1
        );
    }
    assert_eq!(want.lines().count(), out.lines().count());
}

/// The test raid's season with travel speeds, and an instance giving two
/// levels: the first with every kind of the map's new entry.
fn leveled(levels: &str) -> Vec<(String, String)> {
    let mut f = files(&[]);
    f[0].1 = format!("{SEASON}\n[defaults.travel]\nrun = 7.0\nmount = 14.0\n");
    f[1].1 = format!("{INSTANCE}\n{levels}");
    f
}

const LEVELS: &str = r#"
[defaults.view]
zoom_max = 6

[level.2097.map]
floor = "academy-L1"
crop = [1400, -3250, 2000, -2750]
ground = "terrain"
include_groups = [5]
transition = { after = 3, unit = "travel", port = "flash", port_yd = 40, camera = "glide" }

[level.2097.map.fade.roof]
group = 6
alpha = 0.3

[level.2097.map.area.entrance]
shape = [{ center = [1410, -2772], radius = 15 }]

[level.2097.map.area.court]
name = "A boss's court"
shape = [{ points = [[1740, -3110], [1785, -3110], [1785, -3060], [1740, -3060]] }]
far = 1
home = 1.5
near = 6
turn = 90
encounter = 3492

[level.2097.map.area.gone]
enabled = false
shape = [{ center = [0, 0], radius = 1 }]

[level.2097.map.path.in]
from = "entrance"
to = "court"
points = [[1410, -2772], [1600, -2900], [1760, -3085]]

[level.2097.map.path.flight]
from = "court"
to = "entrance"
mode = "vehicle"

[level.2097.view]
zoom = 1.3

[level.2098.map]
ground = 512.5

[level.2098.travel]
run = 8
"#;

#[test]
fn a_level_reads_the_defaults_then_its_own_table() {
    let r = Rubric::from_files(leveled(LEVELS)).unwrap_or_else(|e| panic!("{e:?}"));
    let listed: Vec<(u32, &str, &str)> = r
        .levels()
        .into_iter()
        .map(|(id, s, i)| (id, s.id.as_str(), i.slug.as_str()))
        .collect();
    assert_eq!(listed, [(2097, "s", "raid"), (2098, "s", "raid")]);
    let l = r.level(2097).unwrap().unwrap();
    assert!(check_level(&l).is_empty(), "{:?}", check_level(&l));
    assert_eq!((l.ui_map, l.instance.as_str()), (2097, "raid"));
    // Its map: its own UiMap, the season's ceiling, its own keys; the
    // area switched off is gone.
    assert_eq!(l.map.ui_map, Some(2097));
    assert_eq!(l.map.ceiling, Some(25.0));
    assert_eq!(l.map.floor.as_deref(), Some("academy-L1"));
    assert_eq!(l.map.ground, Some(Datum::Terrain));
    assert_eq!(l.map.crop, Some([1400.0, -3250.0, 2000.0, -2750.0]));
    assert_eq!(l.map.fade["roof"].group, Some(6));
    let keys: Vec<&str> = l.map.area.keys().map(String::as_str).collect();
    assert_eq!(keys, ["court", "entrance"]);
    let court = &l.map.area["court"];
    assert_eq!(
        (court.far, court.home, court.near, court.encounter),
        (Some(1.0), Some(1.5), Some(6.0), Some(3492))
    );
    assert_eq!(court.bounds(), Some([1740.0, -3110.0, 1785.0, -3060.0]));
    assert_eq!(
        l.map.area_at([1760.0, -3085.0]).map(|(k, _)| k),
        Some("court")
    );
    assert_eq!(
        l.map.area_at([1410.0, -2770.0]).map(|(k, _)| k),
        Some("entrance")
    );
    assert!(l.map.area_at([0.0, 0.0]).is_none());
    // Its view: the season's zoom under its own, the instance's zoom_max.
    assert_eq!((l.view.zoom, l.view.zoom_max), (Some(1.3), Some(6.0)));
    // Its paths' speeds: the season's by mode, a vehicle timed to the
    // next post; and the transition as written.
    let (walk, flight) = (&l.map.path["in"], &l.map.path["flight"]);
    assert_eq!((walk.mode, flight.mode), (Mode::Run, Mode::Vehicle));
    assert_eq!(walk.speed_in(&l.travel), Some(7.0));
    assert_eq!(flight.speed_in(&l.travel), None);
    let t = l.map.transition.as_ref().unwrap();
    assert_eq!(
        (t.after, t.unit, t.port, t.port_yd, t.camera),
        (
            Some(3.0),
            Some(UnitQuiet::Travel),
            Some(Port::Flash),
            Some(40.0),
            Some(CameraQuiet::Glide)
        )
    );
    // The other level: a height for its ground, its own run.
    let l = r.level(2098).unwrap().unwrap();
    assert_eq!(l.map.ground, Some(Datum::Height(512.5)));
    assert_eq!(l.travel.speed(Mode::Run), Some(8.0));
    assert_eq!(l.travel.speed(Mode::Mount), Some(14.0));
    assert_eq!(l.travel.speed(Mode::Leap), None);
    assert!(r.level(1).is_none());
    // An encounter reads the season's travel too, and an area may share a
    // key with a level's: they are different tables.
    let mut f = leveled(LEVELS);
    f[1].1
        .push_str("\n[encounter.3492.map.area.court]\nshape = [{ center = [0, 0], radius = 5 }]\n");
    let r = Rubric::from_files(f).unwrap();
    let e = r.encounter(3492, None).unwrap().unwrap();
    assert_eq!(e.travel.mount, Some(14.0));
    assert!(e.map.area.contains_key("court") && check(&e).is_empty());
    assert_eq!(r.level(2097).unwrap().unwrap().map.area.len(), 2);
}

#[test]
fn a_level_is_named_by_its_ui_map_and_gives_only_its_map_view_and_travel() {
    let refused = |levels: &str| Rubric::from_files(leveled(levels)).unwrap_err();
    let errs = refused("[level.academy.map]\nfloor = \"x\"\n");
    assert!(
        errs.iter().any(|e| e.contains("named by its UiMap id")),
        "{errs:?}"
    );
    let errs = refused("[level.2097.npc.x]\nname = \"X\"\n");
    assert!(
        errs.iter()
            .any(|e| e.contains("[level.2097]") && e.contains("`map`, `view`, `travel`, `pull`")),
        "{errs:?}"
    );
    let errs = refused("level = 3\n");
    assert!(
        errs.iter().any(|e| e.contains("`level` is a table")),
        "{errs:?}"
    );
    // A typo is the level's error, naming the key.
    let r = Rubric::from_files(leveled("[level.2097.map.area.a]\nshape = []\nhoem = 2\n")).unwrap();
    let e = r.level(2097).unwrap().unwrap_err();
    assert!(e.contains("hoem") && e.contains("s/raid/level.2097"), "{e}");
    // A ground is a word or a height.
    let r = Rubric::from_files(leveled("[level.2097.map]\nground = \"grass\"\n")).unwrap();
    let e = r.level(2097).unwrap().unwrap_err();
    assert!(e.contains("grass"), "{e}");
    // A season gives a level once; a newer season's is the one read.
    let mut twice = leveled("[level.2097.map]\nfloor = \"a\"\n");
    twice.push((
        "s/other/instance.toml".into(),
        format!("{INSTANCE}\n[level.2097.map]\nfloor = \"b\"\n"),
    ));
    let errs = Rubric::from_files(twice).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("[level.2097]") && e.contains("a season gives a level once")),
        "{errs:?}"
    );
    let mut returning = leveled("[level.2097.map]\nfloor = \"new\"\n");
    returning.push((
        "old/season.toml".into(),
        "schema = 1\nname = \"Old\"\norder = 1\n".into(),
    ));
    returning.push((
        "old/raid/instance.toml".into(),
        format!(
            "{INSTANCE}\n[level.2097.map]\nfloor = \"old\"\n[level.2099.map]\nfloor = \"older\"\n"
        ),
    ));
    let r = Rubric::from_files(returning).unwrap();
    let floor = |id| r.level(id).unwrap().unwrap().map.floor;
    assert_eq!(
        (floor(2097), floor(2099)),
        (Some("new".into()), Some("older".into()))
    );
    let listed: Vec<(u32, &str)> = r
        .levels()
        .into_iter()
        .map(|(id, s, _)| (id, s.id.as_str()))
        .collect();
    assert_eq!(listed, [(2097, "s"), (2099, "old")]);
}

#[test]
fn a_level_is_the_bases_alone() {
    let r = Rubric::from_files(leveled(LEVELS)).unwrap();
    // A layer's instance file giving a level is refused, the user's named
    // and left out, a curated set's refused whole.
    let file = || {
        owned(&[(
            "s/raid/instance.toml",
            "schema = 1\n[level.2097.view]\nturn = 9\n",
        )])
    };
    let user = r.clone().with_user(file());
    let errs = user.user_errors();
    assert!(
        errs.len() == 1 && errs[0].contains("a level is the base's alone"),
        "{errs:?}"
    );
    let errs = r.clone().with_curated("pack", file()).unwrap_err();
    assert!(errs[0].contains("a level is the base's alone"), "{errs:?}");
    // A set may still amend an arena, as the layer stack lets it amend
    // anything: a user's season defaults and their drawing of an
    // encounter reach its arena, never a level, even one with an area of
    // the same key.
    let user = r.with_user(owned(&[
        ("s/season.toml", "schema = 1\n[defaults.view]\nturn = 9\n"),
        (
            "s/raid/instance.toml",
            "schema = 1\n[encounter.3492.map.area.court]\nshape = [{ center = [1, 1], radius = 9 }]\n",
        ),
    ]));
    assert!(user.user_errors().is_empty(), "{:?}", user.user_errors());
    let arena = user.encounter(3492, None).unwrap().unwrap();
    assert_eq!(arena.view.turn, Some(9.0));
    assert_eq!(arena.map.area["court"].shape[0].radius, Some(9.0));
    let level = user.level(2097).unwrap().unwrap();
    assert_eq!(level.view.turn, None);
    assert_eq!(level.map.area["court"].shape[0].radius, None);
}

/// A map's water takes every key but `bed`, and no other; a color is
/// three bytes. A user file amends one key of the base's table, as tables
/// merge.
#[test]
fn a_maps_water_takes_its_keys_and_a_layer_amends_one() {
    let full = "close = [18, 22, 29]\nfar = [14, 20, 38]\nshallow = 0.7\ndeep = 0.85\ndepth = 18";
    let water = |text: &str| toml::from_str::<Water>(text);
    let w = water(full).unwrap();
    assert_eq!((w.close, w.far, w.bed), ([18, 22, 29], [14, 20, 38], None));
    for bad in [
        full.replace("depth = 18", ""),
        full.replace("close = [18, 22, 29]", "close = [18, 22]"),
        full.replace("close = [18, 22, 29]", "close = [18, 22, 290]"),
        format!("{full}\nfog = [1, 2, 3]"),
        format!("{full}\ntypes = [-1]"),
    ] {
        assert!(water(&bad).is_err(), "{bad}");
    }
    // Unsaid, it draws the default light's waters; said, the types named,
    // a magic water's own among them.
    assert!(w.draws(5, false) && !w.draws(924, true));
    let named = water(&format!("{full}\ntypes = [924]")).unwrap();
    assert!(named.draws(924, true) && !named.draws(5, false));
    let base = format!("[level.2097.map.water]\n{full}\n[encounter.3492.map.water]\n{full}\n");
    let r = Rubric::from_files(leveled(&base)).unwrap_or_else(|e| panic!("{e:?}"));
    let level = r.level(2097).unwrap().unwrap();
    assert_eq!(level.map.water.as_ref(), Some(&w));
    assert!(check_level(&level).is_empty(), "{:?}", check_level(&level));
    let user = r.with_user(owned(&[(
        "s/raid/3492-ulatek.toml",
        "schema = 1\n[map.water]\nclose = [1, 2, 3]\n",
    )]));
    assert!(user.user_errors().is_empty(), "{:?}", user.user_errors());
    let mine = user.encounter(3492, None).unwrap().unwrap().map.water;
    assert_eq!(
        mine,
        Some(Water {
            close: [1, 2, 3],
            ..w
        })
    );
}

#[test]
fn a_bad_area_path_fade_or_crop_is_found() {
    let bad = r#"
[level.2097.map]
ui_map = 2098
crop = [5, 2, 1, 6]
include_groups = [4]
exclude_groups = [4, 1]
exposure = 0
hang = 0
transition = { after = -1, port_yd = 0 }
water = { close = [1, 2, 3], far = [4, 5, 6], shallow = 1.5, deep = -0.1, depth = 0, types = [], shore = -1 }

[level.2097.map.fade.both]
group = 1
model = 2
alpha = 0.5

[level.2097.map.fade.opaque]
model = 2
alpha = 1

[level.2097.map.area.empty]
shape = []

[level.2097.map.area.upside-down]
shape = [{ center = [0, 0], radius = 5 }]
far = 2
home = 1.5
near = 0
hang = -1
stencil = [{ center = [0, 0], radius = 5 }, { points = [[0, 0], [1, 1]] }]

[level.2097.map.area.too-high]
shape = [{ center = [0, 0], radius = 5 }]
home = 10
encounter = 7

[level.2097.map.area.too-near]
shape = [{ center = [0, 0], radius = 5 }]
near = 0.8
encounter = 7

[level.2097.map.path.astray]
from = "nowhere"
to = "nowhere"
speed = 0
spell = 5
points = [[0, nan]]

[level.2097.map.glow.lit]
color = [1, 2, 3]
area = [{ center = [0, 0], radius = 1 }]
on = { event = "erupts" }

[level.2097.travel]
mount = -14
"#;
    let r = Rubric::from_files(leveled(bad)).unwrap_or_else(|e| panic!("{e:?}"));
    let problems = check_level(&r.level(2097).unwrap().unwrap());
    for want in [
        "map.ui_map names UiMap 2098, not the level's own 2097",
        "travel has a speed that is not above 0",
        "map.glow.lit names event \"erupts\", which the level does not have",
        "map.crop is no box: [x0, y0, x1, y1] needs x0 < x1 and y0 < y1",
        "map: group 4 is both included and excluded",
        "map.exposure is not above 0",
        "map.water's `shallow` is not from 0 to 1",
        "map.water's `deep` is not from 0 to 1",
        "map.water's `depth` is not above 0",
        "map.water's `types` names none",
        "map.water's `shore` is not 0 or more yards",
        "map.fade.both names both of `group` and `model`",
        "map.fade.both fades group 1, which the map leaves out",
        "map.fade.opaque has an alpha of 1, not above 0 and under 1",
        "map.fade.opaque fades model 2, which map.fade.both fades too",
        "map.area.empty has no shape",
        "map.area.upside-down has an altitude that is not above 0",
        "map.area.upside-down has a hang that is not above 0",
        "map.area.upside-down.stencil[1] is neither a polygon",
        "map.hang is not above 0",
        // Unsaid altitudes are the view's, and checked so: the season's
        // zoom 1.15, the closest 8, the furthest 1.
        "map.area.too-high's altitudes are out of order: far 1 ≤ home 10 ≤ near 8",
        "map.area.too-near's altitudes are out of order: far 1 ≤ home 1.15 ≤ near 0.8",
        "map.area.too-near is encounter 7's court, as map.area.too-high is",
        "map.path.astray names area \"nowhere\", which the map does not have",
        "map.path.astray runs from area \"nowhere\" to itself",
        "map.path.astray has a width or a speed not above 0",
        "map.path.astray has a point that is not a number",
        "map.path.astray names a spell, but only a leap's device casts one",
        "map.transition's `after` is not 0 or more seconds",
        "map.transition's `port_yd` is not above 0",
    ] {
        assert!(
            problems.iter().any(|p| p.starts_with(want)),
            "{want}: {problems:#?}"
        );
    }
    // Each path end is found missing once, and nothing else is said: the
    // area's good stencil shape among it.
    assert_eq!(problems.len(), 31, "{problems:#?}");
    // A reader's altitudes: the area's own, else the view's, else 1, 1.15
    // and 8.
    let l = r.level(2097).unwrap().unwrap();
    let view = View {
        zoom: Some(2.0),
        zoom_max: Some(6.0),
        ..View::default()
    };
    assert_eq!(l.map.area["too-high"].altitudes(&view), [1.0, 10.0, 6.0]);
    assert_eq!(l.map.area["empty"].altitudes(&view), [1.0, 2.0, 6.0]);
    assert_eq!(
        l.map.area["empty"].altitudes(&View::default()),
        [Area::FAR, Area::HOME, Area::NEAR]
    );
    // A fade with neither, and a ground that is no number.
    let r = Rubric::from_files(leveled(
        "[level.2097.map]\nground = nan\n[level.2097.map.fade.none]\nalpha = 0.5\n",
    ))
    .unwrap();
    let problems = check_level(&r.level(2097).unwrap().unwrap());
    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(
        problems.iter().any(|p| p.contains("neither")),
        "{problems:?}"
    );
}

/// A level's pulls over the test instance's creatures (`CREATURES`): two
/// in its hall out of the route's order, one switched off, and the boss's
/// court beside them.
const PULLS: &str = r#"
[level.2097.map.area.hall]
shape = [{ center = [0, 0], radius = 30 }]

[level.2097.map.area.court]
shape = [{ center = [100, 0], radius = 20 }]
encounter = 3492

[level.2097.pull.bridge]
name = "The bridge pack"
order = 2
area = "hall"
npc = ["juggernaut", "channeler", "channeler", "thundercloud"]
spots = [[1, 2], [3, 4], [5, 6], [7, 8]]

[level.2097.pull.entrance]
order = 1
area = "hall"
npc = ["juggernaut"]

[level.2097.pull.gone]
enabled = false
order = 1
area = "nowhere"
npc = []

[level.2098.map]
floor = "academy-L2"
"#;

#[test]
fn a_level_gives_its_pulls_in_the_routes_order() {
    let r = Rubric::from_files(leveled(&format!("{CREATURES}{PULLS}")))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let l = r.level(2097).unwrap().unwrap();
    assert!(check_level(&l).is_empty(), "{:?}", check_level(&l));
    // The one switched off is gone; the rest come in the route's order,
    // not the keys'.
    let keys: Vec<&str> = l.pull.keys().map(String::as_str).collect();
    assert_eq!(keys, ["bridge", "entrance"]);
    let route: Vec<&str> = l.pulls_in_order().into_iter().map(|(k, _)| k).collect();
    assert_eq!(route, ["entrance", "bridge"]);
    let bridge = &l.pull["bridge"];
    assert_eq!(
        (bridge.name.as_deref(), bridge.order, bridge.area.as_str()),
        (Some("The bridge pack"), 2, "hall")
    );
    assert_eq!(bridge.spots[3], [7.0, 8.0]);
    // The instance's creatures are laid in, its switched-off one left out,
    // and a pull's forces are its members': a pet gives none.
    let creatures: Vec<&str> = l.npc.keys().map(String::as_str).collect();
    assert_eq!(creatures, ["channeler", "juggernaut", "thundercloud"]);
    assert!(!l.npc.contains_key("gone"));
    assert_eq!(bridge.forces(&l), 75);
    assert_eq!(l.pull["entrance"].forces(&l), 25);
    // A member the level does not know adds nothing.
    let stray = Pull {
        npc: vec!["juggernaut".into(), "ghost".into()],
        ..bridge.clone()
    };
    assert_eq!(stray.forces(&l), 25);
    // Ties in the route go by key.
    let mut tied = l.clone();
    if let Some(p) = tied.pull.get_mut("bridge") {
        p.order = 1;
    }
    let route: Vec<&str> = tied.pulls_in_order().into_iter().map(|(k, _)| k).collect();
    assert_eq!(route, ["bridge", "entrance"]);
    // The creatures are the instance's, never written as the level's; a
    // level with no pulls writes and dumps as it did before it could have
    // them.
    let written = toml::to_string(&l).unwrap();
    assert!(
        written.contains("[pull.bridge]") && !written.contains("[npc"),
        "{written}"
    );
    let bare = r.level(2098).unwrap().unwrap();
    assert!(!bare.npc.is_empty());
    let written = toml::to_string(&bare).unwrap();
    assert!(
        !written.contains("pull") && !written.contains("npc"),
        "{written}"
    );
    let dumped = format!("{bare:#?}");
    assert!(
        !dumped.contains("pull") && !dumped.contains("npc") && dumped.contains("travel: Travel"),
        "{dumped}"
    );
    assert!(format!("{l:#?}").contains("pull: {"));
    // A level read from text alone, as an editor reads one, has no
    // creatures laid in: it equals the one answered all the same, and is
    // checked with the instance's creatures given.
    let mut read: Level = toml::from_str(&toml::to_string(&l).unwrap()).unwrap();
    (read.ui_map, read.instance) = (l.ui_map, l.instance.clone());
    assert!(read.npc.is_empty());
    assert_eq!(read, l);
    assert_eq!(read.pull["bridge"].forces(&read), 0);
    assert_eq!(
        check_level(&read),
        [
            "pull.bridge names npc \"channeler\", which the instance does not have",
            "pull.bridge names npc \"juggernaut\", which the instance does not have",
            "pull.bridge names npc \"thundercloud\", which the instance does not have",
            "pull.entrance names npc \"juggernaut\", which the instance does not have",
        ]
    );
    let (_, inst) = r.instance_by_map(3004).unwrap();
    assert!(check_level_with(&read, inst.npcs()).is_empty());
    // A typo in a pull is the level's error, naming the key.
    let r = Rubric::from_files(leveled(&format!(
        "{CREATURES}{PULLS}\n[level.2097.pull.typo]\norder = 3\narea = \"hall\"\nnpc = []\nodrer = 4\n"
    )))
    .unwrap();
    let e = r.level(2097).unwrap().unwrap_err();
    assert!(
        e.contains("odrer") && e.contains("s/raid/level.2097"),
        "{e}"
    );
}

/// A pull the usual route may skip at some key levels, last in it beside
/// `PULLS`'s two: a juggernaut and a channeler, 50 forces.
const OPTIONAL: &str = r#"
[level.2097.pull.big]
name = "The big pack"
order = 3
optional = true
area = "hall"
npc = ["juggernaut", "channeler"]
"#;

#[test]
fn a_routes_forces_are_read_with_and_without_its_optional_pulls() {
    let r = Rubric::from_files(leveled(&format!("{CREATURES}{PULLS}{OPTIONAL}")))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let l = r.level(2097).unwrap().unwrap();
    // An optional pull is checked as any other and keeps its place in the
    // route; unsaid, a pull is not optional.
    assert!(check_level(&l).is_empty(), "{:?}", check_level(&l));
    let route: Vec<(&str, bool)> = (l.pulls_in_order().into_iter())
        .map(|(k, p)| (k, p.optional))
        .collect();
    assert_eq!(
        route,
        [("entrance", false), ("bridge", false), ("big", true)]
    );
    // The entrance's 25 and the bridge's 75 without it; its 50 on top.
    assert_eq!(l.pull["big"].forces(&l), 50);
    assert_eq!(l.route_forces(), (100, 150));
    // Written only where it is true, after the pull's order, and read
    // back to the level it was written from.
    let written = toml::to_string(&l).unwrap();
    assert!(
        written.contains(
            "\n[pull.big]\nname = \"The big pack\"\norder = 3\noptional = true\narea = \"hall\"\n"
        ),
        "{written}"
    );
    assert_eq!(written.matches("optional").count(), 1, "{written}");
    let mut read: Level = toml::from_str(&written).unwrap();
    (read.ui_map, read.instance) = (l.ui_map, l.instance.clone());
    assert_eq!(read, l);
    // A level read from text alone counts nothing until the instance's
    // creatures are laid in, and leaves a pull switched off out.
    assert_eq!(read.route_forces(), (0, 0));
    read.npc = l.npc.clone();
    assert_eq!(read.route_forces(), (100, 150));
    if let Some(p) = read.pull.get_mut("bridge") {
        p.enabled = false;
    }
    assert_eq!(read.route_forces(), (25, 75));
    // A route with no optional pull reads alike both ways; a level with
    // no pulls gives none.
    let plain = Rubric::from_files(leveled(&format!("{CREATURES}{PULLS}"))).unwrap();
    assert_eq!(
        plain.level(2097).unwrap().unwrap().route_forces(),
        (100, 100)
    );
    assert_eq!(r.level(2098).unwrap().unwrap().route_forces(), (0, 0));
    // A value that is no boolean is the level's error.
    let r = Rubric::from_files(leveled(&format!(
        "{CREATURES}{PULLS}{OPTIONAL}\n[level.2097.pull.typo]\norder = 4\narea = \"hall\"\nnpc = []\noptional = \"yes\"\n"
    )))
    .unwrap();
    let e = r.level(2097).unwrap().unwrap_err();
    assert!(
        e.contains("s/raid/level.2097") && e.contains("boolean"),
        "{e}"
    );
}

#[test]
fn a_default_is_an_encounters_table_never_a_pull() {
    // The defaults reach every encounter and level under them alike.
    let r = Rubric::from_files(leveled(&format!("{CREATURES}{PULLS}")))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let e = r.encounter(3492, None).unwrap().unwrap();
    let l = r.level(2097).unwrap().unwrap();
    for (view, map, travel) in [(&e.view, &e.map, &e.travel), (&l.view, &l.map, &l.travel)] {
        assert_eq!(
            (view.zoom, map.ceiling, travel.run),
            (Some(1.15), Some(25.0), Some(7.0))
        );
    }
    assert_eq!(l.pull.len(), 2);
    // Every encounter under a `[defaults]` lays it in whole, and knows no
    // `pull`: one would make every encounter of the season or instance
    // unreadable. So a default no encounter has is refused where its file
    // is read, naming the file and the key: the instance's, the season's,
    // one under a difficulty, and one that is no table of either kind.
    for (file, given, key) in [
        (
            1,
            "\n[defaults.pull.everywhere]\norder = 9\narea = \"hall\"\nnpc = [\"juggernaut\"]\n",
            "[defaults.pull]: a pull is one level's alone",
        ),
        (
            0,
            "\n[defaults.pull.too]\norder = 8\narea = \"hall\"\nnpc = [\"channeler\"]\n",
            "[defaults.pull]: a pull is one level's alone",
        ),
        (
            1,
            "\n[defaults.difficulty.mythic.pull.x]\norder = 1\n",
            "[defaults.difficulty.mythic.pull]",
        ),
        (
            0,
            "\n[defaults.level.x]\nzoom = 1\n",
            "[defaults.level]: a default gives only an encounter's tables",
        ),
    ] {
        let mut f = leveled(&format!("{CREATURES}{PULLS}"));
        f[file].1.push_str(given);
        let errs = Rubric::from_files(f).unwrap_err();
        let path = ["s/season.toml", "s/raid/instance.toml"][file];
        assert!(
            errs.iter().any(|e| e.starts_with(path) && e.contains(key)),
            "{key}: {errs:?}"
        );
    }
    // A set's defaults are held the same: a user's file named and left out,
    // the encounter still answered under it; a curated set's refused whole.
    let file = || {
        owned(&[(
            "s/raid/instance.toml",
            "schema = 1\n[defaults.pull.x]\norder = 1\narea = \"hall\"\nnpc = []\n",
        )])
    };
    let user = r.clone().with_user(file());
    let errs = user.user_errors();
    assert!(
        errs.len() == 1 && errs[0].contains("instance.toml") && errs[0].contains("[defaults.pull]"),
        "{errs:?}"
    );
    assert!(user.encounter(3492, None).unwrap().is_ok());
    let errs = r.with_curated("pack", file()).unwrap_err();
    assert!(errs[0].contains("[defaults.pull]"), "{errs:?}");
}

#[test]
fn every_default_table_is_an_encounters() {
    // `DEFAULTS_TABLES` is exactly the encounter's tables, so a table added
    // to the schema is a default too, and nothing else is.
    let Value::Table(every) = Value::try_from(every_key()).unwrap() else {
        panic!("an encounter is a table");
    };
    let tables: std::collections::BTreeSet<&str> = every
        .iter()
        .filter(|(_, v)| v.is_table())
        .map(|(k, _)| k.as_str())
        .collect();
    let defaults: std::collections::BTreeSet<&str> = DEFAULTS_TABLES.into_iter().collect();
    assert_eq!(defaults, tables);
}

#[test]
fn a_bad_pull_is_found() {
    let bad = r#"
[level.2097.map.area.hall]
shape = [{ center = [0, 0], radius = 30 }]

[level.2097.map.area.court]
shape = [{ center = [100, 0], radius = 20 }]
encounter = 3492

[level.2097.pull.astray]
order = 1
area = "nowhere"
npc = ["juggernaut", "ghost", "ghost"]
spots = [[0, 0]]

[level.2097.pull.boss]
order = 2
area = "court"
npc = []

[level.2097.pull.ghostly]
order = 3
area = "hall"
npc = ["gone"]

[level.2097.pull.twin]
order = 1
area = "hall"
npc = ["channeler"]
spots = [[0, nan]]

[level.2097.pull.off]
enabled = false
order = 2
area = "nowhere"
npc = ["ghost"]

[level.2097.pull.zero]
order = 0
area = "hall"
npc = ["juggernaut"]
"#;
    let r = Rubric::from_files(leveled(&format!("{CREATURES}{bad}")))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let problems = check_level(&r.level(2097).unwrap().unwrap());
    assert_eq!(
        problems,
        [
            "pull.astray stands in area \"nowhere\", which the map does not have",
            "pull.astray names npc \"ghost\", which the instance does not have",
            "pull.astray's spots are 1 and its members 3: a spot a member, or none",
            "pull.boss stands in area \"court\", encounter 3492's court: a boss is no pull",
            "pull.boss pulls no one",
            // A creature the instance switched off is none of its own.
            "pull.ghostly names npc \"gone\", which the instance does not have",
            "pull.twin has a spot that is not a number",
            "pull.twin is pull 1 of the route, as pull.astray is",
            // A route counts from 1.
            "pull.zero is pull 0 of the route, which counts from 1",
        ]
    );
    // An instance with no creatures has none for a pull to name.
    let r = Rubric::from_files(leveled(bad)).unwrap();
    let problems = check_level(&r.level(2097).unwrap().unwrap());
    assert!(
        problems
            .iter()
            .any(|p| p == "pull.twin names npc \"channeler\", which the instance does not have"),
        "{problems:#?}"
    );
}

#[test]
fn a_ground_is_written_as_a_word_or_a_height() {
    for (d, text) in [
        (Datum::Terrain, "\"terrain\""),
        (Datum::Walked, "\"walked\""),
        (Datum::Height(390.25), "390.25"),
    ] {
        let m = Map {
            ground: Some(d),
            ..Map::default()
        };
        let written = toml::to_string(&m).unwrap();
        assert_eq!(written.trim(), format!("ground = {text}"));
        assert_eq!(toml::from_str::<Map>(&written).unwrap(), m);
    }
    assert_eq!(
        toml::from_str::<Map>("ground = 12").unwrap().ground,
        Some(Datum::Height(12.0))
    );
}

#[test]
fn the_embedded_rubric_resolves_and_checks_every_level() {
    let r = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let mut problems = Vec::new();
    let encounters: BTreeMap<u32, (String, String)> = r
        .encounters()
        .into_iter()
        .map(|(id, s, i)| (id, (s.id.clone(), i.slug.clone())))
        .collect();
    // An area's encounter is one of its instance's: a level's, and an
    // encounter's own map's.
    let owned_by = |what: String, season: &str, inst: &str, map: &Map| -> Vec<String> {
        map.area
            .iter()
            .filter_map(|(k, a)| {
                let id = a.encounter?;
                (encounters.get(&id) != Some(&(season.to_string(), inst.to_string())))
                    .then(|| format!("{what}: area {k} names encounter {id}, not one of {inst}'s"))
            })
            .collect()
    };
    // A court's encounter is posted on its level: its map's UiMap is the
    // level's key. The exceptions, each the level the log posts the fight
    // on where the encounter's map says another:
    // - Echo of Doragosa (2565): its draft says UiMap 2099 (the floors
    //   cache's Algeth'ar level 3, whose box holds the court too), but
    //   every player post of the fight on both stored runs (6,001 and
    //   6,731) is on 2097, none on 2099.
    let posted_elsewhere: [(u32, u32); 1] = [(2565, 2097)];
    let levels = r.levels();
    assert!(levels.len() >= 7, "{} levels", levels.len());
    for (ui_map, s, i) in levels {
        match r.level(ui_map) {
            Some(Ok(l)) => {
                problems.extend(
                    check_level(&l)
                        .into_iter()
                        .map(|p| format!("level {ui_map}: {p}")),
                );
                problems.extend(owned_by(format!("level {ui_map}"), &s.id, &i.slug, &l.map));
                for (k, a) in &l.map.area {
                    let Some(id) = a.encounter else { continue };
                    let on = r
                        .encounter_at(id, None, Tier::Base)
                        .and_then(Result::ok)
                        .and_then(|e| e.map.ui_map);
                    let excepted = posted_elsewhere.contains(&(id, ui_map));
                    if (on == Some(ui_map)) == excepted {
                        problems.push(format!(
                            "level {ui_map}: area {k}'s encounter {id} is on UiMap {on:?}{}",
                            if excepted {
                                ", so its exception is stale"
                            } else {
                                ""
                            }
                        ));
                    }
                }
            }
            Some(Err(e)) => problems.push(e),
            None => problems.push(format!("level {ui_map} is listed and not answered")),
        }
    }
    for (id, s, i) in r.encounters() {
        if let Some(Ok(e)) = r.encounter_at(id, None, Tier::Base) {
            problems.extend(owned_by(format!("encounter {id}"), &s.id, &i.slug, &e.map));
            for a in e.map.area.values() {
                if a.encounter.is_some_and(|n| n != id) {
                    problems.push(format!("encounter {id}: an area names another encounter"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every embedded level as it resolves, written out whole: a later
/// `[defaults.travel]`, a season's `[defaults.map]` or `[defaults.view]`,
/// or an edit to a level shows here, line by line. `WOWDPS_BLESS=1`
/// writes the fixture anew from this build.
#[test]
fn every_level_resolves_as_its_fixture_says() {
    let r = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let mut out = String::new();
    for (ui_map, s, i) in r.levels() {
        let l = r.level(ui_map).unwrap().unwrap();
        out.push_str(&format!("== {ui_map} {}/{}\n{l:#?}\n", s.id, i.slug));
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/resolved-levels.txt");
    if std::env::var_os("WOWDPS_BLESS").is_some() {
        std::fs::write(path, &out).unwrap();
        return;
    }
    let want = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{path}: {e}; run with WOWDPS_BLESS=1"));
    for (n, (w, g)) in want.lines().zip(out.lines()).enumerate() {
        assert_eq!(
            w,
            g,
            "fixtures/resolved-levels.txt line {}; if the change is meant, run with WOWDPS_BLESS=1",
            n + 1
        );
    }
    assert_eq!(want.lines().count(), out.lines().count());
}

// ---- an instance's trash -----------------------------------------------------

/// The test raid's trash, as a tuned file would give it (its ids made
/// up): an NPC of the instance's amended and one added, a kick target, a
/// pool at a death, a ringed mark, a flash kept to Mythic alone, and
/// overrides a keystone reads through its chain.
const TRASH: &str = r#"
schema = 1

[npc.juggernaut]
still = true

[npc.whelp]
name = "Infused Whelp"
creature = [187894]

[ability.arc-bolt]
name = "Arc Bolt"
spell = [900302]
by = "channeler"
marks = ["interrupt"]

[ground.blaze]
kind = "pool"
shape = { kind = "circle", radius = 4 }
at = [{ death = { npc = "juggernaut" } }]
ends = { after = 12 }

[mark.bomb]
aura = 900303
ring = "purple"
label = "left"

[flash.detonation]
spell = [900304]
only = ["mythic"]

[difficulty.heroic.mark.bomb]
ring = "gold"

[difficulty.keystone.ground.blaze]
shape = { radius = 5 }
"#;

/// The test raid with its own creatures (`CREATURES`), and `trash` its
/// embedded trash file where not empty.
fn trashed(trash: &str) -> Vec<(String, String)> {
    let mut f = files(&[]);
    f[1].1 = format!("{INSTANCE}{CREATURES}");
    if !trash.is_empty() {
        f.push(("s/raid/trash.toml".into(), trash.into()));
    }
    f
}

/// The trash a base of the test raid's creatures alone resolves to.
fn base_trash(r: &Rubric) -> Encounter {
    let (_, i) = r.instance_by_map(3004).unwrap();
    Encounter {
        encounter: 0,
        name: "The Test Raid".into(),
        instance: "raid".into(),
        npc: i.npcs().clone(),
        ..Encounter::default()
    }
}

#[test]
fn a_trash_file_resolves_over_the_instances_creatures() {
    let r = Rubric::from_files(trashed(TRASH)).unwrap_or_else(|e| panic!("{e:?}"));
    let e = r.trash(3004).unwrap().unwrap();
    assert!(check_trash(&e).is_empty(), "{:?}", check_trash(&e));
    // The instance's: its id 0, its name and slug, the format read.
    assert_eq!(
        (
            e.schema,
            e.encounter,
            e.name.as_str(),
            e.instance.as_str(),
            e.order
        ),
        (SCHEMA, 0, "The Test Raid", "raid", 0)
    );
    // The instance's creatures under it, one amended key by key, one
    // added; the switched-off one gone.
    let keys: Vec<&str> = e.npc.keys().map(String::as_str).collect();
    assert_eq!(keys, ["channeler", "juggernaut", "thundercloud", "whelp"]);
    let j = &e.npc["juggernaut"];
    assert_eq!(
        (j.still, j.forces, j.creature.as_slice()),
        (true, Some(25), &[188244][..])
    );
    assert_eq!(e.ability["arc-bolt"].marks, ["interrupt"]);
    // A keystone reads its own override and Heroic's through its chain; an
    // `only` is exact, so Mythic's flash is not a keystone's.
    assert_eq!(e.ground["blaze"].shape, Shape::Circle { radius: 5.0 });
    assert_eq!(e.mark["bomb"].ring.as_deref(), Some("gold"));
    assert!(e.flash.is_empty());
    // Nothing of a room: no map, view, travel, phase or room.
    assert_eq!(
        (&e.map, &e.view, &e.travel),
        (&Map::default(), &View::default(), &Travel::default())
    );
    assert!(e.phase.is_empty() && e.room.is_empty());
    // The traced form is the same encounter, each key by who set it.
    let (traced, p) = r.trash_traced(3004, Tier::User).unwrap().unwrap();
    assert_eq!(traced, e);
    let src = |path: &str| {
        let by = p.source_of(path).unwrap_or_else(|| panic!("{path}"));
        (by.source.clone(), by.difficulty)
    };
    let tuned = Source::Curated(Origin::Embedded);
    assert_eq!(src("npc.juggernaut.forces"), (Source::Instance, None));
    assert_eq!(src("npc.juggernaut.still"), (tuned.clone(), None));
    assert_eq!(
        src("ground.blaze.shape.radius"),
        (tuned.clone(), Some("keystone"))
    );
    assert_eq!(src("mark.bomb.ring"), (tuned, Some("heroic")));
    for path in ["name", "instance", "encounter"] {
        assert_eq!(src(path), (Source::Instance, None), "{path}");
    }
    // A map no season has has no trash.
    assert!(r.trash(1).is_none() && r.trash_traced(1, Tier::User).is_none());
}

#[test]
fn the_base_alone_is_the_instances_creatures() {
    // No trash file: every tier reads the creatures and nothing else.
    let r = Rubric::from_files(trashed("")).unwrap();
    for tier in Tier::ALL {
        assert_eq!(r.trash_at(3004, tier).unwrap().unwrap(), base_trash(&r));
    }
    // With one, the base still reads them alone, and a rubric capped at
    // the base answers so whatever is asked.
    let r = Rubric::from_files(trashed(TRASH)).unwrap();
    assert_eq!(
        r.trash_at(3004, Tier::Base).unwrap().unwrap(),
        base_trash(&r)
    );
    let capped = r.clone().with_tier(Tier::Base);
    assert_eq!(capped.trash(3004).unwrap().unwrap(), base_trash(&r));
    // An instance with no creatures has an empty trash.
    let bare = Rubric::from_files(files(&[])).unwrap();
    let e = bare.trash(3004).unwrap().unwrap();
    assert!(e.npc.is_empty() && check_trash(&e).is_empty());
    // A trash file lays nothing on an encounter.
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), ulatek(None));
    assert_eq!(
        r.layers_of(3492),
        Rubric::from_files(files(&[])).unwrap().layers_of(3492)
    );
}

#[test]
fn a_trash_file_gives_no_map_phase_or_room() {
    for (given, key) in [
        ("[map]\nfloor = \"x\"\n", "`map`"),
        ("[view]\nturn = 1\n", "`view`"),
        ("[travel]\nrun = 1\n", "`travel`"),
        ("[phase.p]\nname = \"P\"\n", "`phase`"),
        ("[room.r]\nname = \"R\"\n", "`room`"),
        ("encounter = 7\n", "`encounter`"),
        ("order = 1\n", "`order`"),
        ("name = \"Trash\"\n", "`name`"),
        (
            "[difficulty.keystone.phase.p]\nname = \"P\"\n",
            "`difficulty.keystone.phase`",
        ),
    ] {
        let file = format!("schema = 1\n{given}");
        // Embedded: the rubric is refused, naming the file and the key.
        let errs = Rubric::from_files(trashed(&file)).unwrap_err();
        assert!(
            errs.len() == 1
                && errs[0].starts_with("s/raid/trash.toml: ")
                && errs[0].contains(key)
                && errs[0].contains("a trash file gives only"),
            "{given}: {errs:?}"
        );
        // A bundle is refused whole; a user's file is named and left out.
        let laid = || owned(&[("s/raid/trash.toml", &file)]);
        let errs = Rubric::from_files(trashed(""))
            .unwrap()
            .with_curated("pack", laid())
            .unwrap_err();
        assert!(errs[0].contains(key), "{given}: {errs:?}");
        let user = Rubric::from_files(trashed("")).unwrap().with_user(laid());
        assert!(
            user.user_errors().len() == 1 && user.user_errors()[0].contains(key),
            "{given}: {:?}",
            user.user_errors()
        );
    }
    // An instance the base lacks.
    let errs = Rubric::from_files(trashed(""))
        .unwrap()
        .with_curated("pack", owned(&[("s/nowhere/trash.toml", "schema = 1\n")]))
        .unwrap_err();
    assert!(errs[0].contains("the base has no s/nowhere"), "{errs:?}");
    let mut f = trashed("");
    f.push(("s/nowhere/trash.toml".into(), "schema = 1\n".into()));
    let errs = Rubric::from_files(f).unwrap_err();
    assert!(errs[0].contains("no s/nowhere/instance.toml"), "{errs:?}");
}

#[test]
fn a_trash_file_amends_a_creature_but_never_its_ids_or_forces() {
    // Its forces or its ids, at the top or under a difficulty, a
    // switched-off creature's too, and forces for one it adds: refused,
    // naming the key, the embedded file, a bundle and a user's alike.
    for (given, key) in [
        ("[npc.juggernaut]\nforces = 99\n", "`npc.juggernaut.forces`"),
        (
            "[npc.juggernaut]\ncreature = [1]\n",
            "`npc.juggernaut.creature`",
        ),
        ("[npc.gone]\ncreature = [2]\n", "`npc.gone.creature`"),
        (
            "[npc.whelp]\nname = \"Whelp\"\ncreature = [187894]\nforces = 1\n",
            "`npc.whelp.forces`",
        ),
        (
            "[difficulty.keystone.npc.juggernaut]\nforces = 99\n",
            "`difficulty.keystone.npc.juggernaut.forces`",
        ),
        (
            "[difficulty.mythic.npc.channeler]\ncreature = [5]\n",
            "`difficulty.mythic.npc.channeler.creature`",
        ),
    ] {
        let file = format!("schema = 1\n{given}");
        let errs = Rubric::from_files(trashed(&file)).unwrap_err();
        assert!(
            errs.len() == 1 && errs[0].starts_with("s/raid/trash.toml: ") && errs[0].contains(key),
            "{given}: {errs:?}"
        );
        let laid = || owned(&[("s/raid/trash.toml", &file)]);
        let errs = Rubric::from_files(trashed(""))
            .unwrap()
            .with_curated("pack", laid())
            .unwrap_err();
        assert!(errs[0].contains(key), "{given}: {errs:?}");
        let user = Rubric::from_files(trashed("")).unwrap().with_user(laid());
        assert!(
            user.user_errors().len() == 1 && user.user_errors()[0].contains(key),
            "{given}: {:?}",
            user.user_errors()
        );
    }
    // The rest of a creature may be amended, and one it adds gives its own
    // ids.
    let amend = r#"schema = 1

[npc.juggernaut]
name = "Big Juggernaut"
role = "boss"
icon = 7
still = true

[npc.thundercloud]
owner = "juggernaut"
shares = "juggernaut"

[npc.whelp]
name = "Infused Whelp"
creature = [187894]
"#;
    let r = Rubric::from_files(trashed(amend)).unwrap_or_else(|e| panic!("{e:?}"));
    let e = r.trash(3004).unwrap().unwrap();
    assert!(check_trash(&e).is_empty(), "{:?}", check_trash(&e));
    let j = &e.npc["juggernaut"];
    assert_eq!(
        (j.name.as_str(), j.role, j.icon, j.still),
        ("Big Juggernaut", Role::Boss, Some(7), true)
    );
    let t = &e.npc["thundercloud"];
    assert_eq!(
        (t.owner.as_deref(), t.shares.as_deref()),
        (Some("juggernaut"), Some("juggernaut"))
    );
    // Whatever is laid over them, each of the instance's creatures is its
    // ids and its forces, and one the trash adds gives none.
    let (_, i) = r.instance_by_map(3004).unwrap();
    for (k, n) in i.npcs() {
        assert_eq!(
            (&e.npc[k].creature, e.npc[k].forces),
            (&n.creature, n.forces),
            "{k}"
        );
        for &c in &n.creature {
            assert_eq!(
                i.npc_of(c).map(|(_, n)| n.forces),
                Some(e.npc[k].forces),
                "{c}"
            );
        }
    }
    assert_eq!(e.npc["whelp"].forces, None);
}

#[test]
fn a_trash_file_switches_a_creature_on_and_an_only_is_exact() {
    // The instance's switched-off creature is not the instance's (`npcs`,
    // `npc_of`), and stays off under a trash that leaves it.
    let r = Rubric::from_files(trashed("")).unwrap();
    assert!(!r.trash(3004).unwrap().unwrap().npc.contains_key("gone"));
    // A trash file switches it on, whole as the instance wrote it.
    let r = Rubric::from_files(trashed("schema = 1\n[npc.gone]\nenabled = true\n"))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let e = r.trash(3004).unwrap().unwrap();
    let gone = &e.npc["gone"];
    assert_eq!(
        (gone.name.as_str(), gone.creature.as_slice()),
        ("Gone", &[1][..])
    );
    assert!(check_trash(&e).is_empty(), "{:?}", check_trash(&e));
    let (_, i) = r.instance_by_map(3004).unwrap();
    assert!(i.npc_of(1).is_none() && !i.npcs().contains_key("gone"));
    // A creature the instance keeps to Mythic is not a keystone's: an
    // `only` is exact. It is still the instance's, and a trash file may
    // keep it to a keystone instead; one the trash keeps to Mythic goes.
    let mythic =
        "\n[npc.mythic-only]\nname = \"Only On Mythic\"\ncreature = [5]\nonly = [\"mythic\"]\n";
    let kept = |trash: &str| {
        let mut f = trashed(trash);
        f[1].1.push_str(mythic);
        let r = Rubric::from_files(f).unwrap_or_else(|e| panic!("{e:?}"));
        let (_, i) = r.instance_by_map(3004).unwrap();
        assert!(i.npc_of(5).is_some() && check_instance(i).is_empty());
        let e = r.trash(3004).unwrap().unwrap();
        (
            e.npc.contains_key("mythic-only"),
            e.npc.contains_key("juggernaut"),
        )
    };
    assert_eq!(kept(""), (false, true));
    assert_eq!(
        kept(
            "schema = 1\n[npc.mythic-only]\nonly = [\"keystone\"]\n[npc.juggernaut]\nonly = [\"mythic\"]\n"
        ),
        (true, false)
    );
}

#[test]
fn a_trash_trigger_counts_from_a_trigger_never_the_start() {
    let file = r#"schema = 1

[event.at-start]
on = "start"

[event.from-start]
on = { after = 30 }

[event.since-start]
on = { after = { secs = 1, since = "start" } }

[event.after-a-death]
on = { after = { secs = 3, since = { death = { npc = "juggernaut" } } } }

[place.spot]
kind = "spawn"
at = [0, 0]
from = { event = "after-a-death" }
"#;
    let r = Rubric::from_files(trashed(file)).unwrap();
    let problems = check_trash(&r.trash(3004).unwrap().unwrap());
    assert_eq!(
        problems,
        [
            "event.at-start fires at the start, which in a trash file counts from the key's start",
            "event.from-start strikes 30 s after the start, which in a trash file counts from the key's start; count from a trigger (`since`)",
            "event.since-start fires at the start, which in a trash file counts from the key's start",
        ]
    );
    // The same in an encounter counts from its pull, and is fine.
    let mut f = files(&[]);
    f[3].1 = format!(
        "{TUNED}\n{}",
        file.replace("schema = 1\n", "")
            .replace("juggernaut", "ulatek")
    );
    let r = Rubric::from_files(f).unwrap();
    let e = r.encounter(3492, None).unwrap().unwrap();
    assert!(check(&e).is_empty(), "{:?}", check(&e));
    // An encounter numbered 0 (a half-built one, `Encounter::default()`'s
    // id) is still an encounter to `check`: the trash's rules are
    // `check_trash`'s alone.
    let half = Encounter {
        encounter: 0,
        ..e.clone()
    };
    assert!(check(&half).is_empty(), "{:?}", check(&half));
    // Its own Stage One begins at the start too.
    assert_eq!(check_trash(&half).len(), 4, "{:?}", check_trash(&half));
    // And a trash names what it lacks as the trash's.
    let r = Rubric::from_files(trashed(
        "schema = 1\n[ability.a]\nname = \"A\"\nby = \"nobody\"\n",
    ))
    .unwrap();
    assert_eq!(
        check_trash(&r.trash(3004).unwrap().unwrap()),
        ["ability.a names npc \"nobody\", which the trash does not have"]
    );
}

#[test]
fn a_trash_file_is_laid_as_a_tuned_file_is() {
    const PACK: &str =
        "schema = 1\n[ground.blaze]\nends = { after = 20 }\n[flash.detonation]\nonly = []\n";
    const MINE: &str = "schema = 1\n[mark.bomb]\nlabel = \"carried\"\n";
    let r = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_curated("pack", owned(&[("s/raid/trash.toml", PACK)]))
        .unwrap()
        .with_user(owned(&[("s/raid/trash.toml", MINE)]));
    assert!(r.user_errors().is_empty(), "{:?}", r.user_errors());
    let at = |tier| r.trash_at(3004, tier).unwrap().unwrap();
    assert_eq!(at(Tier::Base), base_trash(&r));
    let curated = at(Tier::Curated);
    assert_eq!(curated.ground["blaze"].ends, Ends::After(20.0));
    assert!(curated.flash.contains_key("detonation"));
    assert_eq!(curated.mark["bomb"].label, Some(MarkLabel::Left));
    let user = at(Tier::User);
    assert_eq!(user.mark["bomb"].label, Some(MarkLabel::Carried));
    assert_eq!(r.trash(3004).unwrap().unwrap(), user);
    let (_, p) = r.trash_traced(3004, Tier::User).unwrap().unwrap();
    assert_eq!(
        p.source_of("ground.blaze.ends.after").unwrap().source,
        bundle("pack")
    );
    assert_eq!(p.source_of("mark.bomb.label").unwrap().source, Source::User);
    // The version moves with a trash file.
    assert_ne!(
        Rubric::from_files(trashed(TRASH)).unwrap().version(),
        Rubric::from_files(trashed("")).unwrap().version()
    );
    // A tree on disk's trash file takes the embedded one's place: a mark
    // of its own, none of the embedded file's entries.
    const TREE: &str = "schema = 1\n[mark.bomb]\naura = 1\nlabel = \"carried\"\n";
    let tree = tree_on_disk("trash", &owned(&[("s/raid/trash.toml", TREE)]));
    let r = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_curated_dir("tree", &tree)
        .unwrap();
    let e = r.trash_at(3004, Tier::Curated).unwrap().unwrap();
    assert!(e.ground.is_empty() && e.ability.is_empty(), "{e:?}");
    assert_eq!(e.mark["bomb"].label, Some(MarkLabel::Carried));
    let (_, p) = r.trash_traced(3004, Tier::Curated).unwrap().unwrap();
    assert_eq!(
        p.source_of("mark.bomb.label").unwrap().source,
        dir_source("tree")
    );
    let _ = std::fs::remove_dir_all(&tree);
}

#[test]
fn trash_layers_list_the_sources_with_a_trash_file() {
    // The base alone: the instance's file, its creatures.
    let base = Rubric::from_files(trashed("")).unwrap();
    assert_eq!(base.trash_layers_of(3004), [Source::Instance]);
    assert!(!base.has_curated_trash(3004) && !base.has_user_trash(3004));
    assert!(base.trash_layers_of(1).is_empty() && !base.has_curated_trash(1));
    // The embedded file, a bundle and the user's, in the stack's order; a
    // bundle with no trash file is not listed.
    const MINE: &str = "schema = 1\n[mark.bomb]\nlabel = \"carried\"\n";
    let r = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_curated("pack", owned(&[("s/raid/trash.toml", "schema = 1\n")]))
        .unwrap()
        .with_curated(
            "other",
            owned(&[("s/raid/3492-ulatek.toml", "schema = 1\n")]),
        )
        .unwrap()
        .with_user(owned(&[("s/raid/trash.toml", MINE)]));
    assert_eq!(
        r.trash_layers_of(3004),
        [
            Source::Instance,
            Source::Curated(Origin::Embedded),
            bundle("pack"),
            Source::User,
        ]
    );
    assert!(r.has_curated_trash(3004) && r.has_user_trash(3004));
    // A trash file is no encounter's layer, nor an encounter's file a
    // trash's.
    assert!(!r.layers_of(3492).contains(&bundle("pack")));
    assert!(r.layers_of(3492).contains(&bundle("other")));
    // A tree on disk with none takes the embedded one away.
    let empty = tree_on_disk("trash-layers", &[]);
    let r = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_curated_dir("empty", &empty)
        .unwrap();
    assert_eq!(r.trash_layers_of(3004), [Source::Instance]);
    assert!(!r.has_curated_trash(3004));
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn a_bad_trash_file_is_refused_or_left_out() {
    // A typo makes the trash unreadable: a bundle is refused whole, naming
    // it; a user's file is left out and the trash reads as before.
    const TYPO: &str = "schema = 1\n[ground.blaze]\nknd = \"pool\"\n";
    let errs = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_curated("typo", owned(&[("s/raid/trash.toml", TYPO)]))
        .unwrap_err();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].starts_with("curated \"typo\": s/raid's trash (on every difficulty): ")
            && errs[0].contains("knd"),
        "{errs:?}"
    );
    let r = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_user(owned(&[("s/raid/trash.toml", TYPO)]));
    assert!(
        r.user_errors().len() == 1
            && r.user_errors()[0].starts_with("s/raid/trash.toml: left out")
            && r.user_errors()[0].contains("knd"),
        "{:?}",
        r.user_errors()
    );
    assert_eq!(
        r.trash(3004).unwrap().unwrap(),
        Rubric::from_files(trashed(TRASH))
            .unwrap()
            .trash(3004)
            .unwrap()
            .unwrap()
    );
    // A user's file that leans on the embedded trash file (it only widens
    // the pool on a keystone, its override last as every source's are) is
    // read again when a tree takes that file away: left out over the tree,
    // as it would be laid last.
    const LEANS: &str = "schema = 1\n[difficulty.keystone.ground.blaze.shape]\nradius = 7\n";
    let bare = tree_on_disk("trash-bare", &[]);
    let r = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_user(owned(&[("s/raid/trash.toml", LEANS)]));
    assert!(r.user_errors().is_empty(), "{:?}", r.user_errors());
    assert_eq!(
        r.trash(3004).unwrap().unwrap().ground["blaze"].shape,
        Shape::Circle { radius: 7.0 }
    );
    let r = r.with_curated_dir("bare", &bare).unwrap();
    assert!(
        r.user_errors()
            .iter()
            .any(|e| e.starts_with("s/raid/trash.toml: left out over curated \"bare\"")),
        "{:?}",
        r.user_errors()
    );
    assert_eq!(r.trash(3004).unwrap().unwrap(), base_trash(&r));
    // A bundle that leans on it the same is refused by a tree taking it.
    let leaning = Rubric::from_files(trashed(TRASH))
        .unwrap()
        .with_curated("leans", owned(&[("s/raid/trash.toml", LEANS)]))
        .unwrap();
    let errs = leaning.with_curated_dir("bare", &bare).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.starts_with("curated \"bare\": s/raid's trash")),
        "{errs:?}"
    );
    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn a_trash_file_where_no_reader_looks_is_refused() {
    // A returning dungeon: an older season holds the raid too, on the same
    // map, and the newer season answers it.
    let older = "schema = 1\nname = \"Older\"\norder = 1\n";
    let mut f = trashed("");
    f.extend(owned(&[
        ("old/season.toml", older),
        ("old/raid/instance.toml", INSTANCE),
        ("old/raid/3492-ulatek.draft.toml", DRAFT),
    ]));
    let r = Rubric::from_files(f.clone()).unwrap();
    let (s, _) = r.instance_by_map(3004).unwrap();
    assert_eq!(s.id, "s");
    let laid = || owned(&[("old/raid/trash.toml", "schema = 1\n")]);
    let errs = r.clone().with_curated("old", laid()).unwrap_err();
    assert!(
        errs[0].contains("map 3004's trash is answered from s/raid"),
        "{errs:?}"
    );
    let user = r.with_user(laid());
    assert!(
        user.user_errors()[0].contains("answered from s/raid"),
        "{:?}",
        user.user_errors()
    );
    f.push(("old/raid/trash.toml".into(), "schema = 1\n".into()));
    let errs = Rubric::from_files(f).unwrap_err();
    assert_eq!(
        errs,
        [
            "old/raid/trash.toml: map 3004's trash is answered from s/raid; a layer's trash file goes there"
        ]
    );
}

/// Every instance's trash, on a keystone, resolves and checks at every
/// tier; the base alone is the instance's own creatures and nothing else.
/// The tree holds the base alone (the curated set's trash files live at the
/// repository's `curated/seasons/`, read and checked by the replay's
/// tests), so every tier reads the base; a tree that holds trash files
/// checks them, each changing its trash.
#[test]
fn the_embedded_rubric_resolves_and_checks_every_instances_trash() {
    let r = Rubric::embedded().unwrap_or_else(|e| panic!("{}", e.join("\n")));
    let mut problems = Vec::new();
    let mut trashed = 0;
    for s in r.seasons() {
        for i in s.instances.values() {
            let Some(map) = i.map else { continue };
            // A returning dungeon's older copy is not where its map's
            // trash is read.
            let answered = r
                .instance_by_map(map)
                .map(|(s, i)| (s.id.as_str(), i.slug.as_str()));
            if answered != Some((s.id.as_str(), i.slug.as_str())) {
                continue;
            }
            for tier in Tier::ALL {
                match r.trash_at(map, tier) {
                    Some(Ok(e)) => {
                        problems.extend(
                            check_trash(&e)
                                .into_iter()
                                .map(|p| format!("{}/{} {}: {p}", s.id, i.slug, tier.name())),
                        );
                        // Each creature's ids and forces are the
                        // instance's; one a trash file adds gives none.
                        for (k, n) in &e.npc {
                            let own = match i.written.get(k) {
                                Some(base) => {
                                    n.creature == base.creature
                                        && n.forces == base.forces
                                        && (!base.enabled
                                            || n.creature.iter().all(|&c| {
                                                i.npc_of(c).map(|(_, b)| b.forces) == Some(n.forces)
                                            }))
                                }
                                None => n.forces.is_none(),
                            };
                            if !own {
                                problems.push(format!(
                                    "{}/{} {}: npc.{k}'s ids or forces are not the instance's",
                                    s.id,
                                    i.slug,
                                    tier.name()
                                ));
                            }
                        }
                    }
                    Some(Err(e)) => problems.push(format!("{}: {e}", tier.name())),
                    None => problems.push(format!("{}/{}: no trash", s.id, i.slug)),
                }
            }
            let base = r.trash_at(map, Tier::Base).unwrap().unwrap();
            let npc: BTreeMap<String, Npc> = i
                .npcs()
                .iter()
                .filter(|(_, n)| difficulty::applies(&n.only, difficulty::chain(8)))
                .map(|(k, n)| (k.clone(), n.clone()))
                .collect();
            let want = Encounter {
                encounter: 0,
                name: i.name.clone(),
                instance: i.slug.clone(),
                npc,
                ..Encounter::default()
            };
            if base != want {
                problems.push(format!("{}/{}: the base trash is {base:?}", s.id, i.slug));
            }
            if r.trash_at(map, Tier::Curated) != Some(Ok(base)) {
                trashed += 1;
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let files = embedded_files()
        .iter()
        .filter(|(p, _)| matches!(FileRole::of(p), Ok(Some(FileRole::Trash { .. }))))
        .count();
    assert_eq!(trashed, files, "every trash file read changes its trash");
    // Ruby Life Pools', the first given its creatures: with no trash file
    // in this tree its trash is its creatures alone, at every tier. What
    // its trash file makes of it is checked where the curated set lives
    // (the replay's `crates/replay/src/curated/tests.rs`).
    let ruby = r.trash(2521).unwrap().unwrap();
    assert_eq!(Some(Ok(ruby.clone())), r.trash_at(2521, Tier::Base));
    assert!(ruby.ability.is_empty() && ruby.mark.is_empty() && ruby.ground.is_empty());
    assert_eq!(ruby.npc["primal-juggernaut"].forces, Some(25));
}
