//! A place's swirl as pixels (`crate::Swirl`): the one renderer, which the
//! replay calls and the map editor will, so their discs match.
//!
//! The conventions, all on screen, x right and y down:
//!
//! - The disc is `side` px square, its middle at (side/2, side/2) and its
//!   radius R = side/2. A pixel is read at its center: pixel (i, j) lies at
//!   p = (i + ½ − R, j + ½ − R) from the middle, r = |p|.
//! - Its alpha is the rim's alone: 1 within 0.95 R, falling linearly to 0
//!   at R, 0 past it. The texture's own alpha is not read. The output is
//!   straight alpha, not premultiplied; a pixel past the rim is all zeros.
//! - The texture's whole square spans the disc's bounding square
//!   (uv = q / 2R + ½, the texture's top-left corner at the square's), read
//!   bilinearly between texel centers and tiled past its edges, at the
//!   point q that p is turned to.
//! - The twist is ImageMagick's `SwirlImage`: q = Rot(θ(r)) p, with
//!   Rot(θ) = [[cos θ, −sin θ], [sin θ, cos θ]] in these y-down
//!   coordinates (a turn clockwise on screen) and θ(r) = twist · (1 − r/R)²,
//!   so a mock made with `magick … -swirl <twist>` matches the disc at
//!   angle 0. A positive twist carries the picture's middle
//!   counter-clockwise on screen, the rim staying put.
//! - `angle` is how far the whole disc has turned clockwise on screen, in
//!   radians (`Swirl::turned`): q is turned back by it, Rot(−angle), so
//!   the picture moves clockwise as the angle grows. Both turns are about
//!   the middle and the twist reads r alone, so their order does not
//!   matter: q = Rot(θ(r) − angle) p, one `sin_cos` a pixel.
//! - Its color: the gray at q (the mean of r, g and b, 0 to 1), leveled
//!   from 5% to 95% (g' = ((g − 0.05) / 0.90), clamped to [0, 1]), runs
//!   from the dark color to the light one channel by channel in their
//!   stored 8-bit values, no gamma, rounded.
//!
//! A swirl whose colors do not read (`Swirl::rgb`, which `check` holds), or
//! a texture without the texels its size says, draws nothing: every pixel
//! clear.
//!
//! Plain arithmetic: no allocation but `disc`'s output, no threads. A
//! caller that wants threads splits the disc into bands of rows (`rows`).

use std::ops::Range;

use crate::Swirl;

/// The share of the radius the rim feathers over.
const FEATHER: f32 = 0.05;

/// The gray that is the dark color, and the gray that is the light one.
const LEVELS: (f32, f32) = (0.05, 0.95);

/// A gray, tileable texture: `w`×`h`, RGBA rows top first, 4 bytes a texel
/// (its gray is the mean of r, g and b; its alpha is not read).
#[derive(Debug, Clone, Copy)]
pub struct Texture<'a> {
    pub w: u32,
    pub h: u32,
    pub rgba: &'a [u8],
}

impl Texture<'_> {
    /// Whether it holds a texel at least, and every texel its size says.
    fn whole(&self) -> bool {
        let n = (self.w as usize)
            .checked_mul(self.h as usize)
            .and_then(|n| n.checked_mul(4));
        n.is_some_and(|n| n > 0 && self.rgba.len() >= n)
    }

    /// Texel (x, y)'s gray, 0 to 1. Both are in range (`gray_at` wraps).
    fn texel(&self, x: usize, y: usize) -> f32 {
        let i = (y * self.w as usize + x) * 4;
        match self.rgba.get(i..i + 3) {
            Some(&[r, g, b]) => f32::from(u16::from(r) + u16::from(g) + u16::from(b)) / 765.0,
            _ => 0.0,
        }
    }

    /// The gray at (u, v), the texture's square being [0, 1]², bilinear
    /// between texel centers, tiled.
    fn gray_at(&self, u: f32, v: f32) -> f32 {
        let tx = u * self.w as f32 - 0.5;
        let ty = v * self.h as f32 - 0.5;
        let (x0, y0) = (tx.floor(), ty.floor());
        let (fx, fy) = (tx - x0, ty - y0);
        let wrap = |c: f32, n: u32| (c as i64).rem_euclid(i64::from(n)) as usize;
        let (xa, xb) = (wrap(x0, self.w), wrap(x0 + 1.0, self.w));
        let (ya, yb) = (wrap(y0, self.h), wrap(y0 + 1.0, self.h));
        let top = lerp(self.texel(xa, ya), self.texel(xb, ya), fx);
        let bottom = lerp(self.texel(xa, yb), self.texel(xb, yb), fx);
        lerp(top, bottom, fy)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The swirl's disc, `side` px square: `side`² straight-alpha RGBA pixels,
/// rows top first, 4 bytes a pixel, turned `angle` radians clockwise on
/// screen (`Swirl::turned`; 0 is the picture as ImageMagick's `-swirl`
/// gives it). The module's doc says how each pixel is made.
pub fn disc(tex: Texture<'_>, swirl: &Swirl, side: u32, angle: f32) -> Vec<u8> {
    let n = (side as usize)
        .saturating_mul(side as usize)
        .saturating_mul(4);
    let mut out = vec![0; n];
    rows(tex, swirl, side, angle, 0..side, &mut out);
    out
}

/// A band of `disc`'s rows into `out`: rows `band.start` to `band.end`
/// (past `side`, none), `side` · 4 bytes each, the band's first row
/// first; a band's pixels are the whole disc's, so a caller may draw a
/// disc in bands on threads of its own. Rows `out` has no room for are
/// left out, and bytes past the band's are left as they were.
pub fn rows(
    tex: Texture<'_>,
    swirl: &Swirl,
    side: u32,
    angle: f32,
    band: Range<u32>,
    out: &mut [u8],
) {
    let stride = (side as usize).saturating_mul(4);
    if stride == 0 {
        return;
    }
    let band = band.start..band.end.min(side);
    let lines = band.zip(out.chunks_exact_mut(stride));
    let colors = swirl.rgb().filter(|_| tex.whole());
    let Some([[dr, dg, db], [lr, lg, lb]]) = colors else {
        lines.for_each(|(_, line)| line.fill(0));
        return;
    };
    let radius = side as f32 / 2.0;
    let inner = radius * (1.0 - FEATHER);
    let feather = radius * FEATHER;
    let twist = swirl.twist.to_radians();
    let across = 2.0 * radius;
    let (black, white) = LEVELS;
    let mix = |d: u8, l: u8, g: f32| {
        let (d, l) = (f32::from(d), f32::from(l));
        (d + (l - d) * g).round().clamp(0.0, 255.0) as u8
    };
    for (j, line) in lines {
        let dy = j as f32 + 0.5 - radius;
        for (i, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let dx = i as f32 + 0.5 - radius;
            let r = (dx * dx + dy * dy).sqrt();
            let alpha = if r <= inner {
                1.0
            } else if r < radius {
                (radius - r) / feather
            } else {
                *px = [0; 4];
                continue;
            };
            let fall = 1.0 - r / radius;
            let (sin, cos) = (twist * fall * fall - angle).sin_cos();
            let (qx, qy) = (cos * dx - sin * dy, sin * dx + cos * dy);
            let g = tex.gray_at(qx / across + 0.5, qy / across + 0.5);
            let g = ((g - black) / (white - black)).clamp(0.0, 1.0);
            *px = [
                mix(dr, lr, g),
                mix(dg, lg, g),
                mix(db, lb, g),
                (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
            ];
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::{Texture, disc, rows};
    use crate::Swirl;

    fn swirl(dark: &str, light: &str, twist: f32) -> Swirl {
        Swirl {
            texture: 5207558,
            colors: [dark.to_string(), light.to_string()],
            twist,
            spin: -12.0,
        }
    }

    /// A `w`×`h` texture whose texel (x, y) is the gray `f(x, y)`.
    fn texture(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let g = f(x, y);
                [g, g, g, 255]
            })
            .collect()
    }

    /// Pixel (i, j) of a disc `side` px square.
    fn px(out: &[u8], side: u32, i: u32, j: u32) -> [u8; 4] {
        let k = ((j * side + i) * 4) as usize;
        [out[k], out[k + 1], out[k + 2], out[k + 3]]
    }

    /// A pixel's distance from the middle, as the renderer reads it.
    fn distance(side: u32, i: u32, j: u32) -> f32 {
        let c = side as f32 / 2.0;
        let (dx, dy) = (i as f32 + 0.5 - c, j as f32 + 0.5 - c);
        (dx * dx + dy * dy).sqrt()
    }

    /// A flat gray is one color within the rim, the gray leveled between
    /// the two; the alpha is 1 within 0.95 R, 0 from R out, and falls
    /// linearly between.
    #[test]
    fn a_flat_mid_gray_is_the_mid_color_within_and_clear_past_the_rim() {
        let rgba = texture(4, 4, |_, _| 128);
        let tex = Texture {
            w: 4,
            h: 4,
            rgba: &rgba,
        };
        let s = swirl("#000000", "#c86432", 330.0);
        let side = 200;
        let out = disc(tex, &s, side, 1.0);
        let g = ((128.0 / 255.0) - 0.05) / 0.90;
        let mid = [200.0 * g, 100.0 * g, 50.0 * g].map(|c: f32| c.round() as u8);
        let r = side as f32 / 2.0;
        let mut rim = 0;
        for j in 0..side {
            for i in 0..side {
                let [cr, cg, cb, a] = px(&out, side, i, j);
                let d = distance(side, i, j);
                if d >= r {
                    assert_eq!([cr, cg, cb, a], [0; 4], "({i}, {j})");
                    continue;
                }
                assert_eq!([cr, cg, cb], mid, "({i}, {j})");
                if d <= 0.95 * r {
                    assert_eq!(a, 255, "({i}, {j})");
                } else {
                    let want = 255.0 * (r - d) / (0.05 * r);
                    assert!((f32::from(a) - want).abs() <= 0.51, "({i}, {j}): {a}");
                    rim += 1;
                }
            }
        }
        assert!(rim > 100, "{rim} rim pixels");
    }

    /// A gray under 5% is the dark color and one over 95% the light, each
    /// exactly.
    #[test]
    fn the_gray_is_leveled_from_five_to_ninety_five_percent() {
        let s = swirl("#03201a", "#7af0b8", 330.0);
        for (gray, want) in [(5u8, [0x03, 0x20, 0x1a]), (250, [0x7a, 0xf0, 0xb8])] {
            let rgba = texture(8, 8, |_, _| gray);
            let tex = Texture {
                w: 8,
                h: 8,
                rgba: &rgba,
            };
            let out = disc(tex, &s, 32, 0.0);
            let [r, g, b, a] = px(&out, 32, 16, 16);
            assert_eq!(([r, g, b], a), (want, 255), "gray {gray}");
        }
    }

    /// Untwisted, the disc turned a quarter shows at the bottom what it
    /// showed at the right: it turns clockwise. Every pixel moves so.
    #[test]
    fn a_quarter_turn_brings_the_right_to_the_bottom() {
        let rgba = texture(16, 16, |x, y| ((x * 37 + y * 101) % 256) as u8);
        let tex = Texture {
            w: 16,
            h: 16,
            rgba: &rgba,
        };
        let s = swirl("#000000", "#ffffff", 0.0);
        let side = 201;
        let still = disc(tex, &s, side, 0.0);
        let turned = disc(tex, &s, side, FRAC_PI_2);
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1);
        assert_ne!(still, turned);
        for k in [10, 40, 90] {
            let right = px(&still, side, 100 + k, 100);
            let bottom = px(&turned, side, 100, 100 + k);
            assert!(near(right, bottom), "{k}: {right:?} then {bottom:?}");
        }
        // Pixel (x, y) from the middle, turned, shows what (y, −x) did.
        for j in 0..side {
            for i in 0..side {
                let (x, y) = (i as i32 - 100, j as i32 - 100);
                let (si, sj) = ((100 + y) as u32, (100 - x) as u32);
                let (a, b) = (px(&turned, side, i, j), px(&still, side, si, sj));
                assert!(near(a, b), "({i}, {j}): {a:?}, ({si}, {sj}): {b:?}");
            }
        }
    }

    /// A picture bright on its right half, twisted 90°: near the middle
    /// the bright half has gone round to the top (counter-clockwise, as
    /// ImageMagick's `-swirl 90` turns it), and near the rim it stays on
    /// the right.
    #[test]
    fn a_twist_turns_the_middle_counter_clockwise_and_leaves_the_rim() {
        let rgba = texture(64, 64, |x, _| if x >= 32 { 255 } else { 0 });
        let tex = Texture {
            w: 64,
            h: 64,
            rgba: &rgba,
        };
        let s = swirl("#000000", "#ffffff", 90.0);
        let side = 201;
        let out = disc(tex, &s, side, 0.0);
        let gray = |i, j| px(&out, side, i, j)[0];
        // 10 px from the middle (θ ≈ 73°): the top bright, the bottom dark.
        assert!(gray(100, 90) > 200, "{}", gray(100, 90));
        assert!(gray(100, 110) < 55, "{}", gray(100, 110));
        // 90 px out (θ ≈ 1°): the right bright, the left dark.
        assert!(gray(190, 100) > 200, "{}", gray(190, 100));
        assert!(gray(10, 100) < 55, "{}", gray(10, 100));
        // Untwisted, the top is the seam between the halves, neither.
        let flat = disc(tex, &swirl("#000000", "#ffffff", 0.0), side, 0.0);
        let seam = px(&flat, side, 100, 90)[0];
        assert!((120..=136).contains(&seam), "{seam}");
        assert!(px(&flat, side, 190, 100)[0] > 200);
        assert!(px(&flat, side, 10, 100)[0] < 55);
    }

    /// The disc is `side`² pixels of 4 bytes, none for no side, and a disc
    /// of one pixel is that pixel, whole.
    #[test]
    fn the_disc_is_side_squared_by_four_bytes() {
        let rgba = texture(2, 2, |_, _| 200);
        let tex = Texture {
            w: 2,
            h: 2,
            rgba: &rgba,
        };
        let s = swirl("#000000", "#ffffff", 330.0);
        for side in [0u32, 1, 2, 7, 64, 255] {
            assert_eq!(disc(tex, &s, side, 2.0).len(), (side * side * 4) as usize);
        }
        assert!(disc(tex, &s, 0, 0.0).is_empty());
        assert_eq!(disc(tex, &s, 1, 0.0)[3], 255);
    }

    /// A band of rows is the whole disc's rows, and a band past the disc
    /// draws nothing.
    #[test]
    fn a_band_of_rows_is_the_whole_discs() {
        let rgba = texture(16, 16, |x, y| ((x * 13 + y * 7) % 256) as u8);
        let tex = Texture {
            w: 16,
            h: 16,
            rgba: &rgba,
        };
        let s = swirl("#03201a", "#7af0b8", 330.0);
        let side = 96;
        let whole = disc(tex, &s, side, 0.7);
        let stride = side as usize * 4;
        let mut band = vec![7; 30 * stride];
        rows(tex, &s, side, 0.7, 40..70, &mut band);
        assert_eq!(band, whole[40 * stride..70 * stride]);
        // Rows past `side` are none: the bytes stay as they were.
        let mut past = vec![7; 10 * stride];
        rows(tex, &s, side, 0.7, 90..100, &mut past);
        assert_eq!(past[..6 * stride], whole[90 * stride..]);
        assert!(past[6 * stride..].iter().all(|&b| b == 7));
    }

    /// Colors that do not read, or a texture short of its size, draw a
    /// clear disc.
    #[test]
    fn a_swirl_that_cannot_be_drawn_is_clear() {
        let rgba = texture(4, 4, |_, _| 128);
        let whole = Texture {
            w: 4,
            h: 4,
            rgba: &rgba,
        };
        let short = Texture {
            w: 5,
            h: 4,
            rgba: &rgba,
        };
        let empty = Texture {
            w: 0,
            h: 0,
            rgba: &[],
        };
        let good = swirl("#000000", "#ffffff", 330.0);
        let bad = swirl("#000000", "fff", 330.0);
        for (tex, s) in [(whole, &bad), (short, &good), (empty, &good)] {
            assert!(disc(tex, s, 16, 0.0).iter().all(|&b| b == 0), "{tex:?}");
        }
        assert!(disc(whole, &good, 16, 0.0).iter().any(|&b| b != 0));
    }

    /// The colors read as `#rrggbb` alone.
    #[test]
    fn a_swirls_colors_read_as_rrggbb_alone() {
        let s = swirl("#03201a", "#7AF0B8", 0.0);
        assert_eq!(s.rgb(), Some([[0x03, 0x20, 0x1a], [0x7a, 0xf0, 0xb8]]));
        for bad in [
            "03201a", "#fff", "#gg0000", "#+f0000", "#03201a0", "#03201", "",
        ] {
            assert_eq!(swirl(bad, "#ffffff", 0.0).rgb(), None, "{bad:?}");
            assert_eq!(swirl("#ffffff", bad, 0.0).rgb(), None, "{bad:?}");
        }
        assert_eq!(swirl("#0320é", "#ffffff", 0.0).rgb(), None);
    }

    /// `spin` degrees a second, clockwise, as a turn in [0, 2π).
    #[test]
    fn a_swirl_turns_by_its_spin() {
        let s = |spin| Swirl {
            spin,
            ..swirl("#000000", "#ffffff", 0.0)
        };
        assert!((s(90.0).turned(1.0) - FRAC_PI_2).abs() < 1e-6);
        // Counter-clockwise a quarter is clockwise three quarters.
        assert!((s(-90.0).turned(1.0) - 3.0 * FRAC_PI_2).abs() < 1e-5);
        // −12° a second: a whole turn in 30 s, and a quarter
        // counter-clockwise 7.5 s on, an hour in as at the start.
        assert!(s(-12.0).turned(30.0).abs() < 1e-6);
        for t in [7.5, 3600.0 + 7.5] {
            let a = s(-12.0).turned(t);
            assert!((a - 3.0 * FRAC_PI_2).abs() < 1e-4, "{t}: {a}");
        }
        assert_eq!(s(f32::NAN).turned(1.0), 0.0);
        for t in [0.0, 0.3, 29.9, 1e6] {
            let a = s(-12.0).turned(t);
            assert!((0.0..std::f32::consts::TAU).contains(&a), "{t}: {a}");
        }
    }
}
