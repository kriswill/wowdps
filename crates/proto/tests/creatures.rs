//! `proto::creatures` — the store's `creatures.tsv` (the wowdps addon's NPC
//! classifications). The file's bytes are a contract with the replay, so
//! the rendering is golden; the addon's SavedVariables section reads into
//! rows; the merge keeps the newest sighting and every row the addon
//! pruned; a file this build cannot read whole is refused, never guessed.

use std::collections::BTreeMap;

use wowdps_proto::creatures::{self, CLASSIFICATIONS, Creature, FILE, FORMAT};
use wowdps_proto::history::{Affiliation, addon_table};

fn creature(id: u32, classification: &str, lieutenant: bool, seen: i64, name: &str) -> Creature {
    Creature {
        id,
        classification: classification.to_string(),
        lieutenant,
        seen_unix: seen,
        name: name.to_string(),
    }
}

/// The addon's table as the game writes it (CRLF, tabs, bracketed integer
/// keys, raw UTF-8), with the players section beside it: one read of the
/// file feeds both readers.
const SAVED: &str = "\r\nWOWDPS_DATA = {\r\n\
    \t[\"schema\"] = 2,\r\n\
    \t[\"version\"] = \"0.1.0\",\r\n\
    \t[\"characters\"] = { [\"Player-1168-0A1B2C31\"] = true, },\r\n\
    \t[\"players\"] = {\r\n\
    \t\t[\"Player-1168-0A1B2C31\"] = { [\"name\"] = \"Bastión\", [\"guild\"] = \"\", [\"seen\"] = 1757000000, },\r\n\
    \t},\r\n\
    \t[\"creatures\"] = {\r\n\
    \t\t[164567] = {\r\n\
    \t\t\t[\"name\"] = \"Gloomwing Lurker\",\r\n\
    \t\t\t[\"classification\"] = \"elite\",\r\n\
    \t\t\t[\"lieutenant\"] = 0,\r\n\
    \t\t\t[\"seen\"] = 1760000000,\r\n\
    \t\t\t[\"build\"] = \"12.0.5.63906\",\r\n\
    \t\t\t[\"difficulty\"] = 8,\r\n\
    \t\t\t[\"type\"] = \"Beast\",\r\n\
    \t\t\t[\"power\"] = \"MANA\",\r\n\
    \t\t},\r\n\
    \t\t[164568] = {\r\n\
    \t\t\t[\"name\"] = \"Vexmarrow the Silvered\",\r\n\
    \t\t\t[\"classification\"] = \"elite\",\r\n\
    \t\t\t[\"lieutenant\"] = 1,\r\n\
    \t\t\t[\"seen\"] = 1760000100,\r\n\
    \t\t},\r\n\
    \t\t[164569] = {\r\n\
    \t\t\t[\"name\"] = \"Skittering Mote\",\r\n\
    \t\t\t[\"classification\"] = \"minus\",\r\n\
    \t\t\t[\"lieutenant\"] = 0,\r\n\
    \t\t\t[\"seen\"] = 1760000200,\r\n\
    \t\t},\r\n\
    \t\t[\"170000\"] = { [\"name\"] = \"Ösel\\tthe Ancient\", [\"classification\"] = \"worldboss\", [\"lieutenant\"] = true, [\"seen\"] = 1760000300, },\r\n\
    \t\t[170001] = { [\"name\"] = \"Future Mob\", [\"classification\"] = \"champion\", [\"seen\"] = 1760000400, },\r\n\
    \t\t[170002] = { [\"name\"] = \"Timeless\", [\"classification\"] = \"rare\", },\r\n\
    \t\t[0] = { [\"name\"] = \"Zero\", [\"classification\"] = \"rare\", [\"seen\"] = 1760000500, },\r\n\
    \t\t[\"Creature-0-1\"] = { [\"name\"] = \"Keyed Wrong\", [\"classification\"] = \"rare\", [\"seen\"] = 1760000600, },\r\n\
    \t\t[170003] = { [\"classification\"] = \"rareelite\", [\"seen\"] = 1760000700, },\r\n\
    \t},\r\n\
    }\r\n";

#[test]
fn the_addons_creatures_read_into_rows() {
    let mut rows = Creature::read_saved_variables(SAVED).unwrap();
    rows.sort_by_key(|c| c.id);
    assert_eq!(
        rows,
        vec![
            creature(164567, "elite", false, 1_760_000_000, "Gloomwing Lurker"),
            creature(
                164568,
                "elite",
                true,
                1_760_000_100,
                "Vexmarrow the Silvered"
            ),
            creature(164569, "minus", false, 1_760_000_200, "Skittering Mote"),
            // A string key that is a number reads as the id; a boolean
            // lieutenant reads as a number would; a tab in a name is a space.
            creature(170000, "worldboss", true, 1_760_000_300, "Ösel the Ancient"),
            // A record without a name keeps its row, nameless.
            creature(170003, "rareelite", false, 1_760_000_700, ""),
        ],
        "an unknown classification, a missing seen, id 0 and a key that is \
         not an id are skipped"
    );

    // One read of the file feeds both sections.
    let data = addon_table(SAVED).unwrap().unwrap();
    assert_eq!(Creature::from_addon_table(&data).len(), 5);
    let players = Affiliation::from_addon_table(&data, "TEST");
    assert_eq!(players.len(), 1);
    assert!(players[0].mine);
    assert_eq!(
        Affiliation::read_saved_variables(SAVED, "TEST").unwrap(),
        players
    );

    // An addon older than the section, another addon's file, an empty
    // file: nothing, not an error. A torn file is an error.
    let old = "WOWDPS_DATA = { [\"schema\"] = 1, [\"players\"] = {}, }";
    assert_eq!(Creature::read_saved_variables(old).unwrap(), Vec::new());
    assert_eq!(
        Creature::read_saved_variables("OTHER = { 1 }").unwrap(),
        Vec::new()
    );
    assert_eq!(Creature::read_saved_variables("").unwrap(), Vec::new());
    assert_eq!(addon_table("").unwrap(), None);
    assert!(Creature::read_saved_variables("WOWDPS_DATA = { [\"creatures\"] = {").is_err());
}

/// The file's bytes, golden: the header, then one row per creature id,
/// ascending, five tab-separated columns, `\n` line ends.
#[test]
fn the_file_renders_golden_and_round_trips() {
    let table: BTreeMap<u32, Creature> = [
        creature(164569, "minus", false, 1_760_000_200, "Skittering Mote"),
        creature(164567, "elite", false, 1_760_000_000, "Gloomwing Lurker"),
        creature(
            164568,
            "elite",
            true,
            1_760_000_100,
            "Vexmarrow the Silvered",
        ),
        creature(42, "rare", false, 1_700_000_000, "Line\nBreak\tName"),
    ]
    .into_iter()
    .map(|c| (c.id, c))
    .collect();
    let text = creatures::render(table.values());
    assert_eq!(
        text,
        "# wowdps creatures 1\n\
         42\trare\t0\t1700000000\tLine Break Name\n\
         164567\telite\t0\t1760000000\tGloomwing Lurker\n\
         164568\telite\t1\t1760000100\tVexmarrow the Silvered\n\
         164569\tminus\t0\t1760000200\tSkittering Mote\n"
    );
    assert_eq!(creatures::header(), "# wowdps creatures 1");
    assert_eq!((FILE, FORMAT), ("creatures.tsv", 1));
    let back = creatures::parse(&text).unwrap();
    assert_eq!(back.len(), 4);
    assert_eq!(
        back[&42].name, "Line Break Name",
        "a control character never splits a row"
    );
    assert_eq!(back[&164568], table[&164568]);
    assert_eq!(creatures::render(back.values()), text, "stable");
    // Every classification the game answers survives the trip.
    let all: BTreeMap<u32, Creature> = CLASSIFICATIONS
        .iter()
        .zip(1u32..)
        .map(|(c, id)| (id, creature(id, c, false, 1, "x")))
        .collect();
    assert_eq!(
        creatures::parse(&creatures::render(all.values())).unwrap(),
        all
    );
    // An empty table is the header alone.
    assert_eq!(
        creatures::render(BTreeMap::<u32, Creature>::new().values()),
        "# wowdps creatures 1\n"
    );
}

/// A reader takes this format and older and refuses anything else whole;
/// inside a file it reads, a row it cannot place is skipped.
#[test]
fn a_foreign_or_newer_file_is_refused_and_a_bad_row_skipped() {
    let e = creatures::parse("# wowdps creatures 2\n1\telite\t0\t1\tx\n").unwrap_err();
    assert!(e.contains("format 2"), "{e}");
    for foreign in [
        "",
        "1\telite\t0\t1\tx\n",
        "# something else\n",
        "# wowdps creatures x\n",
    ] {
        assert!(creatures::parse(foreign).is_err(), "{foreign:?}");
    }
    let text = "\u{feff}# wowdps creatures 1\r\n\
        # a comment line\r\n\
        7\telite\t1\t1760000000\tKept\textra\tcolumns\r\n\
        8\tchampion\t0\t1760000000\tUnknown classification\n\
        9\telite\t2\t1760000000\tBad lieutenant\n\
        x\telite\t0\t1760000000\tBad id\n\
        0\telite\t0\t1760000000\tZero id\n\
        10\telite\t0\n\
        11\tnormal\t0\tsoon\tBad seen\n\
        12\tnormal\t0\t1760000000\n\
        7\tnormal\t0\t1760000001\tLater row wins\n";
    let rows = creatures::parse(text).unwrap();
    assert_eq!(
        rows.into_values().collect::<Vec<_>>(),
        vec![
            creature(7, "normal", false, 1_760_000_001, "Later row wins"),
            creature(12, "normal", false, 1_760_000_000, ""),
        ]
    );
}

/// Newest seen wins per creature id; a sighting no newer changes nothing;
/// a row the incoming records lack stays (pruned in-game, not refuted).
#[test]
fn the_merge_keeps_the_newest_sighting_and_every_pruned_row() {
    let mut table: BTreeMap<u32, Creature> = BTreeMap::new();
    let changed = creatures::merge(
        &mut table,
        vec![
            creature(1, "elite", false, 100, "A"),
            creature(2, "normal", false, 100, "B"),
        ],
    );
    assert_eq!(changed, 2);
    // Older and same-second sightings change nothing.
    assert_eq!(
        creatures::merge(
            &mut table,
            vec![
                creature(1, "rare", false, 99, "A"),
                creature(2, "elite", true, 100, "B"),
            ],
        ),
        0
    );
    assert_eq!(table[&1].classification, "elite");
    assert!(!table[&2].lieutenant);
    // A newer one replaces the row whole; 2 is absent and stays; 3 is new.
    assert_eq!(
        creatures::merge(
            &mut table,
            vec![
                creature(1, "rareelite", true, 200, "A2"),
                creature(3, "minus", false, 150, "C"),
            ],
        ),
        2
    );
    assert_eq!(
        table.values().cloned().collect::<Vec<_>>(),
        vec![
            creature(1, "rareelite", true, 200, "A2"),
            creature(2, "normal", false, 100, "B"),
            creature(3, "minus", false, 150, "C"),
        ]
    );
    // Two accounts' records of one id in one merge: the newer wins
    // whichever comes first.
    let mut t: BTreeMap<u32, Creature> = BTreeMap::new();
    creatures::merge(
        &mut t,
        vec![
            creature(5, "elite", false, 300, "new"),
            creature(5, "normal", false, 200, "old"),
        ],
    );
    assert_eq!(t[&5].name, "new");
}
