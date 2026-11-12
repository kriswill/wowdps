//! v49: one source and one engine per edition. The daemon follows a
//! source per edition it knows a logs folder of (`DaemonOptions::editions`):
//! each with its own tailer and its own [`Engine`] — its index, its live
//! meter, its loaded segments — so one edition's new log never wipes
//! another's state. Every engine draws its segment ids from one counter
//! ([`Ids`]), so an id names one segment across the daemon and the hub
//! routes it to the engine that holds it.
//!
//! Which engine answers a session: a segment id, the engine holding it; a
//! following session, the LIVE edition's ([`Sources::live`]); a session
//! pinned to an edition, the engine whose log is of it (the live one first
//! when two are, then the one written last), else the empty answer.
//!
//! The live edition: the running game's when one followed edition's game
//! runs; with both running, the one with an open pull — kept while it has
//! one, and while neither has, until the other opens one or its log grows;
//! with none running, the one whose log was written last — kept, too,
//! while it has a pull open or its log is still being written, until the
//! other's log grows. A log growing ([`Engine::grew`]: lines the game
//! appends, or a log it creates, while the daemon watches) moves the live
//! edition to it on the next tick, once the live one has been live, and its
//! log quiet, for [`HANDOFF_QUIET`]: one account plays one game at a time,
//! so the other log growing is the other game being played, while two logs
//! written in turn do not move a following client on every line, and two
//! growing in one tick move nothing. It is chosen again on every tail
//! event, game report and tick, so a pull, a log growing or a log going
//! quiet takes effect within a tick. Only the live engine's
//! trailing segment is what a follower is shown: a stale open pull in
//! another edition's log draws no one.
//!
//! A source is the folder's, not the log's: the header decides a log's
//! edition, so a log in one product's folder whose header names another
//! edition makes that folder's engine that edition's while it follows that
//! log (`Engine::edition`). Before its header is read a source is named by
//! its folder (`Engine::set_source_edition`) — only once more than one
//! source has held a log: a daemon of one source, or of several whose
//! other folders never held one (an install with no Forever log), names
//! its log by its header alone and answers exactly as it did before there
//! were several.

use std::time::{Duration, Instant};

use wowdps_core::model::SegmentId;
use wowdps_core::tail::TailEvent;
use wowdps_model::Edition;
use wowdps_proto::{Cursor, EditionRef, EditionSource, LoadError, SegmentRef};

use crate::engine::{Engine, EngineEvent, Ids, ListKey};
use crate::game::Running;

/// How long the live edition must have been live, and its log quiet, before
/// another edition's log growing moves the live edition there, with no pull
/// open on either side. It bounds the flapping: lines alone — two logs
/// written in turn, however they alternate — never move it twice within
/// this interval (no move on lines comes sooner than this after the last
/// move), and lines landing in both within one tick move nothing. A change
/// of game logs out of one client before the other enters the world, so
/// the old log is quiet from then on: the first batch of the new one's
/// lines to land this long after that moves the live edition.
pub const HANDOFF_QUIET: Duration = Duration::from_secs(10);

/// v49: one followed source as the hub is told of it: how `Status` words
/// what is followed (`spec_display`), and the edition its folder names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSetup {
    pub spec: Option<String>,
    pub edition: Option<Edition>,
}

/// One followed source: its engine, how `Status` words it, and the list
/// key last broadcast for it.
pub struct Source {
    pub engine: Engine,
    pub spec: Option<String>,
    pub last_list: ListKey,
    /// The edition the source's folder names, given to its engine once
    /// more than one source has held a log ([`Sources::on_tail`]).
    folder: Option<Edition>,
    /// Its log grew ([`Engine::grew`]) since the last tick, which reads and
    /// clears it ([`Sources::tick`]).
    grew: bool,
}

/// Every followed source, the first the one `--file`, `--logs` or the
/// config names (always listed), with the live edition's and the game
/// watcher's last word.
pub struct Sources {
    /// The first source; there is always one.
    primary: Source,
    /// The sources followed beside it, one per further edition.
    more: Vec<Source>,
    ids: Ids,
    live: usize,
    /// When the live edition last moved off a source holding a log, `None`
    /// before it has (the start-up pick from the first source to hold one
    /// is no such move): lines alone — a log growing, or the live one's
    /// gone quiet — move it again only once this is [`HANDOFF_QUIET`] old.
    moved_at: Option<Instant>,
    running: Running,
    /// Whether the sources are named by their folders: from when a second
    /// source first held a log, for the daemon's life.
    named: bool,
}

impl Sources {
    /// One engine per setup (at least one), sharing one id counter. Once
    /// more than one has held a log, each is named by its folder until its
    /// header is read ([`Sources::on_tail`]).
    pub fn new(setups: &[SourceSetup]) -> Self {
        let ids = Ids::default();
        let make = |setup: &SourceSetup| Source {
            engine: Engine::with_ids(ids.clone()),
            spec: setup.spec.clone(),
            last_list: ListKey::default(),
            folder: setup.edition,
            grew: false,
        };
        let primary = make(setups.first().unwrap_or(&SourceSetup::default()));
        let more = setups.iter().skip(1).map(make).collect();
        Self {
            primary,
            more,
            ids,
            live: 0,
            moved_at: None,
            running: Running::default(),
            named: false,
        }
    }

    /// How many sources are followed: one at least.
    pub fn len(&self) -> usize {
        1 + self.more.len()
    }

    /// Never: there is always the first source.
    pub fn is_empty(&self) -> bool {
        false
    }

    fn source(&self, i: usize) -> &Source {
        match i.checked_sub(1) {
            Some(j) => self.more.get(j).unwrap_or(&self.primary),
            None => &self.primary,
        }
    }

    /// Source `i`, when there is one.
    pub fn source_mut(&mut self, i: usize) -> Option<&mut Source> {
        match i.checked_sub(1) {
            Some(j) => self.more.get_mut(j),
            None => Some(&mut self.primary),
        }
    }

    /// The first source's engine: what `--file`, `--logs` or the config
    /// names.
    pub fn primary(&self) -> &Engine {
        &self.primary.engine
    }

    /// Source `i`'s engine; the first's for an index out of range.
    pub fn engine(&self, i: usize) -> &Engine {
        &self.source(i).engine
    }

    /// Source `i`'s engine; the first's for an index out of range.
    pub fn engine_mut(&mut self, i: usize) -> &mut Engine {
        let i = if i < self.len() { i } else { 0 };
        match i.checked_sub(1).and_then(|j| self.more.get_mut(j)) {
            Some(s) => &mut s.engine,
            None => &mut self.primary.engine,
        }
    }

    pub fn engines_mut(&mut self) -> impl Iterator<Item = &mut Engine> {
        std::iter::once(&mut self.primary)
            .chain(self.more.iter_mut())
            .map(|s| &mut s.engine)
    }

    /// The live edition's source: what a following session is answered
    /// from.
    pub fn live(&self) -> usize {
        self.live
    }

    /// The watcher's last word.
    pub fn running(&self) -> &Running {
        &self.running
    }

    /// A source is started once its folder has held a log: before that it
    /// is never live, never a pin's, and (but for the first) never listed.
    fn started(&self, i: usize) -> bool {
        self.engine(i).source_path.is_some()
    }

    /// The source serving a session looking in `scope`: following, the live
    /// one; pinned, the started source whose log is of the pin's edition —
    /// the live one first, then the one written last — and `None` when no
    /// source is: such a session is answered the empty answer and acts on
    /// nothing (`DiscardTrash`, a one-shot's `Live`).
    pub fn serving(&self, scope: EditionRef) -> Option<usize> {
        match scope {
            EditionRef::Follow => Some(self.live),
            EditionRef::Pin(_) => {
                let serves = |i: &usize| self.started(*i) && self.engine(*i).serves(scope);
                if serves(&self.live) {
                    return Some(self.live);
                }
                (0..self.len())
                    .filter(serves)
                    .max_by_key(|i| self.engine(*i).written())
            }
        }
    }

    /// The source answering a session looking in `scope`: the one serving
    /// it ([`Sources::serving`]), else the live one, whose `Engine::serves`
    /// answers such a pin the empty answer.
    pub fn for_scope(&self, scope: EditionRef) -> usize {
        self.serving(scope).unwrap_or(self.live)
    }

    /// The source holding segment `id` now, if any does.
    pub fn owner_of(&self, id: SegmentId) -> Option<usize> {
        (0..self.len()).find(|i| self.engine(*i).holds(id))
    }

    /// Why an id no source holds fails ([`Ids::lost`]).
    pub fn lost(&self, id: SegmentId) -> LoadError {
        self.ids.lost(id)
    }

    /// The source a cursor is answered from: the one holding its segment
    /// id (`Err` with why when none does), else the scope's.
    pub fn for_cursor(
        &self,
        cursor: &Cursor,
        scope: EditionRef,
    ) -> Result<usize, (SegmentId, LoadError)> {
        match cursor_id(cursor) {
            Some(id) => self.owner_of(id).ok_or((id, self.lost(id))),
            None => Ok(self.for_scope(scope)),
        }
    }

    /// The source whose loader request `id` was (it asked for the load, or
    /// holds the id); `None` when none does — its log rotated away while
    /// the slice was parsed, and no source can ever resolve the meter.
    pub fn for_load(&self, id: SegmentId) -> Option<usize> {
        (0..self.len())
            .find(|i| self.engine(*i).loading.contains(&id))
            .or_else(|| self.owner_of(id))
    }

    /// Feed source `i` one tail event. `true` when it named the sources by
    /// their folders: the event gave a second source its first log, so
    /// another source's edition may have moved with no event of its own
    /// (an unread log, a header naming none), and its list with it.
    pub fn on_tail(&mut self, i: usize, ev: TailEvent, out: &mut Vec<EngineEvent>) -> bool {
        if let Some(src) = self.source_mut(i) {
            src.engine.on_tail(ev, out);
            src.grew |= src.engine.grew();
        }
        let named = self.name_by_folder();
        // A source's edition may have just been named or changed: whether
        // a game runs for it may have too.
        self.apply_running();
        named
    }

    /// Name every source by its folder once more than one has held a log;
    /// `true` the one time it does. Before then the daemon answers as one
    /// source did: a folder that never holds a log (an install with no
    /// Forever log yet) changes no frame.
    fn name_by_folder(&mut self) -> bool {
        if self.named || (0..self.len()).filter(|i| self.started(*i)).count() < 2 {
            return false;
        }
        self.named = true;
        for src in std::iter::once(&mut self.primary).chain(self.more.iter_mut()) {
            src.engine.set_source_edition(src.folder);
        }
        true
    }

    /// The watcher's word: each engine's game verdict follows it.
    pub fn set_running(&mut self, running: Running) {
        self.running = running;
        self.apply_running();
    }

    /// Whether source `i`'s game runs, for its liveness (`Engine::
    /// game_running`): its edition's client is named running; or a game
    /// runs the watcher cannot name, or of an edition no source is of —
    /// such a game counts for every source, as any game did while there was
    /// one source; or the source's edition is not known and any game runs.
    pub fn runs_for(&self, i: usize) -> bool {
        let r = &self.running;
        let of_some_source = |e: &Edition| {
            (0..self.len()).any(|j| {
                self.engine(j)
                    .edition()
                    .is_some_and(|x| x.counts_as() == e.counts_as())
            })
        };
        let orphan = r.editions.iter().any(|e| !of_some_source(e));
        r.unnamed
            || orphan
            || match self.engine(i).edition() {
                Some(e) => r.names(e),
                None => r.any(),
            }
    }

    fn apply_running(&mut self) {
        let verdicts: Vec<(bool, bool)> = (0..self.len())
            .map(|i| {
                let named = self
                    .engine(i)
                    .edition()
                    .is_some_and(|e| self.running.names(e));
                (self.runs_for(i), named)
            })
            .collect();
        for (engine, (runs, named)) in self.engines_mut().zip(verdicts) {
            engine.game_running = runs;
            engine.edition_running = named;
        }
    }

    /// Choose the live edition again, after a tail event of source
    /// `opened`'s that opened a pull (`Some`) or anything else (`None`).
    /// `true` when it moved: every following session must be moved with it.
    /// One source is always live. A log growing moves it only on the tick
    /// ([`Sources::tick`]).
    pub fn choose_live(&mut self, opened: Option<usize>) -> bool {
        self.choose(opened, &[])
    }

    /// The tick's choice: as [`Sources::choose_live`], and the logs grown
    /// since the last tick may move the live edition — to one of them, when
    /// the live one's did not grow and it has been live, and its log quiet,
    /// for [`HANDOFF_QUIET`]. Two logs growing within one tick, the live
    /// one's among them, move nothing whichever landed first.
    pub fn tick(&mut self) -> bool {
        let grew: Vec<usize> = (0..self.len())
            .filter(|i| {
                self.source_mut(*i)
                    .is_some_and(|src| std::mem::take(&mut src.grew))
            })
            .collect();
        self.choose(None, &grew)
    }

    fn choose(&mut self, opened: Option<usize>, grew: &[usize]) -> bool {
        if self.len() < 2 {
            return false;
        }
        let old = self.live;
        let started: Vec<usize> = (0..self.len()).filter(|i| self.started(*i)).collect();
        if started.is_empty() {
            return false;
        }
        let running: Vec<usize> = started
            .iter()
            .copied()
            .filter(|i| {
                self.engine(*i)
                    .edition()
                    .is_some_and(|e| self.running.names(e))
            })
            .collect();
        let new = match running.as_slice() {
            // One followed game runs: its edition is live.
            [one] => *one,
            // None does: the log written last, kept while its pull is open
            // or it is still being written, until the other's log grows.
            [] => self.keep_or_move(old, &started, opened, grew, Move::WrittenLast),
            // Both do: the one with a pull open, kept while it has one; with
            // neither open, kept until the other opens one or its log grows.
            several => self.keep_or_move(old, several, opened, grew, Move::OnPull),
        };
        self.live = new;
        if new != old && self.started(old) {
            self.moved_at = Some(Instant::now());
        }
        new != old
    }

    /// Among `candidates`: the live one stays while it is one of them and
    /// has a pull open. Otherwise the candidate with a pull open is live —
    /// the one that just opened it (`opened`) first. With none open, a
    /// candidate whose log grew since the last tick (`grew`, the tick's
    /// alone) is live once the live one has been live, and its log quiet,
    /// for [`HANDOFF_QUIET`] ([`Sources::handed_off`]): the one written
    /// last when several did. Else the live one stays: while both games run
    /// ([`Move::OnPull`]), or with none running while its log is still being
    /// written (`Engine::being_written`) or it has been live less than
    /// [`HANDOFF_QUIET`]; then it gives way to the one whose log was written
    /// last ([`Move::WrittenLast`], and whenever the live one is no
    /// candidate) — a log grown since the last tick only on the tick. So
    /// with no pull opening, two moves are at least [`HANDOFF_QUIET`] apart,
    /// and two logs growing within one tick move nothing.
    fn keep_or_move(
        &self,
        old: usize,
        candidates: &[usize],
        opened: Option<usize>,
        grew: &[usize],
        mode: Move,
    ) -> usize {
        let is_candidate = candidates.contains(&old);
        if is_candidate && self.engine(old).pull_open() {
            return old;
        }
        if let Some(x) = opened.filter(|x| candidates.contains(x)) {
            return x;
        }
        if let Some(p) = candidates
            .iter()
            .copied()
            .find(|i| *i != old && self.engine(*i).pull_open())
        {
            return p;
        }
        if let Some(x) = grew
            .iter()
            .copied()
            .filter(|x| *x != old && candidates.contains(x))
            .max_by_key(|x| self.engine(*x).written())
            && (!is_candidate || self.handed_off(old))
        {
            return x;
        }
        if is_candidate
            && (mode == Move::OnPull || self.engine(old).being_written() || !self.settled())
        {
            return old;
        }
        // A log grown since the last tick is not written last before the
        // tick: whether the live one's grew within the same tick is the
        // tick's to see.
        candidates
            .iter()
            .copied()
            .filter(|i| !is_candidate || *i == old || !self.source(*i).grew)
            .max_by_key(|i| (self.engine(*i).written(), *i == old))
            .unwrap_or(old)
    }

    /// Whether the live edition has been live [`HANDOFF_QUIET`] since it
    /// last moved off a source holding a log (or never has): lines alone
    /// may move it again.
    fn settled(&self) -> bool {
        self.moved_at.is_none_or(|t| t.elapsed() >= HANDOFF_QUIET)
    }

    /// Whether the live source `old` gives way to another's log growing:
    /// it is settled ([`Sources::settled`]), and its log was not written
    /// within [`HANDOFF_QUIET`] (nor was a log of it just switched to, its
    /// index not yet landed: its game is writing). Its pull, if open, has
    /// kept it already.
    fn handed_off(&self, old: usize) -> bool {
        let engine = self.engine(old);
        self.settled() && !engine.awaiting_index() && !engine.written_within(HANDOFF_QUIET)
    }

    /// Test hook: as if `by` passed with nothing written to any log — the
    /// live edition's clock and every engine's ([`Engine::age_for_test`]).
    #[cfg(test)]
    pub(crate) fn age_for_test(&mut self, by: Duration) {
        self.moved_at = self.moved_at.and_then(|t| t.checked_sub(by));
        for engine in self.engines_mut() {
            engine.age_for_test(by);
        }
    }

    /// Every source as `Status` lists it: the first always, the others once
    /// their folder has held a log.
    pub fn edition_sources(&self) -> Vec<EditionSource> {
        (0..self.len())
            .filter(|i| *i == 0 || self.started(*i))
            .map(|i| {
                let engine = self.engine(i);
                engine.edition_source(
                    self.source(i).spec.clone(),
                    i == self.live,
                    engine.game_running,
                )
            })
            .collect()
    }
}

/// What else lets the live edition move off a source with no pull open,
/// beside another source opening a pull or its log growing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    /// Another source's log written later, once this one's has gone quiet
    /// (no followed game runs).
    WrittenLast,
    /// Nothing else (both followed games run).
    OnPull,
}

/// The segment id a cursor names, if it names one.
pub fn cursor_id(cursor: &Cursor) -> Option<SegmentId> {
    match cursor {
        Cursor::Segment {
            segment: SegmentRef::Id(id),
            ..
        }
        | Cursor::Compare {
            segment: SegmentRef::Id(id),
            ..
        } => Some(*id),
        _ => None,
    }
}
