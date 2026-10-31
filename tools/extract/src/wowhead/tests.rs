use super::*;
use wowdps_core::model::replay::{Head, Unit};

#[test]
fn a_listing_number_is_the_files_word() {
    let words: Vec<_> = (0..=5).map(listing_class).collect();
    assert_eq!(
        words,
        [
            Some("normal"),
            Some("elite"),
            Some("rareelite"),
            Some("worldboss"),
            Some("rare"),
            None
        ]
    );
    for w in words.into_iter().flatten() {
        assert!(creatures::CLASSIFICATIONS.contains(&w), "{w}");
    }
}

#[test]
fn a_tooltip_rank_is_the_files_word() {
    for (word, class) in [
        ("Normal", "normal"),
        ("Elite", "elite"),
        ("Rare Elite", "rareelite"),
        ("Boss", "worldboss"),
        ("Rare", "rare"),
    ] {
        assert_eq!(tooltip_class(word), Some(class));
        assert!(creatures::CLASSIFICATIONS.contains(&class));
    }
    assert_eq!(tooltip_class("Minion"), None);
    assert_eq!(tooltip_class("elite"), None);
}

/// A tooltip as the nether host answers, cut down.
const TIP: &str = r#"{"name":"Granokk","tooltip":"<table>\n<tr><td><b class=\"q\">Granokk</b></td></tr>\n<tr><td>&lt;Keeper (of Stones)&gt;</td></tr><tr><td>Level 52 Elemental (Rare Elite)</td></tr><tr><td>Elemental</td></tr></table>\n","map":{"zone":1}}"#;

#[test]
fn a_tooltip_is_read_by_its_type_lines_rank() {
    assert_eq!(
        tooltip(TIP).unwrap(),
        Tip {
            name: "Granokk".into(),
            class: Some("rareelite"),
        }
    );
    // The first ranked cell wins; a cell's tags are left out of its text.
    assert_eq!(
        tooltip_rank("<tr><td>Undead <span>(Boss)</span></td></tr><tr><td>Beast (Rare)</td></tr>"),
        Some("worldboss")
    );
    assert_eq!(tooltip_rank("<td>Dragonkin (Normal)</td>"), Some("normal"));
    assert_eq!(tooltip_rank("<td>Level 30 Beast (Rare)</td>"), Some("rare"));
    // A title's parentheses are no rank.
    assert_eq!(
        tooltip_rank("<td>&lt;Keeper (of Stones)&gt;</td><td>Beast</td>"),
        None
    );
}

#[test]
fn a_tooltip_without_a_rank_is_unclassified_and_one_without_a_name_fails() {
    let tip =
        tooltip(r#"{"name":"Lugear Mouse","tooltip":"<table><tr><td>Critter</td></tr></table>"}"#)
            .unwrap();
    // Wowhead writes `(Normal)`; no rank at all is no answer.
    assert_eq!(tip.class, None);
    assert!(tooltip(r#"{"error":"not found"}"#).is_err());
    assert!(tooltip(r#"{"name":"  ","tooltip":""}"#).is_err());
    assert!(tooltip("<html>").is_err());
}

/// A zone page, cut down: another Listview first, then the NPCs, their
/// options in JavaScript and their data in JSON (a name holding brackets
/// and an escaped quote, a null and an absent classification).
const PAGE: &str = r#"<html><head><title>Molten Core - Zone - World of Warcraft</title></head>
<script>
new Listview({template: 'item', id: 'drops', data: [{"id":1,"name":"Not an NPC","classification":2}]});
new Listview({template: 'npc', id: 'npcs', name: WH.TERMS.npcs, note: "<a href=\"\/npcs?filter=6;2717;0\">Filter<\/a>", data: [{"boss":1,"classification":1,"id":11502,"name":"Ragnaros","location":[2717]},{"classification":4,"id":7,"name":"The \"Lost\" [One]}"},{"classification":null,"id":8,"name":"Turtle"},{"id":9,"name":"Plain"},{"classification":3,"id":0,"name":"No id"}]});
new Listview({template: 'object', id: 'objects', data: []});
</script>"#;

#[test]
fn a_zone_pages_npcs_listing_is_read() {
    let (listed, truncated) = npcs(PAGE).unwrap();
    assert!(!truncated);
    assert_eq!(
        listed,
        [
            Listed {
                id: 11502,
                name: "Ragnaros".into(),
                class: Some("elite")
            },
            Listed {
                id: 7,
                name: "The \"Lost\" [One]}".into(),
                class: Some("rare")
            },
            Listed {
                id: 8,
                name: "Turtle".into(),
                class: None
            },
            Listed {
                id: 9,
                name: "Plain".into(),
                class: None
            },
        ]
    );
    assert_eq!(
        page_title(PAGE),
        Some("Molten Core - Zone - World of Warcraft")
    );
}

#[test]
fn a_cut_short_listing_says_so_and_a_page_without_one_fails() {
    let json_style = r#"new Listview({"_truncated":1,"id":"npcs","template":"npc","data":[{"id":5,"name":"A","classification":0}]})"#;
    let (listed, truncated) = npcs(json_style).unwrap();
    assert!(truncated);
    assert_eq!(listed.len(), 1);
    assert!(npcs("<html>no listing</html>").is_err());
    assert!(npcs("new Listview({id: 'npcs', data: [{\"id\":1}").is_err());
}

#[test]
fn an_array_is_cut_at_its_own_bracket() {
    let text = r#"x = [1, [2, "]"], {"a": "\"]"}] tail ]"#;
    assert_eq!(array_at(text, 4), Some(r#"[1, [2, "]"], {"a": "\"]"}]"#));
    assert_eq!(array_at("[1, 2", 0), None);
}

fn unit(kind: UnitKind, name: &str, npc: u32) -> Unit {
    Unit {
        kind,
        name: name.into(),
        guid: String::new(),
        class: None,
        spec: None,
        you: false,
        npc,
        posts: Vec::new(),
    }
}

#[test]
fn the_stores_npcs_are_its_hostile_units() {
    let mut cut = Cut {
        head: Head {
            map: 2526,
            zone: "Algeth'ar Academy".into(),
            ..Head::default()
        },
        ..Cut::default()
    };
    cut.units = vec![
        unit(UnitKind::Player, "A-Realm-US", 0),
        unit(UnitKind::Boss, "?", 100),
        unit(UnitKind::Add, "Whelp", 101),
        unit(UnitKind::Friendly, "Water Elemental", 200),
        unit(UnitKind::Friendly, "Turned", 101),
        unit(UnitKind::Add, "Unnamed", 0),
    ];
    let mut store = StoreNpcs::default();
    store.add(&cut);
    cut.units = vec![unit(UnitKind::Boss, "Overgrown Ancient", 100)];
    store.add(&cut);
    assert_eq!(
        store.ids,
        BTreeMap::from([(100, "Overgrown Ancient".into()), (101, "Whelp".into())])
    );
    assert_eq!(store.friendly, BTreeSet::from([200]));
    let map = &store.maps[&2526];
    assert_eq!((map.zone.as_str(), map.cuts), ("Algeth'ar Academy", 2));
    assert_eq!(map.ids, BTreeSet::from([100, 101]));
}

fn row(id: u32, class: &str, seen: i64, name: &str) -> Creature {
    Creature {
        id,
        classification: class.into(),
        lieutenant: false,
        seen_unix: seen,
        name: name.into(),
    }
}

#[test]
fn the_seed_lies_under_the_addons_sightings() {
    let mut table = BTreeMap::from([
        (1, row(1, "elite", 1_790_000_000, "Seen")),
        (2, row(2, "normal", 0, "Old seed")),
        (3, row(3, "rare", 1_790_000_000, "Seen, not seeded")),
    ]);
    let laid = lay_seed(
        &mut table,
        vec![
            row(1, "rare", 0, "Seen"),
            row(2, "rareelite", 0, "Old seed"),
            row(4, "worldboss", 0, "New"),
        ],
    );
    assert_eq!(
        laid,
        Laid {
            added: 1,
            refreshed: 1,
            sighted: 1,
            untouched: 1
        }
    );
    assert_eq!(table[&1].classification, "elite");
    assert_eq!(table[&2].classification, "rareelite");
    assert_eq!(table[&3].classification, "rare");
    assert_eq!(table[&4].classification, "worldboss");
    assert_eq!(
        histogram(&table),
        [
            ("normal", 0),
            ("elite", 1),
            ("rareelite", 1),
            ("rare", 1),
            ("worldboss", 1),
            ("minus", 0)
        ]
    );
    // A sighting the addon merges later wins over the seed.
    assert_eq!(
        creatures::merge(&mut table, vec![row(4, "elite", 1, "New")]),
        1
    );
    assert_eq!(table[&4].classification, "elite");
}

#[test]
fn every_zone_is_listed_once() {
    let maps: BTreeSet<u32> = ZONES.iter().map(|z| z.map).collect();
    let zones: BTreeSet<u32> = ZONES.iter().map(|z| z.zone).collect();
    assert_eq!(maps.len(), ZONES.len());
    assert_eq!(zones.len(), ZONES.len());
    assert_eq!(zone_url(2717), "https://www.wowhead.com/zone=2717");
    assert_eq!(
        tooltip_url(11502),
        "https://nether.wowhead.com/tooltip/npc/11502?dataEnv=1&locale=0"
    );
}
