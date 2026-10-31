//! A minimal PNG writer: 8-bit RGBA, each row's filter chosen by the
//! minimum sum of absolute differences (the usual heuristic), compressed by
//! this crate's `deflate`. Stdlib-only, like everything here.
//!
//! It reads back what it writes (`decode`), and `read` and `write` take a
//! picture (`img::Image`) from and to a file.

use std::path::Path;

use crate::img::Image;

/// The Paeth predictor (PNG filter 4).
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filter one row (4 bytes a pixel) by filter type `f` against the row
/// above it.
fn filter(f: u8, row: &[u8], up: &[u8], out: &mut Vec<u8>) {
    for (i, &x) in row.iter().enumerate() {
        let a = if i >= 4 {
            row.get(i - 4).copied().unwrap_or(0)
        } else {
            0
        };
        let b = up.get(i).copied().unwrap_or(0);
        let c = if i >= 4 {
            up.get(i - 4).copied().unwrap_or(0)
        } else {
            0
        };
        out.push(match f {
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            3 => x.wrapping_sub(((a as u16 + b as u16) / 2) as u8),
            4 => x.wrapping_sub(paeth(a, b, c)),
            _ => x,
        });
    }
}

/// Filter `rows` (each `stride` bytes; `above` is the row before the first,
/// zeros for an image's first) into `out`: per row its filter byte, then
/// the row filtered by whichever of the five sums the least absolute
/// difference (the first of equals).
fn filter_rows(rows: &[u8], above: &[u8], stride: usize, out: &mut [u8]) {
    let mut up = above;
    let mut trial = Vec::with_capacity(stride);
    let mut best = Vec::with_capacity(stride);
    for (row, dst) in rows
        .chunks_exact(stride)
        .zip(out.chunks_exact_mut(stride + 1))
    {
        let mut best_cost = u64::MAX;
        let mut best_f = 0;
        for f in 0..5u8 {
            trial.clear();
            filter(f, row, up, &mut trial);
            let cost: u64 = trial.iter().map(|&v| (v as i8).unsigned_abs() as u64).sum();
            if cost < best_cost {
                best_cost = cost;
                best_f = f;
                std::mem::swap(&mut best, &mut trial);
            }
        }
        if let Some((kind, bytes)) = dst.split_first_mut() {
            *kind = best_f;
            bytes.copy_from_slice(&best);
        }
        up = row;
    }
}

/// Below this many bytes of pixels an image is filtered on one thread.
const BANDS_FROM: usize = 1 << 20;

/// Encode `rgba` (`w * h * 4` bytes, row-major) as a PNG.
///
/// A row's filter reads only it and the row above, so a large image is
/// filtered in bands of rows at once, each band starting from the last row
/// of the one before; the bytes are the same as one pass's. The Adler-32
/// sum runs beside the compression.
pub fn encode(w: usize, h: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let threads = if rgba.len() < BANDS_FROM {
        1
    } else {
        std::thread::available_parallelism().map_or(4, |n| n.get())
    };
    encode_in(w, h, rgba, threads)
}

/// `encode`, the rows filtered in `threads` bands.
fn encode_in(w: usize, h: usize, rgba: &[u8], threads: usize) -> Result<Vec<u8>, String> {
    if rgba.len() != w * h * 4 || w == 0 || h == 0 {
        return Err(format!("png: {} bytes for {w}x{h}", rgba.len()));
    }
    let stride = w * 4;
    let mut raw = vec![0u8; h * (stride + 1)];
    let zero = vec![0u8; stride];
    let band = h.div_ceil(threads.max(1));
    std::thread::scope(|s| {
        let pixels = rgba.chunks(band * stride);
        for (i, (rows, out)) in pixels.zip(raw.chunks_mut(band * (stride + 1))).enumerate() {
            let above = match (i * band).checked_sub(1) {
                Some(r) => rgba.get(r * stride..(r + 1) * stride).unwrap_or(&zero),
                None => &zero,
            };
            if threads <= 1 {
                filter_rows(rows, above, stride, out);
            } else {
                s.spawn(move || filter_rows(rows, above, stride, out));
            }
        }
    });
    let z = crate::deflate::zlib(&raw);

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// A decoded PNG. Its rows stay where the file laid them, each after its
/// filter byte (undone in place), so a render 12,000 px square is held
/// once; `rgba` reads a pixel.
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    channels: usize,
    data: Vec<u8>,
}

impl Decoded {
    /// The pixel at (`x`, `y`) as RGBA; transparent black outside the image.
    pub fn rgba(&self, x: usize, y: usize) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let at = y * (self.width * self.channels + 1) + 1 + x * self.channels;
        match self.data.get(at..at + self.channels) {
            Some(&[r, g, b, a]) => [r, g, b, a],
            Some(&[r, g, b]) => [r, g, b, 255],
            _ => [0; 4],
        }
    }

    /// The picture as RGBA, a row at a time (RGB gains an opaque alpha):
    /// what `rgba` reads pixel by pixel.
    pub fn into_image(self) -> Image {
        let line = self.width * self.channels;
        let mut rgba = Vec::with_capacity(self.width * self.height * 4);
        for y in 0..self.height {
            let at = y * (line + 1) + 1;
            let row = self.data.get(at..at + line).unwrap_or_default();
            if self.channels == 4 {
                rgba.extend_from_slice(row);
            } else {
                for &[r, g, b] in row.as_chunks::<3>().0 {
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
        }
        Image {
            width: self.width,
            height: self.height,
            rgba,
        }
    }
}

/// Decode an 8-bit, non-interlaced RGB or RGBA PNG: what `encode` writes,
/// which is all this crate reads back.
pub fn decode(png: &[u8]) -> Result<Decoded, String> {
    if png.get(..8) != Some(b"\x89PNG\r\n\x1a\n".as_slice()) {
        return Err("png: not a PNG".into());
    }
    let (mut width, mut height, mut channels) = (0, 0, 0);
    let mut z = Vec::new();
    let mut pos = 8;
    loop {
        let len = crate::raw::u32_be(png, pos, "png: chunk length")? as usize;
        let id = png.get(pos + 4..pos + 8).ok_or("png: truncated chunk")?;
        let body = png
            .get(pos + 8..pos + 8 + len)
            .ok_or("png: truncated chunk")?;
        match id {
            b"IHDR" => {
                let [
                    w0,
                    w1,
                    w2,
                    w3,
                    h0,
                    h1,
                    h2,
                    h3,
                    depth,
                    color,
                    _,
                    _,
                    interlace,
                ] = *body
                else {
                    return Err("png: IHDR is not 13 bytes".into());
                };
                width = u32::from_be_bytes([w0, w1, w2, w3]) as usize;
                height = u32::from_be_bytes([h0, h1, h2, h3]) as usize;
                channels = match (depth, color, interlace) {
                    (8, 6, 0) => 4,
                    (8, 2, 0) => 3,
                    _ => {
                        return Err(format!(
                            "png: depth {depth}, color type {color}, interlace {interlace}: only 8-bit RGB or RGBA, not interlaced"
                        ));
                    }
                };
            }
            b"IDAT" => z.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    if channels == 0 {
        return Err("png: no IHDR".into());
    }
    let mut data = crate::inflate::zlib(&z)?;
    let row_len = width * channels;
    if data.len() < (row_len + 1) * height {
        return Err(format!("png: {} bytes for {width}x{height}", data.len()));
    }
    unfilter(&mut data, row_len, channels, height)?;
    Ok(Decoded {
        width,
        height,
        channels,
        data,
    })
}

/// The picture at `path`, an 8-bit RGB or RGBA PNG (`decode`), as RGBA;
/// an error names the file.
pub fn read(path: &Path) -> Result<Image, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    decode(&bytes)
        .map(Decoded::into_image)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `img` to `path` as a PNG (`encode`), renamed into place.
pub fn write(path: &Path, img: &Image) -> Result<(), String> {
    crate::atomic::write_atomic(path, encode(img.width, img.height, &img.rgba)?)
}

/// Undo each row's filter in place: a row's filter byte, then its bytes,
/// read against the row above it (already undone).
fn unfilter(data: &mut [u8], row_len: usize, bpp: usize, height: usize) -> Result<(), String> {
    let stride = row_len + 1;
    for r in 0..height {
        let (above, rest) = data
            .split_at_mut_checked(r * stride)
            .ok_or("png: truncated image")?;
        let up: &[u8] = match r {
            0 => &[],
            _ => above.get((r - 1) * stride + 1..).unwrap_or(&[]),
        };
        let (f, row) = rest.split_first_mut().ok_or("png: truncated image")?;
        let row = row.get_mut(..row_len).ok_or("png: truncated image")?;
        for i in 0..row_len {
            let a = i
                .checked_sub(bpp)
                .and_then(|j| row.get(j).copied())
                .unwrap_or(0);
            let b = up.get(i).copied().unwrap_or(0);
            let c = i
                .checked_sub(bpp)
                .and_then(|j| up.get(j).copied())
                .unwrap_or(0);
            let Some(x) = row.get_mut(i) else { break };
            *x = match *f {
                0 => *x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                f => return Err(format!("png: row {r} has filter {f}")),
            };
        }
    }
    Ok(())
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(id);
    out.extend_from_slice(data);
    let crc = crc32(out.get(start..).unwrap_or(&[]));
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &x in data {
        crc ^= x as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_a_known_chunk() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    /// Filtered in bands, even and uneven, an image is the same bytes as
    /// filtered in one pass, and reads back to its pixels.
    #[test]
    fn bands_encode_the_same_bytes_as_one_pass() {
        let (w, h) = (97, 61);
        let mut x = 7u32;
        let rgba: Vec<u8> = (0..w * h * 4)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                // Smooth gradients with noise, so every filter wins a row.
                ((i / 4 % w) as u32 * 3 + (i / (4 * w)) as u32 * 2 + (x >> 29)) as u8
            })
            .collect();
        let one = encode_in(w, h, &rgba, 1).unwrap();
        for threads in [2, 7, 24, 61, 200] {
            assert_eq!(encode_in(w, h, &rgba, threads).unwrap(), one, "{threads}");
        }
        let d = decode(&one).unwrap();
        assert_eq!(d.rgba(5, 9), rgba[(9 * w + 5) * 4..][..4]);
        assert_eq!(encode(w, h, &rgba).unwrap(), one);
    }

    /// Undo the filters, as a decoder does.
    fn unfilter(raw: &[u8], w: usize) -> Vec<u8> {
        let stride = w * 4;
        let mut out: Vec<u8> = Vec::new();
        for (r, line) in raw.chunks_exact(stride + 1).enumerate() {
            let f = line[0];
            for i in 0..stride {
                let x = line[1 + i];
                let a = if i >= 4 { out[r * stride + i - 4] } else { 0 };
                let b = if r > 0 { out[(r - 1) * stride + i] } else { 0 };
                let c = if r > 0 && i >= 4 {
                    out[(r - 1) * stride + i - 4]
                } else {
                    0
                };
                out.push(match f {
                    1 => x.wrapping_add(a),
                    2 => x.wrapping_add(b),
                    3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                    4 => x.wrapping_add(paeth(a, b, c)),
                    _ => x,
                });
            }
        }
        out
    }

    #[test]
    fn round_trips_through_this_crates_inflate() {
        let (w, h) = (37, 23);
        // A gradient, a flat block and noise: every filter gets chosen.
        let mut x = 7u32;
        let rgba: Vec<u8> = (0..w * h * 4)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                match (i / (w * 4)) % 3 {
                    0 => (i % 256) as u8,
                    1 => 40,
                    _ => (x >> 24) as u8,
                }
            })
            .collect();
        let png = encode(w, h, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IDAT follows the signature and the 25-byte IHDR chunk.
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let raw = crate::inflate::zlib(&png[41..41 + idat_len]).unwrap();
        assert_eq!(unfilter(&raw, w), rgba);
        // And back through `decode`, every pixel.
        let d = decode(&png).unwrap();
        assert_eq!((d.width, d.height), (w, h));
        for y in 0..h {
            for x in 0..w {
                let at = (y * w + x) * 4;
                assert_eq!(d.rgba(x, y), rgba[at..at + 4], "({x}, {y})");
            }
        }
        assert_eq!(d.rgba(w, 0), [0; 4]);
    }

    #[test]
    fn decode_refuses_what_it_cannot_read() {
        assert!(decode(b"GIF89a").is_err());
        let mut png = encode(2, 2, &[9; 16]).unwrap();
        png[24] = 16; // IHDR's bit depth
        assert!(decode(&png).err().unwrap().contains("depth 16"));
    }
}
