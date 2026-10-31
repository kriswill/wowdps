//! DEFLATE compression (RFC 1951), the counterpart of `inflate`: LZ77 over
//! a 32 KiB window with hash chains, coded with the fixed Huffman tables
//! (block type 1). No dynamic tables: simple, exact, and enough to take the
//! floor renders' PNGs from raw size to a fraction of it.

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// How many earlier positions with the same three-byte hash to try.
const CHAIN: usize = 32;
const HASH_BITS: u32 = 15;

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
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

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    /// `count` bits of `value`, least significant first.
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= (value as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code: written most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        let rev = code.reverse_bits() >> (32 - len);
        self.put(rev, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// The fixed literal/length code for a symbol 0..=287.
fn literal(bits: &mut Bits, sym: u32) {
    match sym {
        0..=143 => bits.code(0x30 + sym, 8),
        144..=255 => bits.code(0x190 + (sym - 144), 9),
        256..=279 => bits.code(sym - 256, 7),
        _ => bits.code(0xC0 + (sym - 280), 8),
    }
}

/// The index of the last table entry whose base is at most `v`.
fn bucket(base: &[u16], v: usize) -> usize {
    base.iter().rposition(|&b| b as usize <= v).unwrap_or(0)
}

fn emit_match(bits: &mut Bits, len: usize, dist: usize) {
    let li = bucket(&LENGTH_BASE, len);
    literal(bits, 257 + li as u32);
    let base = LENGTH_BASE.get(li).copied().unwrap_or(3) as usize;
    let extra = LENGTH_EXTRA.get(li).copied().unwrap_or(0) as u32;
    if extra > 0 {
        bits.put((len - base) as u32, extra);
    }
    let di = bucket(&DIST_BASE, dist);
    bits.code(di as u32, 5);
    let base = DIST_BASE.get(di).copied().unwrap_or(1) as usize;
    let extra = DIST_EXTRA.get(di).copied().unwrap_or(0) as u32;
    if extra > 0 {
        bits.put((dist - base) as u32, extra);
    }
}

/// How many leading bytes `a` and `b` share, compared eight at a time.
fn common(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let (a, b) = (
        a.get(..n).unwrap_or_default(),
        b.get(..n).unwrap_or_default(),
    );
    let ((wa, ra), (wb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    for (k, (x, y)) in wa.iter().zip(wb).enumerate() {
        let diff = u64::from_le_bytes(*x) ^ u64::from_le_bytes(*y);
        if diff != 0 {
            return k * 8 + (diff.trailing_zeros() / 8) as usize;
        }
    }
    wa.len() * 8 + ra.iter().zip(rb).take_while(|(x, y)| x == y).count()
}

fn hash(data: &[u8], i: usize) -> usize {
    let b = |k: usize| data.get(i + k).copied().unwrap_or(0) as u32;
    let v = (b(0) << 16) | (b(1) << 8) | b(2);
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Compress `data` as one fixed-Huffman DEFLATE block.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits {
        out: Vec::with_capacity(data.len() / 3),
        acc: 0,
        n: 0,
    };
    bits.put(1, 1); // final block
    bits.put(1, 2); // fixed Huffman
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; WINDOW];
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + MIN_MATCH <= data.len() {
            let h = hash(data, i);
            if let (Some(p), Some(hd)) = (prev.get_mut(i % WINDOW), head.get_mut(h)) {
                *p = *hd;
                *hd = i;
            }
        }
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + MIN_MATCH <= data.len() {
            let mut cand = head.get(hash(data, i)).copied().unwrap_or(usize::MAX);
            let mut tries = 0;
            let max = MAX_MATCH.min(data.len() - i);
            while cand != usize::MAX && i - cand <= WINDOW && tries < CHAIN {
                // A candidate that differs at the best length so far cannot
                // beat it: its length is not measured, and nothing else
                // changes.
                let may_beat = data.get(cand + best_len) == data.get(i + best_len);
                let len = if may_beat {
                    data.get(cand..cand + max)
                        .zip(data.get(i..i + max))
                        .map_or(0, |(a, b)| common(a, b))
                } else {
                    0
                };
                if len > best_len {
                    best_len = len;
                    best_dist = i - cand;
                    if len == max {
                        break;
                    }
                }
                let next = prev.get(cand % WINDOW).copied().unwrap_or(usize::MAX);
                // A chain entry older than its slot's last writer is stale.
                if next == usize::MAX || next >= cand {
                    break;
                }
                cand = next;
                tries += 1;
            }
        }
        if best_len >= MIN_MATCH {
            emit_match(&mut bits, best_len, best_dist);
            for k in i..i + best_len {
                insert(&mut head, &mut prev, k);
            }
            i += best_len;
        } else {
            literal(&mut bits, data.get(i).copied().unwrap_or(0) as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    literal(&mut bits, 256);
    bits.finish()
}

/// A zlib stream (RFC 1950) around `deflate`; a large input's Adler-32 sum
/// is taken on a thread of its own while it compresses.
pub fn zlib(data: &[u8]) -> Vec<u8> {
    let (body, sum) = if data.len() < 1 << 20 {
        (deflate(data), crate::inflate::adler32(data))
    } else {
        std::thread::scope(|s| {
            let sum = s.spawn(|| crate::inflate::adler32(data));
            let body = deflate(data);
            let sum = sum.join().unwrap_or_else(|_| crate::inflate::adler32(data));
            (body, sum)
        })
    };
    let mut out = vec![0x78, 0x01];
    out.extend(body);
    out.extend_from_slice(&sum.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) {
        let z = zlib(data);
        assert_eq!(crate::inflate::zlib(&z).unwrap(), data);
    }

    #[test]
    fn round_trips_through_this_crates_inflate() {
        round_trip(b"");
        round_trip(b"a");
        round_trip(b"abcabcabcabcabcabcabcabc, and then something else entirely");
        // Long runs, distances past every extra-bit bucket, max-length matches.
        let mut big = Vec::new();
        let mut x = 12345u32;
        for i in 0..200_000u32 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            big.push(if i % 7000 < 3000 {
                0
            } else {
                (x >> 24) as u8 % 5
            });
        }
        round_trip(&big);
        let pattern: Vec<u8> = (0..40_000).map(|i| (i % 251) as u8).collect();
        round_trip(&pattern);
        // Past a MiB, the sum is taken on a thread of its own.
        let large: Vec<u8> = (0..(1 << 20) + 4099)
            .map(|i| ((i % 253) ^ (i / 997)) as u8)
            .collect();
        round_trip(&large);
    }

    /// Eight at a time, the shared prefix is the byte-by-byte one: first
    /// difference in every lane of a word, in the tail, none, and uneven
    /// lengths.
    #[test]
    fn a_shared_prefix_counts_as_byte_by_byte() {
        let plain = |a: &[u8], b: &[u8]| a.iter().zip(b).take_while(|(x, y)| x == y).count();
        let base: Vec<u8> = (0..258u32).map(|i| (i * 37 % 256) as u8).collect();
        for len in [0, 1, 7, 8, 9, 63, 64, 65, 257, 258] {
            let a = &base[..len];
            assert_eq!(common(a, a), len);
            for at in 0..len {
                let mut b = a.to_vec();
                b[at] ^= 0x10;
                assert_eq!(common(a, &b), plain(a, &b), "len {len} at {at}");
                assert_eq!(common(a, &b[..at]), at);
            }
        }
    }

    #[test]
    fn repetition_compresses() {
        let data = vec![7u8; 100_000];
        assert!(zlib(&data).len() < 2_000);
    }
}
