//! The ribbon's arithmetic (R25, v35; the prototype's `.ribbon`): the
//! stretch it spans, the raid's rate as the curve draws it, and what a
//! skull and the peak say. The daemon's series is 1 s buckets
//! (`RaidTimeline`); the curve is drawn from 10 s rates, as the prototype
//! draws it — finer in a short fight, so a 30 s pull still has a shape.
//! Each GUI draws it.

use wowdps_model::fmt::duration;
use wowdps_model::{RaidDeath, RaidTimeline};

/// The curve's rate is taken over this many of the daemon's 1 s buckets
/// (the prototype's 10 s), fewer in a fight too short to show a shape.
pub const STEP_BUCKETS: usize = 10;
/// The fewest points a fight's curve is drawn through.
pub const MIN_POINTS: usize = 12;

/// The stretch the ribbon spans, ms from the fight's start: the fight's
/// clock, or further when the series, a death or a lust runs past it — a
/// visit's Σ is timed by its members' combat but drawn on the visit's wall
/// clock. At least 1, at most `u32::MAX`.
pub fn span_of(raid: &RaidTimeline, duration_ms: i64) -> i64 {
    let bucket = i64::from(raid.bucket_ms.max(1));
    [
        duration_ms,
        raid.series.len() as i64 * bucket,
        raid.deaths.iter().map(|d| d.at_ms).max().unwrap_or(0),
        raid.lust
            .iter()
            .map(|w| w.at_ms + w.dur_ms)
            .max()
            .unwrap_or(0),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
    .clamp(1, i64::from(u32::MAX))
}

/// The raid's rate from the series: 10 s at a time ([`STEP_BUCKETS`] of
/// the live 1 s buckets, fewer in a short fight; a stored pull's coarse
/// 10 s buckets one each), each over the buckets it holds — the last one
/// may hold fewer — in amount per second. `(step_ms, rates)`.
pub fn rates(series: &[u64], bucket_ms: u32, span_ms: i64) -> (u32, Vec<f64>) {
    let buckets = (span_ms / i64::from(bucket_ms)).max(1) as usize;
    let most = (STEP_BUCKETS * 1000 / bucket_ms.max(1) as usize).max(1);
    let per = (buckets / MIN_POINTS).clamp(1, most);
    let secs = f64::from(bucket_ms) / 1000.0;
    let rate = series
        .chunks(per)
        .map(|c| c.iter().sum::<u64>() as f64 / (c.len() as f64 * secs))
        .collect();
    (bucket_ms * per as u32, rate)
}

/// What the curve is called: "Raid dps", "Raid hps", "Raid dtps".
pub fn word(raid: &RaidTimeline) -> String {
    format!("Raid {}", crate::labels::rate_label(raid.view))
}

/// "Raid dps, peak 10.7M".
pub fn peak_words(word: &str, peak: f64) -> String {
    format!(
        "{word}, peak {}",
        crate::table::figure(peak.max(0.0).round() as u64)
    )
}

/// What a skull says of its death after the name: "died 1:10, Venom
/// Rupture" — and an arena's other team says so first — worded as the
/// Deaths table words the blow (a cheat death "ran out").
pub fn skull_words(d: &RaidDeath, hide_realms: bool) -> String {
    let blow = crate::deaths::words(d, hide_realms).blow;
    if d.enemy {
        format!("of the enemy team, died {}, {blow}", duration(d.at_ms))
    } else {
        format!("died {}, {blow}", duration(d.at_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 s steps over a long fight, finer over a short one; the last step
    /// is over the buckets it holds.
    #[test]
    fn the_rate_is_ten_seconds_at_a_time_and_finer_when_short() {
        let series = vec![100; 300];
        let (step, rate) = rates(&series, 1000, 300_000);
        assert_eq!(step, 10_000);
        assert_eq!(rate.len(), 30);
        assert!(rate.iter().all(|r| (*r - 100.0).abs() < 1e-9));
        let (step, rate) = rates(&[100; 30], 1000, 30_000);
        assert_eq!(step, 2_000, "a 30 s pull keeps a shape");
        assert_eq!(rate.len(), 15);
        let (_, rate) = rates(&[100; 25], 1000, 250_000);
        assert_eq!(
            rate.last().copied(),
            Some(100.0),
            "the last step's own buckets"
        );
    }
}
