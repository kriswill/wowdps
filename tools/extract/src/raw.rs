//! Bounds-checked reads over raw binary buffers: every fixed-width read of
//! untrusted file bytes in this crate, so a truncated or malformed file is
//! never a panic. Three contracts, for three kinds of reader:
//!
//! - The functions here: past the end is a descriptive `Err(String)`, so
//!   call sites read as ordinary `?` chains (BLTE, CASC `.idx`, TACT
//!   manifests, WDC5, PNG, BLP headers).
//! - `zero`: little-endian, a byte past the end reads 0. The geometry files
//!   (WMO, ADT, WDT, M2) and BLP blocks, whose parsers stop on a count or a
//!   size rather than at a read: a truncated file reads short.
//! - `be`: big-endian, `None` past the end. The font's tables, whose
//!   parser gives up on what it cannot read.

/// Every reader in this crate reports failures as a message string.
pub type Res<T> = Result<T, String>;

fn oob(what: &str, off: usize, len: usize, have: usize) -> String {
    format!("{what}: need {len} bytes at offset {off}, buffer has {have}")
}

/// `len` bytes starting at `off`.
pub fn take<'a>(d: &'a [u8], off: usize, len: usize, what: &str) -> Res<&'a [u8]> {
    let end = off
        .checked_add(len)
        .ok_or_else(|| oob(what, off, len, d.len()))?;
    d.get(off..end).ok_or_else(|| oob(what, off, len, d.len()))
}

/// Everything from `off` to the end.
pub fn rest<'a>(d: &'a [u8], off: usize, what: &str) -> Res<&'a [u8]> {
    d.get(off..).ok_or_else(|| oob(what, off, 0, d.len()))
}

/// A fixed-size array copied out of `d` at `off`.
pub fn array<const N: usize>(d: &[u8], off: usize, what: &str) -> Res<[u8; N]> {
    let s = take(d, off, N, what)?;
    <[u8; N]>::try_from(s).map_err(|_| oob(what, off, N, d.len()))
}

/// One byte at `off`.
pub fn byte(d: &[u8], off: usize, what: &str) -> Res<u8> {
    d.get(off)
        .copied()
        .ok_or_else(|| oob(what, off, 1, d.len()))
}

pub fn u16_le(d: &[u8], off: usize, what: &str) -> Res<u16> {
    Ok(u16::from_le_bytes(array::<2>(d, off, what)?))
}

pub fn u32_le(d: &[u8], off: usize, what: &str) -> Res<u32> {
    Ok(u32::from_le_bytes(array::<4>(d, off, what)?))
}

pub fn u32_be(d: &[u8], off: usize, what: &str) -> Res<u32> {
    Ok(u32::from_be_bytes(array::<4>(d, off, what)?))
}

pub fn i32_le(d: &[u8], off: usize, what: &str) -> Res<i32> {
    Ok(i32::from_le_bytes(array::<4>(d, off, what)?))
}

pub fn u64_le(d: &[u8], off: usize, what: &str) -> Res<u64> {
    Ok(u64::from_le_bytes(array::<8>(d, off, what)?))
}

/// A big-endian integer of `n` bytes (n <= 8), as CASC `.idx` entries store
/// offsets and sizes in odd widths.
pub fn uint_be(d: &[u8], off: usize, n: usize, what: &str) -> Res<u64> {
    let s = take(d, off, n, what)?;
    Ok(s.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b)))
}

/// Little-endian reads where a byte past the end reads 0.
pub mod zero {
    pub fn byte(b: &[u8], at: usize) -> u8 {
        b.get(at).copied().unwrap_or(0)
    }

    pub fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([byte(b, at), byte(b, at + 1)])
    }

    pub fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([
            byte(b, at),
            byte(b, at + 1),
            byte(b, at + 2),
            byte(b, at + 3),
        ])
    }

    pub fn f32_at(b: &[u8], at: usize) -> f32 {
        f32::from_bits(u32_at(b, at))
    }

    pub fn vec3_at(b: &[u8], at: usize) -> [f32; 3] {
        [f32_at(b, at), f32_at(b, at + 4), f32_at(b, at + 8)]
    }
}

/// Big-endian reads that answer `None` past the end.
pub mod be {
    pub fn u16_at(b: &[u8], at: usize) -> Option<u16> {
        let s = b.get(at..at.checked_add(2)?)?;
        Some(u16::from_be_bytes([*s.first()?, *s.get(1)?]))
    }

    pub fn i16_at(b: &[u8], at: usize) -> Option<i16> {
        u16_at(b, at).map(|v| v as i16)
    }

    pub fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        let s = b.get(at..at.checked_add(4)?)?;
        Some(u32::from_be_bytes([
            *s.first()?,
            *s.get(1)?,
            *s.get(2)?,
            *s.get(3)?,
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_bounds() {
        let d = [1u8, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(u32_le(&d, 0, "x").unwrap(), 0x0403_0201);
        assert_eq!(u32_be(&d, 0, "x").unwrap(), 0x0102_0304);
        assert_eq!(u16_le(&d, 6, "x").unwrap(), 0x0807);
        assert_eq!(byte(&d, 7, "x").unwrap(), 8);
        assert_eq!(uint_be(&d, 0, 3, "x").unwrap(), 0x0001_0203);
        assert!(byte(&d, 8, "x").is_err());
        assert!(take(&d, 6, 4, "x").is_err());
        assert!(take(&d, usize::MAX, 4, "x").is_err());
        assert!(u64_le(&d, 1, "x").is_err());
        assert_eq!(rest(&d, 6, "x").unwrap(), &[7, 8]);
    }

    #[test]
    fn lenient_and_optional_reads() {
        let d = [1u8, 2, 3, 4, 0, 0, 0x80, 0x3f];
        assert_eq!(zero::u32_at(&d, 0), 0x0403_0201);
        assert_eq!(zero::u16_at(&d, 7), 0x3f);
        assert_eq!(zero::byte(&d, 8), 0);
        assert_eq!(zero::f32_at(&d, 4), 1.0);
        assert_eq!(zero::vec3_at(&d, 4), [1.0, 0.0, 0.0]);
        assert_eq!(be::u16_at(&d, 0), Some(0x0102));
        assert_eq!(be::i16_at(&d, 6), Some(0x803f_u16 as i16));
        assert_eq!(be::u32_at(&d, 0), Some(0x0102_0304));
        assert_eq!(be::u32_at(&d, 5), None);
        assert_eq!(be::u16_at(&d, usize::MAX), None);
    }
}
