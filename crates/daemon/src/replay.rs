//! v45 (R29): the replay cut's daemon side — proto's generated
//! placed-spells table (`proto::placed`) behind core's [`PlacedTable`]
//! (core names model alone), and a segment's text cut with it. The history
//! store writes what this makes as its replay tier (`proto::replay`).

use std::collections::HashSet;
use std::sync::OnceLock;

use wowdps_core::index::SegmentText;
use wowdps_core::meter::Meter;
use wowdps_core::model::Edition;
use wowdps_core::model::replay::Cut;
use wowdps_core::replay::{self, PlacedTable};
use wowdps_proto::placed;

/// Proto's `placed` tables: which spells place something in the room
/// (`placed::placed`) and which tell where one stands (`placed::tells`),
/// each edition's own, the second read once into a set per edition that
/// has a table — the cut asks it of every aura and heal line.
pub struct PlacedSpells {
    tells: Vec<(Edition, HashSet<u32>)>,
}

impl PlacedSpells {
    /// The tables this build embeds, read once.
    pub fn get() -> &'static PlacedSpells {
        static TABLE: OnceLock<PlacedSpells> = OnceLock::new();
        TABLE.get_or_init(|| PlacedSpells {
            tells: placed::editions()
                .map(|e| (e, placed::all(e).iter().flat_map(|p| p.telling()).collect()))
                .collect(),
        })
    }
}

impl PlacedTable for PlacedSpells {
    fn places(&self, edition: Edition, spell: u32) -> bool {
        placed::placed(edition, spell).is_some()
    }

    fn tells(&self, edition: Edition, spell: u32) -> bool {
        let edition = edition.counts_as();
        self.tells
            .iter()
            .find(|(e, _)| *e == edition)
            .is_some_and(|(_, set)| set.contains(&spell))
    }
}

/// One segment's cut, over its seed lines and its slice, `owner` its you.
pub fn cut_text(text: &SegmentText, owner: Option<&str>) -> Cut {
    replay::cut(text.seeds(), text.slice(), PlacedSpells::get(), owner)
}

/// One segment's meter (`SegmentText::meter`'s) and its cut, from one parse
/// of its lines.
pub fn meter_and_cut(text: &SegmentText, owner: Option<&str>) -> (Meter, Cut) {
    replay::cut_and_meter(text.seeds(), text.slice(), PlacedSpells::get(), owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The set agrees with the table's own question, spell for spell, in
    /// every edition: each edition's table answers for it (Retail's for a
    /// log naming none too), and an edition with no table answers nothing.
    #[test]
    fn the_set_answers_as_the_table_does() {
        let t = PlacedSpells::get();
        let editions = [
            Edition::Retail,
            Edition::Unknown(0),
            Edition::Forever,
            Edition::Classic(2),
            Edition::Unknown(7),
        ];
        for own in [Edition::Retail, Edition::Forever] {
            for p in placed::all(own) {
                for e in editions {
                    let has = placed::placed(e, p.spell).is_some();
                    assert_eq!(t.places(e, p.spell), has, "{} {e:?}", p.name);
                    if e.counts_as() == own {
                        assert!(has, "{} {e:?}", p.name);
                    }
                    for id in p.telling() {
                        assert_eq!(t.tells(e, id), placed::tells(e, id), "{id} {e:?}");
                    }
                }
                for id in p.telling() {
                    assert!(t.tells(own, id), "{id}");
                }
            }
        }
        // Each edition's ids are its own: Retail's gateway is no Forever
        // spell, Forever's Freezing Trap rank no Retail one, and a table-less
        // edition places neither.
        assert!(t.places(Edition::Retail, 111_771) && !t.places(Edition::Forever, 111_771));
        assert!(t.places(Edition::Forever, 1499) && !t.places(Edition::Retail, 1499));
        assert!(!t.places(Edition::Classic(2), 1499) && !t.places(Edition::Unknown(7), 111_771));
        for e in editions {
            assert!(
                !t.places(e, 133) && !t.tells(e, 133),
                "Fireball places nothing"
            );
        }
    }
}
