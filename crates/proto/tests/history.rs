//! `proto::history` — the history store's record codec. Golden one-line
//! JSON per document type (the files' bytes are a contract with every
//! reader, DuckDB included), round trips, missing-field tolerance, forward
//! compatibility (a document with fields this build doesn't know), and the
//! never-panics fuzz over truncated documents.

use wowdps_model::{
    Class, Encounter, GearItem, Loadout, Mark, MarkKind, MissKind, Mitigation, Role, Row, Spec,
    TalentPick, Timeline, View,
};
use wowdps_proto::history::{
    Annotation, CardPlayer, FightCard, FightDetails, FightKind, FightRows, HISTORY_SCHEMA, KeyInfo,
    PlayerDetail, PlayerMitigation, PlayerSupport, Recap, RoleCount, StoredLoadout,
    TAKEN_SPELLS_CAP, TakenOther, content_id, fight_id, fnv64, loadout_hash, log_id,
    mitigation_from, mitigation_json, sigma_id,
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
            src: String::new(),
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
                // Step 2b: 12 000 / (40 000 + 8 000) = exactly 25 %.
                taken: 40_000,
                mitigated: 12_000,
                prevented: 8_000,
                dtps: 650.4,
                // Step 3b: effective = 123 456 − 1 456 + 1 000 = 123 000,
                // over 61.5 s = exactly 2 000 effective dps.
                overheal: 5_000,
                absorbed: 3_000,
                support_given: 1_000,
                support_received: 1_456,
                healed_received: 7_000,
                self_healed: 1_500,
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
                taken: 0,
                mitigated: 0,
                prevented: 0,
                dtps: 0.0,
                overheal: 0,
                absorbed: 0,
                support_given: 0,
                support_received: 0,
                healed_received: 0,
                self_healed: 0,
            },
        ],
        bosses: Vec::new(),
    }
}

/// Step 2b: a mitigation record with every field non-zero and distinct,
/// the ten miss counts 0x11.. in `MissKind::ALL` order.
fn mitigation() -> Mitigation {
    let mut m = Mitigation {
        absorbed: 1,
        blocked: 2,
        absorbed_full: 3,
        blocked_full: 4,
        stagger: 5,
        stagger_ticked: 6,
        misses: [0; MissKind::COUNT],
    };
    for (i, kind) in MissKind::ALL.iter().enumerate() {
        if let Some(slot) = m.misses.get_mut(kind.index()) {
            *slot = 0x11 + i as u32;
        }
    }
    m
}

fn player_mitigation() -> PlayerMitigation {
    PlayerMitigation {
        guid: "Player-1-A".to_string(),
        record: mitigation(),
        taken_spells: vec![row("Smash", 900), row("Melee", 100)],
        other: TakenOther {
            amount: 55,
            extra: 5,
            count: 9,
            n: 3,
        },
        taken_sources: vec![row("Boss", 1000)],
        other_sources: TakenOther {
            amount: 55,
            extra: 5,
            count: 9,
            n: 2,
        },
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
    r.mitigation = vec![player_mitigation()];
    r.support = vec![player_support()];
    r
}

/// Step 3b: the card's Ana as an Augmentation-shaped supporter — the
/// same 1 000 given / 1 456 received damage shares the card carries, plus
/// the healing shares only this tier keeps.
fn player_support() -> PlayerSupport {
    PlayerSupport {
        guid: "Player-1-A".to_string(),
        given_damage: 1_000,
        given_healing: 20,
        received_damage: 1_456,
        received_healing: 3,
        targets: vec![row("Player-1-B", 1_000)],
    }
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

const CARD_GOLDEN: &str = r#"{"schema":1,"id":"0123456789abcdef-1722000000123","log":"0123456789abcdef","content":"fedcba9876543210","kind":"key","name":"Skyreach +10","encounter":{"id":3130,"difficulty":15,"group_size":20},"key":{"map_id":1209,"difficulty":23,"level":10,"completed":true},"start_local_ms":1722000000123,"tz_min":-240,"start_utc_ms":1722014400123,"duration_ms":61500,"official_ms":61400,"pars_ms":[2040000,1632000,1224000],"success":true,"aborted":false,"build":"12.0.2","project_id":1,"log_version":22,"owner":"Player-1-A","byte_range":[10,20],"pinned":true,"best_pct":null,"players":[{"guid":"Player-1-A","name":"Ana-Realm","class":"Mage","spec":64,"spec_name":"Frost","role":"dps","loadout":"00ff00ff00ff00ff","logged":true,"enemy":false,"damage":123456,"dps":2007.4,"healing":0,"hps":0,"deaths":1,"taken":40000,"mitigated":12000,"prevented":8000,"dtps":650.4,"mitigated_pct":25,"overheal":5000,"absorbed":3000,"support_given":1000,"support_received":1456,"healed_received":7000,"self_healed":1500,"effective_dps":2000},{"guid":"Player-1-B","name":"Bo","class":null,"spec":null,"spec_name":null,"role":null,"loadout":null,"logged":false,"enemy":true,"damage":0,"dps":0,"healing":99,"hps":1.6,"deaths":0,"taken":0,"mitigated":0,"prevented":0,"dtps":0,"mitigated_pct":0,"overheal":0,"absorbed":0,"support_given":0,"support_received":0,"healed_received":0,"self_healed":0,"effective_dps":0}],"bosses":[]}"#;

/// Step 3b: one supporter's block on the rows tier, every scalar distinct;
/// `targets` is one `Segment::support_targets` row.
const SUPPORT_GOLDEN: &str = r#"{"guid":"Player-1-A","given":{"damage":1000,"healing":20},"received":{"damage":1456,"healing":3},"targets":[ROW_B]}"#;

/// Step 2b: the rows tier's per-player mitigation entry, every field
/// non-zero and both lists visibly capped (`other.n` 3, `other_sources.n`
/// 2); the ten miss keys in
/// `MissKind::ALL` order.
const MITIGATION_GOLDEN: &str = r#"{"guid":"Player-1-A","record":{"absorbed":1,"blocked":2,"absorbed_full":3,"blocked_full":4,"stagger":5,"stagger_ticked":6,"misses":{"dodge":17,"parry":18,"block":19,"miss":20,"absorb":21,"immune":22,"deflect":23,"evade":24,"reflect":25,"resist":26}},"taken_spells":[ROW_SMASH,ROW_MELEE],"other":{"amount":55,"extra":5,"count":9,"n":3},"taken_sources":[ROW_BOSS],"other_sources":{"amount":55,"extra":5,"count":9,"n":2}}"#;

const ROW_GOLDEN: &str = r#"{"key":"Player-1-A","label":"Player-1-A-label","amount":100,"extra":7,"count":3,"crits":1,"per_sec":12.5,"pct":33.25,"class":"Mage","spec":64,"hp":[5,6],"gain":true,"spell_id":30451,"enemy":false,"school":32}"#;

const LOADOUT_GOLDEN: &str = r#"{"schema":1,"hash":"HASH","spec_id":64,"talents":[{"node":1,"entry":2,"rank":3}],"gear":[{"item":9,"ilvl":10,"enchants":[],"bonus_ids":[11,12],"gems":[13]}]}"#;

const ANNOTATION_GOLDEN: &str = r#"{"schema":1,"ts_utc_ms":1722000000000,"kind":"note","author":"coach","rubric":null,"body":"late \"pot\"\nline two","tags":["dps"]}"#;

// v24 (R18): every mark writes `src` — empty for item marks — so the SQL
// column keeps one shape.
const TIMELINE_GOLDEN: &str = r#"{"bucket_ms":1000,"buckets":[0,5,10],"marks":[{"at_ms":250,"kind":0,"label":"T","spell_id":7,"dur_ms":9,"src":""}]}"#;

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
    // Step 2b: the mitigation list follows the recaps and is pinned whole.
    let row_line =
        |key: &str, amount: u64| wowdps_proto::history::row_json(&row(key, amount)).to_line();
    let want = MITIGATION_GOLDEN
        .replace("ROW_SMASH", &row_line("Smash", 900))
        .replace("ROW_MELEE", &row_line("Melee", 100))
        .replace("ROW_BOSS", &row_line("Boss", 1000));
    assert_eq!(player_mitigation().to_json().to_line(), want);
    // Step 3b: the support list follows the mitigation list, pinned whole.
    let sup = SUPPORT_GOLDEN.replace("ROW_B", &row_line("Player-1-B", 1_000));
    assert_eq!(player_support().to_json().to_line(), sup);
    assert!(
        r.ends_with(&format!(r#","mitigation":[{want}],"support":[{sup}]}}"#)),
        "{r}"
    );
    assert_eq!(
        FightRows::default().to_json().to_line(),
        r#"{"schema":1,"id":"","views":{"damage":[],"healing":[],"interrupts":[],"cc":[],"dispels":[],"deaths":[],"taken":[]},"recaps":[],"mitigation":[],"support":[]}"#
    );
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

// ---- role (roadmap item 1a, step 1) -----------------------------------------------

/// `CARD_GOLDEN` as a PR #12 store wrote it: no `role` on any player.
fn golden_without_role() -> String {
    let stripped = CARD_GOLDEN
        .replace(r#""role":"dps","#, "")
        .replace(r#""role":null,"#, "");
    assert!(!stripped.contains("\"role\""), "{stripped}");
    assert_ne!(stripped, CARD_GOLDEN);
    stripped
}

#[test]
fn a_card_without_role_answers_it_from_the_spec_and_writes_it_back() {
    let v = json::parse(&golden_without_role()).unwrap();
    let c = FightCard::from_json(&v).expect("a pre-step-1 card still reads");
    assert_eq!(c, card(), "role is derived, never a struct field");
    assert_eq!(c.players[0].role(), Some(Role::Dps), "Frost mage");
    assert_eq!(c.players[1].role(), None, "no spec, no role");
    assert_eq!(
        c.to_json().to_line(),
        CARD_GOLDEN,
        "re-encoding stamps the field for readers that cannot call Spec::role"
    );
}

#[test]
fn a_stored_role_that_contradicts_the_spec_is_ignored() {
    let lying = CARD_GOLDEN
        .replace(r#""role":"dps""#, r#""role":"tank""#)
        .replace(r#""role":null"#, r#""role":"healer""#);
    assert_ne!(lying, CARD_GOLDEN);
    let v = json::parse(&lying).unwrap();
    let c = FightCard::from_json(&v).unwrap();
    assert_eq!(c, card());
    assert_eq!(c.players[0].role(), Some(Role::Dps), "the spec wins");
    assert_eq!(
        c.players[1].role(),
        None,
        "a role without a spec is nothing"
    );
    assert_eq!(
        c.to_json().to_line(),
        CARD_GOLDEN,
        "and the lie is not written back"
    );
}

#[test]
fn roles_counts_the_friendly_side_by_spec() {
    let player = |guid: &str, spec: Option<Spec>, enemy: bool| CardPlayer {
        guid: guid.to_string(),
        name: guid.to_string(),
        class: spec.map(Spec::class),
        spec,
        enemy,
        ..Default::default()
    };
    let c = FightCard {
        id: "x-1".to_string(),
        players: vec![
            player("t1", Some(Spec::ProtectionWarrior), false),
            player("t2", Some(Spec::Blood), false),
            player("h1", Some(Spec::Discipline), false),
            player("d1", Some(Spec::FrostMage), false),
            player("d2", Some(Spec::Arms), false),
            player("d3", Some(Spec::Marksmanship), false),
            player("enemy-healer", Some(Spec::HolyPaladin), true),
            player("unknown", None, false),
        ],
        ..Default::default()
    };
    assert_eq!(
        c.roles(),
        RoleCount {
            tanks: 2,
            healers: 1,
            dps: 3
        },
        "enemies and spec-less players count nowhere"
    );
    assert_eq!(FightCard::default().roles(), RoleCount::default());
    // The count survives the file: every player is stamped, and a reread
    // (with or without the stamp) counts the same.
    let line = c.to_json().to_line();
    assert_eq!(line.matches("\"role\":").count(), 8);
    let v = json::parse(&line).unwrap();
    assert_eq!(FightCard::from_json(&v).unwrap().roles(), c.roles());
}

// ---- tank measures (R17, step 2b) ----------------------------------------------

/// `CARD_GOLDEN` as a PR #16 store wrote it: no tank measures on any player.
fn golden_without_taken() -> String {
    let stripped = CARD_GOLDEN
        .replace(
            r#","taken":40000,"mitigated":12000,"prevented":8000,"dtps":650.4,"mitigated_pct":25"#,
            "",
        )
        .replace(
            r#","taken":0,"mitigated":0,"prevented":0,"dtps":0,"mitigated_pct":0"#,
            "",
        );
    for key in ["taken", "mitigated", "prevented", "dtps", "mitigated_pct"] {
        assert!(!stripped.contains(&format!("\"{key}\"")), "{stripped}");
    }
    stripped
}

#[test]
fn a_card_without_tank_measures_reads_zeros_and_derives_a_zero_pct() {
    let v = json::parse(&golden_without_taken()).unwrap();
    let c = FightCard::from_json(&v).expect("a pre-2b card still reads");
    for p in &c.players {
        assert_eq!((p.taken, p.mitigated, p.prevented), (0, 0, 0), "{}", p.guid);
        assert_eq!(p.dtps, 0.0);
        assert_eq!(p.mitigated_pct(), 0.0, "nothing swung, nothing mitigated");
    }
    // Everything else on the card is the golden's; only the measures are new.
    let mut want = card();
    for p in &mut want.players {
        p.taken = 0;
        p.mitigated = 0;
        p.prevented = 0;
        p.dtps = 0.0;
    }
    assert_eq!(c, want);
}

#[test]
fn mitigated_pct_is_derived_from_the_three_measures_not_stored() {
    let p = &card().players[0];
    assert_eq!(p.mitigated_pct(), 25.0, "12 000 of 48 000 swung");
    assert_eq!(
        p.mitigated_pct(),
        wowdps_model::mitigated_pct(12_000, 40_000, 8_000),
        "one helper for the card and the live record"
    );
    // The same numbers as a live record: partials 2 000 + 2 000, fulls
    // 6 000 + 2 000 — mitigated 12 000, prevented 8 000.
    let live = Mitigation {
        absorbed: 2_000,
        blocked: 2_000,
        absorbed_full: 6_000,
        blocked_full: 2_000,
        ..Mitigation::default()
    };
    assert_eq!(live.mitigated(), 12_000);
    assert_eq!(live.prevented(), 8_000);
    assert_eq!(live.mitigated_pct(40_000), p.mitigated_pct());
    // A stored pct that contradicts the measures is ignored on read and
    // the derived one written back.
    let lying = CARD_GOLDEN.replace(r#""mitigated_pct":25"#, r#""mitigated_pct":99"#);
    assert_ne!(lying, CARD_GOLDEN);
    let c = FightCard::from_json(&json::parse(&lying).unwrap()).unwrap();
    assert_eq!(c, card());
    assert_eq!(c.to_json().to_line(), CARD_GOLDEN);
}

#[test]
fn a_mitigation_record_round_trips_and_missing_miss_keys_are_zero() {
    let m = mitigation();
    let v = reparse(mitigation_json(&m));
    assert_eq!(mitigation_from(&v), Some(m));
    assert_eq!(
        mitigation_json(&Mitigation::default()).to_line(),
        r#"{"absorbed":0,"blocked":0,"absorbed_full":0,"blocked_full":0,"stagger":0,"stagger_ticked":0,"misses":{"dodge":0,"parry":0,"block":0,"miss":0,"absorb":0,"immune":0,"deflect":0,"evade":0,"reflect":0,"resist":0}}"#,
        "all ten kinds are written, zeros included, for a stable column shape"
    );
    // A record from a build with fewer miss kinds: the rest default.
    let v = json::parse(r#"{"absorbed":7,"misses":{"parry":2,"unknown":9}}"#).unwrap();
    let m = mitigation_from(&v).unwrap();
    assert_eq!(m.absorbed, 7);
    assert_eq!(m.misses_of(MissKind::Parry), 2);
    assert_eq!(m.misses(), 2);
    assert_eq!(mitigation_from(&Json::Null), None);
    assert_eq!(mitigation_from(&Json::Arr(vec![])), None);
}

#[test]
fn a_rows_document_without_mitigation_reads_empty_and_a_capped_list_says_so() {
    let mut line = rows().to_json().to_line();
    let cut = line.find(r#","mitigation":"#).expect("the key is written");
    line.truncate(cut);
    line.push('}');
    let v = json::parse(&line).expect("still a document: {line}");
    let r = FightRows::from_json(&v).unwrap();
    assert!(r.mitigation.is_empty(), "a PR #16 rows file");
    assert_eq!(r.recaps, rows().recaps, "and everything else is intact");

    // With it: the full struct round-trips through the file bytes.
    let back = FightRows::from_json(&reparse(rows().to_json())).unwrap();
    assert_eq!(back.mitigation, vec![player_mitigation()]);
    let pm = &back.mitigation[0];
    assert!(
        pm.other.n > 0 && pm.other_sources.n > 0,
        "the writer capped both lists"
    );
    assert!(pm.taken_spells.len() <= TAKEN_SPELLS_CAP);
    assert!(pm.taken_sources.len() <= TAKEN_SPELLS_CAP);
    assert_eq!(
        pm.taken_spells.iter().map(|r| r.amount).sum::<u64>() + pm.other.amount,
        pm.taken_sources.iter().map(|r| r.amount).sum::<u64>() + pm.other_sources.amount,
        "Σ spells + other = Σ sources + other_sources = the Taken row"
    );
    // A malformed entry (no guid) is dropped; a bare guid reads as zeros.
    let v = json::parse(
        r#"{"schema":1,"id":"x","mitigation":[{"record":{}},{"guid":"g","record":7,"other":"no"}]}"#,
    )
    .unwrap();
    let r = FightRows::from_json(&v).unwrap();
    assert_eq!(r.mitigation.len(), 1);
    assert_eq!(r.mitigation[0].record, Mitigation::default());
    assert_eq!(r.mitigation[0].other, TakenOther::default());
    assert_eq!(r.mitigation[0].other_sources, TakenOther::default());
}

// ---- step 3b: the healing split, support, effective dps -----------------------------

/// `CARD_GOLDEN` as a PR #19 store wrote it: no healing split, no support
/// scalars, no `effective_dps` on any player.
fn golden_without_support() -> String {
    let stripped = CARD_GOLDEN
        .replace(
            r#","overheal":5000,"absorbed":3000,"support_given":1000,"support_received":1456,"healed_received":7000,"self_healed":1500,"effective_dps":2000"#,
            "",
        )
        .replace(
            r#","overheal":0,"absorbed":0,"support_given":0,"support_received":0,"healed_received":0,"self_healed":0,"effective_dps":0"#,
            "",
        );
    for key in [
        "overheal",
        "absorbed",
        "support_given",
        "support_received",
        "healed_received",
        "self_healed",
        "effective_dps",
    ] {
        assert!(!stripped.contains(&format!("\"{key}\"")), "{stripped}");
    }
    assert_ne!(stripped, CARD_GOLDEN);
    stripped
}

#[test]
fn a_card_without_support_reads_zeros_and_effective_is_raw_damage() {
    let v = json::parse(&golden_without_support()).unwrap();
    let c = FightCard::from_json(&v).expect("a pre-3b card still reads");
    for p in &c.players {
        assert_eq!(
            (p.overheal, p.absorbed, p.support_given, p.support_received),
            (0, 0, 0, 0),
            "{}",
            p.guid
        );
        assert_eq!((p.healed_received, p.self_healed), (0, 0), "{}", p.guid);
        assert_eq!(p.effective(), p.damage, "no support: effective is damage");
    }
    // Ana's 123 456 over 61.5 s, the raw rate — what the SQL coalesce
    // reproduces on the same card.
    assert_eq!(c.players[0].effective_dps(c.duration_ms), 123_456.0 / 61.5);
    // Everything else on the card is the golden's; only the seven are new.
    let mut want = card();
    for p in &mut want.players {
        p.overheal = 0;
        p.absorbed = 0;
        p.support_given = 0;
        p.support_received = 0;
        p.healed_received = 0;
        p.self_healed = 0;
    }
    assert_eq!(c, want);
    // Written back, the seven keys appear with zeros and the raw rate.
    let line = c.to_json().to_line();
    assert!(line.contains(
        r#""overheal":0,"absorbed":0,"support_given":0,"support_received":0,"healed_received":0,"self_healed":0,"effective_dps":2007.4146341463415"#
    ), "{line}");
}

/// The exact `per_sec` arithmetic of `Meter::finish_rows` (core/meter.rs):
/// `amount as f64 / secs` with `secs = duration_ms as f64 / 1000.0`.
fn finish_rows_per_sec(amount: u64, duration_ms: i64) -> f64 {
    let secs = duration_ms as f64 / 1000.0;
    if secs > 0.0 {
        amount as f64 / secs
    } else {
        0.0
    }
}

#[test]
fn effective_dps_is_the_meters_dps_arithmetic_bit_for_bit() {
    // Awkward numbers so nothing is exact: without support the card's
    // `dps` (the meter's per_sec) and `effective_dps` are the same bits.
    for (damage, duration_ms) in [
        (123_456u64, 61_537i64),
        (1, 1),
        (987_654_321, 3_599_999),
        (7, 1_000_003),
        (u64::MAX / 3, 7_777_777),
    ] {
        let p = CardPlayer {
            damage,
            dps: finish_rows_per_sec(damage, duration_ms),
            ..CardPlayer::default()
        };
        assert_eq!(p.effective(), damage);
        assert_eq!(
            p.effective_dps(duration_ms).to_bits(),
            p.dps.to_bits(),
            "{damage} over {duration_ms} ms"
        );
    }
    // With support the numerator is the model's fold, over the same
    // denominator.
    let p = &card().players[0];
    assert_eq!(p.effective(), 123_000);
    assert_eq!(
        p.effective(),
        wowdps_model::effective(123_456, 1_456, 1_000),
        "one helper for the card and the live segment"
    );
    assert_eq!(p.effective_dps(61_500), 2000.0);
    assert_eq!(
        p.effective_dps(61_537).to_bits(),
        finish_rows_per_sec(123_000, 61_537).to_bits()
    );
    // A received share past the damage clamps at 0 (R19), never wraps.
    let clamped = CardPlayer {
        damage: 10,
        support_received: 11,
        ..CardPlayer::default()
    };
    assert_eq!(clamped.effective(), 0);
    assert_eq!(clamped.effective_dps(1_000), 0.0);
    // No duration, no rate — an aborted card's players read 0.0 like a
    // rate row over a zero-length segment would.
    assert_eq!(p.effective_dps(0), 0.0);
    assert_eq!(p.effective_dps(-5), 0.0);
}

#[test]
fn effective_dps_is_derived_from_the_scalars_not_stored() {
    // A stored value that contradicts the scalars is ignored on read and
    // the derived one written back.
    let lying = CARD_GOLDEN.replace(r#""effective_dps":2000"#, r#""effective_dps":99"#);
    assert_ne!(lying, CARD_GOLDEN);
    let c = FightCard::from_json(&json::parse(&lying).unwrap()).unwrap();
    assert_eq!(c, card());
    assert_eq!(c.to_json().to_line(), CARD_GOLDEN);
    // A player written without its card has no duration to derive from:
    // the key is there for the reader, null.
    let alone = card().players[0].to_json().to_line();
    assert!(
        alone.ends_with(r#","self_healed":1500,"effective_dps":null}"#),
        "{alone}"
    );
    assert_eq!(
        CardPlayer::from_json(&json::parse(&alone).unwrap()),
        Some(card().players[0].clone())
    );
    assert_eq!(
        card().players[0].to_json_in(Some(61_500)).to_line(),
        alone.replace(r#""effective_dps":null"#, r#""effective_dps":2000"#)
    );
}

#[test]
fn a_rows_document_without_support_reads_empty_and_a_block_round_trips() {
    let mut line = rows().to_json().to_line();
    let cut = line.find(r#","support":"#).expect("the key is written");
    line.truncate(cut);
    line.push('}');
    let v = json::parse(&line).expect("still a document: {line}");
    let r = FightRows::from_json(&v).unwrap();
    assert!(r.support.is_empty(), "a PR #19 rows file");
    assert_eq!(
        r.mitigation,
        rows().mitigation,
        "and everything else is intact"
    );
    assert_eq!(r.recaps, rows().recaps);

    // With it: the block round-trips through the file bytes, rows included.
    let back = FightRows::from_json(&reparse(rows().to_json())).unwrap();
    assert_eq!(back.support, vec![player_support()]);
    assert_eq!(
        PlayerSupport::from_json(&reparse(player_support().to_json())),
        Some(player_support())
    );
    // A malformed entry (no guid) is dropped; a bare guid reads as zeros
    // with no targets; a side that is not an object reads as zeros.
    let v = json::parse(
        r#"{"schema":1,"id":"x","support":[{"given":{"damage":1}},{"guid":"g"},{"guid":"h","given":7,"received":{"damage":"no","healing":2},"targets":"no"}]}"#,
    )
    .unwrap();
    let r = FightRows::from_json(&v).unwrap();
    assert_eq!(r.support.len(), 2);
    assert_eq!(
        r.support[0],
        PlayerSupport {
            guid: "g".to_string(),
            ..PlayerSupport::default()
        }
    );
    assert_eq!(
        r.support[1],
        PlayerSupport {
            guid: "h".to_string(),
            received_healing: 2,
            ..PlayerSupport::default()
        }
    );
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
                let _ = CardPlayer::from_json(&v);
                let _ = FightRows::from_json(&v);
                let _ = PlayerSupport::from_json(&v);
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

/// v24 (R18): a role-kind mark round-trips with its caster; a pre-v24
/// timeline (no `src` key) reads an empty caster; a mark whose kind code the
/// reader does not know is dropped — never re-kinded, never an error.
#[test]
fn timeline_marks_carry_their_caster_and_tolerate_older_and_newer_files() {
    use wowdps_proto::history::{timeline_from, timeline_json};

    let mut t = timeline();
    t.marks.push(Mark {
        at_ms: 3000,
        kind: MarkKind::SupportBuff,
        label: "Ebon Might".to_string(),
        spell_id: 395152,
        dur_ms: 10_000,
        src: "Player-1-0E".to_string(),
    });
    let line = timeline_json(&t).to_line();
    assert!(line.contains(
        r#""kind":6,"label":"Ebon Might","spell_id":395152,"dur_ms":10000,"src":"Player-1-0E"}"#
    ));
    assert_eq!(timeline_from(Some(&json::parse(&line).unwrap())), t);

    // A PR #12 file: the same document without `src`.
    let old = json::parse(TIMELINE_GOLDEN.replace(r#","src":"""#, "").as_str()).unwrap();
    assert!(!old.to_line().contains("src"));
    assert_eq!(
        timeline_from(Some(&old)),
        timeline(),
        "an absent caster reads empty"
    );

    // A newer writer's kind: dropped, the rest kept.
    let newer = json::parse(
        r#"{"bucket_ms":1000,"buckets":[1],"marks":[{"at_ms":1,"kind":8,"label":"?","spell_id":1,"dur_ms":0,"src":"x"},{"at_ms":2,"kind":7,"label":"Combustion","spell_id":190319,"dur_ms":12000,"src":"Player-1-0M"}]}"#,
    )
    .unwrap();
    let got = timeline_from(Some(&newer));
    assert_eq!(got.marks.len(), 1);
    assert_eq!(got.marks[0].kind, MarkKind::Cooldown);
    assert_eq!(got.marks[0].src, "Player-1-0M");
}
