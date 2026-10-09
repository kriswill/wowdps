//! The inspector's numbers (`.inum`) and the R17 mitigation line (`.mit`),
//! as words: a player's per view, an opened ability's with what R26's
//! tree adds (casts, misses, a DoT's uptime), and the record of what a
//! tank avoided. Moved from the iced window's inspector.

use wowdps_model::fmt::commas;
use wowdps_model::{Class, Empower, EnergizeRow, MissKind, Mitigation, Row, Spec, View};

use crate::fight_head::{Place, ordinal};
use crate::inspect::power::power_label;
use crate::labels::window_view_name;
use crate::table::{figure, overheal_pct};

/// One number (`.inum`): its label, its value and a quieter tail
/// ("17th" + "of 19 dps").
#[derive(Debug, Clone, PartialEq)]
pub struct Num {
    pub label: String,
    pub value: String,
    pub small: String,
}

pub fn num(label: &str, value: String, small: &str) -> Num {
    Num {
        label: label.to_string(),
        value,
        small: small.to_string(),
    }
}

/// R26: what the ability tree adds to an opened ability's numbers — its
/// casts (0 when none were logged: a swing, a proc), its misses, a DoT's
/// uptime over the fight, and (v44) an empowered spell's stages.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tally {
    pub casts: u64,
    pub misses: u64,
    pub uptime_pct: Option<f64>,
    pub empower: Empower,
}

impl Tally {
    pub fn of(m: &wowdps_model::SpellMeta, fight_ms: u32) -> Self {
        Tally {
            casts: m.casts,
            misses: m.misses,
            uptime_pct: (m.uptime_ms > 0 && fight_ms > 0)
                .then(|| (m.uptime_ms as f64 / f64::from(fight_ms) * 100.0).min(100.0)),
            empower: m.empower,
        }
    }
}

/// "Demonology Warlock".
pub fn plays(class: Option<Class>, spec: Option<Spec>) -> Option<String> {
    match (spec, class) {
        (Some(s), _) => Some(format!("{} {}", s.name(), s.class().name())),
        (None, Some(c)) => Some(c.name().to_string()),
        (None, None) => None,
    }
}

/// What a running total of `view` is called: "Damage so far".
pub fn total_word(view: View) -> &'static str {
    match view {
        View::Healing => "Healing",
        View::Taken => "Taken",
        View::EnemyTaken => "Damage taken",
        _ => "Damage",
    }
}

/// The view's numbers for the player on `row` of `rows`.
pub fn player_nums(view: View, rows: &[Row], me: &Row) -> Vec<Num> {
    let place = || {
        let p = Place::of(rows, me);
        num("Rank", ordinal(p.place), &p.tail())
    };
    let pct = |v: f64, d: usize| format!("{v:.d$}%");
    match view {
        View::Damage => vec![
            num("Dps", commas(me.per_sec.round() as u64), ""),
            num("Damage", figure(me.amount), ""),
            place(),
            num("Crit", pct(me.crit_pct(), 1), ""),
        ],
        // Everyone's place among their own role, as the prototype's
        // `roleRank` has it: a healer's among the healers ("1st of 4
        // healers"), a dps's healing among the dps.
        View::Healing => {
            let mut nums = vec![
                num("Hps", commas(me.per_sec.round() as u64), ""),
                num("Healing", figure(me.amount), ""),
                num("Overheal", pct(overheal_pct(me), 0), ""),
                place(),
            ];
            // v44 (R2): what a heal-absorb ate of the healing, beside the
            // overheal — when anything did.
            if me.heal_absorbed > 0 {
                nums.insert(3, num("Heal absorbed", figure(me.heal_absorbed), ""));
            }
            nums
        }
        View::Taken => vec![
            num("Dtps", commas(me.per_sec.round() as u64), ""),
            num("Taken", figure(me.amount), ""),
            num("Absorbed", figure(me.extra), ""),
            num("Share", pct(me.pct, 1), ""),
        ],
        View::EnemyTaken => vec![
            num("Damage taken", figure(me.amount), ""),
            num("Per sec", figure(me.per_sec.round() as u64), ""),
            num("Share", pct(me.pct, 1), ""),
            num("Crit", pct(me.crit_pct(), 1), ""),
        ],
        View::Deaths | View::Interrupts | View::CrowdControl | View::Dispels => vec![
            num(window_view_name(view), commas(me.amount), ""),
            num("Share", pct(me.pct, 1), ""),
        ],
    }
}

/// An ability's figures, as the inspector's numbers say them (`.inum`):
/// its total, its share of the player, its hits, crit and average hit,
/// and what the view calls its extra (overkill, overheal, absorbed).
pub fn ability_nums(r: &Row, view: View, t: Tally) -> Vec<Num> {
    let known = |v: String| if r.count > 0 { v } else { "—".to_string() };
    let mut nums = vec![
        num("Total", figure(r.amount), ""),
        num("Share", format!("{:.1}%", r.pct), ""),
        num("Hits", commas(r.count), ""),
        num("Crit", known(format!("{:.1}%", r.crit_pct())), ""),
        num(
            "Avg",
            known(figure(r.amount.checked_div(r.count).unwrap_or(0))),
            "",
        ),
    ];
    // R26: what the casts did — how many, and each one's worth (every hit
    // a cast led to: a cleave's both targets, a DoT's every tick).
    if t.casts > 0 {
        nums.push(num("Casts", commas(t.casts), ""));
        nums.push(num("Avg cast", figure(r.amount / t.casts), ""));
    }
    // R26 (v44): how an empowered spell was released — the mean stage, and
    // the charges let go of before a release.
    if let Some(avg) = t.empower.stage_avg() {
        let cancelled = match t.empower.cancelled {
            0 => String::new(),
            n => format!("{n} cancelled"),
        };
        nums.push(num("Stage avg", format!("{avg:.1}"), &cancelled));
    }
    // R26 (step 3): the misses against the hits, and a DoT's uptime.
    if t.misses > 0 {
        let tries = r.count + t.misses;
        nums.push(num(
            "Miss",
            format!("{:.1}%", t.misses as f64 / tries as f64 * 100.0),
            "",
        ));
    }
    if let Some(up) = t.uptime_pct {
        nums.push(num("Uptime", format!("{up:.1}%"), ""));
    }
    if r.extra > 0 {
        let what = match view {
            View::Healing => "Overheal",
            View::Taken | View::EnemyTaken => "Absorbed",
            _ => "Overkill",
        };
        nums.push(num(what, figure(r.extra), ""));
    }
    // v44 (R2): a healing ability's part a heal-absorb ate.
    if view == View::Healing && r.heal_absorbed > 0 {
        nums.push(num("Heal absorbed", figure(r.heal_absorbed), ""));
    }
    nums
}

/// R27 (v44): the drilled player's resources as the inspector's head says
/// them — per power type what reached the pool and what the cap ate:
/// "Mana 1.2M gained, 40.0k wasted · Rage 820 gained". `None` when nothing
/// energized them.
pub fn energize_line(rows: &[EnergizeRow]) -> Option<String> {
    let amount = |v: f64| {
        if v < 1_000.0 {
            let s = format!("{v:.1}");
            s.strip_suffix(".0")
                .map_or_else(|| s.clone(), str::to_string)
        } else {
            figure(v.round() as u64)
        }
    };
    let parts: Vec<String> = rows
        .iter()
        .filter(|e| e.gained > 0.0 || e.wasted > 0.0)
        .map(|e| {
            let mut s = format!("{} {} gained", power_label(e.power_type), amount(e.gained));
            if e.wasted > 0.0 {
                s.push_str(&format!(", {} wasted", amount(e.wasted)));
            }
            s
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// R17's record as the line says it (`.mit`): what was mitigated of
/// everything swung, absorbed, blocked, prevented, reduced by armor and
/// damage reduction, staggered, and the misses by kind.
pub fn mit_pieces(m: &Mitigation, taken: u64) -> Vec<(String, String, String)> {
    let piece = |a: &str, b: String, c: &str| (a.to_string(), b, c.to_string());
    let mut out = vec![piece(
        "Mitigated",
        format!("{:.0}%", m.mitigated_pct(taken)),
        " of everything swung",
    )];
    out.push(piece("Absorbed", commas(m.absorbs()), ""));
    if m.blocked > 0 {
        out.push(piece("Blocked", commas(m.blocked), ""));
    }
    out.push(piece("Prevented", commas(m.prevented()), ""));
    // R17 amendment (v43): what armor and damage reduction took off.
    if m.reduced > 0 {
        out.push(piece("Reduced", commas(m.reduced), " by armor and DR"));
    }
    if m.stagger > 0 {
        out.push(piece("Staggered", commas(m.stagger), ""));
    }
    if m.misses() > 0 {
        let kinds: Vec<String> = MissKind::ALL
            .iter()
            .filter(|k| m.misses_of(**k) > 0)
            .map(|k| format!("{} {}", k.name(), m.misses_of(*k)))
            .collect();
        out.push(piece(
            "Misses",
            m.misses().to_string(),
            &format!(": {}", kinds.join(", ")),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_reduction_joins_the_mitigation_line_only_when_it_happened() {
        let mut m = Mitigation {
            absorbed: 10_000,
            ..Mitigation::default()
        };
        let labels = |m: &Mitigation, taken| -> Vec<String> {
            mit_pieces(m, taken).into_iter().map(|p| p.0).collect()
        };
        assert!(!labels(&m, 40_000).contains(&"Reduced".to_string()));
        // R17 amendment: 30 000 reduced joins `mitigated` and the swung
        // total alike — (10 000 + 30 000) of (40 000 + 30 000) is 57 %.
        m.reduced = 30_000;
        let pieces = mit_pieces(&m, 40_000);
        assert_eq!(pieces[0].1, "57%");
        assert!(
            pieces
                .iter()
                .any(|p| p.0 == "Reduced" && p.1 == "30,000" && p.2 == " by armor and DR"),
            "{pieces:?}"
        );
    }

    fn healer(heal_absorbed: u64) -> Row {
        Row {
            key: "Player-1-A".into(),
            label: "Ana".into(),
            amount: 2_000_000,
            extra: 500_000,
            count: 40,
            per_sec: 10_000.0,
            heal_absorbed,
            ..Row::default()
        }
    }

    /// v44 (R2): a healer whose healing a heal-absorb ate reads it beside
    /// the overheal; one whose nothing ate keeps the four numbers.
    #[test]
    fn heal_absorbed_stands_beside_the_overheal_when_any() {
        let me = healer(150_000);
        let labels = |nums: Vec<Num>| {
            nums.into_iter()
                .map(|n| (n.label, n.value))
                .collect::<Vec<_>>()
        };
        let nums = labels(player_nums(View::Healing, std::slice::from_ref(&me), &me));
        assert_eq!(nums[2].0, "Overheal");
        assert_eq!(nums[3], ("Heal absorbed".to_string(), "150.0k".to_string()));
        assert_eq!(nums.len(), 5);
        let none = healer(0);
        let nums = labels(player_nums(
            View::Healing,
            std::slice::from_ref(&none),
            &none,
        ));
        assert_eq!(nums.len(), 4);
        assert!(nums.iter().all(|n| n.0 != "Heal absorbed"));
        // An ability says its own part, on the healing view alone.
        let ability = labels(ability_nums(&me, View::Healing, Tally::default()));
        assert!(ability.contains(&("Heal absorbed".to_string(), "150.0k".to_string())));
        let ability = labels(ability_nums(&me, View::Damage, Tally::default()));
        assert!(ability.iter().all(|n| n.0 != "Heal absorbed"));
    }

    /// R26 (v44): an empowered ability's numbers say its mean stage and
    /// its cancels among the casts; any other ability says neither.
    #[test]
    fn an_empowered_ability_says_its_stage_avg_and_cancels() {
        let r = Row {
            amount: 90_000,
            count: 6,
            ..Row::default()
        };
        let t = Tally {
            casts: 5,
            empower: Empower {
                stages: [0, 1, 3, 1],
                cancelled: 3,
            },
            ..Tally::default()
        };
        let nums = ability_nums(&r, View::Damage, t);
        let stage = nums
            .iter()
            .find(|n| n.label == "Stage avg")
            .expect("the stage");
        assert_eq!(
            (stage.value.as_str(), stage.small.as_str()),
            ("3.0", "3 cancelled")
        );
        let at = |label: &str| nums.iter().position(|n| n.label == label);
        assert!(at("Avg cast") < at("Stage avg"), "among the casts");
        let t = Tally {
            empower: Empower {
                stages: [1, 1, 0, 0],
                cancelled: 0,
            },
            ..t
        };
        let stage = ability_nums(&r, View::Damage, t)
            .into_iter()
            .find(|n| n.label == "Stage avg")
            .expect("the stage");
        assert_eq!((stage.value.as_str(), stage.small.as_str()), ("1.5", ""));
        let cancels_alone = Tally {
            empower: Empower {
                stages: [0; 4],
                cancelled: 2,
            },
            ..Tally::default()
        };
        assert!(
            ability_nums(&r, View::Damage, cancels_alone)
                .iter()
                .all(|n| n.label != "Stage avg"),
            "no release, no stage to average"
        );
        assert!(
            ability_nums(&r, View::Damage, Tally::default())
                .iter()
                .all(|n| n.label != "Stage avg")
        );
    }

    /// R27 (v44): the head's resource line — per power type the gain and
    /// what the cap ate, fractions kept small, nothing when nothing came.
    #[test]
    fn the_energize_line_names_each_pool_gained_and_wasted() {
        let row = |power_type: u32, gained: f64, wasted: f64| EnergizeRow {
            power_type,
            gained,
            wasted,
            count: 1,
        };
        assert_eq!(
            energize_line(&[row(0, 1_200_000.0, 40_000.0), row(1, 820.0, 0.0)]).as_deref(),
            Some("Mana 1.2M gained, 40.0k wasted · Rage 820 gained")
        );
        assert_eq!(
            energize_line(&[row(7, 1.5, 1.0)]).as_deref(),
            Some("Soul shards 1.5 gained, 1 wasted")
        );
        assert_eq!(energize_line(&[]), None);
        assert_eq!(
            energize_line(&[row(3, 0.0, 0.0)]),
            None,
            "a pool nothing reached"
        );
    }
}
