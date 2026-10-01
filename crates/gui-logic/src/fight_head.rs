//! What the window's fight header says (the prototype's `.fhead`): the
//! difficulty and size after a title, the outcome badge's word, the stat
//! line's figures per view, the "you" chip's words and the owner's place in
//! their role — and `Seen`, what the window remembers of the owner from the
//! other views of the fight on screen. Words and numbers only; how they are
//! laid out and coloured is each GUI's.
//!
//! The deaths come from the snapshot's raid timeline (R25, v35), which
//! answers for the whole fight on every view: "Deaths 6" on the Damage and
//! Enemies lines, "First 1:10" and "Battle rezzes 2" on Deaths, and the
//! chip's "died 5:45". Only the group's deaths count: an arena's other team
//! dies in the timeline, flagged `enemy`, and a self-rez is no battle rez.

use wowdps_model::fmt::{commas, duration, key_tier};
use wowdps_model::{
    Class, Encounter, RaidTimeline, Role, Row, SegmentId, SegmentKind, Spec, View, difficulty_name,
};
use wowdps_proto::ClientState;

use crate::labels::{Tone, plural, rate_label, window_view_name};

/// What the title line says before there is a fight.
pub const WAITING: &str = "waiting for combat…";
/// What it says while the history store answers for a stored pull.
pub const READING: &str = "reading the stored pull…";
/// What it says of a stored pull the store did not answer for.
pub const GONE: &str = "not in the history store";

/// The step buttons' tooltips: what they do, and the key that does it.
pub const OLDER_TIP: &str = "Older pull ( [ )";
pub const NEWER_TIP: &str = "Newer pull ( ] )";
/// What the rail's button on the title line does, and the key that closes
/// what it opens.
pub const RAIL_TIP: &str = "Pulls (Esc closes)";

/// The stat line: the view's figures, and the owner's chip. Both empty
/// while the view's answer is on its way ([`pending`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub pairs: Vec<Pair>,
    pub you: Option<You>,
}

/// One label and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub label: String,
    pub value: String,
}

/// The "you" chip: the owner as the chart knows them, and what the view
/// says of them.
#[derive(Debug, Clone, PartialEq)]
pub struct You {
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    /// "17th of 19 dps", "149,258 hps", "3 interrupts", "died", "survived".
    pub words: String,
    /// The figure after the words, in secondary ink: the rate beside a
    /// place.
    pub figure: Option<String>,
    /// The owner has a row on this view for a press to select. On Deaths
    /// ("survived") and a count view ("no interrupts") the chip speaks from
    /// what another view said (`Seen`), and there is no row to go to: no
    /// press, no tooltip, no hand.
    pub selectable: bool,
}

/// "Heroic, 25 players": the encounter's difficulty as the game names it
/// and how many were in it. Empty off a boss pull — trash, a visit's Σ, an
/// arena — which has no ENCOUNTER_START to say it.
pub fn meta(encounter: Option<Encounter>) -> String {
    let Some(e) = encounter else {
        return String::new();
    };
    let size = (e.group_size > 0).then(|| plural(e.group_size as usize, "player"));
    match (difficulty_name(e.difficulty), size) {
        (Some(d), Some(s)) => format!("{d}, {s}"),
        (Some(d), None) => d.to_string(),
        (None, Some(s)) => s,
        (None, None) => String::new(),
    }
}

/// What an outcome is worded from: the watched segment's verdict, and what
/// kind of segment it is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Verdict {
    pub live: bool,
    pub kind: Option<SegmentKind>,
    pub success: Option<bool>,
    /// R13: an arena match, won or lost.
    pub arena: bool,
    /// R10: a keyed visit's (par, +2, +3) timers.
    pub pars_ms: Option<(i64, i64, i64)>,
    pub duration_ms: i64,
    /// R16: how close a wipe came, when something said. The live meter's
    /// snapshot does not carry it yet — a stored card does.
    pub wipe_pct: Option<u16>,
}

impl Verdict {
    pub fn of(app: &ClientState) -> Self {
        Verdict {
            live: app.is_live(),
            kind: app.segment_kind(),
            success: app.segment_success(),
            arena: app.segment_arena(),
            pars_ms: app.segment_pars_ms(),
            duration_ms: app.duration_ms(),
            wipe_pct: None,
        }
    }
}

/// The outcome badge's word (`.badge`) and what it means: "Live" while the
/// pull goes; Kill, Win and "Timed +2" (the key's upgrade, R10) good;
/// "Wipe at 56%", Loss and "Over time" bad. `None` when there is nothing to
/// say: trash, a raid visit's Σ, a pull the log cut off before its end.
pub fn outcome(v: Verdict) -> Option<(String, Tone)> {
    if v.live {
        return Some(("Live".to_string(), Tone::Live));
    }
    let good = v.success?;
    let word = match (v.kind, good) {
        (Some(SegmentKind::Overall), true) => match v.pars_ms {
            Some(pars) => format!("Timed +{}", key_tier(v.duration_ms, pars)),
            None => "Timed".to_string(),
        },
        (Some(SegmentKind::Overall), false) => "Over time".to_string(),
        (_, true) if v.arena => "Win".to_string(),
        (_, false) if v.arena => "Loss".to_string(),
        (_, true) => "Kill".to_string(),
        (_, false) => match v.wipe_pct {
            Some(pct) => format!("Wipe at {pct}%"),
            None => "Wipe".to_string(),
        },
    };
    Some((word, if good { Tone::Good } else { Tone::Bad }))
}

/// The view's answer is not in yet: the rows in hand are another view's or
/// a placeholder's, and a "Raid hps 0" or a "survived" drawn from them would
/// be the confident false figure the stat line never shows.
pub fn pending(app: &ClientState) -> bool {
    !app.view_answered() || loading(app)
}

/// The stat line's figures for `view`, folded from its rows — OUR side's:
/// an arena's enemy team (R13) is on the chart, not in the fold — and,
/// from the raid timeline (R25, v35), the deaths: their count on Damage and
/// Enemies, and on Deaths the first one's time and the battle rezzes, as
/// the prototype's `statLine()` has them. The timeline answers for the
/// whole fight on every view, so the line says the same of it wherever
/// the reader has been; without one (a card-only stored pull) it says less.
pub fn pairs(view: View, rows: &[Row], raid: Option<&RaidTimeline>) -> Vec<Pair> {
    let ours: Vec<&Row> = rows.iter().filter(|r| !r.enemy).collect();
    let total: u64 = ours.iter().map(|r| r.amount).sum();
    let extra: u64 = ours.iter().map(|r| r.extra).sum();
    let rate: f64 = ours.iter().map(|r| r.per_sec).sum();
    let pair = |label: &str, value: String| Pair {
        label: label.to_string(),
        value,
    };
    // The group's own deaths: an arena's other team (R13) dies in the
    // timeline too, and is never counted as ours.
    let dead: Option<Vec<&wowdps_model::RaidDeath>> =
        raid.map(|r| r.deaths.iter().filter(|d| !d.enemy).collect());
    let deaths = dead.as_ref().map(|d| pair("Deaths", d.len().to_string()));
    let rate = pair(
        &format!("Raid {}", rate_label(view)),
        commas(rate.round() as u64),
    );
    match view {
        View::Damage => [Some(rate), Some(pair("Damage", commas(total))), deaths]
            .into_iter()
            .flatten()
            .collect(),
        View::Healing => {
            let mut line = vec![rate, pair("Healing", commas(total))];
            let fold = Row {
                amount: total,
                extra,
                ..Row::default()
            };
            if total + extra > 0 {
                line.push(pair(
                    "Overheal",
                    format!("{:.1}%", crate::table::overheal_pct(&fold)),
                ));
            }
            line
        }
        View::Taken => vec![
            rate,
            pair("Taken", commas(total)),
            pair("Absorbed", commas(extra)),
        ],
        // "Raid dtps": what the enemies took, a second — the prototype's
        // words beside "Damage to enemies".
        View::EnemyTaken => [
            Some(rate),
            Some(pair("Damage to enemies", commas(total))),
            deaths,
        ]
        .into_iter()
        .flatten()
        .collect(),
        // The deaths in the order they happened: how many, when the first
        // came, and how many a rez undid — or, with no timeline, the rows'
        // count alone.
        View::Deaths => match &dead {
            Some(dead) if !dead.is_empty() => vec![
                pair("Deaths", dead.len().to_string()),
                pair(
                    "First",
                    duration(dead.iter().map(|d| d.at_ms).min().unwrap_or(0)),
                ),
                // Someone else raised them: a self-rez (Reincarnation) is
                // no battle rez.
                pair(
                    "Battle rezzes",
                    dead.iter()
                        .filter(|d| d.battle_rezzed())
                        .count()
                        .to_string(),
                ),
            ],
            _ => vec![pair("Deaths", commas(total))],
        },
        View::Interrupts | View::CrowdControl | View::Dispels => vec![
            pair(window_view_name(view), commas(total)),
            pair("Players", ours.len().to_string()),
        ],
    }
}

/// What the chip says of the owner on `view`, their row being `owner` of
/// `rows`: on Damage their place among their own ROLE — a healer against
/// healers, what the history store grades by — and their rate after it;
/// on Healing and Taken the rate; on a count view the count and its noun
/// ("3 interrupts"), or "no interrupts" when the window saw them in this
/// fight (`seen`, their row on another view) and they have no row here;
/// on Deaths "died 5:45" — when, from the raid timeline (`died_at`, R25),
/// else "died" — or "died 3 times", or — seen, and not among the dead —
/// "survived" once the fight is over and "alive" while it is `live`. `None`
/// is no chip: the owner is not known to be in this fight, or — on the
/// Enemies view — the rows are the enemies.
pub fn you(
    view: View,
    rows: &[Row],
    owner: Option<usize>,
    seen: Option<&Row>,
    live: bool,
    died_at: Option<i64>,
) -> Option<You> {
    if view == View::EnemyTaken {
        return None;
    }
    let me = owner.and_then(|i| rows.get(i));
    // Selectable when it names a row on this chart; a chip made from what
    // another view said has none to go to.
    let chip = |r: &Row, words: String, figure: Option<String>| You {
        name: r.label.clone(),
        class: r.class,
        spec: r.spec,
        words,
        figure,
        selectable: me.is_some(),
    };
    match (view, me) {
        (View::Deaths, Some(me)) => Some(chip(
            me,
            match (me.amount, died_at) {
                (0 | 1, Some(at)) => format!("died {}", duration(at)),
                (0 | 1, None) => "died".to_string(),
                (n, _) => format!("died {n} times"),
            },
            None,
        )),
        (View::Deaths, None) => seen.map(|me| {
            let words = if live { "alive" } else { "survived" };
            chip(me, words.to_string(), None)
        }),
        (View::Interrupts | View::CrowdControl | View::Dispels, None) => {
            seen.map(|me| chip(me, format!("no {}", count_noun(view, 0)), None))
        }
        (_, None) => None,
        (View::Damage, Some(me)) => {
            let place = Place::of(rows, me);
            Some(chip(
                me,
                place.words(),
                Some(commas(me.per_sec.round() as u64)),
            ))
        }
        (View::Healing | View::Taken, Some(me)) => Some(chip(
            me,
            format!("{} {}", commas(me.per_sec.round() as u64), rate_label(view)),
            None,
        )),
        (_, Some(me)) => Some(chip(
            me,
            format!("{} {}", commas(me.amount), count_noun(view, me.amount)),
            None,
        )),
    }
}

/// Where a player stands among their own ROLE on a chart — a healer
/// against healers, what the history store grades by: "17th of 19 dps".
/// One reckoning for the chip and the inspector, so the two never
/// disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub place: usize,
    pub of: usize,
    /// "dps", "healers", "tank"; `None` when their spec (and so their
    /// role) is not known, and the place is among everyone on our side.
    pub noun: Option<&'static str>,
}

impl Place {
    /// `me`'s place among our side's rows of their role on `rows`.
    pub fn of(rows: &[Row], me: &Row) -> Self {
        let role = me.spec.map(Spec::role);
        // Everyone on our side who plays their role; with no spec known,
        // everyone on our side.
        let peers: Vec<&Row> = rows
            .iter()
            .filter(|r| !r.enemy && (role.is_none() || r.spec.map(Spec::role) == role))
            .collect();
        Place {
            place: peers.iter().filter(|r| r.amount > me.amount).count() + 1,
            of: peers.len(),
            noun: role_noun(role, peers.len()),
        }
    }

    /// "17th of 19 dps".
    pub fn words(self) -> String {
        format!("{} {}", ordinal(self.place), self.tail())
    }

    /// What follows the ordinal: "of 19 dps".
    pub fn tail(self) -> String {
        match self.noun {
            Some(noun) => format!("of {} {noun}", self.of),
            None => format!("of {}", self.of),
        }
    }
}

/// What a count view counts, agreeing with `n`: "interrupt(s)",
/// "dispel(s)" — and "crowd control", which has no plural.
pub fn count_noun(view: View, n: u64) -> &'static str {
    match (view, n) {
        (View::Interrupts, 1) => "interrupt",
        (View::Interrupts, _) => "interrupts",
        (View::Dispels, 1) => "dispel",
        (View::Dispels, _) => "dispels",
        _ => "crowd control",
    }
}

/// A role as the chip counts `n` of its members: "dps", "healers",
/// "tanks" — "healer" and "tank" when there is one.
fn role_noun(role: Option<Role>, n: usize) -> Option<&'static str> {
    role.map(|r| match (r, n) {
        (Role::Dps, _) => "dps",
        (Role::Healer, 1) => "healer",
        (Role::Healer, _) => "healers",
        (Role::Tank, 1) => "tank",
        (Role::Tank, _) => "tanks",
    })
}

/// 1st, 2nd, 3rd, 4th … 11th, 12th, 13th … 21st, 22nd.
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A row label names the owner by one of their configured names: the
/// "Name-Realm" whole, or a bare "Name" matching its name half — the way
/// the daemon reads `history_characters`. Case aside.
pub fn is_named(label: &str, name: &str) -> bool {
    let (label, name) = (label.to_lowercase(), name.to_lowercase());
    label == name
        || label
            .strip_prefix(&name)
            .is_some_and(|rest| rest.starts_with('-'))
}

/// The owner's row among `rows` (our side's, never an enemy's), by the
/// most certain thing that names it, over every row before a less certain
/// one is asked: the locked character's `guid`, then one of `names` whole
/// ("Name-Realm", case aside), then a bare name by its name half — and a
/// bare name only when it names ONE row, since a namesake from another
/// realm who out-ranks the owner would otherwise wear their tag, their
/// chip and their chrome.
pub fn owner_among(rows: &[Row], guid: Option<&str>, names: &[String]) -> Option<usize> {
    let ours = || rows.iter().enumerate().filter(|(_, r)| !r.enemy);
    if let Some(guid) = guid
        && let Some((i, _)) = ours().find(|(_, r)| r.key == guid)
    {
        return Some(i);
    }
    if let Some((i, _)) = ours().find(|(_, r)| {
        names
            .iter()
            .any(|n| r.label.to_lowercase() == n.to_lowercase())
    }) {
        return Some(i);
    }
    let mut bare = ours().filter(|(_, r)| names.iter().any(|n| is_named(&r.label, n)));
    match (bare.next(), bare.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

// ---- what other views said -------------------------------------------------

/// What the window has seen of the watched fight on views other than the
/// one on screen: the owner's row, as it last stood — what lets the Deaths
/// view say "survived" and a count view "no interrupts" rather than
/// nothing, since a snapshot carries its own view's rows only. Held for one
/// fight, begun again with the next; never taken from a loading
/// placeholder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    fight: Option<SegmentId>,
    /// The owner's row as it last stood, on any view.
    owner: Option<Row>,
    /// Our side's rows as a player chart (Damage, Healing, Taken) last
    /// listed them: who fought — what the command palette offers as the
    /// pull's players on a view whose rows are not all of them (a count
    /// view's, the enemies').
    players: Vec<Row>,
}

impl Seen {
    /// Take in what is on screen now; `owner` is the owner's row there.
    pub fn observe(&mut self, app: &ClientState, owner: Option<&Row>) {
        let Some(fight) = watched(app) else {
            return;
        };
        if self.fight != Some(fight) {
            *self = Seen {
                fight: Some(fight),
                ..Seen::default()
            };
        }
        if loading(app) {
            return;
        }
        if let Some(me) = owner {
            self.owner = Some(me.clone());
        }
        if player_chart(app.view) && app.view_answered() {
            self.players = app.rows().into_iter().filter(|r| !r.enemy).collect();
        }
    }

    /// What was seen, when it is of the fight on screen.
    pub fn of(&self, app: &ClientState) -> Option<&Seen> {
        (self.fight.is_some() && watched(app) == self.fight).then_some(self)
    }

    /// The owner's row as it last stood.
    pub fn owner(&self) -> Option<&Row> {
        self.owner.as_ref()
    }

    /// The fight on screen's players, as a player chart last listed them:
    /// its rows while it is one — and they are the fight's, not a pull's
    /// the reader just left whose rows stand in while this one loads — else
    /// what one said, none before one did.
    pub fn players(&self, app: &ClientState) -> Vec<Row> {
        if player_chart(app.view) && app.view_answered() && !loading(app) {
            return app.rows().into_iter().filter(|r| !r.enemy).collect();
        }
        self.of(app).map(|s| s.players.clone()).unwrap_or_default()
    }
}

/// The fight on screen is still loading: what is in hand is a placeholder,
/// or the pull before it.
pub fn loading(app: &ClientState) -> bool {
    app.status
        .as_deref()
        .is_some_and(wowdps_proto::is_loading_status)
}

/// A view whose rows are everyone who fought: what they dealt, healed or
/// took. A count view's rows are only who counted; the enemies' are not
/// players at all.
pub fn player_chart(view: View) -> bool {
    matches!(view, View::Damage | View::Healing | View::Taken)
}

/// The fight on screen, by the daemon's id for it: `None` before a
/// snapshot describes one.
pub fn watched(app: &ClientState) -> Option<SegmentId> {
    app.segment_name()?;
    app.entries().get(app.segment_index()).map(|e| e.id)
}
