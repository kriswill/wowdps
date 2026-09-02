//! WDC5 `.db2` structural parser.
//!
//! Layout (per wowdev.wiki `DB2`, cross-checked against DBCD's WDC5Reader):
//! a 204-byte header, section headers, per-field `field_structure` and
//! `field_storage_info`, pallet + common data blocks in field order,
//! encrypted-id blocks for TACT-keyed sections, then the sections themselves.
//! A section is either fixed-size records plus a string block, or — when
//! header flag 0x01 (offset map / "sparse") is set — variable-length records
//! with inlined strings addressed by an offset map. Sections end with the id
//! list, copy table, offset map, relationship data and offset-map id list
//! (the last two swap places under flag 0x02).
//!
//! String fields hold offsets relative to the field's own position within a
//! virtual blob of `[all sections' records][all sections' string blocks]`;
//! [`Db2::string_at`] resolves them against the concatenated string data.

use crate::bits::{read_bits, sign_extend};
use crate::raw;
use std::collections::HashMap;

pub const MAGIC: [u8; 4] = *b"WDC5";
/// magic + version + 128-byte schema string + 9 u32 + 2 u16 + 7 u32.
const HEADER_SIZE: usize = 204;
const STORAGE_INFO_SIZE: usize = 24;

/// Header flag 0x01: offset-map records with inlined strings.
pub const FLAG_SPARSE: u16 = 0x01;
/// Header flag 0x02: relationship entries are keyed by record id, and the
/// offset-map id list precedes the relationship data.
pub const FLAG_SECONDARY_KEY: u16 = 0x02;
/// Header flag 0x04: ids live in the id list, not in a record field.
pub const FLAG_NONINLINE_ID: u16 = 0x04;

#[derive(Debug)]
pub struct Header {
    pub version: u32,
    pub schema: String,
    pub record_count: u32,
    pub field_count: u32,
    pub record_size: u32,
    pub string_table_size: u32,
    pub table_hash: u32,
    pub layout_hash: u32,
    pub min_id: u32,
    pub max_id: u32,
    pub locale: u32,
    pub flags: u16,
    pub id_index: u16,
    pub total_field_count: u32,
    pub bitpacked_data_offset: u32,
    pub lookup_column_count: u32,
    pub field_storage_info_size: u32,
    pub common_data_size: u32,
    pub pallet_data_size: u32,
    pub section_count: u32,
}

#[derive(Debug)]
pub struct SectionHeader {
    pub tact_key_hash: u64,
    pub file_offset: u32,
    pub record_count: u32,
    pub string_table_size: u32,
    pub offset_records_end: u32,
    pub id_list_size: u32,
    pub relationship_data_size: u32,
    pub offset_map_id_count: u32,
    pub copy_table_count: u32,
}

/// `field_structure`: element size for uncompressed fields, as `32 - bits`.
#[derive(Clone, Copy, Debug)]
pub struct FieldStruct {
    pub size: i16,
    pub position: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compression {
    None,
    Bitpacked,
    CommonData,
    PalletIndexed,
    PalletArray,
    BitpackedSigned,
}

#[derive(Clone, Copy, Debug)]
pub struct StorageInfo {
    pub offset_bits: u16,
    pub size_bits: u16,
    pub additional_data_size: u32,
    pub compression: Compression,
    /// Per-compression payload: bitpacked = (offset, size, flags&1 = signed);
    /// common = (default, _, _); pallet array = (_, _, cardinality).
    pub args: [u32; 3],
}

/// One emitted row: a record plus its resolved id and relationship value.
/// Copy-table rows share the source's record range and global index.
#[derive(Debug)]
pub struct Row {
    pub id: u32,
    /// Byte range of the record within the file buffer.
    pub start: usize,
    pub len: usize,
    /// Index within the all-sections record blob (drives string offsets).
    pub global: u32,
    /// Foreign id from relationship data, when present.
    pub foreign: Option<u32>,
}

#[derive(Debug)]
pub struct Db2 {
    pub header: Header,
    pub sections: Vec<SectionHeader>,
    pub fields: Vec<FieldStruct>,
    pub infos: Vec<StorageInfo>,
    /// Per-field pallet values (empty unless pallet-compressed).
    pub pallets: Vec<Vec<u32>>,
    /// Per-field common-data overrides keyed by record id.
    pub commons: Vec<HashMap<u32, u32>>,
    /// All sections' string blocks, concatenated in section order.
    strings: Vec<u8>,
    data: Vec<u8>,
    pub rows: Vec<Row>,
}

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.d.len())
            .ok_or_else(|| format!("wdc5: truncated reading {what} at byte {} (+{n})", self.p))?;
        let s = self
            .d
            .get(self.p..end)
            .ok_or_else(|| format!("wdc5: truncated reading {what} at byte {} (+{n})", self.p))?;
        self.p = end;
        Ok(s)
    }

    /// `take`, re-shaped into a fixed-size array for the integer readers.
    fn arr<const N: usize>(&mut self, what: &str) -> Result<[u8; N], String> {
        let p = self.p;
        let s = self.take(N, what)?;
        <[u8; N]>::try_from(s)
            .map_err(|_| format!("wdc5: truncated reading {what} at byte {p} (+{N})"))
    }

    fn u16(&mut self, what: &str) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.arr::<2>(what)?))
    }

    fn u32(&mut self, what: &str) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.arr::<4>(what)?))
    }

    fn u64(&mut self, what: &str) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.arr::<8>(what)?))
    }

    fn i16(&mut self, what: &str) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.arr::<2>(what)?))
    }

    fn seek(&mut self, p: usize, what: &str) -> Result<(), String> {
        if p > self.d.len() {
            return Err(format!(
                "wdc5: {what} offset {p} beyond file ({})",
                self.d.len()
            ));
        }
        self.p = p;
        Ok(())
    }
}

/// Little-endian u32 out of a 4-byte chunk, as handed out by
/// `as_chunks::<4>()` — the array type makes the conversion infallible.
fn le_u32(b: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*b)
}

impl Db2 {
    pub fn parse(data: Vec<u8>) -> Result<Db2, String> {
        let mut c = Cur { d: &data, p: 0 };

        let magic = c.take(4, "magic")?;
        if magic != MAGIC {
            return Err(format!(
                "not a WDC5 file (magic {:?}); other DB2 versions are unsupported",
                String::from_utf8_lossy(magic)
            ));
        }
        let version = c.u32("version")?;
        let schema_raw = c.take(128, "schema string")?;
        let schema_end = schema_raw.iter().position(|&b| b == 0).unwrap_or(128);
        let schema = String::from_utf8_lossy(
            schema_raw
                .get(..schema_end)
                .ok_or("wdc5: truncated schema string")?,
        )
        .into_owned();
        let header = Header {
            version,
            schema,
            record_count: c.u32("record_count")?,
            field_count: c.u32("field_count")?,
            record_size: c.u32("record_size")?,
            string_table_size: c.u32("string_table_size")?,
            table_hash: c.u32("table_hash")?,
            layout_hash: c.u32("layout_hash")?,
            min_id: c.u32("min_id")?,
            max_id: c.u32("max_id")?,
            locale: c.u32("locale")?,
            flags: c.u16("flags")?,
            id_index: c.u16("id_index")?,
            total_field_count: c.u32("total_field_count")?,
            bitpacked_data_offset: c.u32("bitpacked_data_offset")?,
            lookup_column_count: c.u32("lookup_column_count")?,
            field_storage_info_size: c.u32("field_storage_info_size")?,
            common_data_size: c.u32("common_data_size")?,
            pallet_data_size: c.u32("pallet_data_size")?,
            section_count: c.u32("section_count")?,
        };
        debug_assert_eq!(c.p, HEADER_SIZE);

        let mut sections = Vec::with_capacity(header.section_count as usize);
        for _ in 0..header.section_count {
            sections.push(SectionHeader {
                tact_key_hash: c.u64("tact_key_hash")?,
                file_offset: c.u32("file_offset")?,
                record_count: c.u32("section record_count")?,
                string_table_size: c.u32("section string_table_size")?,
                offset_records_end: c.u32("offset_records_end")?,
                id_list_size: c.u32("id_list_size")?,
                relationship_data_size: c.u32("relationship_data_size")?,
                offset_map_id_count: c.u32("offset_map_id_count")?,
                copy_table_count: c.u32("copy_table_count")?,
            });
        }

        let mut fields = Vec::with_capacity(header.field_count as usize);
        for _ in 0..header.field_count {
            fields.push(FieldStruct {
                size: c.i16("field size")?,
                position: c.u16("field position")?,
            });
        }

        let info_count = header.field_storage_info_size as usize / STORAGE_INFO_SIZE;
        if info_count != header.field_count as usize {
            return Err(format!(
                "wdc5: {} storage infos but {} fields",
                info_count, header.field_count
            ));
        }
        let mut infos = Vec::with_capacity(info_count);
        for _ in 0..info_count {
            let offset_bits = c.u16("storage offset_bits")?;
            let size_bits = c.u16("storage size_bits")?;
            let additional_data_size = c.u32("additional_data_size")?;
            let compression = match c.u32("compression")? {
                0 => Compression::None,
                1 => Compression::Bitpacked,
                2 => Compression::CommonData,
                3 => Compression::PalletIndexed,
                4 => Compression::PalletArray,
                5 => Compression::BitpackedSigned,
                n => return Err(format!("wdc5: unknown field compression {n}")),
            };
            let args = [c.u32("arg0")?, c.u32("arg1")?, c.u32("arg2")?];
            infos.push(StorageInfo {
                offset_bits,
                size_bits,
                additional_data_size,
                compression,
                args,
            });
        }

        // Pallet and common blocks follow in field order.
        let mut pallets = vec![Vec::new(); info_count];
        for (i, info) in infos.iter().enumerate() {
            if matches!(
                info.compression,
                Compression::PalletIndexed | Compression::PalletArray
            ) {
                let bytes = c.take(info.additional_data_size as usize, "pallet data")?;
                *pallets
                    .get_mut(i)
                    .ok_or("wdc5: pallet field index out of range")? =
                    bytes.as_chunks::<4>().0.iter().map(le_u32).collect();
            }
        }
        let mut commons = vec![HashMap::new(); info_count];
        for (i, info) in infos.iter().enumerate() {
            if info.compression == Compression::CommonData {
                let bytes = c.take(info.additional_data_size as usize, "common data")?;
                let map: &mut HashMap<u32, u32> = commons
                    .get_mut(i)
                    .ok_or("wdc5: common-data field index out of range")?;
                for pair in bytes.as_chunks::<8>().0 {
                    let id = raw::u32_le(pair, 0, "wdc5: common data entry")?;
                    let val = raw::u32_le(pair, 4, "wdc5: common data entry")?;
                    map.insert(id, val);
                }
            }
        }

        // Encrypted-id blocks, one per TACT-keyed section. Content unused.
        for s in &sections {
            if s.tact_key_hash != 0 {
                let n = c.u32("encrypted id count")?;
                c.take(n as usize * 4, "encrypted ids")?;
            }
        }

        let sparse = header.flags & FLAG_SPARSE != 0;
        let mut strings = Vec::with_capacity(header.string_table_size as usize);
        let mut rows: Vec<Row> = Vec::with_capacity(header.record_count as usize);
        let mut copies: Vec<(u32, u32)> = Vec::new(); // (new id, source id)
        let mut global = 0u32;

        for (si, s) in sections.iter().enumerate() {
            c.seek(s.file_offset as usize, "section")?;

            // Record region, and the section's contribution to string space.
            let rec_start = c.p;
            let region = if sparse {
                let end = s.offset_records_end as usize;
                if end < rec_start {
                    return Err(format!(
                        "wdc5: section {si} offset_records_end precedes records"
                    ));
                }
                c.seek(end, "sparse records end")?;
                data.get(rec_start..end)
                    .ok_or_else(|| format!("wdc5: section {si} sparse records outside file"))?
            } else {
                let bytes = s.record_count as usize * header.record_size as usize;
                let region = c.take(bytes, "records")?;
                strings.extend_from_slice(c.take(s.string_table_size as usize, "string block")?);
                region
            };

            // A TACT-encrypted section without its key ships zero-filled:
            // skip its rows (string space and global indices still advance).
            if s.tact_key_hash != 0 && region.iter().all(|&b| b == 0) {
                let zeroed = if s.id_list_size > 0 || s.copy_table_count > 0 {
                    raw::u32_le(&data, c.p, "wdc5: encrypted section probe")? == 0
                } else if s.offset_map_id_count > 0 {
                    raw::u16_le(&data, c.p + 4, "wdc5: encrypted section probe")? == 0
                } else {
                    true
                };
                if zeroed {
                    global += s.record_count;
                    continue;
                }
            }

            let mut ids: Vec<u32> = c
                .take(s.id_list_size as usize, "id list")?
                .as_chunks::<4>()
                .0
                .iter()
                .map(le_u32)
                .collect();
            if !ids.is_empty() && ids.iter().all(|&i| i == 0) {
                // Zero-filled id list (partially decrypted sections).
                ids = (0..s.record_count)
                    .map(|i| header.min_id + global + i)
                    .collect();
            }

            for _ in 0..s.copy_table_count {
                let dst = c.u32("copy dst")?;
                let src = c.u32("copy src")?;
                if dst != src {
                    copies.push((dst, src));
                }
            }

            let mut offset_map = Vec::with_capacity(s.offset_map_id_count as usize);
            for _ in 0..s.offset_map_id_count {
                let off = c.u32("offset map offset")?;
                let size = c.u16("offset map size")?;
                offset_map.push((off, size));
            }

            let secondary = header.flags & FLAG_SECONDARY_KEY != 0;
            let mut om_ids: Vec<u32> = Vec::new();
            if secondary && s.offset_map_id_count > 0 {
                om_ids = c
                    .take(s.offset_map_id_count as usize * 4, "offset map ids")?
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(le_u32)
                    .collect();
            }

            // Relationship data: section-local record index -> foreign id
            // (record id -> foreign id under flag 0x02).
            let mut rel_by_index: HashMap<u32, u32> = HashMap::new();
            let mut rel_by_id: HashMap<u32, u32> = HashMap::new();
            if s.relationship_data_size > 0 {
                let rel_start = c.p;
                let num = c.u32("relationship count")?;
                let _min = c.u32("relationship min id")?;
                let _max = c.u32("relationship max id")?;
                for _ in 0..num {
                    let foreign = c.u32("relationship foreign id")?;
                    let key = c.u32("relationship record index")?;
                    if secondary {
                        rel_by_id.insert(key, foreign);
                    } else {
                        rel_by_index.insert(key, foreign);
                    }
                }
                c.seek(
                    rel_start + s.relationship_data_size as usize,
                    "relationship end",
                )?;
            }

            if !secondary && s.offset_map_id_count > 0 {
                om_ids = c
                    .take(s.offset_map_id_count as usize * 4, "offset map ids")?
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(le_u32)
                    .collect();
            }

            let count = if sparse {
                s.offset_map_id_count
            } else {
                s.record_count
            };
            for i in 0..count {
                let (start, len) = if sparse {
                    let (off, size) = offset_map.get(i as usize).copied().ok_or_else(|| {
                        format!("wdc5: sparse record {i} has no offset map entry")
                    })?;
                    let (off, size) = (off as usize, size as usize);
                    if off + size > data.len() {
                        return Err(format!("wdc5: sparse record {i} outside file"));
                    }
                    (off, size)
                } else {
                    (
                        rec_start + i as usize * header.record_size as usize,
                        header.record_size as usize,
                    )
                };

                let id = if sparse && !om_ids.is_empty() {
                    om_ids
                        .get(i as usize)
                        .copied()
                        .ok_or_else(|| format!("wdc5: record {i} beyond offset map id list"))?
                } else if !ids.is_empty() {
                    ids.get(i as usize)
                        .copied()
                        .ok_or_else(|| format!("wdc5: record {i} beyond id list"))?
                } else if header.flags & FLAG_NONINLINE_ID == 0 {
                    decode_inline_id(
                        data.get(start..start + len)
                            .ok_or_else(|| format!("wdc5: record {i} outside file"))?,
                        &header,
                        &fields,
                        &infos,
                        &pallets,
                    )?
                } else {
                    return Err(format!(
                        "wdc5: section {si} has non-inline ids but no id list"
                    ));
                };

                let foreign = rel_by_index.get(&i).or_else(|| rel_by_id.get(&id)).copied();
                rows.push(Row {
                    id,
                    start,
                    len,
                    global: global + i,
                    foreign,
                });
            }
            global += count;
        }

        // Copy-table rows: duplicate the source row under a new id. Sources
        // may live in any section, so resolve after the section loop.
        if !copies.is_empty() {
            let by_id: HashMap<u32, usize> =
                rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
            for (dst, src) in copies {
                let Some(&i) = by_id.get(&src) else {
                    return Err(format!("wdc5: copy table source id {src} has no row"));
                };
                let r = rows
                    .get(i)
                    .ok_or_else(|| format!("wdc5: copy table source id {src} has no row"))?;
                rows.push(Row {
                    id: dst,
                    start: r.start,
                    len: r.len,
                    global: r.global,
                    foreign: r.foreign,
                });
            }
        }

        Ok(Db2 {
            header,
            sections,
            fields,
            infos,
            pallets,
            commons,
            strings,
            data,
            rows,
        })
    }

    /// The record's bytes. Row ranges are validated against the buffer while
    /// parsing, so the fallback is unreachable for rows this type produced.
    pub fn record(&self, row: &Row) -> &[u8] {
        self.data
            .get(row.start..row.start + row.len)
            .unwrap_or(&[][..])
    }

    /// Resolve a string field: `rel` is the record value, read at byte
    /// `field_byte` of the record with global index `global`. Offsets are
    /// relative to the field's position in the records+strings blob; values
    /// pointing at or before the blob's record part mean "empty".
    pub fn string_at(&self, global: u32, field_byte: u32, rel: i64) -> Result<&str, String> {
        let records_end = self.header.record_count as i64 * self.header.record_size as i64;
        let idx =
            global as i64 * self.header.record_size as i64 + field_byte as i64 + rel - records_end;
        if idx < 0 {
            return Ok("");
        }
        let idx = idx as usize;
        if idx >= self.strings.len() {
            return Err(format!("wdc5: string offset {idx} beyond string data"));
        }
        cstr(raw::rest(&self.strings, idx, "wdc5: string data")?)
    }
}

/// Read one field's integer value straight from a record (used for inline
/// ids, which cannot be common-data compressed — that would need the id).
fn decode_inline_id(
    rec: &[u8],
    header: &Header,
    fields: &[FieldStruct],
    infos: &[StorageInfo],
    pallets: &[Vec<u32>],
) -> Result<u32, String> {
    let k = header.id_index as usize;
    let info = infos.get(k).ok_or("wdc5: id_index beyond field count")?;
    let v = match info.compression {
        Compression::None => {
            let fs = fields.get(k).ok_or("wdc5: id_index beyond field count")?;
            let elem = elem_bits(fs, info);
            read_bits(rec, info.offset_bits as usize, elem)
        }
        Compression::Bitpacked | Compression::BitpackedSigned => {
            read_bits(rec, info.offset_bits as usize, info.size_bits as u32)
        }
        Compression::PalletIndexed => {
            let idx = read_bits(rec, info.offset_bits as usize, info.size_bits as u32) as usize;
            pallets
                .get(k)
                .and_then(|p| p.get(idx))
                .copied()
                .ok_or_else(|| format!("wdc5: id pallet index {idx} out of range"))?
                as u64
        }
        Compression::CommonData => {
            return Err("wdc5: id field uses common-data compression".into());
        }
        Compression::PalletArray => {
            return Err("wdc5: id field uses pallet-array compression".into());
        }
    };
    Ok(v as u32)
}

/// Element width in bits for an uncompressed field: `32 - field_structure.size`,
/// falling back to the storage info's bit width when that is degenerate.
pub fn elem_bits(fs: &FieldStruct, info: &StorageInfo) -> u32 {
    let bits = 32 - fs.size as i32;
    if bits > 0 { bits as u32 } else { info.args[1] }
}

/// Signed storage flag for bitpacked fields.
pub fn bitpacked_signed(info: &StorageInfo) -> bool {
    info.compression == Compression::BitpackedSigned
        || (info.compression == Compression::Bitpacked && info.args[2] & 1 != 0)
}

/// Storage-level value of a bitpacked field, sign-extended when flagged.
pub fn bitpacked_value(rec: &[u8], info: &StorageInfo) -> u64 {
    let raw = read_bits(rec, info.offset_bits as usize, info.size_bits as u32);
    if bitpacked_signed(info) {
        sign_extend(raw, info.size_bits as u32) as u64
    } else {
        raw
    }
}

pub fn cstr(bytes: &[u8]) -> Result<&str, String> {
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or("wdc5: unterminated string")?;
    let s = bytes.get(..end).ok_or("wdc5: unterminated string")?;
    std::str::from_utf8(s).map_err(|e| format!("wdc5: bad utf-8 in string: {e}"))
}
