//! v45 (R29): the replay cut's daemon side — proto's generated
//! placed-spells table (`proto::placed`) behind core's [`PlacedTable`]
//! (core names model alone), and a segment's text cut with it. The history
//! store writes what this makes as its replay tier (`proto::replay`).

use std::collections::HashSet;
use std::sync::OnceLock;

use wowdps_core::index::SegmentText;
use wowdps_core::meter::Meter;
use wowdps_core::model::replay::Cut;
use wowdps_core::replay::{self, PlacedTable};
use wowdps_proto::placed;

/// Proto's `placed` table: which spells place something in the room
/// (`placed::placed`) and which tell where one stands (`placed::tells`),
/// the second read once into a set — the cut asks it of every aura and
/// heal line.
pub struct PlacedSpells {
    tells: HashSet<u32>,
}

impl PlacedSpells {
    /// The table this build embeds, read once.
    pub fn get() -> &'static PlacedSpells {
        static TABLE: OnceLock<PlacedSpells> = OnceLock::new();
        TABLE.get_or_init(|| PlacedSpells {
            tells: placed::all().iter().flat_map(|p| p.telling()).collect(),
        })
    }
}

impl PlacedTable for PlacedSpells {
    fn places(&self, spell: u32) -> bool {
        placed::placed(spell).is_some()
    }

    fn tells(&self, spell: u32) -> bool {
        self.tells.contains(&spell)
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

    /// The set agrees with the table's own question, spell for spell.
    #[test]
    fn the_set_answers_as_the_table_does() {
        let t = PlacedSpells::get();
        for p in placed::all() {
            assert!(t.places(p.spell), "{}", p.name);
            for id in p.telling() {
                assert!(t.tells(id) && placed::tells(id), "{id}");
            }
        }
        assert!(!t.places(133) && !t.tells(133), "Fireball places nothing");
    }
}
