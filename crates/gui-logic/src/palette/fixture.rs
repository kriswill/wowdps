//! The palette's test fixtures, shared with the GUIs' gesture tests:
//! tonight's raid and Saturday's keys on the rail, a pull's players, and
//! the characters the window knows you play.

use wowdps_model::{Class, Encounter, Row, Spec};
use wowdps_proto::history::{FightCard, FightKind, KeyInfo};

use super::{Group, Item, items};
use crate::home::{CharLine, parse_ymd};
use crate::rail::{self, Rail};

pub const H: i64 = 3_600_000;

/// 20:00 on a date, on the log's clock.
pub fn evening(ymd: &str) -> i64 {
    parse_ymd(ymd).unwrap_or_default() + 20 * H
}

pub fn card(id: &str, kind: FightKind, name: &str, at: i64) -> FightCard {
    FightCard {
        id: id.to_string(),
        log: 7,
        kind,
        name: name.to_string(),
        start_local_ms: at,
        start_utc_ms: at + 7 * H,
        duration_ms: 300_000,
        ..FightCard::default()
    }
}

pub fn boss(id: &str, name: &str, at: i64, kill: bool, pct: Option<u16>) -> FightCard {
    FightCard {
        encounter: Some(Encounter {
            id: 3492,
            difficulty: 15,
            group_size: 25,
        }),
        success: Some(kill),
        best_pct: pct,
        ..card(id, FightKind::Encounter, name, at)
    }
}

pub fn keyed(id: &str, name: &str, at: i64, clock: i64, timed: bool) -> FightCard {
    FightCard {
        key: Some(KeyInfo {
            map_id: 2521,
            difficulty: 8,
            level: Some(14),
            completed: Some(timed),
        }),
        pars_ms: Some((1_800_000, 1_440_000, 1_080_000)),
        official_ms: Some(clock),
        duration_ms: clock,
        success: Some(timed),
        ..card(id, FightKind::Key, name, at)
    }
}

/// Tonight's raid — a kill, a wipe at 56 %, another kill, trash between —
/// and Saturday's two keys, one over; `extra` more cards on top.
pub fn rail_of(extra: Vec<FightCard>) -> Rail {
    let sun = evening("2026-09-27");
    let sat = evening("2026-09-26");
    let mut cards = vec![
        boss("u2", "Ula'tek", sun + 90 * 60_000, true, None),
        boss("u1", "Ula'tek", sun + 60 * 60_000, false, Some(56)),
        card("t1", FightKind::Trash, "Trash", sun + 30 * 60_000),
        boss("c", "The Coiled Altar", sun + 10 * 60_000, true, Some(0)),
        keyed("k2", "Kings' Rest +14", sat + H, 1_900_000, false),
        keyed("k1", "Kings' Rest +14", sat, 1_700_000, true),
    ];
    cards.extend(extra);
    cards.sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
    rail::Rail::build(&rail::Sources {
        entries: &[],
        log_id: None,
        watched: None,
        cards: &cards,
        owner: None,
        tonight: rail::night_of(sun),
    })
}

pub fn players() -> Vec<Row> {
    let row = |key: &str, label: &str, spec: Spec, enemy: bool| Row {
        key: key.to_string(),
        label: label.to_string(),
        class: Some(spec.class()),
        spec: Some(spec),
        enemy,
        ..Row::default()
    };
    vec![
        row("Player-1", "Swampert-Proudmoore-US", Spec::Arms, false),
        row("Player-2", "Akanôs-Proudmoore-US", Spec::Devourer, false),
        row("Zul'jan", "Zul'jan", Spec::Fire, true),
    ]
}

/// The characters the window knows you play: one the store resolved, and
/// one only the config names (no guid yet), which Home cannot be scoped to.
pub fn chars() -> Vec<CharLine> {
    vec![
        CharLine {
            guid: "Player-ME".to_string(),
            name: "Tranqlock-Proudmoore-US".to_string(),
            class: Some(Class::Warlock),
            spec: Some(Spec::Destruction),
            ..CharLine::default()
        },
        CharLine {
            name: "Unresolved-Realm-US".to_string(),
            ..CharLine::default()
        },
    ]
}

pub fn all() -> Vec<Item> {
    items(&rail_of(Vec::new()), &players(), &chars(), true)
}

pub fn titles(items: &[Item], group: Group) -> Vec<String> {
    items
        .iter()
        .filter(|i| i.group == group)
        .map(|i| i.title.clone())
        .collect()
}
