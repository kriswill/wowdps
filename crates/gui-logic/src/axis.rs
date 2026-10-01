//! A time axis's ticks: where the minute marks under a graph go, by how
//! wide the graph is drawn — the inspector's plot and the ribbon both step
//! up from minutes when minutes would crowd them.

/// The least room a tick's words get, in pixels: ticks closer than this
/// step up to the next wider interval.
pub const TICK_MIN_PX: f32 = 40.0;

/// A minute, in ms.
pub const MINUTE: u32 = 60_000;

/// The axis ticks over `window` (ms), `plot_w` pixels wide: whole minutes
/// while they have the room ([`TICK_MIN_PX`] apart), else the next wider
/// step — 2, 5, 10, 15, 30 minutes — and, zoomed into less than a few
/// minutes, halves, quarters and on down to 5 s.
pub fn ticks(window: (u32, u32), plot_w: f32) -> Vec<u32> {
    const STEPS: [u32; 12] = [
        5_000, 10_000, 15_000, 30_000, 60_000, 120_000, 300_000, 600_000, 900_000, 1_800_000,
        3_600_000, 7_200_000,
    ];
    let (lo, hi) = window;
    let span = hi.saturating_sub(lo).max(1) as f32;
    let step = STEPS
        .iter()
        .copied()
        .find(|s| span / *s as f32 * TICK_MIN_PX <= plot_w.max(1.0))
        .unwrap_or(7_200_000);
    let mut out = Vec::new();
    let mut t = lo.div_ceil(step) * step;
    while t <= hi {
        out.push(t);
        t += step;
    }
    out
}

/// The ribbon's ticks over `span_ms`, `w` wide: whole minutes, as the
/// prototype's `axisTicks()` steps them — [`ticks`]' wider steps when
/// minutes would crowd the axis, and its finer ones only in a pull too
/// short to hold two minutes, which would otherwise read no time at all.
pub fn minute_ticks(span_ms: u32, w: f32) -> Vec<u32> {
    let ticks = ticks((0, span_ms), w);
    let fine = ticks.get(1).is_some_and(|t| *t < MINUTE);
    if !fine || span_ms < 2 * MINUTE {
        return ticks;
    }
    (0..=span_ms).step_by(MINUTE as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minutes while they have the room, wider steps when they crowd, and
    /// finer ones zoomed in.
    #[test]
    fn ticks_step_by_the_room_they_have() {
        assert_eq!(
            ticks((0, 300_000), 200.0),
            [0, 60_000, 120_000, 180_000, 240_000, 300_000]
        );
        // The same five minutes in 1000 px: room for 15 s steps.
        assert_eq!(ticks((0, 300_000), 1000.0).get(1), Some(&15_000));
        // 20 minutes in 200 px: five-minute steps.
        assert_eq!(
            ticks((0, 1_200_000), 200.0),
            [0, 300_000, 600_000, 900_000, 1_200_000]
        );
        // Zoomed into 40 s: 5 s steps.
        assert_eq!(ticks((10_000, 50_000), 400.0).first(), Some(&10_000));
        assert_eq!(ticks((10_000, 50_000), 400.0).get(1), Some(&15_000));
    }

    /// The ribbon keeps whole minutes over a pull long enough for two,
    /// and a short one its finer steps.
    #[test]
    fn the_ribbon_keeps_minutes_unless_the_pull_is_short() {
        let long = minute_ticks(7 * MINUTE, 2000.0);
        assert!(long.iter().all(|t| t % MINUTE == 0), "{long:?}");
        let short = minute_ticks(50_000, 600.0);
        assert!(
            short.len() > 1 && short.iter().any(|t| t % MINUTE != 0),
            "{short:?}"
        );
    }
}
