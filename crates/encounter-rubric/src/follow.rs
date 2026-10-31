//! A line or a cone that follows (`Ground::follows`): the way it points
//! each moment from its spot, and whom an aura names for it to follow, the
//! one reading for the replay and the editors.
//!
//! A ground that follows `"target"` or `{ aura = N }` points from its spot
//! toward its unit as that unit is drawn at the moment (`aim`): where the
//! unit stands within `NEAR_YD` of the spot there is no way from the one
//! to the other, so it points as it last did, toward the latest of the
//! unit's earlier posts off the spot; before any, as its unit faced when
//! its trigger fired. One that follows `"facing"` reads its unit's facing
//! as posted, which needs no reading here. Where the log stops posting the
//! unit, the reader draws it at its last post, so the ground holds the way
//! it last pointed.
//!
//! Nymrissa's Water Jet is cast at the tank, but the log's cast row names
//! no target: the 6 s aura it puts on the tank goes on with it, 0 to 14 ms
//! after the cast on the Mythic kill's jets, and names them
//! (`carrier`).

/// How near its spot (yd) the unit a ground follows has no bearing from
/// it: a point has no way to itself.
pub const NEAR_YD: f32 = 0.1;

/// How long after a ground's firing (ms) the aura its `{ aura = N }` names
/// may go on and name whom it follows: the aura a cast puts on its target
/// goes on with it, a few milliseconds after.
pub const AURA_WITHIN_MS: u32 = 1_000;

/// The way from `spot` toward `to` (world yd), radians as the log writes a
/// facing (0 along its X, a quarter turn along its Y): none where the two
/// lie within `NEAR_YD`, or either is no number.
pub fn bearing(spot: [f32; 2], to: [f32; 2]) -> Option<f32> {
    let (dx, dy) = (to[0] - spot[0], to[1] - spot[1]);
    let b = dy.atan2(dx);
    (dx.hypot(dy) > NEAR_YD && b.is_finite()).then_some(b)
}

/// The way a ground that follows a unit points from `spot` at a moment:
/// toward `now`, where the unit is drawn then, where it stands off the
/// spot (`bearing`); else toward the first of `before` that does (the
/// unit's earlier posts since the ground formed, the latest first); else
/// `formed`, the way its unit faced as its trigger fired. None for `now`
/// is a unit not yet posted.
pub fn aim(
    spot: [f32; 2],
    now: Option<[f32; 2]>,
    before: impl IntoIterator<Item = [f32; 2]>,
    formed: f32,
) -> f32 {
    now.and_then(|p| bearing(spot, p))
        .or_else(|| before.into_iter().find_map(|p| bearing(spot, p)))
        .unwrap_or(formed)
}

/// One of an aura's rows on a unit, in time order: when (ms), on whom, and
/// whether it went on (true) or came off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraRow {
    pub t: u32,
    pub unit: usize,
    pub on: bool,
}

/// Whom a ground that follows `{ aura = N }` follows, its trigger firing
/// at `t`, by the aura's rows on the players (`rows`, in time order; the
/// log's hostile auras on players, the replay's and the editors' alike):
/// the first unit it goes on at or after `t`, within `AURA_WITHIN_MS`;
/// else, of those carrying it at `t` (on at or before it, not off since),
/// the latest to take it. None where neither is. The ground keeps that
/// unit for as long as it stands: the aura going on another later is not
/// followed.
pub fn carrier(rows: &[AuraRow], t: u32) -> Option<usize> {
    let next = rows
        .iter()
        .find(|r| r.on && r.t >= t && r.t - t <= AURA_WITHIN_MS)
        .map(|r| r.unit);
    next.or_else(|| {
        // Each carrier at `t` and when it took the aura.
        let mut on: Vec<(usize, u32)> = Vec::new();
        for r in rows.iter().take_while(|r| r.t <= t) {
            on.retain(|&(u, _)| u != r.unit);
            if r.on {
                on.push((r.unit, r.t));
            }
        }
        on.iter().max_by_key(|&&(_, at)| at).map(|&(u, _)| u)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::f32::consts::{FRAC_PI_2, PI};

    /// The way from a spot to a unit, as a facing is written; none to the
    /// spot itself.
    #[test]
    fn a_bearing_runs_from_the_spot_to_the_unit() {
        assert_eq!(bearing([0.0, 0.0], [5.0, 0.0]), Some(0.0));
        assert_eq!(bearing([1.0, 1.0], [1.0, 4.0]), Some(FRAC_PI_2));
        assert_eq!(bearing([0.0, 0.0], [-2.0, 0.0]), Some(PI));
        assert_eq!(bearing([0.0, 0.0], [0.05, 0.05]), None);
        assert_eq!(bearing([0.0, 0.0], [f32::NAN, 1.0]), None);
    }

    /// A unit off the spot is pointed at; one on it, or not posted, is
    /// pointed at where it last stood off it, else the way it formed.
    #[test]
    fn a_unit_on_the_spot_keeps_the_way_it_last_pointed() {
        let spot = [0.0, 0.0];
        assert_eq!(aim(spot, Some([0.0, 3.0]), [], 1.0), FRAC_PI_2);
        let before = [[0.0, 0.0], [-3.0, 0.0], [0.0, 3.0]];
        assert_eq!(aim(spot, Some([0.0, 0.0]), before, 1.0), PI);
        assert_eq!(aim(spot, None, before, 1.0), PI);
        assert_eq!(aim(spot, Some([0.0, 0.01]), [[0.0, 0.0]], 1.0), 1.0);
        assert_eq!(aim(spot, None, [], 1.0), 1.0);
    }

    /// The aura's carrier: the first it goes on after the firing, within a
    /// second; else the latest to take it still carrying it; else none.
    #[test]
    fn an_aura_names_whom_it_goes_on_with_the_firing() {
        let row = |t: u32, unit: usize, on: bool| AuraRow { t, unit, on };
        let rows = [
            row(1_000, 4, true),
            row(2_000, 5, true),
            row(3_000, 5, false),
            row(10_012, 7, true),
            row(10_020, 8, true),
        ];
        // On 12 ms after the firing; the one on at it is passed over.
        assert_eq!(carrier(&rows, 10_000), Some(7));
        assert_eq!(carrier(&rows, 10_012), Some(7));
        // None goes on within a second: the one still carrying it.
        assert_eq!(carrier(&rows, 5_000), Some(4));
        // Of two carrying it, the later to take it; off again, not it.
        assert_eq!(carrier(&rows, 2_500), Some(5));
        assert_eq!(carrier(&rows, 3_000), Some(4));
        // Too soon after for the next, too late for the first.
        assert_eq!(carrier(&rows[3..], 8_000), None);
        assert_eq!(carrier(&[], 1_000), None);
    }
}
