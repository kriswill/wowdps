//! R10: instance visits and the per-visit Overall, against the committed
//! instance fixture — full replay semantics, scanner parity, lazy-load
//! parity, and checkpoint resumption with a visit in flight.

use wowdps_core::index::{Index, SegmentMeta, load_segment_text, scan, scan_from};
use wowdps_core::meter::{Meter, SegmentKind, View, meter_from_lines, meter_from_seeded};

const INSTANCE_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/instance.txt");

fn range_lines(bytes: &[u8], ranges: &[(u64, u64)]) -> Vec<String> {
    ranges
        .iter()
        .flat_map(|&(s, e)| {
            let range = bytes.get(s as usize..e as usize);
            assert!(range.is_some(), "range {s}..{e} lies in the log");
            String::from_utf8_lossy(range.unwrap_or_default())
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A slice loaded the way the daemon's loader does: its seeds, then its
/// bytes, replayed through `Meter::seed` and `Meter::feed`.
fn lazy(bytes: &[u8], meta: &SegmentMeta) -> Meter {
    let seeds = range_lines(bytes, &meta.seeds);
    let slice = range_lines(bytes, &[meta.byte_range]);
    meter_from_seeded(
        seeds.iter().map(String::as_str),
        slice.iter().map(String::as_str),
    )
}

/// The live meter the way the tailer builds it: the seed lines before the
/// tail (the open segment's, else the checkpoint's) through `Meter::seed`,
/// then everything from `live_offset` through `Meter::feed`.
fn live_meter(bytes: &[u8], idx: &Index) -> Meter {
    let seeds = match idx.open.as_ref() {
        Some(open) => &open.seeds,
        None => &idx.checkpoint.seeds,
    };
    let seeds = range_lines(bytes, seeds);
    let tail = bytes.get(idx.live_offset as usize..);
    assert!(tail.is_some(), "live_offset lies in the log");
    let tail = String::from_utf8_lossy(tail.unwrap_or_default()).into_owned();
    meter_from_seeded(seeds.iter().map(String::as_str), tail.lines())
}

fn replay() -> Meter {
    let text = std::fs::read_to_string(INSTANCE_FIXTURE);
    assert!(text.is_ok(), "{INSTANCE_FIXTURE}: unreadable fixture");
    let text = text.unwrap_or_default();
    meter_from_lines(text.lines())
}

fn amounts(seg: &wowdps_core::meter::Segment, view: View) -> Vec<(String, u64)> {
    seg.rows(view)
        .into_iter()
        .map(|r| (r.label, r.amount))
        .collect()
}

#[test]
fn visits_and_segment_tags_follow_the_zone_rules() {
    let meter = replay();

    let visits = meter.visits();
    assert_eq!(visits.len(), 3);
    assert_eq!(visits[0].name, "Algeth'ar Academy");
    assert_eq!(visits[0].key_level, None, "the zone-in visit is pre-key");
    assert_eq!(visits[0].affixes, [0u32; 0], "and names no affixes");
    assert_eq!(visits[0].completed, None, "the zeroed reset END is ignored");
    assert!(visits[0].end_ms.is_some(), "closed when the key started");
    assert_eq!(visits[1].name, "Algeth'ar Academy");
    assert_eq!(visits[1].key_level, Some(12));
    assert_eq!(visits[1].affixes, [9, 10], "the START's list, in its order");
    assert_eq!(visits[1].completed, Some(true), "the key timed");
    // R10: a finished key is terminal — the visit closes at its own
    // CHALLENGE_MODE_END, not at the next instance. The Guardian trash
    // still open at that moment closes with it.
    assert_eq!(
        visits[1].end_ms,
        meter.segments()[3].end_ms,
        "the key closed at its END, taking the open trash pull with it"
    );
    assert!(
        visits[1].end_ms.unwrap() < meter.segments()[4].start_ms,
        "the city dummy landed after the key had closed"
    );
    assert_eq!(visits[1].display_name(), "Algeth'ar Academy +12");
    assert_eq!(
        visits[1].start_ms,
        visits[0].end_ms.unwrap(),
        "the key's clock starts at CHALLENGE_MODE_START, not at the door"
    );
    // Bosses carry ENCOUNTER_START identity; trash and arena segments never do.
    let encs: Vec<_> = meter
        .segments()
        .iter()
        .filter_map(|s| s.encounter)
        .map(|e| (e.id, e.difficulty, e.group_size))
        .collect();
    assert_eq!(encs, vec![(2562, 8, 5), (1698, 23, 5)]);
    assert_eq!(
        meter.segments()[0].build,
        (12, 0, 0),
        "seeded from the version line"
    );
    assert_eq!(visits[2].name, "Skyreach");
    assert_eq!(visits[2].difficulty, 23);
    assert_eq!(visits[2].key_level, None);
    assert!(visits[2].end_ms.is_none(), "still in progress at EOF");

    let tags: Vec<(SegmentKind, Option<u32>)> =
        meter.segments().iter().map(|s| (s.kind, s.visit)).collect();
    assert_eq!(
        tags,
        vec![
            (SegmentKind::Trash, Some(0)),     // pre-key Crawler poke
            (SegmentKind::Trash, Some(1)),     // Crawler, once the key ran
            (SegmentKind::Encounter, Some(1)), // Vexamus
            (SegmentKind::Trash, Some(1)),     // Guardian
            (SegmentKind::Trash, None),        // city dummy
            (SegmentKind::Trash, Some(2)),     // Skyblade
            (SegmentKind::Encounter, Some(2)), // Ranjit
            (SegmentKind::Trash, None),        // city dummy while suspended
            (SegmentKind::Trash, Some(2)),     // Skyguard, after re-entry
        ]
    );
    // A zone change closes the open trash segment (R10 amendment to R4).
    assert!(meter.segments()[4].end_ms.is_some(), "city trash closed");
    assert!(meter.segments()[8].end_ms.is_none(), "trailing pull open");
}

#[test]
fn the_overall_accumulates_every_member_counter() {
    let meter = replay();

    // The zone-in visit keeps only the pre-key poke — the keyed run does
    // not inherit it, in clock or in counters.
    let o0 = meter.overall(0).expect("the zone-in visit has a member");
    assert_eq!(o0.kind, SegmentKind::Overall);
    assert_eq!(o0.name, "Algeth'ar Academy");
    assert_eq!(o0.success, None);
    assert_eq!(o0.visit, Some(0));
    assert_eq!(
        amounts(&o0, View::Damage),
        vec![("Ana-Realm".to_string(), 100)]
    );
    assert_eq!(
        o0.duration_ms(i64::MAX),
        0,
        "a single-event trash has no span"
    );

    let o1 = meter.overall(1).expect("the keyed visit has members");
    assert_eq!(o1.name, "Algeth'ar Academy +12");
    assert_eq!(o1.success, Some(true));
    assert_eq!(o1.visit, Some(1));
    // Trash 150 then 50 for Ana, Vexamus 300/200: the pre-key poke and
    // city combat are both excluded.
    assert_eq!(
        amounts(&o1, View::Damage),
        vec![
            ("Ana-Realm".to_string(), 500),
            ("Borin-Realm".to_string(), 200)
        ]
    );
    // The keyed clock is CHALLENGE_MODE_END's official totalMs, not the
    // member combat sum (0s Crawler trash + 60s Vexamus + 0s tail).
    assert_eq!(o1.duration_ms(i64::MAX), 900_000);

    let o2 = meter.overall(2).expect("visit 2 has members");
    assert_eq!(o2.name, "Skyreach");
    assert!(o2.end_ms.is_none(), "live overall");
    assert_eq!(
        amounts(&o2, View::Damage),
        vec![
            ("Borin-Realm".to_string(), 500),
            ("Ana-Realm".to_string(), 180)
        ]
    );
    // 0s Skyblade + 30s Ranjit + the open Skyguard pull cut at its last
    // combat event (15s).
    assert_eq!(o2.duration_ms(i64::MAX), 45_000);
}

#[test]
fn the_scanner_mirrors_visits_and_emits_overall_metas() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    // Segment kinds and visit tags agree with a full replay.
    let scanned: Vec<(SegmentKind, Option<u32>)> = idx
        .segments
        .iter()
        .map(|m| (m.kind, m.visit))
        .chain(idx.open.iter().map(|m| (m.kind, m.visit)))
        .collect();
    let replayed: Vec<(SegmentKind, Option<u32>)> =
        meter.segments().iter().map(|s| (s.kind, s.visit)).collect();
    assert_eq!(scanned, replayed);

    // Encounter identity (id / difficulty / group size) mirrors too.
    let scanned_enc: Vec<_> = idx
        .segments
        .iter()
        .chain(idx.open.iter())
        .map(|m| m.encounter)
        .collect();
    let replayed_enc: Vec<_> = meter.segments().iter().map(|s| s.encounter).collect();
    assert_eq!(scanned_enc, replayed_enc);

    // Both closed visits produced Overall metas matching the replay.
    assert_eq!(idx.overalls.len(), 2);
    for (ord, m) in idx.overalls.iter().enumerate() {
        let want = meter.overall(ord as u32).unwrap();
        assert_eq!(m.kind, SegmentKind::Overall);
        assert_eq!(m.name, want.name);
        assert_eq!(m.success, want.success);
        assert_eq!(m.visit, Some(ord as u32));
        assert_eq!(m.duration_ms, want.duration_ms(i64::MAX));
    }

    // The in-progress visit surfaces as `open_visit`: the prefix the live
    // tail cannot see. The open Skyguard pull is the live meter's — it is
    // excluded from the prefix's bytes and clock, or a prefix + live merge
    // would count it twice.
    let ov = idx.open_visit.as_ref().expect("Skyreach is in progress");
    assert_eq!(ov.name, "Skyreach");
    assert_eq!(ov.visit, Some(2));
    assert_eq!(ov.end_ms, None);
    assert_eq!(
        ov.byte_range.1, idx.live_offset,
        "prefix ends where the live tail begins"
    );
    assert_eq!(
        ov.duration_ms, 30_000,
        "closed members only: 0s Skyblade + 30s Ranjit"
    );
}

#[test]
fn a_lazily_loaded_overall_matches_the_full_replay() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    for (meta, ordinal) in idx.overalls.iter().map(|m| (m, m.visit.unwrap())) {
        let text = load_segment_text(std::path::Path::new(INSTANCE_FIXTURE), meta).unwrap();
        let lazy = text.meter();
        let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
        let want = meter.overall(ordinal).unwrap();
        for view in [View::Damage, View::Healing, View::Deaths, View::Taken] {
            assert_eq!(
                amounts(&got, view),
                amounts(&want, view),
                "{:?} in {}",
                view,
                meta.name
            );
        }
        // R17: the merged mitigation records agree too (raw-keyed, folded
        // on read — a lazy Overall must fold exactly like the full one).
        for r in want.rows(View::Taken) {
            assert_eq!(
                got.mitigation(&r.key),
                want.mitigation(&r.key),
                "R17 mitigation for {} in {}",
                r.label,
                meta.name
            );
        }
        assert_eq!(
            got.duration_ms(i64::MAX),
            want.duration_ms(i64::MAX),
            "{}",
            meta.name
        );
        assert_eq!(got.name, want.name);
        // The visit a stored card is written from (an import parses the
        // Σ lazily, a live close reads the full meter's): its key facts,
        // the affixes among them, agree.
        let facts = |m: &Meter| {
            m.visits()
                .get(ordinal as usize)
                .map(|v| (v.key_level, v.affixes.clone()))
        };
        assert_eq!(facts(&lazy), facts(&meter), "{}", meta.name);
    }
}

/// The daemon attaches mid-visit by composing two halves: the lazily loaded
/// `open_visit` prefix, and the live meter (seed lines + everything from
/// `live_offset`, which rebuilds the open member in full). Their merged
/// Overall must equal a full replay — counters and clock. Double counting
/// the scan-time-open member's head is the regression this gates.
#[test]
fn an_attach_mid_visit_composes_to_the_full_replay() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    // The prefix, loaded the way the daemon's loader does.
    let ov = idx.open_visit.as_ref().expect("Skyreach is in progress");
    let prefix = load_segment_text(std::path::Path::new(INSTANCE_FIXTURE), ov).unwrap();
    let prefix = prefix.meter();

    // The live side, built the way the tailer feeds it: the open segment's
    // seed lines, then everything from `live_offset`.
    let live = live_meter(&bytes, &idx);

    // The daemon's LiveOverall merge: live members + the lazy prefix.
    let mut combined = live.overall(2).expect("the open pull is a live member");
    combined.absorb_prefix(&prefix.overall(2).expect("prefix holds the closed members"));

    let want = meter.overall(2).unwrap();
    assert_eq!(
        amounts(&combined, View::Damage),
        amounts(&want, View::Damage),
        "no member counted twice, none dropped"
    );
    assert_eq!(
        combined.duration_ms(0),
        want.duration_ms(i64::MAX),
        "the visit clock survives the split"
    );
}

#[test]
fn a_resumed_scan_matches_a_full_scan_mid_visit() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let full = scan(&mut &bytes[..]);
    let cuts: Vec<usize> = bytes
        .iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'\n')
        .map(|(i, _)| i + 1)
        .chain([bytes.len() / 2, bytes.len()])
        .collect();
    for cut in cuts {
        let prefix = scan(&mut &bytes[..cut]);
        let state = prefix.checkpoint.clone();
        let off = state.offset as usize;
        let resumed = scan_from(&mut &bytes[off..], state);
        assert_eq!(resumed.segments, full.segments, "cut at {cut}");
        assert_eq!(resumed.overalls, full.overalls, "cut at {cut}");
        assert_eq!(resumed.open_visit, full.open_visit, "cut at {cut}");
        assert_eq!(resumed.open, full.open, "cut at {cut}");
        assert_eq!(resumed.checkpoint, full.checkpoint, "cut at {cut}");
    }
}

/// R10, the two arms of a keystone's end. A key that fires
/// `CHALLENGE_MODE_END` is finished and its visit is terminal; a key
/// ABANDONED before its END is not, so zoning out must still only suspend
/// it and re-entry must resume the same visit. The terminal rule keys on
/// the END line alone, never on the zone change, and the scanner mirrors
/// both arms exactly.
const KEY_HEAD: &str = "\
8/1/2026 12:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1
8/1/2026 12:00:05.000-7  ZONE_CHANGE,2526,\"Algeth'ar Academy\",8
8/1/2026 12:00:10.000-7  CHALLENGE_MODE_START,\"Algeth'ar Academy\",2526,558,12,[9,10]
8/1/2026 12:00:20.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Crawler\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
";

const KEY_TAIL: &str = "\
8/1/2026 12:02:00.000-7  ZONE_CHANGE,0,\"Silvermoon City\",0
8/1/2026 12:03:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-4,\"Dummy\",0xa48,0x0,116,\"Frostbolt\",16,50,50,0,0,0,0,0,nil,nil
8/1/2026 12:05:00.000-7  ZONE_CHANGE,2526,\"Algeth'ar Academy\",23
8/1/2026 12:06:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-5,\"Guardian\",0xa48,0x0,116,\"Frostbolt\",16,70,70,0,0,0,0,0,nil,nil
";

const KEY_END: &str =
    "8/1/2026 12:01:30.000-7  CHALLENGE_MODE_END,2526,1,12,900000,300.000000,3029\n";

fn tags(text: &str) -> Vec<(SegmentKind, Option<u32>)> {
    let meter = meter_from_lines(text.lines());
    let replayed: Vec<_> = meter.segments().iter().map(|s| (s.kind, s.visit)).collect();
    let idx = scan(&mut text.as_bytes());
    let scanned: Vec<_> = idx
        .segments
        .iter()
        .map(|m| (m.kind, m.visit))
        .chain(idx.open.iter().map(|m| (m.kind, m.visit)))
        .collect();
    assert_eq!(scanned, replayed, "the scanner mirrors the replay");
    replayed
}

#[test]
fn an_abandoned_key_still_suspends_and_resumes() {
    let text = format!("{KEY_HEAD}{KEY_TAIL}");
    let meter = meter_from_lines(text.lines());

    let visits = meter.visits();
    assert_eq!(visits.len(), 2, "the zone-in visit, then the key");
    assert_eq!(visits[1].key_level, Some(12));
    assert_eq!(visits[1].completed, None, "no END ever fired");
    assert_eq!(
        visits[1].end_ms, None,
        "zoning out of an unfinished key only suspends it"
    );
    assert_eq!(
        tags(&text),
        vec![
            (SegmentKind::Trash, Some(1)), // inside the key
            (SegmentKind::Trash, None),    // the city, while suspended
            (SegmentKind::Trash, Some(1)), // re-entry resumes the SAME visit
        ]
    );
}

#[test]
fn a_finished_key_is_terminal() {
    let text = format!("{KEY_HEAD}{KEY_END}{KEY_TAIL}");
    let meter = meter_from_lines(text.lines());

    let visits = meter.visits();
    assert_eq!(visits.len(), 3, "re-entry opens a NEW visit, not a resume");
    assert_eq!(visits[1].key_level, Some(12));
    assert_eq!(visits[1].completed, Some(true));
    assert_eq!(
        visits[1].end_ms,
        Some(meter.segments()[0].end_ms.unwrap()),
        "the key closed at its own END, with the open pull"
    );
    assert_eq!(visits[2].key_level, None, "the re-entry is a plain visit");
    assert_eq!(
        tags(&text),
        vec![
            (SegmentKind::Trash, Some(1)), // inside the key
            (SegmentKind::Trash, None),    // the city
            (SegmentKind::Trash, Some(2)), // re-entry, a fresh visit
        ]
    );

    // The point of the whole rule: the visit's Overall exists the moment
    // the key ends, which is what the daemon turns into a history card.
    let idx = scan(&mut text.as_bytes());
    let o = idx
        .overalls
        .iter()
        .find(|m| m.visit == Some(1))
        .expect("the finished key emitted an Overall meta without a later instance");
    assert_eq!(o.name, "Algeth'ar Academy +12");
    assert_eq!(o.success, Some(true));
    assert_eq!(o.duration_ms, 900_000, "the official key clock");
    assert!(
        idx.open_visit.as_ref().is_none_or(|v| v.visit != Some(1)),
        "a finished key is never left open at EOF"
    );
}

/// The re-run: a key finishes, the party resets the dungeon and runs it
/// again with no ZONE_CHANGE at difficulty ≠ 0 in between (the game logs
/// the re-entry at difficulty 0, then its zeroed reset END, then a fresh
/// START). Terminality must not eat the second run — the reset marker
/// carries no totalMs, and the finished visit stays current precisely so
/// the START can read its map, difficulty and name.
#[test]
fn a_finished_key_can_be_reset_and_re_run() {
    let text = format!(
        "{KEY_HEAD}{KEY_END}\
8/1/2026 12:02:00.000-7  ZONE_CHANGE,2526,\"Algeth'ar Academy\",0
8/1/2026 12:02:30.000-7  CHALLENGE_MODE_END,2526,0,0,0,0.000000,0.000000
8/1/2026 12:02:31.000-7  CHALLENGE_MODE_START,\"Algeth'ar Academy\",2526,558,12,[9,10]
8/1/2026 12:03:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-6,\"Crawler\",0xa48,0x0,116,\"Frostbolt\",16,90,90,0,0,0,0,0,nil,nil
8/1/2026 12:20:00.000-7  CHALLENGE_MODE_END,2526,1,12,960000,300.000000,3029
"
    );
    let meter = meter_from_lines(text.lines());

    let visits = meter.visits();
    assert_eq!(visits.len(), 3, "the door, the key, the re-run");
    assert_eq!(
        visits[1].official_ms,
        Some(900_000),
        "the first run's clock"
    );
    assert_eq!(
        visits[2].key_level,
        Some(12),
        "the re-run is keyed — the START was not dropped"
    );
    assert_eq!(visits[2].official_ms, Some(960_000));
    assert!(visits[2].end_ms.is_some(), "and it, too, closed at its END");
    assert_eq!(
        tags(&text),
        vec![
            (SegmentKind::Trash, Some(1)), // the first run
            (SegmentKind::Trash, Some(2)), // the re-run
        ]
    );

    // Both runs are Overalls the daemon can store, on their own clocks.
    let idx = scan(&mut text.as_bytes());
    let keys: Vec<_> = idx
        .overalls
        .iter()
        .filter(|m| m.name.contains('+'))
        .map(|m| (m.duration_ms, m.success))
        .collect();
    assert_eq!(keys, vec![(900_000, Some(true)), (960_000, Some(true))]);
    assert_eq!(idx.open_visit, None, "neither run is left open");
}

/// R10 amendment: the door logged difficulty 0 (Voidscar Arena on a real
/// +14), so no visit stood on the map when the START fired. The START opens
/// the keyed visit itself — named from its own field, at the keystone
/// difficulty — and the END closes it, so the run has a Σ; the scanner
/// mirrors it, and the stale previous visit is closed, never reused. Leaving
/// a key mid-run is legal (the shop, a talent swap): a re-entry through the
/// same 0-logged door RESUMES the run, it is neither a new visit nor a fail.
#[test]
fn a_start_with_no_visit_on_its_map_opens_the_key_itself() {
    let text = "\
8/1/2026 12:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1
8/1/2026 12:00:05.000-7  ZONE_CHANGE,1209,\"Skyreach\",23
8/1/2026 12:00:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Crawler\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/1/2026 12:01:00.000-7  ZONE_CHANGE,0,\"Silvermoon City\",0
8/1/2026 12:02:00.000-7  ZONE_CHANGE,2923,\"Voidscar Arena\",0
8/1/2026 12:02:30.000-7  CHALLENGE_MODE_END,2923,0,0,0,0.000000,0.000000
8/1/2026 12:02:31.000-7  CHALLENGE_MODE_START,\"Voidscar Arena\",2923,585,14,[9,10,147]
8/1/2026 12:03:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-6,\"Felguard\",0xa48,0x0,116,\"Frostbolt\",16,90,90,0,0,0,0,0,nil,nil
8/1/2026 12:03:30.000-7  ZONE_CHANGE,2771,\"Slayer's Rise\",0
8/1/2026 12:03:40.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Dummy\",0xa48,0x0,116,\"Frostbolt\",16,5,5,0,0,0,0,0,nil,nil
8/1/2026 12:04:30.000-7  ZONE_CHANGE,2923,\"Voidscar Arena\",0
8/1/2026 12:05:00.000-7  ENCOUNTER_START,3285,\"Taz'Rah\",8,5,2923
8/1/2026 12:05:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-7,\"Taz'Rah\",0xa48,0x0,116,\"Frostbolt\",16,80,80,0,0,0,0,0,nil,nil
8/1/2026 12:06:00.000-7  ENCOUNTER_END,3285,\"Taz'Rah\",8,5,1,60000
8/1/2026 12:06:00.133-7  CHALLENGE_MODE_END,2923,1,14,1761469,395.802734,3159.903564
8/1/2026 12:06:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-8,\"Straggler\",0xa48,0x0,116,\"Frostbolt\",16,10,10,0,0,0,0,0,nil,nil
";
    let meter = meter_from_lines(text.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 2, "Skyreach, then the key: {visits:?}");
    assert!(visits[0].end_ms.is_some(), "the stale visit closed");
    let key = &visits[1];
    assert_eq!(key.display_name(), "Voidscar Arena +14");
    assert_eq!(key.map_id, 2923);
    assert_eq!(key.difficulty, wowdps_core::meter::KEYSTONE_DIFFICULTY);
    assert!(key.keyed);
    assert_eq!(key.completed, Some(true));
    assert_eq!(key.official_ms, Some(1_761_469));
    assert!(key.end_ms.is_some(), "the END closed it");
    assert_eq!(
        tags(text),
        vec![
            (SegmentKind::Trash, Some(0)),     // Skyreach
            (SegmentKind::Trash, Some(1)),     // the key's trash
            (SegmentKind::Trash, None),        // outside, mid-key: legal, no visit
            (SegmentKind::Encounter, Some(1)), // Taz'Rah — the 0-door re-entry resumed the key
            (SegmentKind::Trash, None),        // after the END: no visit
        ]
    );
    let idx = scan(&mut text.as_bytes());
    let keys: Vec<_> = idx
        .overalls
        .iter()
        .filter(|m| m.name.contains('+'))
        .map(|m| (m.name.clone(), m.duration_ms, m.success))
        .collect();
    assert_eq!(
        keys,
        vec![("Voidscar Arena +14".to_string(), 1_761_469, Some(true))]
    );
    assert_eq!(idx.open_visit, None);
}

/// A key joined MID-RUN (coach retest 38: a Ruby Life Pools +12 pug) never
/// sees its CHALLENGE_MODE_START. The door opens a plain visit at the
/// keystone difficulty, and before this rule the run closed as an unkeyed
/// zone Overall — no level, no verdict, no timers, never a key card. The
/// finished END keys that visit from its own fields, and the map's one
/// dungeon supplies the timers.
const MID_RUN: &str = "\
8/1/2026 12:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1
8/1/2026 12:00:05.000-7  ZONE_CHANGE,2521,\"Ruby Life Pools\",8
8/1/2026 12:00:20.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Primal Juggernaut\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/1/2026 12:01:00.000-7  ENCOUNTER_START,2609,\"Melidrussa Chillworn\",8,5,2521
8/1/2026 12:01:05.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-2,\"Melidrussa Chillworn\",0xa48,0x0,116,\"Frostbolt\",16,300,300,0,0,0,0,0,nil,nil
8/1/2026 12:02:00.000-7  ENCOUNTER_END,2609,\"Melidrussa Chillworn\",8,5,1,55000
8/1/2026 12:22:00.000-7  CHALLENGE_MODE_END,2521,1,12,1404488,301.000000,3141
8/1/2026 12:22:30.000-7  ZONE_CHANGE,0,\"Valdrakken\",0
";

/// Ruby Life Pools (challenge 399, map 2521): par, +2, +3.
const RUBY_PARS: Option<(i64, i64, i64)> = Some((1_680_000, 1_344_000, 1_008_000));

#[test]
fn a_key_joined_mid_run_is_keyed_by_its_end() {
    let meter = meter_from_lines(MID_RUN.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 1, "no START, so the door's visit IS the key");
    let key = &visits[0];
    assert!(key.keyed);
    assert_eq!(key.key_level, Some(12));
    assert_eq!(key.affixes, [0u32; 0], "an END names no affixes");
    assert_eq!(key.display_name(), "Ruby Life Pools +12");
    assert_eq!(key.completed, Some(true));
    assert_eq!(key.official_ms, Some(1_404_488));
    assert_eq!(key.pars_ms, RUBY_PARS, "the timers, found by the END's map");
    assert!(key.end_ms.is_some(), "a finished key is terminal");
    assert!(key.joined);
    // The clock is the span the log saw, door to END: the damage before the
    // join was never logged, so the key timer would understate every rate.
    // The game's own run time still decides the verdict.
    let overall = meter.overall(0).expect("the key has members");
    assert_eq!(overall.duration_ms(0), 1_315_000);
    assert_eq!(overall.success, Some(true), "23:24 against a 28:00 par");
    assert_eq!(
        tags(MID_RUN),
        vec![
            (SegmentKind::Trash, Some(0)),
            (SegmentKind::Encounter, Some(0)),
        ]
    );

    // The scanner keys it too: the Overall the daemon turns into a card.
    let idx = scan(&mut MID_RUN.as_bytes());
    let keys: Vec<_> = idx
        .overalls
        .iter()
        .filter(|m| m.name.contains('+'))
        .map(|m| (m.name.clone(), m.duration_ms, m.success, m.pars_ms))
        .collect();
    assert_eq!(
        keys,
        vec![(
            "Ruby Life Pools +12".to_string(),
            1_315_000,
            Some(true),
            RUBY_PARS
        )]
    );
    assert_eq!(idx.open_visit, None);
}

/// The zeroed reset END the game fires on entry carries no totalMs: on an
/// unkeyed visit it keys nothing, or every door would become a +0 key.
#[test]
fn a_zeroed_end_on_an_unkeyed_visit_keys_nothing() {
    let text = "\
8/1/2026 12:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1
8/1/2026 12:00:05.000-7  ZONE_CHANGE,2521,\"Ruby Life Pools\",8
8/1/2026 12:00:20.000-7  CHALLENGE_MODE_END,2521,0,0,0,0.000000,0.000000
8/1/2026 12:00:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Primal Juggernaut\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
";
    let meter = meter_from_lines(text.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 1);
    assert!(!visits[0].keyed);
    assert_eq!(visits[0].key_level, None);
    assert_eq!(visits[0].completed, None);
    assert!(visits[0].end_ms.is_none(), "still in progress");
    assert_eq!(tags(text), vec![(SegmentKind::Trash, Some(0))]);
}

/// R10 amendment: a pull zones in the door that led to it did not. A real
/// raid night (2026-10-04) switched The Venomous Abyss from Heroic to Mythic
/// outside, and every door back in logged difficulty 0 — so no Mythic visit
/// opened, the Mythic pulls landed outside any visit, and the Heroic visit
/// stayed current, and live, for the rest of the night. The first Mythic
/// pull closes the Heroic visit and opens the Mythic one; after a trip to
/// town through the same 0-logged door the next pull resumes it. The trash
/// between a 0-door and its first pull stays outside.
const DIFFICULTY_SWITCH: &str = "\
10/4/2026 19:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
10/4/2026 19:00:05.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",15
10/4/2026 19:01:00.000-7  ENCOUNTER_START,3429,\"The Coiled Altar\",15,25,3004
10/4/2026 19:01:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"The Coiled Altar\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
10/4/2026 19:02:00.000-7  ENCOUNTER_END,3429,\"The Coiled Altar\",15,25,1,60000
10/4/2026 19:03:00.000-7  ZONE_CHANGE,2916,\"Vaults of Atal'Utek\",0
10/4/2026 19:04:00.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",0
10/4/2026 19:04:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-2,\"Venomfang Juggernaut\",0xa48,0x0,116,\"Frostbolt\",16,40,40,0,0,0,0,0,nil,nil
10/4/2026 19:05:00.000-7  ENCOUNTER_START,3470,\"Nek'zali the Soulcoiler\",16,20,3004
10/4/2026 19:05:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-3,\"Nek'zali the Soulcoiler\",0xa48,0x0,116,\"Frostbolt\",16,200,200,0,0,0,0,0,nil,nil
10/4/2026 19:06:00.000-7  ENCOUNTER_END,3470,\"Nek'zali the Soulcoiler\",16,20,0,60000
10/4/2026 19:06:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-4,\"Soulcoiler Revenant\",0xa48,0x0,116,\"Frostbolt\",16,30,30,0,0,0,0,0,nil,nil
10/4/2026 19:08:00.000-7  ZONE_CHANGE,0,\"Silvermoon City\",0
10/4/2026 19:08:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-5,\"Training Dummy\",0xa48,0x0,116,\"Frostbolt\",16,5,5,0,0,0,0,0,nil,nil
10/4/2026 19:09:00.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",0
10/4/2026 19:10:00.000-7  ENCOUNTER_START,3497,\"The Lost Explorers\",16,20,3004
10/4/2026 19:10:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-6,\"The Lost Explorers\",0xa48,0x0,116,\"Frostbolt\",16,300,300,0,0,0,0,0,nil,nil
10/4/2026 19:11:00.000-7  ENCOUNTER_END,3497,\"The Lost Explorers\",16,20,1,60000
10/4/2026 19:11:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-7,\"First Mate Nama\",0xa48,0x0,116,\"Frostbolt\",16,1,1,0,0,0,0,0,nil,nil
";

#[test]
fn a_pull_behind_a_door_logged_at_zero_zones_in() {
    let text = DIFFICULTY_SWITCH;
    let meter = meter_from_lines(text.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 2, "Heroic, then Mythic: {visits:?}");
    let (heroic, mythic) = (&visits[0], &visits[1]);
    assert_eq!((heroic.map_id, heroic.difficulty), (3004, 15));
    let first_mythic_pull = meter.segments()[2].start_ms;
    assert_eq!(
        heroic.end_ms,
        Some(first_mythic_pull),
        "the first Mythic pull closed the Heroic visit"
    );
    assert_eq!((mythic.map_id, mythic.difficulty), (3004, 16));
    assert_eq!(mythic.display_name(), "The Venomous Abyss");
    assert_eq!(mythic.start_ms, first_mythic_pull);
    assert_eq!(mythic.end_ms, None, "still going");
    assert_eq!(
        tags(text),
        vec![
            (SegmentKind::Encounter, Some(0)), // The Coiled Altar, Heroic
            (SegmentKind::Trash, None),        // behind the 0-door, before a pull
            (SegmentKind::Encounter, Some(1)), // Nek'zali opens the Mythic visit
            (SegmentKind::Trash, Some(1)),     // zoned in now
            (SegmentKind::Trash, None),        // the town dummy
            (SegmentKind::Encounter, Some(1)), // the next pull resumes it
            (SegmentKind::Trash, Some(1)),     // still open
        ]
    );

    let bytes = text.as_bytes();
    let idx = scan(&mut &bytes[..]);
    let closed: Vec<_> = idx
        .overalls
        .iter()
        .map(|m| (m.visit, m.start_ms, m.end_ms))
        .collect();
    assert_eq!(
        closed,
        vec![(Some(0), heroic.start_ms, heroic.end_ms)],
        "the Heroic Σ is emitted, closed where the Mythic visit began"
    );
    let open = idx
        .open_visit
        .as_ref()
        .expect("the Mythic visit is in progress");
    assert_eq!(open.visit, Some(1));

    let live = agrees_everywhere(text, &meter);
    assert_eq!(live.visits().len(), 2, "the seeds rebuilt both visits");
    assert_eq!(live.segments().last().and_then(|s| s.visit), Some(1));
}

/// R10's agreement over one log: every slice rebuilds alone and in its own
/// visit (a seeded START opens no segment and restores the visit it opened
/// or resumed), every closed Σ loads lazily as the replay holds it, the
/// visit in progress composes from its closed members' lazy prefix and the
/// live meter (seeds, then everything from the open segment on) to the
/// replay's Σ, as when the daemon attached mid-visit, and a checkpoint at
/// any line resumes to the full scan. The live meter, for the caller's
/// own checks.
// Test code outside a `#[test]`, where clippy's test allowances do not
// reach: a panic is how it fails.
#[allow(clippy::expect_used, clippy::indexing_slicing)]
fn agrees_everywhere(text: &str, meter: &Meter) -> Meter {
    let bytes = text.as_bytes();
    let idx = scan(&mut &bytes[..]);
    for meta in idx.segments.iter().chain(idx.open.iter()) {
        let lazy = lazy(bytes, meta);
        assert_eq!(lazy.segments().len(), 1, "one segment: {}", meta.name);
        assert_eq!(lazy.segments()[0].visit, meta.visit, "{}", meta.name);
        let place = |v: &wowdps_core::meter::Visit| (v.map_id, v.difficulty, v.start_ms);
        let rebuilt: Vec<_> = lazy.visits().iter().map(place).collect();
        let want: Vec<_> = meter
            .visits()
            .iter()
            .take(rebuilt.len())
            .map(place)
            .collect();
        assert_eq!(rebuilt, want, "the visit table up to {}", meta.name);
    }
    for meta in idx.overalls.iter() {
        let ord = meta.visit.expect("an Overall names its visit");
        let got = lazy(bytes, meta)
            .overall(ord)
            .expect("the slice holds the visit");
        let want = meter.overall(ord).expect("the replay holds the visit");
        assert_eq!(amounts(&got, View::Damage), amounts(&want, View::Damage));
    }

    let live = live_meter(bytes, &idx);
    if let Some(open) = idx.open_visit.as_ref() {
        let ord = open.visit.expect("the visit in progress names its ordinal");
        let prefix = lazy(bytes, open).overall(ord).expect("the closed members");
        // The open segment is the live tail's alone; a suspended visit has
        // no member there.
        let combined = match live.overall(ord) {
            Some(mut live_part) => {
                live_part.absorb_prefix(&prefix);
                live_part
            }
            None => prefix,
        };
        let want = meter.overall(ord).expect("the replay holds the visit");
        assert_eq!(
            amounts(&combined, View::Damage),
            amounts(&want, View::Damage)
        );
        // An unkeyed Σ's clock is its members' (the open one cut at its
        // last combat), whatever now is; a key's runs on the wall clock
        // until its END.
        if meter.visits().get(ord as usize).is_some_and(|v| v.keyed) {
            for now in [0, i64::MAX] {
                assert_eq!(combined.duration_ms(now), want.duration_ms(now));
            }
        } else {
            assert_eq!(combined.duration_ms(0), want.duration_ms(i64::MAX));
        }
    }

    let full = scan(&mut &bytes[..]);
    let cuts = bytes
        .iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'\n')
        .map(|(i, _)| i + 1);
    for cut in cuts {
        let state = scan(&mut &bytes[..cut]).checkpoint;
        let off = state.offset as usize;
        let resumed = scan_from(&mut &bytes[off..], state);
        assert_eq!(resumed.segments, full.segments, "cut at {cut}");
        assert_eq!(resumed.overalls, full.overalls, "cut at {cut}");
        assert_eq!(resumed.open_visit, full.open_visit, "cut at {cut}");
        assert_eq!(resumed.open, full.open, "cut at {cut}");
    }
    live
}

/// The amendment's edges. A delve's door logs 0 too, and its pull opens the
/// delve's visit; a world boss's pull (Difficulty.db2's World Boss, an
/// InstanceType of 0) is open-world content and stays outside; a pull on a
/// map no door named opens its visit under the edition's table's name when
/// that table names the map (a party or raid map), and stays outside on a map the table
/// does not know; and a door that zoned us in stands, even when its
/// difficulty and the pull's disagree.
#[test]
fn a_pull_zones_in_only_instanced_content_a_door_or_its_table_names() {
    let delve = "\
8/29/2026 17:50:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
8/29/2026 17:50:54.000-7  ZONE_CHANGE,3077,\"The Ring of Glory\",0
8/29/2026 17:52:00.000-7  ENCOUNTER_START,3514,\"Gnok\",208,2,3077
8/29/2026 17:52:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Gnok\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/29/2026 17:53:00.000-7  ENCOUNTER_END,3514,\"Gnok\",208,2,1,60000
";
    let meter = meter_from_lines(delve.lines());
    let names: Vec<_> = meter
        .visits()
        .iter()
        .map(|v| (v.display_name(), v.difficulty))
        .collect();
    assert_eq!(names, vec![("The Ring of Glory".to_string(), 208)]);
    assert_eq!(tags(delve), vec![(SegmentKind::Encounter, Some(0))]);

    let world_boss = "\
8/29/2026 17:50:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
8/29/2026 17:50:54.000-7  ZONE_CHANGE,2916,\"Vaults of Atal'Utek\",0
8/29/2026 17:52:00.000-7  ENCOUNTER_START,3600,\"A World Boss\",172,40,2916
8/29/2026 17:52:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"A World Boss\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/29/2026 17:53:00.000-7  ENCOUNTER_END,3600,\"A World Boss\",172,40,1,60000
8/29/2026 17:54:00.000-7  ENCOUNTER_START,3601,\"Elsewhere\",15,20,3004
8/29/2026 17:55:00.000-7  ENCOUNTER_END,3601,\"Elsewhere\",15,20,1,60000
";
    // "Elsewhere" is on 3004, a raid in Retail's Map.db2 (InstanceType 2),
    // behind a door onto 2916, which Map.db2 calls open world: the shape of
    // Forever's walk-in door. It stayed outside only because no door gave
    // it a name; the table now does (R10, 2026-11-13), and the START's
    // instance is where the pull is, as for a resume.
    let meter = meter_from_lines(world_boss.lines());
    let names: Vec<_> = meter
        .visits()
        .iter()
        .map(|v| (v.map_id, v.display_name(), v.difficulty))
        .collect();
    assert_eq!(
        names,
        vec![(3004, "The Venomous Abyss".to_string(), 15)],
        "the table's name, never the outdoor door's"
    );
    assert_eq!(
        tags(world_boss),
        vec![
            (SegmentKind::Encounter, None),    // the world boss
            (SegmentKind::Encounter, Some(0)), // Elsewhere opens its raid's visit
        ]
    );

    // A map the edition's table does not know (a newer patch's) has no name
    // to open a visit under: its pull stays outside, as before.
    let unknown = "\
8/29/2026 17:50:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
8/29/2026 17:50:54.000-7  ZONE_CHANGE,2916,\"Vaults of Atal'Utek\",0
8/29/2026 17:54:00.000-7  ENCOUNTER_START,3602,\"Nowhere Known\",15,20,9999
8/29/2026 17:54:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-2,\"Nowhere Known\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/29/2026 17:55:00.000-7  ENCOUNTER_END,3602,\"Nowhere Known\",15,20,1,60000
";
    assert!(meter_from_lines(unknown.lines()).visits().is_empty());
    assert_eq!(tags(unknown), vec![(SegmentKind::Encounter, None)]);

    // The table names party and raid maps alone: a scenario's pull
    // (Proving Grounds, 1148, InstanceType 5) and a battleground's (Classic
    // Arathi Basin, 529, InstanceType 3), each with encounters in the
    // client, stay outside behind a door that did not name them.
    let neither = "\
8/29/2026 17:50:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
8/29/2026 17:50:54.000-7  ZONE_CHANGE,2916,\"Vaults of Atal'Utek\",0
8/29/2026 17:54:00.000-7  ENCOUNTER_START,3603,\"A Scenario Boss\",12,1,1148
8/29/2026 17:54:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-3,\"A Scenario Boss\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/29/2026 17:55:00.000-7  ENCOUNTER_END,3603,\"A Scenario Boss\",12,1,1,60000
8/29/2026 17:56:00.000-7  ENCOUNTER_START,3604,\"A Battleground Boss\",34,15,529
8/29/2026 17:56:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-4,\"A Battleground Boss\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/29/2026 17:57:00.000-7  ENCOUNTER_END,3604,\"A Battleground Boss\",34,15,1,60000
";
    assert!(meter_from_lines(neither.lines()).visits().is_empty());
    assert_eq!(
        tags(neither),
        vec![
            (SegmentKind::Encounter, None),
            (SegmentKind::Encounter, None)
        ]
    );

    let door_stands = "\
8/29/2026 17:50:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
8/29/2026 17:50:54.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",16
8/29/2026 17:52:00.000-7  ENCOUNTER_START,3429,\"The Coiled Altar\",15,20,3004
8/29/2026 17:53:00.000-7  ENCOUNTER_END,3429,\"The Coiled Altar\",15,20,1,60000
";
    let meter = meter_from_lines(door_stands.lines());
    assert_eq!(meter.visits().len(), 1);
    assert_eq!(
        meter.visits()[0].difficulty,
        16,
        "the door's difficulty stands"
    );
    assert_eq!(tags(door_stands), vec![(SegmentKind::Encounter, Some(0))]);
}

const REJOIN_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/rejoin.txt");

/// The fixture's door back into the instance: a ZONE_CHANGE at difficulty
/// 0 onto map 0, whatever its name. Every pull after it names map 2959.
const DOOR_BACK: &str = "ZONE_CHANGE,0,\"City of Dalaran\",0";

fn rejoin() -> String {
    let text = std::fs::read_to_string(REJOIN_FIXTURE);
    assert!(text.is_ok(), "{REJOIN_FIXTURE}: unreadable fixture");
    text.unwrap_or_default()
}

/// The fixture with every line from its door back in on changed by `edit`.
fn behind_the_door(text: &str, edit: impl Fn(&str) -> String) -> String {
    let at = text.find(DOOR_BACK);
    assert!(at.is_some(), "the fixture's door back in");
    let (head, tail) = text.split_at(at.unwrap_or(text.len()));
    format!("{head}{}", edit(tail))
}

/// The fixture's segments in file order: three kills inside the visit, a
/// zone-out, and `after` for the five segments from the door back in on
/// (the trash before the first pull there, a pull, trash, a pull, trash).
fn rejoin_tags(after: [Option<u32>; 4]) -> Vec<(SegmentKind, Option<u32>)> {
    let [shade, between, fel, tail] = after;
    vec![
        (SegmentKind::Trash, Some(0)),     // inside, before the first pull
        (SegmentKind::Encounter, Some(0)), // Atrexis the Grave Knight
        (SegmentKind::Encounter, Some(0)), // Arcane Anomaly
        (SegmentKind::Encounter, Some(0)), // Mana Devourer
        (SegmentKind::Trash, Some(0)),     // inside, before the zone-out
        (SegmentKind::Trash, None),        // Hillsbrad Foothills: zoned out
        (SegmentKind::Trash, None),        // behind the door back in, before a pull
        (SegmentKind::Encounter, shade),   // Shade of the Archmage
        (SegmentKind::Trash, between),
        (SegmentKind::Encounter, fel), // Fel Ancient
        (SegmentKind::Trash, tail),    // still open
    ]
}

/// R10 amendment (2026-11-13): a pull rejoins its visit whatever door led
/// back. Forever can log a door into an instance as a ZONE_CHANGE at
/// difficulty 0 onto another map id, whatever name it carries: on a real
/// run the door back into a Normal dungeon visit on map 2959 logged
/// `ZONE_CHANGE,0,"City of Dalaran",0`, and the pulls after it,
/// whose STARTs named 2959, joined no visit while the resume also had to
/// stand on the last door's map. The Σ held three of five kills.
/// `rejoin.txt` is that run's shape: five players, the two later pulls
/// resume the visit, and the Σ holds every kill.
#[test]
fn a_pull_rejoins_its_visit_behind_a_door_onto_another_map() {
    let text = rejoin();
    let meter = meter_from_lines(text.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 1, "one visit: {visits:?}");
    let visit = &visits[0];
    assert_eq!((visit.map_id, visit.difficulty), (2959, 1));
    assert_eq!(visit.display_name(), "Dalaran");
    assert_eq!(visit.end_ms, None, "suspended, never closed");
    assert_eq!(
        tags(&text),
        rejoin_tags([Some(0), Some(0), Some(0), Some(0)])
    );
    let pulls: Vec<_> = meter
        .segments()
        .iter()
        .filter(|s| s.kind == SegmentKind::Encounter)
        .map(|s| (s.name.as_str(), s.success, s.visit))
        .collect();
    assert_eq!(
        pulls,
        vec![
            ("Atrexis the Grave Knight", Some(true), Some(0)),
            ("Arcane Anomaly", Some(true), Some(0)),
            ("Mana Devourer", Some(true), Some(0)),
            ("Shade of the Archmage", Some(true), Some(0)),
            ("Fel Ancient", Some(true), Some(0)),
        ],
        "every pull is the visit's member"
    );

    // Pull k's hit for the p-th player is 100k + 10p: 1500 + 50p over the
    // five pulls. With the trash inside the visit (Ana 7, Borin 9, Dessa
    // 11, Eryk 13); never Ana's 1000 in Hillsbrad, nor Cael's 500 behind the
    // door back in before its first pull, which stays outside.
    let whole = vec![
        ("Eryk-Realm".to_string(), 1763),
        ("Dessa-Realm".to_string(), 1711),
        ("Cael-Realm".to_string(), 1650),
        ("Borin-Realm".to_string(), 1609),
        ("Ana-Realm".to_string(), 1557),
    ];
    let sigma = meter.overall(0).expect("the visit has members");
    assert_eq!(amounts(&sigma, View::Damage), whole);

    // The pull that rejoined is a seed for every later slice, so a lazy
    // load of the trash after it knows it is inside.
    let bytes = text.as_bytes();
    let idx = scan(&mut &bytes[..]);
    assert!(idx.overalls.is_empty(), "the visit never closed");
    assert_eq!(idx.open_visit.as_ref().and_then(|m| m.visit), Some(0));
    let fel = idx
        .segments
        .iter()
        .find(|m| m.name == "Fel Ancient")
        .expect("the last pull is closed");
    let seeded: Vec<_> = range_lines(bytes, &fel.seeds)
        .into_iter()
        .filter(|l| l.contains("ENCOUNTER_START"))
        .collect();
    assert_eq!(seeded.len(), 1, "one START moved the visit: {seeded:?}");
    assert!(seeded[0].contains("\"Shade of the Archmage\""));

    let live = agrees_everywhere(&text, &meter);
    assert_eq!(live.visits().len(), 1, "the seeds rebuilt the visit");
    assert_eq!(live.segments().last().and_then(|s| s.visit), Some(0));

    // A door into another instance closes the visit: its Σ is emitted as an
    // Overall meta, and its lazy load holds every rejoined member.
    let closed =
        format!("{text}11/3/2026 19:20:00.000-8  ZONE_CHANGE,48,\"Blackfathom Deeps\",1\n");
    let meter = meter_from_lines(closed.lines());
    let places: Vec<_> = meter.visits().iter().map(|v| v.map_id).collect();
    assert_eq!(places, vec![2959, 48]);
    let idx = scan(&mut closed.as_bytes());
    let sigmas: Vec<_> = idx.overalls.iter().map(|m| m.visit).collect();
    assert_eq!(sigmas, vec![Some(0)], "the visit's Σ closed at the door");
    let lazy_sigma = lazy(closed.as_bytes(), &idx.overalls[0])
        .overall(0)
        .expect("the Σ's slice holds the visit");
    assert_eq!(amounts(&lazy_sigma, View::Damage), whole);
    agrees_everywhere(&closed, &meter);
}

/// The rejoin's edges, behind the same door. No pull rejoins the visit,
/// whose Σ keeps the three kills before the zone-out (600 + 30p each, with
/// the trash inside). A pull at another difficulty, or on another instance
/// Forever's table knows (Blackfathom Deeps, 48), has nothing to resume, so
/// it opens a visit of its own under the table's name, closing the
/// dungeon's, as the switch of a raid's difficulty does (R10, 2026-11-13).
/// A pull on a map the table does not know (2960) has no name to open under
/// and stays outside.
#[test]
fn a_pull_rejoins_only_its_own_instance_at_its_own_difficulty() {
    let text = rejoin();
    let before_the_zone_out = vec![
        ("Eryk-Realm".to_string(), 750),
        ("Dessa-Realm".to_string(), 720),
        ("Cael-Realm".to_string(), 690),
        ("Borin-Realm".to_string(), 669),
        ("Ana-Realm".to_string(), 637),
    ];
    // Each pull's START ends with its difficulty, size and instance.
    for (pull, (map, difficulty, name)) in [
        (",2,5,2959", (2959, 2, "City of Dalaran")),
        (",1,5,48", (48, 1, "Blackfathom Deeps")),
    ] {
        let edited = behind_the_door(&text, |t| t.replace(",1,5,2959", pull));
        assert_ne!(edited, text, "{pull}: the edit took");
        let meter = meter_from_lines(edited.lines());
        let places: Vec<_> = meter
            .visits()
            .iter()
            .map(|v| (v.map_id, v.difficulty, v.display_name()))
            .collect();
        assert_eq!(
            places,
            vec![
                (2959, 1, "Dalaran".to_string()),
                (map, difficulty, name.to_string()),
            ],
            "{pull}: the door's name, then the table's"
        );
        let shade = meter.segments()[7].start_ms;
        assert_eq!(
            meter.visits()[0].end_ms,
            Some(shade),
            "{pull}: closed by the pull"
        );
        assert_eq!(
            tags(&edited),
            rejoin_tags([Some(1), Some(1), Some(1), Some(1)]),
            "{pull}"
        );
        let idx = scan(&mut edited.as_bytes());
        let closed: Vec<_> = idx.overalls.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(closed, vec!["Dalaran"], "{pull}: the scanner's names");
        let open = idx.open_visit.as_ref().map(|m| m.name.as_str());
        assert_eq!(open, Some(name), "{pull}: the scanner's names");
        let sigma = meter.overall(0).expect("the dungeon's visit has members");
        assert_eq!(amounts(&sigma, View::Damage), before_the_zone_out, "{pull}");
        // The new visit's: the two pulls (100k + 10p), then Dessa's 11 and
        // Eryk's 13 inside it.
        let sigma = meter.overall(1).expect("the new visit has members");
        assert_eq!(
            amounts(&sigma, View::Damage),
            vec![
                ("Eryk-Realm".to_string(), 1013),
                ("Dessa-Realm".to_string(), 991),
                ("Cael-Realm".to_string(), 960),
                ("Borin-Realm".to_string(), 940),
                ("Ana-Realm".to_string(), 920),
            ],
            "{pull}"
        );
        agrees_everywhere(&edited, &meter);
    }

    let edited = behind_the_door(&text, |t| t.replace(",1,5,2959", ",1,5,2960"));
    assert_ne!(edited, text, "the edit took");
    let meter = meter_from_lines(edited.lines());
    assert_eq!(meter.visits().len(), 1, "no visit opened on 2960");
    assert_eq!(tags(&edited), rejoin_tags([None, None, None, None]));
    let sigma = meter.overall(0).expect("the visit has members");
    assert_eq!(amounts(&sigma, View::Damage), before_the_zone_out);
    agrees_everywhere(&edited, &meter);
}

const WALKIN_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/walkin.txt");

/// The fixture's walk-in door: a ZONE_CHANGE at difficulty 0 onto map 0,
/// whatever its name. No door in the log names map 2959.
const WALK_IN: &str = "ZONE_CHANGE,0,\"City of Dalaran\",0";

fn walkin() -> String {
    let text = std::fs::read_to_string(WALKIN_FIXTURE);
    assert!(text.is_ok(), "{WALKIN_FIXTURE}: unreadable fixture");
    text.unwrap_or_default()
}

/// R10 amendment (2026-11-13): a pull opens its visit behind a walk-in door
/// logged onto the open world. Forever can log the door into an instance
/// as a ZONE_CHANGE at difficulty 0 onto another map id, whatever name it
/// carries: on a real night a whole run of map 2959, eight kills, followed
/// `ZONE_CHANGE,0,"City of Dalaran",0` with no door onto 2959 anywhere, and
/// joined no visit at all, so it had no Σ. `walkin.txt` is that run's
/// shape: five players, two kills on 2959. The first pull has nothing to
/// resume and no door named its map, so it opens the visit under the name
/// Forever's Map.db2 table gives 2959; the trash before it stays outside,
/// the trash after it is inside until the door out.
#[test]
fn a_pull_opens_its_visit_behind_a_walk_in_door_onto_the_open_world() {
    let text = walkin();
    let meter = meter_from_lines(text.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 1, "one visit: {visits:?}");
    let visit = &visits[0];
    assert_eq!((visit.map_id, visit.difficulty), (2959, 1));
    assert_eq!(visit.display_name(), "City of Dalaran", "the table's name");
    let first_pull = meter.segments()[2].start_ms;
    assert_eq!(visit.start_ms, first_pull, "opened by the first pull");
    assert_eq!(visit.end_ms, None, "suspended by the door out");
    assert_eq!(
        tags(&text),
        vec![
            (SegmentKind::Trash, None),        // the Worg, outdoors
            (SegmentKind::Trash, None),        // behind the walk-in door, before a pull
            (SegmentKind::Encounter, Some(0)), // Atrexis opens the visit
            (SegmentKind::Trash, Some(0)),     // inside now
            (SegmentKind::Encounter, Some(0)), // Lyn the Ignored
            (SegmentKind::Trash, Some(0)),     // inside, before the door out
            (SegmentKind::Trash, None),        // Hillsbrad Foothills: zoned out
        ]
    );

    // Pull k's hit for the p-th player is 100k + 10p; with the trash
    // inside (Borin 9, Dessa 11). Never Ana's 1000 outdoors, Cael's 500
    // before the first pull, nor Eryk's 13 past the door out.
    let whole = vec![
        ("Eryk-Realm".to_string(), 400),
        ("Dessa-Realm".to_string(), 391),
        ("Cael-Realm".to_string(), 360),
        ("Borin-Realm".to_string(), 349),
        ("Ana-Realm".to_string(), 320),
    ];
    let sigma = meter.overall(0).expect("the visit has members");
    assert_eq!(amounts(&sigma, View::Damage), whole);

    // The opening START is a seed for every later slice.
    let bytes = text.as_bytes();
    let idx = scan(&mut &bytes[..]);
    let lyn = idx
        .segments
        .iter()
        .find(|m| m.name == "Lyn the Ignored")
        .expect("the second pull is closed");
    let seeded: Vec<_> = range_lines(bytes, &lyn.seeds)
        .into_iter()
        .filter(|l| l.contains("ENCOUNTER_START"))
        .collect();
    assert_eq!(seeded.len(), 1, "one START moved the visit: {seeded:?}");
    assert!(seeded[0].contains("\"Atrexis the Grave Knight\""));

    // A checkpoint between the walk-in door and the first pull holds no
    // visit and the door's map, and resumes to the full scan.
    let at = text.find(WALK_IN).expect("the walk-in door");
    let (_, from_door) = text.split_at(at);
    let door_end = at + from_door.find('\n').expect("the door's line ends") + 1;
    let state = scan(&mut &bytes[..door_end]).checkpoint;
    assert_eq!(state.offset as usize, door_end, "a clean boundary");
    assert!(state.visit.is_none(), "no visit before the pull");
    assert_eq!(state.last_zone_map, Some(0));
    let resumed = scan_from(&mut &bytes[door_end..], state);
    assert_eq!(resumed.segments, idx.segments);
    assert_eq!(resumed.open_visit, idx.open_visit);

    let live = agrees_everywhere(&text, &meter);
    assert_eq!(live.visits().len(), 1, "the seeds rebuilt the visit");

    // Never the outdoor door's name: behind a walk-in door named for the
    // hills, the visit still wears the table's.
    let hills = text.replace(WALK_IN, "ZONE_CHANGE,0,\"Alterac Mountains\",0");
    let named: Vec<_> = meter_from_lines(hills.lines())
        .visits()
        .iter()
        .map(|v| v.display_name())
        .collect();
    assert_eq!(named, vec!["City of Dalaran".to_string()]);
    let open = idx.open_visit.as_ref().expect("the run is in progress");
    assert_eq!(open.name, "City of Dalaran", "the scanner's name too");

    // A door that named the map gives the visit its own name, not the
    // table's, though it logged difficulty 0: the door's "Dalaran", where
    // Map.db2 says "City of Dalaran".
    let door = text.replace(WALK_IN, "ZONE_CHANGE,2959,\"Dalaran\",0");
    let named: Vec<_> = meter_from_lines(door.lines())
        .visits()
        .iter()
        .map(|v| (v.map_id, v.display_name()))
        .collect();
    assert_eq!(named, vec![(2959, "Dalaran".to_string())]);
    let idx = scan(&mut door.as_bytes());
    let open = idx.open_visit.as_ref().expect("the run is in progress");
    assert_eq!(open.name, "Dalaran", "the scanner's name too");

    // An edition with no table of party and raid maps (a Classic client's) names
    // no map, so the pull opens nothing, as before.
    let classic = text.replace("PROJECT_ID,18", "PROJECT_ID,2");
    assert_ne!(classic, text, "the edit took");
    assert!(meter_from_lines(classic.lines()).visits().is_empty());
    assert!(tags(&classic).iter().all(|(_, visit)| visit.is_none()));

    // A door into another instance closes the visit: its Σ is emitted as an
    // Overall meta and loads lazily as the replay holds it.
    let closed =
        format!("{text}11/4/2026 19:20:00.000-8  ZONE_CHANGE,189,\"Scarlet Monastery\",1\n");
    let meter = meter_from_lines(closed.lines());
    let places: Vec<_> = meter.visits().iter().map(|v| v.map_id).collect();
    assert_eq!(places, vec![2959, 189]);
    let idx = scan(&mut closed.as_bytes());
    let sigmas: Vec<_> = idx
        .overalls
        .iter()
        .map(|m| (m.visit, m.name.as_str()))
        .collect();
    assert_eq!(
        sigmas,
        vec![(Some(0), "City of Dalaran")],
        "the run's Σ closed at the door, under the table's name"
    );
    let lazy_sigma = lazy(closed.as_bytes(), &idx.overalls[0])
        .overall(0)
        .expect("the Σ's slice holds the visit");
    assert_eq!(amounts(&lazy_sigma, View::Damage), whole);
    assert_eq!(lazy_sigma.name, "City of Dalaran");
    agrees_everywhere(&closed, &meter);
}

/// A key's pull behind a door onto another map. A FINISHED key is never
/// rejoined, even at the key's own difficulty (8): its END was terminal,
/// so the pull has nothing to resume and opens a NEW unkeyed visit under
/// the table's name, as a door back onto a finished key's map does. A key
/// abandoned before its END is rejoined on its map alone, as any door back
/// onto that map would resume it: here by a pull at 23, which is not the
/// keyed visit's difficulty.
#[test]
fn a_finished_key_is_never_rejoined_behind_another_door() {
    let pull = |difficulty: u32| {
        format!(
            "\
8/1/2026 12:02:00.000-7  ZONE_CHANGE,0,\"Silvermoon City\",0
8/1/2026 12:05:00.000-7  ENCOUNTER_START,2563,\"Overgrown Ancient\",{difficulty},5,2526
8/1/2026 12:05:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-6,\"Overgrown Ancient\",0xa48,0x0,116,\"Frostbolt\",16,200,200,0,0,0,0,0,nil,nil
8/1/2026 12:06:00.000-7  ENCOUNTER_END,2563,\"Overgrown Ancient\",{difficulty},5,1,60000
"
        )
    };
    let finished = format!("{KEY_HEAD}{KEY_END}{}", pull(8));
    let meter = meter_from_lines(finished.lines());
    let visits = meter.visits();
    assert_eq!(visits.len(), 3, "the door's visit, the key, a new one");
    assert!(visits[1].end_ms.is_some(), "the END closed the key");
    let fresh = &visits[2];
    assert!(!fresh.keyed, "a new, unkeyed visit");
    assert_eq!(
        (fresh.map_id, fresh.difficulty, fresh.display_name()),
        (2526, 8, "Algeth'ar Academy".to_string())
    );
    assert_eq!(
        tags(&finished),
        vec![
            (SegmentKind::Trash, Some(1)),     // inside the key
            (SegmentKind::Encounter, Some(2)), // never the finished key
        ]
    );
    agrees_everywhere(&finished, &meter);

    let abandoned = format!("{KEY_HEAD}{}", pull(23));
    let meter = meter_from_lines(abandoned.lines());
    assert_eq!(meter.visits().len(), 2, "no visit opened");
    assert_eq!(meter.visits()[1].difficulty, 8, "the key's, not the pull's");
    assert_eq!(
        tags(&abandoned),
        vec![
            (SegmentKind::Trash, Some(1)),
            (SegmentKind::Encounter, Some(1)), // the key, rejoined
        ]
    );
    agrees_everywhere(&abandoned, &meter);
}

/// R10 amendment: an open-world door is zoned out. The game stamps a door
/// OUT of an instance with the difficulty it just left — a hearth out of a
/// Heroic raid logs `"Silvermoon City",15`, the end of a key
/// `"The Waking Shores",8` — and a nonzero difficulty read as zoned in, so
/// the hearth opened a "Silvermoon City" visit (the history store held five
/// such Σ cards) and closed the raid's, and the next pull behind a 0-logged
/// door opened a second raid visit instead of resuming the first. Map.db2
/// calls those maps open world (InstanceType 0), so the doors read as 0.
#[test]
fn an_open_world_door_is_zoned_out_whatever_its_difficulty() {
    let text = "\
10/4/2026 19:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1
10/4/2026 19:00:05.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",15
10/4/2026 19:01:00.000-7  ENCOUNTER_START,3429,\"The Coiled Altar\",15,25,3004
10/4/2026 19:01:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"The Coiled Altar\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
10/4/2026 19:02:00.000-7  ENCOUNTER_END,3429,\"The Coiled Altar\",15,25,1,60000
10/4/2026 19:03:00.000-7  ZONE_CHANGE,0,\"Silvermoon City\",15
10/4/2026 19:03:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-2,\"Training Dummy\",0xa48,0x0,116,\"Frostbolt\",16,5,5,0,0,0,0,0,nil,nil
10/4/2026 19:05:00.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",0
10/4/2026 19:06:00.000-7  ENCOUNTER_START,3492,\"Ula'tek\",15,26,3004
10/4/2026 19:06:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-3,\"Ula'tek\",0xa48,0x0,116,\"Frostbolt\",16,200,200,0,0,0,0,0,nil,nil
10/4/2026 19:07:00.000-7  ENCOUNTER_END,3492,\"Ula'tek\",15,26,1,60000
10/4/2026 19:08:00.000-7  ZONE_CHANGE,2444,\"The Waking Shores\",8
10/4/2026 19:08:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-4,\"Primal Tarasek\",0xa48,0x0,116,\"Frostbolt\",16,7,7,0,0,0,0,0,nil,nil
";
    let meter = meter_from_lines(text.lines());
    let places: Vec<_> = meter
        .visits()
        .iter()
        .map(|v| (v.display_name(), v.difficulty))
        .collect();
    assert_eq!(
        places,
        vec![("The Venomous Abyss".to_string(), 15)],
        "one raid visit, no city visit"
    );
    assert_eq!(meter.visits()[0].end_ms, None, "suspended, never closed");
    assert_eq!(
        tags(text),
        vec![
            (SegmentKind::Encounter, Some(0)), // The Coiled Altar
            (SegmentKind::Trash, None),        // the town dummy: zoned out
            (SegmentKind::Encounter, Some(0)), // Ula'tek resumes the raid
            (SegmentKind::Trash, None),        // the Waking Shores: zoned out
        ]
    );
    let idx = scan(&mut text.as_bytes());
    assert!(
        idx.overalls.is_empty(),
        "no visit closed: {:?}",
        idx.overalls
    );
    assert_eq!(idx.open_visit.as_ref().and_then(|m| m.visit), Some(0));
}

/// R10: a Σ knows what its members knew, nothing more. The Overall used to
/// seed its identity maps (owners, names, classes …) from the meter as it
/// stood when asked: the end of the file in a full replay, the end of the
/// visit in a lazy load. A guardian whose owner is named only AFTER the
/// key's END (a Lightspawn Lasher's "Sappy Demise" cast, 28 lines after a
/// real +13's END) folded into the full replay's Σ alone — 65,856 damage on
/// Den of Nalorakk +13, 65,053 on The Blinding Vale +11 — while every
/// member, lazy or full, left it out. And a statement after the END landed
/// in the member the END had closed. Both variants: the owner named after
/// post-key combat (the Σ's seeding), and before any (the closed member).
#[test]
fn a_sigma_knows_only_what_its_members_knew() {
    let head = "\
8/1/2026 12:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1
8/1/2026 12:00:05.000-7  ZONE_CHANGE,2526,\"Algeth'ar Academy\",23
8/1/2026 12:00:10.000-7  CHALLENGE_MODE_START,\"Algeth'ar Academy\",2526,558,12,[9,10]
8/1/2026 12:01:00.000-7  ENCOUNTER_START,2562,\"Vexamus\",8,5,2526
8/1/2026 12:01:10.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1-2526-1-194181-0000000001,\"Vexamus\",0xa48,0x0,116,\"Frostbolt\",16,100,100,0,0,0,0,0,nil,nil
8/1/2026 12:01:20.000-7  SPELL_DAMAGE,Creature-0-1-2526-1-254697-00001771EB,\"Lightspawn Lasher\",0x2111,0x0,Creature-0-1-2526-1-194181-0000000001,\"Vexamus\",0xa48,0x0,1253,\"Lightbloom Lashing\",8,50,50,0,0,0,0,0,nil,nil
8/1/2026 12:02:00.000-7  ENCOUNTER_END,2562,\"Vexamus\",8,5,1,60000
8/1/2026 12:02:01.000-7  CHALLENGE_MODE_END,2526,1,12,111000,300.000000,3029
";
    let summon = "8/1/2026 12:02:02.000-7  SPELL_SUMMON,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1-2526-1-254697-00001771EB,\"Lightspawn Lasher\",0x2111,0x0,1252,\"Lightspawn\",0x8\n";
    let after = "8/1/2026 12:02:30.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Training Dummy\",0xa48,0x0,116,\"Frostbolt\",16,5,5,0,0,0,0,0,nil,nil\n";
    for (variant, text) in [
        (
            "owner named after post-key combat",
            format!("{head}{after}{summon}"),
        ),
        (
            "owner named before any post-key combat",
            format!("{head}{summon}{after}"),
        ),
    ] {
        let bytes = text.as_bytes();
        let full = meter_from_lines(text.lines());
        let idx = scan(&mut &bytes[..]);
        let key = idx
            .overalls
            .iter()
            .find(|m| m.name.contains('+'))
            .expect("the key's Σ");
        let ord = key.visit.expect("an Overall names its visit");
        let want = full.overall(ord).expect("the full replay's Σ");
        let got = lazy(bytes, key).overall(ord).expect("the lazy Σ");
        assert_eq!(
            amounts(&got, View::Damage),
            amounts(&want, View::Damage),
            "{variant}: lazy Σ = full Σ"
        );
        // A full replay's Σ is its members merged, nothing more.
        let members: u64 = full
            .segments()
            .iter()
            .filter(|s| s.visit == Some(ord))
            .flat_map(|s| s.rows(View::Damage))
            .map(|r| r.amount)
            .sum();
        let sigma: u64 = want.rows(View::Damage).iter().map(|r| r.amount).sum();
        assert_eq!(sigma, members, "{variant}: Σ = Σ members");
        // Every member, lazily loaded, is the full replay's member.
        for meta in idx.segments.iter().filter(|m| m.visit == Some(ord)) {
            let seg = full
                .segments()
                .iter()
                .find(|s| s.start_ms == meta.start_ms)
                .expect("the member in the full replay");
            let lazy = lazy(bytes, meta);
            assert_eq!(
                amounts(&lazy.segments()[0], View::Damage),
                amounts(seg, View::Damage),
                "{variant}: member {}",
                meta.name
            );
        }
    }
}
