//! RGBA pictures and what the extractor does to them, in one place: the
//! `Image` every decoder hands out (`blp`, `png::read`, the renderers), and
//! the crops, disc masks, reductions, bilinear samplers and blends the
//! generators and tools share.
//!
//! Where two callers looked alike but computed differently, both rules are
//! kept here side by side under their own names, each with its arithmetic
//! exactly as it was, since the pictures they make are compared byte for
//! byte: three reductions (alpha averaged in, clear pixels left out,
//! premultiplied), three bilinear samplers (texel indices wrapped or
//! clamped, texture coordinates wrapped or clamped, pixel coordinates over
//! black) and two blends (bytes, floats).

use crate::png::Decoded;

/// A picture: RGBA, row-major, `width * height * 4` bytes (what the
/// decoders make; a broken source may hold fewer, and every reader here
/// treats a missing byte as 0).
#[derive(Debug, Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

fn byte(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}

impl Image {
    /// No picture: 0×0.
    pub fn empty() -> Image {
        Image {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        }
    }

    /// The pixel at (`x`, `y`); transparent black off the picture.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let at = (y * self.width + x) * 4;
        match self.rgba.get(at..at + 4) {
            Some(&[r, g, b, a]) => [r, g, b, a],
            _ => [0; 4],
        }
    }

    /// The `w`×`h` rectangle from (`x`, `y`), a row at a time; `None` where
    /// it reaches past the picture or its bytes.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Option<Image> {
        if x.checked_add(w)? > self.width || y.checked_add(h)? > self.height {
            return None;
        }
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in y..y + h {
            let at = (row * self.width + x) * 4;
            rgba.extend_from_slice(self.rgba.get(at..at + w * 4)?);
        }
        Some(Image {
            width: w,
            height: h,
            rgba,
        })
    }
}

// -- Discs ---------------------------------------------------------------------

/// How much of a pixel a disc of radius `r` covers, the pixel's middle
/// (`dx`, `dy`) from the disc's: 1 inside, 0 outside, falling over one
/// pixel across the edge, which `edge` moves out (0: the class icons' mask
/// fades just inside the circle; 0.5: the contact sheet's disc is centered
/// on it).
pub fn disc_coverage(dx: f32, dy: f32, r: f32, edge: f32) -> f32 {
    let d = (dx * dx + dy * dy).sqrt();
    (r - d + edge).clamp(0.0, 1.0)
}

/// A soft circular alpha mask over a square picture, so the square art
/// sits where a disc did: each pixel's alpha times its `disc_coverage`
/// (edge 0), truncated.
pub fn mask_disc(img: &mut Image) {
    let edge = img.width;
    let r = edge as f32 / 2.0;
    for y in 0..edge {
        for x in 0..edge {
            let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
            let keep = disc_coverage(dx, dy, r, 0.0);
            if keep < 1.0
                && let Some(a) = img.rgba.get_mut((y * edge + x) * 4 + 3)
            {
                *a = (*a as f32 * keep) as u8;
            }
        }
    }
}

// -- Reductions ----------------------------------------------------------------

/// Box-filter down by a whole `factor` (64→32 uses 2), every channel
/// averaged alike, alpha with them (a clear pixel darkens its block's
/// color): the icons, the talent art and the renders' 128×64 copies.
pub fn downscale_box(img: &Image, factor: usize) -> Image {
    if factor <= 1 {
        return Image {
            width: img.width,
            height: img.height,
            rgba: img.rgba.clone(),
        };
    }
    let w = img.width / factor;
    let h = img.height / factor;
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for dy in 0..factor {
                for dx in 0..factor {
                    let at = ((y * factor + dy) * img.width + x * factor + dx) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += byte(&img.rgba, at + c) as u32;
                    }
                }
            }
            let n = (factor * factor) as u32;
            out.extend(acc.iter().map(|a| (a / n) as u8));
        }
    }
    Image {
        width: w,
        height: h,
        rgba: out,
    }
}

/// Box-filter a `w`×`h` RGBA buffer down so its longer side is at most
/// `max`, averaging only the drawn pixels of each block (alpha above 0):
/// a clear pixel counts as empty, not black, and a block with none drawn
/// is clear. A floor's preview.
pub fn downscale_drawn(w: usize, h: usize, rgba: &[u8], max: usize) -> Image {
    let f = w.max(h).div_ceil(max).max(1);
    let (pw, ph) = (w / f, h / f);
    let mut out = Vec::with_capacity(pw * ph * 4);
    for y in 0..ph {
        for x in 0..pw {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for dy in 0..f {
                for dx in 0..f {
                    let at = ((y * f + dy) * w + x * f + dx) * 4;
                    if rgba.get(at + 3).is_some_and(|&a| a > 0) {
                        for (k, s) in acc.iter_mut().enumerate() {
                            *s += rgba.get(at + k).copied().unwrap_or(0) as u32;
                        }
                        n += 1;
                    }
                }
            }
            if n == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend(acc.map(|s| (s / n) as u8));
            }
        }
    }
    Image {
        width: pw,
        height: ph,
        rgba: out,
    }
}

/// A picture as premultiplied RGBA floats, 0–1.
fn premultiplied(img: &Image) -> Vec<[f32; 4]> {
    img.rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            let a = f32::from(a) / 255.0;
            let c = |v: u8| f32::from(v) / 255.0 * a;
            [c(r), c(g), c(b), a]
        })
        .collect()
}

/// For each of `to` samples along an axis of `from`, the source samples
/// it reads and their weights: a tent as wide as one source pixel when
/// enlarging, as wide as one output pixel when reducing (an area average).
fn taps(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = to as f32 / from.max(1) as f32;
    let radius = if scale < 1.0 { 1.0 / scale } else { 1.0 };
    (0..to)
        .map(|i| {
            let center = (i as f32 + 0.5) / scale - 0.5;
            let lo = (center - radius).floor().max(0.0) as usize;
            let hi = ((center + radius).ceil() as usize).min(from.saturating_sub(1));
            let mut taps: Vec<(usize, f32)> = (lo..=hi)
                .map(|j| (j, (1.0 - (j as f32 - center).abs() / radius).max(0.0)))
                .filter(|&(_, w)| w > 0.0)
                .collect();
            let sum: f32 = taps.iter().map(|t| t.1).sum();
            if sum > 0.0 {
                for t in &mut taps {
                    t.1 /= sum;
                }
            } else {
                // A sample past every source pixel's reach: the nearest.
                taps = vec![(
                    center.round().clamp(0.0, from.saturating_sub(1) as f32) as usize,
                    1.0,
                )];
            }
            taps
        })
        .collect()
}

/// `img` resampled to `w × h` as premultiplied RGBA floats, across then
/// down, through `taps`' tents (an area average when reducing): the
/// contact sheet's tiles, where a clear pixel weighs nothing.
pub fn resample_premultiplied(img: &Image, w: usize, h: usize) -> Vec<[f32; 4]> {
    let src = premultiplied(img);
    let (sw, sh) = (img.width, img.height);
    let xs = taps(sw, w);
    let ys = taps(sh, h);
    let mut rows = vec![[0.0f32; 4]; w * sh];
    for y in 0..sh {
        for (x, tap) in xs.iter().enumerate() {
            let mut acc = [0.0f32; 4];
            for &(j, wt) in tap {
                if let Some(p) = src.get(y * sw + j) {
                    for (a, v) in acc.iter_mut().zip(p) {
                        *a += v * wt;
                    }
                }
            }
            if let Some(o) = rows.get_mut(y * w + x) {
                *o = acc;
            }
        }
    }
    let mut out = vec![[0.0f32; 4]; w * h];
    for (y, tap) in ys.iter().enumerate() {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for &(j, wt) in tap {
                if let Some(p) = rows.get(j * w + x) {
                    for (a, v) in acc.iter_mut().zip(p) {
                        *a += v * wt;
                    }
                }
            }
            if let Some(o) = out.get_mut(y * w + x) {
                *o = acc;
            }
        }
    }
    out
}

// -- Bilinear samplers ---------------------------------------------------------

/// Bilinear sample at texture coordinates (`u`, `v`), RGBA in 0–1: the
/// four texels round `u·w − ½`, `v·h − ½`, each texel INDEX wrapped
/// (`rem_euclid`) or, where `clamp` says for its axis, clamped to the
/// edge; a missing byte reads 0. The floors' textures, terrain layers and
/// liquids.
pub fn bilinear_texel_wrap(img: &Image, u: f32, v: f32, clamp: [bool; 2]) -> [f32; 4] {
    let (w, h) = (img.width as i64, img.height as i64);
    let fx = u * w as f32 - 0.5;
    let fy = v * h as f32 - 0.5;
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let wrap = |c: i64, n: i64, clamp: bool| {
        if clamp {
            c.clamp(0, n - 1)
        } else {
            c.rem_euclid(n)
        }
    };
    let px = |x: i64, y: i64| -> [f32; 4] {
        let at = ((wrap(y, h, clamp[1]) * w + wrap(x, w, clamp[0])) * 4) as usize;
        let mut out = [0.0; 4];
        for (k, o) in out.iter_mut().enumerate() {
            *o = img.rgba.get(at + k).copied().unwrap_or(0) as f32 / 255.0;
        }
        out
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (
        px(x0, y0),
        px(x0 + 1, y0),
        px(x0, y0 + 1),
        px(x0 + 1, y0 + 1),
    );
    let mut out = [0.0; 4];
    for ((((o, a), b), c), d) in out.iter_mut().zip(a).zip(b).zip(c).zip(d) {
        let top = a + (b - a) * tx;
        let bottom = c + (d - c) * tx;
        *o = top + (bottom - top) * ty;
    }
    out
}

/// Bilinear sample at texture coordinates (`u`, `v`), RGBA in 0–1, with
/// the COORDINATE wrapped into 0–1 first (`rem_euclid(1.0)`) or clamped to
/// it, per `wrap`'s axis, and each texel index then wrapped or clamped
/// alike; an empty picture, or a texel past its bytes, reads white. The
/// creature renderer's textures.
pub fn bilinear_uv_wrap(img: &Image, u: f32, v: f32, wrap: (bool, bool)) -> [f32; 4] {
    let (w, h) = (img.width, img.height);
    if w == 0 || h == 0 {
        return [1.0; 4];
    }
    let fit = |t: f32, n: usize, wrap: bool| -> f32 {
        if wrap {
            t.rem_euclid(1.0) * n as f32
        } else {
            t.clamp(0.0, 1.0) * n as f32
        }
    };
    let x = fit(u, w, wrap.0) - 0.5;
    let y = fit(v, h, wrap.1) - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |xi: f32, yi: f32| -> [f32; 4] {
        let wrap_i = |i: f32, n: usize, wrap: bool| -> usize {
            let i = i as i64;
            let n = n as i64;
            if wrap {
                i.rem_euclid(n) as usize
            } else {
                i.clamp(0, n - 1) as usize
            }
        };
        let (xi, yi) = (wrap_i(xi, w, wrap.0), wrap_i(yi, h, wrap.1));
        let o = (yi * w + xi) * 4;
        let p = img.rgba.get(o..o + 4).unwrap_or(&[255, 255, 255, 255]);
        [
            f32::from(byte(p, 0)) / 255.0,
            f32::from(byte(p, 1)) / 255.0,
            f32::from(byte(p, 2)) / 255.0,
            f32::from(byte(p, 3)) / 255.0,
        ]
    };
    let (a, b, c, d) = (
        at(x0, y0),
        at(x0 + 1.0, y0),
        at(x0, y0 + 1.0),
        at(x0 + 1.0, y0 + 1.0),
    );
    let mut out = [0.0; 4];
    for ((o, (a, b)), (c, d)) in out.iter_mut().zip(a.iter().zip(&b)).zip(c.iter().zip(&d)) {
        let top = a + (b - a) * fx;
        let bottom = c + (d - c) * fx;
        *o = top + (bottom - top) * fy;
    }
    out
}

/// Bilinear sample of a decoded PNG at PIXEL coordinates (`col`, `row`),
/// its color premultiplied by alpha (over black), RGB in 0–255: the four
/// pixels round `col − ½`, `row − ½`, each weighted, those before the
/// picture's first row or column left out (and past its last, transparent
/// black). The stencil grid's render, read where it lies, never copied.
pub fn bilinear_over_black(img: &Decoded, col: f32, row: f32) -> [f32; 3] {
    let (c0, r0) = ((col - 0.5).floor(), (row - 0.5).floor());
    let (fc, fr) = (col - 0.5 - c0, row - 0.5 - r0);
    let mut acc = [0.0f32; 3];
    for (dc, dr, w) in [
        (0.0, 0.0, (1.0 - fc) * (1.0 - fr)),
        (1.0, 0.0, fc * (1.0 - fr)),
        (0.0, 1.0, (1.0 - fc) * fr),
        (1.0, 1.0, fc * fr),
    ] {
        let (x, y) = (c0 + dc, r0 + dr);
        if x < 0.0 || y < 0.0 {
            continue;
        }
        let [r, g, b, a] = img.rgba(x as usize, y as usize);
        let a = f32::from(a) / 255.0 * w;
        for (s, v) in acc.iter_mut().zip([r, g, b]) {
            *s += f32::from(v) * a;
        }
    }
    acc
}

// -- Blends --------------------------------------------------------------------

/// Lay `ink` over a pixel's first three bytes at `alpha`, each channel
/// `c·(1 − alpha) + ink·alpha` rounded (at 1, the ink exactly). The
/// stencil grid's lines, dots and digits.
pub fn blend_rgb8(px: &mut [u8], ink: [u8; 3], alpha: f32) {
    for (c, &i) in px.iter_mut().zip(&ink) {
        *c = (f32::from(*c) * (1.0 - alpha) + f32::from(i) * alpha).round() as u8;
    }
}

/// Lay `c` over an RGB float pixel at `coverage`: `p + (c − p)·coverage`.
/// The contact sheet's pictures, discs and labels.
pub fn blend_rgbf(p: &mut [f32; 3], c: [f32; 3], coverage: f32) {
    for (p, c) in p.iter_mut().zip(c) {
        *p += (c - *p) * coverage;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: usize, h: usize) -> Image {
        let rgba = (0..w * h)
            .flat_map(|i| {
                let v = (i * 37 % 251) as u8;
                [v, 255 - v, v / 2, if i % 3 == 0 { 0 } else { 200 }]
            })
            .collect();
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn a_crop_is_its_rows_and_stays_inside() {
        let img = checker(5, 4);
        let c = img.crop(1, 2, 3, 2).unwrap();
        assert_eq!((c.width, c.height), (3, 2));
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(c.pixel(x, y), img.pixel(x + 1, y + 2));
            }
        }
        assert!(img.crop(3, 0, 3, 1).is_none());
        assert!(img.crop(0, 3, 1, 2).is_none());
        assert!(img.crop(usize::MAX, 0, 2, 1).is_none());
        assert_eq!(img.crop(5, 4, 0, 0).map(|c| c.rgba.len()), Some(0));
        assert_eq!(img.pixel(5, 0), [0; 4]);
    }

    #[test]
    fn a_disc_covers_its_inside_and_fades_over_a_pixel() {
        assert_eq!(disc_coverage(0.0, 0.0, 4.0, 0.0), 1.0);
        assert_eq!(disc_coverage(3.0, 4.0, 4.0, 0.0), 0.0);
        assert_eq!(disc_coverage(3.0, 4.0, 4.5, 0.0), 0.0);
        assert!((disc_coverage(3.0, 4.0, 4.5, 0.5) - 0.0).abs() < 1e-6);
        assert!((disc_coverage(0.0, 4.0, 4.5, 0.0) - 0.5).abs() < 1e-6);
        assert!((disc_coverage(0.0, 4.0, 4.5, 0.5) - 1.0).abs() < 1e-6);
        let mut img = Image {
            width: 8,
            height: 8,
            rgba: vec![255; 8 * 8 * 4],
        };
        mask_disc(&mut img);
        assert_eq!(img.pixel(0, 0)[3], 0);
        assert_eq!(img.pixel(4, 4)[3], 255);
        assert_eq!(img.pixel(0, 0)[..3], [255, 255, 255]);
    }

    #[test]
    fn downscale_box_by_one_copies() {
        let img = Image {
            width: 1,
            height: 1,
            rgba: vec![9, 8, 7, 6],
        };
        let same = downscale_box(&img, 1);
        assert_eq!(
            (same.width, same.height, same.rgba),
            (1, 1, vec![9, 8, 7, 6])
        );
    }

    #[test]
    fn downscale_box_averages_blocks() {
        let img = Image {
            width: 2,
            height: 2,
            rgba: vec![
                255, 0, 0, 255, //
                0, 0, 0, 255, //
                0, 0, 0, 255, //
                255, 0, 0, 255,
            ],
        };
        let half = downscale_box(&img, 2);
        assert_eq!((half.width, half.height), (1, 1));
        assert_eq!(&half.rgba, &[127, 0, 0, 255]);
    }

    #[test]
    fn downscale_box_averages_alpha_with_color() {
        let img = Image {
            width: 2,
            height: 2,
            rgba: vec![
                200, 0, 0, 255, 0, 0, 0, 0, //
                100, 0, 0, 255, 0, 0, 0, 0,
            ],
        };
        let same = downscale_box(&img, 1);
        assert_eq!(same.rgba, img.rgba);
        let half = downscale_box(&img, 2);
        assert_eq!((half.width, half.height), (1, 1));
        assert_eq!(half.rgba, [75, 0, 0, 127]);
    }

    #[test]
    fn downscale_drawn_leaves_clear_pixels_out() {
        let rgba = vec![
            200, 0, 0, 255, 0, 0, 0, 0, //
            100, 0, 0, 255, 0, 0, 0, 0, //
            0, 0, 0, 0, 0, 0, 0, 0, //
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let p = downscale_drawn(2, 4, &rgba, 2);
        assert_eq!((p.width, p.height), (1, 2));
        assert_eq!(p.rgba, [150, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn resample_premultiplied_weighs_clear_pixels_nothing() {
        let img = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 0],
        };
        let out = resample_premultiplied(&img, 1, 1);
        let [r, g, _, a] = out[0];
        assert!((r - 0.5).abs() < 1e-6 && g.abs() < 1e-6 && (a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_samplers_wrap_and_clamp_as_named() {
        let img = Image {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 0, 255, 255, 255, 255],
        };
        // Between the two texels.
        assert_eq!(bilinear_texel_wrap(&img, 0.5, 0.5, [false; 2])[0], 0.5);
        assert_eq!(bilinear_uv_wrap(&img, 0.5, 0.5, (true, true))[0], 0.5);
        // Past the right edge: wrapped onto the first texel, or clamped.
        assert_eq!(bilinear_texel_wrap(&img, 1.0, 0.5, [false; 2])[0], 0.5);
        assert_eq!(bilinear_texel_wrap(&img, 1.0, 0.5, [true; 2])[0], 1.0);
        assert_eq!(bilinear_uv_wrap(&img, 1.0, 0.5, (false, false))[0], 1.0);
        assert_eq!(
            bilinear_uv_wrap(&Image::empty(), 0.3, 0.3, (true, true)),
            [1.0; 4]
        );
    }

    #[test]
    fn the_blends_lay_ink_over() {
        let mut px = [100u8, 0, 255, 7];
        blend_rgb8(&mut px, [200, 255, 0], 0.5);
        assert_eq!(px, [150, 128, 128, 7]);
        blend_rgb8(&mut px, [1, 2, 3], 1.0);
        assert_eq!(px, [1, 2, 3, 7]);
        let mut p = [0.0f32, 1.0, 0.5];
        blend_rgbf(&mut p, [1.0, 0.0, 0.5], 0.25);
        assert_eq!(p, [0.25, 0.75, 0.5]);
    }
}
