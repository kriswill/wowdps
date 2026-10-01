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

/// The badge for an instance visit's Σ row: its outcome once known (R10
/// wording), else LIVE while the visit is in progress. A keyed visit's
/// badge carries the tier and overtime detail ("TIMED +2", "OVER +0:26",
/// live pace "LIVE +3"), judged at `clock_ms` — the clock shown beside it.
/// A known outcome beats "still inside": a timed key is TIMED even while
/// the party finishes trash before zoning out.
pub fn overall_tag(row: &wowdps_model::ListRow, clock_ms: i64) -> (String, Tone) {
    use wowdps_model::fmt::key_tag;
    match (row.success, row.pars_ms) {
        (success @ Some(timed), Some(pars)) => (
            key_tag(clock_ms, pars, success),
            if timed { Tone::Good } else { Tone::Bad },
        ),
        (Some(true), None) => ("TIMED".into(), Tone::Good),
        (Some(false), None) => ("OVER".into(), Tone::Bad),
        (None, pars) if row.live => (
            match pars {
                Some(p) => format!("LIVE {}", key_tag(clock_ms, p, None)),
                None => "LIVE".into(),
            },
            Tone::Live,
        ),
        (None, _) => (String::new(), Tone::None),
    }
}

/// A class's two-letter tag, for a disc drawn where the art cache has no
/// crest; "?" for an unknown class.
pub fn class_tag(class: Option<wowdps_model::Class>) -> &'static str {
    use wowdps_model::Class;
    match class {
        Some(Class::Warrior) => "WR",
        Some(Class::Paladin) => "PA",
        Some(Class::Hunter) => "HU",
        Some(Class::Rogue) => "RO",
        Some(Class::Priest) => "PR",
        Some(Class::DeathKnight) => "DK",
        Some(Class::Shaman) => "SH",
        Some(Class::Mage) => "MG",
        Some(Class::Warlock) => "WL",
        Some(Class::Monk) => "MO",
        Some(Class::Druid) => "DR",
        Some(Class::DemonHunter) => "DH",
        Some(Class::Evoker) => "EV",
        None => "?",
    }
}

// ---- the window's words ----------------------------------------------------

/// The window's views in the prototype's order (`VIEWS`): damage and
/// healing, then the two a raid reads next — what was taken and who died —
/// before the counts, and the enemies last. `View::ALL` (the TUI's, and the
/// overlay's cycle) keeps its own order.
pub const WINDOW_VIEWS: [wowdps_model::View; 8] = {
    use wowdps_model::View;
    [
        View::Damage,
        View::Healing,
        View::Taken,
        View::Deaths,
        View::Interrupts,
        View::CrowdControl,
        View::Dispels,
        View::EnemyTaken,
    ]
};

/// A view's name in the window, in the prototype's sentence case and its
/// words ("Crowd control", "Enemies"). The overlay keeps `view_name`'s.
pub fn window_view_name(v: wowdps_model::View) -> &'static str {
    use wowdps_model::View;
    match v {
        View::Damage => "Damage",
        View::Healing => "Healing",
        View::Taken => "Taken",
        View::Deaths => "Deaths",
        View::Interrupts => "Interrupts",
        View::CrowdControl => "Crowd control",
        View::Dispels => "Dispels",
        View::EnemyTaken => "Enemies",
    }
}

/// How a view words its per-second rate: `dps`, `hps`, or `dtps` for damage
/// taken (R17). Count views never show one; they read `dps` here only
/// because nothing asks them.
pub fn rate_label(view: wowdps_model::View) -> &'static str {
    use wowdps_model::View;
    match view {
        View::Healing => "hps",
        View::Taken | View::EnemyTaken => "dtps",
        _ => "dps",
    }
}

/// `s` in sentence case: its first letter capital, every other letter
/// lower — "KILL" → "Kill", "OVER +0:26" → "Over +0:26", "crowd control"
/// → "Crowd control". The window's one case (the prototype's).
pub fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// "1 fight", "3 fights": a count and its noun, agreeing.
pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
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
