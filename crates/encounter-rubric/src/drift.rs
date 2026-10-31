//! The way the players drifted over a stretch (`read`): the bearing a
//! ground's `push` takes where it says `toward = "drift"`, one rule for the
//! replay and the editors alike.
//!
//! A wind the log never writes moves the players as it moves what lies on
//! the floor, so their drift is its only witness: during Sszorak's Howling
//! Maelstrom three gales of about 8 s blow the raid and the Caustic Residue
//! across the platform. The stretch is cut into steps of `STEP_MS` from its
//! start, each step whose end is inside it. A player's place at each end of
//! a step is read between the posts either side of it (the post itself
//! where one falls on it), only where those posts are under `GAP_MS` apart;
//! a player with no place at either end has no step. A step's raid velocity
//! is the mean of its players' step velocities, and a step whose raid
//! velocity is faster than `KNOCK_YD_S` is a knock (a cyst's pop flinging
//! the raid, the rush onto one), not the wind, and is left out. The drift
//! is the mean of the kept steps' raid velocities: its bearing (world
//! degrees, as a volley's `toward` reads them: 0 along the X the combat log
//! writes, 90 along its Y, in [0, 360)), its speed, how many steps were
//! kept of how many, and its coherence: over the kept steps, the mean
//! length of the resultant of the players' unit step vectors, those slower
//! than `STILL_YD_S` in that step left out of it (1: everyone moved one way;
//! near 0: every way). Nothing is snapped to a bearing a rubric gives.
//!
//! The posts are the caller's to choose: the replay gives the living
//! players alone (no NPC, no pet), each one's posts before their death.
//! Plain arithmetic, no serde.

/// How long each step is, ms.
pub const STEP_MS: u32 = 500;

/// How far apart (ms) the posts either side of a step's end may be, and
/// under: further apart, the player is not read there.
pub const GAP_MS: u32 = 1_500;

/// A step whose raid velocity is faster than this (yd/s) is a knock, not
/// the wind, and is left out.
pub const KNOCK_YD_S: f32 = 8.0;

/// A player slower than this (yd/s) in a step is left out of its
/// coherence: one standing still has no way.
pub const STILL_YD_S: f32 = 0.5;

/// A drift slower than this (yd/s) is none: its bearing would be noise.
const NONE_YD_S: f64 = 1e-3;

/// One place a player was posted at: when (ms, on the caller's clock), and
/// where (world yards, X and Y as the combat log writes them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Post {
    pub t: u32,
    pub x: f32,
    pub y: f32,
}

/// What the players' drift over a stretch reads (`read`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drift {
    /// Which way, world degrees in [0, 360): 0 along the log's X, 90 along
    /// its Y.
    pub bearing: f32,
    /// How fast, yd/s: the mean of the kept steps' raid velocities.
    pub speed: f32,
    /// How many steps were kept (read and no knock) …
    pub kept: usize,
    /// … of how many the stretch holds.
    pub steps: usize,
    /// How much the players moved one way, 0 to 1: over the kept steps
    /// with any player moving, the mean length of the resultant of the
    /// moving players' unit step vectors; 0 where no kept step had one.
    pub coherence: f32,
}

/// The players' drift over `from` to `until` (ms): each player's posts in
/// time order. A step read through a post that is no number is none of
/// that player's. None where no step is kept (no player read at both ends
/// of any, or every one a knock), or where the drift is about still; the
/// bearing is in [0, 360) as the f32 it is given as.
pub fn read(players: &[Vec<Post>], from: u32, until: u32) -> Option<Drift> {
    let (mut steps, mut kept) = (0usize, 0usize);
    let mut sum = [0.0f64; 2];
    let (mut coherent, mut moved) = (0.0f64, 0usize);
    let mut start = from;
    while let Some(end) = start.checked_add(STEP_MS).filter(|&e| e <= until) {
        steps += 1;
        let secs = f64::from(STEP_MS) / 1000.0;
        let velocities: Vec<[f64; 2]> = players
            .iter()
            .filter_map(|posts| {
                let a = place(posts, start)?;
                let b = place(posts, end)?;
                let v = [(b[0] - a[0]) / secs, (b[1] - a[1]) / secs];
                // A post that is no number reads no step.
                v.iter().all(|c| c.is_finite()).then_some(v)
            })
            .collect();
        start = end;
        let Some(raid) = mean(&velocities) else {
            continue;
        };
        // Written so a speed that is no number is left out too.
        let fast = raid[0].hypot(raid[1]);
        if fast.is_nan() || fast > f64::from(KNOCK_YD_S) {
            continue;
        }
        kept += 1;
        sum[0] += raid[0];
        sum[1] += raid[1];
        let ways: Vec<[f64; 2]> = velocities
            .iter()
            .filter_map(|v| {
                let speed = v[0].hypot(v[1]);
                (speed >= f64::from(STILL_YD_S)).then(|| [v[0] / speed, v[1] / speed])
            })
            .collect();
        if let Some(r) = mean(&ways) {
            coherent += r[0].hypot(r[1]);
            moved += 1;
        }
    }
    if kept == 0 {
        return None;
    }
    let drift = [sum[0] / kept as f64, sum[1] / kept as f64];
    let speed = drift[0].hypot(drift[1]);
    if speed.is_nan() || speed < NONE_YD_S {
        return None;
    }
    // A hair under 360 can round up to 360 itself as an f32 (or in
    // rem_euclid): that is 0.
    let bearing = drift[1].atan2(drift[0]).to_degrees().rem_euclid(360.0) as f32;
    Some(Drift {
        bearing: if bearing < 360.0 { bearing } else { 0.0 },
        speed: speed as f32,
        kept,
        steps,
        coherence: if moved > 0 {
            (coherent / moved as f64) as f32
        } else {
            0.0
        },
    })
}

/// The mean of some vectors; none of none.
fn mean(vs: &[[f64; 2]]) -> Option<[f64; 2]> {
    if vs.is_empty() {
        return None;
    }
    let n = vs.len() as f64;
    let s = vs
        .iter()
        .fold([0.0, 0.0], |s, v| [s[0] + v[0], s[1] + v[1]]);
    Some([s[0] / n, s[1] / n])
}

/// Where a player stood at `t`: the post at `t` itself, else read in a
/// straight line between the last post before it and the first after it
/// where those are under GAP_MS apart; none before the first post, after
/// the last, or across a wider gap.
fn place(posts: &[Post], t: u32) -> Option<[f64; 2]> {
    let i = posts.partition_point(|p| p.t <= t);
    let a = posts.get(i.checked_sub(1)?)?;
    if a.t == t {
        return Some([f64::from(a.x), f64::from(a.y)]);
    }
    let b = posts.get(i)?;
    if b.t - a.t >= GAP_MS {
        return None;
    }
    let k = f64::from(t - a.t) / f64::from(b.t - a.t);
    Some([
        f64::from(a.x) + (f64::from(b.x) - f64::from(a.x)) * k,
        f64::from(a.y) + (f64::from(b.y) - f64::from(a.y)) * k,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player posted every `every` ms from `from` to `until`, moving at
    /// `v` (yd/s) from `at`.
    fn walker(at: [f32; 2], v: [f32; 2], from: u32, until: u32, every: u32) -> Vec<Post> {
        (from..=until)
            .step_by(every as usize)
            .map(|t| {
                let s = (t - from) as f32 / 1000.0;
                Post {
                    t,
                    x: at[0] + v[0] * s,
                    y: at[1] + v[1] * s,
                }
            })
            .collect()
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// Players blown one way at 4 yd/s read that way at that speed, every
    /// step kept and every player moving with the others; the bearing is
    /// the world's, 90 along the log's Y, 225 down and back along both.
    #[test]
    fn a_steady_drift_reads_its_bearing_and_speed() {
        let players = vec![
            walker([0.0, 0.0], [0.0, 4.0], 0, 10_000, 700),
            walker([10.0, 5.0], [0.0, 4.0], 100, 10_000, 400),
            walker([-3.0, 2.0], [0.0, 4.0], 0, 10_000, 1_000),
        ];
        let d = read(&players, 1_000, 9_000).unwrap();
        assert!(near(d.bearing, 90.0), "{d:?}");
        assert!(near(d.speed, 4.0), "{d:?}");
        assert_eq!((d.kept, d.steps), (16, 16));
        assert!(near(d.coherence, 1.0), "{d:?}");
        let s = -(8.0f32).sqrt();
        let players = vec![walker([0.0, 0.0], [s, s], 0, 10_000, 500)];
        let d = read(&players, 0, 8_000).unwrap();
        assert!(near(d.bearing, 225.0), "{d:?}");
        assert!(near(d.speed, 4.0), "{d:?}");
    }

    /// A step the raid rushes through faster than KNOCK_YD_S is a knock:
    /// left out, so the drift is the wind's alone.
    #[test]
    fn a_knock_is_left_out() {
        // Along X at 3 yd/s, but flung 10 yd along Y between 2.0 and 2.5 s.
        let mut player = walker([0.0, 0.0], [3.0, 0.0], 0, 8_000, 250);
        for p in &mut player {
            if p.t >= 2_500 {
                p.y += 10.0;
            } else if p.t > 2_000 {
                p.y += 10.0 * (p.t - 2_000) as f32 / 500.0;
            }
        }
        let d = read(&[player], 0, 8_000).unwrap();
        assert_eq!((d.kept, d.steps), (15, 16));
        assert!(near(d.bearing, 0.0), "{d:?}");
        assert!(near(d.speed, 3.0), "{d:?}");
    }

    /// A player whose posts either side of a step's end are 1.5 s apart or
    /// more has no step there: the others read it alone, and a step none
    /// is read at is not kept.
    #[test]
    fn a_gap_of_a_second_and_a_half_reads_no_step() {
        // One along X, posted at 0, 1.5, 3 … s: no two posts are under the
        // gap apart, so it is read only on a post, and no step has a post
        // at both ends. Alone, no step is kept.
        let sparse = walker([0.0, 0.0], [5.0, 0.0], 0, 9_000, 1_500);
        assert_eq!(read(std::slice::from_ref(&sparse), 0, 8_000), None);
        // Beside one posted closely along Y, every step is the second's
        // alone.
        let close = walker([0.0, 0.0], [0.0, 2.0], 0, 9_000, 250);
        let d = read(&[sparse, close], 0, 8_000).unwrap();
        assert_eq!((d.kept, d.steps), (16, 16));
        assert!(near(d.bearing, 90.0), "{d:?}");
        assert!(near(d.speed, 2.0), "{d:?}");
        // Posted a little under the gap apart, it is read between them.
        let under = walker([0.0, 0.0], [5.0, 0.0], 0, 9_000, 1_499);
        let d = read(&[under], 0, 8_000).unwrap();
        assert_eq!((d.kept, d.steps), (16, 16));
        assert!(near(d.speed, 5.0), "{d:?}");
    }

    /// A post that is no number reads no step for its player: the others
    /// read the drift, never a bearing of 0 at a speed that is no number.
    #[test]
    fn a_post_that_is_no_number_reads_no_step() {
        let mut players = vec![
            walker([0.0, 0.0], [0.0, 3.0], 0, 9_000, 500),
            walker([5.0, 0.0], [0.0, 3.0], 0, 9_000, 500),
            walker([-5.0, 0.0], [0.0, 3.0], 0, 9_000, 500),
        ];
        if let Some(p) = players.get_mut(1).and_then(|p| p.get_mut(4)) {
            p.x = f32::NAN;
        }
        let d = read(&players, 0, 8_000).unwrap();
        assert!(near(d.bearing, 90.0), "{d:?}");
        assert!(near(d.speed, 3.0), "{d:?}");
        assert_eq!((d.kept, d.steps), (16, 16));
        // Every post of every player no number: nothing is read.
        for p in players.iter_mut().flatten() {
            p.y = f32::NAN;
        }
        assert_eq!(read(&players, 0, 8_000), None);
    }

    /// A drift a hair clockwise of the log's X reads 0, never 360: the
    /// bearing is held to [0, 360) as the f32 it is given as.
    #[test]
    fn a_bearing_a_hair_under_360_reads_0() {
        let players = vec![walker([0.0, 0.0], [4.0, -4e-7], 0, 9_000, 500)];
        let d = read(&players, 0, 8_000).unwrap();
        assert_eq!(d.bearing, 0.0, "{d:?}");
        assert!(near(d.speed, 4.0), "{d:?}");
    }

    /// Players standing still read no drift, and neither does a stretch no
    /// one was posted in or one shorter than a step.
    #[test]
    fn standing_still_reads_nothing() {
        let players = vec![
            walker([5.0, 5.0], [0.0, 0.0], 0, 9_000, 500),
            walker([1.0, 2.0], [0.0, 0.0], 0, 9_000, 300),
        ];
        assert_eq!(read(&players, 0, 8_000), None);
        assert_eq!(read(&players, 20_000, 28_000), None);
        let moving = vec![walker([0.0, 0.0], [1.0, 0.0], 0, 9_000, 500)];
        assert_eq!(read(&moving, 0, 499), None);
        // Each a different way: the drift is their mean, and they hardly
        // moved together.
        let crossed = vec![
            walker([0.0, 0.0], [2.0, 0.0], 0, 9_000, 500),
            walker([0.0, 0.0], [0.0, 2.0], 0, 9_000, 500),
            walker([0.0, 0.0], [-2.0, 0.0], 0, 9_000, 500),
        ];
        let d = read(&crossed, 0, 8_000).unwrap();
        assert!(near(d.bearing, 90.0), "{d:?}");
        assert!(near(d.speed, 2.0 / 3.0), "{d:?}");
        assert!(near(d.coherence, 1.0 / 3.0), "{d:?}");
    }
}
