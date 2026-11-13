//! What a GUI writes a name or an outcome as: a player's name as the game
//! shows it, with or without its realm, and a segment's verdict word.
//! Moved from the iced GUI's `view.rs`, which re-exports them; the colors
//! a GUI draws them in stay its own (a verdict comes with a [`Tone`], not
//! a color). A label is read through `wowdps_model::names`, by the edition
//! of the fight it comes from, and never split here.

use wowdps_model::names::{Label, shown_any};
use wowdps_model::{Edition, Row, SegmentKind};
use wowdps_proto::ClientState;

/// How a pane writes players' names: by the edition of the fight it shows,
/// whose labels it reads, and with or without realms as the option
/// (`hide_realms`) says. A Retail player reads "Durgan" or
/// "Durgan-Nebula-US" (the label itself); a Forever one, whose log writes
/// "Ada-Brightwater-Nebula-US", reads "Ada Brightwater" or
/// "Ada Brightwater-Nebula-US".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Names {
    pub edition: Edition,
    pub hide_realms: bool,
}

impl Default for Names {
    fn default() -> Self {
        Self::new(None, false)
    }
}

impl Names {
    /// A fight of `edition` (Retail's rule when none is known yet).
    pub fn new(edition: Option<Edition>, hide_realms: bool) -> Self {
        Self {
            edition: edition.unwrap_or(Edition::Retail),
            hide_realms,
        }
    }

    /// A player's label, read.
    pub fn label<'a>(&self, label: &'a str) -> Label<'a> {
        Label::player(label, self.edition)
    }

    /// A player's name as the game shows it, never with its realm:
    /// "Durgan", "Ada Brightwater". For the places a name stands alone
    /// (the overlay's rows, a crumb, "you", a rezzer).
    pub fn name(&self, label: &str) -> String {
        self.label(label).name().into_owned()
    }

    /// A player's label as the pane writes it: the name, and the realm
    /// and region after it unless the option hides them. For a label known
    /// to be a player's: the rail's tips, Home, the palette, a drill's
    /// player.
    pub fn shown(&self, label: &str) -> String {
        self.label(label).shown(!self.hide_realms)
    }

    /// A label in a pane of players and creatures alike (a drill's targets
    /// and attackers, a recap, a death's source): a player's as
    /// [`Names::shown`] writes it, a creature's as it is.
    pub fn any(&self, label: &str) -> String {
        shown_any(label, self.edition, !self.hide_realms).into_owned()
    }

    /// [`Names::any`] with no realm whatever the option: the overlay's
    /// narrow recap.
    pub fn any_short(&self, label: &str) -> String {
        shown_any(label, self.edition, false).into_owned()
    }

    /// `rows` as the pane draws them: every label through [`Names::any`].
    pub fn rows(&self, rows: &[Row]) -> Vec<Row> {
        rows.iter()
            .map(|r| Row {
                label: self.any(&r.label),
                ..r.clone()
            })
            .collect()
    }
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
        let retail = Names::new(Some(Edition::Retail), true);
        assert_eq!(retail.name("Durgan-Nebula-US"), "Durgan");
        assert_eq!(retail.shown("Durgan-Nebula-US"), "Durgan");
        assert_eq!(
            retail.any("Word of Glory (Durgan-Nebula-US)"),
            "Word of Glory (Durgan)"
        );
        assert_eq!(retail.any("Yogg-Saron"), "Yogg-Saron");
        assert_eq!(retail.any("Blood-Queen Lana'thel"), "Blood-Queen Lana'thel");
        let shown = Names::new(Some(Edition::Retail), false);
        assert_eq!(shown.shown("Durgan-Nebula-US"), "Durgan-Nebula-US");
        assert_eq!(shown.any("Durgan-Nebula-US"), "Durgan-Nebula-US");
    }

    /// A Forever player is shown by both names, with or without the realm.
    #[test]
    fn a_forever_player_is_shown_by_both_names() {
        let hidden = Names::new(Some(Edition::Forever), true);
        assert_eq!(hidden.name("Ada-Brightwater-Nebula-US"), "Ada Brightwater");
        assert_eq!(hidden.shown("Ada-Brightwater-Nebula-US"), "Ada Brightwater");
        assert_eq!(
            hidden.any("Smite (Ada-Brightwater-Nebula-US)"),
            "Smite (Ada Brightwater)"
        );
        let shown = Names::new(Some(Edition::Forever), false);
        assert_eq!(
            shown.shown("Ada-Brightwater-Nebula-US"),
            "Ada Brightwater-Nebula-US"
        );
        assert_eq!(
            shown.any_short("Smite (Ada-Brightwater-Nebula-US)"),
            "Smite (Ada Brightwater)"
        );
    }

    #[test]
    fn a_state_with_nothing_watched_has_no_word() {
        assert_eq!(header_tag(&ClientState::new()), ("", Tone::None));
        assert_eq!(verdict(&ClientState::new()), None);
    }
}
