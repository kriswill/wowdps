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
//! one, and while neither has, until the other opens one; with none running,
//! the one whose log was written last — kept, too, while it has a pull
//! open or its log is still being written, so two logs written in turn
//! with no game seen do not move a following client on every line. It is
//! chosen again on every tail event, game report and tick, so a pull or a
//! log going quiet takes effect within a tick. Only the live engine's
//! trailing segment is what a follower is shown: a stale open pull in
//! another edition's log draws no one.
//!
//! A source is the folder's, not the log's: the header decides a log's
//! edition, so a log in one product's folder whose header names another
//! edition makes that folder's engine that edition's while it follows that
//! log (`Engine::edition`). Before its header is read a source is named by
//! its folder (`Engine::set_source_edition`) — only while there is more
//! than one: a daemon of one source names its log by its header alone and
//! answers exactly as it did before there were several.

use wowdps_core::model::SegmentId;
use wowdps_core::tail::TailEvent;
use wowdps_model::Edition;
use wowdps_proto::{Cursor, EditionRef, EditionSource, LoadError, SegmentRef};

use crate::engine::{Engine, EngineEvent, Ids, ListKey};
use crate::game::Running;

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
    running: Running,
}

impl Sources {
    /// One engine per setup (at least one), sharing one id counter. With
    /// more than one, each is named by its folder until its header is read.
    pub fn new(setups: &[SourceSetup]) -> Self {
        let ids = Ids::default();
        let several = setups.len() > 1;
        let make = |setup: &SourceSetup| {
            let mut engine = Engine::with_ids(ids.clone());
            if several {
                engine.set_source_edition(setup.edition);
            }
            Source {
                engine,
                spec: setup.spec.clone(),
                last_list: ListKey::default(),
            }
        };
        let primary = make(setups.first().unwrap_or(&SourceSetup::default()));
        let more = setups.iter().skip(1).map(make).collect();
        Self {
            primary,
            more,
            ids,
            live: 0,
            running: Running::default(),
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

    /// Feed source `i` one tail event.
    pub fn on_tail(&mut self, i: usize, ev: TailEvent, out: &mut Vec<EngineEvent>) {
        self.engine_mut(i).on_tail(ev, out);
        // A source's edition may have just been named or changed: whether
        // a game runs for it may have too.
        self.apply_running();
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
        let verdicts: Vec<bool> = (0..self.len()).map(|i| self.runs_for(i)).collect();
        for (engine, v) in self.engines_mut().zip(verdicts) {
            engine.game_running = v;
        }
    }

    /// Choose the live edition again, after a tail event of source
    /// `opened`'s that opened a pull (`Some`) or anything else (`None`).
    /// `true` when it moved: every following session must be moved with it.
    /// One source is always live.
    pub fn choose_live(&mut self, opened: Option<usize>) -> bool {
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
            // or it is still being written.
            [] => self.keep_or_move(old, &started, opened, Move::WrittenLast),
            // Both do: the one with a pull open, kept while it has one; with
            // neither open, kept until the other opens one.
            several => self.keep_or_move(old, several, opened, Move::OnPull),
        };
        self.live = new;
        new != old
    }

    /// Among `candidates`: the live one stays while it is one of them and
    /// has a pull open. Otherwise the candidate with a pull open is live —
    /// the one that just opened it (`opened`) first — and with none open
    /// the live one stays: while both games run ([`Move::OnPull`]), or with
    /// none running while its log is still being written
    /// (`Engine::being_written`); once it has gone quiet it gives way to the
    /// one whose log was written last ([`Move::WrittenLast`], and whenever
    /// the live one is no candidate).
    fn keep_or_move(
        &self,
        old: usize,
        candidates: &[usize],
        opened: Option<usize>,
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
        if is_candidate && (mode == Move::OnPull || self.engine(old).being_written()) {
            return old;
        }
        candidates
            .iter()
            .copied()
            .max_by_key(|i| (self.engine(*i).written(), *i == old))
            .unwrap_or(old)
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

/// What lets the live edition move off a source with no pull open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    /// Another source's log written later (no followed game runs).
    WrittenLast,
    /// Another source opening a pull (both followed games run).
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
