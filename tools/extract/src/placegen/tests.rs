use super::*;
use crate::table::Csv;

fn csv(text: &str) -> Csv {
    parse_csv(text).unwrap()
}

/// The client's word on a gateway and a totem: names, durations (the
/// gateway's own cast 1 ms, its gate's summon 15 min), radii (the totem's
/// aiming circle 8 yd, its slow 10).
fn tables() -> Tables {
    let mut t = Tables::default();
    t.insert(
        "SpellName",
        csv(
            "ID,Name_lang\n111771,Demonic Gateway\n113942,Demonic Gateway\n113886,Demonic Gateway\n\
             2484,Earthbind Totem\n3600,Earthbind\n",
        ),
    );
    t.insert(
        "SpellMisc",
        csv("SpellID,DifficultyID,DurationIndex\n111771,0,1\n113886,0,2\n2484,0,3\n3600,0,4\n"),
    );
    t.insert(
        "SpellDuration",
        csv("ID,Duration\n1,1\n2,900000\n3,20000\n4,5000\n"),
    );
    t.insert(
        "SpellEffect",
        csv(
            "SpellID,DifficultyID,EffectRadiusIndex_0,EffectRadiusIndex_1\n\
             2484,0,8,8\n3600,0,0,13\n3600,16,30,30\n",
        ),
    );
    t.insert(
        "SpellRadius",
        csv("ID,Radius,RadiusMax\n8,8,8\n13,10,10\n30,30,30\n"),
    );
    // The gateway with a cooldown of its own, the totem with a charge.
    t.insert(
        "SpellCooldowns",
        csv("SpellID,DifficultyID,RecoveryTime,CategoryRecoveryTime\n111771,0,10000,0\n"),
    );
    t.insert(
        "SpellCategories",
        csv("SpellID,DifficultyID,ChargeCategory\n2484,0,77\n"),
    );
    t.insert("SpellCategory", csv("ID,ChargeRecoveryTime\n77,30000\n"));
    t
}

const GATE: Curated = entry(
    Way,
    111771,
    "Demonic Gateway",
    &[Caster, Trips(113942)],
    Gate,
    Source::None,
    Spell(113886),
);
const EARTHBIND: Curated = entry(
    Control,
    2484,
    "Earthbind Totem",
    &[Unit],
    Area,
    Spell(3600),
    Spell(2484),
);

fn census(rows: &str) -> Census {
    Census::parse(&format!("event,id,name,a.txt,b.txt\n{rows}")).unwrap()
}

#[test]
fn an_entry_takes_its_reach_and_life_from_the_spells_it_names() {
    let c = census(
        "cast,111771,Demonic Gateway,1,2\naura,113942,Demonic Gateway,5,0\n\
         summon,2484,Earthbind Totem,0,3\n",
    );
    let g = generate_curated(&[GATE, EARTHBIND], &tables(), &c, "1.2.3").unwrap();
    assert_eq!(g.spells, 2);
    // Sorted by spell; the slow's 10 yd over the totem's 8 (the Mythic
    // effect left out); the gate's 15 minutes over the cast's 1 ms.
    assert!(g.content.contains(
        "    Placed { spell: 2484, name: \"Earthbind Totem\", purpose: Control, found: &[Unit], \
         form: Area, reach: 10.0, lasts_ms: 20000, cooldown_ms: 30000, until_touched: false },\n    Placed { spell: 111771,"
    ));
    assert!(
        g.content
            .contains("found: &[Caster, Trips(113942)], form: Gate, reach: 0.0, lasts_ms: 900000, cooldown_ms: 10000,")
    );
    assert!(g.content.contains("build 1.2.3"));
    assert!(g.expected.contains("| 2484 | Earthbind Totem | Control | Unit | Area | 10.0 (3600) | 20000 (2484) | 30 | Earthbind Totem 2484: 3 cast or summon |"));
    assert!(
        g.expected
            .contains("Demonic Gateway 113942: 5 aura or heal")
    );
}

#[test]
fn a_misnamed_or_unlogged_entry_is_refused() {
    let seen = "cast,111771,Demonic Gateway,1,0\naura,113942,Demonic Gateway,1,0\n\
                summon,2484,Earthbind Totem,1,0\n";
    let wrong = entry(
        Control,
        2484,
        "Earthbind",
        &[Unit],
        Area,
        Spell(3600),
        Spell(2484),
    );
    let e = generate_curated(&[wrong], &tables(), &census(seen), "b").unwrap_err();
    assert!(e.contains("SpellName says \"Earthbind Totem\""), "{e}");

    let no_trips = "cast,111771,Demonic Gateway,1,0\n";
    let e = generate_curated(&[GATE], &tables(), &census(no_trips), "b").unwrap_err();
    assert!(
        e.contains("113942 (Demonic Gateway) is never applied"),
        "{e}"
    );

    let no_casts = "aura,113942,Demonic Gateway,1,0\n";
    let e = generate_curated(&[GATE], &tables(), &census(no_casts), "b").unwrap_err();
    assert!(e.contains("no player casts or summons it"), "{e}");

    let no_radius = entry(
        Control,
        2484,
        "Earthbind Totem",
        &[Unit],
        Area,
        Spell(111771),
        Spell(2484),
    );
    let e = generate_curated(&[no_radius], &tables(), &census(seen), "b").unwrap_err();
    assert!(e.contains("111771 has no effect radius"), "{e}");
}

#[test]
fn the_curated_list_is_one_entry_a_spell_and_its_ids_are_the_censuss() {
    let mut spells: Vec<u32> = CURATED.iter().map(|c| c.spell).collect();
    let n = spells.len();
    spells.sort_unstable();
    spells.dedup();
    assert_eq!(spells.len(), n, "a spell curated twice");
    let ids = census_ids();
    for want in ["111771", "113942", "145629", "73921"] {
        assert!(ids.lines().any(|l| l == want), "{want} in the census ids");
    }
}
