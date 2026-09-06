//! Display formatting shared by every frontend, so the TUI and the GUI never
//! disagree on how a number or a clock reads.

use crate::{MissKind, Mitigation, View};

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
    // Both loops above run at least once and stop at UNITS.len(), so `unit`
    // is always in 1..=UNITS.len(); the fallback is the saturation suffix.
    let suffix = UNITS.get(unit.saturating_sub(1)).copied().unwrap_or("B");
    format!("{value:.1}{suffix}")
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
        View::Taken => "Taken",
    }
}

/// R17: one line under a Taken drill — the mitigated share, then only the
/// amounts and miss kinds that happened. Shared by the TUI and GUI so the
/// two renderers can never word it differently.
pub fn mitigation_line(m: &Mitigation, taken: u64) -> String {
    let mut parts = vec![format!("mitigated {:.0}%", m.mitigated_pct(taken))];
    for (name, n) in [
        ("absorbed", m.absorbed),
        ("blocked", m.blocked),
        ("prevented", m.absorbed_full + m.blocked_full),
        ("stagger", m.stagger),
    ] {
        if n > 0 {
            parts.push(format!("{name} {}", human(n)));
        }
    }
    if m.misses() > 0 {
        let kinds: Vec<String> = MissKind::ALL
            .iter()
            .filter(|k| m.misses_of(**k) > 0)
            .map(|k| format!("{} {}", k.name(), m.misses_of(*k)))
            .collect();
        parts.push(format!("misses {} ({})", m.misses(), kinds.join(" ")));
    }
    parts.join(" · ")
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
            (View::Taken, "Taken"),
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

    #[test]
    fn the_mitigation_line_lists_only_what_happened() {
        let mut m = Mitigation::default();
        assert_eq!(mitigation_line(&m, 0), "mitigated 0%");
        m.absorbed = 12_000;
        m.blocked = 18_000;
        m.blocked_full = 55_000;
        for k in [
            MissKind::Dodge,
            MissKind::Parry,
            MissKind::Block,
            MissKind::Miss,
            MissKind::Immune,
        ] {
            m.miss(k);
        }
        assert_eq!(
            mitigation_line(&m, 84_000),
            "mitigated 61% · absorbed 12.0k · blocked 18.0k · prevented 55.0k · \
             misses 5 (dodge 1 parry 1 block 1 miss 1 immune 1)"
        );
        let stagger = Mitigation {
            absorbed: 25_000,
            stagger: 25_000,
            stagger_ticked: 10_000,
            ..Mitigation::default()
        };
        assert_eq!(
            mitigation_line(&stagger, 70_200),
            "mitigated 36% · absorbed 25.0k · stagger 25.0k"
        );
    }
}
