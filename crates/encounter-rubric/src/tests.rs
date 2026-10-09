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

#[test]
fn a_newer_or_unversioned_file_is_refused() {
    let newer = files(&[("s/raid/9-x.toml", "schema = 99\n")]);
    let errs = Rubric::from_files(newer).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("9-x") && e.contains("newer")),
        "{errs:?}"
    );
    let none = files(&[("s/raid/9-x.toml", "name = \"x\"\n")]);
    let errs = Rubric::from_files(none).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("schema")), "{errs:?}");
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
            nth: 2,
        })
    );
    assert_eq!(
        e.event["after-it"].on,
        Trigger::After(Clock {
            secs: 3.0,
            since: Some(Box::new(Trigger::Event("shattering".into()))),
            nth: 1,
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
            nth: 2,
        })
    );
    assert_eq!(
        m.event["cracks"].on,
        Trigger::After(Clock {
            secs: 1.0,
            since: Some(Box::new(Trigger::Start)),
            nth: 1,
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
    assert_eq!(r.tier(), Tier::Curated);
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), ulatek(None));
    let r = r.with_tier(Tier::Base);
    let base = r.encounter_at(3492, None, Tier::Base).unwrap().unwrap();
    assert_eq!(r.encounter(3492, None).unwrap().unwrap(), base);
    assert_eq!(
        r.encounter_at(3492, None, Tier::Curated).unwrap().unwrap(),
        base
    );
    assert_eq!(Tier::from_name(Tier::Base.name()), Some(Tier::Base));
    assert_eq!(Tier::from_name(Tier::Curated.name()), Some(Tier::Curated));
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
            // tuned file.
            for tier in [Tier::Base, Tier::Curated] {
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
            nth: 2,
        }),
        Trigger::Cast {
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
        Trigger::Hit { spell: 1 },
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
                icon: Some(1),
                owner: Some(s("boss")),
                shares: Some(s("boss")),
                still: true,
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
                at: vec![Trigger::Hit { spell: 1 }],
                delay: Some(1.0),
                ahead: Some(1.0),
                from: Some(Trigger::Start),
                ends: Ends::After(1.0),
                grow: Some(Grow::Stacks),
                spread: Some(1.0),
                preview: true,
                orbit: Some(Orbit {
                    around: s("place"),
                    turn: 1.0,
                }),
                seen: vec![1],
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
                from: Trigger::Hit { spell: 1 },
                to: s("boss"),
                delay: Some(1.0),
                hits: vec![1],
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
                from: Trigger::Hit { spell: 1 },
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
                delay: Some(1.0),
                every: Some(1.0),
                times: Some(2),
                alternate: true,
                holes: true,
                faint: true,
                hits: vec![1],
                destroys: vec![s("boss")],
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
                enter: vec![Trigger::Hit { spell: 1 }],
                leave: vec![Trigger::Hit { spell: 1 }],
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
    };
    e.event.insert(
        s("e2"),
        Event {
            on: Trigger::After(Clock::pull(1.0)),
            ..e.event[&s("e")].clone()
        },
    );
    e
}

/// How many entries each kind of named entry holds, the map's layers and
/// glows last. Every field is named, so a kind added to the schema does not
/// build here until it is counted or said not to be.
fn entries(e: &Encounter) -> [usize; 17] {
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
        range.len(),
        apart.len(),
        streak.len(),
        room.len(),
        reach.len(),
        event.len(),
        map.layer.len(),
        map.glow.len(),
    ]
}

#[test]
fn every_kind_of_entry_is_switched_off_and_kept_to_its_difficulties() {
    // Every entry of every kind, switched off, and switched on and kept to
    // Normal (the map's layers and glows have no `only`).
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
        assert_eq!(resolve(&off, d), [0; 17], "{d:?}");
    }
    assert_eq!(resolve(&on, None), all);
    assert_eq!(resolve(&on, Some(14)), all);
    let mut heroic = [0; 17];
    heroic[15..].copy_from_slice(&all[15..]);
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
