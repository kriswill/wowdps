//! Editions of the game in one history store: two editions share encounter
//! and difficulty ids, and neither's fights touch the other's bookkeeping —
//! one's kill marks none of the other's wipes as past progression, one's
//! fights never crowd out or outrace the other's in retention, bests and
//! the owner are reckoned per edition — while a card that names no edition
//! (PROJECT_ID 0 or absent) keeps the grouping it always had, with Retail,
//! and an unlisted nonzero id is some other game, kept apart from Retail.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use wowdps_core::model::{Edition, Role, Spec};
use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{Backend, ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_proto::history::{Affiliation, CardPlayer, FightCard};
use wowdps_proto::{FightSort, HistoryAnswer, HistoryQuery, OriginFilter};

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("wowdps-editions-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One edition's log of one boss — encounter 3130 at Heroic, the same ids
/// in every edition — on the July `day` given, so two logs never share an
/// identity. `project_id` `None` writes a header without the field, as an
/// older client did. One player, `guid` named `name`, hits the boss once
/// per pull for `damage`; pulls are (seconds, kill, damage), ten minutes
/// apart.
struct Log<'a> {
    day: u32,
    project_id: Option<u8>,
    guid: &'a str,
    name: &'a str,
    pulls: &'a [(u32, bool, u64)],
}

impl Log<'_> {
    fn text(&self) -> String {
        let date = format!("7/{}/2026", self.day);
        let header = match self.project_id {
            Some(id) => format!(
                "COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,{id}"
            ),
            None => "COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0".to_string(),
        };
        let mut out = format!(
            "{date} 20:00:00.000-4  {header}\n\
             {date} 20:00:01.000-4  ZONE_CHANGE,2769,\"Hall of Echoes\",15\n"
        );
        for (i, (dur_s, kill, damage)) in self.pulls.iter().enumerate() {
            let h = 20 + (i / 6) as u32;
            let m = ((i % 6) * 10) as u32;
            let ts = |sec: u32| format!("{date} {h:02}:{:02}:{:02}.000-4", m + sec / 60, sec % 60);
            out.push_str(&format!(
                "{}  ENCOUNTER_START,3130,\"The Ashen Warden\",15,20,2769\n",
                ts(0)
            ));
            out.push_str(&format!(
                "{}  SPELL_DAMAGE,{},\"{}\",0x511,0x0,Creature-0-9,\"The Ashen Warden\",0xa48,0x0,116,\"Frostbolt\",16,{damage},{damage},0,0,0,0,0,nil,nil\n",
                ts(1),
                self.guid,
                self.name,
            ));
            out.push_str(&format!(
                "{}  ENCOUNTER_END,3130,\"The Ashen Warden\",15,20,{},{}\n",
                ts(*dur_s),
                u8::from(*kill),
                dur_s * 1000
            ));
        }
        out
    }

    /// Write the log under `tmp` and close every fight in it, as the tail
    /// thread would.
    fn fights(&self, tmp: &Temp) -> (PathBuf, Vec<ClosedFight>) {
        let path = tmp.0.join(format!("WoWCombatLog-{}.txt", self.day));
        std::fs::write(&path, self.text()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let mut engine = Engine::new();
        let mut events = Vec::new();
        engine.on_tail(TailEvent::Switched(path.clone()), &mut events);
        engine.on_tail(
            TailEvent::Lines(text.lines().map(str::to_string).collect()),
            &mut events,
        );
        engine.on_tail(TailEvent::CaughtUp, &mut events);
        let fights = events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::Closed(id) => engine.take_closed(*id),
                EngineEvent::Opened(_) => None,
            })
            .collect();
        (path, fights)
    }
}

/// Store every fight of `logs` into one in-memory store, log by log;
/// the ids of each log's fights, in log order.
fn store_logs(tmp: &Temp, store: &mut Store<MemBackend>, logs: &[Log<'_>]) -> Vec<Vec<String>> {
    logs.iter()
        .map(|log| {
            let (path, fights) = log.fights(tmp);
            let facts = LogFacts::read(&path);
            fights
                .iter()
                .map(|f| store.store(f, facts).expect("stored"))
                .collect()
        })
        .collect()
}

const RETAIL: Option<u8> = Some(1);
const FOREVER: Option<u8> = Some(18);

fn edition_of(store: &Store<MemBackend>, id: &str) -> Edition {
    store.card(id).unwrap().edition()
}

fn has(store: &Store<MemBackend>, id: &str) -> bool {
    store.card(id).is_some()
}

// ---- progression ---------------------------------------------------------------

/// The same boss id at the same difficulty in two editions: a kill in one
/// leaves the other's wipes progression — kept whole, details and all —
/// both ways round.
#[test]
fn one_editions_kill_ends_no_progression_of_anothers() {
    for (wiped, killed) in [(RETAIL, FOREVER), (FOREVER, RETAIL)] {
        let tmp = Temp::new("prog");
        let mut store = Store::open(MemBackend::new(), Retention::default());
        let ids = store_logs(
            &tmp,
            &mut store,
            &[
                Log {
                    day: 27,
                    project_id: wiped,
                    guid: "Player-1-A",
                    name: "Ana-Realm",
                    pulls: &[(30, false, 1000), (40, false, 1000)],
                },
                Log {
                    day: 28,
                    project_id: killed,
                    guid: "Player-2-B",
                    name: "Bex-Realm",
                    pulls: &[(90, true, 1000)],
                },
            ],
        );
        let (wipes, kill) = (&ids[0], &ids[1][0]);
        assert_eq!(
            edition_of(&store, kill),
            Edition::from_project_id(killed.unwrap())
        );
        for id in wipes {
            assert_eq!(
                edition_of(&store, id),
                Edition::from_project_id(wiped.unwrap())
            );
            let card = store.card(id).unwrap();
            assert!(
                store.is_progression(card),
                "{wiped:?} wipe after a {killed:?} kill"
            );
            assert!(store.kept_whole(card));
            assert!(
                store.has_details(id),
                "a 30 s progression wipe keeps its details"
            );
        }
        let st = store.status();
        assert_eq!((st.kept_kills, st.kept_progression), (1, 2), "{wiped:?}");

        // The wiped edition's own kill ends its progression, and only then.
        let tmp2 = Temp::new("prog-own");
        let ids2 = store_logs(
            &tmp2,
            &mut store,
            &[Log {
                day: 29,
                project_id: wiped,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(95, true, 1000)],
            }],
        );
        for id in wipes {
            assert!(!store.is_progression(store.card(id).unwrap()), "{wiped:?}");
        }
        assert!(has(&store, &ids2[0][0]));
        let st = store.status();
        assert_eq!((st.kept_kills, st.kept_progression), (2, 0), "{wiped:?}");
    }
}

// ---- retention -------------------------------------------------------------------

/// The per-group caps count each edition's fights alone: keep one wipe per
/// boss, and each edition keeps its own newest — the other edition's newer
/// wipes never evict it, nor take its details slot.
#[test]
fn the_caps_count_each_editions_fights_apart() {
    let tmp = Temp::new("caps");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_per_encounter: 1,
            keep_details_per_encounter: 1,
            keep_progression: false,
            ..Retention::default()
        },
    );
    let ids = store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: RETAIL,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(70, false, 1000), (71, false, 1000), (72, false, 1000)],
            },
            Log {
                day: 28,
                project_id: FOREVER,
                guid: "Player-2-B",
                name: "Bex-Realm",
                pulls: &[(80, false, 1000), (81, false, 1000), (82, false, 1000)],
            },
        ],
    );
    let kept: Vec<(Edition, i64)> = store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.duration_ms))
        .collect();
    assert_eq!(
        kept,
        [(Edition::Retail, 72_000), (Edition::Forever, 82_000)],
        "each edition's newest wipe"
    );
    assert!(store.has_details(&ids[0][2]) && store.has_details(&ids[1][2]));
    assert_eq!(store.backend().list("details").len(), 2);
}

/// The fastest kill is protected per edition: with kills not kept whole and
/// no room under the cap, each edition keeps its own fastest — a faster kill
/// in the other edition never takes its place.
#[test]
fn the_fastest_kill_is_each_editions_own() {
    let tmp = Temp::new("fastest");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_whole: false,
            keep_per_encounter: 0,
            ..Retention::default()
        },
    );
    store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: RETAIL,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(90, true, 1000), (60, true, 1000)],
            },
            Log {
                day: 28,
                project_id: FOREVER,
                guid: "Player-2-B",
                name: "Bex-Realm",
                pulls: &[(150, true, 1000), (120, true, 1000)],
            },
        ],
    );
    let kept: Vec<(Edition, i64)> = store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.duration_ms))
        .collect();
    assert_eq!(
        kept,
        [(Edition::Retail, 60_000), (Edition::Forever, 120_000)]
    );
}

/// One owner's wipes in two editions' logs, kills not kept whole and no
/// room under the cap, so only each edition's best survives: the kept
/// cards' (edition, dps), oldest first, and their owners.
fn bests_kept(
    a: (Option<u8>, &str, [u64; 2]),
    b: (Option<u8>, &str, [u64; 2]),
) -> Vec<(Edition, f64, Option<String>)> {
    let tmp = Temp::new("best");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_whole: false,
            keep_progression: false,
            keep_per_encounter: 0,
            characters: vec!["Ana".to_string()],
            ..Retention::default()
        },
    );
    let logs = [(27, a), (28, b)].map(|(day, (project_id, guid, [d1, d2]))| {
        (day, project_id, guid, [(60, false, d1), (60, false, d2)])
    });
    let logs: Vec<Log<'_>> = logs
        .iter()
        .map(|(day, project_id, guid, pulls)| Log {
            day: *day,
            project_id: *project_id,
            guid,
            name: if *day == 27 {
                "Ana-Realm"
            } else {
                "Ana-Elsewhere"
            },
            pulls,
        })
        .collect();
    store_logs(&tmp, &mut store, &logs);
    for (project_id, guid, _) in [a, b] {
        let edition = Edition::from_project_id(project_id.unwrap_or(0));
        assert_eq!(
            store.owner_in(edition),
            Some((guid.to_string(), false)),
            "{edition:?}"
        );
    }
    store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.players[0].dps.round(), c.owner.clone()))
        .collect()
}

/// The owner's personal bests are per edition: the same configured name is
/// a character in each edition, and each keeps its own best wipe however
/// the other edition's numbers compare — higher or lower, and between two
/// Classic clients as between Retail and Forever.
#[test]
fn the_owners_best_is_kept_per_edition() {
    let owned = |g: &str| Some(g.to_string());
    assert_eq!(
        bests_kept(
            (RETAIL, "Player-1-A", [12_000, 6_000]),
            (FOREVER, "Player-2-A", [4_800, 3_000])
        ),
        [
            (Edition::Retail, 200.0, owned("Player-1-A")),
            (Edition::Forever, 80.0, owned("Player-2-A")),
        ],
        "Retail's best the higher"
    );
    assert_eq!(
        bests_kept(
            (RETAIL, "Player-1-A", [4_800, 3_000]),
            (FOREVER, "Player-2-A", [12_000, 6_000])
        ),
        [
            (Edition::Retail, 80.0, owned("Player-1-A")),
            (Edition::Forever, 200.0, owned("Player-2-A")),
        ],
        "Forever's best the higher"
    );
    assert_eq!(
        bests_kept(
            (Some(2), "Player-3-A", [4_800, 3_000]),
            (Some(19), "Player-4-A", [12_000, 6_000])
        ),
        [
            (Edition::Classic(2), 80.0, owned("Player-3-A")),
            (Edition::Classic(19), 200.0, owned("Player-4-A")),
        ],
        "two Classic clients"
    );
}

/// Two Classic clients share a key but are two games: the caps count each
/// one's fights apart, and one's kill ends no progression of the other's.
#[test]
fn two_classic_clients_count_apart() {
    let tmp = Temp::new("classic-caps");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_per_encounter: 1,
            keep_progression: false,
            ..Retention::default()
        },
    );
    store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: Some(2),
                guid: "Player-3-A",
                name: "Ana-Realm",
                pulls: &[(70, false, 1000), (71, false, 1000)],
            },
            Log {
                day: 28,
                project_id: Some(19),
                guid: "Player-4-B",
                name: "Bex-Realm",
                pulls: &[(80, false, 1000), (81, false, 1000)],
            },
        ],
    );
    let kept: Vec<(Edition, i64)> = store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.duration_ms))
        .collect();
    assert_eq!(
        kept,
        [
            (Edition::Classic(2), 71_000),
            (Edition::Classic(19), 81_000)
        ],
        "each client's newest wipe"
    );

    let tmp = Temp::new("classic-prog");
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids = store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: Some(19),
                guid: "Player-4-B",
                name: "Bex-Realm",
                pulls: &[(30, false, 1000)],
            },
            Log {
                day: 28,
                project_id: Some(2),
                guid: "Player-3-A",
                name: "Ana-Realm",
                pulls: &[(90, true, 1000)],
            },
        ],
    );
    assert!(store.is_progression(store.card(&ids[0][0]).unwrap()));
}

// ---- no number, and an unlisted one -------------------------------------------------

/// A card whose log named no edition counts as Retail, exactly as every
/// card before editions did: a Retail kill ends its progression and its
/// kill ends Retail's, and one cap counts them together.
#[test]
fn a_card_with_no_number_groups_with_retail() {
    for (wiped, killed) in [(None, RETAIL), (RETAIL, None), (Some(0), RETAIL)] {
        let tmp = Temp::new("unknown-prog");
        let mut store = Store::open(MemBackend::new(), Retention::default());
        let ids = store_logs(
            &tmp,
            &mut store,
            &[
                Log {
                    day: 27,
                    project_id: wiped,
                    guid: "Player-1-A",
                    name: "Ana-Realm",
                    pulls: &[(30, false, 1000)],
                },
                Log {
                    day: 28,
                    project_id: killed,
                    guid: "Player-1-A",
                    name: "Ana-Realm",
                    pulls: &[(90, true, 1000)],
                },
            ],
        );
        let wipe = &ids[0][0];
        assert_eq!(
            edition_of(&store, wipe).counts_as(),
            Edition::Retail,
            "{wiped:?}"
        );
        assert!(
            !store.is_progression(store.card(wipe).unwrap()),
            "{wiped:?} wipe, {killed:?} kill"
        );
    }

    let tmp = Temp::new("unknown-caps");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_per_encounter: 1,
            keep_progression: false,
            ..Retention::default()
        },
    );
    store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: RETAIL,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(70, false, 1000)],
            },
            Log {
                day: 28,
                project_id: None,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(80, false, 1000)],
            },
        ],
    );
    let kept: Vec<(Edition, i64)> = store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.duration_ms))
        .collect();
    assert_eq!(
        kept,
        [(Edition::Unknown(0), 80_000)],
        "one group: the newest wipe of the two"
    );
}

/// A nonzero id this build does not list is some other game, never Retail:
/// Retail's kill leaves its wipes progression, the caps count it apart, and
/// its owner is its own.
#[test]
fn an_unlisted_edition_is_kept_apart_from_retail() {
    const UNLISTED: Option<u8> = Some(7);
    let tmp = Temp::new("unlisted-prog");
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids = store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: UNLISTED,
                guid: "Player-7-A",
                name: "Ana-Realm",
                pulls: &[(30, false, 1000)],
            },
            Log {
                day: 28,
                project_id: RETAIL,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(90, true, 1000)],
            },
        ],
    );
    let wipe = store.card(&ids[0][0]).unwrap();
    assert_eq!(wipe.edition(), Edition::Unknown(7));
    assert_eq!(wipe.edition().counts_as(), Edition::Unknown(7));
    assert!(store.is_progression(wipe), "Retail's kill is not its kill");

    let tmp = Temp::new("unlisted-caps");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_per_encounter: 1,
            keep_progression: false,
            ..Retention::default()
        },
    );
    store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 27,
                project_id: RETAIL,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(70, false, 1000)],
            },
            Log {
                day: 28,
                project_id: UNLISTED,
                guid: "Player-7-A",
                name: "Ana-Realm",
                pulls: &[(80, false, 1000)],
            },
        ],
    );
    let kept: Vec<(Edition, i64)> = store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.duration_ms))
        .collect();
    assert_eq!(
        kept,
        [(Edition::Retail, 70_000), (Edition::Unknown(7), 80_000)],
        "two groups"
    );

    // Its owner is inferred from its own logs alone, and one of its logs
    // is never Retail's second.
    let store = store_of(
        &[
            card(
                1,
                100,
                1,
                &[
                    ("G-me", "Me-Realm", true, Spec::Fire, 500.0),
                    ("G-a", "A-Realm", true, Spec::Arms, 1.0),
                ],
            ),
            card(
                2,
                200,
                1,
                &[
                    ("G-me", "Me-Realm", true, Spec::Fire, 500.0),
                    ("G-b", "B-Realm", true, Spec::Arms, 1.0),
                ],
            ),
            card(
                3,
                300,
                7,
                &[
                    ("G-x", "X-Realm", true, Spec::Arms, 400.0),
                    ("G-c", "C-Realm", true, Spec::Fire, 1.0),
                ],
            ),
            card(
                4,
                400,
                7,
                &[
                    ("G-x", "X-Realm", true, Spec::Arms, 400.0),
                    ("G-d", "D-Realm", true, Spec::Fire, 1.0),
                ],
            ),
        ],
        Retention::default(),
    );
    assert_eq!(store.owner(), Some(("G-me".to_string(), true)));
    assert_eq!(
        store.owner_in(Edition::Unknown(7)),
        Some(("G-x".to_string(), true))
    );
    let store = store_of(
        &[
            card(1, 100, 1, &[("G-me", "Me-Realm", true, Spec::Fire, 500.0)]),
            card(3, 300, 7, &[("G-x", "X-Realm", false, Spec::Arms, 400.0)]),
        ],
        Retention::default(),
    );
    assert_eq!(
        store.owner(),
        None,
        "the unlisted log is not Retail's second"
    );
}

// ---- who is "me" --------------------------------------------------------------------

fn card(
    log: u64,
    start: i64,
    project_id: u8,
    players: &[(&str, &str, bool, Spec, f64)],
) -> FightCard {
    FightCard {
        id: wowdps_proto::history::fight_id(log, start, false),
        log,
        start_local_ms: start,
        start_utc_ms: start,
        project_id,
        players: players
            .iter()
            .map(|(guid, name, logged, spec, dps)| CardPlayer {
                guid: guid.to_string(),
                name: name.to_string(),
                logged: *logged,
                class: Some(spec.class()),
                spec: Some(*spec),
                dps: *dps,
                ..CardPlayer::default()
            })
            .collect(),
        ..FightCard::default()
    }
}

fn store_of(cards: &[FightCard], cfg: Retention) -> Store<MemBackend> {
    let mut backend = MemBackend::new();
    for c in cards {
        backend
            .write(
                "fights",
                &format!("{}.json", c.id),
                c.to_json().to_line().as_bytes(),
            )
            .unwrap();
    }
    Store::open(backend, cfg)
}

/// The two-logs guard counts one edition's logs: a Retail log that logged
/// one player and a Forever log that logged nobody are one Retail log, not
/// two, so they name no owner; before editions the empty log still counted
/// toward the two.
#[test]
fn another_editions_log_is_never_the_second_log() {
    let store = store_of(
        &[
            card(1, 100, 1, &[("G-me", "Me-Realm", true, Spec::Fire, 500.0)]),
            card(2, 200, 18, &[("G-x", "X-Realm", false, Spec::Arms, 400.0)]),
        ],
        Retention::default(),
    );
    assert_eq!(store.owner(), None);
    assert_eq!(store.owner_in(Edition::Forever), None);
    assert!(store.owners().is_empty());
    assert!(!store.status().owner_inferred);
}

/// Each edition infers its own owner from its own logs, and one edition's
/// logs never empty the other's intersection; each card is stamped with
/// its own edition's owner, and the account's characters hold both.
#[test]
fn each_edition_infers_its_own_owner() {
    let cards = [
        card(
            1,
            100,
            1,
            &[
                ("G-me", "Me-Realm", true, Spec::Fire, 500.0),
                ("G-a", "A-Realm", true, Spec::Arms, 450.0),
            ],
        ),
        card(
            2,
            200,
            1,
            &[
                ("G-me", "Me-Realm", true, Spec::Fire, 300.0),
                ("G-b", "B-Realm", true, Spec::Arms, 350.0),
            ],
        ),
        card(
            3,
            300,
            18,
            &[
                ("G-fme", "Fme-Realm", true, Spec::Arms, 400.0),
                ("G-c", "C-Realm", true, Spec::Fire, 50.0),
            ],
        ),
        card(
            4,
            400,
            18,
            &[
                ("G-fme", "Fme-Realm", true, Spec::Arms, 100.0),
                ("G-d", "D-Realm", true, Spec::Fire, 90.0),
            ],
        ),
    ];
    let store = store_of(&cards, Retention::default());
    assert_eq!(store.owner(), Some(("G-me".to_string(), true)));
    assert_eq!(
        store.owner_in(Edition::Unknown(0)),
        Some(("G-me".to_string(), true)),
        "a no-number card's owner is Retail's"
    );
    assert_eq!(
        store.owner_in(Edition::Forever),
        Some(("G-fme".to_string(), true))
    );
    assert_eq!(store.owners().len(), 2);
    assert!(store.status().owner_inferred);
    let owners: Vec<Option<&str>> = store.cards().iter().map(|c| c.owner.as_deref()).collect();
    assert_eq!(
        owners,
        [Some("G-me"), Some("G-me"), Some("G-fme"), Some("G-fme")],
        "stamped on open, each by its own edition's owner"
    );
    let mine = store.mine();
    assert!(mine.owns("G-me", "Me-Realm") && mine.owns("G-fme", "Fme-Realm"));
    assert!(!mine.owns("G-c", "C-Realm"));

    // A fights answer reads each card's own edition's owner: the role
    // filter's subject and the owner-per-second sort.
    let answer = store.answer(&HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: None,
        since_utc_ms: None,
        kind: None,
        sort: FightSort::OwnerPerSec,
        limit: 0,
        after_id: None,
        role: Some(Role::Dps),
        origin: OriginFilter::Local,
    });
    let HistoryAnswer::Fights { cards: got, total } = answer else {
        panic!("Fights asked, {answer:?} answered");
    };
    assert_eq!(total, 4, "every owner played dps on their own cards");
    let order: Vec<i64> = got.iter().map(|c| c.start_utc_ms).collect();
    assert_eq!(order, [100, 300, 200, 400], "500, 400, 300, 100 dps");
    let answer = store.answer(&HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: None,
        since_utc_ms: None,
        kind: None,
        sort: FightSort::Newest,
        limit: 0,
        after_id: None,
        role: Some(Role::Healer),
        origin: OriginFilter::Local,
    });
    let HistoryAnswer::Fights { total, .. } = answer else {
        panic!("Fights asked, {answer:?} answered");
    };
    assert_eq!(total, 0, "no owner healed");
}

/// A store of Retail cards and cards with no number alone has one owner,
/// as it always had: the two are one for the inference.
#[test]
fn retail_and_no_number_cards_share_one_owner() {
    let store = store_of(
        &[
            card(
                1,
                100,
                0,
                &[
                    ("G-me", "Me-Realm", true, Spec::Fire, 500.0),
                    ("G-a", "A-Realm", true, Spec::Arms, 450.0),
                ],
            ),
            card(
                2,
                200,
                1,
                &[
                    ("G-me", "Me-Realm", true, Spec::Fire, 300.0),
                    ("G-b", "B-Realm", true, Spec::Arms, 350.0),
                ],
            ),
        ],
        Retention::default(),
    );
    assert_eq!(store.owner(), Some(("G-me".to_string(), true)));
    let owners = store.owners();
    assert_eq!(owners.len(), 1);
    assert_eq!(
        owners.get(&Edition::Retail),
        Some(&("G-me".to_string(), true))
    );
}

/// The sticky stamp is each edition's own: once an owner was stamped on an
/// edition's cards, a later night that empties that edition's intersection
/// (an alt's) keeps them known, and a newer night of the other edition —
/// with its own stamped owner — never replaces them. Both ways round.
#[test]
fn the_sticky_owner_stamp_is_each_editions_own() {
    let stamp = |owner: &str, mut c: FightCard| {
        c.owner = Some(owner.to_string());
        c
    };
    let p = |g, n| (g, n, true, Spec::Fire, 100.0);
    for (first, second) in [(1u8, 18u8), (18, 1)] {
        let (me1, me2) = if first == 1 {
            ("G-me", "G-fme")
        } else {
            ("G-fme", "G-me")
        };
        let cards = [
            // The first edition: a stamped night, then an alt's night.
            stamp(
                me1,
                card(1, 100, first, &[p(me1, "One-Realm"), p("G-a", "A-Realm")]),
            ),
            card(
                2,
                200,
                first,
                &[p("G-alt", "Alt-Realm"), p("G-b", "B-Realm")],
            ),
            // The other edition, newer: the same, with its own owner.
            stamp(
                me2,
                card(3, 300, second, &[p(me2, "Two-Realm"), p("G-c", "C-Realm")]),
            ),
            card(
                4,
                400,
                second,
                &[p("G-falt", "Falt-Realm"), p("G-d", "D-Realm")],
            ),
        ];
        let store = store_of(&cards, Retention::default());
        let first_e = Edition::from_project_id(first);
        let second_e = Edition::from_project_id(second);
        assert_eq!(
            store.owner_in(first_e),
            Some((me1.to_string(), true)),
            "{first_e:?} keeps its stamp under {second_e:?}'s newer one"
        );
        assert_eq!(store.owner_in(second_e), Some((me2.to_string(), true)));
        // The alt nights stay unowned: neither owner is on their roster.
        assert_eq!(store.cards()[1].owner, None);
        assert_eq!(store.cards()[3].owner, None);
    }
}

/// The addon's own-character set names each edition's owner from that
/// edition's newest card it is on: the account's character on the newest
/// card of one edition never becomes the other edition's owner. Both ways
/// round.
#[test]
fn the_addons_own_character_names_each_editions_owner() {
    let p = |g, n| (g, n, true, Spec::Fire, 100.0);
    for (older, newer) in [(1u8, 18u8), (18, 1)] {
        // One log per edition, so the intersection names nobody: the
        // addon alone says who the account's characters are.
        let mut store = store_of(
            &[
                card(
                    1,
                    100,
                    older,
                    &[p("G-old", "Old-Realm"), p("G-a", "A-Realm")],
                ),
                card(
                    2,
                    200,
                    newer,
                    &[p("G-new", "New-Realm"), p("G-b", "B-Realm")],
                ),
            ],
            Retention::default(),
        );
        assert!(store.owners().is_empty());
        let affiliation = |guid: &str| Affiliation {
            schema: wowdps_proto::history::HISTORY_SCHEMA,
            guid: guid.to_string(),
            name: guid.to_string(),
            realm: "Testrealm".to_string(),
            guild: String::new(),
            guild_realm: None,
            rank: None,
            class: None,
            faction: None,
            mine: true,
            seen_utc_ms: 1_000,
            account: "TEST".to_string(),
        };
        store.merge_affiliations(vec![affiliation("G-old"), affiliation("G-new")]);
        let older_e = Edition::from_project_id(older);
        let newer_e = Edition::from_project_id(newer);
        assert_eq!(
            store.owner_in(older_e),
            Some(("G-old".to_string(), true)),
            "{older_e:?} under {newer_e:?}'s newer card"
        );
        assert_eq!(store.owner_in(newer_e), Some(("G-new".to_string(), true)));
        assert_eq!(store.owners().len(), 2);
    }
}
