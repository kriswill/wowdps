//! Where a volley's kill stood, where the log never says (`place`): the
//! pick behind a volley's `place_kills`, one rule for the replay and the
//! editors alike.
//!
//! An NPC of a volley's `destroys` that dies while the volley is out was
//! cleared by it, so it stood on one of its projectiles' fronts at its
//! death: each projectile of each sending in flight then, not yet past its
//! reach, gives one candidate, `start + speed × (death − departure)` yards
//! out along its bearing from its spot. The candidates outside the room's
//! stencil are dropped, and of those left the pick is the one farthest
//! from every spot of the same NPC still standing at the death, posted or
//! placed; ties go to the earliest sending, then the lowest projectile.
//! The count, the moment and the distance are the log's; the spot is
//! picked. No grid, no table: the game's scripted spots are not the same
//! from pull to pull.
//!
//! The spots still standing are the caller's to give `place`: those of the
//! NPC's units the log posted, and of its other kills, that first acted by
//! this one's death and had not died before it. `place_latest_first`
//! places every kill of one NPC latest death first, so each kill's placed
//! spots still standing are picked before it, beside the posted ones it is
//! given. Plain arithmetic, no serde.

use crate::StencilShape;

/// One projectile's way: the spot it set out from (world yards, X and Y as
/// the combat log writes them), its bearing as a unit step along the log's
/// X and Y, and its reach: how far out from the spot its middle goes before
/// it is gone (yards; `f32::INFINITY` for one the stencil alone stops). A
/// front past its reach is none. The replay gives the yards its projectile
/// is drawn to: its path's end (its `length` past `start`, else where it
/// leaves the room) and a width more, over which it fades, as long as its
/// sending is in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Way {
    pub from: [f32; 2],
    pub step: [f32; 2],
    pub reach: f32,
}

/// One sending of a volley in flight as its kill died: when it set out
/// (ms on the pull's clock), how far out its projectiles' middles set out
/// (yards), how fast they go (yards a second) and each projectile's way.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub departs: u32,
    pub start: f32,
    pub speed: f32,
    pub ways: Vec<Way>,
}

impl Run {
    /// Where the front of its `way`-th projectile stood at `at` (ms): its
    /// middle `start + speed × (at − departs)` yards out along its bearing.
    /// `None` before it set out, once it is past its reach, or for a
    /// projectile it does not have.
    pub fn front(&self, way: usize, at: u32) -> Option<[f32; 2]> {
        let w = self.ways.get(way)?;
        let secs = at.checked_sub(self.departs)? as f32 / 1000.0;
        let d = self.start + self.speed * secs;
        (d <= w.reach).then(|| [w.from[0] + w.step[0] * d, w.from[1] + w.step[1] * d])
    }
}

/// Where a projectile's front stood at the death, inside the stencil: the
/// spot, and which sending (its place in the runs given) and which of its
/// projectiles (its place in the sending's ways) put it there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub at: [f32; 2],
    pub run: usize,
    pub way: usize,
}

/// The spot picked for a kill: where, from which sending and projectile,
/// and out of how many candidates inside the stencil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    pub at: [f32; 2],
    pub run: usize,
    pub way: usize,
    pub candidates: usize,
}

/// What `place` found for one kill: every candidate inside the stencil, in
/// the runs' order and each run's ways' order, and the pick among them;
/// none where no candidate is inside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guess {
    pub candidates: Vec<Candidate>,
    pub pick: Option<Pick>,
}

/// Whether a world point lies inside a stencil's shapes (on an edge
/// counts), as `Map::stencil_depth` reads one: everywhere with no shape.
pub fn inside(stencil: &[StencilShape], p: [f32; 2]) -> bool {
    stencil.is_empty() || stencil.iter().any(|s| s.depth(p) >= 0.0)
}

/// Where a kill dying at `death` (ms) stood: of the fronts of every
/// projectile of `runs` (the sendings in flight then) inside `stencil`
/// (none: all of the room), the one farthest from the nearest of
/// `standing` (the spots of the same NPC still standing then); with none
/// standing, or a tie, the earliest sending's (by `departs`, then its
/// place in `runs`), then its lowest projectile's. A sending that sets out
/// after the death gives none.
pub fn place(runs: &[Run], death: u32, stencil: &[StencilShape], standing: &[[f32; 2]]) -> Guess {
    let candidates: Vec<Candidate> = runs
        .iter()
        .enumerate()
        .flat_map(|(run, r)| {
            (0..r.ways.len()).filter_map(move |way| {
                let at = r.front(way, death)?;
                Some(Candidate { at, run, way })
            })
        })
        .filter(|c| inside(stencil, c.at))
        .collect();
    // How far a spot is from the nearest standing: infinitely, with none.
    let clear = |p: [f32; 2]| {
        standing
            .iter()
            .map(|s| (p[0] - s[0]).hypot(p[1] - s[1]))
            .fold(f32::INFINITY, f32::min)
    };
    let departs = |run: usize| runs.get(run).map_or(u32::MAX, |r| r.departs);
    // The earliest sending first, then its lowest projectile; the first of
    // the farthest wins.
    let mut order: Vec<&Candidate> = candidates.iter().collect();
    order.sort_by_key(|c| (departs(c.run), c.run, c.way));
    let mut best: Option<(&Candidate, f32)> = None;
    for c in order {
        let d = clear(c.at);
        if best.is_none_or(|(_, b)| d > b) {
            best = Some((c, d));
        }
    }
    let pick = best.map(|(c, _)| Pick {
        at: c.at,
        run: c.run,
        way: c.way,
        candidates: candidates.len(),
    });
    Guess { candidates, pick }
}

/// One kill to place: when it first acted and when it died (ms), and the
/// sendings in flight as it died.
#[derive(Debug, Clone, PartialEq)]
pub struct Kill {
    pub from: u32,
    pub death: u32,
    pub runs: Vec<Run>,
}

/// A spot of the NPC standing from `from` until `until` (ms): one of its
/// units the log posted (a still NPC's one spot, from its first act to its
/// death), or a kill already placed (until its death).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Standing {
    pub from: u32,
    pub until: u32,
    pub at: [f32; 2],
}

impl Standing {
    /// Whether it stands at a death at `t`: it first acted by then and had
    /// not died before it (one dying at `t` still stands).
    pub fn at_death(&self, t: u32) -> bool {
        self.from <= t && self.until >= t
    }
}

/// Every kill of one NPC placed, latest death first, so the spots still
/// standing at each death are known as it is placed: those of `posted`
/// standing then (`Standing::at_death`), and of the kills placed already
/// that stand then too. What each found, in `kills`' order. Of the kills
/// dying at one moment (a wave crossing several at once), the later in
/// `kills` is placed first and the earlier sees it standing, not the other
/// way round, so no two of them are put on one front unless every other
/// front is nearer one still standing.
pub fn place_latest_first(
    kills: &[Kill],
    posted: &[Standing],
    stencil: &[StencilShape],
) -> Vec<Guess> {
    let mut order: Vec<usize> = (0..kills.len()).collect();
    order.sort_by_key(|&k| std::cmp::Reverse(kills.get(k).map_or((0, 0), |kill| (kill.death, k))));
    let mut out: Vec<Guess> = vec![Guess::default(); kills.len()];
    // The kills placed so far, each standing from its first act to its
    // death.
    let mut placed: Vec<Standing> = Vec::new();
    for k in order {
        let Some(kill) = kills.get(k) else {
            continue;
        };
        let standing: Vec<[f32; 2]> = (posted.iter().chain(&placed))
            .filter(|s| s.at_death(kill.death))
            .map(|s| s.at)
            .collect();
        let g = place(&kill.runs, kill.death, stencil, &standing);
        if let Some(p) = g.pick {
            placed.push(Standing {
                from: kill.from,
                until: kill.death,
                at: p.at,
            });
        }
        if let Some(slot) = out.get_mut(k) {
            *slot = g;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Kill, Run, Standing, Way, inside, place, place_latest_first};
    use crate::StencilShape;

    /// A sending from `from` at `departs` ms, 2.5 yd out at 12 yd a second
    /// (Vashnik's Plague Wave), to the four points of the log's compass.
    fn wave(departs: u32, from: [f32; 2]) -> Run {
        Run {
            departs,
            start: 2.5,
            speed: 12.0,
            ways: [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]]
                .into_iter()
                .map(|step| Way {
                    from,
                    step,
                    reach: f32::INFINITY,
                })
                .collect(),
        }
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    fn square(half: f32) -> StencilShape {
        StencilShape {
            points: vec![[-half, -half], [half, -half], [half, half], [-half, half]],
            ..StencilShape::default()
        }
    }

    /// Two seconds after it set out a wave's front is 2.5 + 24 yd out on
    /// each bearing; none before it set out.
    #[test]
    fn the_fronts_stand_as_far_out_as_the_wave_went() {
        let r = wave(1_000, [10.0, -5.0]);
        assert!(near(r.front(0, 3_000).unwrap(), [36.5, -5.0]));
        assert!(near(r.front(1, 3_000).unwrap(), [10.0, 21.5]));
        assert!(near(r.front(2, 3_000).unwrap(), [-16.5, -5.0]));
        assert!(near(r.front(3, 3_000).unwrap(), [10.0, -31.5]));
        // At its departure, its start out.
        assert!(near(r.front(0, 1_000).unwrap(), [12.5, -5.0]));
        assert_eq!(r.front(0, 999), None);
        assert_eq!(r.front(4, 3_000), None);
        let g = place(&[r], 3_000, &[], &[]);
        assert_eq!(g.candidates.len(), 4);
        assert!(near(g.candidates[2].at, [-16.5, -5.0]));
    }

    /// Only the fronts inside the stencil are candidates: a square 30 yd
    /// across round the spot keeps none of a wave 26.5 yd out, a circle 30
    /// yd round it keeps all four, and no stencil keeps everything.
    #[test]
    fn only_the_fronts_inside_the_stencil_count() {
        let r = wave(0, [0.0, 0.0]);
        let small = [square(15.0)];
        assert!(inside(&small, [15.0, 0.0]), "an edge is inside");
        assert!(!inside(&small, [15.1, 0.0]));
        let g = place(std::slice::from_ref(&r), 2_000, &small, &[]);
        assert!(g.candidates.is_empty());
        assert_eq!(g.pick, None);
        let round = [StencilShape {
            center: Some([0.0, 0.0]),
            radius: Some(30.0),
            ..StencilShape::default()
        }];
        assert_eq!(
            place(std::slice::from_ref(&r), 2_000, &round, &[])
                .candidates
                .len(),
            4
        );
        // Half a room: the shapes join, and only the east and north
        // fronts lie in the half kept.
        let half = [StencilShape {
            points: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]],
            ..StencilShape::default()
        }];
        let g = place(&[r], 2_000, &half, &[]);
        let ways: Vec<usize> = g.candidates.iter().map(|c| c.way).collect();
        assert_eq!(ways, vec![0, 1]);
        assert_eq!(g.pick.unwrap().candidates, 2);
    }

    /// The pick is the candidate farthest from the nearest spot still
    /// standing: with one standing east of the spot, the west front.
    #[test]
    fn the_pick_is_the_farthest_from_what_still_stands() {
        let r = wave(0, [0.0, 0.0]);
        let g = place(std::slice::from_ref(&r), 1_000, &[], &[[14.5, 0.0]]);
        let p = g.pick.unwrap();
        assert_eq!((p.run, p.way, p.candidates), (0, 2, 4));
        assert!(near(p.at, [-14.5, 0.0]));
        // Standing east and west: north and south are as far from the
        // nearest, so the lower projectile, north (the log's +Y), wins.
        let g = place(&[r], 1_000, &[], &[[14.5, 0.0], [-14.5, 0.0]]);
        assert_eq!(g.pick.unwrap().way, 1);
    }

    /// Two sendings in flight give eight candidates; with nothing standing
    /// the earliest sending's first projectile is picked, wherever it is
    /// listed; with a spot standing, the farthest of all eight.
    #[test]
    fn ties_go_to_the_earliest_sending_then_its_lowest_projectile() {
        let later = wave(500, [100.0, 0.0]);
        let earlier = wave(0, [0.0, 0.0]);
        let runs = [later, earlier];
        let g = place(&runs, 1_000, &[], &[]);
        assert_eq!(g.candidates.len(), 8);
        let p = g.pick.unwrap();
        assert_eq!((p.run, p.way, p.candidates), (1, 0, 8));
        assert!(near(p.at, [14.5, 0.0]));
        // One standing at the earlier sending's spot: the later sending's
        // east front, 108.5 yd off, is the farthest.
        let p = place(&runs, 1_000, &[], &[[0.0, 0.0]]).pick.unwrap();
        assert_eq!((p.run, p.way), (0, 0));
        assert!(near(p.at, [108.5, 0.0]));
        // A sending that sets out after the death gives nothing.
        let g = place(&[wave(2_000, [0.0, 0.0])], 1_000, &[], &[]);
        assert!(g.candidates.is_empty() && g.pick.is_none());
    }

    /// Nothing is placed where no front is inside: no runs, or every
    /// front past the room's edge.
    #[test]
    fn nothing_is_placed_without_a_candidate_inside() {
        assert_eq!(place(&[], 1_000, &[], &[]).pick, None);
        let far = place(&[wave(0, [0.0, 0.0])], 10_000, &[square(50.0)], &[]);
        assert!(far.candidates.is_empty());
        assert_eq!(far.pick, None);
    }

    /// Kills placed latest death first: the last to die takes the earliest
    /// sending's first front; one dying before it, while it still stands,
    /// is put as far from it as its own wave allows; two dying at one
    /// moment stand for each other, so the second keeps clear of both;
    /// one that first acted after a death does not stand for it.
    #[test]
    fn kills_are_placed_from_the_last_to_die() {
        let w = |departs| vec![wave(departs, [0.0, 0.0])];
        let kill = |from, death, departs| Kill {
            from,
            death,
            runs: w(departs),
        };
        let kills = [
            // Dies at 1 s, while the second and the third stand.
            kill(0, 1_000, 0),
            // The last to die, standing since the start.
            kill(0, 5_000, 4_000),
            // Dies at 1 s too: placed before the first, it stands for it.
            kill(0, 1_000, 0),
            // First acts at 2 s and dies at 3 s: it never stood at 1 s.
            kill(2_000, 3_000, 2_000),
        ];
        let g = place_latest_first(&kills, &[], &[]);
        let at = |k: usize| g[k].pick.unwrap().at;
        // The last: east, 14.5 yd out.
        assert!(near(at(1), [14.5, 0.0]));
        // At 3 s, as far from it as its wave goes: west.
        assert!(near(at(3), [-14.5, 0.0]));
        // The two at 1 s: the later listed west, clear of the last; the
        // first clear of both, north (north and south tie; the lower
        // projectile), the one at 3 s not yet up.
        assert!(near(at(2), [-14.5, 0.0]));
        assert!(near(at(0), [0.0, 14.5]));
        // A kill with no candidate inside is placed nowhere and stands for
        // no one.
        let lost = [
            Kill {
                from: 0,
                death: 1_000,
                runs: w(0),
            },
            Kill {
                from: 0,
                death: 9_000,
                runs: w(0),
            },
        ];
        let g = place_latest_first(&lost, &[], &[square(20.0)]);
        assert_eq!(g[1].pick, None);
        assert_eq!(g[0].pick.unwrap().way, 0);
    }

    /// A projectile past its reach gives no front, whatever the stencil
    /// says: a way whose middle goes 12.5 yd (a `length` of 10 past its 2.5
    /// start) is 8.5 yd out half a second in and still there just short of
    /// its reach, and gone a second in, when it would be 14.5 yd out.
    #[test]
    fn a_projectile_past_its_reach_gives_no_front() {
        let mut r = wave(0, [0.0, 0.0]);
        for w in &mut r.ways {
            w.reach = 12.5;
        }
        assert!(near(r.front(0, 500).unwrap(), [8.5, 0.0]));
        assert!(near(r.front(0, 833).unwrap(), [12.496, 0.0]));
        assert_eq!(r.front(0, 1_000), None);
        // One way short, the others not: three candidates.
        r.ways[0].reach = f32::INFINITY;
        let g = place(std::slice::from_ref(&r), 1_000, &[], &[]);
        assert_eq!(g.candidates.len(), 1);
        assert_eq!(g.pick.unwrap().way, 0);
        r.ways[0].reach = 12.5;
        assert_eq!(place(&[r], 1_000, &[], &[]).pick, None);
    }

    /// A unit the log posted stands for every kill dying from its first act
    /// to its death, as a placed kill does: one posted east keeps the last
    /// to die off the east front; one posted only later, or dead before,
    /// keeps nothing off.
    #[test]
    fn a_posted_spot_stands_from_its_first_act_to_its_death() {
        let w = vec![wave(0, [0.0, 0.0])];
        let kills = [Kill {
            from: 0,
            death: 1_000,
            runs: w,
        }];
        let east = |from, until| Standing {
            from,
            until,
            at: [14.5, 0.0],
        };
        assert!(east(0, 1_000).at_death(1_000) && !east(0, 999).at_death(1_000));
        let g = place_latest_first(&kills, &[east(0, 5_000)], &[]);
        let p = g[0].pick.unwrap();
        assert_eq!(p.way, 2);
        assert!(near(p.at, [-14.5, 0.0]));
        for gone in [east(2_000, 5_000), east(0, 900)] {
            let g = place_latest_first(&kills, &[gone], &[]);
            assert_eq!(g[0].pick.unwrap().way, 0);
        }
    }
}
