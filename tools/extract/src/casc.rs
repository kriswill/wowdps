//! Local CASC storage: the `Data/data` directory of an installed game.
//!
//! Sixteen `.idx` journals (`BBVVVVVVVV.idx`: bucket byte + version, keep
//! the newest per bucket) map truncated 9-byte encoding keys to
//! `(archive, offset, size)` triples; `data.NNN` archives hold each file as
//! a 0x1E-byte entry header (reversed ekey, size, flags, checksums)
//! followed by its BLTE blob. A key's bucket is the nibble-folded XOR of
//! its first 9 bytes.

use crate::raw;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const ENTRY_HEADER: usize = 0x1E;

#[derive(Debug)]
pub struct LocalStore {
    dir: PathBuf,
    map: HashMap<[u8; 9], Loc>,
}

#[derive(Debug)]
struct Loc {
    archive: u16,
    offset: u32,
    size: u32,
}

pub fn bucket(key: &[u8]) -> u8 {
    let i = key.iter().take(9).fold(0u8, |a, &b| a ^ b);
    (i & 0xF) ^ (i >> 4)
}

impl LocalStore {
    pub fn open(dir: &Path) -> Result<LocalStore, String> {
        // Newest .idx per bucket.
        let mut newest: HashMap<u8, (u32, PathBuf)> = HashMap::new();
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(hex) = name.strip_suffix(".idx") else {
                continue;
            };
            if hex.len() != 10 {
                continue;
            }
            let (Some(bucket_hex), Some(version_hex)) = (hex.get(..2), hex.get(2..)) else {
                continue; // not split on a char boundary: not an .idx name
            };
            let (Ok(bucket), Ok(version)) = (
                u8::from_str_radix(bucket_hex, 16),
                u32::from_str_radix(version_hex, 16),
            ) else {
                continue;
            };
            let slot = newest.entry(bucket).or_insert((version, entry.path()));
            if version > slot.0 {
                *slot = (version, entry.path());
            }
        }
        if newest.len() != 16 {
            return Err(format!(
                "{}: found {} index buckets, expected 16",
                dir.display(),
                newest.len()
            ));
        }

        let mut map = HashMap::new();
        for (bucket_no, (_, path)) in newest {
            parse_idx(&path, bucket_no, &mut map)?;
        }
        Ok(LocalStore {
            dir: dir.to_path_buf(),
            map,
        })
    }

    pub fn entry_count(&self) -> usize {
        self.map.len()
    }

    /// Fetch the BLTE blob for an encoding key.
    pub fn read(&self, ekey: &[u8; 16]) -> Result<Vec<u8>, String> {
        let prefix: [u8; 9] = raw::array(ekey, 0, "casc: ekey prefix")?;
        let loc = self
            .map
            .get(&prefix)
            .ok_or_else(|| format!("ekey {} not in local storage", hex(&ekey[..])))?;

        let path = self.dir.join(format!("data.{:03}", loc.archive));
        let mut f = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        f.seek(SeekFrom::Start(loc.offset as u64))
            .map_err(|e| e.to_string())?;

        let mut header = [0u8; ENTRY_HEADER];
        f.read_exact(&mut header)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        // Stored key is the full ekey reversed; only 9 bytes are significant,
        // so walk the stored key backwards against the ekey's first nine.
        if header[..16]
            .iter()
            .rev()
            .zip(ekey.iter())
            .take(9)
            .any(|(a, b)| a != b)
        {
            return Err(format!(
                "archive entry key mismatch for ekey {} in {}",
                hex(&ekey[..]),
                path.display()
            ));
        }
        let total =
            u32::from_le_bytes([header[0x10], header[0x11], header[0x12], header[0x13]]) as usize;
        if total < ENTRY_HEADER || (total as u32) != loc.size {
            return Err(format!(
                "archive entry size {} disagrees with index {}",
                total, loc.size
            ));
        }
        let mut blob = vec![0u8; total - ENTRY_HEADER];
        f.read_exact(&mut blob)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(blob)
    }
}

fn parse_idx(path: &Path, bucket_no: u8, map: &mut HashMap<[u8; 9], Loc>) -> Result<(), String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let err = |what: &str| format!("{}: {what}", path.display());
    if d.len() < 0x28 {
        return Err(err("truncated idx header"));
    }
    let version = raw::u16_le(&d, 0x08, "idx: version")?;
    let file_bucket = raw::byte(&d, 0x0A, "idx: bucket byte")?;
    let spec = raw::take(&d, 0x0C, 4, "idx: layout spec")?; // size, offset, key, offset_bits
    if version != 7 || spec != [4, 5, 9, 30] {
        return Err(err(&format!(
            "unsupported idx layout (version {version}, spec {spec:?})"
        )));
    }
    if file_bucket != bucket_no {
        return Err(err("bucket byte disagrees with filename"));
    }
    let entries_size = raw::u32_le(&d, 0x20, "idx: entry block size")? as usize;
    let entries = d
        .get(0x28..0x28 + entries_size)
        .ok_or_else(|| err("entry block beyond file"))?;
    for e in entries.as_chunks::<18>().0 {
        let key: [u8; 9] = raw::array(e, 0, "idx: entry key")?;
        let packed = raw::uint_be(e, 9, 5, "idx: entry location")?;
        let loc = Loc {
            archive: (packed >> 30) as u16,
            offset: (packed & 0x3FFF_FFFF) as u32,
            size: raw::u32_le(e, 14, "idx: entry size")?,
        };
        map.insert(key, loc);
    }
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err(format!("odd-length hex string {s:?}"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            let pair = s.get(i..i + 2).ok_or_else(|| format!("bad hex {s:?}"))?;
            u8::from_str_radix(pair, 16).map_err(|_| format!("bad hex {s:?}"))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn bucket_folds_nibbles() {
        // XOR of first nine bytes = 0x37 -> (7) ^ (3) = 4.
        let key = [0x37, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(bucket(&key), 0x4);
        assert_eq!(bucket(&[0u8; 9]), 0);
    }

    #[test]
    fn idx_and_archive_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wowdps-casc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // One file: ekey with bucket B, stored in data.001 at offset 0x40.
        let mut ekey = [0u8; 16];
        ekey[..4].copy_from_slice(b"key1");
        let b = bucket(&ekey);

        let payload = b"BLTEdata-pretend";
        let total = (ENTRY_HEADER + payload.len()) as u32;

        // Archive: padding to 0x40, then entry header + payload.
        let mut archive = vec![0u8; 0x40];
        let mut rev = ekey;
        rev.reverse();
        archive.extend_from_slice(&rev);
        archive.extend_from_slice(&total.to_le_bytes());
        archive.extend_from_slice(&[0u8; 10]); // flags + checksums
        archive.extend_from_slice(payload);
        std::fs::write(dir.join("data.001"), &archive).unwrap();

        // 16 idx files; the right bucket holds our entry.
        for bucket_no in 0..16u8 {
            let mut f = Vec::new();
            f.extend_from_slice(&0x10u32.to_le_bytes()); // header hash size
            f.extend_from_slice(&0u32.to_le_bytes()); // header hash (unchecked)
            f.extend_from_slice(&7u16.to_le_bytes());
            f.push(bucket_no);
            f.push(0);
            f.extend_from_slice(&[4, 5, 9, 30]);
            f.extend_from_slice(&0x4000000000u64.to_le_bytes());
            f.extend_from_slice(&[0u8; 8]); // pad to 0x20
            let entries: u32 = if bucket_no == b { 18 } else { 0 };
            f.extend_from_slice(&entries.to_le_bytes());
            f.extend_from_slice(&0u32.to_le_bytes()); // entries hash (unchecked)
            if bucket_no == b {
                f.extend_from_slice(&ekey[..9]);
                let packed: u64 = 1 << 30 | 0x40; // archive 1, offset 0x40
                f.extend_from_slice(&packed.to_be_bytes()[3..]);
                f.extend_from_slice(&total.to_le_bytes());
            }
            std::fs::write(dir.join(format!("{bucket_no:02x}00000001.idx")), &f).unwrap();
        }

        let store = LocalStore::open(&dir).unwrap();
        assert_eq!(store.entry_count(), 1);
        assert_eq!(store.read(&ekey).unwrap(), payload);

        let mut missing = [0xFFu8; 16];
        missing[0] = 1;
        assert!(store.read(&missing).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// One `.idx` journal: version 7, the standard layout spec, and the
    /// given `(ekey, packed archive<<30 | offset, size)` entries.
    pub(crate) fn idx_bytes(bucket_no: u8, entries: &[([u8; 16], u64, u32)]) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&0x10u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&7u16.to_le_bytes());
        f.push(bucket_no);
        f.push(0);
        f.extend_from_slice(&[4, 5, 9, 30]);
        f.extend_from_slice(&0x4000000000u64.to_le_bytes());
        f.extend_from_slice(&[0u8; 8]);
        f.extend_from_slice(&((entries.len() * 18) as u32).to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        for (ekey, packed, size) in entries {
            f.extend_from_slice(&ekey[..9]);
            f.extend_from_slice(&packed.to_be_bytes()[3..]);
            f.extend_from_slice(&size.to_le_bytes());
        }
        f
    }

    /// A fresh store directory with all sixteen journals (each entry filed
    /// under its key's bucket), `tweak` applied to every journal first.
    fn write_store(
        name: &str,
        entries: &[([u8; 16], u64, u32)],
        tweak: impl Fn(u8, &mut Vec<u8>),
    ) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wowdps-casc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for b in 0..16u8 {
            let mine: Vec<_> = entries
                .iter()
                .filter(|e| bucket(&e.0) == b)
                .copied()
                .collect();
            let mut f = idx_bytes(b, &mine);
            tweak(b, &mut f);
            std::fs::write(dir.join(format!("{b:02x}00000001.idx")), &f).unwrap();
        }
        dir
    }

    pub(crate) fn archive_entry(ekey: &[u8; 16], total: u32, payload: &[u8]) -> Vec<u8> {
        let mut rev = *ekey;
        rev.reverse();
        let mut e = rev.to_vec();
        e.extend_from_slice(&total.to_le_bytes());
        e.extend_from_slice(&[0u8; 10]);
        e.extend_from_slice(payload);
        e
    }

    #[test]
    fn open_ignores_foreign_files_and_keeps_the_newest_journal() {
        let dir = write_store("names", &[], |_, _| {});
        std::fs::write(dir.join("junk.idx"), b"x").unwrap();
        std::fs::write(dir.join("zz00000001.idx"), b"x").unwrap();
        std::fs::write(dir.join("0\u{e9}0000000.idx"), b"x").unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        // Older and newer journals of bucket 0, all valid.
        std::fs::write(dir.join("0000000000.idx"), idx_bytes(0, &[])).unwrap();
        std::fs::write(dir.join("0000000002.idx"), idx_bytes(0, &[])).unwrap();
        let store = LocalStore::open(&dir).unwrap();
        assert_eq!(store.entry_count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_needs_all_sixteen_buckets() {
        let dir = write_store("fifteen", &[], |_, _| {});
        std::fs::remove_file(dir.join("0500000001.idx")).unwrap();
        assert!(
            LocalStore::open(&dir)
                .unwrap_err()
                .contains("found 15 index buckets")
        );
        assert!(LocalStore::open(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    type Tweak = fn(u8, &mut Vec<u8>);

    #[test]
    fn journal_rejections() {
        let cases: [(&str, Tweak, &str); 4] = [
            (
                "short",
                |b, f| {
                    if b == 3 {
                        f.truncate(0x20)
                    }
                },
                "truncated idx header",
            ),
            (
                "version",
                |b, f| {
                    if b == 3 {
                        f[8] = 6
                    }
                },
                "unsupported idx layout",
            ),
            (
                "bucket",
                |b, f| {
                    if b == 3 {
                        f[0x0A] = 4
                    }
                },
                "bucket byte disagrees",
            ),
            (
                "entries",
                |b, f| {
                    if b == 3 {
                        f[0x20..0x24].copy_from_slice(&0x1000u32.to_le_bytes());
                    }
                },
                "entry block beyond file",
            ),
        ];
        for (name, tweak, msg) in cases {
            let dir = write_store(name, &[], tweak);
            let err = LocalStore::open(&dir).unwrap_err();
            assert!(err.contains(msg), "{name}: {err}");
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn archive_entries_must_agree_with_the_journal() {
        let mut good = [0u8; 16];
        good[..4].copy_from_slice(b"good");
        let mut wrong = [0u8; 16];
        wrong[..4].copy_from_slice(b"wrng");
        let mut other = [0u8; 16];
        other[..4].copy_from_slice(b"othr");
        let mut short = [0u8; 16];
        short[..4].copy_from_slice(b"shrt");
        let mut lost = [0u8; 16];
        lost[..4].copy_from_slice(b"lost");
        let payload = b"payload!";
        let total = (ENTRY_HEADER + payload.len()) as u32;

        let mut archive = vec![0u8; 0x40];
        archive.extend_from_slice(&archive_entry(&good, total, payload));
        archive.resize(0x100, 0);
        archive.extend_from_slice(&archive_entry(&other, total, payload));
        archive.resize(0x200, 0);
        archive.extend_from_slice(&archive_entry(&short, total - 4, &payload[..4]));

        let dir = write_store(
            "archive",
            &[
                (good, 1 << 30 | 0x40, total),
                (wrong, 1 << 30 | 0x100, total),
                (short, 1 << 30 | 0x200, total),
                (lost, 2 << 30, total),
            ],
            |_, _| {},
        );
        std::fs::write(dir.join("data.001"), &archive).unwrap();
        let store = LocalStore::open(&dir).unwrap();
        assert_eq!(store.entry_count(), 4);
        assert_eq!(store.read(&good).unwrap(), payload);
        assert!(store.read(&wrong).unwrap_err().contains("key mismatch"));
        assert!(
            store
                .read(&short)
                .unwrap_err()
                .contains("disagrees with index")
        );
        assert!(store.read(&lost).unwrap_err().contains("data.002"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hex_helpers() {
        assert_eq!(hex(&[0xAB, 0x01]), "ab01");
        assert_eq!(unhex("ab01").unwrap(), vec![0xAB, 0x01]);
        assert!(unhex("abc").unwrap_err().contains("odd-length"));
        assert!(unhex("zz").unwrap_err().contains("bad hex"));
    }
}
