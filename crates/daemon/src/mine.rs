//! v35: whose rows are the reader's. The account's characters as the daemon
//! knows them — the wowdps addon's own-character set, the history store's
//! owner and every stored card's owner, by guid, and the configured
//! `history_characters` by name — and the one place an answer's rows are
//! marked `mine` from them, so every character of the account is "you" in
//! any fight without a client matching labels.
//!
//! Resolved by the history thread (it holds the store and the addon's
//! records) and read by the hub on every snapshot it builds; a daemon with
//! no store knows nobody, and nothing is marked.

use std::collections::HashSet;

use wowdps_core::meter::Segment;
use wowdps_core::model::Row;
use wowdps_model::RaidTimeline;
use wowdps_proto::Breakdown;

use crate::history::name_matches;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mine {
    /// The addon's own characters, the store's owner, every card's owner.
    guids: HashSet<String>,
    /// The configured characters, lowercased — "Name-Realm" matches whole,
    /// a bare "Name" the name half, exactly as the store matches them.
    names: Vec<String>,
}

impl Mine {
    pub fn new(guids: impl IntoIterator<Item = String>, names: &[String]) -> Self {
        Self {
            guids: guids.into_iter().filter(|g| !g.is_empty()).collect(),
            names: names
                .iter()
                .map(|n| n.trim().to_lowercase())
                .filter(|n| !n.is_empty())
                .collect(),
        }
    }

    /// Is the player `guid`, called `name`, one of the account's?
    pub fn owns(&self, guid: &str, name: &str) -> bool {
        self.guids.contains(guid) || name_matches(&self.names, name)
    }

    /// The names the account's characters wear among `players` (guid,
    /// name) — what a drill row keyed by NAME is matched against.
    pub fn names_among<'a>(
        &self,
        players: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Vec<String> {
        players
            .into_iter()
            .filter(|(g, n)| self.owns(g, n))
            .map(|(_, n)| n.to_string())
            .collect()
    }

    /// The same, for a segment: the names it knows the account's guids by.
    pub fn names_in(&self, seg: &Segment) -> Vec<String> {
        self.guids
            .iter()
            .filter_map(|g| seg.name_of(g))
            .map(str::to_string)
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
            r.mine = here.contains(&r.label) || name_matches(&self.names, &r.label);
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
            &["Tranqster".to_string(), "Other-Realm".to_string()],
        );
        assert!(mine.owns("Player-1-A", "Anybody-Realm"));
        assert!(mine.owns("Player-1-B", "Tranqster-Proudmoore"));
        assert!(mine.owns("Player-1-C", "Other-Realm"));
        assert!(!mine.owns("Player-1-D", "Other-Elsewhere"));
        assert!(!Mine::default().owns("Player-1-A", "Tranqster"));
        let mut rows = vec![
            row("Player-1-A", "Tranqlock-X"),
            row("Player-1-Z", "Swampert-X"),
        ];
        mine.mark_rows(&mut rows);
        assert_eq!(
            rows.iter().map(|r| r.mine).collect::<Vec<_>>(),
            [true, false]
        );
    }

    /// A heal's targets are keyed by name: the names the account's guids
    /// wear here mark them, and a stranger stays unmarked.
    #[test]
    fn a_drill_row_is_mine_by_the_name_the_fight_gives_the_guid() {
        let mine = Mine::new(["Player-1-A".to_string()], &[]);
        let here = mine.names_among([("Player-1-A", "Tranqlock-X"), ("Player-1-Z", "Swampert-X")]);
        assert_eq!(here, ["Tranqlock-X"]);
        let mut b = Breakdown {
            by_target: vec![
                row("Tranqlock-X", "Tranqlock-X"),
                row("Swampert-X", "Swampert-X"),
            ],
            ..Breakdown::default()
        };
        mine.mark_breakdown(&mut b, false, &here);
        assert_eq!(
            b.by_target.iter().map(|r| r.mine).collect::<Vec<_>>(),
            [true, false]
        );
    }
}
