//! Hand-made cards Home's tests build on, in every crate that draws Home:
//! players at a rate, a card, a boss pull, a visit's Σ, a key, a raid
//! roster — and Home fed without a round trip.

use wowdps_model::{Encounter, Role, Spec};
use wowdps_proto::HistoryAnswer;
use wowdps_proto::history::{CardPlayer, FightCard, FightKind, KeyInfo};

use super::{CharLine, Home, parse_ymd};

/// Fold `cards` into `home` without a round trip, for render tests.
pub fn absorb(home: &mut Home, cards: Vec<FightCard>) {
    let total = cards.len() as u32;
    home.pending = Some(0);
    home.absorb(0, &HistoryAnswer::Fights { cards, total });
}

/// Two characters of yours, both Warlocks: "Me" and "Alt".
pub fn chars() -> Vec<CharLine> {
    ["Me", "Alt"]
        .into_iter()
        .map(|g| CharLine {
            guid: g.to_string(),
            name: format!("{g}-Realm-US"),
            class: Some(wowdps_model::Class::Warlock),
            ..CharLine::default()
        })
        .collect()
}

pub const H: i64 = 3_600_000;
pub const DAY: i64 = 86_400_000;
/// Every card here is 100 s long, so a player's damage is 100× their dps.
pub const SECS: i64 = 100;

/// 20:00 on a date, on the log's clock.
pub fn evening(ymd: &str) -> i64 {
    parse_ymd(ymd).unwrap_or_default() + 20 * H
}

/// A player of `spec` at `rate`: effective dps for a damage dealer or a
/// tank, hps for a healer.
pub fn player(guid: &str, spec: Spec, rate: f64) -> CardPlayer {
    let healer = spec.role() == Role::Healer;
    CardPlayer {
        guid: guid.to_string(),
        name: format!("{guid}-Realm-US"),
        class: Some(spec.class()),
        spec: Some(spec),
        damage: if healer {
            0
        } else {
            (rate * SECS as f64) as u64
        },
        dps: if healer { 0.0 } else { rate },
        hps: if healer { rate } else { 0.0 },
        ..CardPlayer::default()
    }
}

/// A card of `kind` owned by `owner`, starting at `at` on the log's clock.
pub fn card(id: &str, kind: FightKind, name: &str, at: i64, owner: &str) -> FightCard {
    FightCard {
        id: id.to_string(),
        log: 7,
        kind,
        name: name.to_string(),
        start_local_ms: at,
        start_utc_ms: at + 7 * H,
        duration_ms: SECS * 1000,
        owner: Some(owner.to_string()),
        ..FightCard::default()
    }
}

/// A raid boss pull: `enc` at `difficulty`, a kill or a wipe.
pub fn boss(
    id: &str,
    name: &str,
    enc: u32,
    at: i64,
    kill: bool,
    players: Vec<CardPlayer>,
) -> FightCard {
    FightCard {
        encounter: Some(Encounter {
            id: enc,
            difficulty: 15,
            group_size: 20,
        }),
        success: Some(kill),
        players,
        ..card(id, FightKind::Encounter, name, at, "Me")
    }
}

/// A raid visit's Σ card: the instance's name, at `difficulty`.
pub fn sigma(id: &str, name: &str, at: i64, difficulty: u32) -> FightCard {
    FightCard {
        key: Some(KeyInfo {
            map_id: 2769,
            difficulty,
            ..KeyInfo::default()
        }),
        ..card(id, FightKind::Overall, name, at, "Me")
    }
}

/// A key run on the key clock `clock` against (par, +2, +3), `who` at
/// their rate over the whole of it.
pub fn key(
    id: &str,
    name: &str,
    at: i64,
    clock: i64,
    success: Option<bool>,
    who: CardPlayer,
) -> FightCard {
    let who = CardPlayer {
        damage: (who.dps * clock as f64 / 1000.0) as u64,
        ..who
    };
    FightCard {
        key: Some(KeyInfo {
            map_id: 2521,
            difficulty: 8,
            level: Some(14),
            completed: success,
        }),
        pars_ms: Some((1_800_000, 1_440_000, 1_080_000)),
        official_ms: Some(clock),
        duration_ms: clock,
        success,
        owner: Some(who.guid.clone()),
        players: vec![who],
        ..card(id, FightKind::Key, name, at, "Me")
    }
}

/// A raid of 19 damage dealers and 4 healers, `me` among the dealers at
/// `rate` (the others at 100..1900).
pub fn raid_of(me: f64) -> Vec<CardPlayer> {
    let mut players = vec![player("Me", Spec::Destruction, me)];
    players.extend((1..=18).map(|i| player(&format!("D{i}"), Spec::Fire, f64::from(i) * 100.0)));
    players.extend((1..=4).map(|i| player(&format!("H{i}"), Spec::HolyPriest, f64::from(i) * 1e4)));
    players
}
