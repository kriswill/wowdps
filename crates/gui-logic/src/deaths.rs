//! The Deaths view's words and order (R25, v35): on that view the window's
//! meter is a chronological table (the prototype's `.v-deaths`) — every
//! death in the order it happened, from the snapshot's raid timeline — and
//! the ribbon a skull per death. What a death is called, which the filter
//! keeps, which one the inspector recaps and how its recap events are timed
//! are here; each GUI draws them.

use wowdps_model::fmt::duration;
use wowdps_model::{RaidDeath, RaidTimeline, Row};
use wowdps_proto::ClientState;

use crate::labels::{display_name, plural};

/// What a death is, to open it: the player's meter key and label, and which
/// of their death windows (R9's index) — the Deaths drill's own question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub key: String,
    pub label: String,
    pub index: u32,
}

impl Pick {
    /// The pick that opens death `d`.
    pub fn of(d: &RaidDeath) -> Self {
        Pick {
            key: d.guid.clone(),
            label: d.name.clone(),
            index: d.index,
        }
    }
}

/// What a death with no damage in its recap says in the killing blow's
/// place — a mechanic that removed the player without a hit. A note, not
/// an ability: set in the faint ink.
pub const NO_DAMAGE: &str = "No damage logged";

/// The enemy team's word after a name (R13).
pub const ENEMY: &str = "enemy";

/// A death in words: the killing blow and what follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    /// The killing blow ("Venom Rupture"), a cheat death's ("Purgatory ran
    /// out") or the note that there was none ([`NO_DAMAGE`]).
    pub blow: String,
    /// `blow` is the note, not an ability.
    pub note: bool,
    /// Who dealt it: "Zul'jan", "self" for their own, "cheat death" for a
    /// cheat death running out, nothing for a nil source or no damage.
    pub source: String,
    /// "rezzed 2:03", when a rez raised them.
    pub rez: Option<String>,
    /// A cheat death ran out (their own blow of 1 health or less): the
    /// hit is the log's bookkeeping, and no figure is shown for it.
    pub cheat: bool,
}

impl Words {
    /// What follows the blow, as one run (the prototype's `.src`: `${src},
    /// rezzed ${t}`): "Zul'jan, rezzed 2:03", "Zul'jan", "rezzed 4:25" —
    /// `None` when neither is there.
    pub fn after(&self) -> Option<String> {
        match (self.source.is_empty(), &self.rez) {
            (true, None) => None,
            (false, None) => Some(self.source.clone()),
            (true, Some(rez)) => Some(rez.clone()),
            (false, Some(rez)) => Some(format!("{}, {rez}", self.source)),
        }
    }
}

/// A death's words: its killing blow, its source and its rez — "Venom
/// Rupture" / "Zul'jan" / "rezzed 2:03". A blow the player dealt themselves
/// for 1 health or less is a cheat death running out (Purgatory: the log
/// writes its end as a self-kill), worded as the prototype words it —
/// "Purgatory ran out", "cheat death"; any other of their own is "self"; a
/// player source loses its realm when the options say so.
pub fn words(d: &RaidDeath, hide_realms: bool) -> Words {
    let own = !d.source.is_empty() && d.source == d.name;
    let cheat = own && !d.blow.is_empty() && d.hit <= 1;
    let (blow, note) = if d.blow.is_empty() {
        (NO_DAMAGE.to_string(), true)
    } else if cheat {
        (format!("{} ran out", d.blow), false)
    } else {
        (d.blow.clone(), false)
    };
    let source = if cheat {
        "cheat death".to_string()
    } else if own {
        "self".to_string()
    } else if hide_realms {
        display_name(&d.source).to_string()
    } else {
        d.source.clone()
    };
    Words {
        blow,
        note,
        source,
        rez: d
            .rez
            .as_ref()
            .map(|r| format!("rezzed {}", duration(r.at_ms))),
        cheat,
    }
}

/// The rows `raid.deaths` stand for as the meter's owner rule reads them:
/// a death's player as a row keyed by their guid — what a GUI's owner
/// resolution finds "you" among when the daemon marked none of the deaths.
pub fn owner_rows(raid: &RaidTimeline) -> Vec<Row> {
    raid.deaths
        .iter()
        .map(|d| Row {
            key: d.guid.clone(),
            label: d.name.clone(),
            enemy: d.enemy,
            ..Row::default()
        })
        .collect()
}

/// The daemon marked one of the deaths as the reader's own (`mine`).
pub fn any_mine(raid: &RaidTimeline) -> bool {
    raid.deaths.iter().any(|d| d.mine)
}

/// Is `d` the reader's own death — the daemon's mark, or the guid a GUI
/// found when the daemon marked none — and never an arena enemy's?
pub fn is_mine(d: &RaidDeath, owner: Option<&str>) -> bool {
    !d.enemy && (d.mine || owner == Some(d.guid.as_str()))
}

/// The deaths the filter keeps, by place in `raid.deaths` — matched as the
/// meter's rows are, by name, class, spec or role.
pub fn drawn(raid: &RaidTimeline, filter: &str) -> Vec<usize> {
    let rows: Vec<Row> = raid
        .deaths
        .iter()
        .map(|d| Row {
            label: d.name.clone(),
            class: d.class,
            spec: d.spec,
            ..Row::default()
        })
        .collect();
    crate::table::filtered_indexed(rows, filter)
        .into_iter()
        .map(|(i, _)| i)
        .collect()
}

/// The death the inspector recaps, by its place in `raid.deaths`: the
/// drilled player's window the Deaths drill asked for, or answered with —
/// their last, when it names none.
pub fn selected(app: &ClientState, raid: &RaidTimeline) -> Option<usize> {
    let key = &app.drill.as_ref()?.key;
    let want = app.death_request().or_else(|| app.deaths().1);
    match want {
        Some(index) => raid
            .deaths
            .iter()
            .position(|d| d.guid == *key && d.index == index),
        None => raid.deaths.iter().rposition(|d| d.guid == *key),
    }
}

/// "2 battle rezzes", "1 battle rez".
pub fn rez_words(n: usize) -> String {
    if n == 1 {
        "1 battle rez".to_string()
    } else {
        format!("{n} battle rezzes")
    }
}

/// The table's total: the group's deaths, the battle rezzes that undid
/// some (a self-rez is none), and an arena's enemy deaths apart.
pub fn total_words(all: &[RaidDeath]) -> String {
    let ours: Vec<&RaidDeath> = all.iter().filter(|d| !d.enemy).collect();
    let rezzes = ours.iter().filter(|d| d.battle_rezzed()).count();
    let enemies = all.len() - ours.len();
    let mut label = plural(ours.len(), "death");
    if rezzes > 0 {
        label = format!("{label}, {}", rez_words(rezzes));
    }
    if enemies > 0 {
        label = format!("{label}, {}", plural(enemies, "enemy death"));
    }
    label
}

/// What an empty table says: nobody died, or the filter hides everyone
/// who did.
pub fn empty_words(any: bool, filter: &str) -> String {
    if any {
        format!(
            "No player matches \u{201c}{filter}\u{201d}. Filter by name, class, spec or role, \
             or press Esc to clear it."
        )
    } else {
        "Nobody died in this pull.".to_string()
    }
}

/// "−4.25s": a recap event's time before the death (R9, v35 `offset_ms`) —
/// in hundredths under ten seconds, where a raid's last events crowd (a
/// ring of 32 can span half a second of heals and hits), in tenths under a
/// minute ("−12.4s"), and "−1:05" a minute or more before; "0.00s" on the
/// death's own moment.
pub fn before(ms: i64) -> String {
    let back = ms.unsigned_abs();
    let secs = back as f64 / 1000.0;
    // Under 5 ms it rounds to the death's own moment: no sign on a zero.
    if back < 5 {
        "0.00s".to_string()
    } else if back < 10_000 {
        format!("\u{2212}{secs:.2}s")
    } else if back < 60_000 {
        format!("\u{2212}{secs:.1}s")
    } else {
        format!("\u{2212}{}", duration(back as i64))
    }
}
