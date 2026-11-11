//! The game's editions as the window shows them: what it calls each, which
//! ones this machine has, the switch's entries, and the edition every
//! scoped read asks for. One idea runs through it: **the window shows one
//! game at a time, and by default the one you are playing.**
//!
//! - **Names.** Retail goes by its current expansion's name, looked up from
//!   the major version of its newest build (12 is "Midnight"), and Forever
//!   by "Forever"; the build is the tooltip's. A name never changes on its
//!   own: before any build is known Retail is the newest expansion this
//!   build knows. In code, in filters and in stored data the edition stays
//!   its key (`retail`).
//! - **Which editions are known.** One is known when the daemon follows a
//!   source of it (`Status`'s `editions`) or the history store holds a
//!   fight of it ([`Census`]), and stays known for the session. With one
//!   known the window draws no switch and asks every question as it always
//!   has: for every edition.
//! - **The choice.** The switch lists the games and nothing else. Picking
//!   the game the window would follow anyway is following
//!   (`EditionRef::Follow`), picking another pins it ([`choice_for`]); the
//!   reader never sees the difference, only whether the game shown is the
//!   one being played. It scopes the live meter
//!   (`ClientState::set_edition`) and, once more than one edition is
//!   known, every history read the window makes ([`history_edition`]).
//! - **What an edition lacks is said.** A log that carries no builds
//!   says so where the talents and gear would be ([`builds_gap`]); a game
//!   with no fights says how to get some ([`empty_stage`]).
//!
//! Logos are a later step: an entry is its name here, and the GUI draws an
//! emblem beside it once one is cached.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use wowdps_model::Edition;
use wowdps_proto::history::FightCard;
use wowdps_proto::{
    ClientMsg, EditionRef, EditionSource, FightSort, HistoryAnswer, HistoryQuery, OriginFilter,
};

/// The editions the census asks the store about at launch: the ones this
/// build knows an install folder for. Any other edition a source or a card
/// names is asked about once it is seen.
pub const PROBED: [Edition; 2] = [Edition::Retail, Edition::Forever];

/// The least time between two rounds of asking the store again about the
/// editions it held no fight of: a closing pull writes a fight, and a
/// wipe-heavy night writes them faster than anyone reads a switch.
pub const REPROBE_EVERY: Duration = Duration::from_secs(5);

/// The current game's expansions by the major version of its build: what
/// Retail is called.
const EXPANSIONS: [(u16, &str); 6] = [
    (7, "Legion"),
    (8, "Battle for Azeroth"),
    (9, "Shadowlands"),
    (10, "Dragonflight"),
    (11, "The War Within"),
    (12, "Midnight"),
];

/// The newest expansion this build knows: what Retail is called before any
/// build of it is known, so its name does not change when the first fight
/// is stored.
pub fn newest_expansion() -> &'static str {
    EXPANSIONS.last().map_or("Retail", |(_, name)| *name)
}

/// The expansion a Retail build of this major version belongs to; `None`
/// for one this build does not know (a newer expansion's).
pub fn expansion(major: u16) -> Option<&'static str> {
    EXPANSIONS
        .iter()
        .find(|(m, _)| *m == major)
        .map(|(_, name)| *name)
}

/// A build as the game words it: "12.1.5". `None` for no build (0.0.0:
/// a card stored before its log's header carried one).
pub fn build_words(build: Option<(u16, u16, u16)>) -> Option<String> {
    let (a, b, c) = build.filter(|b| *b != (0, 0, 0))?;
    Some(format!("{a}.{b}.{c}"))
}

/// What the window calls an edition: Retail by its expansion, looked up
/// from `build` ("Midnight") — the newest one this build knows while no
/// build is, "Retail" for a build of an expansion it does not know;
/// Forever "Forever"; Classic "Classic"; a number this build does not know
/// "Unknown edition".
pub fn name(edition: Edition, build: Option<(u16, u16, u16)>) -> String {
    match edition.counts_as() {
        Edition::Retail => match build.filter(|b| *b != (0, 0, 0)) {
            Some((major, _, _)) => expansion(major).unwrap_or("Retail"),
            None => newest_expansion(),
        }
        .to_string(),
        Edition::Unknown(_) => "Unknown edition".to_string(),
        other => other.label().to_string(),
    }
}

/// An edition's tooltip: its edition and build where the name is not the
/// edition's own ("Retail, build 12.1.5"), else the build alone ("Build
/// 1.60.1"); the edition alone without a build. An unknown number says its
/// number.
pub fn tip(edition: Edition, build: Option<(u16, u16, u16)>) -> String {
    let e = edition.counts_as();
    let shown = name(e, build);
    let label = match e {
        Edition::Unknown(n) => format!("Unknown edition {n}"),
        other => other.label().to_string(),
    };
    match (build_words(build), shown == label) {
        (Some(b), true) => format!("Build {b}"),
        (Some(b), false) => format!("{label}, build {b}"),
        (None, _) => label,
    }
}

/// The element id of an edition's entry: `edition-retail`,
/// `edition-forever`, a Classic client or an unknown number by its id
/// (`edition-classic-2`).
pub fn entry_id(edition: Edition) -> String {
    match edition.counts_as() {
        e @ (Edition::Retail | Edition::Forever) => format!("edition-{}", e.key()),
        other => format!("edition-{}-{}", other.key(), other.project_id()),
    }
}

/// The note beside the game being played right now, in the switch's menu.
pub const PLAYING_NOW: &str = "Playing now";

/// What the stage says in a game with no fight to show: a heading ("No
/// Forever fights yet") and how to get one.
pub fn empty_stage(name: &str) -> (String, String) {
    (
        format!("No {name} fights yet"),
        format!("Play a fight in {name} with combat logging on and it shows up here."),
    )
}

/// Whether an edition's combat log carries the players' builds (their
/// talents and gear, `COMBATANT_INFO`). Forever's does not: its players
/// have a class and no spec.
pub fn logs_builds(edition: Edition) -> bool {
    edition.counts_as() != Edition::Forever
}

/// What the talent viewer says in a fight of an edition whose log carries
/// no builds, where the talents and gear would be; `None` for any other.
pub fn builds_gap(edition: Option<Edition>) -> Option<String> {
    let e = edition?.counts_as();
    (!logs_builds(e)).then(|| {
        format!(
            "{}'s combat log does not carry talents or gear.",
            name(e, None)
        )
    })
}

/// The edition the window is in: the pinned one, or, following, the live
/// edition (`live`: the edition the daemon last answered in, else the one
/// `Status` names live) — and until either names one, `last`, the edition
/// the window was in, so a pick never passes through another edition on
/// its way. What the per-edition keys (the character, its class, the
/// dates Home counts from) are read by.
pub fn window_edition(choice: EditionRef, live: Option<Edition>, last: Edition) -> Edition {
    match choice.canonical() {
        EditionRef::Pin(e) => e.counts_as(),
        EditionRef::Follow => live.unwrap_or(last).counts_as(),
    }
}

/// What picking `pick` asks of the meter: following, when it is the game
/// the window would follow anyway (`follows_to`: the live edition), else a
/// pin to it. Looking at the game being played is never a state of its
/// own.
pub fn choice_for(pick: Edition, follows_to: Option<Edition>) -> EditionRef {
    let pick = pick.counts_as();
    if follows_to.map(Edition::counts_as) == Some(pick) {
        EditionRef::Follow
    } else {
        EditionRef::Pin(pick)
    }
}

/// The edition every history read asks for: none (every edition, as the
/// window always asked) while one edition or none is known, else the
/// edition the window is in.
pub fn history_edition(several: bool, window: Edition) -> Option<Edition> {
    several.then_some(window.counts_as())
}

/// The fight header's meta line with the fight's edition named where a
/// reader could take it for another: a fight of any edition but Retail
/// says which (`fight_head::edition_meta`), and while the window knows
/// several editions a Retail fight in another edition's window says its
/// name too — "Midnight, Heroic, 20 players". A stage with no fight on it
/// (`fight` `None`: nothing answered yet, an empty game) names no edition.
/// `window` is the edition the window is in, `None` while it knows one;
/// `build` the fight's own (its stored card's), else Retail's newest.
pub fn header_meta(
    meta: String,
    fight: Option<Edition>,
    window: Option<Edition>,
    build: Option<(u16, u16, u16)>,
) -> String {
    let fight_e = fight.map(Edition::counts_as);
    let retail_elsewhere = fight_e == Some(Edition::Retail)
        && window.is_some_and(|w| w.counts_as() != Edition::Retail);
    if !retail_elsewhere {
        return crate::fight_head::edition_meta(meta, fight);
    }
    let word = name(Edition::Retail, build);
    if meta.is_empty() {
        word
    } else {
        format!("{word}, {meta}")
    }
}

// ---- the census --------------------------------------------------------------

/// What the window knows of one edition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seen {
    /// The daemon follows a source of it.
    pub source: bool,
    /// The store holds fights of it, as its last answer counted them.
    pub stored: u32,
    /// The newest build a card of it carried, and that card's start.
    pub build: Option<(u16, u16, u16)>,
    newest_ms: i64,
    /// It was known once this session: it stays known, so a switch once
    /// drawn never goes away under a reader (a source the daemon stopped
    /// listing, say, while its edition is shown).
    once: bool,
}

impl Seen {
    fn known(&self) -> bool {
        self.once || self.source || self.stored > 0
    }

    fn mark(&mut self) {
        self.once |= self.source || self.stored > 0;
    }
}

/// Which editions this machine has: the sources `Status` lists, and per
/// edition one `HistoryQuery::Fights` of a single card, which answers
/// whether the store holds any of its fights (its `total`) and the newest
/// one's build. One question in flight at a time, as every reader of the
/// window's; asked at launch for [`PROBED`] and for any other edition a
/// source or a card names, and again — at most every [`REPROBE_EVERY`] —
/// for an edition with no fight yet each time the store writes one.
#[derive(Debug)]
pub struct Census {
    seen: BTreeMap<Edition, Seen>,
    /// The live edition's source, as `Status` names it: what a following
    /// window shows.
    live: Option<Edition>,
    /// The editions whose game runs now, as `Status` tells.
    playing: Vec<Edition>,
    /// To ask about, in order.
    queue: Vec<Edition>,
    /// The question in flight.
    pending: Option<(u32, Edition)>,
    /// Editions whose empty answer may have been a refused read (the
    /// daemon's read quota full answers as an empty store does): asked once
    /// more after a pause.
    again: Vec<Edition>,
    retry_at: Option<Instant>,
    /// Asked once more already this round: a second empty answer is the
    /// store's.
    retried: Vec<Edition>,
    /// The store wrote a fight: ask again about the editions it had none
    /// of, once this is past.
    reprobe: bool,
    reprobed_at: Option<Instant>,
}

impl Default for Census {
    fn default() -> Self {
        Self::new()
    }
}

impl Census {
    /// A census about to ask about [`PROBED`].
    pub fn new() -> Self {
        Self {
            seen: BTreeMap::new(),
            live: None,
            playing: Vec::new(),
            queue: PROBED.to_vec(),
            pending: None,
            again: Vec::new(),
            retry_at: None,
            retried: Vec::new(),
            reprobe: false,
            reprobed_at: None,
        }
    }

    /// The editions known, in the switch's order: Retail, Forever, the
    /// Classic clients, unknown numbers.
    pub fn known(&self) -> Vec<Edition> {
        self.seen
            .iter()
            .filter(|(_, s)| s.known())
            .map(|(e, _)| *e)
            .collect()
    }

    /// More than one edition is known: the switch is drawn and every read
    /// is scoped.
    pub fn several(&self) -> bool {
        self.seen.values().filter(|s| s.known()).count() > 1
    }

    /// The live edition, as `Status` last named it: the edition a following
    /// window shows. `None` before `Status`, or while no source the daemon
    /// follows names an edition.
    pub fn live(&self) -> Option<Edition> {
        self.live
    }

    /// `edition`'s game runs now, as the daemon's watcher tells.
    pub fn playing(&self, edition: Edition) -> bool {
        self.playing.contains(&edition.counts_as())
    }

    /// What is known of `edition`.
    pub fn seen(&self, edition: Edition) -> Seen {
        self.seen
            .get(&edition.counts_as())
            .copied()
            .unwrap_or_default()
    }

    /// The newest build seen of `edition`.
    pub fn build(&self, edition: Edition) -> Option<(u16, u16, u16)> {
        self.seen(edition).build
    }

    /// What the window calls `edition`, by its newest build.
    pub fn name(&self, edition: Edition) -> String {
        name(edition, self.build(edition))
    }

    /// Its tooltip, by its newest build.
    pub fn tip(&self, edition: Edition) -> String {
        tip(edition, self.build(edition))
    }

    fn ask_about(&mut self, edition: Edition) {
        let e = edition.counts_as();
        let asking = self.pending.is_some_and(|(_, p)| p == e);
        if !asking && !self.queue.contains(&e) {
            self.queue.push(e);
        }
    }

    /// `Status`'s sources: an edition the daemon follows is known, the live
    /// one is what a following window shows, and a running one is being
    /// played. `true` when any of that changed.
    pub fn on_sources(&mut self, sources: &[EditionSource]) -> bool {
        let named: Vec<Edition> = sources
            .iter()
            .filter_map(|s| s.edition)
            .map(Edition::counts_as)
            .collect();
        let live = sources
            .iter()
            .find(|s| s.live)
            .and_then(|s| s.edition)
            .map(Edition::counts_as);
        let mut playing: Vec<Edition> = sources
            .iter()
            .filter(|s| s.running)
            .filter_map(|s| s.edition)
            .map(Edition::counts_as)
            .collect();
        playing.sort();
        playing.dedup();
        let before = (self.known(), self.live, self.playing.clone());
        self.live = live;
        self.playing = playing;
        let mut new = Vec::new();
        for e in &named {
            let s = self.seen.entry(*e).or_default();
            if !s.known() {
                new.push(*e);
            }
        }
        for (e, s) in self.seen.iter_mut() {
            s.source = named.contains(e);
            s.mark();
        }
        // Their fights and builds, unless the launch's round asks.
        for e in new.into_iter().filter(|e| !PROBED.contains(e)) {
            self.ask_about(e);
        }
        (self.known(), self.live, self.playing.clone()) != before
    }

    /// Cards the window read elsewhere (the rail's pages, Home's): an
    /// edition with a card has stored fights, and the newest card of each
    /// names its build. `true` when that changed what is known or what one is called.
    pub fn learn<'a>(&mut self, cards: impl IntoIterator<Item = &'a FightCard>) -> bool {
        let mut changed = false;
        for c in cards {
            let e = c.edition().counts_as();
            let s = self.seen.entry(e).or_default();
            if s.stored == 0 {
                s.stored = 1;
                s.mark();
                changed = true;
            }
            changed |= s.note(c);
        }
        changed
    }

    /// The store wrote a fight: the editions it held none of are asked
    /// about again, at most every [`REPROBE_EVERY`].
    pub fn store_changed(&mut self) {
        self.reprobe = true;
    }

    /// The connection the question in flight went out on is gone (the
    /// daemon restarted): nothing will answer it, so it is asked again,
    /// first.
    pub fn lost(&mut self) {
        if let Some((_, e)) = self.pending.take() {
            self.queue.retain(|q| *q != e);
            self.queue.insert(0, e);
        }
    }

    /// The question to send `now`, if any.
    pub fn next_request(&mut self, req_id: u32, now: Instant) -> Option<ClientMsg> {
        if self.pending.is_some() {
            return None;
        }
        if self.reprobe && self.reprobed_at.is_none_or(|at| now >= at + REPROBE_EVERY) {
            self.reprobe = false;
            self.reprobed_at = Some(now);
            self.retried.clear();
            let empty: Vec<Edition> = PROBED
                .iter()
                .copied()
                .chain(self.seen.keys().copied())
                .filter(|e| self.seen(*e).stored == 0)
                .collect();
            for e in empty {
                self.ask_about(e);
            }
        }
        if self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
            for e in std::mem::take(&mut self.again) {
                self.ask_about(e);
            }
        }
        if self.queue.is_empty() {
            return None;
        }
        let edition = self.queue.remove(0);
        self.pending = Some((req_id, edition));
        Some(ClientMsg::GetHistory {
            req_id,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                guid: None,
                since_utc_ms: None,
                kind: None,
                sort: FightSort::Newest,
                limit: 1,
                after_id: None,
                role: None,
                origin: OriginFilter::Local,
                edition: Some(edition),
            },
        })
    }

    /// Fold an answer in; `(mine, changed)`: whether it answered this
    /// census, and whether it changed what is known or what an edition is
    /// called.
    pub fn absorb(&mut self, req_id: u32, answer: &HistoryAnswer) -> (bool, bool) {
        let Some((asked, edition)) = self.pending else {
            return (false, false);
        };
        if asked != req_id {
            return (false, false);
        }
        self.pending = None;
        let HistoryAnswer::Fights { cards, total } = answer else {
            return (true, false);
        };
        let before = self.known();
        let build_before = self.seen(edition).build;
        // An empty answer may be a refused read: an edition with no fight
        // known is asked once more after a pause; one with fights keeps
        // what it had.
        if *total == 0 && cards.is_empty() {
            if self.seen(edition).stored == 0 && !self.retried.contains(&edition) {
                self.retried.push(edition);
                self.again.push(edition);
                self.retry_at
                    .get_or_insert(Instant::now() + crate::history::RETRY_AFTER);
            }
            return (true, false);
        }
        let s = self.seen.entry(edition).or_default();
        s.stored = (*total).max(1);
        s.mark();
        for c in cards {
            s.note(c);
        }
        let changed = self.known() != before || self.seen(edition).build != build_before;
        (true, changed)
    }
}

impl Seen {
    /// A card of this edition: the newest one's build is the edition's.
    /// `true` when that changed the build.
    fn note(&mut self, c: &FightCard) -> bool {
        if c.build == (0, 0, 0) || c.start_utc_ms < self.newest_ms {
            return false;
        }
        self.newest_ms = c.start_utc_ms;
        let changed = self.build != Some(c.build);
        self.build = Some(c.build);
        changed
    }
}

// ---- the switch ------------------------------------------------------------------

/// One of the switch's entries: a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub edition: Edition,
    /// "Midnight", "Forever".
    pub name: String,
    /// [`PLAYING_NOW`] beside the game being played; nothing with no game
    /// running.
    pub note: Option<&'static str>,
    /// Its tooltip: the edition and its build.
    pub tip: String,
    /// The game the window shows.
    pub on: bool,
    /// Its element id ([`entry_id`]).
    pub id: String,
}

/// The switch's face: the game the window shows, by name, and whether it
/// is the one being played (the live dot before the name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub edition: Edition,
    pub name: String,
    pub tip: String,
    pub playing: bool,
}

/// The switch while more than one edition is known: one entry per known
/// game, the one `shown` lit; `None` with one or none, when the window
/// draws no switch.
pub fn entries(census: &Census, shown: Edition) -> Option<Vec<Entry>> {
    if !census.several() {
        return None;
    }
    let shown = shown.counts_as();
    Some(
        census
            .known()
            .into_iter()
            .map(|e| Entry {
                edition: e,
                name: census.name(e),
                note: census.playing(e).then_some(PLAYING_NOW),
                tip: census.tip(e),
                on: e == shown,
                id: entry_id(e),
            })
            .collect(),
    )
}

/// The switch's face for the game `shown`.
pub fn face(census: &Census, shown: Edition) -> Face {
    let edition = shown.counts_as();
    Face {
        edition,
        name: census.name(edition),
        tip: census.tip(edition),
        playing: census.playing(edition),
    }
}

#[cfg(test)]
mod tests;
