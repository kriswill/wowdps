//! v50 (R30): the role a row's player plays where no spec names one. An
//! edition whose log carries no spec has its roles inferred per fight
//! (`Segment::inferred_roles`); the daemon sets them on the rows it answers
//! with (`Row::inferred_role`), so every reader takes a row's role from
//! `Row::role` alone, the spec's or the inferred one alike.
//!
//! Solved once per answer, never per row: [`Roles::of`] runs the solver
//! for a segment of an edition that infers roles and nothing at all for any
//! other (Retail's role is its spec's), and an empty `Roles` marks nothing,
//! so a Retail frame is the bytes it was before v50. A stored fight's rows
//! wear the card's stored roles ([`Roles::of_card`]), which the store took
//! from the same solver when it stored the fight.

use std::collections::BTreeMap;

use wowdps_core::meter::Segment;
use wowdps_core::model::Row;
use wowdps_model::{RaidTimeline, Role};
use wowdps_proto::Breakdown;
use wowdps_proto::history::{CardPlayer, FightCard};

/// The roles one answer's rows wear, by player guid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roles(BTreeMap<String, Role>);

impl Roles {
    /// The roles `seg`'s own lines give its players: empty, and solved not
    /// at all, for an edition whose log carries a spec.
    pub fn of(seg: &Segment) -> Self {
        Self(seg.inferred_roles())
    }

    /// A stored fight's: each player's stored role, for those whose log
    /// named no spec (`CardPlayer::stored_role`).
    pub fn of_card(card: &FightCard) -> Self {
        Self::of_players(&card.players)
    }

    fn of_players(players: &[CardPlayer]) -> Self {
        Self(
            players
                .iter()
                .filter(|p| p.spec.is_none())
                .filter_map(|p| p.stored_role.map(|r| (p.guid.clone(), r)))
                .collect(),
        )
    }

    /// The role `guid` plays here, if one was inferred.
    pub fn get(&self, guid: &str) -> Option<Role> {
        self.0.get(guid).copied()
    }

    /// Rows keyed by a player's guid (the meter's, a comparison's total, the
    /// enemy view's attackers): each with no spec wears its player's role.
    /// A row keyed by anything else (an ability, a name) matches no guid.
    pub fn mark_rows(&self, rows: &mut [Row]) {
        if self.0.is_empty() {
            return;
        }
        for r in rows.iter_mut().filter(|r| r.spec.is_none()) {
            r.inferred_role = self.get(&r.key);
        }
    }

    /// A drill's lists, wherever one names a player by guid.
    pub fn mark_breakdown(&self, b: &mut Breakdown) {
        if self.0.is_empty() {
            return;
        }
        self.mark_rows(&mut b.by_spell);
        self.mark_rows(&mut b.by_target);
        if let Some(t) = b.spell_targets.as_mut() {
            self.mark_rows(t);
        }
    }

    /// R25: the raid timeline's deaths, filtered by role as their rows are.
    pub fn mark_raid(&self, raid: &mut RaidTimeline) {
        if self.0.is_empty() {
            return;
        }
        for d in raid.deaths.iter_mut().filter(|d| d.spec.is_none()) {
            d.inferred_role = self.get(&d.guid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::Spec;

    fn row(key: &str, spec: Option<Spec>) -> Row {
        Row {
            key: key.to_string(),
            spec,
            ..Row::default()
        }
    }

    fn roles(pairs: &[(&str, Role)]) -> Roles {
        Roles(pairs.iter().map(|(g, r)| (g.to_string(), *r)).collect())
    }

    /// A specless row wears its player's role; a row with a spec keeps the
    /// spec's alone (the wire could not say both), and a row of nobody the
    /// solver named wears none.
    #[test]
    fn a_role_is_worn_only_where_no_spec_says_one() {
        let r = roles(&[("Player-1-A", Role::Tank), ("Player-1-B", Role::Healer)]);
        let mut rows = vec![
            row("Player-1-A", None),
            row("Player-1-B", Some(Spec::FrostMage)),
            row("Player-1-C", None),
            row("Frostbolt", None),
        ];
        r.mark_rows(&mut rows);
        let worn: Vec<_> = rows.iter().map(|r| r.inferred_role).collect();
        assert_eq!(worn, vec![Some(Role::Tank), None, None, None]);
        let read: Vec<_> = rows.iter().map(Row::role).collect();
        assert_eq!(read, vec![Some(Role::Tank), Some(Role::Dps), None, None]);
    }

    /// An empty set (Retail's, every segment of an edition with specs)
    /// leaves every row exactly as the meter built it.
    #[test]
    fn no_roles_touch_nothing() {
        let mut rows = vec![row("Player-1-A", None)];
        rows[0].inferred_role = Some(Role::Healer);
        Roles::default().mark_rows(&mut rows);
        assert_eq!(rows[0].inferred_role, Some(Role::Healer));
    }

    /// A card's roles are its specless players' stored ones.
    #[test]
    fn a_cards_roles_are_its_stored_roles() {
        let player = |guid: &str, spec, stored_role| CardPlayer {
            guid: guid.to_string(),
            spec,
            stored_role,
            ..CardPlayer::default()
        };
        let players = [
            player("Player-1-A", None, Some(Role::Tank)),
            player("Player-1-B", Some(Spec::Discipline), Some(Role::Tank)),
            player("Player-1-C", None, None),
        ];
        assert_eq!(
            Roles::of_players(&players),
            roles(&[("Player-1-A", Role::Tank)])
        );
    }
}
