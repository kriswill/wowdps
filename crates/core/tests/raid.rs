//! R25: the raid timeline over every committed fixture — the view's raid
//! series sums to the view's friendly total on every segment and every
//! Overall, the deaths run in the order they happened and each is the
//! recap's own killing blow, the recap's offsets are never after the death
//! and never rise, the lust family unions into windows, a rez is named —
//! and lazy = full = checkpoint-resume parity for all of it. Every fixture
//! in `FIXTURES` must exist — a missing one fails, never skips.

use std::path::Path;

use wowdps_core::index::{load_segment_text, scan, scan_from};
use wowdps_core::meter::{Meter, Segment, View, meter_from_lines};
use wowdps_model::RaidTimeline;

const FIXTURES: &[&str] = &[
    "sample.txt",
    "instance.txt",
    "arena.txt",
    "relog.txt",
    "taken.txt",
    "support.txt",
    "spans.txt",
    "stacks.txt",
    "shields.txt",
];

fn fixture_path(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Every fixture, as (name, text). A missing or unreadable one fails.
fn fixtures() -> Vec<(&'static str, String)> {
    FIXTURES
        .iter()
        .map(|name| {
            let text = std::fs::read_to_string(fixture_path(name));
            assert!(
                text.is_ok(),
                "{name}: unreadable fixture: {:?}",
                text.as_ref().err()
            );
            (*name, text.unwrap_or_default())
        })
        .collect()
}

/// Every segment of a replay, then every visit's Overall, named.
fn every_segment(meter: &Meter) -> Vec<(String, Segment)> {
    let mut out: Vec<(String, Segment)> = meter
        .segments()
        .iter()
        .map(|s| (s.name.clone(), s.clone()))
        .collect();
    for ord in 0..meter.visits().len() as u32 {
        if let Some(o) = meter.overall(ord) {
            out.push((format!("Σ {}", o.name), o));
        }
    }
    out
}

/// Every view a snapshot can be on — each carries a raid timeline.
const VIEWS: [View; 8] = View::ALL;

/// Σ series = Σ the friendly rows' amounts of the series' own view, on
/// every view, every segment and every Overall of every fixture — the
/// identity R25 promises a reader who sums the ribbon.
#[test]
fn the_raid_series_sums_to_the_views_friendly_total() {
    let mut checked = 0;
    for (name, text) in fixtures() {
        let meter = meter_from_lines(text.lines());
        for (seg_name, seg) in every_segment(&meter) {
            for view in VIEWS {
                let raid = seg.raid_timeline(view);
                assert_eq!(
                    raid.view,
                    view.raid_series(),
                    "{name} / {seg_name} / {view:?}"
                );
                assert_eq!(raid.bucket_ms, 1000);
                let total: u64 = seg
                    .rows(raid.view)
                    .iter()
                    .filter(|r| !r.enemy)
                    .map(|r| r.amount)
                    .sum();
                let sum: u64 = raid.series.iter().sum();
                assert_eq!(sum, total, "{name} / {seg_name} / {view:?}");
                checked += usize::from(total > 0);
            }
        }
    }
    assert!(checked > 0, "some series carried an amount");
}

/// The three series are the three views': Healing reads healing, Taken and
/// Deaths what was taken, the rest damage.
#[test]
fn each_view_carries_its_own_series() {
    assert_eq!(View::Damage.raid_series(), View::Damage);
    assert_eq!(View::Healing.raid_series(), View::Healing);
    assert_eq!(View::Taken.raid_series(), View::Taken);
    assert_eq!(View::Deaths.raid_series(), View::Taken);
    for v in [
        View::Interrupts,
        View::CrowdControl,
        View::Dispels,
        View::EnemyTaken,
    ] {
        assert_eq!(v.raid_series(), View::Damage, "{v:?}");
    }
}

/// The deaths run in the order they happened; each player's are their
/// death windows, index for index; and each death's killing blow is its
/// recap's newest damage — the row the Deaths drill leads with.
#[test]
fn deaths_are_in_time_order_and_are_the_recaps_own() {
    let mut seen = 0;
    for (name, text) in fixtures() {
        let meter = meter_from_lines(text.lines());
        for (seg_name, seg) in every_segment(&meter) {
            let raid = seg.raid_timeline(View::Deaths);
            let at: Vec<i64> = raid.deaths.iter().map(|d| d.at_ms).collect();
            assert!(
                at.windows(2).all(|w| w[0] <= w[1]),
                "{name} / {seg_name}: {at:?}"
            );
            // As many as the Deaths rows count, less what the cap dropped.
            let rows = seg.rows(View::Deaths);
            let kept: u64 = rows
                .iter()
                .map(|r| r.amount - u64::from(seg.deaths_dropped(&r.key)))
                .sum();
            assert_eq!(raid.deaths.len() as u64, kept, "{name} / {seg_name}");
            for d in &raid.deaths {
                seen += 1;
                let windows = seg.death_windows(&d.guid);
                let w = windows.iter().find(|(i, _)| *i == d.index);
                assert_eq!(
                    w.map(|(_, ts)| (ts - seg.start_ms).max(0)),
                    Some(d.at_ms),
                    "{name} / {seg_name}: {} #{}",
                    d.name,
                    d.index
                );
                let row = rows.iter().find(|r| r.key == d.guid);
                assert_eq!(row.map(|r| r.label.as_str()), Some(d.name.as_str()));
                assert_eq!(row.and_then(|r| r.class), d.class);
                let (events, _) = seg.breakdown_at(&d.guid, View::Deaths, Some(d.index));
                match events.iter().find(|e| !e.gain) {
                    Some(blow) => {
                        assert!(
                            blow.label.starts_with(&d.blow) && !d.blow.is_empty(),
                            "{name}: {} vs {}",
                            blow.label,
                            d.blow
                        );
                        assert_eq!(blow.amount, d.hit);
                        assert_eq!((blow.extra > 0).then_some(blow.extra), d.overkill);
                        if !d.source.is_empty() {
                            assert_eq!(blow.label, format!("{} ({})", d.blow, d.source));
                        }
                    }
                    None => assert!(d.blow.is_empty() && d.hit == 0 && d.overkill.is_none()),
                }
                assert!(!d.mine, "the meter never marks: the daemon does");
            }
        }
    }
    assert!(seen > 0, "the fixtures hold deaths");
}

/// A recap entry's offset is the time before ITS death: every entry has
/// one, none is after the death, and newest first they never rise — the
/// time column reads down the page without a jump back.
#[test]
fn recap_offsets_are_before_the_death_and_never_rise() {
    let mut seen = 0;
    for (name, text) in fixtures() {
        let meter = meter_from_lines(text.lines());
        for (seg_name, seg) in every_segment(&meter) {
            for d in seg.raid_timeline(View::Deaths).deaths {
                let (events, attackers) = seg.breakdown_at(&d.guid, View::Deaths, Some(d.index));
                let offsets: Vec<i64> = events.iter().filter_map(|e| e.offset_ms).collect();
                assert_eq!(
                    offsets.len(),
                    events.len(),
                    "{name} / {seg_name}: every entry"
                );
                assert!(offsets.iter().all(|o| *o <= 0), "{offsets:?}");
                assert!(offsets.windows(2).all(|w| w[0] >= w[1]), "{offsets:?}");
                assert!(attackers.iter().all(|r| r.offset_ms.is_none()));
                seen += offsets.len();
            }
            // Only a recap row has a time before a death.
            for view in VIEWS.into_iter().filter(|v| *v != View::Deaths) {
                assert!(
                    seg.rows(view)
                        .iter()
                        .all(|r| r.offset_ms.is_none() && !r.mine)
                );
            }
        }
    }
    assert!(seen > 0, "the fixtures hold recaps");
}

/// R13 + R25: an arena's hostile team dies too — Yel falls at 0:25 of the
/// Ashamane's Fall win — and the raid timeline keeps that death flagged
/// `enemy`, as its meter row is: the group's own deaths are none, so a
/// reader counting them must never read the other team's as ours.
#[test]
fn an_arena_enemys_death_is_flagged_enemy() {
    let text = std::fs::read_to_string(fixture_path("arena.txt")).unwrap_or_default();
    let meter = meter_from_lines(text.lines());
    let seg = meter
        .segments()
        .iter()
        .find(|s| s.name == "Ashamane's Fall (Skirmish)")
        .expect("the win");
    let deaths = seg.raid_timeline(View::Deaths).deaths;
    let flat: Vec<(&str, i64, bool)> = deaths
        .iter()
        .map(|d| (d.name.as_str(), d.at_ms, d.enemy))
        .collect();
    assert_eq!(flat, [("Yel-Realm", 25_000, true)]);
    assert_eq!(deaths[0].blow, "Fireball");
    assert_eq!(deaths.iter().filter(|d| !d.enemy).count(), 0, "ours: none");
    // Its Deaths row is the enemy team's too.
    let rows = seg.rows(View::Deaths);
    assert!(rows.iter().all(|r| r.enemy), "{rows:?}");
    // Nowhere else in the fixtures is a death an enemy's.
    for (name, text) in fixtures().into_iter().filter(|(n, _)| *n != "arena.txt") {
        let meter = meter_from_lines(text.lines());
        for (seg_name, seg) in every_segment(&meter) {
            let raid = seg.raid_timeline(View::Deaths);
            assert!(raid.deaths.iter().all(|d| !d.enemy), "{name} / {seg_name}");
        }
    }
}

/// The spans fixture's Mage casts Time Warp on the whole group at 1.000
/// and it comes off at 41.000: one window, 40 s, named for it.
#[test]
fn the_spans_fixture_lusts_once() {
    let text = std::fs::read_to_string(fixture_path("spans.txt")).unwrap_or_default();
    let meter = meter_from_lines(text.lines());
    let seg = &meter.segments()[0];
    let lust = seg.raid_timeline(View::Damage).lust;
    assert_eq!(lust.len(), 1, "{lust:?}");
    assert_eq!(lust[0].label, "Time Warp");
    assert_eq!(lust[0].dur_ms, 40_000);
    let warp = seg
        .spans("Player-1168-0A1B2C31")
        .into_iter()
        .find(|m| m.spell_id == 80353);
    assert_eq!(warp.map(|m| m.at_ms), Some(lust[0].at_ms));
}

// ---- synthetic log lines --------------------------------------------------

const A_UNIT: &str = "Player-1168-0A1B2C51,\"Ardent-Nebula-US\",0x511,0x80000000";
const B_UNIT: &str = "Player-1168-0A1B2C52,\"Brisk-Nebula-US\",0x514,0x80000000";
const S_UNIT: &str = "Player-1168-0A1B2C53,\"Shaw-Nebula-US\",0x514,0x80000000";
const A: &str = "Player-1168-0A1B2C51";
const B: &str = "Player-1168-0A1B2C52";
const S: &str = "Player-1168-0A1B2C53";
const BOSS_GUID: &str = "Creature-0-4232-2662-31585-217000-0000AD01";
const BOSS_UNIT: &str = "Creature-0-4232-2662-31585-217000-0000AD01,\"Raid Test Boss\",0xa48,0x80";

/// A line at `ms` after 20:05:00.000.
fn line(ms: i64, body: &str) -> String {
    let total = 20 * 3_600_000 + 5 * 60_000 + ms;
    let (h, rem) = (total / 3_600_000, total % 3_600_000);
    let (m, rem) = (rem / 60_000, rem % 60_000);
    let (s, milli) = (rem / 1000, rem % 1000);
    format!("9/5/2026 {h}:{m:02}:{s:02}.{milli:03}-4  {body}")
}

/// The boss's Crushing Smash on `dst` for `amount`, with `overkill` (-1
/// for a hit that did not kill) and the victim's health after it.
fn smash(ms: i64, dst_unit: &str, dst: &str, amount: u64, overkill: i64, hp: u64) -> String {
    line(
        ms,
        &format!(
            "SPELL_DAMAGE,{BOSS_UNIT},{dst_unit},1234,\"Crushing Smash\",0x1,{dst},0000000000000000,{hp},500000,0,0,0,0,0,0,0,0,0,0,-812.44,2145.87,2287,4.7123,83,{amount},{amount},{overkill},1,0,0,0,nil,nil,nil,ST"
        ),
    )
}

fn hit(ms: i64, src_unit: &str, amount: u64) -> String {
    line(
        ms,
        &format!(
            "SPELL_DAMAGE,{src_unit},{BOSS_UNIT},23922,\"Shield Slam\",0x1,{BOSS_GUID},0000000000000000,276000,296000,0,0,0,0,0,0,0,0,0,0,-812.44,2145.87,2287,4.7123,83,{amount},{amount},-1,1,0,0,0,nil,nil,nil,ST"
        ),
    )
}

fn aura(ms: i64, event: &str, src_unit: &str, dst_unit: &str, id: u32, name: &str) -> String {
    line(
        ms,
        &format!("{event},{src_unit},{dst_unit},{id},\"{name}\",0x1,BUFF"),
    )
}

fn died(ms: i64, unit: &str) -> String {
    line(
        ms,
        &format!("UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,{unit},0"),
    )
}

/// A two-minute pull: S lusts the group (Heroism) at 10 s — B takes it a
/// second late and loses it early — and again at 90 s; A dies at 30 s to
/// a Crushing Smash with 7 000 overkill and S raises him at 45 s; B dies
/// at 60 s and stays down.
fn pull() -> Vec<String> {
    let heroism = |ms: i64, event: &str, dst: &str| aura(ms, event, S_UNIT, dst, 32182, "Heroism");
    vec![
        line(0, "ENCOUNTER_START,3147,\"Raid Test Boss\",16,3,2769"),
        hit(500, A_UNIT, 1_000),
        heroism(10_000, "SPELL_AURA_APPLIED", A_UNIT),
        heroism(10_000, "SPELL_AURA_APPLIED", S_UNIT),
        heroism(11_000, "SPELL_AURA_APPLIED", B_UNIT),
        heroism(25_000, "SPELL_AURA_REMOVED", B_UNIT),
        smash(28_000, A_UNIT, A, 200_000, -1, 300_000),
        smash(30_000, A_UNIT, A, 307_000, 7_000, 0),
        died(30_000, A_UNIT),
        heroism(30_000, "SPELL_AURA_REMOVED", A_UNIT),
        line(
            45_000,
            &format!("SPELL_RESURRECT,{S_UNIT},{A_UNIT},20484,\"Rebirth\",0x8"),
        ),
        heroism(50_000, "SPELL_AURA_REMOVED", S_UNIT),
        smash(60_000, B_UNIT, B, 500_000, 1, 0),
        died(60_000, B_UNIT),
        hit(89_000, A_UNIT, 1_000),
        heroism(90_000, "SPELL_AURA_APPLIED", A_UNIT),
        line(
            120_000,
            "ENCOUNTER_END,3147,\"Raid Test Boss\",16,3,1,120000",
        ),
    ]
}

/// The pull's raid timeline on `view` — its one segment's.
fn raid_of(lines: &[String], view: View) -> RaidTimeline {
    let meter = meter_from_lines(lines.iter().map(String::as_str));
    let raid = meter.segments().first().map(|s| s.raid_timeline(view));
    assert!(raid.is_some(), "the pull is a segment");
    raid.unwrap_or(RaidTimeline {
        view,
        bucket_ms: 0,
        series: Vec::new(),
        deaths: Vec::new(),
        lust: Vec::new(),
    })
}

/// Every player's Heroism is one window from its first application to its
/// last removal — B's late and short one inside it — and the second cast
/// is a second window, open at the kill and closed against it.
#[test]
fn lust_spans_on_the_group_union_into_windows() {
    let lust = raid_of(&pull(), View::Damage).lust;
    let flat: Vec<(i64, i64, &str)> = lust
        .iter()
        .map(|w| (w.at_ms, w.dur_ms, w.label.as_str()))
        .collect();
    assert_eq!(
        flat,
        [(10_000, 40_000, "Heroism"), (90_000, 30_000, "Heroism")]
    );
}

/// A's death names the smash that killed him, its overkill, and the Rebirth
/// that raised him; B's names his and no rez. The recap's time column is
/// the entries' distance from the death.
#[test]
fn a_death_names_its_killing_blow_and_its_rez() {
    let raid = raid_of(&pull(), View::Deaths);
    let flat: Vec<(&str, i64, &str, u64, Option<u64>)> = raid
        .deaths
        .iter()
        .map(|d| (d.guid.as_str(), d.at_ms, d.blow.as_str(), d.hit, d.overkill))
        .collect();
    assert_eq!(
        flat,
        [
            (A, 30_000, "Crushing Smash", 307_000, Some(7_000)),
            (B, 60_000, "Crushing Smash", 500_000, Some(1)),
        ]
    );
    assert_eq!(raid.deaths[0].source, "Raid Test Boss");
    assert_eq!(raid.deaths[0].name, "Ardent-Nebula-US");
    let rez = raid.deaths[0].rez.as_ref();
    assert_eq!(rez.map(|r| r.at_ms), Some(45_000));
    assert_eq!(rez.map(|r| r.by.as_str()), Some(S));
    assert_eq!(rez.map(|r| r.by_name.as_str()), Some("Shaw-Nebula-US"));
    assert!(raid.deaths[0].battle_rezzed());
    assert!(!raid.deaths[1].battle_rezzed(), "nothing raised B");
    assert_eq!(rez.map(|r| r.spell.as_str()), Some("Rebirth"));
    assert_eq!(raid.deaths[1].rez, None);
    // Damage taken is the series: A's two smashes and B's one — less the
    // overkill (R1, 2026-10-02: the 7 000 and the 1 past their last health
    // point), which the recap's killing blow above still names.
    assert_eq!(raid.view, View::Taken);
    assert_eq!(raid.series.iter().sum::<u64>(), 1_007_000 - 7_000 - 1);
    assert_eq!(raid.series.get(30), Some(&300_000));

    let meter = meter_from_lines(pull().iter().map(String::as_str));
    let (events, _) = meter.segments()[0].breakdown(A, View::Deaths);
    let offsets: Vec<Option<i64>> = events.iter().map(|e| e.offset_ms).collect();
    assert_eq!(offsets, [Some(0), Some(-2_000)]);
}

/// Lazy load == full replay == a scan resumed from any checkpoint, for the
/// whole raid timeline on every view, on every segment and every Overall
/// of every fixture — nothing here is ever state the slice cannot rebuild.
#[test]
fn the_raid_timeline_survives_lazy_loading_and_checkpoints() {
    let picture = |seg: &Segment| -> Vec<RaidTimeline> {
        VIEWS.iter().map(|v| seg.raid_timeline(*v)).collect()
    };
    let mut checked = 0;
    for (name, text) in fixtures() {
        let path = fixture_path(name);
        let bytes = text.as_bytes();
        let idx = scan(&mut &bytes[..]);
        let full = meter_from_lines(text.lines());
        let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
        assert_eq!(metas.len(), full.segments().len(), "{name}: segment count");
        for (meta, seg) in metas.iter().zip(full.segments()) {
            let lazy = load_segment_text(Path::new(&path), meta)
                .expect("slice loads")
                .meter();
            assert_eq!(lazy.segments().len(), 1, "{name}: one segment per slice");
            assert_eq!(
                picture(&lazy.segments()[0]),
                picture(seg),
                "{name} / {}",
                meta.name
            );
            checked += 1;
        }
        for meta in &idx.overalls {
            let ordinal = meta.visit.expect("an Overall meta names its visit");
            let lazy = load_segment_text(Path::new(&path), meta)
                .expect("visit loads")
                .meter();
            let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
            let want = full.overall(ordinal).expect("full replay has the visit");
            assert_eq!(picture(&got), picture(&want), "{name} / {}", meta.name);
        }
        let cuts: Vec<usize> = bytes
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .map(|(i, _)| i + 1)
            .collect();
        for cut in cuts {
            let prefix = scan(&mut &bytes[..cut]);
            let state = prefix.checkpoint.clone();
            let off = state.offset as usize;
            let resumed = scan_from(&mut &bytes[off..], state);
            let rmetas: Vec<_> = resumed
                .segments
                .iter()
                .chain(resumed.open.as_ref())
                .collect();
            for (meta, seg) in rmetas.iter().zip(full.segments()) {
                let lazy = load_segment_text(Path::new(&path), meta)
                    .expect("slice loads")
                    .meter();
                assert_eq!(
                    picture(&lazy.segments()[0]),
                    picture(seg),
                    "{name} / {}: resumed at {cut}",
                    meta.name
                );
            }
        }
    }
    assert!(checked > 0);
}
