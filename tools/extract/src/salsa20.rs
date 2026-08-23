//! Salsa20 stream cipher, as used by BLTE 'E' chunks (TACT encryption).
//!
//! CASC uses 16-byte keys, so the state is built with the 128-bit-key
//! constants ("expand 16-byte k") and the key material repeated in both key
//! slots, exactly as the client (and CascLib) does. The caller supplies the
//! 8-byte nonce (the chunk IV, zero-padded, XORed with the chunk index).

const TAU: [u32; 4] = [
    u32::from_le_bytes(*b"expa"),
    u32::from_le_bytes(*b"nd 1"),
    u32::from_le_bytes(*b"6-by"),
    u32::from_le_bytes(*b"te k"),
];

/// XOR `data` in place with the Salsa20/20 keystream.
pub fn apply(key: &[u8; 16], nonce: &[u8; 8], data: &mut [u8]) {
    // Both inputs are fixed-size arrays, so the chunking below always
    // yields exactly the words asked for.
    let words = |src: &[u8], out: &mut [u32]| {
        for (o, c) in out.iter_mut().zip(src.as_chunks::<4>().0) {
            *o = u32::from_le_bytes(*c);
        }
    };
    let mut k = [0u32; 4];
    words(key, &mut k);
    let mut n = [0u32; 2];
    words(nonce, &mut n);
    let mut state = [
        TAU[0], k[0], k[1], k[2], k[3], TAU[1], n[0], n[1], 0, 0, TAU[2], k[0], k[1], k[2], k[3],
        TAU[3],
    ];

    for block in data.chunks_mut(64) {
        let keystream = core(&state);
        for (b, ks) in block
            .iter_mut()
            .zip(keystream.iter().flat_map(|w| w.to_le_bytes()))
        {
            *b ^= ks;
        }
        state[8] = state[8].wrapping_add(1);
        if state[8] == 0 {
            state[9] = state[9].wrapping_add(1);
        }
    }
}

fn core(input: &[u32; 16]) -> [u32; 16] {
    let mut x = *input;
    // The quarter-round takes its four state slots as literals so every
    // index is a compile-time constant into the fixed-size state.
    macro_rules! qr {
        ($a:literal, $b:literal, $c:literal, $d:literal) => {{
            x[$b] ^= x[$a].wrapping_add(x[$d]).rotate_left(7);
            x[$c] ^= x[$b].wrapping_add(x[$a]).rotate_left(9);
            x[$d] ^= x[$c].wrapping_add(x[$b]).rotate_left(13);
            x[$a] ^= x[$d].wrapping_add(x[$c]).rotate_left(18);
        }};
    }
    for _ in 0..10 {
        // column round
        qr!(0, 4, 8, 12);
        qr!(5, 9, 13, 1);
        qr!(10, 14, 2, 6);
        qr!(15, 3, 7, 11);
        // row round
        qr!(0, 1, 2, 3);
        qr!(5, 6, 7, 4);
        qr!(10, 11, 8, 9);
        qr!(15, 12, 13, 14);
    }
    for (o, i) in x.iter_mut().zip(input) {
        *o = o.wrapping_add(*i);
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ECRYPT verified test vector, set 6 vector 0 of the 128-bit-key
    /// suite: key 80.., IV 0, first keystream bytes.
    #[test]
    fn ecrypt_vector() {
        let mut key = [0u8; 16];
        key[0] = 0x80;
        let nonce = [0u8; 8];
        let mut data = [0u8; 16];
        apply(&key, &nonce, &mut data);
        // stream[0..16] for this vector
        let expect: [u8; 16] = [
            0x4D, 0xFA, 0x5E, 0x48, 0x1D, 0xA2, 0x3E, 0xA0, 0x9A, 0x31, 0x02, 0x20, 0x50, 0x85,
            0x99, 0x36,
        ];
        assert_eq!(data, expect);
    }

    #[test]
    fn roundtrip() {
        let key = *b"0123456789abcdef";
        let nonce = *b"nonce!!!";
        let mut data = b"the quick brown fox jumps over the lazy dog, twice around".to_vec();
        let orig = data.clone();
        apply(&key, &nonce, &mut data);
        assert_ne!(data, orig);
        apply(&key, &nonce, &mut data);
        assert_eq!(data, orig);
    }
}
