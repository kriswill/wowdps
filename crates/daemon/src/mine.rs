//! v35: whose rows are the reader's. The account's characters as the daemon
//! knows them — the wowdps addon's own-character set, the history store's
//! owner and every stored card's owner, by guid, and the configured
//! `history_characters` by name — and the one place an answer's rows are
//! marked `mine` from them, so every character of the account is "you" in
//! any fight without a client matching labels.
//!
//! Resolved by the history thread (it holds the store and the addon's
//! records) and read by the hub on every snapshot it builds; a daemon with
//! no store knows nobody by these. R31 adds the fight's own word: on a
//! source the install's config or discovery names, the character its unit
//! flags name as the one writing the log (`Segment::logger`) is "you" in it
//! ([`In::logged_by`]), store or none.

use std::collections::HashSet;

use wowdps_core::meter::Segment;
use wowdps_core::model::Row;
use wowdps_model::{Edition, RaidTimeline};
use wowdps_proto::Breakdown;

use crate::history::name_matches;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mine {
    /// The addon's own characters, the store's owner, every card's owner.
    guids: HashSet<String>,
    /// The configured characters, trimmed: "Name-Realm" or "Name" (a
    /// Forever character's "First Last"), matched as the store matches
    /// them ([`name_matches`]).
    names: Vec<String>,
}

impl Mine {
    pub fn new(guids: impl IntoIterator<Item = String>, names: &[String]) -> Self {
        Self {
            guids: guids.into_iter().filter(|g| !g.is_empty()).collect(),
            names: names
                .iter()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
        }
    }

    /// The account's characters in a fight of `edition`, whose labels are
    /// read by that edition's rule (`wowdps_model::names`).
    pub fn of(&self, edition: Edition) -> In<'_> {
        In {
            mine: self,
            edition,
            logger: None,
        }
    }
}

/// [`Mine`] in a fight of one edition: what marks its rows.
#[derive(Debug, Clone, Copy)]
pub struct In<'a> {
    mine: &'a Mine,
    edition: Edition,
    /// R31: the fight's own logger, "you" in it whoever else is.
    logger: Option<&'a str>,
}

impl<'a> In<'a> {
    /// R31: the character the fight's log names as its writer is the
    /// account's in it too: a live segment's [`Segment::logger`], a card's
    /// stamped owner. `None` adds nobody.
    pub fn logged_by(self, logger: Option<&'a str>) -> Self {
        Self {
            logger: logger.or(self.logger),
            ..self
        }
    }

    /// Is the player `guid`, called `name`, one of the account's?
    pub fn owns(&self, guid: &str, name: &str) -> bool {
        self.logger == Some(guid)
            || self.mine.guids.contains(guid)
            || name_matches(&self.mine.names, name, self.edition)
    }

    /// The names `seg` knows the account's guids by, its logger's among
    /// them — what a drill row keyed by NAME is matched against.
    pub fn names_in(&self, seg: &Segment) -> Vec<String> {
        self.mine
            .guids
            .iter()
            .map(String::as_str)
            .chain(self.logger.filter(|g| !self.mine.guids.contains(*g)))
            .filter_map(|g| seg.name_of(g))
            .map(str::to_string)
            .collect()
    }

    /// The names the account's characters wear among `players` (guid,
    /// name) — what a drill row keyed by NAME is matched against.
    pub fn names_among<'p>(
        &self,
        players: impl IntoIterator<Item = (&'p str, &'p str)>,
    ) -> Vec<String> {
        players
            .into_iter()
            .filter(|(g, n)| self.owns(g, n))
            .map(|(_, n)| n.to_string())
            .collect()
    }

    /// Meter rows, keyed by guid: the account's characters are `mine`.
    pub fn mark_rows(&self, rows: &mut [Row]) {
        for r in rows {
            r.mine = self.owns(&r.key, &r.label);
        }
    }

    /// Drill rows that name a player by NAME (a heal's targets, a death's
    /// attackers), against the names `here` gives the account's characters.
    pub fn mark_named(&self, rows: &mut [Row], here: &[String]) {
        for r in rows {
            r.mine =
                here.contains(&r.label) || name_matches(&self.mine.names, &r.label, self.edition);
        }
    }

    /// A drill: its player-naming lists. On the enemy view the attackers
    /// are keyed by guid (R24's owner fold); everywhere else by name. The
    /// by-spell list names abilities (or a recap's events) and is left be.
    pub fn mark_breakdown(&self, b: &mut Breakdown, by_guid: bool, here: &[String]) {
        if by_guid {
            self.mark_rows(&mut b.by_target);
        } else {
            self.mark_named(&mut b.by_target, here);
            if let Some(t) = b.spell_targets.as_mut() {
                self.mark_named(t, here);
            }
        }
    }

    /// R25: the raid timeline's deaths.
    pub fn mark_raid(&self, raid: &mut RaidTimeline) {
        for d in &mut raid.deaths {
            d.mine = self.owns(&d.guid, &d.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, label: &str) -> Row {
        Row {
            key: key.to_string(),
            label: label.to_string(),
            ..Row::default()
        }
    }

    /// A guid is the account's whatever it is called, and a configured name
    /// is matched as the store matches it: whole with a realm, by the name
    /// half without one.
    #[test]
    fn a_character_is_mine_by_guid_or_by_its_configured_name() {
        let mine = Mine::new(
            ["Player-1-A".to_string()],
            &["Durgan".to_string(), "Other-Realm".to_string()],
        );
        let retail = mine.of(Edition::Retail);
        assert!(retail.owns("Player-1-A", "Anybody-Realm"));
        assert!(retail.owns("Player-1-B", "Durgan-Nebula"));
        assert!(retail.owns("Player-1-C", "Other-Realm"));
        assert!(!retail.owns("Player-1-D", "Other-Elsewhere"));
        assert!(
            !Mine::default()
                .of(Edition::Retail)
                .owns("Player-1-A", "Durgan")
        );
        // A configured "Name-Realm" names its character whatever region
        // the log writes after it.
        assert!(retail.owns("Player-1-C", "Other-Realm-US"));
        let mut rows = vec![
            row("Player-1-A", "Brannoc-X"),
            row("Player-1-Z", "Merrow-X"),
        ];
        retail.mark_rows(&mut rows);
        assert_eq!(
            rows.iter().map(|r| r.mine).collect::<Vec<_>>(),
            [true, false]
        );
    }

    /// A Forever character is configured by its full name, as the game
    /// shows it: its first name alone is no one's, since a stranger who
    /// shares it would be marked "you".
    #[test]
    fn a_forever_character_is_mine_by_its_full_name_never_its_first() {
        let mine = Mine::new(
            Vec::new(),
            &["Ada Brightwater".to_string(), "Corwin".to_string()],
        );
        let forever = mine.of(Edition::Forever);
        assert!(forever.owns("Player-6101-A", "Ada-Brightwater-Nebula-US"));
        assert!(!forever.owns("Player-6101-B", "Ada-Thornfield-Nebula-US"));
        assert!(!forever.owns("Player-6101-C", "Corwin-Ashdown-Nebula-US"));
        let dashed = Mine::new(Vec::new(), &["ada-brightwater-nebula".to_string()]);
        assert!(
            dashed
                .of(Edition::Forever)
                .owns("Player-6101-A", "Ada-Brightwater-Nebula-US")
        );
    }

    /// A heal's targets are keyed by name: the names the account's guids
    /// wear here mark them, and a stranger stays unmarked.
    #[test]
    fn a_drill_row_is_mine_by_the_name_the_fight_gives_the_guid() {
        let mine = Mine::new(["Player-1-A".to_string()], &[]);
        let retail = mine.of(Edition::Retail);
        let here = retail.names_among([("Player-1-A", "Brannoc-X"), ("Player-1-Z", "Merrow-X")]);
        assert_eq!(here, ["Brannoc-X"]);
        let mut b = Breakdown {
            by_target: vec![row("Brannoc-X", "Brannoc-X"), row("Merrow-X", "Merrow-X")],
            ..Breakdown::default()
        };
        retail.mark_breakdown(&mut b, false, &here);
        assert_eq!(
            b.by_target.iter().map(|r| r.mine).collect::<Vec<_>>(),
            [true, false]
        );
    }
}
