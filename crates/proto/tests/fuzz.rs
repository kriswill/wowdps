//! Wire-decoder robustness beyond `codec.rs`'s exhaustive prefix
//! truncations: random byte mutations of every valid encoded message, fully
//! random byte strings, and mutated bodies fed straight past the framing
//! layer. The claim under test is CONTRACT.md's "decoding returns `Result`
//! — never panics, never attacker-sized allocations": a mutated frame may
//! legitimately still decode (a flipped byte inside a string is just a
//! different string), so the only assertion is that decode RETURNS — Ok or
//! a clean `DecodeError`, never an unwind.
//!
//! proto is stdlib-only, so mutation comes from a hand-rolled fixed-seed
//! xorshift64 — every run identical, every failure reproducible.

use wowdps_model::{
    Class, ListRow, Mark, MarkKind, Row, SegmentId, SegmentInfo, SegmentKind, Spec, Timeline, View,
};
use wowdps_proto::wire;
use wowdps_proto::{
    Breakdown, ClientKind, ClientMsg, CompareSide, Cursor, DaemonMsg, HistoryStatus, ListEntry,
    LoadError, OverlayState, PROTO_VERSION, SegmentRef,
};

/// xorshift64, fixed seed. Deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        assert!(seed != 0, "xorshift64 degenerates at 0");
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        assert!(n > 0);
        (self.next() % n as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }
}

// ---- corpus: every message variant, mirroring codec.rs ----------------------

fn row(key: &str, class: Option<Class>) -> Row {
    Row {
        key: key.to_string(),
        label: format!("«{key}»"),
        amount: u64::MAX,
        extra: 7,
        count: 1234,
        crits: u64::MAX,
        per_sec: 123456.789,
        pct: 99.25,
        class,
        spec: class.map(|_| Spec::FrostMage),
        hp: class.map(|_| (123_456, u64::MAX)),
        gain: class.is_some(),
        spell_id: if class.is_some() { 116 } else { 0 },
        enemy: class.is_none(),
        school: 0x24,
    }
}

fn info() -> SegmentInfo {
    SegmentInfo {
        kind: SegmentKind::Encounter,
        name: "Verkath the Hollow".to_string(),
        start_ms: -62_135_596_800_000,
        duration_ms: 45_000,
        success: Some(false),
        live: true,
        instance: Some(7),
        pars_ms: Some((1_680_000, 1_344_000, 1_008_000)),
        arena: true,
        encounter: None,
    }
}

fn compare_side(guid: &str) -> CompareSide {
    CompareSide {
        guid: guid.to_string(),
        total: row(guid, Some(Class::Mage)),
        spells: vec![row("Frostbolt", Some(Class::Mage))],
        spell_timeline: None,
        timeline: Timeline {
            bucket_ms: 1000,
            buckets: vec![0, u64::MAX, 42],
            marks: vec![
                Mark {
                    at_ms: 250,
                    kind: MarkKind::TrinketProc,
                    label: "Sigil «of» Ruin".to_string(),
                    spell_id: u32::MAX,
                    dur_ms: i64::MAX,
                },
                Mark {
                    at_ms: 300,
                    kind: MarkKind::External,
                    label: "Bloodlust".to_string(),
                    spell_id: 2825,
                    dur_ms: 0,
                },
            ],
        },
    }
}

/// Every ClientMsg variant (the same shapes codec.rs pins).
fn client_msgs() -> Vec<ClientMsg> {
    vec![
        ClientMsg::Hello {
            proto: PROTO_VERSION,
            client: ClientKind::Overlay,
            pid: u32::MAX,
        },
        ClientMsg::Watch(Cursor::List),
        ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Id(SegmentId(u64::MAX)),
            view: View::Deaths,
            top_n: Some(0),
            drill: Some("Player-1301-0AB7C3D2".to_string()),
            spell: Some("Chaos Bolt".to_string()),
        }),
        ClientMsg::Watch(Cursor::Compare {
            segment: SegmentRef::Live,
            a: "Player-1301-0AB7C3D2".to_string(),
            b: "Player-1301-0AB7C3D3".to_string(),
            range: Some((0, u32::MAX)),
            spell: Some("Chaos Bolt".to_string()),
        }),
        ClientMsg::GetStatus { req_id: 42 },
        ClientMsg::VisibilityChanged { visible: false },
        ClientMsg::Shutdown,
        ClientMsg::DiscardTrash,
    ]
}

/// Every DaemonMsg variant (the same shapes codec.rs pins).
fn daemon_msgs() -> Vec<DaemonMsg> {
    vec![
        DaemonMsg::HelloAck {
            proto: PROTO_VERSION,
            version: "0.1.0".to_string(),
        },
        DaemonMsg::Snapshot {
            seq: u64::MAX,
            segment: SegmentRef::Id(SegmentId(3)),
            id: Some(SegmentId(3)),
            view: View::Healing,
            info: info(),
            rows: vec![row("Player-1-A", Some(Class::Evoker)), row("Pet-x", None)],
            total_rows: 40,
            breakdown: Some(Breakdown {
                by_spell: vec![row("Frostbolt", Some(Class::Mage))],
                by_target: vec![],
                timeline: None,
                spell_timeline: None,
                spell_targets: None,
            }),
            segment_count: 12,
            source: Some("WoWCombatLog-080226_190155.txt".to_string()),
            status: None,
        },
        DaemonMsg::CompareSnapshot {
            seq: 1,
            segment: SegmentRef::Live,
            id: None,
            info: info(),
            a: Box::new(compare_side("Player-1-A")),
            b: Box::new(CompareSide::default()),
            range: Some((15_000, 45_000)),
            source: None,
            status: Some("loading…".to_string()),
        },
        DaemonMsg::SegmentList {
            seq: 9,
            entries: vec![ListEntry {
                id: SegmentId(u64::MAX),
                row: ListRow {
                    kind: SegmentKind::Overall,
                    name: "Häxenmeister +3".to_string(),
                    start_ms: 1_722_000_000_123,
                    success: None,
                    duration_ms: 61_500,
                    live: true,
                    instance: Some(0),
                    pars_ms: Some((2_040_000, 1_632_000, 1_224_000)),
                    arena: false,
                    encounter: None,
                },
            }],
            source: Some("log.txt".to_string()),
            active: true,
            log_id: None,
        },
        DaemonMsg::SegmentOpened { id: SegmentId(17) },
        DaemonMsg::LoadFailed {
            segment: SegmentId(3),
            error: LoadError::Io("read: interrupted".to_string()),
        },
        DaemonMsg::Status {
            req_id: 1,
            game_running: true,
            source: Some("/logs/WoWCombatLog.txt".to_string()),
            clients: 3,
            linger: true,
            overlay: OverlayState::Failed("no WAYLAND_DISPLAY".to_string()),
            history: HistoryStatus::default(),
        },
        DaemonMsg::SetVisible(true),
        DaemonMsg::Fatal("protocol mismatch".to_string()),
    ]
}

fn all_frames() -> Vec<Vec<u8>> {
    let mut frames: Vec<Vec<u8>> = Vec::new();
    frames.extend(client_msgs().iter().map(|m| m.encode()));
    frames.extend(daemon_msgs().iter().map(|m| m.encode()));
    assert!(
        frames.len() >= 15,
        "corpus suspiciously small ({}) — variants dropped?",
        frames.len()
    );
    frames
}

/// The one assertion of this suite: both full decode paths (framing + message)
/// must return, never unwind. Their `Result` values are free.
fn assert_decodes_without_panic(bytes: &[u8], ctx: &str) {
    let outcome = std::panic::catch_unwind(|| {
        if let Ok((tag, body, rest)) = wire::split_frame(bytes) {
            let _ = ClientMsg::decode(tag, body);
            let _ = DaemonMsg::decode(tag, body);
            let _ = rest;
        }
        // The stream reader must reach the same verdict without panicking.
        let _ = wire::read_frame(&mut &bytes[..]);
    });
    assert!(outcome.is_ok(), "decoder panicked ({ctx}) on {bytes:?}");
}

fn mutate(rng: &mut Rng, bytes: &mut Vec<u8>) {
    match rng.below(3) {
        0 if !bytes.is_empty() => {
            let at = rng.below(bytes.len());
            if let Some(b) = bytes.get_mut(at) {
                *b = rng.byte();
            }
        }
        1 => {
            let at = rng.below(bytes.len() + 1);
            bytes.insert(at, rng.byte());
        }
        _ if !bytes.is_empty() => {
            let at = rng.below(bytes.len());
            bytes.remove(at);
        }
        _ => {}
    }
}

/// Random substitute/insert/delete mutations of every valid frame. One to
/// four mutations per iteration, so both near-valid and doubly-corrupted
/// frames are covered.
#[test]
fn mutated_frames_decode_or_error_without_panic() {
    let frames = all_frames();
    let mut rng = Rng::new(0x1D0_5EED_0000_0001);
    for (n, frame) in frames.iter().enumerate() {
        for i in 0..600 {
            let mut bytes = frame.clone();
            for _ in 0..1 + rng.below(4) {
                mutate(&mut rng, &mut bytes);
            }
            assert_decodes_without_panic(&bytes, &format!("frame {n}, iteration {i}"));
        }
    }
}

/// Fully random byte strings — no valid structure at all — through the frame
/// splitter, the stream reader, and (with a random tag) both message
/// decoders directly.
#[test]
fn fully_random_bytes_never_panic_any_decoder() {
    let mut rng = Rng::new(0x1D0_5EED_0000_0002);
    for i in 0..5_000 {
        let len = rng.below(96);
        let bytes: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
        assert_decodes_without_panic(&bytes, &format!("random iteration {i}"));

        let tag = rng.byte();
        let outcome = std::panic::catch_unwind(|| {
            let _ = ClientMsg::decode(tag, &bytes);
            let _ = DaemonMsg::decode(tag, &bytes);
        });
        assert!(
            outcome.is_ok(),
            "message decoder panicked (random iteration {i}, tag {tag:#04x}) on {bytes:?}"
        );
    }
}

/// Mutated *bodies* fed straight to `ClientMsg::decode`/`DaemonMsg::decode`
/// with their real tag — mirroring how codec.rs bypasses framing — so the
/// frame splitter's length check is never the only thing standing between
/// corrupt bytes and a panic.
#[test]
fn mutated_bodies_past_the_framing_layer_never_panic() {
    let frames = all_frames();
    let mut rng = Rng::new(0x1D0_5EED_0000_0003);
    for (n, frame) in frames.iter().enumerate() {
        let split = wire::split_frame(frame);
        assert!(split.is_ok(), "corpus frame {n} did not split");
        let Ok((tag, body, _)) = split else { continue };
        for i in 0..400 {
            let mut bytes = body.to_vec();
            for _ in 0..1 + rng.below(4) {
                mutate(&mut rng, &mut bytes);
            }
            let outcome = std::panic::catch_unwind(|| {
                let _ = ClientMsg::decode(tag, &bytes);
                let _ = DaemonMsg::decode(tag, &bytes);
            });
            assert!(
                outcome.is_ok(),
                "body decoder panicked (frame {n}, iteration {i}, tag {tag:#04x}) on {bytes:?}"
            );
        }
    }
}
