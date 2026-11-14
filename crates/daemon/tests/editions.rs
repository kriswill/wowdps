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

use std::path::{Path, PathBuf};

use wowdps_core::model::{Edition, Role, SegmentId, Spec, View};
use wowdps_core::tail::{SourceSpec, TailEvent, Tailer};
use wowdps_daemon::engine::{Engine, EngineEvent, line_utc_ms};
use wowdps_daemon::history::{Backend, ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_daemon::mock::{MockDaemon, pump};
use wowdps_proto::history::{Affiliation, CardPlayer, FightCard};
use wowdps_proto::{
    ClientMsg, ClientState, Cursor, DaemonMsg, EditionRef, FightSort, HistoryAnswer, HistoryQuery,
    OriginFilter, SegmentRef, TrendBucket, TrendMeasure,
};

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
/// `configured`: the owner is named by `history_characters`, the logs read
/// as a typed `--file` or `--logs` names them (their logger untrusted).
/// Else (R31) nothing is configured, and the logs are the install's own:
/// each card's logger, the one player flagged MINE, is its owner.
fn bests_kept(
    a: (Option<u8>, &str, [u64; 2]),
    b: (Option<u8>, &str, [u64; 2]),
    configured: bool,
) -> Vec<(Edition, f64, Option<String>)> {
    let tmp = Temp::new("best");
    let mut store = Store::open(
        MemBackend::new(),
        Retention {
            keep_whole: false,
            keep_progression: false,
            keep_per_encounter: 0,
            // A Forever character is configured by both names: its first
            // name alone names no one.
            characters: if configured {
                vec!["Ana".to_string(), "Ana Moss".to_string()]
            } else {
                Vec::new()
            },
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
            } else if *project_id == FOREVER {
                "Ana-Moss-Elsewhere-US"
            } else {
                "Ana-Elsewhere"
            },
            pulls,
        })
        .collect();
    for log in &logs {
        let (path, fights) = log.fights(&tmp);
        let facts = LogFacts::read(&path);
        for mut f in fights {
            f.own_log = !configured;
            store.store(&f, facts).expect("stored");
        }
    }
    for (project_id, guid, _) in [a, b] {
        let edition = Edition::from_project_id(project_id.unwrap_or(0));
        // A configured owner is named; one the flags name is inferred.
        assert_eq!(
            store.owner_in(edition),
            Some((guid.to_string(), !configured)),
            "{edition:?}"
        );
    }
    store
        .cards()
        .iter()
        .map(|c| (c.edition(), c.players[0].dps.round(), c.owner.clone()))
        .collect()
}

/// The owner's personal bests are per edition: the same configured names
/// name a character in each edition (Forever's by both its names), and each keeps its own best wipe however
/// the other edition's numbers compare — higher or lower, and between two
/// Classic clients as between Retail and Forever.
#[test]
fn the_owners_best_is_kept_per_edition() {
    the_owners_best_per_edition(true);
}

/// R31: and with nothing configured, by the logger each edition's own log
/// flags — Forever's, whose log names no COMBATANT_INFO to intersect, as
/// Retail's.
#[test]
fn the_owners_best_is_kept_per_edition_by_the_logger_alone() {
    the_owners_best_per_edition(false);
}

fn the_owners_best_per_edition(configured: bool) {
    let owned = |g: &str| Some(g.to_string());
    assert_eq!(
        bests_kept(
            (RETAIL, "Player-1-A", [12_000, 6_000]),
            (FOREVER, "Player-2-A", [4_800, 3_000]),
            configured
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
            (FOREVER, "Player-2-A", [12_000, 6_000]),
            configured
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
            (Some(19), "Player-4-A", [12_000, 6_000]),
            configured
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
    assert!(
        mine.of(Edition::Retail).owns("G-me", "Me-Realm")
            && mine.of(Edition::Retail).owns("G-fme", "Fme-Realm")
    );
    assert!(!mine.of(Edition::Retail).owns("G-c", "C-Realm"));

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
        edition: None,
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
        edition: None,
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

// ---- v49: the queries' edition filter --------------------------------------------

/// Every fixed question takes an edition: none answers every edition, each
/// card naming its own; one answers that edition's cards — a card whose log
/// named no edition with Retail's — and an edition with no cards answers
/// empty. The same player (one guid) in two editions' logs.
#[test]
fn every_query_takes_an_edition_filter() {
    let tmp = Temp::new("filter");
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids = store_logs(
        &tmp,
        &mut store,
        &[
            Log {
                day: 21,
                project_id: RETAIL,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(30, false, 1000), (60, true, 2000)],
            },
            Log {
                day: 23,
                project_id: FOREVER,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(50, true, 3000)],
            },
            Log {
                day: 25,
                project_id: None,
                guid: "Player-1-A",
                name: "Ana-Realm",
                pulls: &[(40, false, 4000)],
            },
        ],
    );
    let forever_kill = ids[1][0].clone();
    let fights = |edition| -> (Vec<String>, u32) {
        match store.answer(&HistoryQuery::Fights {
            encounter: Some(3130),
            difficulty: Some(15),
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Newest,
            limit: 0,
            after_id: None,
            role: None,
            origin: OriginFilter::Local,
            edition,
        }) {
            HistoryAnswer::Fights { cards, total } => {
                let mut ids: Vec<String> = cards.into_iter().map(|c| c.id).collect();
                ids.sort();
                (ids, total)
            }
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(fights(None).1, 4, "every edition");
    assert_eq!(
        fights(Some(Edition::Retail)).1,
        3,
        "with the unnumbered one"
    );
    assert_eq!(fights(Some(Edition::Unknown(0))).1, 3, "reckoned as Retail");
    assert_eq!(
        fights(Some(Edition::Forever)),
        (vec![forever_kill.clone()], 1)
    );
    assert_eq!(fights(Some(Edition::Classic(2))).1, 0, "none of its cards");

    let progression = |edition| match store.answer(&HistoryQuery::Progression {
        encounter: 3130,
        difficulty: 15,
        local_cutover_hour: None,
        edition,
    }) {
        HistoryAnswer::Progression {
            pulls,
            kills,
            first_kill,
            nights,
            ..
        } => (pulls, kills, first_kill.map(|c| c.edition()), nights),
        other => panic!("{other:?}"),
    };
    let (pulls, kills, first, _) = progression(None);
    assert_eq!((pulls, kills, first), (4, 2, Some(Edition::Retail)));
    let (pulls, kills, first, _) = progression(Some(Edition::Retail));
    assert_eq!((pulls, kills, first), (3, 1, Some(Edition::Retail)));
    let (pulls, kills, first, nights) = progression(Some(Edition::Forever));
    assert_eq!((pulls, kills, first), (1, 1, Some(Edition::Forever)));

    let trend = |edition| match store.answer(&HistoryQuery::Trend {
        guid: "Player-1-A".to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure: TrendMeasure::Dps,
        bucket: TrendBucket::None,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
        edition,
    }) {
        HistoryAnswer::Trend(points) => points.into_iter().map(|p| p.fight_id).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    assert_eq!(trend(None).len(), 4);
    assert_eq!(trend(Some(Edition::Forever)), vec![forever_kill]);
    assert_eq!(trend(Some(Edition::Retail)).len(), 3);

    // The Forever kill's night, asked in each edition.
    let night = nights[0].day_utc_ms;
    let role_night = |edition| match store.answer(&HistoryQuery::RoleNight {
        encounter: 3130,
        difficulty: 15,
        night,
        local_cutover_hour: None,
        edition,
    }) {
        HistoryAnswer::RoleNight { night, rows } => (night.pulls, rows.len()),
        other => panic!("{other:?}"),
    };
    assert_eq!(role_night(None), (1, 1));
    assert_eq!(role_night(Some(Edition::Forever)), (1, 1));
    assert_eq!(role_night(Some(Edition::Retail)), (0, 0));
}

/// A Forever night is rostered by the roles R30 stored, not under an
/// unknown role: the Forever fixture's log names no spec, so the Priest who
/// healed the others is the night's healer and the other four deal damage,
/// each folded over the one pull by their role's measure.
#[test]
fn a_forever_night_is_rostered_by_its_stored_roles() {
    let path = Path::new(FOREVER_LOG);
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.to_path_buf()), &mut events);
    engine.on_tail(
        TailEvent::Lines(text.lines().map(str::to_string).collect()),
        &mut events,
    );
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    let facts = LogFacts::read(path);
    let mut store = Store::open(MemBackend::new(), Retention::default());
    for e in &events {
        if let EngineEvent::Closed(id) = e
            && let Some(fight) = engine.take_closed(*id)
        {
            store.store(&fight, facts);
        }
    }
    let kill = store
        .cards()
        .iter()
        .find(|c| c.name == "Geilhast")
        .expect("the kill is stored")
        .clone();
    assert_eq!(kill.edition(), Edition::Forever);
    assert!(kill.players.iter().all(|p| p.spec.is_none()), "no spec");
    let healer = "Player-6101-0A2B3C04";
    let night = match store.answer(&HistoryQuery::Progression {
        encounter: 2914,
        difficulty: 1,
        local_cutover_hour: None,
        edition: None,
    }) {
        HistoryAnswer::Progression { nights, .. } => nights[0].day_utc_ms,
        other => panic!("{other:?}"),
    };
    for edition in [None, Some(Edition::Forever)] {
        let rows = match store.answer(&HistoryQuery::RoleNight {
            encounter: 2914,
            difficulty: 1,
            night,
            local_cutover_hour: None,
            edition,
        }) {
            HistoryAnswer::RoleNight { night, rows } => {
                assert_eq!(night.pulls, 1);
                rows
            }
            other => panic!("{other:?}"),
        };
        let roster: Vec<(&str, Option<Role>, u32)> = rows
            .iter()
            .map(|r| (r.guid.as_str(), r.role, r.pulls))
            .collect();
        assert_eq!(
            roster,
            [
                (healer, Some(Role::Healer), 1),
                // Damage dealers by effective DPS: 670, 205, 180, 140.
                ("Player-6101-0A2B3C02", Some(Role::Dps), 1),
                ("Player-6101-0A2B3C05", Some(Role::Dps), 1),
                ("Player-6101-0A2B3C01", Some(Role::Dps), 1),
                ("Player-6101-0A2B3C03", Some(Role::Dps), 1),
            ],
            "{edition:?}"
        );
        for r in &rows {
            let p = kill.players.iter().find(|p| p.guid == r.guid).unwrap();
            assert_eq!(r.role, p.role(), "the card's role");
            assert_eq!(r.spec, None);
            let want = if r.guid == healer {
                p.hps
            } else {
                p.effective_dps(kill.rate_ms())
            };
            assert_eq!(r.measure.to_bits(), want.to_bits(), "{}", r.guid);
        }
    }
}

// ---- v49: the live side, one source ----------------------------------------------

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const FOREVER_LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");

fn live_cursor() -> Cursor {
    Cursor::Segment {
        segment: SegmentRef::Live,
        view: View::Damage,
        top_n: None,
        drill: None,
        death: None,
        spell: None,
        range: None,
    }
}

/// The one frame a Watch is answered with.
fn answer(mock: &mut MockDaemon, cursor: Cursor, edition: EditionRef) -> DaemonMsg {
    let mut out = mock.handle(ClientMsg::Watch { cursor, edition });
    assert_eq!(out.len(), 1, "{out:?}");
    out.pop().unwrap()
}

/// One source, as the daemon has today: following is answered as before,
/// every frame and row naming the edition the log names; a pin to that
/// edition is answered exactly as following; a pin to another is the empty
/// answer — no list, no live segment — never an error; an id still names
/// its segment under any pin. For a Retail log and a Forever log alike.
#[test]
fn one_source_answers_a_pin_to_its_edition_and_nothing_else() {
    for (path, edition, other) in [
        (SAMPLE, Edition::Retail, Edition::Forever),
        (FOREVER_LOG, Edition::Forever, Edition::Retail),
    ] {
        let mut mock = MockDaemon::fixture_at(Path::new(path));
        let list = |m: DaemonMsg| match m {
            DaemonMsg::SegmentList {
                entries,
                edition,
                source,
                ..
            } => (entries, edition, source),
            other => panic!("{other:?}"),
        };
        let (follow, named, source) = list(answer(&mut mock, Cursor::List, EditionRef::Follow));
        assert!(!follow.is_empty(), "{path}");
        assert_eq!(named, Some(edition), "{path}");
        assert!(source.is_some());
        assert!(
            follow.iter().all(|e| e.row.edition == edition),
            "every row is the log's: {path}"
        );
        let (pinned, named, _) = list(answer(&mut mock, Cursor::List, EditionRef::Pin(edition)));
        assert_eq!(pinned, follow, "{path}");
        assert_eq!(named, Some(edition));

        let meter = |m: DaemonMsg| match m {
            DaemonMsg::Snapshot {
                id,
                info,
                rows,
                edition,
                ..
            } => (id, info, rows, edition),
            other => panic!("{other:?}"),
        };
        let (id, info, rows, named) = meter(answer(&mut mock, live_cursor(), EditionRef::Follow));
        assert!(id.is_some() && !rows.is_empty(), "{path}");
        let guid = rows[0].key.clone();
        assert_eq!((info.edition, named), (edition, Some(edition)));
        let pinned = meter(answer(&mut mock, live_cursor(), EditionRef::Pin(edition)));
        assert_eq!(pinned, (id, info, rows, named), "{path}");

        // Another edition: nothing of this log.
        let (entries, named, source) =
            list(answer(&mut mock, Cursor::List, EditionRef::Pin(other)));
        assert!(entries.is_empty());
        assert_eq!((named, source), (Some(other), None), "{path}");
        let (id, info, rows, named) =
            meter(answer(&mut mock, live_cursor(), EditionRef::Pin(other)));
        assert_eq!((id, rows.len(), named), (None, 0, Some(other)), "{path}");
        assert_eq!(info.edition, other);
        // A one-shot's `Live` resolves in the session's last Watch's edition.
        let loadout = mock.handle(ClientMsg::GetLoadout {
            req_id: 1,
            segment: SegmentRef::Live,
            guid,
        });
        assert!(
            matches!(
                loadout.as_slice(),
                [DaemonMsg::Loadout { loadout: None, .. }]
            ),
            "{loadout:?}"
        );
        // A segment id names its segment wherever the cursor looks, and
        // its frame names where it lives: this source's edition.
        let first = follow.first().unwrap().id;
        let (id, info, _, named) = meter(answer(
            &mut mock,
            Cursor::Segment {
                segment: SegmentRef::Id(first),
                view: View::Damage,
                top_n: None,
                drill: None,
                death: None,
                spell: None,
                range: None,
            },
            EditionRef::Pin(other),
        ));
        assert_eq!(
            (id, info.edition, named),
            (Some(first), edition, Some(edition))
        );

        // Status names the one source's edition.
        match mock.handle(ClientMsg::GetStatus { req_id: 2 }).as_slice() {
            [DaemonMsg::Status { editions, .. }] => {
                assert_eq!(editions.len(), 1);
                assert_eq!(editions[0].edition, Some(edition), "{path}");
                assert!(editions[0].live);
            }
            other => panic!("{other:?}"),
        }
    }
}

/// A client state over the mock: pinned to an edition the log is not, it
/// sees an empty list of that edition; following again, the log's.
#[test]
fn a_client_state_pinned_elsewhere_sees_an_empty_list() {
    let mut mock = MockDaemon::fixture_at(Path::new(SAMPLE));
    let mut st = ClientState::new();
    let first = st.initial_request();
    pump(&mut st, &mut mock, vec![first]);
    let all = st.entries().len();
    assert!(all > 0);
    assert_eq!(st.edition(), Some(Edition::Retail));

    let sent = st.set_edition(EditionRef::Pin(Edition::Forever));
    pump(&mut st, &mut mock, sent);
    assert!(st.entries().is_empty());
    assert_eq!(st.edition(), Some(Edition::Forever));
    assert_eq!(st.source, None);

    let sent = st.set_edition(EditionRef::Follow);
    pump(&mut st, &mut mock, sent);
    assert_eq!(st.entries().len(), all);
    assert_eq!(st.edition(), Some(Edition::Retail));
}

/// A pin made of a value no number decodes to is the pin its number names,
/// in process as over the wire: `Unknown(18)` is a pin to Forever, served
/// by a Forever log and naming Forever.
#[test]
fn a_hand_made_pin_is_served_as_its_number_names() {
    let mut mock = MockDaemon::fixture_at(Path::new(FOREVER_LOG));
    for made in [Edition::Unknown(18), Edition::Classic(18)] {
        match answer(&mut mock, Cursor::List, EditionRef::Pin(made)) {
            DaemonMsg::SegmentList {
                entries, edition, ..
            } => {
                assert!(!entries.is_empty(), "{made:?}");
                assert_eq!(edition, Some(Edition::Forever), "{made:?}");
            }
            other => panic!("{other:?}"),
        }
    }
}

fn id_cursor(id: SegmentId) -> Cursor {
    Cursor::Segment {
        segment: SegmentRef::Id(id),
        view: View::Damage,
        top_n: None,
        drill: None,
        death: None,
        spell: None,
        range: None,
    }
}

/// A segment id's frame names where the segment lives — this source and
/// its edition — whatever edition the session looks in; `Live` and the
/// list keep naming the session's edition.
#[test]
fn a_segment_ids_frame_names_the_source_it_lives_in() {
    let mut mock = MockDaemon::fixture_at(Path::new(SAMPLE));
    let first = match answer(&mut mock, Cursor::List, EditionRef::Follow) {
        DaemonMsg::SegmentList { entries, .. } => entries[0].id,
        other => panic!("{other:?}"),
    };
    match answer(
        &mut mock,
        id_cursor(first),
        EditionRef::Pin(Edition::Forever),
    ) {
        DaemonMsg::Snapshot {
            id,
            source,
            edition,
            info,
            ..
        } => {
            assert_eq!(id, Some(first));
            assert!(source.as_deref().is_some_and(|s| s.contains("sample.txt")));
            assert_eq!(edition, Some(Edition::Retail), "the source's");
            assert_eq!(info.edition, Edition::Retail);
        }
        other => panic!("{other:?}"),
    }
}

/// Two editions' sources, one client: a state pinned to Forever, its list
/// Forever's, opens a segment by id that resolves in the Retail source —
/// as an id from one counter shared by every source can. That frame names
/// the Retail source and is taken: the state is not reset, its edition
/// and list stay Forever's, and Forever's next list lands as before.
#[test]
fn an_id_resolving_in_another_editions_source_moves_nothing() {
    let mut retail = MockDaemon::fixture_at(Path::new(SAMPLE));
    let mut forever = MockDaemon::fixture_at(Path::new(FOREVER_LOG));
    let mut st = ClientState::new();
    let sent = st.set_edition(EditionRef::Pin(Edition::Forever));
    pump(&mut st, &mut forever, sent);
    let rows = st.entries().len();
    assert!(rows > 1);
    // The state opens its list's first row by id …
    let sent = st.goto_list_pos(0);
    let Some(ClientMsg::Watch { cursor, edition }) = sent.first().cloned() else {
        panic!("{sent:?}");
    };
    // … and the source that holds that id is the other edition's.
    for frame in retail.handle(ClientMsg::Watch { cursor, edition }) {
        assert!(st.on_msg(frame).is_empty(), "no reset, no Watch");
    }
    assert_eq!(st.segment_edition(), Some(Edition::Retail));
    assert_eq!(st.edition(), Some(Edition::Forever));
    assert_eq!(st.edition_ref(), EditionRef::Pin(Edition::Forever));
    assert_eq!(st.entries().len(), rows);
    let source = st.source.clone();
    assert!(source.as_deref().is_some_and(|s| s.contains("forever.txt")));
    // Forever's list goes on landing without a reset.
    for frame in forever.handle(ClientMsg::Watch {
        cursor: Cursor::List,
        edition: EditionRef::Pin(Edition::Forever),
    }) {
        assert!(st.on_msg(frame).is_empty());
    }
    assert_eq!(st.source, source);
}

/// The daemon may name a source's edition from where it came from, before
/// its log's header is read: an engine named Forever serves Forever pins
/// and names Forever, not Retail, while its log says nothing — and the
/// header decides once read.
#[test]
fn a_source_named_before_its_header_is_that_edition() {
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.set_source_edition(Some(Edition::Forever));
    engine.on_tail(
        TailEvent::Switched(PathBuf::from("/logs/WoWCombatLog-new.txt")),
        &mut events,
    );
    assert_eq!(engine.edition(), Some(Edition::Forever));
    assert!(engine.serves(EditionRef::Pin(Edition::Forever)));
    assert!(!engine.serves(EditionRef::Pin(Edition::Retail)));
    engine.on_tail(
        TailEvent::Lines(vec![
            "7/27/2026 21:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1".to_string(),
        ]),
        &mut events,
    );
    assert_eq!(
        engine.edition(),
        Some(Edition::Retail),
        "the header decides"
    );
}

/// A client state pinned to Retail over a log whose first pull came before
/// any version line (reckoned Retail's, so it is served that pull), then a
/// line naming Forever: the state stops being served and is emptied at
/// once — the mock answers as the hub does — never left holding the
/// Forever log's ids under a Retail pin.
#[test]
fn a_state_that_stops_being_served_is_emptied() {
    let tmp = Temp::new("unserved");
    let path = tmp.0.join("WoWCombatLog-072726_210000.txt");
    // The pull is on a map no edition's table names (9999), so it opens no
    // visit (R10): what this holds is the edition switch, over a pull alone.
    std::fs::write(
        &path,
        "7/27/2026 21:00:10.000-4  ENCOUNTER_START,3130,\"Boss\",1,5,9999\n\
         7/27/2026 21:00:11.000-4  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n\
         7/27/2026 21:00:40.000-4  ENCOUNTER_END,3130,\"Boss\",1,5,1,30000\n",
    )
    .unwrap();
    let mut mock = MockDaemon::fixture_at(&path);
    let mut st = ClientState::new();
    let sent = st.set_edition(EditionRef::Pin(Edition::Retail));
    pump(&mut st, &mut mock, sent);
    assert_eq!(st.entries().len(), 1, "a log naming none serves Retail");
    let sent = st.goto_list_pos(0);
    pump(&mut st, &mut mock, sent);
    let replies = mock.feed(vec![
        "7/27/2026 21:05:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18".to_string(),
    ]);
    let mut sent = Vec::new();
    for reply in replies {
        sent.extend(st.on_msg(reply));
    }
    pump(&mut st, &mut mock, sent);
    assert!(st.entries().is_empty(), "no Forever id under a Retail pin");
    assert_eq!(st.edition(), Some(Edition::Retail));
}

/// A zero named as a source's edition is no edition named: `None`, never
/// `Unknown(0)`, which would differ from the Retail its header then names.
#[test]
fn a_source_named_zero_is_named_nothing() {
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(
        TailEvent::Switched(PathBuf::from("/logs/WoWCombatLog-new.txt")),
        &mut events,
    );
    engine.set_source_edition(Some(Edition::Unknown(0)));
    assert_eq!(engine.edition(), None);
    engine.set_source_edition(Some(Edition::Classic(18)));
    assert_eq!(
        engine.edition(),
        Some(Edition::Forever),
        "as its number names it"
    );
}

// ---- the live edition is the log showing the newest line ----------------------

/// Every event the tailer has now, fed to `engine` poll after poll until a
/// poll has none (the backlog read, then caught up); the source's newest
/// line time after them (`Engine::line_ms`).
fn settle(tailer: &mut Tailer, engine: &mut Engine) -> Option<i64> {
    for _ in 0..64 {
        let events = tailer.poll();
        if events.is_empty() {
            return engine.line_ms();
        }
        for ev in events {
            engine.on_tail(ev, &mut Vec::new());
        }
    }
    panic!("the tailer never settled");
}

fn append(path: &Path, text: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

const BUFF: &str = "11/1/2026 20:50:00.000-8  SPELL_AURA_APPLIED,Player-6101-0A2B3C03,\"Tharokk-Emberfell-Nebula-US\",0x512,0x80000000,Player-6101-0A2B3C03,\"Tharokk-Emberfell-Nebula-US\",0x512,0x80000000,71,\"Defensive Stance\",0x1,BUFF";

/// The newest line time a source's logs show, what the live edition is
/// ranked by, through a real tailer over a logs folder: the log's last
/// line when it is opened, at start or as the folder's first; then each
/// complete line appended, a half-written one not until it is complete;
/// and kept through every switch — the same log re-read after a
/// truncation, a newer log whose lines are older, a switch back to an
/// older log — never lowered by one.
#[test]
fn a_source_shows_its_newest_line_time_through_a_real_tailer() {
    let tmp = Temp::new("lines");
    let dir = tmp.0.join("Logs");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("WoWCombatLog-110126_200000.txt");
    std::fs::copy(FOREVER_LOG, &first).unwrap();
    let mut tailer = Tailer::new(SourceSpec::Dir(dir.clone()));
    let mut engine = Engine::new();
    // How far the log had reached: its last line, read when its index
    // landed (the scan leaves no line for the tail when nothing is open).
    let last = std::fs::read_to_string(FOREVER_LOG)
        .unwrap()
        .lines()
        .last()
        .and_then(line_utc_ms);
    assert!(last.is_some());
    assert_eq!(
        settle(&mut tailer, &mut engine),
        last,
        "the log opened at start"
    );
    assert!(engine.source_path.is_some());

    append(&first, BUFF);
    assert_eq!(settle(&mut tailer, &mut engine), last, "half a line");
    append(&first, "\n");
    let buff = line_utc_ms(BUFF);
    assert!(buff > last);
    assert_eq!(settle(&mut tailer, &mut engine), buff, "the line completed");

    let text = std::fs::read_to_string(FOREVER_LOG).unwrap();
    let half: String = text.lines().take(4).map(|l| format!("{l}\n")).collect();
    std::fs::write(&first, half).unwrap();
    assert_eq!(
        settle(&mut tailer, &mut engine),
        buff,
        "the same log re-read"
    );
    append(&first, &format!("{BUFF}\n"));
    assert_eq!(
        settle(&mut tailer, &mut engine),
        buff,
        "a line after the re-read"
    );

    let second = dir.join("WoWCombatLog-110126_210000.txt");
    std::fs::copy(FOREVER_LOG, &second).unwrap();
    assert_eq!(
        settle(&mut tailer, &mut engine),
        buff,
        "a newer log whose last line is older"
    );
    assert_eq!(engine.source_path.as_deref(), Some(second.as_path()));

    std::fs::remove_file(&second).unwrap();
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&first)
        .and_then(|f| f.set_modified(hour_ago))
        .unwrap();
    assert_eq!(
        settle(&mut tailer, &mut engine),
        buff,
        "back to an older log"
    );
    assert_eq!(engine.source_path.as_deref(), Some(first.as_path()));

    let empty = tmp.0.join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    let mut tailer = Tailer::new(SourceSpec::Dir(empty.clone()));
    let mut engine = Engine::new();
    assert_eq!(settle(&mut tailer, &mut engine), None, "no log yet");
    assert!(engine.source_path.is_none());
    std::fs::copy(FOREVER_LOG, empty.join("WoWCombatLog-110126_220000.txt")).unwrap();
    assert_eq!(
        settle(&mut tailer, &mut engine),
        last,
        "the folder's first log"
    );
}

/// A line's time for comparing two logs: its date and time less the offset
/// the line writes, so midnight and the year roll over as the clock does and
/// a daylight saving change keeps the order; no time for the legacy format
/// (no year, no offset) or a line with none.
#[test]
fn line_times_compare_across_midnight_the_year_and_daylight_saving() {
    let at = |stamp: &str| {
        line_utc_ms(&format!(
            "{stamp}  {}",
            "ZONE_CHANGE,48,\"Blackfathom Deeps\",1"
        ))
    };
    let ms = |stamp: &str| at(stamp).unwrap();
    assert_eq!(
        ms("11/2/2026 00:00:01.000-8") - ms("11/1/2026 23:59:59.000-8"),
        2_000
    );
    assert_eq!(
        ms("1/1/2027 00:00:00.000-8") - ms("12/31/2026 23:59:59.500-8"),
        500
    );
    // Fall back: 1:10 standard time is after 1:30 daylight time.
    assert_eq!(
        ms("11/1/2026 01:10:00.000-8") - ms("11/1/2026 01:30:00.000-7"),
        40 * 60_000
    );
    assert_eq!(
        ms("11/1/2026 20:00:00.000-8"),
        ms("11/1/2026 23:00:00.000-5"),
        "one moment, two zones"
    );
    assert_eq!(at("11/2 10:00:00.000"), None, "the legacy format");
    assert_eq!(line_utc_ms("no time here"), None);
}
