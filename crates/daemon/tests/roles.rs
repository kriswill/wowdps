//! v50 (R30) through the daemon: the role a log with no spec gives each
//! player is on the rows the daemon answers with. A fight read live, the
//! same fight loaded lazily from the log's index and the card the store
//! kept of it name the same roles; a comparison's sides and the raid
//! timeline's deaths wear them as the meter's rows do; and a Retail log's
//! answers wear none, so every Retail row's spec slot is what it was.
//! Numbers are `crates/core/fixtures/roles.expected.md`'s.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{Role, Row, SegmentId, View};
use wowdps_proto::{
    ClientMsg, Cursor, DaemonMsg, EditionRef, FightSort, HistoryAnswer, HistoryQuery, ListEntry,
    OriginFilter, SegmentRef,
};

const ROLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/roles.txt");
const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");

const GARROND: &str = "Player-6101-0C1D2E01";
const MIRELA: &str = "Player-6101-0C1D2E02";
const ASHVEL: &str = "Player-6101-0C1D2E03";
const CORBIN: &str = "Player-6101-0C1D2E04";
const TESKAR: &str = "Player-6101-0C1D2E05";
const VARSK: &str = "Player-6101-0C1D2E0A";

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("wowdps-roles-{tag}-{}-{n}", std::process::id()));
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

fn segment(segment: SegmentRef, view: View) -> Cursor {
    Cursor::Segment {
        segment,
        view,
        top_n: None,
        drill: None,
        death: None,
        spell: None,
        range: None,
    }
}

fn watch(mock: &mut MockDaemon, cursor: Cursor) -> Vec<DaemonMsg> {
    mock.handle(ClientMsg::Watch {
        cursor,
        edition: EditionRef::Follow,
    })
}

/// The frame a Watch is answered with.
fn answer(mock: &mut MockDaemon, cursor: Cursor) -> DaemonMsg {
    let out = watch(mock, cursor);
    out.into_iter()
        .rev()
        .find(|m| {
            matches!(
                m,
                DaemonMsg::Snapshot { .. }
                    | DaemonMsg::SegmentList { .. }
                    | DaemonMsg::CompareSnapshot { .. }
            )
        })
        .expect("a frame")
}

fn list(mock: &mut MockDaemon) -> Vec<ListEntry> {
    match answer(mock, Cursor::List) {
        DaemonMsg::SegmentList { entries, .. } => entries,
        other => panic!("a list, not {other:?}"),
    }
}

/// The roles a set of rows wears, by guid — read as every client reads a
/// row's role (`Row::role`), the inferred field checked to be the one it
/// came from.
fn worn(rows: &[Row]) -> BTreeMap<String, Role> {
    rows.iter()
        .filter_map(|r| {
            assert!(r.spec.is_none(), "Forever's log names no spec");
            assert_eq!(r.role(), r.inferred_role);
            r.role().map(|role| (r.key.clone(), role))
        })
        .collect()
}

/// Every role a segment's answers name, over the views whose rows are
/// players: the same player wears the same role on each.
fn roles_of(mock: &mut MockDaemon, id: SegmentId) -> BTreeMap<String, Role> {
    let mut all = BTreeMap::new();
    for view in [View::Damage, View::Healing, View::Taken, View::Deaths] {
        let DaemonMsg::Snapshot { rows, raid, .. } =
            answer(mock, segment(SegmentRef::Id(id), view))
        else {
            panic!("a snapshot")
        };
        for (guid, role) in worn(&rows) {
            assert_eq!(*all.entry(guid).or_insert(role), role, "{view:?}");
        }
        // The window's mock session is sent the raid timeline: its deaths
        // wear their players' roles.
        for d in raid.expect("a window's raid timeline").deaths {
            assert_eq!(d.role(), d.inferred_role);
            assert_eq!(d.role(), all.get(&d.guid).copied(), "{}", d.guid);
        }
    }
    all
}

fn expected(pairs: &[(&str, Role)]) -> BTreeMap<String, Role> {
    pairs.iter().map(|(g, r)| (g.to_string(), *r)).collect()
}

/// roles.txt read live: a log of its header alone, then every line fed as
/// the tail feeds them, so each fight is the live meter's own.
fn live_mock(tmp: &Temp) -> MockDaemon {
    let text = std::fs::read_to_string(ROLES).unwrap();
    let mut lines = text.lines();
    let path = tmp.0.join("WoWCombatLog-110226_200000.txt");
    std::fs::write(&path, format!("{}\n", lines.next().unwrap())).unwrap();
    let mut mock = MockDaemon::fixture_at(&path);
    mock.feed(lines.map(str::to_string).collect());
    mock
}

/// R30's lazy/full parity, held through the daemon: every segment of
/// roles.txt — the five fights and the visit's Overall — names the same
/// roles read from the live meter as loaded lazily from the log's index.
/// The two bosses name the roles the fixture's goldens give them.
#[test]
fn a_live_and_a_lazily_loaded_segment_name_the_same_roles() {
    let tmp = Temp::new("parity");
    let mut live = live_mock(&tmp);
    let mut lazy = MockDaemon::fixture_at(Path::new(ROLES));
    let (live_list, lazy_list) = (list(&mut live), list(&mut lazy));
    let shape = |l: &[ListEntry]| -> Vec<_> {
        l.iter()
            .map(|e| (e.row.kind, e.row.name.clone(), e.row.start_ms))
            .collect()
    };
    assert_eq!(shape(&live_list), shape(&lazy_list), "one log, one list");
    assert!(live_list.len() >= 6, "five fights and the Overall");
    let mut named = Vec::new();
    for (a, b) in live_list.iter().zip(&lazy_list) {
        let (from_live, from_lazy) = (roles_of(&mut live, a.id), roles_of(&mut lazy, b.id));
        assert_eq!(from_live, from_lazy, "{}", a.row.name);
        named.push((a.row.name.clone(), from_live));
    }
    let of = |name: &str| {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r.clone())
            .unwrap()
    };
    assert_eq!(
        of("Twilight Lord Kelris"),
        expected(&[
            (GARROND, Role::Tank),
            (MIRELA, Role::Healer),
            (ASHVEL, Role::Dps),
            (CORBIN, Role::Dps),
            (TESKAR, Role::Dps),
        ]),
        "Varsk, off the roster, is named nothing"
    );
    assert_eq!(of("Aku'mai").len(), 4, "two tanks, a healer, a dps");
}

/// The store's card of each fight keeps the roles its rows wore live, and
/// a stored fight's rows wear them back (`GetFight`), as live rows do.
#[test]
fn a_stored_fight_wears_the_roles_it_was_stored_with() {
    let tmp = Temp::new("stored");
    let mut live = live_mock(&tmp);
    let live_roles: Vec<(i64, BTreeMap<String, Role>)> = list(&mut live)
        .iter()
        .map(|e| (e.row.start_ms, roles_of(&mut live, e.id)))
        .collect();
    let mut mock = MockDaemon::fixture_at(Path::new(ROLES)).with_history();
    let cards = match mock.history().answer(&HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: None,
        since_utc_ms: None,
        kind: None,
        sort: FightSort::Newest,
        limit: u32::MAX,
        after_id: None,
        role: None,
        origin: OriginFilter::Local,
        edition: None,
    }) {
        HistoryAnswer::Fights { cards, .. } => cards,
        other => panic!("fights, not {other:?}"),
    };
    let bosses: Vec<_> = cards.iter().filter(|c| c.encounter.is_some()).collect();
    assert_eq!(bosses.len(), 2, "both kills stored");
    for card in bosses {
        let kept: BTreeMap<String, Role> = card
            .players
            .iter()
            .filter_map(|p| p.stored_role.map(|r| (p.guid.clone(), r)))
            .collect();
        assert!(!kept.is_empty(), "{}", card.name);
        let out = mock.handle(ClientMsg::GetFight {
            req_id: 1,
            fight_id: card.id.clone(),
            view: View::Damage,
            drill: None,
            death: None,
            boss: None,
            range: None,
            spell: None,
            pair: None,
        });
        let Some(DaemonMsg::Fight {
            fight: Some(fight), ..
        }) = out
            .into_iter()
            .find(|m| matches!(m, DaemonMsg::Fight { .. }))
        else {
            panic!("{} answered", card.name)
        };
        let rows = worn(&fight.rows);
        assert_eq!(
            rows,
            kept.iter()
                .filter(|(g, _)| fight.rows.iter().any(|r| &r.key == *g))
                .map(|(g, r)| (g.clone(), *r))
                .collect::<BTreeMap<_, _>>(),
            "{}: the rows wear the card's roles",
            card.name
        );
        assert!(
            live_roles.iter().any(|(_, r)| r == &kept),
            "{}: the card keeps what the live rows wore",
            card.name
        );
    }
}

/// A comparison's sides are rows of their players: each wears its role.
#[test]
fn a_comparisons_sides_wear_their_roles() {
    let mut mock = MockDaemon::fixture_at(Path::new(ROLES));
    let boss = list(&mut mock)
        .into_iter()
        .find(|e| e.row.name == "Twilight Lord Kelris")
        .unwrap();
    let DaemonMsg::CompareSnapshot { a, b, .. } = answer(
        &mut mock,
        Cursor::Compare {
            segment: SegmentRef::Id(boss.id),
            a: GARROND.to_string(),
            b: VARSK.to_string(),
            view: View::Damage,
            range: None,
            spell: None,
        },
    ) else {
        panic!("a comparison")
    };
    assert_eq!(a.total.role(), Some(Role::Tank));
    assert_eq!(b.total.role(), None, "named nothing");
}

/// A live fight's roles move as it goes: none under R30's 10 s of combat,
/// then settled on what the whole fight says, each push carrying what the
/// fight says so far.
#[test]
fn a_live_fights_roles_arrive_after_ten_seconds() {
    let tmp = Temp::new("moving");
    let text = std::fs::read_to_string(ROLES).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let path = tmp.0.join("WoWCombatLog-110226_200000.txt");
    std::fs::write(&path, format!("{}\n{}\n", lines[0], lines[1])).unwrap();
    let mut mock = MockDaemon::fixture_at(&path);
    watch(&mut mock, segment(SegmentRef::Live, View::Damage));
    let end = lines
        .iter()
        .position(|l| l.contains("ENCOUNTER_END"))
        .unwrap();
    let mut seen = Vec::new();
    for line in &lines[2..=end] {
        let secs: u32 = line[16..18].parse().unwrap();
        for msg in mock.feed(vec![line.to_string()]) {
            if let DaemonMsg::Snapshot { rows, .. } = msg
                && !rows.is_empty()
            {
                seen.push((secs, worn(&rows)));
            }
        }
    }
    // The pull opens at :10; ten seconds of combat is :20.
    assert!(seen.iter().any(|(s, r)| *s < 20 && r.is_empty()));
    assert!(
        seen.iter().all(|(s, r)| *s >= 20 || r.is_empty()),
        "no role under 10 s"
    );
    let (_, last) = seen.last().unwrap();
    assert_eq!(last.get(GARROND), Some(&Role::Tank));
    assert_eq!(last.get(MIRELA), Some(&Role::Healer));
}

/// Retail's role is its spec's: no answer of a Retail log — any segment,
/// any view, the raid timeline's deaths — wears an inferred role, so each
/// row's spec slot carries its specID, or 0, exactly as v49 sent it.
#[test]
fn a_retail_log_wears_no_inferred_role() {
    let mut mock = MockDaemon::fixture_at(Path::new(SAMPLE));
    let entries = list(&mut mock);
    assert!(!entries.is_empty());
    let mut rows_seen = 0;
    for e in &entries {
        for view in View::ALL {
            let DaemonMsg::Snapshot { rows, raid, .. } =
                answer(&mut mock, segment(SegmentRef::Id(e.id), view))
            else {
                panic!("a snapshot")
            };
            rows_seen += rows.len();
            assert!(rows.iter().all(|r| r.inferred_role.is_none()), "{view:?}");
            assert!(
                raid.iter()
                    .flat_map(|r| &r.deaths)
                    .all(|d| d.inferred_role.is_none())
            );
        }
    }
    assert!(rows_seen > 0);
}
