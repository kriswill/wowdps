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
        if val >> i & 1 != 0 {
            buf[bit / 8] |= 1 << (bit % 8);
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
