//! A turn to propose for a room: the boss at the top, as seen from where
//! the raid stood at the pull (`room_turn`), and the map plane it is read
//! on (`plane`, `midpoint`).
//!
//! The rule (`docs/replay-assets.md` §4, "Orientation"): the bearing from
//! the raid's midpoint to the bosses' (a council's midpoint), snapped to
//! the room's four wall-square turns where its render knows its walls well
//! (the floor sidecar's `wall_angle` and `wall_share`,
//! `floor::Sidecar::wall`), else to 15°, and north up where raid and boss
//! stand too close to say. It proposes a map's `view.turn` (`View::turn`):
//! the extractor's `suggest-turns` prints it from stored pulls to seed the
//! rubric, and the map editor offers it as a suggestion. No render reads
//! it at runtime: a map's turn is the rubric's alone, and an unsaid one is
//! north up, so a room never turns from one pull to the next.

use std::f32::consts::{FRAC_PI_2, TAU};

/// The window after the pull's start (ms) a unit's first sample must lie
/// within to count toward its side's midpoint: a unit first seen later
/// came in after the pull and says nothing of where the raid stood.
pub const ROOM_TURN_WINDOW_MS: u32 = 15_000;

/// The least share of a render's wall area its wall angle must hold
/// (the floor sidecar's `wall_share`) for the turn to square with its
/// walls; under it the turn is snapped to 15° instead.
pub const ROOM_WALL_SHARE: f32 = 0.4;

/// Raid and boss closer than this (yd) on the plane give no bearing worth
/// turning by: `room_turn` answers north up. Public so a tool that
/// proposes a turn can tell "too close to say" from a north-up answer.
pub const ROOM_TURN_GUARD_YD: f32 = 3.0;

/// The turn's step where the walls do not decide it (degrees).
const STEP_DEG: f32 = 15.0;

/// World (x up the game's map, y to its left) to the map plane: u right, v down.
pub fn plane(x: f32, y: f32) -> (f32, f32) {
    (-y, -x)
}

/// The mean of points on the map plane; `None` of none.
pub fn midpoint(points: impl IntoIterator<Item = (f32, f32)>) -> Option<(f32, f32)> {
    let mut n = 0usize;
    let (su, sv) = points.into_iter().fold((0.0, 0.0), |a, p| {
        n += 1;
        (a.0 + p.0, a.1 + p.1)
    });
    let n = n as f32;
    (n > 0.0).then(|| (su / n, sv / n))
}

/// The turn to propose for a room, in radians in [0, 2π): how far the
/// view turns the world clockwise on screen to bring the boss above the
/// raid; 0 is north up, the game's +X at the top.
///
/// `raid` and `boss` are the two sides' midpoints on the map plane
/// (`plane`, `midpoint`; a council's bosses are one side), and `wall` the
/// render's walls as (angle in degrees, share): the floor sidecar's
/// `wall_angle` and `wall_share`. Within 3 yd of each other the two give
/// 0. Where the share is at least `ROOM_WALL_SHARE` the turn is the
/// nearest of the four wall-square turns (the angle and its quarter
/// turns), else the bearing to the nearest 15°. A caller with no sample
/// on either side has no midpoint to give and proposes none.
pub fn room_turn(raid: (f32, f32), boss: (f32, f32), wall: Option<(f32, f32)>) -> f32 {
    let (du, dv) = (boss.0 - raid.0, boss.1 - raid.1);
    if du.hypot(dv) < ROOM_TURN_GUARD_YD {
        return 0.0;
    }
    // The view turns the world clockwise by `a`: a direction at screen angle
    // φ (y down) lands at φ + a, and the top is −90°.
    let bearing = -FRAC_PI_2 - dv.atan2(du);
    let snapped = match wall {
        Some((angle, share)) if share >= ROOM_WALL_SHARE => {
            let base = angle.to_radians();
            (0..4)
                .map(|k| base + k as f32 * FRAC_PI_2)
                .min_by(|a, b| {
                    let d = |x: f32| {
                        let r = norm_turn(x - bearing);
                        r.min(TAU - r)
                    };
                    d(*a).total_cmp(&d(*b))
                })
                .unwrap_or(0.0)
        }
        _ => {
            let step = STEP_DEG.to_radians();
            (bearing / step).round() * step
        }
    };
    norm_turn(snapped)
}

/// An angle in [0, 2π).
fn norm_turn(a: f32) -> f32 {
    a.rem_euclid(TAU)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::{ROOM_WALL_SHARE, midpoint, plane, room_turn};

    /// The raid's midpoint in every test.
    const RAID: (f32, f32) = (40.0, -12.0);

    /// The boss 20 yd from `RAID` at `deg` clockwise of straight up the
    /// plane: the bearing a turn of `deg` brings to the top.
    fn boss_at(deg: f32) -> (f32, f32) {
        let phi = (-90.0 - deg).to_radians();
        (RAID.0 + 20.0 * phi.cos(), RAID.1 + 20.0 * phi.sin())
    }

    fn near(a: f32, deg: f32) -> bool {
        (a - deg.to_radians()).abs() < 1e-4
    }

    /// The boss straight up the plane from the raid, no walls: north up.
    #[test]
    fn the_boss_straight_up_is_north_up() {
        assert_eq!(room_turn(RAID, (RAID.0, RAID.1 - 30.0), None), 0.0);
        assert!(near(room_turn(RAID, boss_at(0.0), None), 0.0));
    }

    /// The boss to the raid's right on the plane (screen angle 0) is
    /// brought to the top (−90°) by a turn of 270°.
    #[test]
    fn the_boss_to_the_right_turns_three_quarters() {
        let a = room_turn(RAID, (RAID.0 + 10.0, RAID.1), None);
        assert!(near(a, 270.0), "{}", a.to_degrees());
        assert!((a - 3.0 * std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        // To the left: a quarter turn.
        assert!(near(room_turn(RAID, (RAID.0 - 10.0, RAID.1), None), 90.0));
    }

    /// Raid and boss within 3 yd give north up, walls or none; just past
    /// it the bearing counts.
    #[test]
    fn raid_and_boss_within_three_yards_are_north_up() {
        assert_eq!(room_turn(RAID, (RAID.0 + 2.9, RAID.1), None), 0.0);
        assert_eq!(
            room_turn(RAID, (RAID.0 + 2.9, RAID.1), Some((50.5, 0.9))),
            0.0
        );
        assert_eq!(room_turn(RAID, RAID, None), 0.0);
        assert!(near(room_turn(RAID, (RAID.0 + 3.1, RAID.1), None), 270.0));
    }

    /// Without walls the bearing is rounded to 15°: 7.5° is the line.
    #[test]
    fn a_bearing_rounds_to_fifteen_degrees() {
        for deg in [7.0, 7.4] {
            assert!(near(room_turn(RAID, boss_at(deg), None), 0.0), "{deg}");
        }
        for deg in [7.6, 8.0, 15.0, 22.0] {
            assert!(near(room_turn(RAID, boss_at(deg), None), 15.0), "{deg}");
        }
        assert!(near(room_turn(RAID, boss_at(23.0), None), 30.0));
    }

    /// At a share of 0.4 the turn squares with the walls, the nearest of
    /// the four; at 0.39 it falls back to 15° steps.
    #[test]
    fn walls_known_well_square_the_turn() {
        assert_eq!(ROOM_WALL_SHARE, 0.4);
        let well = Some((50.5, 0.4));
        let boss = boss_at(130.0);
        assert!(near(room_turn(RAID, boss, well), 140.5));
        assert!(near(room_turn(RAID, boss, Some((50.5, 0.39))), 135.0));
        // The nearest wall-square turn may be the angle itself, or wrap.
        assert!(near(room_turn(RAID, boss_at(60.0), well), 50.5));
        assert!(near(room_turn(RAID, boss_at(-30.0), well), 320.5));
    }

    /// A bearing left of straight up is a turn short of a whole one, in
    /// [0, 2π) like every other.
    #[test]
    fn a_turn_is_in_zero_to_a_whole_turn() {
        assert!(near(room_turn(RAID, boss_at(-20.0), None), 345.0));
        for deg in (-180..=180).step_by(5) {
            let deg = deg as f32 + 0.3;
            for wall in [None, Some((50.5, 0.4)), Some((0.0, 1.0))] {
                let a = room_turn(RAID, boss_at(deg), wall);
                assert!((0.0..TAU).contains(&a), "{deg} {wall:?} gave {a}");
            }
        }
    }

    /// The mean of the points, and none of none.
    #[test]
    fn the_midpoint_is_the_mean() {
        assert_eq!(midpoint([]), None);
        assert_eq!(midpoint([(1.0, 2.0)]), Some((1.0, 2.0)));
        assert_eq!(
            midpoint([(0.0, 0.0), (4.0, -2.0), (2.0, 8.0)]),
            Some((2.0, 2.0))
        );
    }

    /// The game's +X is up the plane (v falls) and its +Y to the left
    /// (u falls).
    #[test]
    fn the_plane_turns_the_world() {
        assert_eq!(plane(10.0, 0.0), (0.0, -10.0));
        assert_eq!(plane(0.0, 10.0), (-10.0, 0.0));
        assert_eq!(plane(3.0, -4.0), (4.0, -3.0));
    }
}
