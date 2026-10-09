//! v45 (R29): the replay cut's daemon side — the encounter rubric's
//! generated placed-spells table behind core's [`PlacedTable`] (core cannot
//! name the rubric), and a segment's text cut with it. The history store
//! writes what this makes as its replay tier (`proto::replay`).

use std::collections::HashSet;
use std::sync::OnceLock;

use wowdps_core::index::SegmentText;
use wowdps_core::meter::Meter;
use wowdps_core::model::replay::Cut;
use wowdps_core::replay::{self, PlacedTable};
use wowdps_encounter_rubric::placed;

/// The rubric's `placed` table: which spells place something in the room
/// (`placed::placed`) and which tell where one stands (`placed::tells`),
/// the second read once into a set — the cut asks it of every aura and
/// heal line.
pub struct RubricPlaced {
    tells: HashSet<u32>,
}

impl RubricPlaced {
    /// The table this build embeds, read once.
    pub fn get() -> &'static RubricPlaced {
        static TABLE: OnceLock<RubricPlaced> = OnceLock::new();
        TABLE.get_or_init(|| RubricPlaced {
            tells: placed::all().iter().flat_map(|p| p.telling()).collect(),
        })
    }
}

impl PlacedTable for RubricPlaced {
    fn places(&self, spell: u32) -> bool {
        placed::placed(spell).is_some()
    }

    fn tells(&self, spell: u32) -> bool {
        self.tells.contains(&spell)
    }
}

/// One segment's cut, over its seed lines and its slice, `owner` its you.
pub fn cut_text(text: &SegmentText, owner: Option<&str>) -> Cut {
    replay::cut(text.seeds(), text.slice(), RubricPlaced::get(), owner)
}

/// One segment's meter (`SegmentText::meter`'s) and its cut, from one parse
/// of its lines.
pub fn meter_and_cut(text: &SegmentText, owner: Option<&str>) -> (Meter, Cut) {
    replay::cut_and_meter(text.seeds(), text.slice(), RubricPlaced::get(), owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The set agrees with the rubric's own question, spell for spell.
    #[test]
    fn the_table_answers_as_the_rubric_does() {
        let t = RubricPlaced::get();
        for p in placed::all() {
            assert!(t.places(p.spell), "{}", p.name);
            for id in p.telling() {
                assert!(t.tells(id) && placed::tells(id), "{id}");
            }
        }
        assert!(!t.places(133) && !t.tells(133), "Fireball places nothing");
    }
}
