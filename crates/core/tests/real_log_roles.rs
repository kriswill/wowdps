//! R30 over a real log of an edition whose log carries no spec: every boss
//! fight's inferred roles, printed by class and score, and every closed
//! fight, boss or trash, read twice — the full replay's and a lazy load of
//! the fight alone — and required to agree. A five-player boss fight that is
//! judged names exactly one tank and one healer, as the first Forever log's
//! three do (one warrior tank, one shaman healer, three dealing damage). The
//! trash is summarized by how many of its fights name one of each.
//!
//! A real log carries real names: nothing here prints a name or a guid, a
//! failure included. Players are compared and shown by their place in the
//! fight's roster (sorted by guid, the same order on both sides) and their
//! class; a trash fight by its place in the log, never its name.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_roles -- --ignored --nocapture`

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::Path;

use wowdps_core::index::{SegmentMeta, load_segment_text, scan};
use wowdps_core::meter::{Segment, SegmentKind, View, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};
use wowdps_core::roles::RoleScore;
use wowdps_model::{Class, Edition, Role};

fn class_name(c: Option<Class>) -> &'static str {
    c.map_or("?", Class::name)
}

fn role_name(r: Option<Role>) -> &'static str {
    r.map_or("-", Role::name)
}

/// A fight's scores with every guid replaced by its place in the roster:
/// what a comparison may print.
fn anon(scores: &[RoleScore]) -> Vec<String> {
    scores
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let e = &s.evidence;
            format!(
                "#{i} {} tank {:.6} healer {:.6} damage {} healing {} on-others {} swings {} \
                 casts {} kit {} heals {} form {}",
                role_name(s.role),
                s.tank,
                s.healer,
                e.damage,
                e.healing,
                e.heal_others,
                e.swings,
                e.casts,
                e.kit,
                e.heals,
                e.form_ms
            )
        })
        .collect()
}

/// The fight a lazy load of `meta` holds, read as the full replay's `seg`
/// is: the same roster in the same order and the same scores, shown on a
/// failure by place alone.
fn lazy_equals_full(path: &str, label: &str, seg: &Segment, meta: &SegmentMeta) {
    let lazy = load_segment_text(Path::new(path), meta)
        .expect("load the fight")
        .meter();
    let Some(mine) = lazy
        .segments()
        .iter()
        .find(|s| s.kind == seg.kind && s.start_ms == seg.start_ms)
    else {
        panic!("{label}: the lazy load holds no fight at its start");
    };
    let (full, part) = (seg.role_scores(), mine.role_scores());
    let same_roster = full
        .iter()
        .map(|s| &s.evidence.guid)
        .eq(part.iter().map(|s| &s.evidence.guid));
    assert!(
        same_roster,
        "{label}: the rosters differ ({} players full, {} lazy)",
        full.len(),
        part.len()
    );
    let (a, b) = (anon(&full), anon(&part));
    assert!(
        a == b,
        "{label}: lazy and full differ\nfull: {a:#?}\nlazy: {b:#?}"
    );
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
        lazy_equals_full(&path, &seg.name, seg, meta);

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
    let (mut compared, mut unlisted) = (0, 0);
    let detail = std::env::var_os("WOWDPS_ROLES_TRASH").is_some();
    // A trash fight is labeled by its place in the log, never its name: one
    // fought in the open world can be named after anything.
    let trash = segments.iter().filter(|s| s.kind == SegmentKind::Trash);
    for (n, seg) in trash.enumerate() {
        let scores = seg.role_scores();
        // Every closed one the scan lists reads the same loaded alone (the
        // scan lists no fight R11 finds meaningless).
        if seg.end_ms.is_some() && !scores.is_empty() {
            match idx
                .segments
                .iter()
                .find(|m| m.kind == SegmentKind::Trash && m.start_ms == seg.start_ms)
            {
                Some(meta) => {
                    lazy_equals_full(&path, &format!("trash #{n}"), seg, meta);
                    compared += 1;
                }
                None => unlisted += 1,
            }
        }
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
    println!(
        "lazy = full on {} boss fights and {compared} closed trash fights ({unlisted} the scan \
         does not list)",
        bosses.len()
    );
}

/// One fight's roles, by class and score: never a name or a guid.
fn print_fight(label: &str, seg: &Segment, scores: &[RoleScore]) {
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
