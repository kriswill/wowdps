//! A player's label as the combat log writes it, read into the character's
//! name as the game shows it, its realm and its region: the one place a
//! label is split at a dash (CONTRACT.md, "Player names").
//!
//! Retail writes a player `Name-Realm-Region` (`Durgan-Nebula-US`): one
//! name, then the realm as the game normalizes it, with no space or dash.
//! Forever writes `First-Last-Realm-Region` (`Ada-Brightwater-Nebula-US`):
//! a character there has a first and a last name, which the game shows
//! with a space between ("Ada Brightwater"). Both editions write a bare
//! name with no realm now and then, on a line a stranger outside the group
//! casts on one of it. The shape alone cannot always tell them apart: a
//! label of four parts is a Forever name, or a Retail realm written with a
//! dash; one of two parts is Retail's `Name-Realm` or a Forever name with
//! neither. So every reading takes the fight's [`Edition`]. Labels on the
//! wire and in the store stay as the log wrote them; only a reader splits
//! them, here.
//!
//! Nothing here is keyed on: rows, cards, sorting and dedup key on the
//! guid. A name is for showing and for a lookup to resolve, and a lookup
//! that names two players resolves to neither ([`find`]).

use std::borrow::Cow;

use crate::Edition;

/// A player's label read by the edition of its fight: the name's part, as
/// the log writes it, and the realm and region after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label<'a> {
    /// The whole label, as the log wrote it.
    label: &'a str,
    /// The name's part of it, dashed as the log writes it: `Durgan`,
    /// `Ada-Brightwater`.
    name: &'a str,
    realm: Option<&'a str>,
    region: Option<&'a str>,
    /// Whether the edition gives a character two names (Forever).
    two_names: bool,
}

/// How closely a query names a player, from the loosest: [`find`] keeps
/// the closest a query reaches over every player it is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Named {
    /// The start of the name: "Kael" for "Kael'thar", "Ada Bri" for "Ada
    /// Brightwater". Like a first name, it resolves only when it names one
    /// player, and it never names the account's own character.
    Prefix,
    /// A Forever character's first name alone ("Ada"). Two players often
    /// share one, so it resolves only when it names one player, and it
    /// never names the account's own character.
    First,
    /// The character's name as the game shows it: "Durgan", "Ada
    /// Brightwater" (or the log's "Ada-Brightwater").
    Name,
    /// The name and its realm: "Durgan-Nebula", "Ada Brightwater-Nebula".
    Realm,
    /// The label as the log wrote it, region and all.
    Whole,
}

/// What a lookup over several players resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found<K> {
    Nobody,
    One(K),
    /// The query names more than one player at its closest: none is
    /// picked, and every one is listed.
    Several(Vec<K>),
}

/// Two capitals, as every region the logs on hand write is (`US`, `EU`).
fn is_region(part: &str) -> bool {
    part.len() == 2 && part.bytes().all(|b| b.is_ascii_uppercase())
}

/// Whether `edition` gives a character a first and a last name.
fn two_names(edition: Edition) -> bool {
    edition.counts_as() == Edition::Forever
}

impl<'a> Label<'a> {
    /// `label`, known to be a player's (a meter row's, a card's player, a
    /// death's), read by the edition of its fight. Every label reads: a
    /// bare one is a name with no realm.
    ///
    /// - Forever: when the last part is a region and two parts or more come
    ///   before it, the part before the region is the realm and everything
    ///   before that the name; otherwise the whole label is the name.
    /// - Every other edition: the name is the first part; with a region
    ///   last, the realm is everything between, else the second part (a
    ///   log older than the region suffix writes `Name-Realm`).
    pub fn player(label: &'a str, edition: Edition) -> Self {
        let two_names = two_names(edition);
        let whole = Label {
            label,
            name: label,
            realm: None,
            region: None,
            two_names,
        };
        if two_names {
            let Some((head, region)) = label.rsplit_once('-') else {
                return whole;
            };
            match head.rsplit_once('-') {
                Some((name, realm)) if is_region(region) => Label {
                    name,
                    realm: Some(realm),
                    region: Some(region),
                    ..whole
                },
                _ => whole,
            }
        } else {
            let Some((name, rest)) = label.split_once('-') else {
                return whole;
            };
            match rest.rsplit_once('-') {
                Some((realm, region)) if is_region(region) => Label {
                    name,
                    realm: Some(realm),
                    region: Some(region),
                    ..whole
                },
                _ => Label {
                    name,
                    realm: Some(rest),
                    ..whole
                },
            }
        }
    }

    /// `label` read as a player's only when it is shaped as one whole:
    /// three parts or more, the last a region, none empty or holding a
    /// space. For a pane of players and creatures alike (a drill's targets
    /// and attackers, a recap): a creature's dash ("Yogg-Saron",
    /// "Blood-Queen Lana'thel") is part of its name, not a realm.
    pub fn shaped(label: &'a str, edition: Edition) -> Option<Self> {
        let parts = label.split('-');
        let mut count = 0;
        for part in parts {
            if part.is_empty() || part.contains(' ') {
                return None;
            }
            count += 1;
        }
        let read = Self::player(label, edition);
        (count >= 3 && read.region.is_some()).then_some(read)
    }

    /// The character's name as the game shows it: "Durgan", "Ada
    /// Brightwater" (the log's dashes made spaces).
    pub fn name(&self) -> Cow<'a, str> {
        if self.two_names && self.name.contains('-') {
            Cow::Owned(self.name.replace('-', " "))
        } else {
            Cow::Borrowed(self.name)
        }
    }

    /// The first word of the name: "Durgan", "Ada".
    pub fn first(&self) -> &'a str {
        self.name.split('-').next().unwrap_or(self.name)
    }

    pub fn realm(&self) -> Option<&'a str> {
        self.realm
    }

    pub fn region(&self) -> Option<&'a str> {
        self.region
    }

    /// The name with every word after the first cut to its initial: "Ada
    /// B." for "Ada Brightwater" — what a column too narrow for the whole
    /// name writes, so two players sharing a first name stay apart where
    /// their last names start apart. A one-word name is itself.
    pub fn initialed(&self) -> String {
        let mut words = self.name.split('-');
        let mut out = words.next().unwrap_or_default().to_string();
        for w in words {
            if let Some(c) = w.chars().next() {
                out.push(' ');
                out.push(c);
                out.push('.');
            }
        }
        out
    }

    /// The label as a pane writes it: the name, and when `realms`, the
    /// rest of the label as the log wrote it ("Ada Brightwater-Nebula-US").
    /// Retail's with its realm is the label itself.
    pub fn shown(&self, realms: bool) -> String {
        let name = self.name();
        if realms {
            let rest = self.label.get(self.name.len()..).unwrap_or_default();
            format!("{name}{rest}")
        } else {
            name.into_owned()
        }
    }

    /// How `query` names this player, if it does, case aside and a space
    /// and a dash alike: the whole label, the name and realm, the name,
    /// (Forever) the first name alone, or the start of the name. A Forever label that is one word is
    /// a first name alone, so a one-word query reaches it only as
    /// [`Named::First`].
    pub fn named_by(&self, query: &str) -> Option<Named> {
        let query = one_space(query);
        let query = query.as_ref();
        if query.is_empty() {
            return None;
        }
        if let Some(realm) = self.realm {
            if same(query, [self.label]) {
                return Some(Named::Whole);
            }
            if same(query, [self.name, "-", realm]) {
                return Some(Named::Realm);
            }
        }
        let first_alone = self.two_names && !self.name.contains('-');
        if !first_alone && same(query, [self.name]) {
            return Some(Named::Name);
        }
        if self.two_names && same(query, [self.first()]) {
            return Some(Named::First);
        }
        starts(query, self.name).then_some(Named::Prefix)
    }

    /// Whether a configured name (the daemon's `history_characters`, the
    /// window's fallback) is this character: its whole label, its name and
    /// realm, or its name, never a Forever first name alone.
    pub fn is_called(&self, configured: &str) -> bool {
        self.named_by(configured).is_some_and(|n| n >= Named::Name)
    }
}

/// `query` trimmed, every run of white space one space: "ada  brightwater"
/// reads as "ada brightwater".
fn one_space(query: &str) -> Cow<'_, str> {
    let query = query.trim();
    let odd = |c: char| c.is_whitespace() && c != ' ';
    if !query.contains("  ") && !query.contains(odd) {
        Cow::Borrowed(query)
    } else {
        Cow::Owned(query.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

/// A character as a query is compared: lowercase, a space read as a dash.
fn fold(c: char) -> impl Iterator<Item = char> {
    let c = if c == ' ' { '-' } else { c };
    c.to_lowercase()
}

/// Whether `query` reads as `pieces` joined, case aside and a space and a
/// dash alike.
fn same<const N: usize>(query: &str, pieces: [&str; N]) -> bool {
    query
        .chars()
        .flat_map(fold)
        .eq(pieces.iter().flat_map(|p| p.chars()).flat_map(fold))
}

/// Whether `query` reads as the start of `name`, case aside and a space
/// and a dash alike.
fn starts(query: &str, name: &str) -> bool {
    let mut name = name.chars().flat_map(fold);
    query.chars().flat_map(fold).all(|q| name.next() == Some(q))
}

/// Whether a configured name can never name a Forever character: one word,
/// which a Forever label answers to only as a first name, and a configured
/// name is never a first name alone ([`Label::is_called`]). It may still
/// name a Retail character.
pub fn never_names_forever(configured: &str) -> bool {
    let configured = one_space(configured);
    !configured.is_empty() && !configured.contains([' ', '-'])
}

/// `label` as a pane of players and creatures alike writes it: a whole
/// player label as [`Label::shown`] writes it, or the player in the
/// parentheses a recap line or an ability wears ("Word of Glory
/// (Ada-Brightwater-Nebula-US)" → "Word of Glory (Ada Brightwater)"); any
/// other label as it is.
pub fn shown_any(label: &str, edition: Edition, realms: bool) -> Cow<'_, str> {
    if let Some(head) = label.strip_suffix(')')
        && let Some((what, who)) = head.rsplit_once(" (")
    {
        return match Label::shaped(who, edition) {
            Some(p) => Cow::Owned(format!("{what} ({})", p.shown(realms))),
            None => Cow::Borrowed(label),
        };
    }
    match Label::shaped(label, edition) {
        Some(p) => Cow::Owned(p.shown(realms)),
        None => Cow::Borrowed(label),
    }
}

/// The one player among `players` (each a key, its label and its fight's
/// edition) that `query` names at the closest [`Named`] it reaches over
/// all of them, no looser than `floor`. Players are told apart by their
/// keys (guids), never by a name: a key listed twice is one player, two
/// keys at the closest reach are [`Found::Several`]. A lookup over a
/// fight's own rows may take [`Named::Prefix`]; one over many stored
/// fights takes [`Named::First`] at the loosest, so the start of a name
/// never answers about a stranger who happens to begin with it.
pub fn find<K, L>(
    query: &str,
    floor: Named,
    players: impl IntoIterator<Item = (K, L, Edition)>,
) -> Found<K>
where
    K: PartialEq,
    L: AsRef<str>,
{
    find_reach(query, floor, players).0
}

/// [`find`], and the reach it settled at: what tells a pick by a first
/// name or the start of a name, which an answer says it made, from one by
/// the name or more. `None` when nobody answers.
pub fn find_reach<K, L>(
    query: &str,
    floor: Named,
    players: impl IntoIterator<Item = (K, L, Edition)>,
) -> (Found<K>, Option<Named>)
where
    K: PartialEq,
    L: AsRef<str>,
{
    let mut best: Option<Named> = None;
    let mut hits: Vec<K> = Vec::new();
    for (key, label, edition) in players {
        let Some(named) = Label::player(label.as_ref(), edition)
            .named_by(query)
            .filter(|n| *n >= floor)
        else {
            continue;
        };
        match best {
            Some(b) if named < b => {}
            Some(b) if named == b => {
                if !hits.contains(&key) {
                    hits.push(key);
                }
            }
            _ => {
                best = Some(named);
                hits.clear();
                hits.push(key);
            }
        }
    }
    let mut hits = hits.into_iter();
    let found = match (hits.next(), hits.next()) {
        (None, _) => Found::Nobody,
        (Some(one), None) => Found::One(one),
        (Some(a), Some(b)) => Found::Several([a, b].into_iter().chain(hits).collect()),
    };
    (found, best)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Edition = Edition::Retail;
    const F: Edition = Edition::Forever;

    #[test]
    fn retail_reads_one_name_then_the_realm_and_region() {
        let p = Label::player("Durgan-Nebula-US", R);
        assert_eq!(p.name(), "Durgan");
        assert_eq!(p.first(), "Durgan");
        assert_eq!((p.realm(), p.region()), (Some("Nebula"), Some("US")));
        assert_eq!(p.shown(true), "Durgan-Nebula-US");
        assert_eq!(p.shown(false), "Durgan");
        assert_eq!(p.initialed(), "Durgan");
        // A log older than the region suffix, and a bare name.
        let old = Label::player("Durgan-Nebula", R);
        assert_eq!(
            (old.name(), old.realm(), old.region()),
            ("Durgan".into(), Some("Nebula"), None)
        );
        let bare = Label::player("Durgan", R);
        assert_eq!((bare.name(), bare.realm()), ("Durgan".into(), None));
        assert_eq!(bare.shown(true), "Durgan");
        // A realm the game wrote with a dash stays the realm in Retail.
        let dashed = Label::player("Durgan-Azure-Reach-US", R);
        assert_eq!(
            (dashed.name(), dashed.realm()),
            ("Durgan".into(), Some("Azure-Reach"))
        );
    }

    #[test]
    fn forever_reads_a_first_and_a_last_name_before_the_realm() {
        let p = Label::player("Ada-Brightwater-Nebula-US", F);
        assert_eq!(p.name(), "Ada Brightwater");
        assert_eq!(p.first(), "Ada");
        assert_eq!((p.realm(), p.region()), (Some("Nebula"), Some("US")));
        assert_eq!(p.shown(true), "Ada Brightwater-Nebula-US");
        assert_eq!(p.shown(false), "Ada Brightwater");
        assert_eq!(p.initialed(), "Ada B.");
        // A lowercase last word is a word all the same.
        assert_eq!(Label::player("Ada-of-Nebula-US", F).name(), "Ada of");
        // The stranger's bare first name.
        let bare = Label::player("Ada", F);
        assert_eq!((bare.name(), bare.realm()), ("Ada".into(), None));
        // The same four parts read by Retail's rule are a dashed realm.
        assert_eq!(Label::player("Ada-Brightwater-Nebula-US", R).name(), "Ada");
        // An edition read as Retail (a card that named none) reads as Retail.
        assert_eq!(
            Label::player("Ada-Brightwater-Nebula-US", Edition::Unknown(0)).name(),
            "Ada"
        );
    }

    #[test]
    fn a_mixed_pane_rewrites_players_and_leaves_creatures_alone() {
        for e in [R, F] {
            assert_eq!(shown_any("Yogg-Saron", e, false), "Yogg-Saron");
            assert_eq!(
                shown_any("Blood-Queen Lana'thel", e, false),
                "Blood-Queen Lana'thel"
            );
            assert_eq!(shown_any("Lightning Bolt", e, false), "Lightning Bolt");
        }
        assert_eq!(shown_any("Durgan-Nebula-US", R, false), "Durgan");
        assert_eq!(shown_any("Durgan-Nebula-US", R, true), "Durgan-Nebula-US");
        assert_eq!(
            shown_any("Ada-Brightwater-Nebula-US", F, false),
            "Ada Brightwater"
        );
        assert_eq!(
            shown_any("Ada-Brightwater-Nebula-US", F, true),
            "Ada Brightwater-Nebula-US"
        );
        assert_eq!(
            shown_any("Word of Glory (Ada-Brightwater-Nebula-US)", F, false),
            "Word of Glory (Ada Brightwater)"
        );
        assert_eq!(
            shown_any("Word of Glory (Durgan-Nebula-US)", R, true),
            "Word of Glory (Durgan-Nebula-US)"
        );
        assert_eq!(
            shown_any("Hurl (Yogg-Saron)", R, false),
            "Hurl (Yogg-Saron)"
        );
    }

    #[test]
    fn a_query_names_a_player_whole_by_realm_by_name_or_by_first_name() {
        let p = Label::player("Ada-Brightwater-Nebula-US", F);
        assert_eq!(p.named_by("ada-brightwater-nebula-us"), Some(Named::Whole));
        assert_eq!(p.named_by("Ada Brightwater-Nebula-US"), Some(Named::Whole));
        assert_eq!(p.named_by("Ada Brightwater-Nebula"), Some(Named::Realm));
        assert_eq!(p.named_by("ada brightwater"), Some(Named::Name));
        assert_eq!(p.named_by("Ada-Brightwater"), Some(Named::Name));
        assert_eq!(p.named_by(" Ada "), Some(Named::First));
        assert_eq!(p.named_by("Brightwater"), None);
        assert_eq!(p.named_by("Ad"), Some(Named::Prefix));
        assert_eq!(p.named_by("ada bright"), Some(Named::Prefix));
        assert_eq!(p.named_by(""), None);
        // A bare Forever label is a first name alone.
        assert_eq!(Label::player("Ada", F).named_by("Ada"), Some(Named::First));
        let r = Label::player("Durgan-Nebula-US", R);
        assert_eq!(r.named_by("durgan"), Some(Named::Name));
        assert_eq!(r.named_by("Durgan-Nebula"), Some(Named::Realm));
        assert_eq!(r.named_by("Durgan-Elsewhere"), None);
        assert_eq!(r.named_by("Dur"), Some(Named::Prefix));
        // A configured name is never a first name alone.
        assert!(p.is_called("Ada Brightwater"));
        assert!(!p.is_called("Ada") && !p.is_called("Ada Bright"));
        assert!(r.is_called("Durgan") && r.is_called("Durgan-Nebula"));
        // One word names no Forever character, whatever its label.
        assert!(never_names_forever(" Ada ") && never_names_forever("Durgan"));
        assert!(!never_names_forever("Ada Brightwater") && !never_names_forever("Durgan-Nebula"));
        assert!(!never_names_forever("  "));
        for label in ["Ada-Brightwater-Nebula-US", "Ada-Nebula-US", "Ada"] {
            assert!(!Label::player(label, F).is_called("Ada"), "{label}");
        }
    }

    /// Two players who share a first name are never one: the first name
    /// resolves to neither, and each full name to its own.
    #[test]
    fn two_players_sharing_a_first_name_are_never_confused() {
        let players = [
            ("Player-1-A", "Ada-Brightwater-Nebula-US", F),
            ("Player-1-B", "Ada-Thornfield-Nebula-US", F),
            ("Player-1-C", "Corwin-Ashdown-Nebula-US", F),
        ];
        assert_eq!(
            find("Ada", Named::Prefix, players),
            Found::Several(vec!["Player-1-A", "Player-1-B"])
        );
        assert_eq!(
            find("Ada Thornfield", Named::Prefix, players),
            Found::One("Player-1-B")
        );
        assert_eq!(
            find("ada-brightwater", Named::Prefix, players),
            Found::One("Player-1-A")
        );
        assert_eq!(
            find("Corwin", Named::Prefix, players),
            Found::One("Player-1-C")
        );
        assert_eq!(find("Brightwater", Named::Prefix, players), Found::Nobody);
        assert_eq!(
            find("Ada T", Named::Prefix, players),
            Found::One("Player-1-B")
        );
        assert_eq!(find("Co", Named::Prefix, players), Found::One("Player-1-C"));
        // A player listed twice (two cards, a bare label beside the whole
        // one) is one player.
        let twice = [
            ("Player-1-A", "Ada-Brightwater-Nebula-US", F),
            ("Player-1-A", "Ada", F),
        ];
        assert_eq!(find("Ada", Named::Prefix, twice), Found::One("Player-1-A"));
        // A closer reach wins over a looser one: the full name over a first.
        let close = [
            ("Player-1-A", "Ada-Brightwater-Nebula-US", F),
            ("Player-1-D", "Ada-Ada-Nebula-US", F),
        ];
        assert_eq!(
            find("Ada Ada", Named::Prefix, close),
            Found::One("Player-1-D")
        );
        // Retail's namesakes on two realms are two players too.
        let realms = [
            ("Player-1-E", "Durgan-Nebula-US", R),
            ("Player-2-F", "Durgan-Halcyon-US", R),
        ];
        assert_eq!(
            find("Durgan", Named::Prefix, realms),
            Found::Several(vec!["Player-1-E", "Player-2-F"])
        );
        assert_eq!(
            find("Durgan-Halcyon", Named::Prefix, realms),
            Found::One("Player-2-F")
        );
        // Over many stored fights the start of a name answers nobody: a
        // name only a longer one begins with is no one's there.
        let longer = [
            ("Player-1-G", "Brannocks-Halcyon-US", R),
            ("Player-6101-H", "Corwinna-Vale-Nebula-US", F),
        ];
        assert_eq!(find("Brannoc", Named::First, longer), Found::Nobody);
        assert_eq!(find("Corwin", Named::First, longer), Found::Nobody);
        assert_eq!(
            find("Brannoc", Named::Prefix, longer),
            Found::One("Player-1-G")
        );
        // The reach a pick settled at.
        assert_eq!(
            find_reach("Corwinna", Named::First, longer),
            (Found::One("Player-6101-H"), Some(Named::First))
        );
        assert_eq!(
            find_reach("Corwinna Vale", Named::First, longer),
            (Found::One("Player-6101-H"), Some(Named::Name))
        );
        assert_eq!(
            find_reach("Nobody", Named::First, longer),
            (Found::Nobody, None)
        );
        // A Forever first name is a stored lookup's loosest reach.
        assert_eq!(
            find("Corwinna", Named::First, longer),
            Found::One("Player-6101-H")
        );
        // Runs of white space read as one space.
        assert_eq!(
            find("corwinna   vale", Named::Name, longer),
            Found::One("Player-6101-H")
        );
    }
}
