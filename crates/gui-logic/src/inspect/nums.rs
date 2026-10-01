//! The inspector's numbers (`.inum`) and the R17 mitigation line (`.mit`),
//! as words: a player's per view, an opened ability's with what R26's
//! tree adds (casts, misses, a DoT's uptime), and the record of what a
//! tank avoided. Moved from the iced window's inspector.

use wowdps_model::fmt::commas;
use wowdps_model::{Class, MissKind, Mitigation, Row, Spec, View};

use crate::fight_head::{Place, ordinal};
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
/// casts (0 when none were logged: a swing, a proc), its misses, and a
/// DoT's uptime over the fight.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tally {
    pub casts: u64,
    pub misses: u64,
    pub uptime_pct: Option<f64>,
}

impl Tally {
    pub fn of(m: &wowdps_model::SpellMeta, fight_ms: u32) -> Self {
        Tally {
            casts: m.casts,
            misses: m.misses,
            uptime_pct: (m.uptime_ms > 0 && fight_ms > 0)
                .then(|| (m.uptime_ms as f64 / f64::from(fight_ms) * 100.0).min(100.0)),
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
        View::Healing => vec![
            num("Hps", commas(me.per_sec.round() as u64), ""),
            num("Healing", figure(me.amount), ""),
            num("Overheal", pct(overheal_pct(me), 0), ""),
            place(),
        ],
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
    nums
}

/// R17's record as the line says it (`.mit`): what was mitigated of
/// everything swung, absorbed, blocked, prevented, staggered, and the
/// misses by kind.
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
