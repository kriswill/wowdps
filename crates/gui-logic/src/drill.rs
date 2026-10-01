//! A drill's words and the overlay drill's grid, free of any GUI: the rate
//! a view is read in, the R17 mitigation line, a school's name, the
//! ability drill's stat cards, a death recap's label and health, and the
//! column widths the overlay's drill rows and their caption share.

use wowdps_model::fmt::{human, mitigation_line};
use wowdps_model::{Row, View};
use wowdps_proto::ClientState;

/// Column widths shared by the overlay drilldown rows and their caption
/// line, so the numbers sit under their headings: (hits, crit%, total), px
/// at zoom 1.
pub const OVERLAY_DRILL_COLS: (f32, f32, f32) = (40.0, 40.0, 48.0);

/// R26: the fold caret's column on every line of a tree drill, px at zoom 1.
pub const OVERLAY_CARET_W: f32 = 8.0;

/// How a view words its per-second rate: `dps`, `hps`, or `dtps` for damage
/// taken (R17). Count views never show one; they read `dps` here only
/// because nothing asks them.
pub fn rate_label(view: View) -> &'static str {
    match view {
        View::Healing => "hps",
        View::Taken | View::EnemyTaken => "dtps",
        _ => "dps",
    }
}

/// A view whose rows count events rather than sum amounts: they cannot
/// crit, and their total IS the count, so one column says it all.
pub fn counts(view: View) -> bool {
    matches!(view, View::Interrupts | View::CrowdControl | View::Dispels)
}

/// R17: the drilled player's mitigation record as one line, when the view
/// is Taken and the daemon sent one. Shared by the window and the overlay.
pub fn drill_mitigation_line(app: &ClientState) -> Option<String> {
    if app.view != View::Taken {
        return None;
    }
    let drill = app.drill.as_ref()?;
    let m = app.drill_mitigation()?;
    let taken = app
        .rows()
        .iter()
        .find(|r| r.key == drill.key)
        .map_or(0, |r| r.amount);
    Some(mitigation_line(m, taken))
}

/// v17: the game's name for a school bitmask — the singles, the named
/// combos players actually see, and a component join for the rest.
pub fn school_name(mask: u32) -> Option<String> {
    let named = match mask {
        0x01 => Some("Physical"),
        0x02 => Some("Holy"),
        0x04 => Some("Fire"),
        0x08 => Some("Nature"),
        0x10 => Some("Frost"),
        0x20 => Some("Shadow"),
        0x40 => Some("Arcane"),
        0x06 => Some("Radiant"),
        0x0C => Some("Volcanic"),
        0x14 => Some("Frostfire"),
        0x18 => Some("Froststorm"),
        0x22 => Some("Twilight"),
        0x24 => Some("Shadowflame"),
        0x28 => Some("Plague"),
        0x30 => Some("Shadowfrost"),
        0x44 => Some("Spellfire"),
        0x48 => Some("Astral"),
        0x50 => Some("Spellfrost"),
        0x60 => Some("Spellshadow"),
        0x7C => Some("Elemental"),
        0x7E => Some("Chromatic"),
        0x7F => Some("Chaos"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_string());
    }
    let parts: Vec<&str> = [
        (0x01, "Physical"),
        (0x02, "Holy"),
        (0x04, "Fire"),
        (0x08, "Nature"),
        (0x10, "Frost"),
        (0x20, "Shadow"),
        (0x40, "Arcane"),
    ]
    .iter()
    .filter(|(bit, _)| mask & bit != 0)
    .map(|(_, n)| *n)
    .collect();
    (!parts.is_empty()).then(|| parts.join("+"))
}

/// What a stat card's value wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatTone {
    /// The surface's ink.
    Plain,
    /// The surface's crit colour.
    Crit,
    /// Bad news: overkill, overheal, absorbed.
    Bad,
}

/// One card of the ability drill's stat strip: its caption, its value and
/// what the value wears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatCard {
    pub label: &'static str,
    pub value: String,
    pub tone: StatTone,
}

/// v16: the ability drill's stat strip — the numbers its by-spell row
/// already carried but the table never showed: total, share of the player,
/// hits, crit rate, average hit, and the view's `extra` (overkill /
/// overheal / absorbed) when there is any.
pub fn stat_cards(r: &Row, view: View) -> Vec<StatCard> {
    let card = |label, value, tone| StatCard { label, value, tone };
    let avg = match r.amount.checked_div(r.count) {
        Some(v) if r.count > 0 => human(v),
        _ => "—".to_string(),
    };
    let crit = if r.count > 0 {
        format!("{:.0}%", r.crit_pct())
    } else {
        "—".to_string()
    };
    let mut cards = vec![
        card("total", human(r.amount), StatTone::Plain),
        card("share", format!("{:.1}%", r.pct), StatTone::Plain),
        card("hits", human(r.count), StatTone::Plain),
        card("crit", crit, StatTone::Crit),
        card("avg", avg, StatTone::Plain),
    ];
    if r.extra > 0 {
        let what = match view {
            View::Healing => "overheal",
            View::Taken | View::EnemyTaken => "absorbed",
            _ => "overkill",
        };
        cards.push(card(what, human(r.extra), StatTone::Bad));
    }
    cards
}

/// A recap line's label for the overlay's narrow panel: the attacker or
/// healer in parens loses its realm, as the meter rows' names do.
pub fn compact_recap_label(label: &str) -> String {
    match label.split_once(" (") {
        Some((head, tail)) => {
            let who = tail.trim_end_matches(')');
            let short = who.split('-').next().unwrap_or(who);
            format!("{head} ({short})")
        }
        None => label.to_string(),
    }
}

/// A recap line's health after the event, as a whole percent; empty when
/// none was reported.
pub fn recap_hp(r: &Row) -> String {
    r.hp.map(|(cur, max_hp)| format!("{:.0}%", cur as f64 / max_hp.max(1) as f64 * 100.0))
        .unwrap_or_default()
}

/// A recap line's amount, signed when it is a gain.
pub fn recap_amount(r: &Row) -> String {
    let sign = if r.gain { "+" } else { "" };
    format!("{sign}{}", human(r.amount))
}

/// `part` of `whole` in whole percent, 0..=100 — a recap bar's length.
pub fn whole_pct(part: u64, whole: u64) -> u16 {
    (part as f64 / whole.max(1) as f64 * 100.0)
        .clamp(0.0, 100.0)
        .round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_label_follows_the_view() {
        assert_eq!(rate_label(View::Taken), "dtps");
        assert_eq!(rate_label(View::Healing), "hps");
        assert_eq!(rate_label(View::Damage), "dps");
        assert!(counts(View::Interrupts) && counts(View::Dispels));
        assert!(!counts(View::Damage) && !counts(View::Deaths));
    }

    #[test]
    fn school_names_cover_singles_combos_and_joins() {
        assert_eq!(school_name(0x01).as_deref(), Some("Physical"));
        assert_eq!(school_name(0x40).as_deref(), Some("Arcane"));
        assert_eq!(school_name(0x24).as_deref(), Some("Shadowflame"));
        assert_eq!(school_name(0x7F).as_deref(), Some("Chaos"));
        assert_eq!(school_name(0x41).as_deref(), Some("Physical+Arcane"));
        assert_eq!(school_name(0x23).as_deref(), Some("Physical+Holy+Shadow"));
        assert_eq!(school_name(0), None);
        assert_eq!(school_name(0x80), None);
        // The game's own combo names, every one.
        for (mask, name) in [
            (0x02, "Holy"),
            (0x04, "Fire"),
            (0x08, "Nature"),
            (0x10, "Frost"),
            (0x20, "Shadow"),
            (0x06, "Radiant"),
            (0x0C, "Volcanic"),
            (0x14, "Frostfire"),
            (0x18, "Froststorm"),
            (0x22, "Twilight"),
            (0x28, "Plague"),
            (0x30, "Shadowfrost"),
            (0x44, "Spellfire"),
            (0x48, "Astral"),
            (0x50, "Spellfrost"),
            (0x60, "Spellshadow"),
            (0x7C, "Elemental"),
            (0x7E, "Chromatic"),
        ] {
            assert_eq!(school_name(mask).as_deref(), Some(name), "{mask:#x}");
        }
    }

    #[test]
    fn stat_cards_word_the_row_and_the_views_extra() {
        let r = Row {
            amount: 9_000,
            count: 3,
            crits: 1,
            pct: 12.5,
            extra: 400,
            ..Row::default()
        };
        let cards = stat_cards(&r, View::Healing);
        let words: Vec<(&str, &str)> = cards.iter().map(|c| (c.label, c.value.as_str())).collect();
        assert_eq!(
            words,
            [
                ("total", "9.0k"),
                ("share", "12.5%"),
                ("hits", "3"),
                ("crit", "33%"),
                ("avg", "3.0k"),
                ("overheal", "400"),
            ]
        );
        assert_eq!(cards[3].tone, StatTone::Crit);
        assert_eq!(cards[5].tone, StatTone::Bad);
        assert_eq!(stat_cards(&r, View::Taken)[5].label, "absorbed");
        assert_eq!(stat_cards(&r, View::Damage)[5].label, "overkill");
        let none = stat_cards(&Row::default(), View::Damage);
        assert_eq!(none.len(), 5, "no extra, no card");
        assert_eq!((none[3].value.as_str(), none[4].value.as_str()), ("—", "—"));
    }

    #[test]
    fn a_recap_line_words_its_source_amount_and_health() {
        assert_eq!(
            compact_recap_label("Melee (Grunk-Proudmoore-US)"),
            "Melee (Grunk)"
        );
        assert_eq!(compact_recap_label("Fall"), "Fall");
        let hit = Row {
            amount: 1_500,
            hp: Some((250, 1_000)),
            ..Row::default()
        };
        assert_eq!(recap_hp(&hit), "25%");
        assert_eq!(recap_amount(&hit), "1.5k");
        let heal = Row {
            gain: true,
            hp: None,
            ..hit
        };
        assert_eq!(recap_amount(&heal), "+1.5k");
        assert_eq!(recap_hp(&heal), "");
        assert_eq!(whole_pct(1, 3), 33);
        assert_eq!(whole_pct(5, 0), 100);
    }
}
