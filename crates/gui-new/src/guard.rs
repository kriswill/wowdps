//! The render guard's comparison (spec §9): a state's pixels against the
//! blessed PNG in `snapshots/<suite>/<state>.png`, with a TOLERANCE rather
//! than a hash. Spike S5 measured why: the GPU and Mesa's lavapipe draw
//! the same frame within a pixel or two of one 8-bit level, and the weekly
//! lock update moves Mesa, so exact hashes would churn.
//!
//! A pixel differs when any channel moves by more than `CHANNEL`; a state
//! passes while at most `BUDGET` of its pixels differ. `WOWDPS_BLESS=1`
//! writes the current pictures as the new blessed ones instead of
//! comparing, for an intended change; `WOWDPS_GUARD_PNG=<dir>` also saves
//! every picture taken, blessed or not, for a look.

#![expect(
    dead_code,
    reason = "the overlay's first guarded states arrive in step 2.1"
)]

use std::path::{Path, PathBuf};

use image::RgbaImage;

/// The largest per-channel move that is still the same pixel.
pub const CHANNEL: u8 = 3;

/// The share of a picture's pixels allowed to differ.
pub const BUDGET: f64 = 0.0005;

/// Where a suite's blessed pictures live.
pub fn dir(suite: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("snapshots")
        .join(suite)
}

/// How two pictures of one state differ.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub pixels: usize,
    pub total: usize,
    pub worst: u8,
}

impl Diff {
    pub fn within(&self) -> bool {
        self.pixels as f64 <= self.total as f64 * BUDGET
    }
}

/// Pixel by pixel; `None` when the sizes differ.
pub fn diff(blessed: &RgbaImage, actual: &RgbaImage) -> Option<Diff> {
    if blessed.dimensions() != actual.dimensions() {
        return None;
    }
    let mut out = Diff {
        pixels: 0,
        total: (actual.width() * actual.height()) as usize,
        worst: 0,
    };
    for (a, b) in blessed.pixels().zip(actual.pixels()) {
        let delta =
            a.0.iter()
                .zip(b.0.iter())
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
        out.worst = out.worst.max(delta);
        if delta > CHANNEL {
            out.pixels += 1;
        }
    }
    Some(out)
}

/// Check `actual` against the blessed picture of `state`, or bless it.
/// Returns what failed, for the caller to collect across states.
pub fn check(suite: &str, state: &str, actual: &RgbaImage) -> Result<(), String> {
    if let Some(dir) = std::env::var_os("WOWDPS_GUARD_PNG") {
        let path = PathBuf::from(dir).join(format!("{suite}-{state}.png"));
        actual
            .save(&path)
            .map_err(|e| format!("{state}: saving {}: {e}", path.display()))?;
    }
    let path = dir(suite).join(format!("{state}.png"));
    if std::env::var_os("WOWDPS_BLESS").is_some() {
        std::fs::create_dir_all(dir(suite)).map_err(|e| format!("{state}: {e}"))?;
        return actual
            .save(&path)
            .map_err(|e| format!("{state}: blessing {}: {e}", path.display()));
    }
    let blessed = image::open(&path)
        .map_err(|e| {
            format!(
                "{state}: no blessed picture at {} ({e}); run with WOWDPS_BLESS=1",
                path.display()
            )
        })?
        .into_rgba8();
    match diff(&blessed, actual) {
        None => Err(format!(
            "{state}: {}x{} now, {}x{} blessed",
            actual.width(),
            actual.height(),
            blessed.width(),
            blessed.height()
        )),
        Some(d) if d.within() => Ok(()),
        Some(d) => Err(format!(
            "{state}: {} of {} pixels moved past {CHANNEL} (worst {}); budget {:.0}",
            d.pixels,
            d.total,
            d.worst,
            d.total as f64 * BUDGET
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(v: u8) -> RgbaImage {
        RgbaImage::from_pixel(100, 100, image::Rgba([v, v, v, 255]))
    }

    #[test]
    fn a_level_or_two_is_the_same_picture_and_a_stroke_is_not() {
        let base = flat(100);
        let near = flat(102);
        assert_eq!(
            diff(&base, &near).map(|d| d.pixels),
            Some(0),
            "within the channel"
        );
        let mut stroke = base.clone();
        for x in 0..100 {
            stroke.put_pixel(x, 50, image::Rgba([255, 255, 255, 255]));
        }
        let d = diff(&base, &stroke).expect("same size");
        assert_eq!((d.pixels, d.worst), (100, 155));
        assert!(!d.within(), "a 100-pixel stroke is over a 5-pixel budget");
        let mut speck = base.clone();
        speck.put_pixel(3, 3, image::Rgba([0, 0, 0, 255]));
        assert!(
            diff(&base, &speck).is_some_and(|d| d.within()),
            "one speck is noise"
        );
        assert_eq!(diff(&base, &RgbaImage::new(10, 10)), None);
    }
}
