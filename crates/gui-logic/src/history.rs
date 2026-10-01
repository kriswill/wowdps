//! The history store as the window reads it: the pages of stored fights the
//! pull rail lists under tonight's ([`Earlier`]), and a stored pull opened
//! on the stage ([`Stored`]). Window-local like Home: `ClientState` never
//! learns a store exists.
//!
//! A stored pull is drawn by the code that draws a live one. `GetFight`
//! answers in a live snapshot's shape, so the window feeds each answer to a
//! `ClientState` of the pull's own — one list entry, the card as its
//! segment — and the meter, the fight header and the inspector read THAT
//! where they would read the log's, never learning which they draw. What
//! that state asks for (a `Watch` naming a view, a drill, a death window)
//! becomes the `GetFight` that answers it, one in flight: a held `j` asks
//! for the player it lands on, not for every one it passed — and one for the
//! whole window: a pull the reader steps onto while the last one's read is
//! still out waits for that answer before it asks (a held `[` walks the
//! rail without stacking reads in the daemon's queue). What the store does
//! not keep has no request to become: a comparison, an ability's own curve,
//! the enemies' view.
//!
//! The daemon answers a read it refuses — its read quota full, the store
//! off — as it answers for a fight it does not hold: empty (a `Fight` with
//! no fight, a page of no cards and a total of 0). So an empty answer is
//! asked again once, after [`RETRY_AFTER`] (a quota full now is a quota
//! full in the same instant), before a fight is called gone; and a page's
//! total of 0 while cards are in hand is never taken as the store's — the
//! second such answer leaves the rail as it stood, its total with it.

use std::time::{Duration, Instant};

use wowdps_model::{Drill, ListRow, Loadout, SegmentId, SegmentInfo, SegmentKind, View};
use wowdps_proto::history::{FightCard, FightKind};
use wowdps_proto::{
    ClientMsg, ClientState, Cursor, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery, ListEntry,
    SegmentRef, StoredFight,
};

/// Cards per request — the rail's pages and Home's alike. Small enough that
/// the answer is one modest frame and the first screenful arrives quickly;
/// the daemon caps it anyway (§2).
pub const PAGE: u32 = 200;

// ---- the earlier nights ------------------------------------------------------

/// How long an answer that looked like a refused read waits before it is
/// asked again: long enough for the daemon's read queue to drain.
pub const RETRY_AFTER: Duration = Duration::from_millis(750);

/// The newest cards a store write asks for: what the fight it wrote
/// brought, merged over the pages in hand — not a whole page again for
/// every closed pull, trash included.
pub const FRESH: u32 = 20;

/// Which page a request asks for: the newest (the first page), the few
/// newest cards again after the store wrote a fight, or the page after the
/// oldest card in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Newest,
    Fresh,
    Older,
}

/// The stored fights the rail lists under tonight's: pages of
/// `HistoryQuery::Fights`, newest first, merged by id. One request is in
/// flight at a time — the client half of the daemon's read quota, which
/// keeps half its queue for the writes a closing pull needs — and none asks
/// for more than [`PAGE`] cards, under the store's `FIGHTS_CAP`.
#[derive(Debug, Default)]
pub struct Earlier {
    /// Every card in hand, newest first, each once.
    pub cards: Vec<FightCard>,
    /// The request in flight, and which page it asks for.
    pending: Option<(u32, Page)>,
    /// Asked for and not yet sent, each kept until it goes: the newest page
    /// first (at launch), the fresh cards (the store wrote a fight), then
    /// the older one the reader asked for — none overwrites another.
    want_newest: bool,
    want_fresh: bool,
    want_older: bool,
    /// The page in flight is a second asking after an answer that looked
    /// like a refusal: a second such answer leaves the rail as it stands.
    again: bool,
    /// That second asking waits until then.
    retry_at: Option<Instant>,
    /// The id the next older page starts after: the oldest card of the
    /// last page that went back in time.
    cursor: Option<String>,
    /// How many cards the store holds, as its last answer counted them.
    total: Option<u32>,
    /// An answer has landed: an empty list is the store's, not a wait.
    pub answered: bool,
}

impl Earlier {
    /// Ask for the newest page — at launch, and whenever the store wrote a
    /// fight — merged over what is in hand, so the older pages the reader
    /// already paged in stay.
    pub fn want_newest(&mut self) {
        self.want_newest = true;
    }

    /// The store wrote a fight: its newest few cards, merged over what is
    /// in hand — or the whole first page, while none has landed.
    pub fn want_fresh(&mut self) {
        if self.answered {
            self.want_fresh = true;
        } else {
            self.want_newest = true;
        }
    }

    /// "Show older nights": the page after the oldest card in hand.
    pub fn want_older(&mut self) {
        if self.more() {
            self.want_older = true;
        }
    }

    /// There is more of the store than the rail holds.
    pub fn more(&self) -> bool {
        self.total.is_none_or(|t| (self.cards.len() as u32) < t)
    }

    /// A page is on its way.
    pub fn asking(&self) -> bool {
        self.pending.is_some()
    }

    /// The connection the request in flight went out on is gone (the
    /// daemon restarted): nothing will answer it. The newest page is asked
    /// for again, and an older one the reader wanted stays wanted.
    pub fn lost(&mut self) {
        if let Some((_, Page::Older)) = self.pending.take() {
            self.want_older = true;
        }
        self.again = false;
        self.retry_at = None;
        self.want_newest = true;
    }

    /// The request to send `now`, if any: what was asked for, once nothing
    /// is in flight and no second asking is waiting out its pause.
    pub fn next_request(&mut self, req_id: u32, now: Instant) -> Option<ClientMsg> {
        if self.pending.is_some() || self.retry_at.is_some_and(|at| now < at) {
            return None;
        }
        self.retry_at = None;
        let page = if std::mem::take(&mut self.want_newest) {
            // The first page holds the few newest cards too.
            self.want_fresh = false;
            Page::Newest
        } else if std::mem::take(&mut self.want_fresh) {
            Page::Fresh
        } else if std::mem::take(&mut self.want_older) {
            Page::Older
        } else {
            return None;
        };
        let after_id = match page {
            Page::Newest | Page::Fresh => None,
            Page::Older => Some(self.cursor.clone()?),
        };
        self.pending = Some((req_id, page));
        Some(ClientMsg::GetHistory {
            req_id,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                // Every character's: the rail is the whole store's, and a
                // dot says whose each pull was.
                guid: None,
                since_utc_ms: None,
                kind: None,
                sort: FightSort::Newest,
                limit: match page {
                    Page::Fresh => FRESH,
                    Page::Newest | Page::Older => PAGE,
                },
                after_id,
                role: None,
            },
        })
    }

    /// Fold an answer in; `true` when it was this pager's.
    pub fn absorb(&mut self, req_id: u32, answer: &HistoryAnswer) -> bool {
        if let HistoryAnswer::Pinned { fight_id, pinned } = answer {
            return self.pinned(fight_id, *pinned);
        }
        let Some((asked, page)) = self.pending else {
            return false;
        };
        if asked != req_id {
            return false;
        }
        self.pending = None;
        let HistoryAnswer::Fights { cards, total } = answer else {
            return true;
        };
        // No cards and a total of 0 while the rail holds cards is how the
        // daemon refuses a read (its quota full): the page is asked for
        // again, once, after a pause, and the total in hand stands — or
        // "Show older nights" would vanish over a store that still holds
        // them. A second such answer changes nothing either: the quota may
        // still be full, and the next want (a store write, the button)
        // asks again.
        if cards.is_empty() && *total == 0 && !self.cards.is_empty() {
            if !std::mem::replace(&mut self.again, true) {
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
                match page {
                    Page::Newest => self.want_newest = true,
                    Page::Fresh => self.want_fresh = true,
                    Page::Older => self.want_older = true,
                }
            } else {
                self.again = false;
            }
            self.answered = true;
            return true;
        }
        self.again = false;
        self.answered = true;
        self.total = Some(*total);
        for c in cards {
            match self.cards.iter_mut().find(|have| have.id == c.id) {
                // A newer copy of a card in hand: a pin, a regrade.
                Some(have) => *have = c.clone(),
                None => self.cards.push(c.clone()),
            }
        }
        self.cards
            .sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
        let last = cards.last().map(|c| c.id.clone());
        self.cursor = match page {
            Page::Older => last.or(self.cursor.take()),
            // The newest cards move the cursor only when there was none: the
            // reader's older pages are further back than they go.
            Page::Newest | Page::Fresh => self.cursor.take().or(last),
        };
        true
    }

    /// The store pinned (or let go of) `fight_id`: the card in hand says
    /// so. `true` when the rail holds it.
    pub fn pinned(&mut self, fight_id: &str, pinned: bool) -> bool {
        match self.cards.iter_mut().find(|c| c.id == fight_id) {
            Some(c) => {
                c.pinned = pinned;
                true
            }
            None => false,
        }
    }

    /// A card the window holds from elsewhere (Home's lists), so the rail
    /// can place the pull it opens.
    pub fn adopt(&mut self, card: FightCard) {
        if !self.cards.iter().any(|c| c.id == card.id) {
            self.cards.push(card);
            self.cards
                .sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
        }
    }

    pub fn card(&self, fight_id: &str) -> Option<&FightCard> {
        self.cards.iter().find(|c| c.id == fight_id)
    }
}

// ---- a stored pull on the stage ----------------------------------------------

/// What a `GetFight` asks the store for: the view, whose drill, which of
/// their deaths.
#[derive(Debug, Clone, PartialEq)]
struct Want {
    view: View,
    drill: Option<String>,
    death: Option<u32>,
}

/// A stored pull on the stage: its own `ClientState`, fed from `GetFight`.
pub struct Stored {
    pub fight_id: String,
    /// The pull as the stage reads it — following the selection, as the
    /// window's own state does.
    pub state: ClientState,
    /// Its card: the one it was opened from, then the store's answer's.
    pub card: Option<FightCard>,
    /// The `GetFight` in flight, and what it asks for — nothing of this
    /// pull's when it is the read a pull the reader left still has out,
    /// which this one waits on ([`Stored::open`]'s `inherit`).
    pending: Option<(u32, Option<Want>)>,
    /// What the state asked for since that one went out — the newest only:
    /// it goes out when the one in flight answers.
    queued: Option<Want>,
    /// The request in flight asks again after an empty answer.
    again: bool,
    /// That second asking, waiting out its pause: when, and what it asks.
    retry: Option<(Instant, Want)>,
    /// The store answered twice that it has no such fight (evicted since
    /// the rail listed it, or the store is off).
    pub missing: bool,
    /// The last answer drilled into a player and came without their
    /// breakdown: the store kept this pull's rows, not its details.
    bare: bool,
    /// The drilled player's logged loadout, as the last answer carried it:
    /// their guid and their build.
    loadout: Option<(String, Loadout)>,
}

impl Stored {
    /// Open `fight_id` on `view` — the view the stage was on, which a
    /// stored pull keeps when it has it — with `drill` (the player the
    /// stage was inspecting) carried over, and ask for it. `card` is the
    /// rail's, when it has one. `inherit` is a `GetFight` a pull the reader
    /// just left still has out: this pull's own waits for its answer, so
    /// one read is out for the whole window however fast the rail is walked.
    pub fn open(
        fight_id: String,
        card: Option<FightCard>,
        view: View,
        drill: Option<Drill>,
        inherit: Option<u32>,
        next_id: &mut u32,
    ) -> (Self, Vec<ClientMsg>) {
        let mut state = ClientState::new();
        let _ = state.set_follow(true);
        state.view = if view.is_stored() { view } else { View::Damage };
        // The row is a placeholder until the card is known: the header reads
        // the answer's own, and the list holds only the pull's id.
        let row = list_row(&card.as_ref().map_or_else(pending_info, info_of));
        let _ = state.on_msg(DaemonMsg::SegmentList {
            seq: 0,
            entries: vec![ListEntry {
                id: segment_id(&fight_id),
                row,
            }],
            source: Some(source_of(&fight_id)),
            active: false,
            log_id: None,
        });
        // The player carries over, their list with them; a spell of theirs
        // does not — the store keeps no ability's own curve.
        state.drill = drill.map(|d| Drill {
            spell: None,
            spell_sel: 0,
            target_sel: 0,
            ..d
        });
        let sent = state.goto_list_pos(0);
        let mut stored = Stored {
            fight_id,
            state,
            card,
            pending: inherit.map(|id| (id, None)),
            queued: None,
            again: false,
            retry: None,
            missing: false,
            bare: false,
            loadout: None,
        };
        let asked = stored.route(sent, next_id);
        (stored, asked)
    }

    /// The `GetFight` still out, by its req_id: what a pull opened next
    /// inherits when the reader leaves this one before it answers.
    pub fn in_flight(&self) -> Option<u32> {
        self.pending.as_ref().map(|(id, _)| *id)
    }

    /// The window's tick: a second asking whose pause is over goes out.
    pub fn tick(&mut self, now: Instant, next_id: &mut u32) -> Vec<ClientMsg> {
        if self.pending.is_some() || self.retry.as_ref().is_none_or(|(at, _)| now < *at) {
            return Vec::new();
        }
        match self.retry.take() {
            Some((_, want)) => self.ask(want, next_id),
            None => Vec::new(),
        }
    }

    /// What the pull's own state asked for, as the store answers it: its
    /// newest `Watch` of the meter becomes the `GetFight` for the same
    /// view, drill and death window — now, or, while one is out, when that
    /// one answers. A comparison's cursor has no stored answer and asks for
    /// nothing.
    pub fn route(&mut self, sent: Vec<ClientMsg>, next_id: &mut u32) -> Vec<ClientMsg> {
        let Some(want) = sent.into_iter().rev().find_map(|m| match m {
            ClientMsg::Watch(Cursor::Segment {
                view, drill, death, ..
            }) => Some(Want { view, drill, death }),
            _ => None,
        }) else {
            return Vec::new();
        };
        if self.pending.is_some() {
            self.queued = Some(want);
            return Vec::new();
        }
        // A second asking waiting out its pause keeps its turn: the newest
        // want takes it — and, another than the one refused, is its first.
        if let Some((at, parked)) = self.retry.take() {
            if parked != want {
                self.again = false;
            }
            self.retry = Some((at, want));
            return Vec::new();
        }
        self.again = false;
        self.ask(want, next_id)
    }

    /// Send `want`, as the one request in flight.
    fn ask(&mut self, want: Want, next_id: &mut u32) -> Vec<ClientMsg> {
        let req_id = *next_id;
        *next_id = next_id.wrapping_add(1);
        let msg = ClientMsg::GetFight {
            req_id,
            fight_id: self.fight_id.clone(),
            view: want.view,
            drill: want.drill.clone(),
            death: want.death,
            boss: None,
        };
        self.pending = Some((req_id, Some(want)));
        vec![msg]
    }

    /// The connection the request in flight went out on is gone (the
    /// daemon restarted): ask again, on the new one, for what the state
    /// shows now.
    pub fn lost(&mut self, next_id: &mut u32) -> Vec<ClientMsg> {
        self.pending = None;
        self.queued = None;
        self.retry = None;
        self.again = false;
        let watch = self.state.initial_request();
        self.route(vec![watch], next_id)
    }

    /// The store's answer to `req_id`: a snapshot of the pull for the state
    /// that asked, and whatever that state asks for next, routed. An answer
    /// to a request since superseded — the reader moved on while it was out
    /// — is set aside for the newer one, which goes out now; the stage
    /// keeps what it shows until that lands. The answer to a read another
    /// pull left out is its turn ending: this pull's newest want goes.
    pub fn absorb(
        &mut self,
        req_id: u32,
        fight: Option<StoredFight>,
        next_id: &mut u32,
    ) -> Vec<ClientMsg> {
        let Some((_, want)) = self.pending.take_if(|(id, _)| *id == req_id) else {
            return Vec::new();
        };
        let Some(want) = want else {
            return match self.queued.take() {
                Some(next) => self.ask(next, next_id),
                None => Vec::new(),
            };
        };
        if let Some(next) = self.queued.take()
            && next != want
        {
            self.again = false;
            return self.ask(next, next_id);
        }
        let Some(fight) = fight else {
            // Empty: a refused read, or a fight the store no longer holds.
            // Asked once more, after a pause (the quota that refused it is
            // still full this instant), before it is believed; the stage
            // keeps what it shows meanwhile.
            if !std::mem::replace(&mut self.again, true) {
                self.retry = Some((Instant::now() + RETRY_AFTER, want));
                return Vec::new();
            }
            self.again = false;
            self.missing = true;
            return Vec::new();
        };
        self.again = false;
        self.missing = false;
        let Want { view, drill, .. } = want;
        self.bare = drill.is_some() && fight.breakdown.is_none();
        self.loadout = drill.zip(fight.loadout);
        let info = info_of(&fight.card);
        self.card = Some(fight.card);
        let sent = self.state.on_msg(DaemonMsg::Snapshot {
            seq: 0,
            segment: SegmentRef::Live,
            id: Some(segment_id(&self.fight_id)),
            view,
            info,
            total_rows: fight.rows.len() as u32,
            rows: fight.rows,
            breakdown: fight.breakdown,
            segment_count: 1,
            source: Some(source_of(&self.fight_id)),
            status: None,
            // v35 (R25): the store's rebuild of the pull's raid timeline —
            // the ribbon, the Deaths table and the stat line's deaths.
            raid: fight.raid,
        });
        self.route(sent, next_id)
    }

    /// The drill the stage shows was answered without its breakdown.
    pub fn bare(&self) -> bool {
        self.bare && self.pending.is_none()
    }

    /// `guid`'s logged build, when the last answer carried it.
    pub fn loadout_of(&self, guid: &str) -> Option<&Loadout> {
        self.loadout
            .as_ref()
            .filter(|(who, _)| who == guid)
            .map(|(_, l)| l)
    }

    /// The store pinned (or let go of) `fight_id`: its card says so, when
    /// it is this pull's.
    pub fn pinned(&mut self, fight_id: &str, pinned: bool) {
        if let Some(c) = self.card.as_mut().filter(|c| c.id == fight_id) {
            c.pinned = pinned;
        }
    }
}

/// A stored pull's segment id: the fight id's hash, with the top bit set
/// so it never meets a daemon's (which count up from zero) — what keeps
/// what the window remembers per fight (`fight_head::Seen`) apart.
fn segment_id(fight_id: &str) -> SegmentId {
    SegmentId(wowdps_proto::history::fnv64(fight_id.as_bytes()) | 1 << 63)
}

/// The source a stored pull's state is told it follows: one per pull, so
/// nothing of another's is ever taken for its own.
fn source_of(fight_id: &str) -> String {
    format!("history store: {fight_id}")
}

/// A card as the meter's header reads a segment: a key's Σ is the visit's
/// (an Overall with its timers, on the key clock), an arena match wears
/// Win and Loss, and nothing stored is live.
pub fn info_of(card: &FightCard) -> SegmentInfo {
    let kind = match card.kind {
        FightKind::Encounter | FightKind::Arena => SegmentKind::Encounter,
        FightKind::Key | FightKind::Overall => SegmentKind::Overall,
        FightKind::Trash => SegmentKind::Trash,
    };
    let duration_ms = match card.kind {
        FightKind::Key => card.official_ms.unwrap_or(card.duration_ms),
        _ => card.duration_ms,
    };
    SegmentInfo {
        kind,
        name: card.name.clone(),
        start_ms: card.start_local_ms,
        duration_ms,
        success: card.success,
        live: false,
        instance: None,
        pars_ms: card.pars_ms,
        arena: card.kind == FightKind::Arena,
        encounter: card.encounter,
    }
}

/// What a pull opened by id alone is until the store answers: nothing yet.
fn pending_info() -> SegmentInfo {
    SegmentInfo {
        kind: SegmentKind::Encounter,
        name: String::new(),
        start_ms: 0,
        duration_ms: 0,
        success: None,
        live: false,
        instance: None,
        pars_ms: None,
        arena: false,
        encounter: None,
    }
}

fn list_row(info: &SegmentInfo) -> ListRow {
    ListRow {
        kind: info.kind,
        name: info.name.clone(),
        start_ms: info.start_ms,
        success: info.success,
        duration_ms: info.duration_ms,
        live: info.live,
        instance: info.instance,
        pars_ms: info.pars_ms,
        arena: info.arena,
        encounter: info.encounter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_daemon::mock::MockDaemon;

    fn cards() -> Vec<FightCard> {
        let mock = MockDaemon::fixture().with_history();
        let mut cards = mock.history().cards().to_vec();
        cards.sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms));
        cards
    }

    fn now() -> Instant {
        Instant::now()
    }

    /// Past any pause a refusal set.
    fn later() -> Instant {
        Instant::now() + RETRY_AFTER + Duration::from_millis(1)
    }

    /// One page in flight, the newest first; older pages start after the
    /// oldest card in hand; a refresh merges rather than starts over, so
    /// the pages the reader asked for stay.
    #[test]
    fn the_pages_come_one_at_a_time_and_merge() {
        let cards = cards();
        assert!(cards.len() >= 2, "the fixture stores more than one fight");
        let mut e = Earlier::default();
        assert!(e.next_request(1, now()).is_none(), "nothing asked for");
        e.want_newest();
        let first = e.next_request(1, now()).expect("the newest page");
        assert!(matches!(
            first,
            ClientMsg::GetHistory {
                query: HistoryQuery::Fights {
                    after_id: None,
                    limit: PAGE,
                    sort: FightSort::Newest,
                    guid: None,
                    ..
                },
                ..
            }
        ));
        e.want_older();
        assert!(e.next_request(2, now()).is_none(), "one in flight");
        assert!(e.asking());
        // A stale answer is not this pager's.
        assert!(!e.absorb(
            9,
            &HistoryAnswer::Fights {
                cards: cards.clone(),
                total: 99,
            }
        ));
        assert!(e.cards.is_empty());
        let (head, tail) = cards.split_at(1);
        assert!(e.absorb(
            1,
            &HistoryAnswer::Fights {
                cards: head.to_vec(),
                total: cards.len() as u32,
            }
        ));
        assert!(e.answered && e.more());
        let older = e.next_request(3, now()).expect("the older page, now");
        assert!(matches!(
            older,
            ClientMsg::GetHistory {
                query: HistoryQuery::Fights { after_id: Some(ref id), .. },
                ..
            } if *id == head[0].id
        ));
        assert!(e.absorb(
            3,
            &HistoryAnswer::Fights {
                cards: tail.to_vec(),
                total: cards.len() as u32,
            }
        ));
        assert_eq!(e.cards.len(), cards.len());
        assert!(!e.more(), "the store is in hand");
        e.want_older();
        assert!(
            e.next_request(4, now()).is_none(),
            "nothing older to ask for"
        );
        // The store wrote a fight: its newest few cards, merged — not a
        // whole page again for every closed pull.
        e.want_fresh();
        let fresh = e.next_request(5, now()).expect("the fresh cards");
        assert!(
            matches!(
                fresh,
                ClientMsg::GetHistory {
                    query: HistoryQuery::Fights {
                        after_id: None,
                        limit: FRESH,
                        ..
                    },
                    ..
                }
            ),
            "{fresh:?}"
        );
        assert!(e.absorb(
            5,
            &HistoryAnswer::Fights {
                cards: head.to_vec(),
                total: cards.len() as u32,
            }
        ));
        assert_eq!(e.cards.len(), cards.len(), "merged, not doubled");
        // A newer copy of a card in hand replaces it: a pin, a regrade.
        let mut pinned = head[0].clone();
        pinned.pinned = !pinned.pinned;
        e.want_newest();
        assert!(e.next_request(6, now()).is_some());
        assert!(e.absorb(
            6,
            &HistoryAnswer::Fights {
                cards: vec![pinned.clone()],
                total: cards.len() as u32,
            }
        ));
        assert_eq!(e.card(&pinned.id).map(|c| c.pinned), Some(pinned.pinned));
    }

    /// A refused page — no cards and a total of 0, while the rail holds
    /// cards — is asked again after a pause, and the total in hand stands,
    /// so "Show older nights" does not vanish over a store that still holds
    /// them; a second such answer leaves the rail as it stood. A newest
    /// page asked for while an older one waits sends both, the newest
    /// first. A lost connection's page is asked again.
    #[test]
    fn a_refused_page_is_asked_again_and_no_want_is_lost() {
        let cards = cards();
        let (head, tail) = cards.split_at(1);
        let total = cards.len() as u32;
        let mut e = Earlier::default();
        e.want_newest();
        let _ = e.next_request(1, now()).unwrap();
        assert!(e.absorb(
            1,
            &HistoryAnswer::Fights {
                cards: head.to_vec(),
                total,
            }
        ));
        // The reader asks for older nights; the store writes a fight before
        // that page is sent: both go, newest first.
        e.want_older();
        e.want_newest();
        let first = e.next_request(2, now()).unwrap();
        assert!(matches!(
            first,
            ClientMsg::GetHistory {
                query: HistoryQuery::Fights { after_id: None, .. },
                ..
            }
        ));
        assert!(e.absorb(
            2,
            &HistoryAnswer::Fights {
                cards: head.to_vec(),
                total,
            }
        ));
        let second = e
            .next_request(3, now())
            .expect("the older page, still wanted");
        assert!(matches!(
            second,
            ClientMsg::GetHistory {
                query: HistoryQuery::Fights {
                    after_id: Some(_),
                    ..
                },
                ..
            }
        ));
        // Refused: asked again, and the store's count stands.
        let refused = HistoryAnswer::Fights {
            cards: Vec::new(),
            total: 0,
        };
        assert!(e.absorb(3, &refused));
        assert!(e.more(), "the total in hand stands");
        assert!(
            e.next_request(4, now()).is_none(),
            "not in the instant the quota refused it"
        );
        assert!(e.next_request(4, later()).is_some(), "asked again");
        assert!(e.absorb(
            4,
            &HistoryAnswer::Fights {
                cards: tail.to_vec(),
                total,
            }
        ));
        assert_eq!(e.cards.len(), cards.len());
        // Refused twice over: still the quota's word, never the store's —
        // the cards and the count in hand stand, and nothing more is asked
        // until something wants it.
        let held = e.cards.len();
        e.want_newest();
        let _ = e.next_request(5, now()).unwrap();
        assert!(e.absorb(5, &refused));
        let _ = e.next_request(6, later()).unwrap();
        assert!(e.absorb(6, &refused));
        assert_eq!(e.cards.len(), held);
        assert_eq!(e.total, Some(total), "the held total");
        assert!(e.next_request(7, later()).is_none());
        // A page out on a connection that died is asked again.
        e.want_newest();
        let _ = e.next_request(8, now()).unwrap();
        assert!(e.asking());
        e.lost();
        assert!(!e.asking());
        assert!(e.next_request(9, now()).is_some());
        // The store's word on a pin lands on the card in hand.
        let id = cards[0].id.clone();
        let pinned = !cards[0].pinned;
        assert!(e.absorb(
            77,
            &HistoryAnswer::Pinned {
                fight_id: id.clone(),
                pinned,
            }
        ));
        assert_eq!(e.card(&id).map(|c| c.pinned), Some(pinned));
    }

    /// A stored pull opens on the stage's view, asks the store for it, and
    /// takes the answer as its own snapshot — then follows the selection
    /// as a live pull does, each move its own `GetFight`.
    #[test]
    fn a_stored_pull_asks_the_store_what_its_state_watches() {
        let mut mock = MockDaemon::fixture().with_history();
        let card = mock
            .history()
            .cards()
            .iter()
            .find(|c| c.kind == FightKind::Encounter && c.success == Some(true))
            .cloned()
            .expect("a stored kill");
        let mut next = 10;
        let (mut s, sent) = Stored::open(
            card.id.clone(),
            Some(card.clone()),
            View::EnemyTaken,
            None,
            None,
            &mut next,
        );
        assert_eq!(s.state.view, View::Damage, "no enemies in the store");
        let [
            ClientMsg::GetFight {
                req_id: 10,
                fight_id,
                view: View::Damage,
                drill: None,
                ..
            },
        ] = sent.as_slice()
        else {
            panic!("{sent:?}");
        };
        assert_eq!(*fight_id, card.id);
        assert!(s.state.segment_name().is_none(), "nothing in yet");
        let answer = |mock: &mut MockDaemon, msgs: Vec<ClientMsg>| {
            let mut out = Vec::new();
            for m in msgs {
                for reply in mock.handle(m) {
                    if let DaemonMsg::Fight { req_id, fight } = reply {
                        out.push((req_id, fight));
                    }
                }
            }
            out
        };
        // The answer is a snapshot; following, the state drills the top row.
        let mut asked = sent;
        for _ in 0..4 {
            let replies = answer(&mut mock, std::mem::take(&mut asked));
            for (req_id, fight) in replies {
                asked.extend(s.absorb(req_id, fight, &mut next));
            }
            if asked.is_empty() {
                break;
            }
        }
        assert_eq!(s.state.segment_name().as_deref(), Some(card.name.as_str()));
        // The card's outcome, what the fight header's badge reads (the iced
        // GUI's fight_head test words it "Kill").
        assert_eq!(s.state.segment_kind(), Some(SegmentKind::Encounter));
        assert_eq!(s.state.segment_success(), Some(true));
        assert!(!s.state.is_live());
        assert!(!s.state.rows().is_empty());
        let drill = s.state.drill.clone().expect("the selection's drill");
        assert_eq!(drill.key, s.state.rows()[0].key);
        assert!(s.state.drill_breakdown().is_some(), "the details tier");
        assert!(!s.bare());
        // A move of the selection is a GetFight for the next player.
        let sent = s.state.apply(wowdps_model::Action::Down);
        let routed = s.route(sent, &mut next);
        assert!(
            matches!(routed.as_slice(), [ClientMsg::GetFight { drill: Some(d), .. }] if *d == s.state.rows()[1].key),
            "{routed:?}"
        );
        let req_of = |msgs: &[ClientMsg]| {
            msgs.iter()
                .find_map(|m| match m {
                    ClientMsg::GetFight { req_id, .. } => Some(*req_id),
                    _ => None,
                })
                .expect("a GetFight")
        };
        // An answer to no request of this pull's is dropped. An empty one
        // is asked again before it is believed — the daemon answers a read
        // it refused the same way — after a pause, not in the instant the
        // quota refused it; only a second says the fight is gone.
        assert!(s.absorb(3, None, &mut next).is_empty());
        assert!(!s.missing);
        assert!(
            s.absorb(req_of(&routed), None, &mut next).is_empty(),
            "not asked again in the same instant"
        );
        assert!(s.tick(Instant::now(), &mut next).is_empty(), "paused");
        let again = s.tick(later(), &mut next);
        assert!(
            matches!(again.as_slice(), [ClientMsg::GetFight { drill: Some(d), .. }] if *d == s.state.rows()[1].key),
            "asked again: {again:?}"
        );
        assert!(!s.missing, "one empty answer is not the store's word");
        assert!(!s.state.rows().is_empty(), "the stage keeps what it shows");
        assert!(s.absorb(req_of(&again), None, &mut next).is_empty());
        assert!(s.missing, "evicted since the rail listed it");
        // A comparison has no stored answer.
        assert!(
            s.route(
                vec![ClientMsg::Watch(Cursor::Compare {
                    segment: SegmentRef::Live,
                    a: "a".into(),
                    b: "b".into(),
                    view: View::Damage,
                    range: None,
                    spell: None,
                })],
                &mut next
            )
            .is_empty()
        );
    }

    /// A held `j` asks for the player it lands on, not for every one it
    /// passes: one `GetFight` is in flight, the newest move waits behind
    /// it, and the answer to a move since passed is set aside for it.
    #[test]
    fn a_held_key_keeps_one_fight_in_flight() {
        let mut mock = MockDaemon::fixture().with_history();
        let card = mock
            .history()
            .cards()
            .iter()
            .find(|c| c.kind == FightKind::Encounter && c.success == Some(true))
            .cloned()
            .expect("a stored kill");
        let mut next = 10;
        let (mut s, mut asked) = Stored::open(
            card.id.clone(),
            Some(card),
            View::Damage,
            None,
            None,
            &mut next,
        );
        // Settle: the rows, and the top row's drill.
        for _ in 0..4 {
            let mut out = Vec::new();
            for m in std::mem::take(&mut asked) {
                for reply in mock.handle(m) {
                    if let DaemonMsg::Fight { req_id, fight } = reply {
                        out.extend(s.absorb(req_id, fight, &mut next));
                    }
                }
            }
            asked = out;
        }
        assert!(asked.is_empty());
        let rows = s.state.rows();
        assert!(rows.len() >= 3, "the kill has players to walk");
        // Three presses: one request goes out, the rest wait.
        let sent = s.state.apply(wowdps_model::Action::Down);
        let first = s.route(sent, &mut next);
        assert_eq!(first.len(), 1);
        for _ in 0..2 {
            let sent = s.state.apply(wowdps_model::Action::Down);
            assert!(s.route(sent, &mut next).is_empty(), "one in flight");
        }
        let landed = s.state.drill.as_ref().map(|d| d.key.clone());
        assert_eq!(
            landed.as_deref(),
            Some(rows[3.min(rows.len() - 1)].key.as_str())
        );
        // The first answers: it is for a player since passed, so it is set
        // aside and the newest move goes out in its place.
        let (req_id, fight) = mock
            .handle(first[0].clone())
            .into_iter()
            .find_map(|m| match m {
                DaemonMsg::Fight { req_id, fight } => Some((req_id, fight)),
                _ => None,
            })
            .unwrap();
        let then = s.absorb(req_id, fight, &mut next);
        assert!(
            matches!(then.as_slice(), [ClientMsg::GetFight { drill, .. }] if *drill == landed),
            "{then:?}"
        );
        assert_eq!(
            s.state.drill.as_ref().map(|d| d.key.clone()),
            landed,
            "the selection stays where the keys left it"
        );
    }

    /// A key's card is the visit's Σ on the key clock with its timers; an
    /// arena match reads as one; two pulls never share a segment id, nor
    /// one with a daemon's.
    #[test]
    fn a_card_reads_as_the_segment_it_was() {
        let key = FightCard {
            kind: FightKind::Key,
            name: "Kings' Rest +14".to_string(),
            duration_ms: 2_000_000,
            official_ms: Some(1_898_895),
            pars_ms: Some((2_040_000, 1_632_000, 1_224_000)),
            success: Some(true),
            ..FightCard::default()
        };
        let info = info_of(&key);
        assert_eq!(info.kind, SegmentKind::Overall);
        assert_eq!(info.duration_ms, 1_898_895, "the key clock");
        assert_eq!(info.pars_ms, key.pars_ms);
        assert!(!info.live);
        let arena = FightCard {
            kind: FightKind::Arena,
            success: Some(false),
            ..FightCard::default()
        };
        assert!(info_of(&arena).arena);
        assert_ne!(segment_id("a-1"), segment_id("a-2"));
        assert!(segment_id("a-1").0 >= 1 << 63);
    }
}
