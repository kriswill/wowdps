//! `proto::history` — the history store's record codec. Golden one-line
//! JSON per document type (the files' bytes are a contract with every
//! reader, DuckDB included), round trips, missing-field tolerance, forward
//! compatibility (a document with fields this build doesn't know), and the
//! never-panics fuzz over truncated documents.

use wowdps_model::{
    Class, Encounter, GearItem, Loadout, Mark, MarkKind, Row, Spec, TalentPick, Timeline, View,
};
use wowdps_proto::history::{
    Annotation, CardPlayer, FightCard, FightDetails, FightKind, FightRows, HISTORY_SCHEMA, KeyInfo,
    PlayerDetail, Recap, StoredLoadout, content_id, fight_id, fnv64, loadout_hash, log_id,
    sigma_id,
};
use wowdps_proto::json::{self, Json};

fn row(key: &str, amount: u64) -> Row {
    Row {
        key: key.to_string(),
        label: format!("{key}-label"),
        amount,
        extra: 7,
        count: 3,
        crits: 1,
        per_sec: 12.5,
        pct: 33.25,
        class: Some(Class::Mage),
        spec: Some(Spec::FrostMage),
        hp: Some((5, 6)),
        gain: true,
        spell_id: 30451,
        enemy: false,
        school: 32,
    }
}

fn timeline() -> Timeline {
    Timeline {
        bucket_ms: 1000,
        buckets: vec![0, 5, 10],
        marks: vec![Mark {
            at_ms: 250,
            kind: MarkKind::TrinketUse,
            label: "T".to_string(),
            spell_id: 7,
            dur_ms: 9,
        }],
    }
}

fn loadout() -> Loadout {
    Loadout {
        spec_id: Some(64),
        talents: vec![TalentPick {
            node_id: 1,
            entry_id: 2,
            rank: 3,
        }],
        gear: vec![GearItem {
            item_id: 9,
            ilvl: 10,
            enchants: vec![],
            bonus_ids: vec![11, 12],
            gems: vec![13],
        }],
    }
}

fn card() -> FightCard {
    FightCard {
        schema: HISTORY_SCHEMA,
        id: fight_id(0x0123_4567_89ab_cdef, 1_722_000_000_123, false),
        log: 0x0123_4567_89ab_cdef,
        content: 0xfedc_ba98_7654_3210,
        kind: FightKind::Key,
        name: "Skyreach +10".to_string(),
        encounter: Some(Encounter {
            id: 3130,
            difficulty: 15,
            group_size: 20,
        }),
        key: Some(KeyInfo {
            map_id: 1209,
            difficulty: 23,
            level: Some(10),
            completed: Some(true),
        }),
        start_local_ms: 1_722_000_000_123,
        tz_min: Some(-240),
        start_utc_ms: 1_722_000_000_123 + 240 * 60_000,
        duration_ms: 61_500,
        official_ms: Some(61_400),
        pars_ms: Some((2_040_000, 1_632_000, 1_224_000)),
        success: Some(true),
        aborted: false,
        build: (12, 0, 2),
        project_id: 1,
        log_version: 22,
        owner: Some("Player-1-A".to_string()),
        byte_range: Some((10, 20)),
        pinned: true,
        best_pct: None,
        players: vec![
            CardPlayer {
                guid: "Player-1-A".to_string(),
                name: "Ana-Realm".to_string(),
                class: Some(Class::Mage),
                spec: Some(Spec::FrostMage),
                loadout: Some(0x00ff_00ff_00ff_00ff),
                logged: true,
                enemy: false,
                damage: 123_456,
                dps: 2007.4,
                healing: 0,
                hps: 0.0,
                deaths: 1,
            },
            CardPlayer {
                guid: "Player-1-B".to_string(),
                name: "Bo".to_string(),
                class: None,
                spec: None,
                loadout: None,
                logged: false,
                enemy: true,
                damage: 0,
                dps: 0.0,
                healing: 99,
                hps: 1.6,
                deaths: 0,
            },
        ],
        bosses: Vec::new(),
    }
}

fn rows() -> FightRows {
    let mut r = FightRows {
        id: "x-1".to_string(),
        ..Default::default()
    };
    if let Some(v) = r.views.get_mut(View::Damage.index()) {
        *v = vec![row("Player-1-A", 100)];
    }
    if let Some(v) = r.views.get_mut(View::Deaths.index()) {
        *v = vec![row("Player-1-A", 1)];
    }
    r.recaps = vec![Recap {
        guid: "Player-1-A".to_string(),
        events: vec![row("Smash", 50)],
        attackers: vec![row("Boss", 50)],
    }];
    r
}

fn details() -> FightDetails {
    FightDetails {
        id: "x-1".to_string(),
        players: vec![PlayerDetail {
            guid: "Player-1-A".to_string(),
            damage_spells: vec![row("Frostbolt", 60)],
            damage_targets: vec![row("Boss", 60)],
            heal_spells: vec![],
            heal_targets: vec![],
            damage_timeline: timeline(),
            heal_timeline: Timeline::default(),
        }],
        ..Default::default()
    }
}

fn annotation() -> Annotation {
    Annotation {
        ts_utc_ms: 1_722_000_000_000,
        kind: "note".to_string(),
        author: "coach".to_string(),
        rubric: None,
        body: "late \"pot\"\nline two".to_string(),
        tags: vec!["dps".to_string()],
    }
}

// ---- goldens --------------------------------------------------------------------

const CARD_GOLDEN: &str = r#"{"schema":1,"id":"0123456789abcdef-1722000000123","log":"0123456789abcdef","content":"fedcba9876543210","kind":"key","name":"Skyreach +10","encounter":{"id":3130,"difficulty":15,"group_size":20},"key":{"map_id":1209,"difficulty":23,"level":10,"completed":true},"start_local_ms":1722000000123,"tz_min":-240,"start_utc_ms":1722014400123,"duration_ms":61500,"official_ms":61400,"pars_ms":[2040000,1632000,1224000],"success":true,"aborted":false,"build":"12.0.2","project_id":1,"log_version":22,"owner":"Player-1-A","byte_range":[10,20],"pinned":true,"best_pct":null,"players":[{"guid":"Player-1-A","name":"Ana-Realm","class":"Mage","spec":64,"spec_name":"Frost","loadout":"00ff00ff00ff00ff","logged":true,"enemy":false,"damage":123456,"dps":2007.4,"healing":0,"hps":0,"deaths":1},{"guid":"Player-1-B","name":"Bo","class":null,"spec":null,"spec_name":null,"loadout":null,"logged":false,"enemy":true,"damage":0,"dps":0,"healing":99,"hps":1.6,"deaths":0}],"bosses":[]}"#;

const ROW_GOLDEN: &str = r#"{"key":"Player-1-A","label":"Player-1-A-label","amount":100,"extra":7,"count":3,"crits":1,"per_sec":12.5,"pct":33.25,"class":"Mage","spec":64,"hp":[5,6],"gain":true,"spell_id":30451,"enemy":false,"school":32}"#;

const LOADOUT_GOLDEN: &str = r#"{"schema":1,"hash":"HASH","spec_id":64,"talents":[{"node":1,"entry":2,"rank":3}],"gear":[{"item":9,"ilvl":10,"enchants":[],"bonus_ids":[11,12],"gems":[13]}]}"#;

const ANNOTATION_GOLDEN: &str = r#"{"schema":1,"ts_utc_ms":1722000000000,"kind":"note","author":"coach","rubric":null,"body":"late \"pot\"\nline two","tags":["dps"]}"#;

const TIMELINE_GOLDEN: &str = r#"{"bucket_ms":1000,"buckets":[0,5,10],"marks":[{"at_ms":250,"kind":0,"label":"T","spell_id":7,"dur_ms":9}]}"#;

#[test]
fn golden_documents_pin_the_file_format() {
    assert_eq!(HISTORY_SCHEMA, 1, "bumped? re-bless the goldens");
    assert_eq!(card().to_json().to_line(), CARD_GOLDEN);
    assert_eq!(
        wowdps_proto::history::row_json(&row("Player-1-A", 100)).to_line(),
        ROW_GOLDEN
    );
    assert_eq!(
        wowdps_proto::history::timeline_json(&timeline()).to_line(),
        TIMELINE_GOLDEN
    );
    let stored = StoredLoadout::new(loadout());
    assert_eq!(
        stored.to_json().to_line(),
        LOADOUT_GOLDEN.replace("HASH", &format!("{:016x}", stored.hash))
    );
    assert_eq!(annotation().to_json().to_line(), ANNOTATION_GOLDEN);

    // Rows and details are compositions of the pieces above; pin their
    // skeletons rather than another wall of row bytes.
    let r = rows().to_json().to_line();
    assert!(r.starts_with(r#"{"schema":1,"id":"x-1","views":{"damage":[{"key":"Player-1-A""#));
    assert!(r.contains(r#""healing":[],"interrupts":[],"cc":[],"dispels":[],"deaths":[{"key""#));
    assert!(r.contains(r#""recaps":[{"guid":"Player-1-A","events":[{"key":"Smash""#));
    let d = details().to_json().to_line();
    assert!(d.starts_with(r#"{"schema":1,"id":"x-1","players":[{"guid":"Player-1-A","damage_spells":[{"key":"Frostbolt""#));
    assert!(
        d.contains(r#""heal_spells":[],"heal_targets":[],"damage_timeline":{"bucket_ms":1000"#)
    );
    assert!(d.ends_with(r#""heal_timeline":{"bucket_ms":0,"buckets":[],"marks":[]}}]}"#));
}

// ---- round trips ------------------------------------------------------------------

fn reparse(v: Json) -> Json {
    json::parse(&v.to_line()).unwrap_or(Json::Null)
}

#[test]
fn every_document_round_trips() {
    assert_eq!(
        FightCard::from_json(&reparse(card().to_json())),
        Some(card())
    );
    assert_eq!(
        FightRows::from_json(&reparse(rows().to_json())),
        Some(rows())
    );
    assert_eq!(
        FightDetails::from_json(&reparse(details().to_json())),
        Some(details())
    );
    let stored = StoredLoadout::new(loadout());
    assert_eq!(
        StoredLoadout::from_json(&reparse(stored.to_json())),
        Some(stored)
    );
    assert_eq!(
        Annotation::from_json(&reparse(annotation().to_json())),
        Some(annotation())
    );
}

#[test]
fn an_aborted_legacy_card_round_trips_its_nulls() {
    let c = FightCard {
        id: "abc-1".to_string(),
        kind: FightKind::Trash,
        tz_min: None,
        success: None,
        aborted: true,
        owner: None,
        byte_range: None,
        ..Default::default()
    };
    let back = FightCard::from_json(&reparse(c.to_json())).unwrap();
    assert_eq!(back, c);
    assert_eq!(back.kind, FightKind::Trash);
}

// ---- tolerance --------------------------------------------------------------------

#[test]
fn missing_fields_take_defaults_and_only_identity_is_required() {
    let v = json::parse(r#"{"schema":1,"id":"x-9"}"#).unwrap();
    let c = FightCard::from_json(&v).expect("identity suffices");
    assert_eq!(c.id, "x-9");
    assert_eq!(c.kind, FightKind::Encounter);
    assert!(c.players.is_empty());
    assert_eq!(c.build, (0, 0, 0));
    assert_eq!(FightRows::from_json(&v).unwrap().rows(View::Damage), &[]);
    assert!(FightDetails::from_json(&v).unwrap().players.is_empty());

    for bad in [
        r#"{"id":"x"}"#,
        r#"{"schema":1}"#,
        r#"{"schema":1,"id":""}"#,
        r#"{"schema":"1","id":"x"}"#,
        r#"[]"#,
        r#"7"#,
    ] {
        let v = json::parse(bad).unwrap();
        assert!(FightCard::from_json(&v).is_none(), "{bad}");
        assert!(FightRows::from_json(&v).is_none(), "{bad}");
        assert!(FightDetails::from_json(&v).is_none(), "{bad}");
    }
    // A loadout file needs its hash (its name), an annotation its kind.
    let v = json::parse(r#"{"schema":1,"spec_id":64}"#).unwrap();
    assert!(StoredLoadout::from_json(&v).is_none());
    let v = json::parse(r#"{"schema":1,"body":"b"}"#).unwrap();
    assert!(Annotation::from_json(&v).is_none());
}

#[test]
fn a_document_from_the_future_still_reads() {
    // Fields this build has never heard of — an older daemon reading a
    // newer lake — are ignored, and the known ones still land.
    let mut line = CARD_GOLDEN.to_string();
    line.insert_str(line.len() - 1, r#","affixes":[9,10],"players_v2":{"x":1}"#);
    let v = json::parse(&line).unwrap();
    assert_eq!(FightCard::from_json(&v), Some(card()));
}

#[test]
fn malformed_fields_degrade_to_defaults_not_errors() {
    let v = json::parse(
        r#"{"schema":1,"id":"x","log":"zz","kind":"raid","class":3,"pars_ms":[1],"byte_range":"no","players":[{"name":"no guid"},{"guid":"g","spec":"Frost","deaths":-1}],"tz_min":99999}"#,
    )
    .unwrap();
    let c = FightCard::from_json(&v).unwrap();
    assert_eq!(c.log, 0);
    assert_eq!(c.kind, FightKind::Encounter);
    assert_eq!(c.pars_ms, None);
    assert_eq!(c.byte_range, None);
    assert_eq!(c.tz_min, None, "out of i16 range");
    assert_eq!(c.players.len(), 1, "a player without a guid is dropped");
    assert_eq!(c.players[0].spec, None);
    assert_eq!(c.players[0].deaths, 0);
}

// ---- never panics -----------------------------------------------------------------

#[test]
fn every_truncation_of_every_golden_is_survivable() {
    let stored = StoredLoadout::new(loadout());
    let docs = [
        CARD_GOLDEN.to_string(),
        rows().to_json().to_line(),
        details().to_json().to_line(),
        stored.to_json().to_line(),
        ANNOTATION_GOLDEN.to_string(),
    ];
    for doc in &docs {
        for cut in 0..doc.len() {
            let Some(prefix) = doc.get(..cut) else {
                continue; // inside a multi-byte char
            };
            if let Ok(v) = json::parse(prefix) {
                let _ = FightCard::from_json(&v);
                let _ = FightRows::from_json(&v);
                let _ = FightDetails::from_json(&v);
                let _ = StoredLoadout::from_json(&v);
                let _ = Annotation::from_json(&v);
            }
        }
    }
}

// ---- identity ---------------------------------------------------------------------

#[test]
fn fight_ids_come_from_the_header_line_and_survive_a_crlf_copy() {
    let header = "7/27/2026 20:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1";
    let a = log_id(Some(header), "WoWCombatLog-072726_200000.txt");
    let b = log_id(Some(&format!("{header}\r\n")), "copy.txt");
    let c = log_id(Some(&format!("{header}\n")), "copy.txt");
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(a, fnv64(header.as_bytes()));
    // No usable first line: the file name identifies the log.
    assert_eq!(
        log_id(None, "x.txt"),
        fnv64(b"x.txt"),
        "a log begun mid-session"
    );
    assert_eq!(log_id(Some("  \n"), "x.txt"), fnv64(b"x.txt"));
    assert_eq!(fight_id(0xab, -5, false), "00000000000000ab--5");
    assert_eq!(fight_id(0xab, -5, true), "00000000000000ab--5s");
    assert_eq!(sigma_id("00000000000000ab--5"), "00000000000000ab--5s");
    assert_eq!(sigma_id("00000000000000ab--5s"), "00000000000000ab--5s");
    assert_eq!(
        fight_id(a, 1_722_000_000_123, false),
        format!("{a:016x}-1722000000123")
    );
}

#[test]
fn content_ids_ignore_guid_order_and_sub_second_jitter() {
    let enc = Some(Encounter {
        id: 3130,
        difficulty: 15,
        group_size: 20,
    });
    let x = content_id(enc, 1_722_000_000_123, ["B", "A", "A"]);
    let y = content_id(enc, 1_722_000_000_900, ["A", "B"]);
    assert_eq!(x, y);
    assert_ne!(x, content_id(enc, 1_722_000_001_000, ["A", "B"]));
    assert_ne!(x, content_id(None, 1_722_000_000_123, ["A", "B"]));
    assert_ne!(x, content_id(enc, 1_722_000_000_123, ["A"]));
}

#[test]
fn loadout_hashes_are_the_wire_bytes_hashed() {
    let l = loadout();
    assert_eq!(
        loadout_hash(&l),
        fnv64(&wowdps_proto::msg::loadout_bytes(&l))
    );
    assert_eq!(StoredLoadout::new(l.clone()).hash, loadout_hash(&l));
    let mut other = l;
    other.gear[0].ilvl += 1;
    assert_ne!(loadout_hash(&other), loadout_hash(&loadout()));
}
