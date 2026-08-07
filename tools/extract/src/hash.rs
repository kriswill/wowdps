//! Bob Jenkins' lookup3 `hashlittle2`, the "Jenkins96" hash TACT uses for
//! filename lookups in the root manifest (and for .idx checksums).
//!
//! Paths are normalized (uppercase, `/` -> `\`) before hashing; the root's
//! 8-byte name hash combines both 32-bit results.

fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    *a = a.wrapping_sub(*c) ^ c.rotate_left(4);
    *c = c.wrapping_add(*b);
    *b = b.wrapping_sub(*a) ^ a.rotate_left(6);
    *a = a.wrapping_add(*c);
    *c = c.wrapping_sub(*b) ^ b.rotate_left(8);
    *b = b.wrapping_add(*a);
    *a = a.wrapping_sub(*c) ^ c.rotate_left(16);
    *c = c.wrapping_add(*b);
    *b = b.wrapping_sub(*a) ^ a.rotate_left(19);
    *a = a.wrapping_add(*c);
    *c = c.wrapping_sub(*b) ^ b.rotate_left(4);
    *b = b.wrapping_add(*a);
}

fn fin(a: &mut u32, b: &mut u32, c: &mut u32) {
    *c ^= *b;
    *c = c.wrapping_sub(b.rotate_left(14));
    *a ^= *c;
    *a = a.wrapping_sub(c.rotate_left(11));
    *b ^= *a;
    *b = b.wrapping_sub(a.rotate_left(25));
    *c ^= *b;
    *c = c.wrapping_sub(b.rotate_left(16));
    *a ^= *c;
    *a = a.wrapping_sub(c.rotate_left(4));
    *b ^= *a;
    *b = b.wrapping_sub(a.rotate_left(14));
    *c ^= *b;
    *c = c.wrapping_sub(b.rotate_left(24));
}

/// lookup3 hashlittle2 with both seeds zero; returns (pc, pb).
pub fn hashlittle2(data: &[u8]) -> (u32, u32) {
    let init = 0xDEAD_BEEF_u32.wrapping_add(data.len() as u32);
    let (mut a, mut b, mut c) = (init, init, init);

    let mut rest = data;
    while rest.len() > 12 {
        let w = |i: usize| u32::from_le_bytes(rest[i * 4..i * 4 + 4].try_into().unwrap());
        a = a.wrapping_add(w(0));
        b = b.wrapping_add(w(1));
        c = c.wrapping_add(w(2));
        mix(&mut a, &mut b, &mut c);
        rest = &rest[12..];
    }

    if rest.is_empty() {
        return (c, b);
    }
    let mut tail = [0u8; 12];
    tail[..rest.len()].copy_from_slice(rest);
    let w = |i: usize| u32::from_le_bytes(tail[i * 4..i * 4 + 4].try_into().unwrap());
    // lookup3's tail switch zero-fills exactly the missing bytes; with a
    // zero-padded buffer the three-word adds are equivalent.
    a = a.wrapping_add(w(0));
    b = b.wrapping_add(w(1));
    c = c.wrapping_add(w(2));
    fin(&mut a, &mut b, &mut c);
    (c, b)
}

/// The 64-bit name hash stored in WoW's root manifest for a game path.
pub fn name_hash(path: &str) -> u64 {
    let normalized: Vec<u8> = path
        .bytes()
        .map(|b| {
            if b == b'/' {
                b'\\'
            } else {
                b.to_ascii_uppercase()
            }
        })
        .collect();
    let (pc, pb) = hashlittle2(&normalized);
    u64::from(pb) << 32 | u64::from(pc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From lookup3.c's self-test (driver5): hashlittle2 of "" with zero
    /// seeds yields pc=0xdeadbeef pb=0xdeadbeef.
    #[test]
    fn empty() {
        assert_eq!(hashlittle2(b""), (0xDEAD_BEEF, 0xDEAD_BEEF));
    }

    /// lookup3.c documents hashlittle("Four score and seven years ago", 0)
    /// = 0x17770551; hashlittle is hashlittle2's pc.
    #[test]
    fn four_score() {
        let (pc, _) = hashlittle2(b"Four score and seven years ago");
        assert_eq!(pc, 0x1777_0551);
    }

    #[test]
    fn normalization() {
        assert_eq!(
            name_hash("dbfilesclient/skilllineability.db2"),
            name_hash("DBFilesClient\\SkillLineAbility.DB2"),
        );
    }
}
