//! The varint coding the history store's two binary tiers share — the
//! series tier (`series`, v39) and the replay tier (`replay`, v45): LEB128
//! varints, zigzag for signed values, length-prefixed strings, and a
//! bounds-checked cursor whose every read is `None` past the end, so a
//! decoder never panics and never allocates past the bytes in hand.

/// `v` as an unsigned LEB128 varint.
pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// `v` zigzagged into a varint: small magnitudes either side of 0 stay short.
pub(crate) fn put_zz(out: &mut Vec<u8>, v: i64) {
    put_varint(out, ((v << 1) ^ (v >> 63)) as u64);
}

/// A string: its byte length as a varint, then its UTF-8.
pub(crate) fn put_str(out: &mut Vec<u8>, s: &str) {
    put_varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// An option: a byte 0 for none, else a byte 1 and the value's varint.
pub(crate) fn put_opt(out: &mut Vec<u8>, v: Option<u64>) {
    match v {
        Some(v) => {
            out.push(1);
            put_varint(out, v);
        }
        None => out.push(0),
    }
}

/// A bounds-checked cursor: every read is `None` past the end.
pub(crate) struct Cur<'a> {
    b: &'a [u8],
}

impl<'a> Cur<'a> {
    pub(crate) fn new(b: &'a [u8]) -> Self {
        Self { b }
    }

    /// The bytes not yet read.
    pub(crate) fn left(&self) -> usize {
        self.b.len()
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        let (&first, rest) = self.b.split_first()?;
        self.b = rest;
        Some(first)
    }

    pub(crate) fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            v |= u64::from(byte & 0x7f).checked_shl(shift)?;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    /// A zigzagged varint.
    pub(crate) fn zz(&mut self) -> Option<i64> {
        let v = self.varint()?;
        Some((v >> 1) as i64 ^ -((v & 1) as i64))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        u32::try_from(self.varint()?).ok()
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        i32::try_from(self.zz()?).ok()
    }

    pub(crate) fn usize(&mut self) -> Option<usize> {
        usize::try_from(self.varint()?).ok()
    }

    /// A count of items each at least `min` bytes long: more than the
    /// bytes left could hold is a lie, refused before any allocation.
    pub(crate) fn count(&mut self, min: usize) -> Option<usize> {
        let n = self.usize()?;
        (n.saturating_mul(min) <= self.b.len()).then_some(n)
    }

    pub(crate) fn str(&mut self) -> Option<String> {
        let len = self.usize()?;
        if len > self.b.len() {
            return None;
        }
        let (s, rest) = self.b.split_at(len);
        self.b = rest;
        String::from_utf8(s.to_vec()).ok()
    }

    /// Every byte read.
    pub(crate) fn done(&self) -> bool {
        self.b.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_zigzags_and_strings_round_trip() {
        let mut out = Vec::new();
        for v in [0, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            put_varint(&mut out, v);
        }
        for v in [0, -1, 1, -64, 64, i64::MIN, i64::MAX] {
            put_zz(&mut out, v);
        }
        put_str(&mut out, "Ula'tek");
        put_opt(&mut out, Some(7));
        put_opt(&mut out, None);
        let mut c = Cur::new(&out);
        for v in [0, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            assert_eq!(c.varint(), Some(v));
        }
        for v in [0, -1, 1, -64, 64, i64::MIN, i64::MAX] {
            assert_eq!(c.zz(), Some(v));
        }
        assert_eq!(c.str().as_deref(), Some("Ula'tek"));
        assert_eq!((c.u8(), c.varint(), c.u8()), (Some(1), Some(7), Some(0)));
        assert!(c.done());
        assert_eq!(c.u8(), None, "past the end is None");
    }

    #[test]
    fn a_count_the_bytes_cannot_hold_is_refused() {
        let mut out = Vec::new();
        put_varint(&mut out, 1_000_000);
        out.extend([0; 10]);
        assert_eq!(Cur::new(&out).count(1), None);
        let mut short = Vec::new();
        put_varint(&mut short, 3);
        short.extend([0; 3]);
        assert_eq!(Cur::new(&short).count(1), Some(3));
        // A varint running past 64 bits, and a string longer than its bytes.
        assert_eq!(Cur::new(&[0xff; 11]).varint(), None);
        assert_eq!(Cur::new(&[5, b'a']).str(), None);
    }
}
