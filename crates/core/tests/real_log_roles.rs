//! R30 over a real log of an edition whose log carries no spec: every boss
//! fight's inferred roles, printed by class and score (never a name or a
//! guid), each read twice — the full replay's and a lazy load of the fight
//! alone — and required to agree. A five-player boss fight that is judged
//! names exactly one tank and one healer, as the first Forever log's three
//! do (one warrior tank, one shaman healer, three dealing damage). The
//! trash between them is summarized by how many of its fights name one of
//! each.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_roles -- --ignored --nocapture`

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, View, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};
use wowdps_core::roles::RoleScore;
use wowdps_model::{Class, Edition, Role};

fn class_name(c: Option<Class>) -> &'static str {
    c.map_or("?", Class::name)
}

fn role_name(r: Option<Role>) -> &'static str {
    r.map_or("-", Role::name)
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_roles() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    // The header names the edition: a log of one that reads its specs (a
    // Retail log, the other gates' usual) is passed over before it is read.
    let mut head = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(std::fs::File::open(&path).expect("open the log")),
        &mut head,
    )
    .expect("read the header");
    let edition = match parse_line(head.trim_end()).map(|l| l.event) {
        Some(Event::Version { project_id, .. }) => Edition::from_project_id(project_id),
        _ => Edition::Retail,
    };
    if edition.counts_as() != Edition::Forever {
        println!("{edition:?}: this log's edition reads its specs; no role is inferred");
        return;
    }
    let text = std::fs::read_to_string(&path).expect("read the log");
    let meter = meter_from_lines(text.lines());
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);

    let segments = meter.segments();
    let bosses: Vec<_> = segments
        .iter()
        .filter(|s| s.kind == SegmentKind::Encounter && s.end_ms.is_some())
        .collect();
    let metas: Vec<_> = idx
        .segments
        .iter()
        .filter(|m| m.kind == SegmentKind::Encounter && m.end_ms.is_some())
        .collect();
    assert_eq!(
        bosses.len(),
        metas.len(),
        "scan and replay see the same pulls"
    );
    if bosses.iter().all(|s| s.role_scores().is_empty()) {
        println!("no boss fight infers roles: this log's edition reads its specs");
        return;
    }

    for (seg, meta) in bosses.iter().zip(&metas) {
        let scores = seg.role_scores();
        let lazy = load_segment_text(Path::new(&path), meta)
            .expect("load the pull")
            .meter();
        let lazy_seg = lazy
            .segments()
            .iter()
            .find(|s| s.kind == SegmentKind::Encounter && s.start_ms == seg.start_ms)
            .expect("the lazy load holds the pull");
        assert_eq!(scores, lazy_seg.role_scores(), "{}: lazy = full", seg.name);

        print_fight(&seg.name, seg, &scores);
        let enc = seg.encounter.as_ref();
        let count = |r: Role| scores.iter().filter(|s| s.role == Some(r)).count();
        if enc.is_some_and(|e| e.group_size == 5) && scores.iter().any(|s| s.role.is_some()) {
            assert_eq!(
                (count(Role::Tank), count(Role::Healer)),
                (1, 1),
                "{}: a judged five-player boss fight names one tank and one healer",
                seg.name
            );
        }
    }

    // The trash: how many judged fights name one tank and one healer, and
    // how many name none of either.
    let (mut judged, mut one_each, mut no_tank, mut no_healer, mut unjudged) = (0, 0, 0, 0, 0);
    let detail = std::env::var_os("WOWDPS_ROLES_TRASH").is_some();
    // A trash fight is labeled by its place in the log, never its name: one
    // fought in the open world can be named after anything.
    let trash = segments.iter().filter(|s| s.kind == SegmentKind::Trash);
    for (n, seg) in trash.enumerate() {
        let scores = seg.role_scores();
        if scores.len() < 3 {
            continue;
        }
        if detail {
            let place = if seg.visit.is_some() {
                "in an instance"
            } else {
                "outside"
            };
            print_fight(&format!("trash #{n}, {place}"), seg, &scores);
        }
        if scores.iter().all(|s| s.role.is_none()) {
            unjudged += 1;
            continue;
        }
        judged += 1;
        let count = |r: Role| scores.iter().filter(|s| s.role == Some(r)).count();
        let (t, h) = (count(Role::Tank), count(Role::Healer));
        one_each += usize::from(t == 1 && h == 1);
        no_tank += usize::from(t == 0);
        no_healer += usize::from(h == 0);
    }
    println!(
        "\ntrash of three or more players: {judged} judged ({one_each} with one tank and one \
         healer, {no_tank} with no tank, {no_healer} with no healer), {unjudged} too short to judge"
    );
}

/// One fight's roles, by class and score: never a name or a guid.
fn print_fight(label: &str, seg: &wowdps_core::meter::Segment, scores: &[RoleScore]) {
    let classes: std::collections::HashMap<String, Option<Class>> = seg
        .rows(View::Damage)
        .into_iter()
        .chain(seg.rows(View::Healing))
        .map(|r| (r.key, r.class))
        .collect();
    let total_damage: u64 = scores.iter().map(|s| s.evidence.damage).sum();
    let mut sorted: Vec<&RoleScore> = scores.iter().collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.evidence.damage));
    let enc = seg.encounter.as_ref();
    println!(
        "\n{} (encounter {}, size {}, {} s, {})",
        label,
        enc.map_or(0, |e| e.id),
        enc.map_or(0, |e| e.group_size),
        seg.duration_ms(seg.start_ms) / 1000,
        match seg.success {
            Some(true) => "kill",
            Some(false) => "wipe",
            None => "open",
        }
    );
    println!("  class    role    tank  healer  swings kit heals/casts  dmg%  heal_others form_s");
    for s in &sorted {
        let e = &s.evidence;
        println!(
            "  {:<8} {:<6} {:>5.2} {:>6.2} {:>7} {:>3} {:>5}/{:<5} {:>5.1} {:>12} {:>6}",
            class_name(classes.get(&e.guid).copied().flatten()),
            role_name(s.role),
            s.tank,
            s.healer,
            e.swings,
            e.kit,
            e.heals,
            e.casts,
            if total_damage == 0 {
                0.0
            } else {
                100.0 * e.damage as f64 / total_damage as f64
            },
            e.heal_others,
            e.form_ms / 1000,
        );
    }
}
