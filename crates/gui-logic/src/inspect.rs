//! The window inspector's model, free of any GUI (plan step 3.3, wave B):
//! who is who ([`Roster`]), the graph's curves and dead spans ([`plot`]),
//! its lanes ([`lanes`]) and its stacked bands' seating ([`stack`]), its
//! curves ([`curves`]), numbers ([`nums`]) and recap words ([`recap`]), a
//! healer's mana line under it ([`power`], R28), the
//! graph's geometry ([`geometry`]) and R21's matrices ([`matrix`]), and
//! how wide it stands ([`Fit`]) —
//! moved from the iced window's inspector, so both window GUIs build the
//! same graph from the same snapshot.

use std::collections::HashMap;

use wowdps_model::{Class, Row, Spec};

pub mod curves;
pub mod geometry;
pub mod lanes;
pub mod list;
pub mod matrix;
pub mod nums;
pub mod plot;
pub mod power;
pub mod recap;
pub mod stack;

/// Everyone the window has seen on a meter, by guid: their name and class
/// — what a lane's span is coloured by when its caster is not the player
/// (the Heroism's Shaman is on the Damage rows, and perhaps not on the
/// Taken ones the inspector shows). A guid's class never changes, so the
/// roster only grows, and a class learned is never forgotten.
///
/// It answers by NAME too: a drill's target rows are keyed by name and
/// wear the DRILLED player's class (the model's "drilldown rows alike"),
/// so who a target is — a player in their colour, or a creature — is the
/// roster's to say.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    people: HashMap<String, (String, Option<Class>)>,
    names: HashMap<String, (Option<Class>, Option<Spec>)>,
}

impl Roster {
    /// Take in a meter's rows (players: never the enemies' view).
    pub fn observe(&mut self, rows: &[Row]) {
        for r in rows {
            match self.people.get_mut(&r.key) {
                Some(have) if have.1.is_none() && r.class.is_some() => have.1 = r.class,
                Some(_) => {}
                None => {
                    self.people
                        .insert(r.key.clone(), (r.label.clone(), r.class));
                }
            }
            // Every tick walks every row: a name already known is looked up,
            // and only a new one is allocated for.
            if !self.names.contains_key(&r.label) {
                self.names.insert(r.label.clone(), (None, None));
            }
            if let Some(named) = self.names.get_mut(&r.label) {
                if r.class.is_some() {
                    named.0 = r.class;
                }
                if r.spec.is_some() {
                    named.1 = r.spec;
                }
            }
        }
    }

    /// Their name ("Name-Realm") and class, when seen.
    pub fn get(&self, guid: &str) -> Option<(&str, Option<Class>)> {
        self.people.get(guid).map(|(n, c)| (n.as_str(), *c))
    }

    /// `rows` — a drill's targets, keyed by name — each wearing its OWN
    /// class and spec: a player the roster has seen in theirs, anything
    /// else none (a creature).
    pub fn as_themselves(&self, rows: &[Row]) -> Vec<Row> {
        rows.iter()
            .map(|r| {
                let (class, spec) = self.names.get(&r.label).copied().unwrap_or_default();
                Row {
                    class,
                    spec,
                    ..r.clone()
                }
            })
            .collect()
    }
}
/// The inspector's width beside the meter (`.split{grid-template-
/// columns:minmax(0,1fr) minmax(360px,520px)}`), and in a tile
/// (`minmax(320px,410px)`). The meter's `1fr` may shrink to nothing, so a
/// grid hands the column its most at every width it stands beside the
/// meter (a tile starts at 821 px, a wide window at 1181): the minimums
/// never bind, and the column is simply its maximum.
pub const WIDE: f32 = 520.0;

pub const TILE: f32 = 410.0;

/// At this width and under, an ability list keeps its amount, share and
/// crit (`@container insp (max-width: 440px)`).
pub const NARROW_LIST: f32 = 440.0;

/// How wide the window is, by the prototype's breakpoints: above 1180 px
/// the numbers stand four across, under it two; at 820 px and under the
/// inspector is pushed rather than beside the meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Wide,
    Tile,
    Narrow,
}

impl Fit {
    pub fn of(window: f32) -> Self {
        if window <= crate::theme::NARROW_WINDOW {
            Fit::Narrow
        } else if window <= crate::theme::TILE_WINDOW {
            Fit::Tile
        } else {
            Fit::Wide
        }
    }
}

/// The inspector's width beside the meter in a window `window` wide —
/// `None` at 820 px and under, where the meter is alone and the inspector
/// is pushed over it instead.
pub fn beside(window: f32) -> Option<f32> {
    match Fit::of(window) {
        Fit::Narrow => None,
        Fit::Tile => Some(TILE),
        Fit::Wide => Some(WIDE),
    }
}

/// The inspector widened over the stage (its corner button, `f`): the
/// whole width under the view tabs, the meter set aside. Its graph stands
/// taller, its numbers in one line, and its lists side by side where the
/// stage has the room — a drill's abilities beside its targets, a recap
/// beside its attackers — one over the other where it has not.
pub mod wide {
    use super::list::Room;

    /// The widened graph's plot (beside the meter: `geometry::PLOT_H`).
    pub const PLOT_H: f32 = 180.0;
    /// At this stage width and over, two lists stand side by side.
    pub const SPLIT: f32 = 1100.0;
    /// The first list's share of a split: the abilities (or the recap)
    /// carry the columns, the targets (or attackers) four.
    pub const FIRST: f32 = 0.6;
    /// A comparison's half this wide or wider shows the rate, hits and
    /// crit beside its amount and share.
    pub const PAIR_ROOMY: f32 = 520.0;
    /// The most numbers one line of the head holds.
    pub const NUMS_MAX: usize = 8;

    /// Do two lists stand side by side on a stage `stage` wide?
    pub fn split(stage: f32) -> bool {
        stage >= SPLIT
    }

    /// The room a comparison's two lists have, side by side on the stage.
    pub fn pair_room(stage: f32) -> Room {
        if stage / 2.0 >= PAIR_ROOMY {
            Room::Wide
        } else {
            Room::Normal
        }
    }

    /// The numbers to a line: all of them, between four and the most.
    pub fn per_row(nums: usize) -> usize {
        nums.clamp(4, NUMS_MAX)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_wide_stage_splits_and_a_tile_stacks() {
            // 1440 less the docked rail, and a 1000 px tile's stage.
            assert!(split(1203.0) && !split(1000.0));
            assert_eq!(pair_room(1203.0), Room::Wide);
            assert_eq!(pair_room(900.0), Room::Normal);
            assert_eq!((per_row(2), per_row(6), per_row(12)), (4, 6, NUMS_MAX));
            const { assert!(PLOT_H > super::super::geometry::PLOT_H) };
        }
    }
}
