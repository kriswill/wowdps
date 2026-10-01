//! What a GUI writes a name or an outcome as: a player's name without its
//! realm, and a segment's verdict word. Moved from the iced GUI's
//! `view.rs`, which re-exports them; the colours a GUI draws them in stay
//! its own (a verdict comes with a [`Tone`], not a colour).

use wowdps_model::SegmentKind;
use wowdps_proto::ClientState;

/// "Keanucleavês-Proudmoore-US" → "Keanucleavês". Character names cannot
/// contain '-', so everything from the first dash is realm noise.
pub fn display_name(label: &str) -> &str {
    label.split('-').next().unwrap_or(label)
}

/// `label` with a player's realm taken off wherever one is written: a whole
/// label ("Bearlysimpin-Proudmoore-US" → "Bearlysimpin"), or the source in
/// the parentheses a recap line or an ability wears ("Word of Glory
/// (Soundscape-Proudmoore-US)" → "Word of Glory (Soundscape)"). For the
/// panes whose rows are players and creatures alike — a drill's targets and
/// attackers, a recap — so, unlike [`display_name`], it touches only what
/// reads as a player's "Name-Realm-Region": a creature's hyphen ("Yogg-
/// Saron", "Blood-Queen Lana'thel") is part of its name, not a realm.
pub fn realmless(label: &str) -> String {
    if let Some(head) = label.strip_suffix(')')
        && let Some((what, who)) = head.rsplit_once(" (")
    {
        return match player_name(who) {
            Some(name) => format!("{what} ({name})"),
            None => label.to_string(),
        };
    }
    player_name(label).map_or_else(|| label.to_string(), str::to_string)
}

/// The name in a player label the log writes as "Name-Realm-Region" (the
/// region two capitals, no part of it holding a space), else `None`.
fn player_name(label: &str) -> Option<&str> {
    let mut parts = label.split('-');
    let name = parts.next().filter(|n| !n.is_empty() && !n.contains(' '))?;
    let rest: Vec<&str> = parts.collect();
    let region = rest.last()?;
    let shaped = rest.len() >= 2
        && region.len() == 2
        && region.chars().all(|c| c.is_ascii_uppercase())
        && rest.iter().all(|p| !p.is_empty() && !p.contains(' '));
    shaped.then_some(name)
}

/// What a verdict word means, for a GUI to colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Still going.
    Live,
    Good,
    Bad,
    /// No word at all.
    None,
}

/// Header badge for the watched segment: LIVE while accumulating, else
/// success worded by kind — KILL/WIPE for fights, WIN/LOSS for an arena
/// match (R13), TIMED/OVER for a keyed visit's overall (R10).
pub fn header_tag(app: &ClientState) -> (&'static str, Tone) {
    if app.is_live() {
        return ("LIVE", Tone::Live);
    }
    match verdict(app) {
        Some((word, true)) => (word, Tone::Good),
        Some((word, false)) => (word, Tone::Bad),
        None => ("", Tone::None),
    }
}

/// A closed segment's outcome: its word, and whether it went well. `None`
/// while it has none (trash, an unfinished pull).
pub fn verdict(app: &ClientState) -> Option<(&'static str, bool)> {
    let overall = app.segment_kind() == Some(SegmentKind::Overall);
    let good = app.segment_success()?;
    Some(match (good, overall) {
        (true, false) if app.segment_arena() => ("WIN", true),
        (false, false) if app.segment_arena() => ("LOSS", false),
        (true, false) => ("KILL", true),
        (false, false) => ("WIPE", false),
        (true, true) => ("TIMED", true),
        (false, true) => ("OVER", false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_player_loses_the_realm_and_a_creature_keeps_its_hyphen() {
        assert_eq!(display_name("Keanucleavês-Proudmoore-US"), "Keanucleavês");
        assert_eq!(realmless("Bearlysimpin-Proudmoore-US"), "Bearlysimpin");
        assert_eq!(
            realmless("Word of Glory (Soundscape-Proudmoore-US)"),
            "Word of Glory (Soundscape)"
        );
        assert_eq!(realmless("Yogg-Saron"), "Yogg-Saron");
        assert_eq!(realmless("Blood-Queen Lana'thel"), "Blood-Queen Lana'thel");
    }

    #[test]
    fn a_state_with_nothing_watched_has_no_word() {
        assert_eq!(header_tag(&ClientState::new()), ("", Tone::None));
        assert_eq!(verdict(&ClientState::new()), None);
    }
}
