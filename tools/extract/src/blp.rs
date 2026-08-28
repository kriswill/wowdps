//! BLP2 texture decoding — enough of the format to turn the client's icon
//! files into RGBA, in the same hand-rolled spirit as `inflate`/`wdc5`.
//!
//! Icons are 64×64 BLP2, almost always DXT-compressed (DXT1 for the old
//! class crests, DXT5 for modern spell art); palettized and raw files are
//! handled too because nothing about the header promises otherwise.
//!
//! Like everything in this workspace, malformed input is an `Err`, never a
//! panic — all reads go through bounds-checked accessors, and a read past a
//! block's edge (impossible for well-formed data) yields zero.

/// Decoded mip 0.
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// RGBA, row-major, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

fn byte(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}

fn u16at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([byte(b, at), byte(b, at + 1)])
}

fn u32at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([byte(s, 0), byte(s, 1), byte(s, 2), byte(s, 3)]))
        .ok_or_else(|| format!("blp: truncated at {at}"))
}

/// Decode a BLP2 file's top mip to RGBA.
pub fn decode(data: &[u8]) -> Result<Image, String> {
    if data.get(..4) != Some(b"BLP2") {
        return Err("blp: bad magic".into());
    }
    let compression = byte(data, 8);
    let alpha_depth = byte(data, 9);
    let alpha_type = byte(data, 10);
    let width = u32at(data, 12)? as usize;
    let height = u32at(data, 16)? as usize;
    // 4096 admits the UI texture atlases (2048×1024 sheets) that the
    // talent-art generator crops; icons stay far below it.
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(format!("blp: unreasonable size {width}x{height}"));
    }
    let offset = u32at(data, 20)? as usize;
    let size = u32at(data, 84)? as usize;
    let mip = data
        .get(offset..offset.saturating_add(size))
        .ok_or("blp: mip range out of bounds")?;

    let rgba = match compression {
        1 => palettized(data, mip, width, height, alpha_depth)?,
        2 => match alpha_type {
            7 => dxt(mip, width, height, Dxt::Five)?,
            1 if alpha_depth == 8 => dxt(mip, width, height, Dxt::Three)?,
            _ => dxt(mip, width, height, Dxt::One)?,
        },
        3 => raw_bgra(mip, width, height)?,
        c => return Err(format!("blp: unsupported compression {c}")),
    };
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// Compression 1: one palette index per pixel, then packed alpha rows.
fn palettized(
    file: &[u8],
    mip: &[u8],
    w: usize,
    h: usize,
    alpha_depth: u8,
) -> Result<Vec<u8>, String> {
    let pal = file.get(148..148 + 1024).ok_or("blp: no palette")?;
    let n = w * h;
    let idx = mip.get(..n).ok_or("blp: palettized mip too small")?;
    let bits = mip.get(n..).unwrap_or(&[]);
    let mut out = Vec::with_capacity(n * 4);
    for (i, &p) in idx.iter().enumerate() {
        let at = p as usize * 4; // BGRA entries
        let alpha = match alpha_depth {
            0 => 255,
            1 => {
                let bit = bits.get(i / 8).map_or(1, |b| (b >> (i % 8)) & 1);
                if bit == 1 { 255 } else { 0 }
            }
            4 => {
                let nib = bits.get(i / 2).map_or(0xF, |b| (b >> ((i % 2) * 4)) & 0xF);
                nib * 17
            }
            8 => bits.get(i).copied().unwrap_or(255),
            d => return Err(format!("blp: palettized alpha depth {d}")),
        };
        out.extend_from_slice(&[byte(pal, at + 2), byte(pal, at + 1), byte(pal, at), alpha]);
    }
    Ok(out)
}

/// Compression 3: BGRA, one u32 per pixel.
fn raw_bgra(mip: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let n = w * h;
    let src = mip.get(..n * 4).ok_or("blp: raw mip too small")?;
    let mut out = Vec::with_capacity(n * 4);
    for &[b, g, r, a] in src.as_chunks::<4>().0 {
        out.extend_from_slice(&[r, g, b, a]);
    }
    Ok(out)
}

fn c565(v: u16) -> [u8; 3] {
    let r = ((v >> 11) & 0x1F) as u32;
    let g = ((v >> 5) & 0x3F) as u32;
    let b = (v & 0x1F) as u32;
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
    ]
}

/// The 4-color (or 3+transparent) palette of one DXT color block.
fn color_block(c0: u16, c1: u16, opaque: bool) -> [[u8; 4]; 4] {
    let a = c565(c0);
    let b = c565(c1);
    let mix = |x: u8, y: u8, num: u32, den: u32| -> u8 {
        ((x as u32 * num + y as u32 * (den - num)) / den) as u8
    };
    if c0 > c1 || opaque {
        [
            [a[0], a[1], a[2], 255],
            [b[0], b[1], b[2], 255],
            [
                mix(a[0], b[0], 2, 3),
                mix(a[1], b[1], 2, 3),
                mix(a[2], b[2], 2, 3),
                255,
            ],
            [
                mix(a[0], b[0], 1, 3),
                mix(a[1], b[1], 1, 3),
                mix(a[2], b[2], 1, 3),
                255,
            ],
        ]
    } else {
        [
            [a[0], a[1], a[2], 255],
            [b[0], b[1], b[2], 255],
            [
                mix(a[0], b[0], 1, 2),
                mix(a[1], b[1], 1, 2),
                mix(a[2], b[2], 1, 2),
                255,
            ],
            [0, 0, 0, 0], // 1-bit transparent
        ]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dxt {
    One,
    Three,
    Five,
}

fn dxt(mip: &[u8], w: usize, h: usize, kind: Dxt) -> Result<Vec<u8>, String> {
    let block_bytes = if kind == Dxt::One { 8 } else { 16 };
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    if mip.len() < bw * bh * block_bytes {
        return Err("blp: dxt mip too small".into());
    }
    // The color half sits after the alpha half in DXT3/5 blocks.
    let c = if kind == Dxt::One { 0 } else { 8 };
    let mut out = vec![0u8; w * h * 4];
    for (bi, block) in mip.chunks_exact(block_bytes).take(bw * bh).enumerate() {
        let (bx, by) = (bi % bw, bi / bw);
        let pal = color_block(u16at(block, c), u16at(block, c + 2), kind != Dxt::One);
        let color_bits = u32at(block, c + 4).unwrap_or(0);
        // DXT5's 48 bits of 3-bit alpha indices.
        let (a0, a1) = (byte(block, 0) as u32, byte(block, 1) as u32);
        let alpha_bits = u64::from_le_bytes([
            byte(block, 2),
            byte(block, 3),
            byte(block, 4),
            byte(block, 5),
            byte(block, 6),
            byte(block, 7),
            0,
            0,
        ]);
        for t in 0..16 {
            let (x, y) = (bx * 4 + t % 4, by * 4 + t / 4);
            if x >= w || y >= h {
                continue;
            }
            let mut px = pal
                .get(((color_bits >> (t * 2)) & 3) as usize)
                .copied()
                .unwrap_or([0, 0, 0, 0]);
            match kind {
                Dxt::One => {}
                Dxt::Three => {
                    let nib = (byte(block, t / 2) >> ((t % 2) * 4)) & 0xF;
                    if let Some(a) = px.get_mut(3) {
                        *a = nib * 17;
                    }
                }
                Dxt::Five => {
                    let i = ((alpha_bits >> (t * 3)) & 7) as u32;
                    let alpha = match (i, a0 > a1) {
                        (0, _) => a0,
                        (1, _) => a1,
                        (i, true) => ((8 - i) * a0 + (i - 1) * a1) / 7,
                        (6, false) => 0,
                        (7, false) => 255,
                        (i, false) => ((6 - i) * a0 + (i - 1) * a1) / 5,
                    };
                    if let Some(a) = px.get_mut(3) {
                        *a = alpha as u8;
                    }
                }
            }
            if let Some(dst) = out.get_mut((y * w + x) * 4..(y * w + x) * 4 + 4) {
                dst.copy_from_slice(&px);
            }
        }
    }
    Ok(out)
}

/// Box-filter an RGBA image down by an integer factor (64→32 uses 2).
pub fn downscale(img: &Image, factor: usize) -> Image {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal BLP2: 4×4, one DXT1 block, c0 > c1 so all four palette
    /// entries are opaque. Indices all 0 → every texel is c0.
    fn tiny_dxt1() -> Vec<u8> {
        let mut f = vec![0u8; 1172];
        f[..4].copy_from_slice(b"BLP2");
        f[4] = 1; // version
        f[8] = 2; // compression: dxt
        f[9] = 0; // alpha depth
        f[10] = 0; // alpha type -> DXT1
        f[12] = 4; // width
        f[16] = 4; // height
        f[20..24].copy_from_slice(&1172u32.to_le_bytes()); // mip 0 offset
        f[84..88].copy_from_slice(&8u32.to_le_bytes()); // mip 0 size
        // c0 = pure red in 565 (0xF800), c1 = 0, indices = 0.
        f.extend_from_slice(&[0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        f
    }

    #[test]
    fn dxt1_decodes_a_solid_block() {
        let img = decode(&tiny_dxt1()).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
        assert!(img.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn dxt1_transparent_mode_uses_index_3() {
        let mut f = tiny_dxt1();
        // c0 <= c1 flips to 3-color + transparent; indices all 3.
        let mip = f.len() - 8;
        f[mip..mip + 4].copy_from_slice(&[0, 0, 0xFF, 0xFF]);
        f[mip + 4..].copy_from_slice(&[0xFF; 4]);
        let img = decode(&f).unwrap();
        assert!(img.rgba.chunks(4).all(|p| p[3] == 0), "all transparent");
    }

    #[test]
    fn dxt5_full_alpha_range() {
        let mut f = tiny_dxt1();
        f[9] = 8; // alpha depth
        f[10] = 7; // alpha type -> DXT5
        f[84..88].copy_from_slice(&16u32.to_le_bytes());
        // alpha0=255 alpha1=0 (a0>a1), indices 0 -> all alpha0; colors red.
        let block = [
            255, 0, 0, 0, 0, 0, 0, 0, // alpha half
            0x00, 0xF8, 0, 0, 0, 0, 0, 0, // color half
        ];
        f.truncate(1172);
        f.extend_from_slice(&block);
        let img = decode(&f).unwrap();
        assert!(img.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn garbage_is_an_error_never_a_panic() {
        assert!(decode(b"BLP2").is_err());
        assert!(decode(&[0u8; 200]).is_err());
        let mut f = tiny_dxt1();
        f[20] = 0xFF; // mip offset out of bounds
        assert!(decode(&f).is_err());
    }

    #[test]
    fn downscale_averages_blocks() {
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
        let half = downscale(&img, 2);
        assert_eq!((half.width, half.height), (1, 1));
        assert_eq!(&half.rgba, &[127, 0, 0, 255]);
    }
}
