//! The coach's grading core (roadmap item 1a, step 1): where a player stands
//! among the fight's players of the SAME ROLE, by the measure that role is
//! judged on — DPS by EFFECTIVE damage per second (R19, step 3b: `damage`
//! minus the shares an Augmentation gave them plus the shares they gave
//! others — `dps` bit for bit when nobody supported, so there is no "fight
//! has support" predicate anywhere), healers by healing per second, tanks
//! by nothing yet (damage taken is step 2). Pure and `pub` so the SQL
//! lake's parity gate can hold `role_ranks` to exactly these numbers; the
//! policy (the two floors) lives here rather than in proto because proto is
//! codec and client, and the GUI must not inherit coach rubric constants.

use wowdps_model::Role;
use wowdps_proto::history::{CardPlayer, FightCard};

/// Below this fraction of the OTHER same-role players' median a player is
/// not a data point (dead at the pull, disconnected, AFK): they leave the
/// median, the count and the ranking, and `excluded` says how many did.
/// Judged against the others so one zero can never drag the floor down to
/// itself.
pub const DPS_FLOOR: f64 = 0.10;

/// …and below this fraction of the TOP same-role player regardless, so a
/// false start where most of the raid never swung (others' median near
/// zero) does not keep a 30-DPS row as a data point.
pub const DPS_TOP_FLOOR: f64 = 0.01;

/// What a role is ranked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// Raw damage per second — the legacy `rank_dps` / `dps_*` block only;
    /// inflated for a player an Augmentation buffs.
    Dps,
    Hps,
    /// R19: effective damage per second (`CardPlayer::effective_dps`) — what
    /// the DPS role is graded on since step 3b.
    Effective,
}

impl Measure {
    /// The wire / SQL spelling: `"dps"` | `"hps"` | `"effective_dps"`.
    pub fn name(self) -> &'static str {
        match self {
            Measure::Dps => "dps",
            Measure::Hps => "hps",
            Measure::Effective => "effective_dps",
        }
    }

    /// The role this measure ranks.
    fn role(self) -> Role {
        match self {
            Measure::Dps | Measure::Effective => Role::Dps,
            Measure::Hps => Role::Healer,
        }
    }

    /// The player's value of this measure on a card of `duration_ms` — the
    /// effective measure derives its rate from it the way `finish_rows`
    /// derived `dps`.
    fn of(self, p: &CardPlayer, duration_ms: i64) -> f64 {
        match self {
            Measure::Dps => p.dps,
            Measure::Hps => p.hps,
            Measure::Effective => p.effective_dps(duration_ms),
        }
    }
}

/// One player's standing among the fight's players of their role.
#[derive(Debug, Clone, PartialEq)]
pub struct Grade {
    pub role: Option<Role>,
    /// `Effective` for a DPS-role subject (`Dps` only out of `dps_pool`),
    /// `Hps` for a healer, `None` for a tank (unranked until damage taken
    /// lands) or an unknown spec.
    pub measure: Option<Measure>,
    /// 1-based among the same-role friendly players that pass the floors;
    /// `None` when the subject was excluded or the role has no measure.
    pub rank: Option<usize>,
    /// Players ranked (after the floors); for a tank, the friendly tanks.
    pub count: usize,
    /// Median of the ranked measures.
    pub median: Option<f64>,
    /// Same-role players the floors dropped.
    pub excluded: usize,
    /// The subject's measure as a percentage of ALL friendly players' same
    /// measure — the number a meter shows; `None` when that total is 0. For
    /// `Effective` the total is Σ effective, which is Σ damage unless the
    /// R19 clamp bit (a player received more shares than they dealt).
    pub share: Option<f64>,
}

impl Grade {
    fn empty(role: Option<Role>) -> Self {
        Grade {
            role,
            measure: None,
            rank: None,
            count: 0,
            median: None,
            excluded: 0,
            share: None,
        }
    }
}

/// Grade `guid` on `card` relative to their role; `None` when the guid is
/// not on the card. The DPS role is ALWAYS ranked by `Measure::Effective`:
/// on a fight nobody supported `effective_dps` is `dps` bit for bit, so no
/// number an Augmentation-less card ever gave moves.
pub fn grade(card: &FightCard, guid: &str) -> Option<Grade> {
    let me = card.players.iter().find(|p| p.guid == guid)?;
    Some(match me.role() {
        Some(Role::Dps) => pool(card, me, Measure::Effective),
        Some(Role::Healer) => pool(card, me, Measure::Hps),
        Some(Role::Tank) => Grade {
            count: card
                .players
                .iter()
                .filter(|p| !p.enemy && p.role() == Some(Role::Tank))
                .count(),
            ..Grade::empty(Some(Role::Tank))
        },
        None => Grade::empty(None),
    })
}

/// The pre-step-1 `me` block: `guid` measured against the fight's DPS-role
/// pool by RAW `dps` whatever their own role — what `rank_dps` / `dps_count`
/// / `dps_median` / `dps_excluded` / `dps_share` always said, and the block
/// an Augmentation's buffs inflate. `None` when the guid is not on the card.
pub(crate) fn dps_pool(card: &FightCard, guid: &str) -> Option<Grade> {
    let me = card.players.iter().find(|p| p.guid == guid)?;
    Some(pool(card, me, Measure::Dps))
}

/// Rank `me` among the friendly players of `measure`'s role. The subject
/// is ranked only when they are of that role themselves and pass the
/// floors; count / median / excluded describe the pool regardless.
fn pool(card: &FightCard, me: &CardPlayer, measure: Measure) -> Grade {
    let mut all: Vec<f64> = card
        .players
        .iter()
        .filter(|p| !p.enemy && p.role() == Some(measure.role()))
        .map(|p| measure.of(p, card.duration_ms))
        .collect();
    all.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let top = all.first().copied().unwrap_or(0.0);
    let kept: Vec<f64> = all
        .iter()
        .enumerate()
        .filter(|&(i, &d)| {
            let others: Vec<f64> = all
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(_, &d)| d)
                .collect();
            median_of(&others).is_none_or(|m| d >= m * DPS_FLOOR) && d >= top * DPS_TOP_FLOOR
        })
        .map(|(_, &d)| d)
        .collect();
    let mine = measure.of(me, card.duration_ms);
    // An enemy (R13 arena side) is never ranked among the friendly pool and
    // has no share of the friendly total, whatever their spec.
    let counted = !me.enemy && me.role() == Some(measure.role()) && kept.contains(&mine);
    let rank = counted.then(|| {
        kept.iter()
            .position(|&d| d <= mine)
            .map_or(kept.len(), |i| i + 1)
    });
    let total: f64 = card
        .players
        .iter()
        .filter(|p| !p.enemy)
        .map(|p| measure.of(p, card.duration_ms))
        .sum();
    Grade {
        role: me.role(),
        measure: Some(measure),
        rank,
        count: kept.len(),
        median: median_of(&kept),
        excluded: all.len() - kept.len(),
        share: (!me.enemy && total > 0.0).then(|| mine / total * 100.0),
    }
}

fn median_of(sorted_desc: &[f64]) -> Option<f64> {
    match sorted_desc.len() {
        0 => None,
        n if n % 2 == 1 => sorted_desc.get(n / 2).copied(),
        n => match (sorted_desc.get(n / 2 - 1), sorted_desc.get(n / 2)) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::{Class, Spec};
    use wowdps_proto::history::FightKind;

    fn player(guid: &str, spec: Spec, dps: f64, hps: f64) -> CardPlayer {
        CardPlayer {
            guid: guid.to_string(),
            name: guid.to_string(),
            class: Some(Class::Warrior),
            spec: Some(spec),
            loadout: None,
            logged: true,
            enemy: false,
            damage: dps as u64 * 100,
            dps,
            healing: hps as u64 * 100,
            hps,
            deaths: 0,
            ..CardPlayer::default()
        }
    }

    fn card(players: Vec<CardPlayer>) -> FightCard {
        FightCard {
            schema: wowdps_proto::history::HISTORY_SCHEMA,
            id: "t".to_string(),
            log: 1,
            content: 1,
            kind: FightKind::Encounter,
            name: "Test".to_string(),
            encounter: None,
            key: None,
            start_local_ms: 0,
            tz_min: None,
            start_utc_ms: 0,
            duration_ms: 100_000,
            official_ms: None,
            pars_ms: None,
            success: Some(true),
            aborted: false,
            build: (12, 0, 0),
            project_id: 1,
            log_version: 22,
            owner: None,
            byte_range: None,
            pinned: false,
            best_pct: None,
            players,
            bosses: Vec::new(),
        }
    }

    #[test]
    fn absent_guid_is_none() {
        let c = card(vec![player("a", Spec::Arms, 100.0, 0.0)]);
        assert_eq!(grade(&c, "zzz"), None);
        assert_eq!(dps_pool(&c, "zzz"), None);
    }

    #[test]
    fn tanks_are_counted_but_unranked() {
        let c = card(vec![
            player("t1", Spec::Blood, 500.0, 50.0),
            player("t2", Spec::ProtectionWarrior, 400.0, 40.0),
            player("d", Spec::Arms, 1000.0, 0.0),
            player("h", Spec::Discipline, 100.0, 900.0),
        ]);
        let g = grade(&c, "t1").expect("on card");
        assert_eq!(g.role, Some(Role::Tank));
        assert_eq!(g.measure, None);
        assert_eq!(g.rank, None);
        assert_eq!(g.count, 2);
        assert_eq!(g.median, None);
        assert_eq!(g.excluded, 0);
        assert_eq!(g.share, None);
        // The legacy block for the same tank: the DPS pool, unranked.
        let d = dps_pool(&c, "t1").expect("on card");
        assert_eq!(d.rank, None);
        assert_eq!(d.count, 1);
        assert_eq!(d.median, Some(1000.0));
        assert_eq!(d.share, Some(500.0 / 2000.0 * 100.0));
    }

    #[test]
    fn healers_rank_by_hps_with_the_floors() {
        let c = card(vec![
            player("h1", Spec::Discipline, 100.0, 800.0),
            player("h2", Spec::RestorationShaman, 50.0, 1000.0),
            player("h3", Spec::HolyPaladin, 0.0, 0.0),
            player("d", Spec::Arms, 1000.0, 200.0),
        ]);
        let g = grade(&c, "h1").expect("on card");
        assert_eq!(g.role, Some(Role::Healer));
        assert_eq!(g.measure, Some(Measure::Hps));
        assert_eq!(g.rank, Some(2));
        assert_eq!(g.count, 2);
        assert_eq!(g.median, Some(900.0));
        assert_eq!(g.excluded, 1);
        assert_eq!(g.share, Some(800.0 / 2000.0 * 100.0));
        let top = grade(&c, "h2").expect("on card");
        assert_eq!(top.rank, Some(1));
        let dropped = grade(&c, "h3").expect("on card");
        assert_eq!(dropped.rank, None);
        assert_eq!(dropped.count, 2);
        assert_eq!(dropped.excluded, 1);
        assert_eq!(dropped.share, Some(0.0));
        // A healer's legacy DPS block never ranks them.
        assert_eq!(dps_pool(&c, "h1").and_then(|d| d.rank), None);
    }

    #[test]
    fn dps_floors_match_the_old_grader() {
        // Two live DPS, one dead at the pull (30 of a 1000 top: under both
        // floors) and one at 200 (over 10% of the others' median 515, over
        // 1% of the top): three ranked, one excluded, as before step 1.
        let c = card(vec![
            player("a", Spec::Arms, 1000.0, 0.0),
            player("b", Spec::Marksmanship, 30.0, 0.0),
            player("c", Spec::Fire, 900.0, 0.0),
            player("d", Spec::FrostMage, 200.0, 0.0),
            player("h", Spec::Discipline, 100.0, 900.0),
        ]);
        let g = grade(&c, "d").expect("on card");
        assert_eq!(g.measure, Some(Measure::Effective));
        assert_eq!(g.rank, Some(3));
        assert_eq!(g.count, 3);
        assert_eq!(g.median, Some(900.0));
        assert_eq!(g.excluded, 1);
        assert_eq!(g.share, Some(200.0 / 2230.0 * 100.0));
        assert_eq!(grade(&c, "b").and_then(|g| g.rank), None);
        assert_eq!(grade(&c, "a").and_then(|g| g.rank), Some(1));
        // For a DPS subject on a card nobody supported the generic and legacy
        // blocks are one grade, number for number (only the label differs).
        assert_eq!(
            Grade {
                measure: Some(Measure::Dps),
                ..grade(&c, "d").unwrap()
            },
            dps_pool(&c, "d").unwrap()
        );
        // Enemies never enter a pool.
        let mut c2 = c.clone();
        c2.players.push(CardPlayer {
            enemy: true,
            ..player("e", Spec::Arms, 5000.0, 0.0)
        });
        assert_eq!(grade(&c2, "d"), grade(&c, "d"));
        // …and an enemy is never ranked, even when their number ties a
        // friendly player's (a real arena card carries enemy specs).
        let mut c3 = c.clone();
        c3.players.push(CardPlayer {
            enemy: true,
            ..player("f", Spec::Arms, 1000.0, 0.0)
        });
        let f = grade(&c3, "f").unwrap();
        assert_eq!(f.rank, None);
        assert_eq!(f.share, None);
        assert_eq!(f.count, grade(&c, "d").unwrap().count);
    }

    /// `support.expected.md`'s encounter as a card: the Evoker's 69 500 raw
    /// damage is 85 900 effective (+23 900 given − 7 500 received), the
    /// Mage's 271 000 is 269 350, the Warrior's 242 000 is 227 250.
    fn support_card() -> FightCard {
        let dps = |damage: u64| damage as f64 / 60.0;
        let mut evoker = player("E", Spec::Augmentation, dps(69_500), 0.0);
        evoker.damage = 69_500;
        evoker.support_given = 23_900;
        evoker.support_received = 7_500;
        let mut mage = player("M", Spec::Fire, dps(271_000), 0.0);
        mage.damage = 271_000;
        mage.support_received = 1_650;
        let mut warrior = player("W", Spec::Arms, dps(242_000), 0.0);
        warrior.damage = 242_000;
        warrior.support_received = 14_750;
        let mut priest = player("H", Spec::HolyPriest, 0.0, 88_000.0 / 60.0);
        priest.healing = 88_000;
        let mut c = card(vec![evoker, mage, warrior, priest]);
        c.duration_ms = 60_000;
        c
    }

    #[test]
    fn dps_role_is_ranked_by_effective_dps() {
        let c = support_card();
        let e = grade(&c, "E").expect("on card");
        assert_eq!(e.measure, Some(Measure::Effective));
        // 85 900 / 60 s against the Mage's 269 350 and the Warrior's 227 250.
        assert_eq!(e.rank, Some(3));
        assert_eq!(e.count, 3);
        assert_eq!(e.excluded, 0);
        assert_eq!(e.median, Some(227_250.0 / 60.0));
        // Σ effective = Σ damage (582 500): this card has no clamp case.
        // (Summed in roster order, as `pool` does, so the bits agree.)
        let total = 0.0 + 85_900.0 / 60.0 + 269_350.0 / 60.0 + 227_250.0 / 60.0 + 0.0;
        assert_eq!(e.share, Some((85_900.0 / 60.0) / total * 100.0));
        let m = grade(&c, "M").expect("on card");
        assert_eq!(m.rank, Some(1));
        assert_eq!(m.median, Some(227_250.0 / 60.0));
        assert_eq!(grade(&c, "W").and_then(|g| g.rank), Some(2));
        // The legacy block still ranks RAW dps: the Mage first at 271 000,
        // the Evoker last at 69 500 (over both floors), the median the
        // Warrior's 242 000.
        let legacy = dps_pool(&c, "E").expect("on card");
        assert_eq!(legacy.measure, Some(Measure::Dps));
        assert_eq!(legacy.rank, Some(3));
        assert_eq!(legacy.median, Some(242_000.0 / 60.0));
        let raw_total = 0.0 + 69_500.0 / 60.0 + 271_000.0 / 60.0 + 242_000.0 / 60.0 + 0.0;
        assert_eq!(legacy.share, Some((69_500.0 / 60.0) / raw_total * 100.0));
        assert_eq!(dps_pool(&c, "M").and_then(|g| g.rank), Some(1));
        assert_eq!(dps_pool(&c, "W").and_then(|g| g.rank), Some(2));
        // The two blocks can even disagree on the floors: at 12 000 raw
        // damage (200 dps, under 10% of the others' median 4 275) the Evoker
        // leaves the legacy pool, yet 28 400 effective (473/s, over 10% of
        // the others' effective median 4 138) still ranks among the three.
        let mut c2 = c.clone();
        c2.players[0].damage = 12_000;
        c2.players[0].dps = 200.0;
        assert_eq!(dps_pool(&c2, "E").and_then(|g| g.rank), None);
        assert_eq!(dps_pool(&c2, "E").map(|g| g.excluded), Some(1));
        assert_eq!(grade(&c2, "E").and_then(|g| g.rank), Some(3));
        assert_eq!(grade(&c2, "E").map(|g| g.excluded), Some(0));
    }

    #[test]
    fn without_support_the_generic_grade_is_the_legacy_grade() {
        // Strip the shares: every number `grade()` gives is `dps_pool()`'s,
        // because `effective_dps(d)` is `dps` bit for bit on such a card.
        let mut c = support_card();
        for p in &mut c.players {
            p.support_given = 0;
            p.support_received = 0;
        }
        for guid in ["E", "M", "W"] {
            let g = grade(&c, guid).unwrap();
            assert_eq!(g.measure, Some(Measure::Effective));
            assert_eq!(
                Grade {
                    measure: Some(Measure::Dps),
                    ..g
                },
                dps_pool(&c, guid).unwrap(),
                "{guid}"
            );
        }
        assert_eq!(grade(&c, "M").and_then(|g| g.rank), Some(1));
        assert_eq!(grade(&c, "W").and_then(|g| g.rank), Some(2));
        assert_eq!(grade(&c, "E").and_then(|g| g.rank), Some(3));
        assert_eq!(grade(&c, "H").map(|g| g.measure), Some(Some(Measure::Hps)));
    }

    #[test]
    fn unknown_spec_has_no_role() {
        let mut p = player("x", Spec::Arms, 100.0, 0.0);
        p.spec = None;
        let c = card(vec![p]);
        assert_eq!(grade(&c, "x"), Some(Grade::empty(None)));
    }
}
