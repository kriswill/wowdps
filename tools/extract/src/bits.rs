//! Little-endian bit extraction for WDC5 record data.
//!
//! Bitpacked fields start at an arbitrary bit offset and are up to 64 bits
//! wide, so a read can span 9 bytes; accumulate through u128. Reads past the
//! end of the slice yield zero bits — the client pads records the same way,
//! and trailing fields of the last record may end mid-byte.

/// Read `width` bits (0..=64) starting `bit_offset` bits into `data`.
pub fn read_bits(data: &[u8], bit_offset: usize, width: u32) -> u64 {
    debug_assert!(width <= 64);
    if width == 0 {
        return 0;
    }
    let first = bit_offset / 8;
    let shift = bit_offset % 8;
    let nbytes = (shift + width as usize).div_ceil(8);
    let mut acc: u128 = 0;
    for i in 0..nbytes {
        let b = data.get(first + i).copied().unwrap_or(0);
        acc |= (b as u128) << (8 * i);
    }
    ((acc >> shift) as u64) & mask(width)
}

/// A mask of the low `width` bits.
pub fn mask(width: u32) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// Reinterpret the low `bits` of `v` as a two's-complement signed value.
pub fn sign_extend(v: u64, bits: u32) -> i64 {
    if bits == 0 || bits >= 64 {
        return v as i64;
    }
    let m = 1u64 << (bits - 1);
    ((v & mask(bits)) ^ m).wrapping_sub(m) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_bytes() {
        let d = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_bits(&d, 0, 32), 0x1234_5678);
        assert_eq!(read_bits(&d, 8, 16), 0x3456);
        assert_eq!(read_bits(&d, 0, 8), 0x78);
    }

    #[test]
    fn unaligned_spans() {
        // bits: 0b...1101_1010_1111 laid LE
        let d = [0b1010_1111, 0b0000_1101];
        assert_eq!(read_bits(&d, 0, 4), 0b1111);
        assert_eq!(read_bits(&d, 4, 8), 0b1101_1010);
        assert_eq!(read_bits(&d, 3, 9), 0b1_1011_0101);
    }

    #[test]
    fn max_width_at_offset() {
        // 64-bit read at bit offset 4 spans 9 bytes.
        let mut d = [0u8; 9];
        d[0] = 0xF0;
        d[8] = 0x0F;
        assert_eq!(read_bits(&d, 4, 64), 0xF000_0000_0000_000F);
    }

    #[test]
    fn past_end_reads_zero() {
        let d = [0xFF];
        assert_eq!(read_bits(&d, 0, 32), 0xFF);
        assert_eq!(read_bits(&d, 16, 8), 0);
    }

    #[test]
    fn sign_extension() {
        assert_eq!(sign_extend(0b11111, 5), -1);
        assert_eq!(sign_extend(0b10000, 5), -16);
        assert_eq!(sign_extend(0b01111, 5), 15);
        assert_eq!(sign_extend(0xFFFF_FFFF, 32), -1);
        assert_eq!(sign_extend(7, 0), 7);
        assert_eq!(sign_extend(u64::MAX, 64), -1);
    }
}
