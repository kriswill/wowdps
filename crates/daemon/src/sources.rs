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
//! THE RULE. The live edition is the started source whose log shows the
//! newest line time. An open pull in the live edition holds it, and so does
//! a move made less than HANDOFF_QUIET ago. When the watcher names exactly
//! one followed game running, that game is live. A source with no timed
//! line is ranked by its log's modified time.
//!
//! In its terms: a started source is one whose folder has held a log; its
//! log's newest line time is `Engine::line_ms` — each line's own date, time
//! and UTC offset (`engine::line_utc_ms`), kept through a rotation, and a
//! time read ahead of now believed at most `engine::LINE_AHEAD` past the
//! clock; an open pull is `Engine::pull_open`; a move is the live edition
//! changing from one started source with a rank (a line time or a modified
//! time) to another ([`HANDOFF_QUIET`]); its modified time is
//! `Engine::written`. Ties keep the live edition. It is
//! chosen on every tick and every game report ([`Sources::choose_live`]),
//! so the lines that land within one tick are judged together; with one
//! source it never moves.
//!
//! It rests on two premises: a game writes no line after its player logs
//! out, and both logs' line times are on one clock — and on the game's own
//! fact that one account is in the world in one edition at a time, so going
//! from one game to the other logs the first out. Under them a game left
//! behind, flushing late or on exit, shows no line newer than the game the
//! player moved to has shown, and takes nothing; a game moved to that has
//! logged nothing yet takes nothing until it does. Where they fail, the
//! rule follows the line times: a line written after the logout takes the
//! live edition; a second client in the world, such as another account's,
//! writes current lines and takes it at its flushes. The cap bounds only a live log read ahead,
//! holding the live edition a minute at most: an old game read ahead still
//! takes it with a late flush, and a log read behind is not followed until
//! its lines pass the other's newest. Only the live
//! engine's trailing segment is what a follower is shown: a stale open
//! pull in another edition's log draws no one.
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

use std::time::{Duration, Instant, SystemTime};

use wowdps_core::model::SegmentId;
use wowdps_core::tail::TailEvent;
use wowdps_model::Edition;
use wowdps_proto::{Cursor, EditionRef, EditionSource, LoadError, SegmentRef};

use crate::engine::{Engine, EngineEvent, Ids, ListKey};
use crate::game::Running;

/// How long a move holds the live edition (THE RULE): a change of the live
/// edition from one started source to another holds it this long, whatever
/// the other log shows, so two logs written in turn never move it twice
/// within this interval. Only the watcher naming exactly one followed game
/// running moves it sooner.
pub const HANDOFF_QUIET: Duration = Duration::from_secs(10);

/// v49: one followed source as the hub is told of it: how `Status` words
/// what is followed (`spec_display`), and the edition its folder names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSetup {
    pub spec: Option<String>,
    pub edition: Option<Edition>,
    /// R31: the source is one the config or install discovery names
    /// (`Followed::own`), so its fights' logger is the account's.
    pub own: bool,
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
    /// When the live edition last moved from one started source with a rank
    /// to another (`None` before it has): the move holds it for
    /// [`HANDOFF_QUIET`].
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
            engine: {
                let mut engine = Engine::with_ids(ids.clone());
                engine.set_own(setup.own);
                engine
            },
            spec: setup.spec.clone(),
            last_list: ListKey::default(),
            folder: setup.edition,
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
        self.engine_mut(i).on_tail(ev, out);
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

    /// Choose the live edition by THE RULE (the module's doc): the watcher
    /// naming exactly one followed game running decides; else an open pull
    /// in the live edition, or a move made less than [`HANDOFF_QUIET`] ago,
    /// holds it; else the started source whose log shows the newest line
    /// time ([`Sources::rank`]), ties keeping the live one. `true` when it
    /// moved: every following session must be moved with it. One source is
    /// always live.
    pub fn choose_live(&mut self) -> bool {
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
        let recent = self.moved_at.is_some_and(|t| t.elapsed() < HANDOFF_QUIET);
        let held = self.started(old) && (self.engine(old).pull_open() || recent);
        let new = match running.as_slice() {
            [one] => *one,
            _ if held => old,
            _ => started
                .iter()
                .copied()
                .max_by_key(|i| (self.rank(*i), *i == old))
                .unwrap_or(old),
        };
        self.live = new;
        // A pick from a source that never held a log, or that has shown
        // nothing to rank it by yet (its index not landed), is no move.
        if new != old && self.started(old) && self.rank(old).is_some() {
            self.moved_at = Some(Instant::now());
        }
        new != old
    }

    /// What source `i` is ranked by for the live edition: the newest line
    /// time its log shows (`Engine::line_ms`), else, with no timed line, its
    /// log's modified time (`Engine::written`), both in UTC milliseconds.
    fn rank(&self, i: usize) -> Option<i64> {
        let engine = self.engine(i);
        engine.line_ms().or_else(|| {
            engine
                .written()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
        })
    }

    /// Test hook: as if `by` passed with nothing written to any log — the
    /// move's hold and every engine's clocks ([`Engine::age_for_test`]).
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
