//! Local CASC storage: the `Data/data` directory of an installed game.
//!
//! Sixteen `.idx` journals (`BBVVVVVVVV.idx`: bucket byte + version, keep
//! the newest per bucket) map truncated 9-byte encoding keys to
//! `(archive, offset, size)` triples; `data.NNN` archives hold each file as
//! a 0x1E-byte entry header (reversed ekey, size, flags, checksums)
//! followed by its BLTE blob. A key's bucket is the nibble-folded XOR of
//! its first 9 bytes.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const ENTRY_HEADER: usize = 0x1E;

pub struct LocalStore {
    dir: PathBuf,
    map: HashMap<[u8; 9], Loc>,
}

struct Loc {
    archive: u16,
    offset: u32,
    size: u32,
}

pub fn bucket(key: &[u8]) -> u8 {
    let i = key[..9].iter().fold(0u8, |a, &b| a ^ b);
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
            let (Ok(bucket), Ok(version)) = (
                u8::from_str_radix(&hex[..2], 16),
                u32::from_str_radix(&hex[2..], 16),
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
        let prefix: [u8; 9] = ekey[..9].try_into().unwrap();
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
        // Stored key is the full ekey reversed; only 9 bytes are significant.
        for i in 0..9 {
            if header[15 - i] != ekey[i] {
                return Err(format!(
                    "archive entry key mismatch for ekey {} in {}",
                    hex(&ekey[..]),
                    path.display()
                ));
            }
        }
        let total = u32::from_le_bytes(header[0x10..0x14].try_into().unwrap()) as usize;
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
    let version = u16::from_le_bytes(d[0x08..0x0A].try_into().unwrap());
    let file_bucket = d[0x0A];
    let spec = &d[0x0C..0x10]; // size, offset, key, offset_bits
    if version != 7 || spec != [4, 5, 9, 30] {
        return Err(err(&format!(
            "unsupported idx layout (version {version}, spec {spec:?})"
        )));
    }
    if file_bucket != bucket_no {
        return Err(err("bucket byte disagrees with filename"));
    }
    let entries_size = u32::from_le_bytes(d[0x20..0x24].try_into().unwrap()) as usize;
    let entries = d
        .get(0x28..0x28 + entries_size)
        .ok_or_else(|| err("entry block beyond file"))?;
    for e in entries.chunks_exact(18) {
        let key: [u8; 9] = e[..9].try_into().unwrap();
        let mut packed = 0u64;
        for &b in &e[9..14] {
            packed = packed << 8 | u64::from(b);
        }
        let loc = Loc {
            archive: (packed >> 30) as u16,
            offset: (packed & 0x3FFF_FFFF) as u32,
            size: u32::from_le_bytes(e[14..18].try_into().unwrap()),
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
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| format!("bad hex {s:?}")))
        .collect()
}

#[cfg(test)]
mod tests {
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
}
