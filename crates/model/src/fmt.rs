//! Display formatting shared by every frontend, so the TUI and the GUI never
//! disagree on how a number or a clock reads.

use crate::View;

/// `12.3k`, `1.2M` — meter-style short numbers.
pub fn human(n: u64) -> String {
    const UNITS: [&str; 3] = ["k", "M", "B"];
    if n < 1_000 {
        return n.to_string();
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1_000.0 && unit < UNITS.len() {
        value /= 1_000.0;
        unit += 1;
    }
    // One decimal place can round 999.99k up to "1000.0k"; promote instead.
    if value >= 999.95 && unit < UNITS.len() {
        value /= 1_000.0;
        unit += 1;
    }
    format!("{value:.1}{}", UNITS[unit - 1])
}

/// `2:14`, `1:02:03`.
pub fn duration(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// R10: the keystone upgrade tier a key clock earns against the dungeon's
/// (par, +2, +3) timers — 3, 2 or 1 within time, 0 once past par.
pub fn key_tier(clock_ms: i64, pars_ms: (i64, i64, i64)) -> u8 {
    let (par, plus2, plus3) = pars_ms;
    if clock_ms <= plus3 {
        3
    } else if clock_ms <= plus2 {
        2
    } else if clock_ms <= par {
        1
    } else {
        0
    }
}

/// R10: the header badge for a keyed visit's Σ row: earned tier while the
/// run is within time ("+2", timed once resolved: "TIMED +2"), the overtime
/// amount once past par ("OVER +0:26"). `success`/`live` follow the row;
/// `clock_ms` is the key clock actually displayed next to the badge.
pub fn key_tag(clock_ms: i64, pars_ms: (i64, i64, i64), success: Option<bool>) -> String {
    match success {
        Some(true) => format!("TIMED +{}", key_tier(clock_ms, pars_ms)),
        // Depleted (final or live): how far past par, rounded up to the
        // whole second like the game's own "(26 sec overtime)" wording.
        Some(false) => {
            let over = (clock_ms - pars_ms.0).max(0);
            format!("OVER +{}", duration(over + (1000 - over % 1000) % 1000))
        }
        // In progress, within time: the tier this pace earns.
        None => format!("+{}", key_tier(clock_ms, pars_ms)),
    }
}

pub fn view_name(view: View) -> &'static str {
    match view {
        View::Damage => "Damage",
        View::Healing => "Healing",
        View::Interrupts => "Interrupts",
        View::CrowdControl => "Crowd Control",
        View::Dispels => "Dispels",
        View::Deaths => "Deaths",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_numbers_read_like_a_damage_meter() {
        assert_eq!(human(0), "0");
        assert_eq!(human(999), "999");
        assert_eq!(human(1_000), "1.0k");
        assert_eq!(human(12_345), "12.3k");
        assert_eq!(human(1_234_567), "1.2M");
        assert_eq!(human(2_500_000_000), "2.5B");
    }

    #[test]
    fn human_promotes_instead_of_rendering_1000_of_a_unit() {
        assert_eq!(human(999_999), "1.0M");
        assert_eq!(human(999_999_999), "1.0B");
    }

    #[test]
    fn durations_are_mm_ss_until_an_hour() {
        assert_eq!(duration(0), "0:00");
        assert_eq!(duration(9_000), "0:09");
        assert_eq!(duration(134_000), "2:14");
        assert_eq!(duration(3_599_000), "59:59");
        assert_eq!(duration(3_723_000), "1:02:03");
        assert_eq!(duration(-5), "0:00", "never render a negative clock");
    }

    #[test]
    fn every_view_has_a_name() {
        for (view, name) in [
            (View::Damage, "Damage"),
            (View::Healing, "Healing"),
            (View::Interrupts, "Interrupts"),
            (View::CrowdControl, "Crowd Control"),
            (View::Dispels, "Dispels"),
            (View::Deaths, "Deaths"),
        ] {
            assert_eq!(view_name(view), name);
        }
    }

    #[test]
    fn key_tier_walks_the_thresholds() {
        let pars = (2_040_000, 1_632_000, 1_224_000); // Magisters' Terrace
        assert_eq!(key_tier(1_224_000, pars), 3, "at +3 exactly");
        assert_eq!(key_tier(1_224_001, pars), 2);
        assert_eq!(key_tier(1_632_001, pars), 1);
        assert_eq!(key_tier(2_040_000, pars), 1, "at par exactly: timed");
        assert_eq!(key_tier(2_040_001, pars), 0);
    }

    #[test]
    fn key_tag_words_the_outcome() {
        let pars = (2_040_000, 1_632_000, 1_224_000);
        // The 34:25 Magisters' run: the game words 25.4s over as "26 sec".
        assert_eq!(key_tag(2_065_365, pars, Some(false)), "OVER +0:26");
        assert_eq!(key_tag(1_600_000, pars, Some(true)), "TIMED +2");
        assert_eq!(key_tag(1_000_000, pars, None), "+3", "live pace");
    }
}
