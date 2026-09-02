//! TACT metadata: the chain from an install directory to a file's bytes.
//!
//! `.build.info` (pipe-separated, written by the launcher) names the active
//! build config; the build config (a `key = value` file under
//! `Data/config/xx/yy/`) names the root and encoding manifests; the
//! encoding manifest maps content keys to encoding keys; the root manifest
//! maps FileDataIDs (and Jenkins96 name hashes) to content keys. Everything
//! here parses decoded bytes — fetching and BLTE-decoding is the caller's
//! job, since encoding/root are themselves stored like any other file.

use crate::blte::Keys;
use crate::casc::unhex;
use crate::raw;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---- .build.info ----

#[derive(Debug)]
pub struct BuildInfo {
    pub build_key: String,
    pub version: String,
    pub keyring: Option<String>,
}

impl BuildInfo {
    /// Pick the active row for `product` (e.g. "wow").
    pub fn parse(text: &str, product: &str) -> Result<BuildInfo, String> {
        let mut lines = text.lines();
        let header = lines.next().ok_or(".build.info: empty")?;
        let names: Vec<&str> = header
            .split('|')
            .map(|c| c.split('!').next().unwrap_or(c))
            .collect();
        let col = |name: &str| names.iter().position(|&n| n == name);
        let (Some(build), Some(version), Some(prod)) =
            (col("Build Key"), col("Version"), col("Product"))
        else {
            return Err(".build.info: missing expected columns".into());
        };
        let keyring = col("KeyRing");
        let active = col("Active");

        let mut fallback = None;
        for line in lines {
            let cells: Vec<&str> = line.split('|').collect();
            // Column indices come from the header, and the row is only read
            // once its cell count matches, so every `cell` below is in range.
            let cell = |i: usize| cells.get(i).copied().unwrap_or_default();
            if cells.len() != names.len() || cell(prod) != product {
                continue;
            }
            let info = BuildInfo {
                build_key: cell(build).to_string(),
                version: cell(version).to_string(),
                keyring: keyring
                    .map(|k| cell(k).to_string())
                    .filter(|k| !k.is_empty()),
            };
            match active.map(cell) {
                Some("1") | None => return Ok(info),
                _ => fallback = Some(info),
            }
        }
        fallback.ok_or_else(|| format!(".build.info: no row for product {product:?}"))
    }
}

/// `Data/config/xx/yy/<hash>` for a config hash.
pub fn config_path(data_dir: &Path, hash: &str) -> PathBuf {
    // Config hashes are 32 hex characters; anything shorter (or split
    // mid-character, which hex can't be) yields a path that simply won't
    // exist, which the caller reports as a missing config.
    let (a, b) = (hash.get(..2).unwrap_or(""), hash.get(2..4).unwrap_or(""));
    data_dir.join("config").join(a).join(b).join(hash)
}

// ---- build config ----

#[derive(Debug)]
pub struct BuildConfig {
    pub root_ckey: [u8; 16],
    pub encoding_ckey: [u8; 16],
    pub encoding_ekey: [u8; 16],
}

impl BuildConfig {
    pub fn parse(text: &str) -> Result<BuildConfig, String> {
        let mut fields: HashMap<&str, Vec<&str>> = HashMap::new();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                fields.insert(k.trim(), v.split_whitespace().collect());
            }
        }
        let key16 = |name: &str, idx: usize| -> Result<[u8; 16], String> {
            let vals = fields
                .get(name)
                .ok_or_else(|| format!("build config: missing {name}"))?;
            let s = vals
                .get(idx)
                .ok_or_else(|| format!("build config: {name} has no value {idx}"))?;
            unhex(s)?
                .try_into()
                .map_err(|_| format!("build config: {name} is not 16 bytes"))
        };
        Ok(BuildConfig {
            root_ckey: key16("root", 0)?,
            encoding_ckey: key16("encoding", 0)?,
            encoding_ekey: key16("encoding", 1)?,
        })
    }
}

// ---- encoding manifest ----

/// Decoded encoding file; resolves content keys to encoding keys by binary
/// search over the page index, so the (large) byte buffer is kept as-is.
#[derive(Debug)]
pub struct Encoding<'a> {
    index: &'a [u8],
    pages: &'a [u8],
    page_size: usize,
    page_count: usize,
}

impl<'a> Encoding<'a> {
    pub fn new(d: &'a [u8]) -> Result<Encoding<'a>, String> {
        let head: [u8; 0x16] = raw::array(d, 0, "encoding: header")
            .map_err(|_| "encoding: bad signature".to_string())?;
        if &head[..2] != b"EN" {
            return Err("encoding: bad signature".into());
        }
        if head[2] != 1 || head[3] != 16 || head[4] != 16 {
            return Err(format!(
                "encoding: unsupported version/key sizes {:?}",
                &head[2..5]
            ));
        }
        let page_size = u16::from_be_bytes([head[5], head[6]]) as usize * 1024;
        let page_count = u32::from_be_bytes([head[9], head[10], head[11], head[12]]) as usize;
        let espec_size =
            u32::from_be_bytes([head[0x12], head[0x13], head[0x14], head[0x15]]) as usize;

        let index_off = 0x16 + espec_size;
        let pages_off = index_off + page_count * 32;
        let end = pages_off + page_count * page_size;
        let truncated = || "encoding: truncated page tables".to_string();
        Ok(Encoding {
            index: d.get(index_off..pages_off).ok_or_else(truncated)?,
            pages: d.get(pages_off..end).ok_or_else(truncated)?,
            page_size,
            page_count,
        })
    }

    /// First encoding key for a content key, if present.
    pub fn ekey(&self, ckey: &[u8; 16]) -> Option<[u8; 16]> {
        // Last page whose first_ckey <= ckey.
        let (mut lo, mut hi) = (0usize, self.page_count);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let first = self.index.get(mid * 32..mid * 32 + 16)?;
            if first <= &ckey[..] {
                lo = mid + 1
            } else {
                hi = mid
            }
        }
        let page_no = lo.checked_sub(1)?;
        let page = self
            .pages
            .get(page_no * self.page_size..(page_no + 1) * self.page_size)?;

        let mut pos = 0;
        while pos + 6 + 16 <= page.len() {
            let key_count = *page.get(pos)? as usize;
            if key_count == 0 {
                break; // zero padding at the end of the page
            }
            let entry_ckey = page.get(pos + 6..pos + 22)?;
            let ekeys = page.get(pos + 22..pos + 22 + key_count * 16)?;
            if entry_ckey == ckey {
                return ekeys.get(..16)?.try_into().ok();
            }
            pos += 22 + key_count * 16;
        }
        None
    }
}

// ---- root manifest ----

pub const LOCALES: [(&str, u32); 15] = [
    ("enUS", 0x2),
    ("koKR", 0x4),
    ("frFR", 0x10),
    ("deDE", 0x20),
    ("zhCN", 0x40),
    ("esES", 0x80),
    ("zhTW", 0x100),
    ("enGB", 0x200),
    ("esMX", 0x1000),
    ("ruRU", 0x2000),
    ("ptBR", 0x4000),
    ("itIT", 0x8000),
    ("ptPT", 0x10000),
    ("enCN", 0x400),
    ("enTW", 0x800),
];

const CONTENT_LOW_VIOLENCE: u32 = 0x80;
const CONTENT_NO_NAME_HASH: u32 = 0x1000_0000;

#[derive(Debug)]
pub struct RootMatch {
    pub fdid: u32,
    pub ckey: [u8; 16],
    pub locale: u32,
    pub content: u32,
}

/// Scan the decoded root manifest for a FileDataID or a name hash. The
/// manifest is block-structured; this walks every block rather than
/// building a map of all ~6M files.
pub fn root_find(
    d: &[u8],
    want_fdid: Option<u32>,
    want_name: Option<u64>,
    locale_mask: u32,
) -> Result<Option<RootMatch>, String> {
    let head: [u8; 24] =
        raw::array(d, 0, "root: header").map_err(|_| "root: not an MFST manifest".to_string())?;
    if &head[..4] != b"TSFM" {
        return Err("root: not an MFST manifest".into());
    }
    let header_size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let version = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    if !(24..0x100).contains(&header_size) || !(1..=2).contains(&version) {
        return Err(format!(
            "root: unrecognized manifest layout (header_size {header_size}, version {version}); \
             the format may have changed — check wowdev.wiki"
        ));
    }
    let total = u32::from_le_bytes([head[12], head[13], head[14], head[15]]);
    let named = u32::from_le_bytes([head[16], head[17], head[18], head[19]]);
    let allow_unnamed = total != named;

    let mut candidates: Vec<RootMatch> = Vec::new();
    let mut pos = header_size;
    while pos < d.len() {
        let take = |p: usize, n: usize| -> Result<&[u8], String> {
            d.get(p..p + n)
                .ok_or_else(|| format!("root: truncated block at {p}"))
        };
        let num = raw::u32_le(take(pos, 4)?, 0, "root: record count")? as usize;
        pos += 4;
        let (locale, content) = if version == 2 {
            let h = take(pos, 13)?;
            let locale = raw::u32_le(h, 0, "root: block locale")?;
            let f1 = raw::u32_le(h, 4, "root: block flags")?;
            let f2 = raw::u32_le(h, 8, "root: block flags")?;
            let f3 = raw::byte(h, 12, "root: block flags")?;
            pos += 13;
            (locale, f1 | f2 | u32::from(f3) << 17)
        } else {
            let h = take(pos, 8)?;
            let content = raw::u32_le(h, 0, "root: block content flags")?;
            let locale = raw::u32_le(h, 4, "root: block locale")?;
            pos += 8;
            (locale, content)
        };
        let deltas = take(pos, num * 4)?;
        pos += num * 4;
        let ckeys = take(pos, num * 16)?;
        pos += num * 16;
        let names = if allow_unnamed && content & CONTENT_NO_NAME_HASH != 0 {
            None
        } else {
            let n = take(pos, num * 8)?;
            pos += num * 8;
            Some(n)
        };

        let mut fdid: i64 = -1;
        for i in 0..num {
            let delta = raw::i32_le(deltas, i * 4, "root: fdid delta")?;
            fdid += i64::from(delta) + 1;
            let hit = want_fdid.is_some_and(|w| i64::from(w) == fdid)
                || want_name.is_some_and(|w| {
                    names.is_some_and(|n| {
                        raw::u64_le(n, i * 8, "root: name hash").is_ok_and(|h| h == w)
                    })
                });
            if hit {
                candidates.push(RootMatch {
                    fdid: fdid as u32,
                    ckey: raw::array(ckeys, i * 16, "root: record ckey")?,
                    locale,
                    content,
                });
            }
        }
    }

    // Prefer the wanted locale, and non-low-violence content within it.
    candidates.sort_by_key(|c| {
        (
            c.locale & locale_mask == 0,
            c.content & CONTENT_LOW_VIOLENCE != 0,
        )
    });
    Ok(candidates.into_iter().next())
}

// ---- TACT keys ----

/// Load keys in TACTKeys/keyring format: `key-<name> = <hex>` (keyring
/// config) or `<name> <hex>` (wowdev TACTKeys repo). Key-name byte order in
/// the wild is ambiguous, so each key is registered under both readings.
pub fn load_keys(text: &str, keys: &mut Keys) -> Result<usize, String> {
    let mut n = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name_part, key_part) = if let Some(rest) = line.strip_prefix("key-") {
            let Some((name, val)) = rest.split_once('=') else {
                continue;
            };
            (name.trim(), val.trim())
        } else {
            let mut it = line.split_whitespace();
            match (it.next(), it.next()) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            }
        };
        if name_part.len() != 16 || key_part.len() != 32 {
            continue;
        }
        let Ok(name) = u64::from_str_radix(name_part, 16) else {
            continue;
        };
        let Ok(key) = unhex(key_part) else { continue };
        let key: [u8; 16] = key.try_into().map_err(|_| "key not 16 bytes")?;
        keys.insert(name, key);
        keys.insert(name.swap_bytes(), key);
        n += 1;
    }
    Ok(n)
}

pub fn locale_mask(name: &str) -> Result<u32, String> {
    LOCALES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, m)| m)
        .ok_or_else(|| format!("unknown locale {name:?}"))
}

pub fn describe_locale(mask: u32) -> String {
    let names: Vec<&str> = LOCALES
        .iter()
        .filter(|(_, m)| mask & m != 0)
        .map(|&(n, _)| n)
        .collect();
    if names.is_empty() {
        format!("{mask:#x}")
    } else {
        names.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_picks_active_product_row() {
        let text = "\
Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Version!STRING:0|KeyRing!HEX:16|Product!STRING:0
eu|0|aaaa|1.0.0.1||wow
us|1|bbbb|12.0.7.68974|cccc|wow
us|1|dddd|1.15.0.1|	|wow_classic";
        let bi = BuildInfo::parse(text, "wow").unwrap();
        assert_eq!(bi.build_key, "bbbb");
        assert_eq!(bi.version, "12.0.7.68974");
        assert_eq!(bi.keyring.as_deref(), Some("cccc"));
        assert!(BuildInfo::parse(text, "agent").is_err());
    }

    #[test]
    fn build_config_keys() {
        let text = "# Build Configuration\n\nroot = aa0000000000000000000000000000bb\n\
                    encoding = cc000000000000000000000000000000 dd000000000000000000000000000000\n";
        let c = BuildConfig::parse(text).unwrap();
        assert_eq!(c.root_ckey[0], 0xAA);
        assert_eq!(c.root_ckey[15], 0xBB);
        assert_eq!(c.encoding_ckey[0], 0xCC);
        assert_eq!(c.encoding_ekey[0], 0xDD);
    }

    #[test]
    fn encoding_lookup() {
        // One 1 KiB page, two entries.
        let ckey_a = [0x10u8; 16];
        let ckey_b = [0x20u8; 16];
        let ekey_a = [0xAAu8; 16];
        let ekey_b1 = [0xBBu8; 16];
        let ekey_b2 = [0xB2u8; 16];

        let mut d = Vec::new();
        d.extend_from_slice(b"EN");
        d.push(1);
        d.push(16);
        d.push(16);
        d.extend_from_slice(&1u16.to_be_bytes()); // 1 KiB ckey pages
        d.extend_from_slice(&1u16.to_be_bytes());
        d.extend_from_slice(&1u32.to_be_bytes()); // one ckey page
        d.extend_from_slice(&0u32.to_be_bytes()); // no ekey-spec pages
        d.push(0);
        d.extend_from_slice(&0u32.to_be_bytes()); // no espec block
        // page index
        d.extend_from_slice(&ckey_a);
        d.extend_from_slice(&[0u8; 16]);
        // the page
        let mut page = Vec::new();
        for (ckey, ekeys) in [(&ckey_a, vec![ekey_a]), (&ckey_b, vec![ekey_b1, ekey_b2])] {
            page.push(ekeys.len() as u8);
            page.extend_from_slice(&[0u8; 5]); // u40 size
            page.extend_from_slice(ckey);
            for e in &ekeys {
                page.extend_from_slice(e);
            }
        }
        page.resize(1024, 0);
        d.extend_from_slice(&page);

        let enc = Encoding::new(&d).unwrap();
        assert_eq!(enc.ekey(&ckey_a), Some(ekey_a));
        assert_eq!(enc.ekey(&ckey_b), Some(ekey_b1));
        assert_eq!(enc.ekey(&[0x30u8; 16]), None);
        assert_eq!(enc.ekey(&[0x01u8; 16]), None); // before first page
    }

    /// One synthetic root record: fdid delta, ckey, and name hash (absent
    /// when the block's flags say the manifest carries none).
    type Record = (i32, [u8; 16], Option<u64>);

    fn root_v2(blocks: &[(u32, u32, Vec<Record>)]) -> Vec<u8> {
        // (locale, content, records); name hashes present unless None.
        let total: u32 = blocks.iter().map(|b| b.2.len() as u32).sum();
        let named: u32 = blocks
            .iter()
            .filter(|b| b.1 & CONTENT_NO_NAME_HASH == 0)
            .map(|b| b.2.len() as u32)
            .sum();
        let mut d = Vec::new();
        d.extend_from_slice(b"TSFM");
        d.extend_from_slice(&24u32.to_le_bytes());
        d.extend_from_slice(&2u32.to_le_bytes());
        d.extend_from_slice(&total.to_le_bytes());
        d.extend_from_slice(&named.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // padding
        for (locale, content, records) in blocks {
            d.extend_from_slice(&(records.len() as u32).to_le_bytes());
            d.extend_from_slice(&locale.to_le_bytes());
            // v2 splits content flags; put the low bits in the first field.
            d.extend_from_slice(&content.to_le_bytes());
            d.extend_from_slice(&0u32.to_le_bytes());
            d.push(0);
            for (delta, _, _) in records {
                d.extend_from_slice(&delta.to_le_bytes());
            }
            for (_, ckey, _) in records {
                d.extend_from_slice(ckey);
            }
            if content & CONTENT_NO_NAME_HASH == 0 {
                for (_, _, name) in records {
                    d.extend_from_slice(&name.unwrap_or(0).to_le_bytes());
                }
            }
        }
        d
    }

    #[test]
    fn root_scan_by_fdid_and_name() {
        let ck1 = [1u8; 16];
        let ck2 = [2u8; 16];
        let ck3 = [3u8; 16];
        let name2 = crate::hash::name_hash("dbfilesclient/two.db2");
        // Block 1 (enUS): fdids 5 and 7 (delta 5, then 1). Block 2
        // (frFR, no names): fdid 7 again with different content.
        let d = root_v2(&[
            (0x2, 0, vec![(5, ck1, Some(111)), (1, ck2, Some(name2))]),
            (0x10, CONTENT_NO_NAME_HASH, vec![(7, ck3, None)]),
        ]);

        let m = root_find(&d, Some(5), None, 0x2).unwrap().unwrap();
        assert_eq!((m.fdid, m.ckey), (5, ck1));

        // fdid 7 exists in both blocks; enUS wins for enUS callers.
        let m = root_find(&d, Some(7), None, 0x2).unwrap().unwrap();
        assert_eq!(m.ckey, ck2);
        let m = root_find(&d, Some(7), None, 0x10).unwrap().unwrap();
        assert_eq!(m.ckey, ck3);

        let m = root_find(&d, None, Some(name2), 0x2).unwrap().unwrap();
        assert_eq!(m.fdid, 7);

        assert!(root_find(&d, Some(999), None, 0x2).unwrap().is_none());
    }

    #[test]
    fn build_info_edge_cases() {
        assert!(BuildInfo::parse("", "wow").unwrap_err().contains("empty"));
        assert!(
            BuildInfo::parse("Branch!STRING:0|Version!STRING:0\nus|1.0\n", "wow")
                .unwrap_err()
                .contains("missing expected columns")
        );
        // No Active column: the first product row wins; a short row and a
        // blank keyring are tolerated.
        let text = "Build Key!HEX:16|Version!STRING:0|Product!STRING:0|KeyRing!HEX:16\n\
                    short|row\n\
                    aaaa|1.0.0.1|wow|\n\
                    bbbb|2.0.0.2|wow|kk\n";
        let bi = BuildInfo::parse(text, "wow").unwrap();
        assert_eq!(
            (bi.build_key.as_str(), bi.version.as_str()),
            ("aaaa", "1.0.0.1")
        );
        assert_eq!(bi.keyring, None);
        // Inactive rows fall back when nothing is active.
        let text = "Active!DEC:1|Build Key!HEX:16|Version!STRING:0|Product!STRING:0\n\
                    0|cccc|3.0.0.3|wow\n";
        assert_eq!(BuildInfo::parse(text, "wow").unwrap().build_key, "cccc");
    }

    #[test]
    fn config_paths_split_the_hash() {
        let p = config_path(Path::new("/d"), "abcdef0123");
        assert_eq!(p, Path::new("/d/config/ab/cd/abcdef0123"));
        let p = config_path(Path::new("/d"), "a");
        assert_eq!(p, Path::new("/d/config/a"));
    }

    #[test]
    fn build_config_rejections() {
        let err = |s: &str| BuildConfig::parse(s).unwrap_err();
        assert!(err("encoding = aa bb\n").contains("missing root"));
        assert!(
            err("root = aa0000000000000000000000000000bb\nencoding = cc000000000000000000000000000000\n")
                .contains("encoding has no value 1")
        );
        assert!(err("root = abcd\nencoding = aa bb\n").contains("root is not 16 bytes"));
        assert!(err("root = zz\nencoding = aa bb\n").contains("bad hex"));
    }

    #[test]
    fn encoding_rejections() {
        assert!(Encoding::new(b"EN").unwrap_err().contains("bad signature"));
        let mut d = vec![0u8; 0x16];
        d[..2].copy_from_slice(b"XX");
        assert!(Encoding::new(&d).unwrap_err().contains("bad signature"));
        d[..2].copy_from_slice(b"EN");
        d[2] = 2;
        assert!(
            Encoding::new(&d)
                .unwrap_err()
                .contains("unsupported version/key sizes")
        );
        d[2] = 1;
        d[3] = 16;
        d[4] = 16;
        d[6] = 1; // 1 KiB pages
        d[12] = 1; // one page, but no bytes for it
        assert!(
            Encoding::new(&d)
                .unwrap_err()
                .contains("truncated page tables")
        );
    }

    #[test]
    fn root_rejections_and_v1_blocks() {
        assert!(
            root_find(b"TSF", None, None, 0)
                .unwrap_err()
                .contains("MFST")
        );
        let mut d = root_v2(&[]);
        d[..4].copy_from_slice(b"XXXX");
        assert!(root_find(&d, None, None, 0).unwrap_err().contains("MFST"));
        let mut d = root_v2(&[]);
        d[8] = 3; // version 3
        assert!(
            root_find(&d, Some(1), None, 0)
                .unwrap_err()
                .contains("unrecognized manifest layout")
        );
        // Truncated block.
        let mut d = root_v2(&[(0x2, 0, vec![(5, [1u8; 16], Some(1))])]);
        d.truncate(d.len() - 4);
        assert!(
            root_find(&d, Some(5), None, 0x2)
                .unwrap_err()
                .contains("truncated")
        );

        // Version 1: an 8-byte (content, locale) block header, every
        // record named.
        let ck = [9u8; 16];
        let mut d = Vec::new();
        d.extend_from_slice(b"TSFM");
        d.extend_from_slice(&24u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes()); // total
        d.extend_from_slice(&1u32.to_le_bytes()); // named
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes()); // one record
        d.extend_from_slice(&CONTENT_LOW_VIOLENCE.to_le_bytes());
        d.extend_from_slice(&0x2u32.to_le_bytes());
        d.extend_from_slice(&41i32.to_le_bytes()); // fdid 41
        d.extend_from_slice(&ck);
        d.extend_from_slice(&77u64.to_le_bytes());
        let m = root_find(&d, Some(41), None, 0x2).unwrap().unwrap();
        assert_eq!(
            (m.fdid, m.ckey, m.locale, m.content),
            (41, ck, 0x2, CONTENT_LOW_VIOLENCE)
        );
        let m = root_find(&d, None, Some(77), 0x2).unwrap().unwrap();
        assert_eq!(m.fdid, 41);
        assert!(root_find(&d, Some(40), None, 0x2).unwrap().is_none());
    }

    #[test]
    fn key_lines_that_do_not_parse_are_skipped() {
        let mut keys = Keys::new();
        let n = load_keys(
            "key-broken\n\
             lonely\n\
             4eb4869f95f23b5 c9316739348dcc033aa8112f9a3acf5d\n\
             zzzzzzzzzzzzzzzz c9316739348dcc033aa8112f9a3acf5d\n\
             4eb4869f95f23b53 zz316739348dcc033aa8112f9a3acf5d\n\
             4eb4869f95f23b53 c9316739348dcc033aa8112f9a3acf5d\n",
            &mut keys,
        )
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn locale_names_round_trip() {
        assert_eq!(locale_mask("enUS").unwrap(), 0x2);
        assert_eq!(locale_mask("dede").unwrap(), 0x20);
        assert!(locale_mask("xxYY").unwrap_err().contains("unknown locale"));
        assert_eq!(describe_locale(0x2 | 0x10), "enUS+frFR");
        assert_eq!(describe_locale(0x8000_0000), "0x80000000");
    }

    #[test]
    fn keys_both_formats_and_orders() {
        let mut keys = Keys::new();
        let n = load_keys(
            "# comment\nkey-4eb4869f95f23b53 = c9316739348dcc033aa8112f9a3acf5d\n\
             FA505078126ACB3E BDC51862ABED79B2DE48C8E7E66C6200 taxi thing\n",
            &mut keys,
        )
        .unwrap();
        assert_eq!(n, 2);
        assert!(keys.contains_key(&0x4EB4_869F_95F2_3B53));
        assert!(keys.contains_key(&0x533B_F295_9F86_B44E)); // swapped
        assert!(keys.contains_key(&0xFA50_5078_126A_CB3E));
    }
}
