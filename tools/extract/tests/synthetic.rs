//! Golden tests against hand-built WDC5 files.
//!
//! The builder here writes the format as specified (wowdev.wiki DB2, WDC5
//! section); expected CSV cells are computed by hand from the same spec, so
//! these tests pin the parser's semantics rather than its implementation.
//! Covered: every field compression, arrays, floats, strings referenced
//! across sections, non-inline ids, the copy table, relationship data, and
//! a sparse (offset-map) table with inlined strings.

use wowdps_extract::{dbd::Dbd, table, wdc5::Db2};

struct Buf(Vec<u8>);

impl Buf {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
}

fn set_bits(buf: &mut [u8], bit_offset: usize, width: u32, val: u64) {
    for i in 0..width as usize {
        let bit = bit_offset + i;
        if val >> i & 1 != 0
            && let Some(byte) = buf.get_mut(bit / 8)
        {
            *byte |= 1 << (bit % 8);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn header(
    b: &mut Buf,
    record_count: u32,
    field_count: u32,
    record_size: u32,
    string_table_size: u32,
    layout_hash: u32,
    min_id: u32,
    max_id: u32,
    flags: u16,
    id_index: u16,
    bitpacked_data_offset: u32,
    common_data_size: u32,
    pallet_data_size: u32,
    section_count: u32,
) {
    b.bytes(b"WDC5");
    b.u32(5);
    let mut schema = [0u8; 128];
    schema[..12].copy_from_slice(b"WowStatic_Tv");
    b.bytes(&schema);
    b.u32(record_count);
    b.u32(field_count);
    b.u32(record_size);
    b.u32(string_table_size);
    b.u32(0x1122_3344); // table_hash
    b.u32(layout_hash);
    b.u32(min_id);
    b.u32(max_id);
    b.u32(0xFFFF_FFFF); // locale
    b.u16(flags);
    b.u16(id_index);
    b.u32(field_count); // total_field_count
    b.u32(bitpacked_data_offset);
    b.u32(1); // lookup_column_count
    b.u32(field_count * 24);
    b.u32(common_data_size);
    b.u32(pallet_data_size);
    b.u32(section_count);
    assert_eq!(b.0.len(), 204);
}

fn storage_info(
    b: &mut Buf,
    offset_bits: u16,
    size_bits: u16,
    extra: u32,
    comp: u32,
    args: [u32; 3],
) {
    b.u16(offset_bits);
    b.u16(size_bits);
    b.u32(extra);
    b.u32(comp);
    for a in args {
        b.u32(a);
    }
}

/// Two sections, nine storage fields covering all compressions, non-inline
/// ids, a copy-table row and relationship data.
#[test]
fn packed_all_compressions() {
    const RECORD_SIZE: usize = 16;
    let mut b = Buf(Vec::new());

    // Pre-section sizes: header 204 + 2 sections * 40 + 9 fields * 4
    // + 9 storage infos * 24 + pallets (12 + 16) + common 8.
    let s1_off: u32 = 204 + 80 + 36 + 216 + 28 + 8;
    // s1 body: 2 records * 16 + strings 13 + id list 8 + relationship 28.
    let s2_off: u32 = s1_off + 32 + 13 + 8 + 28;

    header(
        &mut b,
        3,
        9,
        RECORD_SIZE as u32,
        17,
        0xABCD_1234,
        1,
        10,
        0x04,
        0,
        14,
        8,
        28,
        2,
    );

    // section headers: tact, offset, records, strings, sparse_end, id_list,
    // relationship, offset_map, copies
    b.u64(0);
    b.u32(s1_off);
    b.u32(2);
    b.u32(13);
    b.u32(0);
    b.u32(8);
    b.u32(12 + 2 * 8);
    b.u32(0);
    b.u32(0);

    b.u64(0);
    b.u32(s2_off);
    b.u32(1);
    b.u32(4);
    b.u32(0);
    b.u32(4);
    b.u32(12 + 8);
    b.u32(0);
    b.u32(1);

    // field_structure (size = 32 - element bits, position)
    for (size, pos) in [
        (0i16, 0u16), // A u32
        (24, 4),      // B u8 elements
        (0, 6),       // H string
        (0, 10),      // I float
        (32, 14),     // C bitpacked
        (32, 14),     // D bitpacked signed
        (32, 15),     // F pallet
        (32, 15),     // G pallet array
        (32, 16),     // E common
    ] {
        b.i16(size);
        b.u16(pos);
    }

    storage_info(&mut b, 0, 32, 0, 0, [0, 0, 0]); // A none
    storage_info(&mut b, 32, 16, 0, 0, [0, 0, 0]); // B none u8[2]
    storage_info(&mut b, 48, 32, 0, 0, [0, 0, 0]); // H string
    storage_info(&mut b, 80, 32, 0, 0, [0, 0, 0]); // I float
    storage_info(&mut b, 112, 5, 0, 1, [0, 5, 0]); // C bitpacked
    storage_info(&mut b, 117, 5, 0, 5, [5, 5, 1]); // D bitpacked signed
    storage_info(&mut b, 122, 2, 12, 3, [10, 2, 0]); // F pallet
    storage_info(&mut b, 124, 2, 16, 4, [12, 2, 2]); // G pallet array, card 2
    storage_info(&mut b, 0, 0, 8, 2, [7, 0, 0]); // E common, default 7

    // pallet data: F then G (field order)
    for v in [7u32, 9, 11] {
        b.u32(v);
    }
    for v in [1u32, 2, 3, 4] {
        b.u32(v);
    }
    // common data for E: id 2 -> 42
    b.u32(2);
    b.u32(42);

    assert_eq!(b.0.len(), s1_off as usize);

    // --- section 1: records for ids 1 and 2 ---
    // String space: records end at 3*16 = 48; string blob is
    // "\0hello\0world\0" (s1, offsets 0..13) + "bye\0" (s2, offset 13).
    let mut r0 = [0u8; RECORD_SIZE];
    set_bits(&mut r0, 0, 32, 1); // A
    set_bits(&mut r0, 32, 8, 1); // B[0]
    set_bits(&mut r0, 40, 8, 0xFF); // B[1] = -1
    set_bits(&mut r0, 48, 32, 1 + 48 - 6); // H -> "hello" at blob 1, field byte 6
    set_bits(&mut r0, 80, 32, 1.5f32.to_bits() as u64); // I
    set_bits(&mut r0, 112, 5, 31); // C
    set_bits(&mut r0, 117, 5, 0b11111); // D = -1
    set_bits(&mut r0, 122, 2, 0); // F -> pallet[0] = 7
    set_bits(&mut r0, 124, 2, 1); // G -> [3,4]
    b.bytes(&r0);

    let mut r1 = [0u8; RECORD_SIZE];
    set_bits(&mut r1, 0, 32, 0xFFFF_FFFF);
    set_bits(&mut r1, 48, 32, 0); // H empty
    set_bits(&mut r1, 112, 5, 3);
    set_bits(&mut r1, 117, 5, 5);
    set_bits(&mut r1, 122, 2, 2); // F -> 11
    set_bits(&mut r1, 124, 2, 0); // G -> [1,2]
    b.bytes(&r1);

    b.bytes(b"\0hello\0world\0");
    b.u32(1); // id list
    b.u32(2);
    // relationship: 2 entries, (foreign, record index)
    b.u32(2);
    b.u32(0);
    b.u32(0);
    b.u32(100);
    b.u32(0);
    b.u32(200);
    b.u32(1);

    assert_eq!(b.0.len(), s2_off as usize);

    // --- section 2: record for id 5, global index 2 ---
    let mut r2 = [0u8; RECORD_SIZE];
    set_bits(&mut r2, 0, 32, 7);
    set_bits(&mut r2, 32, 8, 5);
    set_bits(&mut r2, 40, 8, 6);
    set_bits(&mut r2, 48, 32, 13 + 48 - 38); // H -> "bye" at blob 13, field byte 2*16+6
    set_bits(&mut r2, 80, 32, (-2.25f32).to_bits() as u64);
    set_bits(&mut r2, 117, 5, 0b10000); // D = -16
    set_bits(&mut r2, 122, 2, 1); // F -> 9
    b.bytes(&r2);

    b.bytes(b"bye\0");
    b.u32(5); // id list
    b.u32(10); // copy table: new id 10 copies id 1
    b.u32(1);
    b.u32(1); // relationship: 1 entry
    b.u32(0);
    b.u32(0);
    b.u32(300);
    b.u32(0);

    const DBD: &str = "\
COLUMNS
int ID
int A
int B
string H
float I
int C
int D
int F
int G
int E
int R

LAYOUT ABCD1234
BUILD 12.0.0.11111
$noninline,id$ID
A<u32>
B<8>[2]
H
I
C<u32>
D<32>
F<u32>
G<u32>[2]
E<u32>
$noninline,relation$R
";

    let db2 = Db2::parse(b.0).unwrap();
    assert_eq!(db2.header.layout_hash, 0xABCD_1234);
    assert_eq!(db2.rows.len(), 4); // 3 records + 1 copy row

    let dbd = Dbd::parse(DBD).unwrap();
    let mut out = Vec::new();
    table::write_csv(&db2, &dbd, &mut out).unwrap();
    let csv = String::from_utf8(out).unwrap();

    let expected = "\
ID,A,B_0,B_1,H,I,C,D,F,G_0,G_1,E,R
1,1,1,-1,hello,1.5,31,-1,7,3,4,7,100
2,4294967295,0,0,,0,3,5,11,1,2,42,200
5,7,5,6,bye,-2.25,0,-16,9,1,2,7,300
10,1,1,-1,hello,1.5,31,-1,7,3,4,7,100
";
    assert_eq!(csv, expected);
}

/// Sparse (offset-map) table: variable-size records, inlined strings, ids
/// from the offset-map id list.
#[test]
fn sparse_with_inline_strings() {
    let mut b = Buf(Vec::new());

    // header 204 + 1 section * 40 + 3 fields * 4 + 3 infos * 24.
    let s_off: u32 = 204 + 40 + 12 + 72;
    let records_end: u32 = s_off + 9 + 7;

    header(
        &mut b,
        2,
        3,
        9,
        0,
        0x0000_00AB,
        11,
        12,
        0x01 | 0x04,
        0,
        0,
        0,
        0,
        1,
    );

    b.u64(0);
    b.u32(s_off);
    b.u32(2);
    b.u32(0);
    b.u32(records_end);
    b.u32(0); // id_list_size
    b.u32(0); // relationship
    b.u32(2); // offset_map_id_count
    b.u32(0); // copies

    for (size, pos) in [(0i16, 0u16), (0, 4), (16, 8)] {
        b.i16(size);
        b.u16(pos);
    }
    storage_info(&mut b, 0, 32, 0, 0, [0, 0, 0]); // A u32
    storage_info(&mut b, 32, 0, 0, 0, [0, 0, 0]); // S string, inline
    storage_info(&mut b, 0, 16, 0, 0, [0, 0, 0]); // W u16

    assert_eq!(b.0.len(), s_off as usize);

    // record 0 (id 11): A=7, S="hi", W=300 -> 9 bytes
    b.u32(7);
    b.bytes(b"hi\0");
    b.u16(300);
    // record 1 (id 12): A=1, S="", W=65535 -> 7 bytes
    b.u32(1);
    b.bytes(b"\0");
    b.u16(65535);
    assert_eq!(b.0.len(), records_end as usize);

    // offset map entries (offset u32, size u16), then the id list.
    b.u32(s_off);
    b.u16(9);
    b.u32(s_off + 9);
    b.u16(7);
    b.u32(11);
    b.u32(12);

    const DBD: &str = "\
COLUMNS
int ID
int A
string S
int W

LAYOUT 000000AB
BUILD 12.0.0.11111
$noninline,id$ID
A<u32>
S
W<u16>
";

    let db2 = Db2::parse(b.0).unwrap();
    let dbd = Dbd::parse(DBD).unwrap();
    let mut out = Vec::new();
    table::write_csv(&db2, &dbd, &mut out).unwrap();
    let csv = String::from_utf8(out).unwrap();

    let expected = "\
ID,A,S,W
11,7,hi,300
12,1,,65535
";
    assert_eq!(csv, expected);
}

/// Truncated and non-WDC5 inputs must error, never panic.
#[test]
fn rejects_garbage() {
    assert!(Db2::parse(Vec::new()).is_err());
    assert!(Db2::parse(b"WDC3".to_vec()).is_err());
    assert!(Db2::parse(b"WDC5\x05\0\0\0".to_vec()).is_err());
    let mut almost = b"WDC5".to_vec();
    almost.extend_from_slice(&[0u8; 300]);
    // Claims 10 sections but the file ends before their headers.
    almost[200..204].copy_from_slice(&10u32.to_le_bytes());
    assert!(Db2::parse(almost).is_err());
}

// ---------------------------------------------------------------------------
// A declarative builder for the remaining shapes: every inline-id
// compression, encrypted (zero-filled) sections, zeroed id lists, the
// secondary-key flag, float columns under every compression, sparse
// arrays/floats/relationships, and the structural rejections.

#[derive(Default)]
struct Sec {
    tact: u64,
    enc_ids: Vec<u32>,
    records: Vec<Vec<u8>>,
    strings: Vec<u8>,
    ids: Vec<u32>,
    copies: Vec<(u32, u32)>,
    /// (foreign id, key) pairs; the key is a record index, or an id under
    /// the secondary-key flag.
    rel: Vec<(u32, u32)>,
    om_ids: Vec<u32>,
    /// Overrides the computed offset map of a sparse section.
    offset_map: Option<Vec<(u32, u16)>>,
}

/// `(field_structure, storage_info)`: (size, position) and
/// (offset_bits, size_bits, additional_data_size, compression, args).
type FieldSpec = ((i16, u16), (u16, u16, u32, u32, [u32; 3]));

#[derive(Default)]
struct Spec {
    flags: u16,
    id_index: u16,
    record_size: u32,
    layout: u32,
    min_id: u32,
    fields: Vec<FieldSpec>,
    /// Per field; empty unless pallet-compressed.
    pallets: Vec<Vec<u32>>,
    /// Per field; empty unless common-data compressed.
    commons: Vec<Vec<(u32, u32)>>,
    sections: Vec<Sec>,
}

fn build(spec: &Spec) -> Vec<u8> {
    let sparse = spec.flags & 0x01 != 0;
    let secondary = spec.flags & 0x02 != 0;
    let nf = spec.fields.len();
    let pallet_bytes: usize = spec.pallets.iter().map(|p| p.len() * 4).sum();
    let common_bytes: usize = spec.commons.iter().map(|c| c.len() * 8).sum();
    let enc_bytes: usize = spec
        .sections
        .iter()
        .filter(|s| s.tact != 0)
        .map(|s| 4 + s.enc_ids.len() * 4)
        .sum();
    let mut off =
        204 + spec.sections.len() * 40 + nf * 28 + pallet_bytes + common_bytes + enc_bytes;

    let mut heads: Vec<[u32; 8]> = Vec::new();
    let mut bodies: Vec<Vec<u8>> = Vec::new();
    for s in &spec.sections {
        let mut body = Buf(Vec::new());
        let mut cursor = off;
        let mut om: Vec<(u32, u16)> = Vec::new();
        for r in &s.records {
            body.bytes(r);
            om.push((cursor as u32, r.len() as u16));
            cursor += r.len();
        }
        let records_end = cursor;
        if !sparse {
            body.bytes(&s.strings);
        }
        for id in &s.ids {
            body.u32(*id);
        }
        for (dst, src) in &s.copies {
            body.u32(*dst);
            body.u32(*src);
        }
        if let Some(over) = &s.offset_map {
            om = over.clone();
        }
        if !sparse {
            om.clear();
        }
        for (o, size) in &om {
            body.u32(*o);
            body.u16(*size);
        }
        if secondary {
            for id in &s.om_ids {
                body.u32(*id);
            }
        }
        let rel_size = if s.rel.is_empty() {
            0
        } else {
            12 + 8 * s.rel.len()
        };
        if rel_size > 0 {
            body.u32(s.rel.len() as u32);
            body.u32(0);
            body.u32(0);
            for (foreign, key) in &s.rel {
                body.u32(*foreign);
                body.u32(*key);
            }
        }
        if !secondary {
            for id in &s.om_ids {
                body.u32(*id);
            }
        }
        heads.push([
            off as u32,
            s.records.len() as u32,
            if sparse { 0 } else { s.strings.len() as u32 },
            if sparse { records_end as u32 } else { 0 },
            (s.ids.len() * 4) as u32,
            rel_size as u32,
            om.len() as u32,
            s.copies.len() as u32,
        ]);
        off += body.0.len();
        bodies.push(body.0);
    }

    let mut b = Buf(Vec::new());
    let record_count: u32 = spec.sections.iter().map(|s| s.records.len() as u32).sum();
    let string_size: u32 = spec.sections.iter().map(|s| s.strings.len() as u32).sum();
    header(
        &mut b,
        record_count,
        nf as u32,
        spec.record_size,
        string_size,
        spec.layout,
        spec.min_id,
        spec.min_id + record_count,
        spec.flags,
        spec.id_index,
        0,
        common_bytes as u32,
        pallet_bytes as u32,
        spec.sections.len() as u32,
    );
    for (s, h) in spec.sections.iter().zip(&heads) {
        b.u64(s.tact);
        for v in h {
            b.u32(*v);
        }
    }
    for ((size, pos), _) in &spec.fields {
        b.i16(*size);
        b.u16(*pos);
    }
    for (_, (o, sz, extra, comp, args)) in &spec.fields {
        storage_info(&mut b, *o, *sz, *extra, *comp, *args);
    }
    for p in &spec.pallets {
        for v in p {
            b.u32(*v);
        }
    }
    for c in &spec.commons {
        for (id, v) in c {
            b.u32(*id);
            b.u32(*v);
        }
    }
    for s in spec.sections.iter().filter(|s| s.tact != 0) {
        b.u32(s.enc_ids.len() as u32);
        for id in &s.enc_ids {
            b.u32(*id);
        }
    }
    for body in bodies {
        b.bytes(&body);
    }
    b.0
}

fn csv_of(data: Vec<u8>, dbd: &str) -> Result<String, String> {
    let db2 = Db2::parse(data)?;
    let dbd = Dbd::parse(dbd)?;
    let mut out = Vec::new();
    table::write_csv(&db2, &dbd, &mut out)?;
    String::from_utf8(out).map_err(|e| e.to_string())
}

fn u32_none(offset_bits: u16) -> FieldSpec {
    ((0, offset_bits / 8), (offset_bits, 32, 0, 0, [0, 0, 0]))
}

/// Inline ids plus a float under every compression, a 5-bit bitpacked
/// int, and strings that need fputcsv quoting.
#[test]
fn inline_ids_and_float_compressions() {
    let bits = |f: f32| f.to_bits();
    let mut r0 = [0u8; 16];
    set_bits(&mut r0, 0, 32, 1); // ID
    set_bits(&mut r0, 32, 32, bits(0.25).into()); // F
    set_bits(&mut r0, 64, 5, 5); // Q
    set_bits(&mut r0, 69, 2, 1); // H -> 4.0
    set_bits(&mut r0, 71, 1, 1); // P -> [3, 4]
    set_bits(&mut r0, 72, 32, 32 - 9); // S -> blob 0
    let mut r1 = [0u8; 16];
    set_bits(&mut r1, 0, 32, 2);
    set_bits(&mut r1, 32, 32, bits(-1.0).into());
    set_bits(&mut r1, 64, 5, 31);
    set_bits(&mut r1, 72, 32, 32 - 25 + 13); // S -> blob 13
    let spec = Spec {
        flags: 0,
        id_index: 0,
        record_size: 16,
        layout: 0xABCD_0001,
        min_id: 1,
        fields: vec![
            u32_none(0),
            ((32, 4), (32, 32, 0, 1, [32, 32, 0])),
            ((32, 8), (64, 5, 0, 1, [64, 5, 0])),
            ((32, 8), (0, 0, 8, 2, [bits(2.5), 0, 0])),
            ((32, 8), (69, 2, 8, 3, [69, 2, 0])),
            ((32, 8), (71, 1, 16, 4, [71, 1, 2])),
            ((0, 9), (72, 32, 0, 0, [0, 0, 0])),
        ],
        pallets: vec![
            vec![],
            vec![],
            vec![],
            vec![],
            vec![bits(0.5), bits(4.0)],
            vec![bits(1.0), bits(2.0), bits(3.0), bits(4.0)],
            vec![],
        ],
        commons: vec![
            vec![],
            vec![],
            vec![],
            vec![(1, bits(1.5))],
            vec![],
            vec![],
            vec![],
        ],
        sections: vec![Sec {
            records: vec![r0.to_vec(), r1.to_vec()],
            strings: b"he said \"hi\"\0a,b\0".to_vec(),
            ..Sec::default()
        }],
    };
    const DBD: &str = "\
COLUMNS
int ID
float F
int Q
float G
float H
float P
string S

LAYOUT ABCD0001
BUILD 12.0.0.1
$id$ID<32>
F
Q<u32>
G
H
P[2]
S
";
    let data = build(&spec);
    assert_eq!(
        csv_of(data.clone(), DBD).unwrap(),
        "ID,F,Q,G,H,P_0,P_1,S\n\
         1,0.25,5,1.5,4,3,4,\"he said \"\"hi\"\"\"\n\
         2,-1,31,2.5,0.5,1,2,\"a,b\"\n"
    );

    // Layout mismatches and dbd/file disagreements are reported.
    let err = csv_of(data.clone(), &DBD.replace("ABCD0001", "ABCD0002")).unwrap_err();
    assert!(
        err.contains("layout ABCD0001 not in dbd (has: ABCD0002)"),
        "{err}"
    );
    let err = csv_of(data.clone(), &DBD.replace("\nS\n", "\n")).unwrap_err();
    assert!(err.contains("has 6 inline fields, file has 7"), "{err}");
    let err = csv_of(
        data.clone(),
        &DBD.replace("int Q", "float Q").replace("Q<u32>", "Q"),
    )
    .unwrap_err();
    assert!(err.contains("field Q: bitpacked non-int column"), "{err}");
    let err = csv_of(data, &DBD.replace("P[2]", "P[3]")).unwrap_err();
    assert!(
        err.contains("dbd array [3] vs pallet cardinality 2"),
        "{err}"
    );
}

#[test]
fn element_width_disagreements() {
    let spec = Spec {
        flags: 0x04,
        record_size: 8,
        layout: 0xAB,
        min_id: 1,
        fields: vec![
            ((24, 0), (0, 32, 0, 0, [0, 0, 0])),
            ((32, 4), (32, 0, 0, 0, [0, 0, 0])),
        ],
        pallets: vec![vec![], vec![]],
        commons: vec![vec![], vec![]],
        sections: vec![Sec {
            records: vec![vec![1, 2, 3, 4, 5, 6, 7, 8]],
            ids: vec![1],
            ..Sec::default()
        }],
        ..Spec::default()
    };
    let dbd = |x: &str| {
        format!(
            "COLUMNS\nint ID\nint X\nint Y\n\nLAYOUT 000000AB\nBUILD 1.0.0.1\n\
             $noninline,id$ID\n{x}\nY<32>\n"
        )
    };
    let err = csv_of(build(&spec), &dbd("X<8>[2]")).unwrap_err();
    assert!(
        err.contains("field X: dbd says 2x8 bits, file says 32"),
        "{err}"
    );
    let err = csv_of(build(&spec), &dbd("X<8>[4]")).unwrap_err();
    assert!(err.contains("field Y: bad element width 0"), "{err}");
}

/// One-field tables whose id lives in the record under each compression.
#[test]
fn inline_id_compressions() {
    let one = |field: FieldSpec, pallet: Vec<u32>, rec: Vec<u8>| Spec {
        record_size: 4,
        layout: 1,
        min_id: 1,
        fields: vec![field],
        pallets: vec![pallet],
        commons: vec![vec![]],
        sections: vec![Sec {
            records: vec![rec],
            ..Sec::default()
        }],
        ..Spec::default()
    };
    let ids = |spec: &Spec| -> Result<Vec<u32>, String> {
        Ok(Db2::parse(build(spec))?.rows.iter().map(|r| r.id).collect())
    };
    let bitpacked = one(((32, 0), (0, 8, 0, 1, [0, 8, 0])), vec![], vec![7, 0, 0, 0]);
    assert_eq!(ids(&bitpacked).unwrap(), [7]);
    let pallet = one(
        ((32, 0), (0, 2, 8, 3, [0, 2, 0])),
        vec![5, 9],
        vec![1, 0, 0, 0],
    );
    assert_eq!(ids(&pallet).unwrap(), [9]);
    let pallet_oor = one(
        ((32, 0), (0, 2, 8, 3, [0, 2, 0])),
        vec![5, 9],
        vec![3, 0, 0, 0],
    );
    assert!(
        ids(&pallet_oor)
            .unwrap_err()
            .contains("id pallet index 3 out of range")
    );
    let common = one(((32, 0), (0, 0, 0, 2, [1, 0, 0])), vec![], vec![0; 4]);
    assert!(
        ids(&common)
            .unwrap_err()
            .contains("common-data compression")
    );
    let array = one(((32, 0), (0, 1, 8, 4, [0, 1, 2])), vec![1, 2], vec![0; 4]);
    assert!(
        ids(&array)
            .unwrap_err()
            .contains("pallet-array compression")
    );
    let mut beyond = bitpacked;
    beyond.id_index = 5;
    assert!(
        ids(&beyond)
            .unwrap_err()
            .contains("id_index beyond field count")
    );
}

const ID_S_DBD: &str = "\
COLUMNS
int ID
string S

LAYOUT 00000001
BUILD 1.0.0.1
$noninline,id$ID
S
";

fn string_only(sections: Vec<Sec>) -> Spec {
    Spec {
        flags: 0x04,
        record_size: 4,
        layout: 1,
        min_id: 1,
        fields: vec![u32_none(0)],
        pallets: vec![vec![]],
        commons: vec![vec![]],
        sections,
        ..Spec::default()
    }
}

/// The second section's record: global index 1, string at blob offset 0.
fn plain_second() -> Sec {
    Sec {
        records: vec![4u32.to_le_bytes().to_vec()],
        strings: b"s\0".to_vec(),
        ids: vec![2],
        ..Sec::default()
    }
}

#[test]
fn encrypted_sections_without_keys_are_skipped() {
    // Zero-filled records and a zero-filled id list: skipped, string
    // space and global indices still advance.
    let spec = string_only(vec![
        Sec {
            tact: 0xDEAD_BEEF,
            enc_ids: vec![5],
            records: vec![vec![0; 4]],
            ids: vec![0],
            ..Sec::default()
        },
        plain_second(),
    ]);
    assert_eq!(csv_of(build(&spec), ID_S_DBD).unwrap(), "ID,S\n2,s\n");

    // No id list, copies or offset map to probe: skipped on the records.
    let spec = string_only(vec![
        Sec {
            tact: 0xDEAD_BEEF,
            records: vec![vec![0; 4]],
            ..Sec::default()
        },
        plain_second(),
    ]);
    assert_eq!(csv_of(build(&spec), ID_S_DBD).unwrap(), "ID,S\n2,s\n");

    // A real id list under zero records: not encrypted after all.
    let spec = string_only(vec![
        Sec {
            tact: 0xDEAD_BEEF,
            records: vec![vec![0; 4]],
            ids: vec![1],
            ..Sec::default()
        },
        plain_second(),
    ]);
    assert_eq!(csv_of(build(&spec), ID_S_DBD).unwrap(), "ID,S\n1,\n2,s\n");

    // Sparse: the probe is the first offset-map entry's size.
    let spec = Spec {
        flags: 0x01 | 0x04,
        record_size: 8,
        layout: 1,
        min_id: 11,
        fields: vec![u32_none(0), ((0, 4), (32, 0, 0, 0, [0, 0, 0]))],
        pallets: vec![vec![], vec![]],
        commons: vec![vec![], vec![]],
        sections: vec![
            Sec {
                tact: 0xDEAD_BEEF,
                records: vec![vec![0; 4]],
                offset_map: Some(vec![(0, 0)]),
                ..Sec::default()
            },
            Sec {
                records: vec![b"\x07\0\0\0hi\0".to_vec()],
                om_ids: vec![12],
                ..Sec::default()
            },
        ],
        ..Spec::default()
    };
    const DBD: &str = "\
COLUMNS
int ID
int A
string S

LAYOUT 00000001
BUILD 1.0.0.1
$noninline,id$ID
A<u32>
S
";
    assert_eq!(csv_of(build(&spec), DBD).unwrap(), "ID,A,S\n12,7,hi\n");
}

#[test]
fn zero_filled_id_lists_are_synthesized_from_min_id() {
    let mut spec = string_only(vec![Sec {
        records: vec![vec![1, 0, 0, 0], vec![2, 0, 0, 0]],
        ids: vec![0, 0],
        ..Sec::default()
    }]);
    spec.min_id = 10;
    let ids: Vec<u32> = Db2::parse(build(&spec))
        .unwrap()
        .rows
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, [10, 11]);
}

#[test]
fn secondary_key_sparse_relationships() {
    let spec = Spec {
        flags: 0x01 | 0x02 | 0x04,
        record_size: 4,
        layout: 1,
        min_id: 11,
        fields: vec![u32_none(0)],
        pallets: vec![vec![]],
        commons: vec![vec![]],
        sections: vec![Sec {
            records: vec![vec![7, 0, 0, 0], vec![8, 0, 0, 0]],
            om_ids: vec![11, 12],
            rel: vec![(100, 11), (200, 12)],
            ..Sec::default()
        }],
        ..Spec::default()
    };
    const DBD: &str = "\
COLUMNS
int ID
int A
int R

LAYOUT 00000001
BUILD 1.0.0.1
$noninline,id$ID
A<u32>
$noninline,relation$R
";
    assert_eq!(
        csv_of(build(&spec), DBD).unwrap(),
        "ID,A,R\n11,7,100\n12,8,200\n"
    );
}

#[test]
fn sparse_inline_ids_arrays_and_floats() {
    let mut rec = 11u32.to_le_bytes().to_vec();
    rec.extend_from_slice(b"x y\0");
    rec.extend_from_slice(&1u16.to_le_bytes());
    rec.extend_from_slice(&2u16.to_le_bytes());
    rec.extend_from_slice(&1.5f32.to_le_bytes());
    let spec = Spec {
        flags: 0x01,
        record_size: 16,
        layout: 1,
        min_id: 11,
        fields: vec![
            u32_none(0),
            ((0, 4), (32, 0, 0, 0, [0, 0, 0])),
            ((16, 8), (0, 32, 0, 0, [0, 0, 0])),
            ((32, 12), (0, 32, 0, 0, [0, 0, 0])),
        ],
        pallets: vec![vec![]; 4],
        commons: vec![vec![]; 4],
        sections: vec![Sec {
            records: vec![rec],
            om_ids: vec![11],
            ..Sec::default()
        }],
        ..Spec::default()
    };
    const DBD: &str = "\
COLUMNS
int A
string S
int W
float F

LAYOUT 00000001
BUILD 1.0.0.1
$id$A<32>
S
W<16>[2]
F
";
    let data = build(&spec);
    assert_eq!(
        csv_of(data.clone(), DBD).unwrap(),
        "A,S,W_0,W_1,F\n11,\"x y\",1,2,1.5\n"
    );
    let err = csv_of(data, &DBD.replace("$id$A<32>\nS", "A<32>\n$id$S")).unwrap_err();
    assert!(err.contains("sparse string id field"), "{err}");

    // Compressed fields have no place in a sparse table.
    let spec = Spec {
        flags: 0x01 | 0x04,
        record_size: 4,
        layout: 1,
        min_id: 11,
        fields: vec![((32, 0), (0, 8, 0, 1, [0, 8, 0]))],
        pallets: vec![vec![]],
        commons: vec![vec![]],
        sections: vec![Sec {
            records: vec![vec![7, 0, 0, 0]],
            om_ids: vec![11],
            ..Sec::default()
        }],
        ..Spec::default()
    };
    const BITPACKED: &str = "\
COLUMNS
int ID
int A

LAYOUT 00000001
BUILD 1.0.0.1
$noninline,id$ID
A<u32>
";
    let err = csv_of(build(&spec), BITPACKED).unwrap_err();
    assert!(
        err.contains("sparse field A uses Bitpacked compression"),
        "{err}"
    );
}

#[test]
fn structural_rejections() {
    let good = string_only(vec![plain_second()]);
    let base = build(&good);
    let parse_err = |data: Vec<u8>| Db2::parse(data).unwrap_err();

    // Section offset beyond the file.
    let mut d = base.clone();
    d[204 + 8..204 + 12].copy_from_slice(&0xFFFFu32.to_le_bytes());
    assert!(parse_err(d).contains("offset 65535 beyond file"));
    // Storage-info block not matching the field count.
    let mut d = base.clone();
    d[188..192].copy_from_slice(&0u32.to_le_bytes());
    assert!(parse_err(d).contains("0 storage infos but 1 fields"));
    // Unknown compression.
    let mut d = base.clone();
    d[204 + 40 + 4 + 8..204 + 40 + 4 + 12].copy_from_slice(&9u32.to_le_bytes());
    assert!(parse_err(d).contains("unknown field compression 9"));

    // Non-inline ids need an id list.
    let spec = string_only(vec![Sec {
        records: vec![vec![0; 4]],
        ..Sec::default()
    }]);
    assert!(parse_err(build(&spec)).contains("non-inline ids but no id list"));
    // A short id list.
    let spec = string_only(vec![Sec {
        records: vec![vec![0; 4], vec![0; 4]],
        ids: vec![1],
        ..Sec::default()
    }]);
    assert!(parse_err(build(&spec)).contains("record 1 beyond id list"));
    // Copy-table sources must exist.
    let mut sec = plain_second();
    sec.copies = vec![(10, 99)];
    assert!(
        parse_err(build(&string_only(vec![sec]))).contains("copy table source id 99 has no row")
    );
    // A string offset past the string block.
    let mut sec = plain_second();
    sec.records = vec![200u32.to_le_bytes().to_vec()];
    let err = csv_of(build(&string_only(vec![sec])), ID_S_DBD).unwrap_err();
    assert!(err.contains("beyond string data"), "{err}");

    // Sparse: records end before they start, or a record outside the file.
    let sparse = |offset_map: Option<Vec<(u32, u16)>>| Spec {
        flags: 0x01 | 0x04,
        record_size: 4,
        layout: 1,
        min_id: 11,
        fields: vec![u32_none(0)],
        pallets: vec![vec![]],
        commons: vec![vec![]],
        sections: vec![Sec {
            records: vec![vec![7, 0, 0, 0]],
            om_ids: vec![11],
            offset_map,
            ..Sec::default()
        }],
        ..Spec::default()
    };
    let mut d = build(&sparse(None));
    d[204 + 20..204 + 24].copy_from_slice(&0u32.to_le_bytes());
    assert!(parse_err(d).contains("offset_records_end precedes records"));
    let err = parse_err(build(&sparse(Some(vec![(0xFFFF_FF00, 4)]))));
    assert!(err.contains("sparse record 0 outside file"), "{err}");
}
