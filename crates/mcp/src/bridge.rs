//! The daemon side of the MCP server: one `DaemonClient` connection reused
//! across tool calls. MCP is request/response while the daemon is
//! subscription-push, so each call declares the cursor it needs and blocks
//! (bounded) for the first snapshot that answers it — the coalescing inbox
//! already guarantees that snapshot is the freshest one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use wowdps_model::Edition;
use wowdps_model::SegmentId;
use wowdps_proto::{
    ClientKind, ClientMsg, CompareSide, Cursor, DaemonClient, DaemonMsg, EditionRef, EditionSource,
    HistoryAnswer, HistoryQuery, HistoryStatus, ListEntry, OverlayState, SegmentRef, StoredFight,
};

/// Historical segments go through the loader pool; a cold 300 MB log segment
/// can take a few seconds. Anything beyond this is a wedged daemon.
const DEADLINE: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(10);

/// What an offline bridge's every daemon call answers.
pub const OFFLINE: &str = "offline bridge: this one never reaches a daemon";

pub struct Bridge {
    /// `None` until the first call that needs the daemon: registering the
    /// MCP server in a harness must not spawn a daemon, and an unreachable
    /// daemon must surface as a tool-level error, not a dead transport.
    client: Option<DaemonClient>,
    /// Never connect or spawn: every call that needs the daemon fails with
    /// [`OFFLINE`] ([`Bridge::offline`], the unit tests').
    offline: bool,
    next_req: u32,
    /// v49: the edition the live tools look in for this call — following
    /// the live edition, or pinned to the one its `edition` names.
    scope: EditionRef,
    /// The edition of the last Watch: a list or `Live` Watch in another
    /// one is fenced ([`Bridge::watch`]), so no frame pushed before it
    /// answers it.
    watched: Option<EditionRef>,
    /// Each log's identity by the list that named it — its edition and its
    /// file — so a fight header can name its history id without a second
    /// list round trip, whichever edition's log the fight is in.
    log_ids: Vec<(Option<Edition>, Option<String>, Option<u64>)>,
}

/// One `DaemonMsg::SegmentList`, unpacked for the tools.
pub struct Segments {
    pub entries: Vec<ListEntry>,
    pub active: bool,
    pub source: Option<String>,
    /// The tailed log's identity once its header is complete: with a
    /// closed row's `start_ms` it names the history-store fight id.
    pub log_id: Option<u64>,
    /// v49: the edition the list is of — the pinned one, else the live one.
    pub edition: Option<Edition>,
}

/// One `DaemonMsg::Snapshot`, unpacked for the tools.
pub struct Snap {
    pub id: Option<SegmentId>,
    pub info: wowdps_model::SegmentInfo,
    pub rows: Vec<wowdps_model::Row>,
    pub total_rows: u32,
    pub breakdown: Option<wowdps_proto::Breakdown>,
    /// v35 (R25): the whole group's pull.
    pub raid: Option<wowdps_model::RaidTimeline>,
    /// v49: the log the segment is in, by name, and its source's edition
    /// (`DaemonMsg::Snapshot`'s `source` and `edition`): what
    /// [`Bridge::log_id_of`] names its history id by.
    pub source: Option<String>,
    pub edition: Option<Edition>,
}

/// One `DaemonMsg::CompareSnapshot`, unpacked for the tools.
pub struct Pair {
    pub info: wowdps_model::SegmentInfo,
    pub a: CompareSide,
    pub b: CompareSide,
    /// As [`Snap`]'s.
    pub source: Option<String>,
    pub edition: Option<Edition>,
}

pub struct Status {
    pub game_running: bool,
    pub source: Option<String>,
    pub clients: u32,
    pub linger: bool,
    pub overlay: OverlayState,
    pub history: HistoryStatus,
    /// v49: every edition's source, the live one marked.
    pub editions: Vec<EditionSource>,
}

impl Bridge {
    /// No I/O yet: the daemon is reached (and spawned on demand) by the
    /// first tool call that needs it, via [`Bridge::client`].
    pub fn lazy() -> Bridge {
        Bridge {
            client: None,
            offline: false,
            next_req: 1,
            scope: EditionRef::Follow,
            watched: None,
            log_ids: Vec::new(),
        }
    }

    /// One that never connects, finds or spawns a daemon (unit tests): a
    /// tool that asks it anything gets [`OFFLINE`] back, so a test of the
    /// errors a tool raises before it asks cannot pass by reaching the
    /// user's live daemon, nor fail for want of one.
    #[cfg(test)]
    pub(crate) fn offline() -> Bridge {
        Bridge {
            offline: true,
            ..Bridge::lazy()
        }
    }

    /// Over an existing stream (tests).
    pub fn over(stream: std::os::unix::net::UnixStream) -> std::io::Result<Bridge> {
        let client = DaemonClient::over(stream, ClientKind::Mcp)?;
        Ok(Bridge {
            client: Some(client),
            offline: false,
            next_req: 1,
            scope: EditionRef::Follow,
            watched: None,
            log_ids: Vec::new(),
        })
    }

    /// The live connection: connect on first use, spawning the daemon on
    /// demand — the daemon binary is the `wowdps` dispatcher itself,
    /// preferred as a sibling of this binary (same build), else found on
    /// $PATH. A daemon that idle-exited between tool calls is respawned,
    /// not reported — the next answer is what the caller wants either way.
    fn client(&mut self) -> Result<&mut DaemonClient, String> {
        if self.offline {
            return Err(OFFLINE.to_string());
        }
        if self.client.is_none() {
            let bin = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("wowdps")))
                .filter(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from("wowdps"));
            let client = DaemonClient::connect(&bin, None, ClientKind::Mcp)
                .map_err(|e| format!("cannot reach or spawn the daemon: {e}"))?;
            self.client = Some(client);
        }
        let Some(client) = self.client.as_mut() else {
            return Err("daemon connection missing after connect".to_string());
        };
        if client.is_dead() {
            client.reconnect_if_dead();
        }
        Ok(client)
    }

    pub fn status(&mut self) -> Result<Status, String> {
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.send(&ClientMsg::GetStatus { req_id });
        wait(client, |msg| match msg {
            DaemonMsg::Status {
                req_id: got,
                game_running,
                source,
                clients,
                linger,
                overlay,
                history,
                editions,
            } if got == req_id => Some(Status {
                game_running,
                source,
                clients,
                linger,
                overlay,
                history,
                editions,
            }),
            _ => None,
        })
    }

    /// v49: the edition this call's live tools look in — `Follow` unless a
    /// tool's `edition` pins one. Set by every tool call before it asks.
    pub fn set_scope(&mut self, scope: EditionRef) {
        self.scope = scope.canonical();
    }

    pub fn scope(&self) -> EditionRef {
        self.scope
    }

    /// Declare `cursor` in `edition`. Ids answer the same in every
    /// edition, but the list and `Live` resolve inside it, and frames the
    /// daemon pushed before it took this Watch may still be on the way —
    /// the old cursor's, and the lists it broadcasts in the session's
    /// edition. When the last Watch was in another edition, a status round
    /// trip fences them (the daemon answers in order, so every frame after
    /// its answer is the new cursor's) and the cursor is declared again,
    /// so a frame that answers it comes after the fence. A frame pushed in
    /// the same edition answers alike, as before editions.
    fn watch(&mut self, cursor: Cursor, edition: EditionRef) -> Result<(), String> {
        let scoped = match &cursor {
            Cursor::List => true,
            Cursor::Segment { segment, .. } | Cursor::Compare { segment, .. } => {
                *segment == SegmentRef::Live
            }
        };
        let fence = scoped && self.watched.is_some_and(|w| w != edition);
        self.watched = Some(edition);
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.watch_in(cursor.clone(), edition);
        if fence {
            client.send(&ClientMsg::GetStatus { req_id });
            wait(client, |msg| match msg {
                DaemonMsg::Status { req_id: got, .. } if got == req_id => Some(()),
                _ => None,
            })?;
            client.watch_in(cursor, edition);
        }
        Ok(())
    }

    /// The segment list of this call's edition plus the daemon's liveness
    /// verdict.
    pub fn segments(&mut self) -> Result<Segments, String> {
        self.segments_in(self.scope)
    }

    /// The segment list of one edition (or the live one's, following).
    pub fn segments_in(&mut self, edition: EditionRef) -> Result<Segments, String> {
        let edition = edition.canonical();
        self.watch(Cursor::List, edition)?;
        let want = match edition {
            EditionRef::Pin(e) => Some(e),
            EditionRef::Follow => None,
        };
        let client = self.client()?;
        let segs = wait(client, |msg| match msg {
            DaemonMsg::SegmentList {
                entries,
                active,
                source,
                log_id,
                edition,
                ..
            } if want.is_none() || edition == want => Some(Segments {
                entries,
                active,
                source,
                log_id,
                edition,
            }),
            _ => None,
        })?;
        self.remember_log(segs.edition, segs.source.clone(), segs.log_id);
        Ok(segs)
    }

    /// The identity of the log a frame named (its source's edition and its
    /// file, [`Snap::edition`] / [`Snap::source`]): from the last list of
    /// that log, else that edition's list now. `None` while the log's
    /// header is not whole, and for a log no list names any more (rotated
    /// away): no history id is better than another log's.
    pub fn log_id_of(
        &mut self,
        edition: Option<Edition>,
        source: Option<&str>,
    ) -> Result<Option<u64>, String> {
        let edition = edition.map(Edition::canonical);
        let known = |ids: &[(Option<Edition>, Option<String>, Option<u64>)]| {
            ids.iter()
                .find(|(e, s, _)| *e == edition && s.as_deref() == source)
                .map(|(_, _, id)| *id)
        };
        if let Some(Some(id)) = known(&self.log_ids) {
            return Ok(Some(id));
        }
        let segs = self.segments_in(edition.map_or(EditionRef::Follow, EditionRef::Pin))?;
        Ok(segs.log_id.filter(|_| segs.source.as_deref() == source))
    }

    fn remember_log(&mut self, edition: Option<Edition>, source: Option<String>, id: Option<u64>) {
        let edition = edition.map(Edition::canonical);
        self.log_ids.retain(|(e, _, _)| *e != edition);
        self.log_ids.push((edition, source, id));
    }

    /// One meter snapshot for `cursor` (which must be a `Cursor::Segment`).
    pub fn snapshot(&mut self, cursor: Cursor) -> Result<Snap, String> {
        let (want_seg, want_view, want_drill) = match &cursor {
            Cursor::Segment {
                segment,
                view,
                drill,
                ..
            } => (*segment, *view, drill.is_some()),
            _ => return Err("snapshot() takes a segment cursor".to_string()),
        };
        let scope = self.scope;
        self.watch(cursor, scope)?;
        let client = self.client()?;
        wait(client, |msg| match msg {
            DaemonMsg::Snapshot {
                segment,
                id,
                view,
                info,
                rows,
                total_rows,
                breakdown,
                ref status,
                raid,
                source,
                edition,
                ..
            } if segment == want_seg
                && view == want_view
                && breakdown.is_some() == want_drill
                && !is_loading(status.as_deref()) =>
            {
                Some(Ok(Snap {
                    id,
                    info,
                    rows,
                    total_rows,
                    breakdown,
                    raid,
                    source,
                    edition,
                }))
            }
            // Only one cursor is ever outstanding, so any load failure is ours.
            DaemonMsg::LoadFailed { error, .. } => Some(Err(load_error(error))),
            _ => None,
        })?
    }

    /// One player's COMBATANT_INFO loadout for one segment (v19). `None` is
    /// an answer, not a failure: an unknown guid, or a player whose
    /// COMBATANT_INFO never fired (the game logs one only inside instances).
    pub fn loadout(
        &mut self,
        segment: SegmentRef,
        guid: String,
    ) -> Result<Option<wowdps_model::Loadout>, String> {
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.send(&ClientMsg::GetLoadout {
            req_id,
            segment,
            guid,
        });
        wait(client, |msg| match msg {
            DaemonMsg::Loadout {
                req_id: got,
                loadout,
                ..
            } if got == req_id => Some(Ok(loadout)),
            // The hub answers a failed load with `None`, which would read as
            // "never logged"; the failure it pushes alongside is ours (the
            // watch cursor is parked on the same segment), so surface it.
            DaemonMsg::LoadFailed { error, .. } => Some(Err(load_error(error))),
            _ => None,
        })?
    }

    /// One comparison snapshot: two players of one segment, side by side.
    pub fn compare(&mut self, segment: SegmentRef, a: String, b: String) -> Result<Pair, String> {
        let scope = self.scope;
        self.watch(
            Cursor::Compare {
                segment,
                a,
                b,
                // The `compare` tool answers about damage dealt; a taken
                // comparison is `breakdown`/`stored_fight`'s `view: "taken"`.
                view: wowdps_model::View::Damage,
                range: None,
                spell: None,
            },
            scope,
        )?;
        let client = self.client()?;
        wait(client, |msg| match msg {
            DaemonMsg::CompareSnapshot {
                segment: got,
                info,
                a,
                b,
                ref status,
                source,
                edition,
                ..
            } if got == segment && !is_loading(status.as_deref()) => Some(Ok(Pair {
                info,
                a: *a,
                b: *b,
                source,
                edition,
            })),
            DaemonMsg::LoadFailed { error, .. } => Some(Err(load_error(error))),
            _ => None,
        })?
    }
}

/// Block (bounded) until `pick` claims a message.
impl Bridge {
    /// v20: one of the store's fixed questions. Always answered; a
    /// disabled store answers empty.
    pub fn history(&mut self, query: HistoryQuery) -> Result<HistoryAnswer, String> {
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.send(&ClientMsg::GetHistory { req_id, query });
        wait(client, |msg| match msg {
            DaemonMsg::History {
                req_id: got,
                answer,
            } if got == req_id => Some(Ok(answer)),
            _ => None,
        })?
    }

    /// v20: one stored fight; `None` when unknown or evicted.
    pub fn stored_fight(
        &mut self,
        fight_id: String,
        view: wowdps_model::View,
        drill: Option<String>,
        death: Option<u32>,
    ) -> Result<Option<StoredFight>, String> {
        self.stored_fight_boss(fight_id, view, drill, death, None, None)
    }

    /// `stored_fight` for one of a key's member bosses (name or index into
    /// the card's `bosses`), parsed from the log on demand.
    /// v39: `range` is a zoom window in ms from the fight's start — a
    /// Damage or Healing drill of a fight with the series tier answers it.
    pub fn stored_fight_boss(
        &mut self,
        fight_id: String,
        view: wowdps_model::View,
        drill: Option<String>,
        death: Option<u32>,
        boss: Option<String>,
        range: Option<(u32, u32)>,
    ) -> Result<Option<StoredFight>, String> {
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.send(&ClientMsg::GetFight {
            req_id,
            fight_id,
            view,
            drill,
            death,
            boss,
            range,
            spell: None,
            pair: None,
        });
        wait(client, |msg| match msg {
            DaemonMsg::Fight { req_id: got, fight } if got == req_id => Some(Ok(fight)),
            _ => None,
        })?
    }

    /// v20: pin or release a stored fight; the answer is the resulting state.
    /// Queue a rewrite of stored cards from their logs; the count queued.
    pub fn regrade(
        &mut self,
        fight_id: Option<String>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        kind: Option<wowdps_proto::history::FightKind>,
        edition: Option<Edition>,
    ) -> Result<u32, String> {
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.send(&ClientMsg::Regrade {
            req_id,
            fight_id,
            encounter,
            difficulty,
            kind,
            edition,
        });
        wait(client, |msg| match msg {
            DaemonMsg::History {
                req_id: got,
                answer: HistoryAnswer::Regraded { queued },
            } if got == req_id => Some(Ok(queued)),
            _ => None,
        })?
    }

    pub fn pin_fight(&mut self, fight_id: String, pinned: bool) -> Result<bool, String> {
        let req_id = self.next_req;
        self.next_req += 1;
        let client = self.client()?;
        client.send(&ClientMsg::PinFight {
            req_id,
            fight_id,
            pinned,
        });
        wait(client, |msg| match msg {
            DaemonMsg::History {
                req_id: got,
                answer: HistoryAnswer::Pinned { pinned, .. },
            } if got == req_id => Some(Ok(pinned)),
            _ => None,
        })?
    }
}

fn wait<T>(
    client: &mut DaemonClient,
    mut pick: impl FnMut(DaemonMsg) -> Option<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + DEADLINE;
    loop {
        for msg in client.poll() {
            if let DaemonMsg::Fatal(e) = &msg {
                return Err(format!("daemon: {e}"));
            }
            if let Some(t) = pick(msg) {
                return Ok(t);
            }
        }
        if client.is_dead() {
            return Err("daemon connection lost".to_string());
        }
        if Instant::now() >= deadline {
            return Err("daemon did not answer in time".to_string());
        }
        std::thread::sleep(POLL);
    }
}

/// The hub answers a watch on a cold segment immediately with a placeholder
/// — empty rows, status `loading <name>…` — then pushes the real snapshot
/// when the loader delivers. Interactive clients paint the placeholder; a
/// request/response bridge must wait through it. The status string is its
/// only wire marker, shared with the daemon via `wowdps_proto`.
fn is_loading(status: Option<&str>) -> bool {
    status.is_some_and(wowdps_proto::is_loading_status)
}

fn load_error(e: wowdps_proto::LoadError) -> String {
    match e {
        wowdps_proto::LoadError::NotFound => {
            "no such segment (ids are per daemon run — call list_fights again)".to_string()
        }
        wowdps_proto::LoadError::Rotated => {
            "the log rotated out from under that segment".to_string()
        }
        wowdps_proto::LoadError::Io(msg) => format!("loading the segment failed: {msg}"),
    }
}
