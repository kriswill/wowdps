//! Bounds-checked reads over raw binary buffers.
//!
//! The formats below (BLTE, CASC `.idx`, TACT manifests, WDC5) are all
//! fixed-width field soup over untrusted file bytes: a truncated or
//! malformed file must surface as an error, never a panic. These helpers
//! are the one place that turns "past the end" into a descriptive
//! `Err(String)`, so call sites read as ordinary `?` chains.

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
}
