//! The inspector graph's lanes (the prototype's `.lanes`): a player's R18
//! spans and R12 item marks under the curve, sorted into four labelled
//! rows — cooldowns, items, externals, defensives — where the drill graph
//! washed them over the plot as full-height bands that drowned the curve
//! on a tank. Every span wears its CASTER's class colour, so an external
//! says who gave it: Heroism in Shaman blue, Rallying Cry in Warrior tan,
//! Power Word: Barrier in Priest white. A comparison shares each lane
//! between its two players ([`pair`]), so a gap in the curves can be read
//! off who pressed what, and when. Pure: marks in, lanes out.
//!
//! Window-only. The overlay's graph keeps its bands. Moved from the iced
//! window's inspector.

use crate::theme::Color;
use wowdps_model::{Class, Mark, MarkKind};

use super::Roster;

/// The four lanes, in the order they stand under the curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// A major offensive or healing cooldown's own buff.
    Cooldowns,
    /// Trinket uses and procs, potions and the rest of the consumables.
    Items,
    /// What someone else put on the player: the Bloodlust family, Power
    /// Infusion, a support buff, a defensive cast on them.
    Externals,
    /// What the player pressed to take less: their own defensives and a
    /// tank's active mitigation.
    Defensives,
}

impl Lane {
    pub const ALL: [Lane; 4] = [
        Lane::Cooldowns,
        Lane::Items,
        Lane::Externals,
        Lane::Defensives,
    ];

    /// The lane's label, beside it.
    pub fn label(self) -> &'static str {
        match self {
            Lane::Cooldowns => "Cooldowns",
            Lane::Items => "Items",
            Lane::Externals => "Externals",
            Lane::Defensives => "Defensives",
        }
    }

    /// Which lane a mark on `player`'s timeline stands in. A defensive is
    /// the player's own unless somebody else cast it — then it is an
    /// external (a Pain Suppression, an Ironbark). A death is no lane's:
    /// the plot hatches it.
    pub fn of(m: &Mark, player: &str) -> Option<Lane> {
        let theirs = !m.src.is_empty() && m.src != player;
        Some(match m.kind {
            MarkKind::Cooldown | MarkKind::HealingCooldown => Lane::Cooldowns,
            MarkKind::TrinketUse | MarkKind::TrinketProc | MarkKind::Consumable => Lane::Items,
            MarkKind::External | MarkKind::SupportBuff => Lane::Externals,
            MarkKind::Defensive if theirs => Lane::Externals,
            MarkKind::Defensive | MarkKind::ActiveMitigation => Lane::Defensives,
            MarkKind::Death => return None,
        })
    }
}

/// Who a span came from when the window never saw its caster on a meter.
const SOMEONE: &str = "someone else";

/// One span on a lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Milliseconds from the fight's start.
    pub at_ms: i64,
    /// How long it lasted; 0 is a moment (a trinket's use, a potion),
    /// drawn as a tick.
    pub dur_ms: i64,
    pub label: String,
    /// Who cast it, by name — `None` when it was the player's own.
    pub caster: Option<String>,
    /// The caster's class colour, the player's for their own.
    pub color: Color,
    /// In a comparison, whose timeline it is on — the first player's spans
    /// stand in the top half of the track, the second's (`second`) in the
    /// bottom — and their name, for the hover. `None` on one player's.
    pub whose: Option<String>,
    pub second: bool,
}

/// One lane and its spans, in time order.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub lane: Lane,
    pub spans: Vec<Span>,
}

/// The lanes of `player`'s marks — only the lanes something stands in,
/// in [`Lane::ALL`]'s order. `class` is the player's own, for what they
/// cast themselves; everyone else's is the `roster`'s, and a caster the
/// window has never seen on a meter is drawn `classless` (the theme's
/// grey). A mark the log wrote twice at one instant (the same label, the
/// same moment) is one span.
pub fn lanes(
    marks: &[Mark],
    player: &str,
    class: Option<Class>,
    roster: &Roster,
    classless: Color,
) -> Vec<Row> {
    let own = class.map_or(classless, Color::of_class);
    let mut rows: Vec<Row> = Vec::new();
    for lane in Lane::ALL {
        let mut spans: Vec<Span> = Vec::new();
        for m in marks.iter().filter(|m| Lane::of(m, player) == Some(lane)) {
            if spans
                .iter()
                .any(|s| s.at_ms == m.at_ms && s.label == m.label)
            {
                continue;
            }
            let (caster, color) = if m.src.is_empty() || m.src == player {
                (None, own)
            } else {
                match roster.get(&m.src) {
                    Some((name, who)) => (
                        Some(crate::labels::display_name(name).to_string()),
                        who.map_or(classless, Color::of_class),
                    ),
                    // A caster no meter named (a pet, someone never on
                    // screen): a guid's tail ("0A1B2C02") is no name a
                    // reader knows, so the hover says only that it was not
                    // the player.
                    None => (Some(SOMEONE.to_string()), classless),
                }
            };
            spans.push(Span {
                at_ms: m.at_ms,
                dur_ms: m.dur_ms.max(0),
                label: m.label.clone(),
                caster,
                color,
                whose: None,
                second: false,
            });
        }
        if !spans.is_empty() {
            spans.sort_by_key(|s| s.at_ms);
            rows.push(Row { lane, spans });
        }
    }
    rows
}

/// A comparison's lanes: `a`'s and `b`'s ([`lanes`] of each) sharing one
/// row per lane either of them has — `a`'s spans in the track's top half,
/// `b`'s in its bottom — so the two players' cooldowns and trinkets stand
/// one over the other on the fight's one clock.
pub fn pair(a: Vec<Row>, b: Vec<Row>, a_name: &str, b_name: &str) -> Vec<Row> {
    let mark = |rows: Vec<Row>, name: &str, second: bool| {
        rows.into_iter()
            .map(|r| Row {
                lane: r.lane,
                spans: r
                    .spans
                    .into_iter()
                    .map(|s| Span {
                        whose: Some(name.to_string()),
                        second,
                        ..s
                    })
                    .collect(),
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (mark(a, a_name, false), mark(b, b_name, true));
    Lane::ALL
        .into_iter()
        .filter_map(|lane| {
            let mut spans: Vec<Span> = a
                .iter()
                .chain(b.iter())
                .filter(|r| r.lane == lane)
                .flat_map(|r| r.spans.iter().cloned())
                .collect();
            spans.sort_by_key(|s| s.at_ms);
            (!spans.is_empty()).then_some(Row { lane, spans })
        })
        .collect()
}

/// What hovering a span says: its name, and "4:02, 40s, from Vingsham" —
/// when, how long (a moment has no length), and who, when it was not the
/// player; a comparison's leads with whose it is ("Swampert, 4:02, 40s").
pub fn span_words(s: &Span) -> (String, String) {
    let mut when = match &s.whose {
        Some(who) => format!("{who}, {}", crate::graph::mmss(s.at_ms.max(0) as u32)),
        None => crate::graph::mmss(s.at_ms.max(0) as u32),
    };
    if s.dur_ms >= 1000 {
        when.push_str(&format!(", {}s", s.dur_ms / 1000));
    }
    if let Some(who) = &s.caster {
        when.push_str(&format!(", from {who}"));
    }
    (s.label.clone(), when)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::Row as MeterRow;

    // `navy`'s classless grey.
    const CLASSLESS: Color = Color::rgb(0.42, 0.44, 0.52);

    fn mark(kind: MarkKind, label: &str, at_ms: i64, dur_ms: i64, src: &str) -> Mark {
        Mark {
            at_ms,
            kind,
            label: label.to_string(),
            spell_id: 0,
            dur_ms,
            src: src.to_string(),
        }
    }

    fn roster() -> Roster {
        let mut r = Roster::default();
        r.observe(&[
            MeterRow {
                key: "Player-Shaman".into(),
                label: "Vingsham-Proudmoore-US".into(),
                class: Some(Class::Shaman),
                ..MeterRow::default()
            },
            MeterRow {
                key: "Player-Priest".into(),
                label: "Eggtoast-Proudmoore-US".into(),
                class: Some(Class::Priest),
                ..MeterRow::default()
            },
        ]);
        r
    }

    /// Each kind goes to its lane, a defensive by who cast it, and a death
    /// to none; empty lanes are left out and the rest keep their order.
    #[test]
    fn marks_sort_into_the_four_lanes() {
        let me = "Player-Me";
        let marks = vec![
            mark(MarkKind::Defensive, "Unending Resolve", 9_000, 8_000, me),
            mark(
                MarkKind::Cooldown,
                "Summon Demonic Tyrant",
                20_000,
                15_000,
                me,
            ),
            mark(
                MarkKind::External,
                "Heroism",
                234_500,
                40_000,
                "Player-Shaman",
            ),
            mark(
                MarkKind::Defensive,
                "Pain Suppression",
                100_000,
                8_000,
                "Player-Priest",
            ),
            mark(MarkKind::TrinketUse, "Signet", 30_000, 0, me),
            mark(MarkKind::Consumable, "Healthstone", 50_000, 0, me),
            mark(
                MarkKind::ActiveMitigation,
                "Shield Block",
                60_000,
                6_000,
                "",
            ),
            mark(MarkKind::Death, "Death", 345_000, 10_000, ""),
        ];
        let rows = lanes(&marks, me, Some(Class::Warlock), &roster(), CLASSLESS);
        let lanes_of: Vec<Lane> = rows.iter().map(|r| r.lane).collect();
        assert_eq!(lanes_of, Lane::ALL.to_vec());
        let labels = |lane: Lane| -> Vec<String> {
            rows.iter()
                .find(|r| r.lane == lane)
                .map(|r| r.spans.iter().map(|s| s.label.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(labels(Lane::Cooldowns), ["Summon Demonic Tyrant"]);
        assert_eq!(labels(Lane::Items), ["Signet", "Healthstone"]);
        assert_eq!(labels(Lane::Externals), ["Pain Suppression", "Heroism"]);
        assert_eq!(
            labels(Lane::Defensives),
            ["Unending Resolve", "Shield Block"]
        );
        let only_items = lanes(&marks[4..5], me, None, &Roster::default(), CLASSLESS);
        assert_eq!(
            only_items.len(),
            1,
            "a lane with nothing in it is not drawn"
        );
        assert_eq!(only_items[0].lane, Lane::Items);
    }

    /// A span wears its caster's class colour, the player's own for what
    /// they cast (and for an item, whose mark names its owner), the
    /// classless grey for a caster the window never saw.
    #[test]
    fn spans_wear_their_caster_s_class() {
        let me = "Player-Me";
        let marks = vec![
            mark(
                MarkKind::External,
                "Heroism",
                1_000,
                40_000,
                "Player-Shaman",
            ),
            mark(
                MarkKind::External,
                "Power Infusion",
                2_000,
                15_000,
                "Player-Priest",
            ),
            mark(
                MarkKind::External,
                "Blessing",
                3_000,
                10_000,
                "Player-1168-0A1B2C02",
            ),
            mark(MarkKind::Cooldown, "Tyrant", 4_000, 15_000, me),
            mark(MarkKind::TrinketProc, "Proc", 5_000, 0, ""),
        ];
        let rows = lanes(&marks, me, Some(Class::Warlock), &roster(), CLASSLESS);
        let span = |label: &str| -> Span {
            rows.iter()
                .flat_map(|r| r.spans.iter())
                .find(|s| s.label == label)
                .cloned()
                .unwrap()
        };
        assert_eq!(span("Heroism").color, Color::of_class(Class::Shaman));
        assert_eq!(span("Heroism").caster.as_deref(), Some("Vingsham"));
        assert_eq!(span("Power Infusion").color, Color::of_class(Class::Priest));
        assert_eq!(span("Blessing").color, CLASSLESS);
        assert_eq!(
            span("Blessing").caster.as_deref(),
            Some("someone else"),
            "never the guid's tail"
        );
        assert_eq!(
            span_words(&span("Blessing")).1,
            "0:03, 10s, from someone else"
        );
        assert_eq!(span("Tyrant").color, Color::of_class(Class::Warlock));
        assert_eq!(span("Tyrant").caster, None, "the player's own names nobody");
        assert_eq!(span("Proc").color, Color::of_class(Class::Warlock));
    }

    /// The same mark twice at one instant is one span; the hover words it.
    #[test]
    fn a_doubled_mark_is_one_span_and_the_hover_words_it() {
        let me = "Player-Me";
        let marks = vec![
            mark(
                MarkKind::External,
                "Heroism",
                242_000,
                40_000,
                "Player-Shaman",
            ),
            mark(
                MarkKind::External,
                "Heroism",
                242_000,
                40_000,
                "Player-Shaman",
            ),
            mark(MarkKind::TrinketUse, "Signet", 61_000, 0, me),
        ];
        let rows = lanes(&marks, me, None, &roster(), CLASSLESS);
        let externals = rows.iter().find(|r| r.lane == Lane::Externals).unwrap();
        assert_eq!(externals.spans.len(), 1);
        assert_eq!(
            span_words(&externals.spans[0]),
            (
                "Heroism".to_string(),
                "4:02, 40s, from Vingsham".to_string()
            )
        );
        let items = rows.iter().find(|r| r.lane == Lane::Items).unwrap();
        assert_eq!(
            span_words(&items.spans[0]).1,
            "1:01",
            "a moment has no length"
        );
    }

    /// A comparison's lanes: one row per lane either player has, both
    /// players' spans on it in time order, each marked with whose it is —
    /// the second player's on the track's lower half — and the hover
    /// leading with the name.
    #[test]
    fn a_pair_shares_each_lane_between_the_two() {
        let (a, b) = ("Player-A", "Player-B");
        let a_rows = lanes(
            &[
                mark(MarkKind::Cooldown, "Tyrant", 20_000, 15_000, a),
                mark(MarkKind::TrinketUse, "Signet", 30_000, 0, a),
            ],
            a,
            Some(Class::Warlock),
            &roster(),
            CLASSLESS,
        );
        let b_rows = lanes(
            &[
                mark(MarkKind::Cooldown, "Avatar", 10_000, 20_000, b),
                mark(MarkKind::Defensive, "Die by the Sword", 50_000, 8_000, b),
            ],
            b,
            Some(Class::Warrior),
            &roster(),
            CLASSLESS,
        );
        let rows = pair(a_rows, b_rows, "Tranqlock", "Swampert");
        let lanes_of: Vec<Lane> = rows.iter().map(|r| r.lane).collect();
        assert_eq!(
            lanes_of,
            [Lane::Cooldowns, Lane::Items, Lane::Defensives],
            "either player's lanes, in the one order"
        );
        let cds = &rows[0].spans;
        assert_eq!(
            cds.iter()
                .map(|s| (s.label.as_str(), s.second))
                .collect::<Vec<_>>(),
            [("Avatar", true), ("Tyrant", false)],
            "in time order, each on its half"
        );
        assert_eq!(cds[0].color, Color::of_class(Class::Warrior));
        assert_eq!(
            span_words(&cds[1]).1,
            "Tranqlock, 0:20, 15s",
            "whose, then when"
        );
    }
}
