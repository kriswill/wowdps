//! R30: the role a player played in one fight, inferred from what they did,
//! for an edition whose log carries no spec (CONTRACT.md R30). Retail's role
//! is its spec's and never comes from here: only an edition with a
//! role-signal table (`tables.rs`) infers one.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use wowdps_model::Role;

/// What a spell says about the player who casts it (or, for a form, the
/// player under it): an id in an edition's curated role-signal table
/// (`forever/role_signals.rs`, from `tools/gen-role-signals.sh`). The
/// generator proves each kind against the client's own tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleSignal {
    /// A cast that makes an enemy attack the caster (the client's
    /// ATTACK_ME effect or MOD_TAUNT aura).
    Taunt,
    /// A tank's mitigation or threat cast that needs no shield (Revenge,
    /// Last Stand, Frenzied Regeneration, a bear's Maul).
    Mitigation,
    /// A cast the client allows only with a shield equipped (Shield Block,
    /// Shield Slam). The log shows no gear without COMBATANT_INFO, so these
    /// casts are the only sight of a shield.
    Shield,
    /// A tanking form or stance: the aura on the player (MOD_SHAPESHIFT to
    /// Bear Form, Defensive Stance).
    Form,
    /// A healing cast (the client's HEAL effect or PERIODIC_HEAL aura).
    Heal,
}

impl RoleSignal {
    /// Every kind, in code order: the generated table's codes index it.
    pub const ALL: [RoleSignal; 5] = [
        RoleSignal::Taunt,
        RoleSignal::Mitigation,
        RoleSignal::Shield,
        RoleSignal::Form,
        RoleSignal::Heal,
    ];

    /// The kind's code in the generated table: its place in [`Self::ALL`].
    pub fn code(self) -> u8 {
        match self {
            RoleSignal::Taunt => 0,
            RoleSignal::Mitigation => 1,
            RoleSignal::Shield => 2,
            RoleSignal::Form => 3,
            RoleSignal::Heal => 4,
        }
    }

    /// The inverse of [`Self::code`]; any other code is `None`.
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    /// The kind as the review twin and the tests spell it.
    pub fn name(self) -> &'static str {
        match self {
            RoleSignal::Taunt => "taunt",
            RoleSignal::Mitigation => "mitigation",
            RoleSignal::Shield => "shield",
            RoleSignal::Form => "form",
            RoleSignal::Heal => "heal",
        }
    }
}

// ---- the weights ------------------------------------------------------------
//
// Hand-set, not fitted: one real log of the first edition exists. A score is
// a weighted sum of signals each in [0, 1], so each side's weights sum to 1
// and a score reads as a share of the evidence a textbook tank or healer
// shows. Every signal that compares the player with the group is relative
// to the group's own fight, never to a fixed amount.

/// A fight with less combat than this names no role: a pull this short holds
/// too few lines to weigh, and "unknown" beats a wrong guess.
pub const MIN_COMBAT_MS: i64 = 10_000;
/// Fewer enemy melee swings at the group than this, and who took them says
/// nothing: the swing signal reads 0 for everyone.
pub const MIN_SWINGS: u64 = 5;
/// This many casts of the tank kit (taunts, mitigation, shield casts) is the
/// whole kit signal; fewer is a part of it.
pub const KIT_FULL: u64 = 3;

/// Tank: the player's share of the enemies' melee swings at the group, over
/// the top taker's (so two tanks splitting a boss both read high). The
/// strongest sign, but not enough alone: whoever an enemy hits in a group
/// with no tank leads it too.
pub const TANK_SWINGS: f64 = 0.40;
/// Tank: casts of the kit only a tank presses (a taunt, Revenge, a cast that
/// needs a shield), saturating at `KIT_FULL`.
pub const TANK_KIT: f64 = 0.30;
/// Tank: the share of the fight spent under a tanking form or stance. A
/// stance taken before the pull writes no line inside it, so this is
/// support, never required.
pub const TANK_FORM: f64 = 0.20;
/// Tank: dealing less than a fair share of the group's damage. Weak: a
/// dungeon tank deals near a fair share.
pub const TANK_LOW_DAMAGE: f64 = 0.10;

/// Healer: healing landed on OTHER players, over the group's top healer's
/// (two raid healers both read high). Not enough alone: in a group with no
/// healer, whoever heals most leads it.
pub const HEAL_SHARE: f64 = 0.40;
/// Healer: healing casts as a share of all the player's casts. A heal's
/// cast at an enemy (Holy Shock's or Penance's damage) is no healing cast.
pub const HEAL_CASTS: f64 = 0.35;
/// Healer: dealing less than a fair share of the group's damage. Strong for
/// a healer, who deals far less than the rest most of the time.
pub const HEAL_LOW_DAMAGE: f64 = 0.25;

/// A player whose tank or healer score reaches this plays that role (the
/// higher, a tie the tank's); everyone else who acted deals damage.
pub const THRESHOLD: f64 = 0.5;

/// What one fight's lines say about one friendly player: what the solver
/// weighs, read from the segment (`Segment::role_scores`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleEvidence {
    pub guid: String,
    /// The Damage row (R1, pets folded).
    pub damage: u64,
    /// The Healing row (R2/R3): with `damage` and `casts`, whether the
    /// player did anything at all.
    pub healing: u64,
    /// Effective healing and absorbs landed on another player (or their
    /// pet): never their own, nor their own pet's.
    pub heal_others: u64,
    /// Melee swings an enemy aimed at them, landed or not.
    pub swings: u64,
    /// Every cast that went off (`SPELL_CAST_SUCCESS`), of the player's own.
    pub casts: u64,
    /// Casts of a taunt, a mitigation cast or a cast that needs a shield.
    pub kit: u64,
    /// Healing casts: at a friendly unit, at no unit or at themselves.
    pub heals: u64,
    /// Time under a tanking form or stance, inside the fight.
    pub form_ms: i64,
}

/// One player's scores and the role they give.
#[derive(Debug, Clone, PartialEq)]
pub struct RoleScore {
    pub evidence: RoleEvidence,
    pub tank: f64,
    pub healer: f64,
    /// `None` when the fight is too short to judge or the player did
    /// nothing of their own (no damage, no healing, no cast: a player who
    /// only died).
    pub role: Option<Role>,
}

/// R30's solver: every player's tank and healer scores over one fight, and
/// the role they give. `combat_ms` is the fight's combat clock (R7) and
/// `span_ms` the wall time its lines cover (a form's share is of that).
/// Pure and order-free: integer sums before any division, so the same
/// evidence in any order gives the same scores and roles.
pub fn solve(players: &[RoleEvidence], combat_ms: i64, span_ms: i64) -> Vec<RoleScore> {
    let acted = |p: &RoleEvidence| p.damage > 0 || p.healing > 0 || p.casts > 0;
    let group = players.iter().filter(|p| acted(p)).count() as u64;
    let total_damage: u64 = players.iter().filter(|p| acted(p)).map(|p| p.damage).sum();
    let total_swings: u64 = players.iter().map(|p| p.swings).sum();
    let top_swings = players.iter().map(|p| p.swings).max().unwrap_or(0);
    let top_heal = players.iter().map(|p| p.heal_others).max().unwrap_or(0);
    let judged = combat_ms >= MIN_COMBAT_MS;
    let ratio = |a: u64, b: u64| if b == 0 { 0.0 } else { a as f64 / b as f64 };
    players
        .iter()
        .map(|p| {
            let swings = if total_swings >= MIN_SWINGS {
                ratio(p.swings, top_swings)
            } else {
                0.0
            };
            let kit = ratio(p.kit.min(KIT_FULL), KIT_FULL);
            let form = if span_ms > 0 {
                (p.form_ms as f64 / span_ms as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // How far under a fair share (1 / the group) their damage is:
            // 1 for none, 0 at a fair share or above.
            let low_damage = if total_damage == 0 {
                1.0
            } else {
                (1.0 - ratio(p.damage.saturating_mul(group), total_damage)).clamp(0.0, 1.0)
            };
            let heal_share = ratio(p.heal_others, top_heal);
            let heal_casts = ratio(p.heals, p.casts);
            let tank = TANK_SWINGS * swings
                + TANK_KIT * kit
                + TANK_FORM * form
                + TANK_LOW_DAMAGE * low_damage;
            let healer =
                HEAL_SHARE * heal_share + HEAL_CASTS * heal_casts + HEAL_LOW_DAMAGE * low_damage;
            let named = if tank >= THRESHOLD && tank >= healer {
                Role::Tank
            } else if healer >= THRESHOLD {
                Role::Healer
            } else {
                Role::Dps
            };
            let role = (judged && acted(p)).then_some(named);
            RoleScore {
                evidence: p.clone(),
                tank,
                healer,
                role,
            }
        })
        .collect()
}

// ---- the ledger ---------------------------------------------------------------

/// One player's casts by what they signal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Casts {
    all: u64,
    kit: u64,
    heals: u64,
}

/// One player's tanking-form time: what closed, what is open (aura id →
/// since), and every aura that has been on in the fight, by an apply or by
/// the segment-start rule (which fires only for an aura not here).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Forms {
    closed_ms: i64,
    open: BTreeMap<u32, i64>,
    seen: BTreeSet<u32>,
}

/// R30's per-fight ledger: what the role solver reads, kept in the segment
/// beside the meter's own and written only for an edition with a role-signal
/// table. Keys are raw guids, folded onto owners when read, like every
/// per-actor ledger; nothing in it is carried into the next segment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RoleLedger {
    /// Per raw player guid: their own casts.
    casts: HashMap<String, Casts>,
    /// Per raw player guid: enemy melee swings at them.
    swings: HashMap<String, u64>,
    /// Per raw healer, per raw target: effective healing and absorbs.
    healing: HashMap<String, HashMap<String, u64>>,
    /// Per raw player guid: time under a tanking form or stance.
    forms: HashMap<String, Forms>,
    /// An Overall's: the wall time its members' ledgers covered.
    merged_span_ms: i64,
}

/// `f` on the value `key` holds in `map`, a default one made first when it
/// has none: a hot path's lookup allocates no key it already has.
fn with<V: Default>(map: &mut HashMap<String, V>, key: &str, f: impl FnOnce(&mut V)) {
    if let Some(v) = map.get_mut(key) {
        f(v);
    } else {
        let mut v = V::default();
        f(&mut v);
        map.insert(key.to_string(), v);
    }
}

impl RoleLedger {
    /// A cast that went off, by a player, with what its spell signals.
    pub(crate) fn cast(&mut self, player: &str, signal: Option<RoleSignal>) {
        with(&mut self.casts, player, |c| {
            c.all += 1;
            match signal {
                Some(RoleSignal::Taunt | RoleSignal::Mitigation | RoleSignal::Shield) => {
                    c.kit += 1;
                }
                Some(RoleSignal::Heal) => c.heals += 1,
                Some(RoleSignal::Form) | None => {}
            }
        });
    }

    /// An enemy's melee swing at a player.
    pub(crate) fn swing(&mut self, player: &str) {
        with(&mut self.swings, player, |n| *n += 1);
    }

    /// Effective healing (or an absorb) from `healer` landing on `target`.
    pub(crate) fn heal(&mut self, healer: &str, target: &str, amount: u64) {
        if amount > 0 {
            with(&mut self.healing, healer, |targets| {
                with(targets, target, |n| *n += amount);
            });
        }
    }

    /// A tanking form's aura on `player` at `ts`; one already open stays.
    pub(crate) fn form_on(&mut self, player: &str, aura: u32, ts: i64) {
        with(&mut self.forms, player, |f| {
            f.open.entry(aura).or_insert(ts);
            f.seen.insert(aura);
        });
    }

    /// The aura off at `ts`. With none open, and none on before in this
    /// fight, it was on when the fight began: from `start_ms` (R18's
    /// segment-start rule). A removal after the aura's own came off is a
    /// repeat and adds nothing.
    pub(crate) fn form_off(&mut self, player: &str, aura: u32, ts: i64, start_ms: i64) {
        with(&mut self.forms, player, |f| match f.open.remove(&aura) {
            Some(since) => f.closed_ms += (ts - since).max(0),
            None if f.seen.insert(aura) => f.closed_ms += (ts - start_ms).max(0),
            None => {}
        });
    }

    /// A player's form time, the open spans closed at `close_ms`.
    fn form_ms(&self, player: &str, close_ms: i64) -> i64 {
        self.forms.get(player).map_or(0, |f| {
            f.closed_ms
                + f.open
                    .values()
                    .map(|since| (close_ms - since).max(0))
                    .sum::<i64>()
        })
    }

    /// R10: an Overall takes a member's ledger — counts sum, and the
    /// member's form time and span close on the member's own clock.
    pub(crate) fn absorb(&mut self, other: &RoleLedger, close_ms: i64, span_ms: i64) {
        for (k, c) in &other.casts {
            let mine = self.casts.entry(k.clone()).or_default();
            mine.all += c.all;
            mine.kit += c.kit;
            mine.heals += c.heals;
        }
        for (k, n) in &other.swings {
            *self.swings.entry(k.clone()).or_default() += n;
        }
        for (healer, targets) in &other.healing {
            with(&mut self.healing, healer, |mine| {
                for (target, n) in targets {
                    with(mine, target, |m| *m += n);
                }
            });
        }
        for k in other.forms.keys() {
            self.forms.entry(k.clone()).or_default().closed_ms += other.form_ms(k, close_ms);
        }
        self.merged_span_ms += if other.merged_span_ms > 0 {
            other.merged_span_ms
        } else {
            span_ms
        };
    }

    /// An Overall's span: what its members covered.
    pub(crate) fn merged_span_ms(&self) -> i64 {
        self.merged_span_ms
    }

    /// The raw healers the ledger holds, sorted.
    #[cfg(test)]
    pub(crate) fn healers(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.healing.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    /// The evidence for each of `players` (folded owner guids, with their
    /// Damage and Healing rows), `fold` resolving a raw guid to its owner.
    pub(crate) fn evidence<'a>(
        &self,
        players: impl IntoIterator<Item = (&'a str, u64, u64)>,
        fold: impl Fn(&str) -> String,
        close_ms: i64,
    ) -> Vec<RoleEvidence> {
        // Healing to others, folded on both sides; a heal on oneself or
        // one's own pet is no one else's.
        let mut heal_others: HashMap<String, u64> = HashMap::new();
        for (healer, targets) in &self.healing {
            let h = fold(healer);
            for (target, n) in targets {
                let t = fold(target);
                if h != t && t.starts_with("Player-") {
                    *heal_others.entry(h.clone()).or_default() += n;
                }
            }
        }
        players
            .into_iter()
            .map(|(guid, damage, healing)| {
                let c = self.casts.get(guid).copied().unwrap_or_default();
                RoleEvidence {
                    guid: guid.to_string(),
                    damage,
                    healing,
                    heal_others: heal_others.get(guid).copied().unwrap_or(0),
                    swings: self.swings.get(guid).copied().unwrap_or(0),
                    casts: c.all,
                    kit: c.kit,
                    heals: c.heals,
                    form_ms: self.form_ms(guid, close_ms),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(guid: &str) -> RoleEvidence {
        RoleEvidence {
            guid: guid.to_string(),
            ..RoleEvidence::default()
        }
    }

    /// The first log's shape: a warrior taking the enemies' swings with a
    /// shield and taunts, a shaman healing, three dealing damage.
    fn dungeon() -> Vec<RoleEvidence> {
        let dps = |g: &str, damage: u64, casts: u64| RoleEvidence {
            damage,
            casts,
            swings: 2,
            ..p(g)
        };
        vec![
            RoleEvidence {
                damage: 9_000,
                casts: 60,
                kit: 40,
                swings: 88,
                ..p("Player-1-tank")
            },
            RoleEvidence {
                damage: 300,
                healing: 12_000,
                heal_others: 11_500,
                casts: 40,
                heals: 36,
                ..p("Player-1-heal")
            },
            dps("Player-1-a", 14_000, 70),
            dps("Player-1-b", 12_000, 80),
            RoleEvidence {
                heal_others: 71,
                healing: 71,
                heals: 1,
                ..dps("Player-1-c", 13_000, 90)
            },
        ]
    }

    fn roles(scores: &[RoleScore]) -> Vec<Option<Role>> {
        scores.iter().map(|s| s.role).collect()
    }

    #[test]
    fn the_first_logs_group_reads_one_tank_one_healer_three_dps() {
        let s = solve(&dungeon(), 60_000, 60_000);
        assert_eq!(
            roles(&s),
            [
                Some(Role::Tank),
                Some(Role::Healer),
                Some(Role::Dps),
                Some(Role::Dps),
                Some(Role::Dps)
            ]
        );
        // Nothing near a line.
        assert!(s[0].tank > 0.7 && s[1].healer > 0.8, "{s:?}");
        assert!(
            s[2..].iter().all(|x| x.tank < 0.3 && x.healer < 0.3),
            "{s:?}"
        );
    }

    #[test]
    fn order_does_not_move_a_score() {
        let mut d = dungeon();
        let a = solve(&d, 60_000, 60_000);
        d.reverse();
        let mut b = solve(&d, 60_000, 60_000);
        b.reverse();
        assert_eq!(a, b);
    }

    /// Swings alone, or the heaviest healing alone, never make the role: a
    /// group with no tank and no healer deals damage, all of it.
    #[test]
    fn a_group_with_no_tank_and_no_healer_deals_damage() {
        let mut d = dungeon();
        d.remove(1);
        d[0].kit = 0;
        let s = solve(&d, 60_000, 60_000);
        assert!(roles(&s).iter().all(|r| *r == Some(Role::Dps)), "{s:?}");
        assert!(s[0].tank < THRESHOLD && s[3].healer < THRESHOLD, "{s:?}");
    }

    /// Two tanks splitting a boss's swings both read as tanks, and two
    /// healers both as healers.
    #[test]
    fn two_tanks_and_two_healers_in_a_raid() {
        let mut d = dungeon();
        d[0].swings = 50;
        let mut off = d[0].clone();
        off.guid = "Player-1-tank2".into();
        off.swings = 38;
        off.kit = 12;
        let mut heal2 = d[1].clone();
        heal2.guid = "Player-1-heal2".into();
        heal2.heal_others = 7_000;
        d.push(off);
        d.push(heal2);
        let s = solve(&d, 300_000, 300_000);
        let r = roles(&s);
        assert_eq!(r[0], Some(Role::Tank));
        assert_eq!(r[5], Some(Role::Tank));
        assert_eq!(r[1], Some(Role::Healer));
        assert_eq!(r[6], Some(Role::Healer));
        assert!(r[2..5].iter().all(|x| *x == Some(Role::Dps)));
    }

    /// A solo player heals no one else and leads every share: never a
    /// healer, a tank only with the kit.
    #[test]
    fn a_solo_player() {
        let solo = RoleEvidence {
            damage: 5_000,
            healing: 2_000,
            casts: 20,
            heals: 5,
            swings: 30,
            ..p("Player-1-solo")
        };
        let s = solve(std::slice::from_ref(&solo), 30_000, 30_000);
        assert_eq!(s[0].role, Some(Role::Dps), "{s:?}");
        let with_kit = RoleEvidence { kit: 6, ..solo };
        assert_eq!(solve(&[with_kit], 30_000, 30_000)[0].role, Some(Role::Tank));
    }

    /// A pull under `MIN_COMBAT_MS` names nobody; a player who did nothing
    /// of their own (only died) is named in no fight.
    #[test]
    fn a_short_pull_and_a_player_who_only_dies_name_no_role() {
        let s = solve(&dungeon(), MIN_COMBAT_MS - 1, MIN_COMBAT_MS - 1);
        assert!(roles(&s).iter().all(Option::is_none), "{s:?}");
        let mut d = dungeon();
        d.push(RoleEvidence {
            swings: 3,
            ..p("Player-1-dead")
        });
        let s = solve(&d, 60_000, 60_000);
        assert_eq!(s[5].role, None);
        // And they do not change the others' fair share.
        assert_eq!(roles(&s[..5]), roles(&solve(&dungeon(), 60_000, 60_000)));
    }

    /// Fewer than `MIN_SWINGS` swings at the group read as none; a tanking
    /// form held all fight still counts with the kit.
    #[test]
    fn too_few_swings_say_nothing_and_a_form_supports_the_kit() {
        let mut d = dungeon();
        for x in &mut d {
            x.swings = 0;
        }
        d[0].swings = MIN_SWINGS - 1;
        let s = solve(&d, 60_000, 60_000);
        assert_eq!(s[0].role, Some(Role::Dps), "{s:?}");
        d[0].form_ms = 60_000;
        let s = solve(&d, 60_000, 60_000);
        assert_eq!(s[0].role, Some(Role::Tank), "{s:?}");
    }

    /// The segment-start rule fires only for a form not yet on in the
    /// fight: a removal after the form's own removal is a repeat and adds
    /// nothing, never the fight's start to it (a 12 s stance reads 12 s).
    #[test]
    fn a_form_removed_twice_is_on_once() {
        let mut l = RoleLedger::default();
        l.form_on("Player-1-t", 71, 4_000);
        l.form_off("Player-1-t", 71, 16_000, 0);
        l.form_off("Player-1-t", 71, 20_000, 0);
        assert_eq!(l.form_ms("Player-1-t", 60_000), 12_000);
        // A form first seen by its removal is the rule's, once.
        l.form_off("Player-1-t", 5487, 9_000, 0);
        l.form_off("Player-1-t", 5487, 30_000, 0);
        assert_eq!(l.form_ms("Player-1-t", 60_000), 12_000 + 9_000);
    }

    /// The ledger: a form with no open aura began with the fight (once),
    /// one still on closes at the fight's end, healing on oneself or one's
    /// pet is nobody else's, and an Overall sums its members.
    #[test]
    fn the_ledger_reads_forms_healing_and_merges() {
        let mut l = RoleLedger::default();
        l.form_off("Player-1-t", 71, 5_000, 1_000);
        l.form_off("Player-1-t", 71, 6_000, 1_000);
        l.form_on("Player-1-t", 71, 8_000);
        l.heal("Player-1-h", "Player-1-t", 100);
        l.heal("Player-1-h", "Player-1-h", 50);
        l.heal("Player-1-h", "Pet-1-h", 25);
        l.heal("Player-1-h", "Pet-1-t", 10);
        l.cast("Player-1-t", Some(RoleSignal::Taunt));
        l.cast("Player-1-t", Some(RoleSignal::Form));
        l.cast("Player-1-h", Some(RoleSignal::Heal));
        l.swing("Player-1-t");
        let fold = |g: &str| match g {
            "Pet-1-h" => "Player-1-h".to_string(),
            "Pet-1-t" => "Player-1-t".to_string(),
            g => g.to_string(),
        };
        let ev = l.evidence(
            [("Player-1-t", 10, 0), ("Player-1-h", 0, 185)],
            fold,
            10_000,
        );
        assert_eq!(ev[0].form_ms, 4_000 + 2_000);
        assert_eq!((ev[0].casts, ev[0].kit, ev[0].swings), (2, 1, 1));
        assert_eq!((ev[1].heal_others, ev[1].heals), (110, 1));
        let mut o = RoleLedger::default();
        o.absorb(&l, 10_000, 9_000);
        o.absorb(&l, 12_000, 2_000);
        assert_eq!(o.merged_span_ms(), 11_000);
        let ev = o.evidence([("Player-1-t", 0, 0)], |g: &str| g.to_string(), 0);
        assert_eq!(ev[0].form_ms, 6_000 + 8_000);
        assert_eq!(ev[0].casts, 4);
    }
}
