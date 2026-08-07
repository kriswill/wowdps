//! DEFLATE (RFC 1951) and zlib (RFC 1950) decompression, stdlib only.
//!
//! BLTE 'Z' chunks are zlib streams, and nothing else in the workspace may
//! pull in a compression crate, so this is a compact canonical-Huffman
//! inflater in the style of zlib's puff.c: bit-serial decode against
//! (count-per-length, symbols-in-code-order) tables. Correctness over
//! speed; it still decodes the ~190 MB encoding manifest in seconds in a
//! release build.

use crate::raw;

/// Inflate a zlib stream (2-byte header + deflate + Adler-32 trailer).
pub fn zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib: stream too short".into());
    }
    let cmf = raw::byte(data, 0, "zlib: header")?;
    let flg = raw::byte(data, 1, "zlib: header")?;
    if cmf & 0x0F != 8 {
        return Err(format!(
            "zlib: compression method {} is not deflate",
            cmf & 0x0F
        ));
    }
    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err("zlib: header check failed".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib: preset dictionaries are unsupported".into());
    }
    let mut out = Vec::new();
    let consumed = inflate(raw::rest(data, 2, "zlib: deflate stream")?, &mut out)?;
    let trailer = raw::rest(data, 2 + consumed, "zlib: adler32 trailer")?;
    if trailer.len() < 4 {
        return Err("zlib: missing adler32 trailer".into());
    }
    let want = raw::u32_be(trailer, 0, "zlib: adler32 trailer")?;
    let got = adler32(&out);
    if want != got {
        return Err(format!(
            "zlib: adler32 mismatch (want {want:08x}, got {got:08x})"
        ));
    }
    Ok(out)
}

pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 is the largest n with n*(n+1)/2*255 + n*65520 < 2^32.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    b << 16 | a
}

/// Inflate a raw deflate stream into `out`; returns bytes of input consumed.
pub fn inflate(data: &[u8], out: &mut Vec<u8>) -> Result<usize, String> {
    let mut s = Inflater {
        d: data,
        pos: 0,
        bit: 0,
        out,
    };
    loop {
        let last = s.bits(1)? != 0;
        match s.bits(2)? {
            0 => s.stored()?,
            1 => s.fixed()?,
            2 => s.dynamic()?,
            _ => return Err("deflate: reserved block type".into()),
        }
        if last {
            // Consumed input rounds up to the next byte boundary.
            return Ok(s.pos + usize::from(s.bit > 0));
        }
    }
}

struct Inflater<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u8,
    out: &'a mut Vec<u8>,
}

/// A canonical Huffman code: symbol counts per bit length, and the symbols
/// sorted by (length, symbol). Decoding walks lengths shortest-first.
struct Huff {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huff {
    /// Build from per-symbol code lengths (0 = unused).
    fn new(lengths: &[u8]) -> Result<Huff, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            // Code lengths are 4-bit in the wire format, but a corrupt
            // dynamic table could still hand us something wider.
            let c = counts
                .get_mut(l as usize)
                .ok_or("deflate: code length above 15")?;
            *c += 1;
        }
        counts[0] = 0;
        // Reject over-subscribed codes (incomplete codes are tolerated, as
        // in puff: they only fail if a missing code is actually used).
        let mut left = 1i32;
        for &c in &counts[1..] {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return Err("deflate: over-subscribed huffman code".into());
            }
        }
        // offsets[l] = where length-l symbols start = Σ counts[1..l].
        let mut offsets = [0usize; 16];
        let mut acc = 0usize;
        for (off, &count) in offsets.iter_mut().zip(counts.iter()).skip(1) {
            *off = acc;
            acc += count as usize;
        }
        let mut symbols = vec![0u16; lengths.iter().filter(|&&l| l != 0).count()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let off = offsets
                    .get_mut(l as usize)
                    .ok_or("deflate: code length above 15")?;
                let slot = symbols
                    .get_mut(*off)
                    .ok_or("deflate: huffman symbol table overflow")?;
                *slot = sym as u16;
                *off += 1;
            }
        }
        Ok(Huff { counts, symbols })
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Order in which code-length-code lengths are stored in a dynamic block.
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

impl Inflater<'_> {
    fn bits(&mut self, n: u8) -> Result<u32, String> {
        let mut v = 0u32;
        for i in 0..n {
            let byte = *self
                .d
                .get(self.pos)
                .ok_or("deflate: unexpected end of input")?;
            v |= u32::from(byte >> self.bit & 1) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(v)
    }

    fn decode(&mut self, h: &Huff) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= self.bits(1)? as i32;
            let count = i32::from(*h.counts.get(len).ok_or("deflate: code length above 15")?);
            if code - first < count {
                let i = usize::try_from(index + (code - first))
                    .map_err(|_| "deflate: negative huffman symbol index".to_string())?;
                return h
                    .symbols
                    .get(i)
                    .copied()
                    .ok_or_else(|| "deflate: huffman symbol index out of range".to_string());
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("deflate: invalid huffman code".into())
    }

    fn stored(&mut self) -> Result<(), String> {
        if self.bit > 0 {
            self.bit = 0;
            self.pos += 1;
        }
        let what = "deflate: stored block header";
        let len = raw::u16_le(self.d, self.pos, what)?;
        let nlen = raw::u16_le(self.d, self.pos + 2, what)?;
        if len != !nlen {
            return Err("deflate: stored block length check failed".into());
        }
        self.pos += 4;
        let body = raw::take(self.d, self.pos, len as usize, "deflate: stored block")?;
        self.out.extend_from_slice(body);
        self.pos += len as usize;
        Ok(())
    }

    fn fixed(&mut self) -> Result<(), String> {
        let mut lit = [0u8; 288];
        lit[..144].fill(8);
        lit[144..256].fill(9);
        lit[256..280].fill(7);
        lit[280..].fill(8);
        let dist = [5u8; 30];
        let lit = Huff::new(&lit)?;
        let dist = Huff::new(&dist)?;
        self.block(&lit, &dist)
    }

    fn dynamic(&mut self) -> Result<(), String> {
        let hlit = self.bits(5)? as usize + 257;
        let hdist = self.bits(5)? as usize + 1;
        let hclen = self.bits(4)? as usize + 4;
        if hlit > 286 || hdist > 30 {
            return Err("deflate: bad dynamic block counts".into());
        }
        let mut cl_lengths = [0u8; 19];
        for &pos in CL_ORDER.iter().take(hclen) {
            let bits = self.bits(3)? as u8;
            *cl_lengths
                .get_mut(pos)
                .ok_or("deflate: code-length order out of range")? = bits;
        }
        let cl = Huff::new(&cl_lengths)?;

        let mut lengths = vec![0u8; hlit + hdist];
        let mut i = 0;
        while i < lengths.len() {
            let sym = self.decode(&cl)?;
            let (val, repeat) = match sym {
                0..=15 => {
                    *lengths
                        .get_mut(i)
                        .ok_or("deflate: code length past table end")? = sym as u8;
                    i += 1;
                    continue;
                }
                16 => {
                    let prev = i
                        .checked_sub(1)
                        .and_then(|p| lengths.get(p).copied())
                        .ok_or("deflate: repeat with no previous length")?;
                    (prev, 3 + self.bits(2)? as usize)
                }
                17 => (0, 3 + self.bits(3)? as usize),
                _ => (0, 11 + self.bits(7)? as usize),
            };
            lengths
                .get_mut(i..i + repeat)
                .ok_or("deflate: length repeat overflows table")?
                .fill(val);
            i += repeat;
        }
        if lengths.get(256).copied().unwrap_or(0) == 0 {
            return Err("deflate: no end-of-block code".into());
        }
        let lit = Huff::new(lengths.get(..hlit).ok_or("deflate: short length table")?)?;
        let dist = Huff::new(lengths.get(hlit..).ok_or("deflate: short length table")?)?;
        self.block(&lit, &dist)
    }

    fn block(&mut self, lit: &Huff, dist: &Huff) -> Result<(), String> {
        loop {
            let sym = self.decode(lit)?;
            match sym {
                0..=255 => self.out.push(sym as u8),
                256 => return Ok(()),
                257..=285 => {
                    let idx = sym as usize - 257;
                    let bad_len = || "deflate: invalid length symbol".to_string();
                    let base = LEN_BASE.get(idx).copied().ok_or_else(bad_len)?;
                    let extra = LEN_EXTRA.get(idx).copied().ok_or_else(bad_len)?;
                    let len = base as usize + self.bits(extra)? as usize;
                    let dsym = self.decode(dist)? as usize;
                    let bad_dist = || "deflate: invalid distance symbol".to_string();
                    let dbase = DIST_BASE.get(dsym).copied().ok_or_else(bad_dist)?;
                    let dextra = DIST_EXTRA.get(dsym).copied().ok_or_else(bad_dist)?;
                    let d = dbase as usize + self.bits(dextra)? as usize;
                    if d > self.out.len() {
                        return Err("deflate: distance beyond output start".into());
                    }
                    // Byte-at-a-time: matches may overlap their own copy.
                    let start = self.out.len() - d;
                    for j in 0..len {
                        let b = self
                            .out
                            .get(start + j)
                            .copied()
                            .ok_or("deflate: back-reference past output")?;
                        self.out.push(b);
                    }
                }
                _ => return Err("deflate: invalid literal/length symbol".into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(s.get(i..i + 2).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn stored_block() {
        // BFINAL=1 BTYPE=00, then LEN/NLEN and raw "abc".
        let mut data = vec![0x01, 0x03, 0x00, 0xFC, 0xFF];
        data.extend_from_slice(b"abc");
        let mut out = Vec::new();
        assert_eq!(inflate(&data, &mut out).unwrap(), 8);
        assert_eq!(out, b"abc");
    }

    // Reference streams generated with CPython's zlib at level 9.

    #[test]
    fn zlib_fixed_block() {
        // "hello hello hello\n": fixed-huffman block with a length/distance
        // match. First deflate byte 0xcb = BFINAL|BTYPE 01.
        let data = hex("78dacb48cdc9c957c840905c0040b50687");
        assert_eq!(zlib(&data).unwrap(), b"hello hello hello\n");
    }

    #[test]
    fn zlib_dynamic_block() {
        // 2000 bytes of seeded noise over a 30-symbol alphabet: a dynamic
        // huffman block (first deflate byte 0x1d = BFINAL|BTYPE 10). The
        // stream's own adler32 pins the full output; spot values and byte
        // sum were computed independently of this implementation.
        let data = hex(
            "78da1d55d78d05210c6c65e880d0110844109864b1e59fdfeddf0a30f624fca29aa326ec2fbb54727c2f86482feb0fa1d62f7f1fe590c92cbc45f8409a19df57cb06f5f8651430bde3c7e7c64650dc4f881f12ac9f0388488f8f5f0e2f733cf429abbdef1bf8d4883a36748e7976d48ff54027cbfb8ecd0072395833103983c883a8d9a59fa1d6df664b61455e81c385feb8e1ae471b9f5fdd24549e7958730c2bfa7d40a55c62190c8f8cfbc9281b379a025ab56df9010ca45fc485e96783ee9c6e497b7572f7484913bb69693bde477a3ac9d700a58e292a689a2a94843def550dad74eca1dcdc3b0604145f000b7de1ea56356a8623d54f81e5a7aa8f6ad2a01927f3baebb343d5050db0db8b6849cd5991a65bcf146dca6d05299ccf377a7a65b9c60efeb44d4f3b9b16ad94ae1987718f5abc8009ca9e5322a8032b7496960fd2a07bac8bc30b59a9f436122929dc93b5ae39b2458ff540c3470e2f1686823dab57de4b981841ca5a30ebc0fd25f2af6c1fe7be9ec6e8977c02528453c122b696ede1767efc39d68d16b9f4a1fbd8b2eafc6c6cb746d5224c884d88902acfb98695530fe096b938080882d948cc6dabdbf2bd3391c01edc00ebfa58c46929a197e08bb67dd6453beb75639db63dce91359924da0a1d79489971dbf6e443b6289be5d288966a09e9dcd8745cf465ba221e51743267b1edca828e9f1095a0aeac75b7a9bed9555a2e9df469adae665179e72ec76e5e558ca207e57a2b7140bb43c5e992a5c829ce89a6a269619e12661d30d282639c8c5a4bc4f2ad8a39f0ad2bba6d2cb349fb3d618cf1e3b2203a5c8893d268549c2bf18b6ba6b6454135358bbe03ac347df9424c84b9de260fedaf3f5e7cef4abf56bad891c87c71bb779ac90d59545add0aed255fce56cf5e752a394b546614d5bd81b3aa119f3a478bf13cd0e6ac8dc418202670e5589255a0309c3f4302a484c882b20fbba9afd83bcda33753a1e665e4f6ed253aadca98129b159bd5f160b4b92bbf7ea4d227a625e79c4e668ca010d6123ef5ecd51c3b229d7cd50a9fcbb5438c2dd4f4f31f429f5c21fbef258ce6abb641a44ee1be5e91efcf89f5c516f839c12b77ffcb182a57619a148b84dcd1258cd08ec5ef6339ff1c672c27c172b15eee9969d82eb1279cb02f5c5d9a238327e9ed2579445271bf9f7e0aeda3a1d58bb98bf0c88de2f7b4b8f505550f65ca232a979b206de17793d06ab3772f8eb8f388a96abc5a50eee3906aee69e9037c257224b7ce42f0c68fad14a2407b7ef6ad19445e9c2666840cf4e42c8725245a0e93cce123230a835fdb6db6322459c382755ff151a04b2954852af35c581df4705b4c23501589dfe2e24e8607da3922b30ac9bf92b75f599206b3baa2aff68f7fb11e4b1641203e28d1c8aa142b536f0fa220d9fbd9fdde3547ed5a680a034f32123a922c48a6ad9be58c72244f8cdb33f1dae0c42e4ac9a3942a583a3bf82c514bd176742dcc2a13645dc213fc954c89e5a558616a5bb2257d38b3f6b26190a907f5e10b8f788f34f88afb47183f6c86e6ffc6d77e3baa9112e75c5774d39f99623ee2e8aad63511b36b942542514e31abe942d41ba7b1b7685652b52587f23551a0dfb93089547e8bc44a5e8f937235db0c4a5f17e38a9d8c3c2fbbd1fed6f6270a3398193a3579933ec722b83bb370259dbff407f1f0b880",
        );
        let out = zlib(&data).unwrap();
        assert_eq!(out.len(), 2000);
        assert_eq!(&out[..10], b"bqojhe0o r");
        assert_eq!(&out[1990..], b"phcq  jwwf");
        assert_eq!(out.iter().map(|&b| u32::from(b)).sum::<u32>(), 178273);
    }

    #[test]
    fn adler32_vectors() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"abc"), 0x024D_0127);
        assert_eq!(adler32(&[0xFF; 100_000]), 0x149A_302C);
    }

    #[test]
    fn rejects_garbage() {
        assert!(zlib(&[0x78]).is_err());
        assert!(zlib(&[0x79, 0x9C, 0, 0, 0, 0]).is_err()); // bad method
        assert!(zlib(&[0x78, 0x9D, 0, 0, 0, 0]).is_err()); // bad fcheck
        let mut out = Vec::new();
        assert!(inflate(&[0x07], &mut out).is_err()); // reserved block type
    }
}
