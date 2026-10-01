//! A Wayland output's name (Hyprland's `DP-1`) as the UUID GPUI keys its
//! displays by (spike S4). GPUI exposes no output name, but on Wayland a
//! display's `uuid()` is the RFC 4122 version 5 UUID of its name in the DNS
//! namespace (`gpui-pre-linux` `wayland/display.rs`), so recomputing it
//! finds the display EXACTLY — never by bounds, which GPUI derives wrongly
//! at fractional scale and under a transform (DP-1 is rotated).
//!
//! The SHA-1 is here, twenty lines, rather than a `uuid` dependency: the
//! UUID is compared as bytes, and this is the only hash wowdps needs.

/// RFC 4122's DNS namespace, `6ba7b810-9dad-11d1-80b4-00c04fd430c8`.
const NAMESPACE_DNS: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

/// The UUIDv5 of an output's name, as GPUI's `PlatformDisplay::uuid` holds
/// it for that output: compare it with `display.uuid()?.as_bytes()`.
pub fn output_uuid(name: &str) -> [u8; 16] {
    let mut input = NAMESPACE_DNS.to_vec();
    input.extend_from_slice(name.as_bytes());
    let digest = sha1(&input);
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(digest.get(..16).unwrap_or(&[0; 16]));
    // Version 5 in the high nibble of byte 6, RFC 4122's variant in byte 8.
    if let Some(b) = uuid.get_mut(6) {
        *b = (*b & 0x0F) | 0x50;
    }
    if let Some(b) = uuid.get_mut(8) {
        *b = (*b & 0x3F) | 0x80;
    }
    uuid
}

/// SHA-1 (FIPS 180-4), for the one short message a UUIDv5 hashes.
fn sha1(message: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut data = message.to_vec();
    let bits = (message.len() as u64).wrapping_mul(8);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bits.to_be_bytes());
    for block in data.as_chunks::<64>().0 {
        // The message schedule: sixteen words of the block, then each new one
        // from the words 3, 8, 14 and 16 back.
        let mut w: Vec<u32> = block
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_be_bytes(*word))
            .collect();
        while w.len() < 80 {
            let back = |k: usize| w.get(w.len() - k).copied().unwrap_or(0);
            let next = (back(3) ^ back(8) ^ back(14) ^ back(16)).rotate_left(1);
            w.push(next);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_matches_the_fips_vectors() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        // Two blocks: the padding spills past the first.
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    /// Python's documented example: uuid5(NAMESPACE_DNS, "python.org").
    #[test]
    fn output_uuid_is_rfc_4122_version_5() {
        assert_eq!(
            hex(&output_uuid("python.org")),
            "886313e13b8a53729b900c9aee199e5d"
        );
        let dp1 = output_uuid("DP-1");
        assert_eq!(dp1[6] >> 4, 5, "version 5");
        assert_eq!(dp1[8] >> 6, 0b10, "RFC 4122 variant");
        assert_ne!(dp1, output_uuid("DP-3"));
    }
}
