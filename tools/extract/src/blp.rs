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

use crate::img::Image;
use crate::raw::{
    self,
    zero::{byte, u16_at},
};

/// Decode a BLP2 file's top mip to RGBA.
pub fn decode(data: &[u8]) -> Result<Image, String> {
    decode_level(data, 0)
}

/// How many mip levels the file stores (1 when it stores none beyond the
/// top), counting the offsets the header fills in.
pub fn levels(data: &[u8]) -> usize {
    if byte(data, 11) == 0 {
        return 1;
    }
    (0..16)
        .take_while(|l| raw::u32_le(data, 20 + 4 * l, "blp").is_ok_and(|o| o != 0))
        .count()
        .max(1)
}

/// Decode one mip level to RGBA: level 0 is the full image, each level
/// after it half the size (never under 1 px), as the file stores them.
pub fn decode_level(data: &[u8], level: usize) -> Result<Image, String> {
    if data.get(..4) != Some(b"BLP2") {
        return Err("blp: bad magic".into());
    }
    if level >= 16 {
        return Err(format!("blp: no mip level {level}"));
    }
    let compression = byte(data, 8);
    let alpha_depth = byte(data, 9);
    let alpha_type = byte(data, 10);
    let top_w = raw::u32_le(data, 12, "blp")? as usize;
    let top_h = raw::u32_le(data, 16, "blp")? as usize;
    // 4096 admits the UI texture atlases (2048×1024 sheets) that the
    // talent-art generator crops; icons stay far below it.
    if top_w == 0 || top_h == 0 || top_w > 4096 || top_h > 4096 {
        return Err(format!("blp: unreasonable size {top_w}x{top_h}"));
    }
    let width = (top_w >> level).max(1);
    let height = (top_h >> level).max(1);
    let offset = raw::u32_le(data, 20 + 4 * level, "blp")? as usize;
    let size = raw::u32_le(data, 84 + 4 * level, "blp")? as usize;
    if offset == 0 || size == 0 {
        return Err(format!("blp: no mip level {level}"));
    }
    let mip = data
        .get(offset..offset.saturating_add(size))
        .ok_or("blp: mip range out of bounds")?;

    let rgba = match compression {
        1 => palettized(data, mip, width, height, alpha_depth)?,
        // Alpha type 7 is DXT5; otherwise alpha deeper than one bit is DXT3
        // (its 4-bit explicit alpha: the raid target icons are depth 4),
        // and one bit or none is DXT1.
        2 => match alpha_type {
            7 => dxt(mip, width, height, Dxt::Five)?,
            _ if alpha_depth > 1 => dxt(mip, width, height, Dxt::Three)?,
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
        let pal = color_block(u16_at(block, c), u16_at(block, c + 2), kind != Dxt::One);
        let color_bits = raw::u32_le(block, c + 4, "blp").unwrap_or(0);
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

    /// A BLP2 header (1172 bytes, palette zeroed) whose mip 0 of `size`
    /// bytes follows immediately.
    fn header(
        compression: u8,
        alpha_depth: u8,
        alpha_type: u8,
        w: u32,
        h: u32,
        size: u32,
    ) -> Vec<u8> {
        let mut f = vec![0u8; 1172];
        f[..4].copy_from_slice(b"BLP2");
        f[4] = 1;
        f[8] = compression;
        f[9] = alpha_depth;
        f[10] = alpha_type;
        f[12..16].copy_from_slice(&w.to_le_bytes());
        f[16..20].copy_from_slice(&h.to_le_bytes());
        f[20..24].copy_from_slice(&1172u32.to_le_bytes());
        f[84..88].copy_from_slice(&size.to_le_bytes());
        f
    }

    fn pixels(img: &Image) -> Vec<[u8; 4]> {
        img.rgba
            .chunks(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect()
    }

    #[test]
    fn palettized_with_every_alpha_depth() {
        // 2x2; palette entry 0 = red, entry 1 = blue (BGRA on disk).
        let mut base = header(1, 0, 0, 2, 2, 4);
        base[148..152].copy_from_slice(&[0, 0, 255, 0]);
        base[152..156].copy_from_slice(&[255, 0, 0, 0]);
        let idx = [0u8, 1, 1, 0];

        let mut f = base.clone();
        f.extend_from_slice(&idx);
        let img = decode(&f).unwrap();
        assert_eq!(
            pixels(&img),
            [
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                [0, 0, 255, 255],
                [255, 0, 0, 255]
            ]
        );

        // 1-bit alpha: one byte of bits, pixel i at bit i.
        let mut f = base.clone();
        f[9] = 1;
        f[84] = 5;
        f.extend_from_slice(&idx);
        f.push(0b0101);
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(a, [255, 0, 255, 0]);
        // Missing alpha rows read as opaque.
        f.pop();
        f[84] = 4;
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(a, [255, 255, 255, 255]);

        // 4-bit alpha: one nibble per pixel, low nibble first.
        let mut f = base.clone();
        f[9] = 4;
        f[84] = 6;
        f.extend_from_slice(&idx);
        f.extend_from_slice(&[0xF0, 0x08]);
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(a, [0, 255, 136, 0]);
        f.truncate(f.len() - 2);
        f[84] = 4;
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(a, [255, 255, 255, 255]);

        // 8-bit alpha: one byte per pixel.
        let mut f = base.clone();
        f[9] = 8;
        f[84] = 8;
        f.extend_from_slice(&idx);
        f.extend_from_slice(&[1, 2, 3, 4]);
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(a, [1, 2, 3, 4]);

        // Unknown depth, short mip, short file (no palette).
        let mut f = base.clone();
        f[9] = 2;
        f.extend_from_slice(&idx);
        assert!(decode(&f).unwrap_err().contains("alpha depth 2"));
        let mut f = base.clone();
        f.extend_from_slice(&idx[..2]);
        f[84] = 2;
        assert!(decode(&f).unwrap_err().contains("too small"));
        let mut short = base[..148].to_vec();
        short.extend_from_slice(&idx);
        short[20..24].copy_from_slice(&148u32.to_le_bytes());
        assert!(decode(&short).unwrap_err().contains("no palette"));
    }

    #[test]
    fn raw_bgra_swizzles() {
        let mut f = header(3, 8, 0, 2, 1, 8);
        f.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(pixels(&decode(&f).unwrap()), [[3, 2, 1, 4], [7, 6, 5, 8]]);
        f.truncate(f.len() - 1);
        f[84] = 7;
        assert!(decode(&f).unwrap_err().contains("raw mip too small"));
    }

    #[test]
    fn header_rejections() {
        let mut f = header(5, 0, 0, 4, 4, 8);
        f.extend_from_slice(&[0; 8]);
        assert!(
            decode(&f)
                .unwrap_err()
                .contains("unsupported compression 5")
        );
        let f = header(2, 0, 0, 0, 4, 8);
        assert!(decode(&f).unwrap_err().contains("unreasonable size 0x4"));
        let f = header(2, 0, 0, 4, 5000, 8);
        assert!(decode(&f).unwrap_err().contains("unreasonable size"));
        // A DXT mip shorter than its block grid.
        let mut f = header(2, 0, 0, 4, 4, 4);
        f.extend_from_slice(&[0; 4]);
        assert!(decode(&f).unwrap_err().contains("dxt mip too small"));
        // Truncated header fields.
        assert!(decode(&b"BLP2\0\0\0\0\0\0\0\0\x04"[..]).is_err());
    }

    #[test]
    fn dxt3_at_alpha_depth_4() {
        // The raid target icons: DXT, alpha depth 4, alpha type 1. A 4-bit
        // explicit alpha is DXT3's, so the block is 16 bytes, not DXT1's 8.
        let mut f = header(2, 4, 1, 4, 4, 16);
        f.extend_from_slice(&[0x0F; 8]);
        f.extend_from_slice(&[0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        let px = pixels(&decode(&f).unwrap());
        assert_eq!(px[0], [255, 0, 0, 255]);
        assert_eq!(px[1], [255, 0, 0, 0]);
    }

    #[test]
    fn dxt3_explicit_alpha() {
        let mut f = header(2, 8, 1, 4, 4, 16);
        // Alpha nibbles 0x0F per byte: even texels opaque, odd transparent.
        f.extend_from_slice(&[0x0F; 8]);
        f.extend_from_slice(&[0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        let px = pixels(&decode(&f).unwrap());
        assert_eq!(px[0], [255, 0, 0, 255]);
        assert_eq!(px[1], [255, 0, 0, 0]);
        assert_eq!(px[15], [255, 0, 0, 0]);
    }

    /// Six bytes of 3-bit DXT5 alpha indices, texel t at bits 3t.
    fn alpha_indices(idx: [u8; 16]) -> [u8; 6] {
        let mut v = 0u64;
        for (t, i) in idx.iter().enumerate() {
            v |= u64::from(*i) << (3 * t);
        }
        let b = v.to_le_bytes();
        [b[0], b[1], b[2], b[3], b[4], b[5]]
    }

    #[test]
    fn dxt5_interpolated_alpha_both_modes() {
        let mut idx = [0u8; 16];
        for (t, i) in idx.iter_mut().enumerate() {
            *i = (t % 8) as u8;
        }
        let color = [0x00, 0xF8, 0, 0, 0, 0, 0, 0];

        // a0 > a1: eight interpolated steps.
        let mut f = header(2, 8, 7, 4, 4, 16);
        f.extend_from_slice(&[255, 0]);
        f.extend_from_slice(&alpha_indices(idx));
        f.extend_from_slice(&color);
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(&a[..8], &[255, 0, 218, 182, 145, 109, 72, 36]);

        // a0 <= a1: six steps plus explicit 0 and 255.
        let mut f = header(2, 8, 7, 4, 4, 16);
        f.extend_from_slice(&[0, 255]);
        f.extend_from_slice(&alpha_indices(idx));
        f.extend_from_slice(&color);
        let a: Vec<u8> = pixels(&decode(&f).unwrap()).iter().map(|p| p[3]).collect();
        assert_eq!(&a[..8], &[0, 255, 51, 102, 153, 204, 0, 255]);
    }

    #[test]
    fn partial_blocks_clip_to_the_image() {
        // 2x2 DXT1: one block, only its top-left texels land.
        let mut f = header(2, 0, 0, 2, 2, 8);
        f.extend_from_slice(&[0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        let img = decode(&f).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.rgba.len(), 16);
        assert!(pixels(&img).iter().all(|p| *p == [255, 0, 0, 255]));
    }
}
