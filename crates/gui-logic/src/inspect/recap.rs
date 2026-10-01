//! A death's recap in words (R9, R25): the head's "died 5:45 to …,
//! rezzed 2:03 by …", each event's signed change, and the insight a recap
//! adds when the player's own hit mattered. Moved from the iced window's
//! inspector.

use wowdps_model::fmt::{commas, duration};
use wowdps_model::{Class, Row};

use crate::labels::display_name;
use crate::table::split_pet;

/// R9: a death's last events as the recap draws them.
#[derive(Debug, Clone, PartialEq)]
pub struct Recap {
    /// The events, NEWEST first as the daemon sends them (the list draws
    /// them oldest first, the way the death happened).
    pub rows: Vec<Row>,
    /// The player who died, as their events' sources name them — what
    /// their own events say instead ("yours" for the owner, else "self").
    pub who: String,
    pub yours: bool,
    pub class: Option<Class>,
}

/// R25: a death as the recap's head words it (the prototype's
/// `recapPanel`): "died 5:45 to Coalesced Venom", "died 5:15 as Purgatory
/// ran out", "died 6:01, no damage logged" — then ", rezzed 2:03 by
/// Soundscape" when someone raised them (their own spell's name for a
/// self-rez: "by Reincarnation").
pub fn died_words(d: &wowdps_model::RaidDeath, hide_realms: bool) -> String {
    let w = crate::deaths::words(d, hide_realms);
    let at = duration(d.at_ms);
    let mut words = if w.note {
        format!("died {at}, {}", w.blow.to_lowercase())
    } else if w.cheat {
        format!("died {at} as {}", w.blow)
    } else {
        format!("died {at} to {}", w.blow)
    };
    if let Some(rez) = &d.rez {
        let by = if rez.by == d.guid {
            rez.spell.clone()
        } else if hide_realms {
            display_name(&rez.by_name).to_string()
        } else {
            rez.by_name.clone()
        };
        words.push_str(&format!(", rezzed {} by {by}", duration(rez.at_ms)));
    }
    words
}

/// An event's change as the recap signs it: "+756", "−82,509".
pub fn change_words(e: &Row) -> String {
    let sign = if e.gain { "+" } else { "\u{2212}" };
    format!("{sign}{}", commas(e.amount))
}

/// The insight's words, a piece at a time — `true` on the one set bold in
/// the owner's text colour (the ability that did it).
pub type Insight = Vec<(String, bool)>;

/// A hit of their own is worth an insight when it took this share of the
/// player's health …
pub const INSIGHT_SHARE: f64 = 0.05;

/// … or found them under this share of it with the death this close after.
pub const INSIGHT_LOW: f64 = 0.30;

pub const INSIGHT_SOON_MS: i64 = 5_000;

/// v35 (R9): what a recap says about damage the player did to THEMSELVES
/// (an event whose source is their own name — a Burning Rush, a Soul
/// Burn): the biggest such hit, the health it found them at (the health
/// after the event before it), and what finished them after it — "Your own
/// **Burning Rush** took 30,660 while you were at 5.2% health. Three
/// Coalesced Venom hits finished it." Only a MATERIAL hit is named — at
/// least [`INSIGHT_SHARE`] of their health, or one that found them under
/// [`INSIGHT_LOW`] with the death within [`INSIGHT_SOON_MS`]: a warlock's
/// Burning Rush ticks all fight, and a tick at 80 % health killed nobody.
/// `None` when every hit was someone else's, or theirs was no matter.
pub fn insight(r: &Recap) -> Option<Insight> {
    let oldest: Vec<&Row> = r.rows.iter().rev().collect();
    let own = |e: &Row| {
        !e.gain
            && split_pet(&e.label)
                .1
                .is_some_and(|s| display_name(s) == display_name(&r.who))
    };
    // The biggest; the latest of equals.
    let (at, hit) = oldest
        .iter()
        .enumerate()
        .filter(|(_, e)| own(e))
        .max_by_key(|(i, e)| (e.amount, *i))?;
    let before = at
        .checked_sub(1)
        .and_then(|j| oldest.get(j))
        .and_then(|e| e.hp)
        .filter(|(_, max)| *max > 0);
    let max = hit.hp.or(before).map(|(_, m)| m).filter(|m| *m > 0);
    let share = max.map(|m| hit.amount as f64 / m as f64);
    let low = before.map(|(cur, m)| cur as f64 / m as f64);
    let soon = hit.offset_ms.is_none_or(|o| o >= -INSIGHT_SOON_MS);
    let material =
        share.is_some_and(|s| s >= INSIGHT_SHARE) || (low.is_some_and(|l| l < INSIGHT_LOW) && soon);
    if !material {
        return None;
    }
    let what = split_pet(&hit.label).0.to_string();
    let (whose, were) = if r.yours {
        ("Your own ".to_string(), "you were")
    } else {
        (format!("{}'s own ", display_name(&r.who)), "they were")
    };
    let health = low
        .map(|l| format!(" while {were} at {:.1}% health", l * 100.0))
        .unwrap_or_default();
    let after: Vec<&str> = oldest
        .iter()
        .skip(at + 1)
        .filter(|e| !e.gain)
        .map(|e| split_pet(&e.label).0)
        .collect();
    let finish = match after.first() {
        None => " It was the killing blow.".to_string(),
        Some(first) if after.iter().all(|s| s == first) => format!(
            " {} {first} {} finished it.",
            count_word(after.len()),
            if after.len() == 1 { "hit" } else { "hits" }
        ),
        Some(_) => format!(" {} more hits finished it.", count_word(after.len())),
    };
    Some(vec![
        (whose, false),
        (what, true),
        (
            format!(" took {}{health}.{finish}", commas(hit.amount)),
            false,
        ),
    ])
}

/// "One", "Two" … "Nine", then the figure — how a sentence starts a count.
pub fn count_word(n: usize) -> String {
    const WORDS: [&str; 9] = [
        "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    n.checked_sub(1)
        .and_then(|i| WORDS.get(i))
        .map_or_else(|| n.to_string(), |w| w.to_string())
}
