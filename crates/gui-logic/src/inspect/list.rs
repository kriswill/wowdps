//! The inspector's lists, as data: which columns each kind of list shows
//! on which grid, the seven hues an ability without an icon wears on its
//! lettered square, and the foe's disc — a lit sphere — as pixels. Moved
//! from the iced window's inspector.

use wowdps_model::View;

use crate::table::{Col, Grid};
use crate::theme::Color;

/// The stand-in squares' hues, for an ability the icon cache cannot draw
/// (the prototype's `HUES`): one per name, the same one every time.
pub const HUES: [Color; 7] = [
    Color::hex(0xC9844A),
    Color::hex(0x8F7BD6),
    Color::hex(0x5FA7D6),
    Color::hex(0x6FBF73),
    Color::hex(0xD6C35F),
    Color::hex(0xB98E6B),
    Color::hex(0xD07AB5),
];

/// A foe's disc (`.disc.foe{--c:#8E2C2C}`): a lit sphere of that red.
pub const FOE: Color = Color::hex(0x8E2C2C);

/// Which list, for its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A player's abilities (or what hit them): the throughput columns.
    Abilities,
    /// Targets, attackers: amount, share and hits.
    Targets,
    /// One side of a comparison: amount and share.
    Pair,
}

/// How much room a list has for its figures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Room {
    /// An inspector 440 px or narrower (`@container insp (max-width:
    /// 440px)`): the amount, the share and one more.
    Narrow,
    /// The inspector beside the meter: the prototype's columns.
    Normal,
    /// A list in the inspector widened over the stage: the rate joins
    /// them, and the view's own last word (overheal, absorbed).
    Wide,
}

impl Kind {
    /// The columns this list shows in `view` with `room` for them, and the
    /// grid they stand on: the prototype's `.t-ab` (Hits and Avg giving way
    /// in a narrow inspector), `.t-tg` and `.cmp2`, and in a widened
    /// inspector every figure a row carries. A count has no rate, crit or
    /// average; what hit a player has no crit of theirs. A column no row
    /// can fill (a rate the rows were never given) is the drawer's to drop.
    pub fn columns(self, view: View, room: Room) -> (Vec<Col>, Grid) {
        let counted = !view.is_rate();
        let narrow = room == Room::Narrow;
        match self {
            Kind::Abilities => {
                let cols = match (counted, view, room) {
                    (true, _, _) => vec![Col::Amount, Col::Pct],
                    (_, View::Taken, Room::Narrow) => vec![Col::Amount, Col::Pct],
                    (_, View::Taken, Room::Normal) => {
                        vec![Col::Amount, Col::Pct, Col::Hits, Col::Avg]
                    }
                    (_, View::Taken, Room::Wide) => vec![
                        Col::Amount,
                        Col::Rate,
                        Col::Pct,
                        Col::Hits,
                        Col::Avg,
                        Col::Absorbed,
                    ],
                    (_, _, Room::Narrow) => vec![Col::Amount, Col::Pct, Col::Crit],
                    (_, _, Room::Normal) => {
                        vec![Col::Amount, Col::Pct, Col::Hits, Col::Crit, Col::Avg]
                    }
                    (_, View::Healing, Room::Wide) => vec![
                        Col::Amount,
                        Col::Rate,
                        Col::Pct,
                        Col::Hits,
                        Col::Crit,
                        Col::Avg,
                        Col::Overheal,
                    ],
                    (_, _, Room::Wide) => vec![
                        Col::Amount,
                        Col::Rate,
                        Col::Pct,
                        Col::Hits,
                        Col::Crit,
                        Col::Avg,
                    ],
                };
                (cols, Grid::Abilities { narrow })
            }
            Kind::Targets if counted => (vec![Col::Amount, Col::Pct], Grid::Targets),
            Kind::Targets if room == Room::Wide => (
                vec![Col::Amount, Col::Rate, Col::Pct, Col::Hits],
                Grid::Targets,
            ),
            Kind::Targets => (vec![Col::Amount, Col::Pct, Col::Hits], Grid::Targets),
            Kind::Pair if counted || room != Room::Wide => {
                (vec![Col::Amount, Col::Pct], Grid::Pair)
            }
            Kind::Pair if view == View::Taken => (
                vec![Col::Amount, Col::Rate, Col::Pct, Col::Hits],
                Grid::Pair,
            ),
            Kind::Pair => (
                vec![Col::Amount, Col::Rate, Col::Pct, Col::Hits, Col::Crit],
                Grid::Pair,
            ),
        }
    }
}

/// The lettered square's hue for `name`: a stable hash of it into
/// [`HUES`].
pub fn hue(name: &str) -> Color {
    let h = name
        .chars()
        .fold(0_u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let [first, ..] = HUES;
    HUES.get(h as usize % HUES.len()).copied().unwrap_or(first)
}

/// The sphere's pixels, `n` × `n` RGBA (straight alpha).
pub fn sphere(n: u32, c: Color) -> Vec<u8> {
    let mix = |a: Color, b: Color, t: f32| Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    };
    // `color-mix(in srgb, c 55%, #fff)` and `…#000`.
    let lit = mix(Color::WHITE, c, 0.55);
    let rim = mix(Color::BLACK, c, 0.55);
    let side = n as f32;
    let radius = side / 2.0;
    // `circle at 34% 30%` reaching the farthest corner.
    let focus = (0.34 * side, 0.30 * side);
    let reach = (0.66_f32 * side).hypot(0.70 * side);
    // The inset ring is 1 css px: two of these pixels, at twice the size.
    let ring = 2.0;
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = (px - radius).hypot(py - radius);
            let cover = (radius - d + 0.5).clamp(0.0, 1.0);
            let t = ((px - focus.0).hypot(py - focus.1) / reach).clamp(0.0, 1.0);
            let mut ink = if t < 0.58 {
                mix(lit, c, t / 0.58)
            } else {
                mix(c, rim, (t - 0.58) / 0.42)
            };
            if d > radius - ring {
                ink = mix(ink, Color::BLACK, 0.4);
            }
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend([byte(ink.r), byte(ink.g), byte(ink.b), byte(cover)]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lettered squares take their hue from the name, as the
    /// prototype's `sqc` does: from the seven, and not all one of them.
    #[test]
    fn a_name_takes_a_hue_from_the_seven() {
        let names = ["Demonbolt", "Shadow Bolt", "Hand of Gul'dan", "Implosion"];
        let hues: Vec<Color> = names.iter().map(|n| hue(n)).collect();
        assert!(hues.iter().all(|h| HUES.contains(h)));
        assert!(hues.iter().any(|h| *h != hues[0]), "{hues:?}");
    }

    /// A count has no crit or average; a narrow inspector keeps three.
    #[test]
    fn each_list_has_its_columns() {
        assert_eq!(
            Kind::Abilities.columns(View::Interrupts, Room::Normal).0,
            [Col::Amount, Col::Pct]
        );
        assert_eq!(
            Kind::Abilities.columns(View::Damage, Room::Narrow).0,
            [Col::Amount, Col::Pct, Col::Crit]
        );
        assert_eq!(Kind::Targets.columns(View::Damage, Room::Normal).0.len(), 3);
        assert_eq!(
            Kind::Pair.columns(View::Healing, Room::Normal).1,
            Grid::Pair
        );
        // Widened: the rate joins every rate list, and the view's last word.
        let (wide, grid) = Kind::Abilities.columns(View::Healing, Room::Wide);
        assert_eq!(grid, Grid::Abilities { narrow: false });
        assert_eq!((wide[1], wide.last()), (Col::Rate, Some(&Col::Overheal)));
        assert_eq!(
            Kind::Abilities.columns(View::Taken, Room::Wide).0.last(),
            Some(&Col::Absorbed)
        );
        assert!(
            Kind::Targets
                .columns(View::Damage, Room::Wide)
                .0
                .contains(&Col::Rate)
        );
        assert_eq!(Kind::Pair.columns(View::Damage, Room::Wide).0.len(), 5);
        assert_eq!(
            Kind::Abilities.columns(View::Deaths, Room::Wide).0,
            [Col::Amount, Col::Pct],
            "a count is a count at any width"
        );
    }
}
