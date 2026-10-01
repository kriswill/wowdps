//! The window's reads of the history store (plan steps 3.4 and 3.5): the
//! pages the rail lists under tonight's (`Earlier`), a stored pull on the
//! stage (`Stored`), and Home's week (`Home`), all over the one daemon
//! connection — each asked through `Session::request` and answered from
//! its `Reply` events — and the store's state from `Status`, which tells a
//! disabled store from a cold one from a degraded one.
//!
//! The rules are gui-logic's (`history`, `home`): one request in flight
//! per reader, a refused read asked again after a pause, a stored pull's
//! own `ClientState` fed from `GetFight` answers. This module only routes:
//! what each reader wants goes out, what comes back goes to its reader.

use std::time::{Duration, Instant};

use gpui_kit::{App, Context, Entity, ScrollHandle, Subscription, Task};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::history::{Earlier, Stored};
use wowdps_gui_logic::home::{Char, CharLine, Home, Panels, Season};
use wowdps_gui_logic::rail::Pull;
use wowdps_model::{Drill, SegmentKind, View};
use wowdps_proto::history::{FightCard, fight_id};
use wowdps_proto::{ClientMsg, ClientState, DaemonMsg, HistoryAnswer, HistoryStatus};

use super::{Gui, Place};
use crate::session::{Reply, Session};

/// Least time between two `GetStatus` asks off the store-changed path: a
/// wipe-heavy night closes fights faster than anyone reads a banner.
pub const STATUS_REFRESH: Duration = Duration::from_secs(5);

/// What an answer changed, for the window to redraw and re-derive.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    /// The rail's pages moved.
    pub rail: bool,
    /// Home's cards moved: derive its panels again.
    pub home: bool,
    /// The stored pull on the stage was answered.
    pub stage: bool,
}

/// The window's history readers.
pub struct Store {
    /// The rail's earlier nights.
    pub earlier: Earlier,
    /// The stored pull on the stage, while one is.
    pub stored: Option<Stored>,
    /// A `GetFight` a pull the reader left still has out: the next stored
    /// pull waits on it, so one read is out for the whole window.
    stray: Option<u32>,
    /// Home's paging while Home is up, and its panels as last derived.
    pub home: Option<Home>,
    pub panels: Panels,
    pub season: Season,
    /// The store is off, and why; requests the daemon dropped.
    pub disabled: Option<String>,
    pub dropped: u32,
    /// When `GetStatus` was last asked off the store-changed path.
    status_at: Option<Instant>,
    next_id: u32,
}

impl Store {
    /// The readers at launch: the rail asks for its newest page at once.
    pub fn new(cfg: &Config) -> Self {
        let mut earlier = Earlier::default();
        earlier.want_newest();
        Self {
            earlier,
            stored: None,
            stray: None,
            home: None,
            panels: Panels::default(),
            season: Season::from_config(cfg),
            disabled: None,
            dropped: 0,
            status_at: None,
            // 0 is the session's own `GetStatus`.
            next_id: 1,
        }
    }

    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Whatever every reader wants now: the rail's next page, a stored
    /// pull's second asking once its pause is over, Home's next page.
    pub fn requests(&mut self, now: Instant) -> Vec<ClientMsg> {
        let mut out = Vec::new();
        let id = self.next_id;
        if let Some(msg) = self.earlier.next_request(id, now) {
            self.id();
            out.push(msg);
        }
        if let Some(s) = self.stored.as_mut() {
            out.extend(s.tick(now, &mut self.next_id));
        }
        let id = self.next_id;
        if let Some(msg) = self
            .home
            .as_mut()
            .and_then(|h| h.next_request(id, &self.season))
        {
            self.id();
            out.push(msg);
        }
        out
    }

    /// A reply of the daemon's, to whichever reader asked; what it changed.
    pub fn on_reply(&mut self, msg: &DaemonMsg) -> (Changed, Vec<ClientMsg>) {
        let mut changed = Changed::default();
        let mut out = Vec::new();
        match msg {
            DaemonMsg::History { req_id, answer } => {
                changed.rail |= self.earlier.absorb(*req_id, answer);
                if let Some(home) = self.home.as_mut() {
                    let before = home.cards.len();
                    let answered = home.answered;
                    home.absorb(*req_id, answer);
                    changed.home |= home.cards.len() != before || home.answered != answered;
                }
                if let HistoryAnswer::Pinned { fight_id, pinned } = answer {
                    changed.rail |= self.earlier.pinned(fight_id, *pinned);
                    if let Some(s) = self.stored.as_mut() {
                        s.pinned(fight_id, *pinned);
                        changed.stage = true;
                    }
                }
            }
            DaemonMsg::Fight { req_id, fight } => {
                if self.stray == Some(*req_id) {
                    self.stray = None;
                }
                if let Some(s) = self.stored.as_mut() {
                    out.extend(s.absorb(*req_id, fight.clone(), &mut self.next_id));
                    changed.stage = true;
                }
            }
            DaemonMsg::HistoryChanged { .. } => {
                // The rail's newest few cards, merged over what it holds;
                // Home's week read again from the top.
                self.earlier.want_fresh();
                if let Some(home) = self.home.as_mut() {
                    home.reset();
                    changed.home = true;
                }
                if self
                    .status_at
                    .is_none_or(|at| at.elapsed() >= STATUS_REFRESH)
                {
                    self.status_at = Some(Instant::now());
                    out.push(self.ask_status());
                }
            }
            _ => {}
        }
        out.extend(self.requests(Instant::now()));
        (changed, out)
    }

    /// The store's state, from `Status`.
    pub fn on_status(&mut self, history: &HistoryStatus) -> bool {
        let disabled = (!history.enabled).then(|| {
            history
                .error
                .clone()
                .unwrap_or_else(|| "no reason given".to_string())
        });
        let changed = disabled != self.disabled || history.dropped != self.dropped;
        self.disabled = disabled;
        self.dropped = history.dropped;
        if let Some(home) = self.home.as_mut() {
            home.disabled_reason = self.disabled.clone();
            home.dropped = self.dropped;
        }
        changed
    }

    /// A `GetStatus` one-shot: `Status` carries the whole answer.
    pub fn ask_status(&mut self) -> ClientMsg {
        ClientMsg::GetStatus { req_id: self.id() }
    }

    // ---- a stored pull ---------------------------------------------------------

    /// Put stored fight `id` on the stage on `view` with `drill` carried
    /// over, asking for it — after the read a pull the reader left still
    /// has out.
    pub fn open_stored(&mut self, id: String, view: View, drill: Option<Drill>) -> Vec<ClientMsg> {
        let card = self.earlier.card(&id).cloned();
        self.leave_stored();
        let inherit = self.stray.take();
        let (stored, sent) = Stored::open(id, card, view, drill, inherit, &mut self.next_id);
        self.stored = Some(stored);
        sent
    }

    /// Take the stored pull off the stage — `true` when there was one —
    /// keeping the read it still has out for the next stored pull to wait
    /// on.
    pub fn leave_stored(&mut self) -> bool {
        let Some(s) = self.stored.take() else {
            return false;
        };
        if let Some(id) = s.in_flight() {
            self.stray = Some(id);
        }
        true
    }

    /// Act on the stored pull's own state, as a gesture on the stage would:
    /// what it asks for becomes the `GetFight` that answers it.
    pub fn act_stored(
        &mut self,
        f: impl FnOnce(&mut ClientState) -> Vec<ClientMsg>,
    ) -> Vec<ClientMsg> {
        let Some(s) = self.stored.as_mut() else {
            return Vec::new();
        };
        let sent = f(&mut s.state);
        s.route(sent, &mut self.next_id)
    }

    /// The stored card of `fight_id`, as the rail's pages hold it.
    pub fn card(&self, fight_id: &str) -> Option<&FightCard> {
        self.earlier.card(fight_id)
    }

    // ---- Home ------------------------------------------------------------------------

    /// Open Home scoped to `scope` (a guid, or every character) and ask
    /// for its first page, and for the store's state: the daemon never
    /// broadcasts it, and a value read at launch is stale by the first
    /// pull.
    pub fn open_home(&mut self, scope: Option<String>) -> Vec<ClientMsg> {
        let mut home = Home::new();
        home.scope = scope;
        home.disabled_reason = self.disabled.clone();
        home.dropped = self.dropped;
        self.home = Some(home);
        self.panels = Panels::default();
        let mut out = vec![self.ask_status()];
        out.extend(self.requests(Instant::now()));
        out
    }

    /// Home stood aside: its paging stops; its panels stay for next time.
    pub fn close_home(&mut self) {
        self.home = None;
    }
}

impl Store {
    /// A req_id for a one-shot the window sends itself.
    pub fn next_req(&mut self) -> u32 {
        self.id()
    }
}

// ---- the window's side --------------------------------------------------------

/// How often the readers are asked whether a paused second asking is due.
const TICK: Duration = Duration::from_millis(250);

/// Everything the rail and Home keep in the window: the store's readers,
/// the rail's toggle, highlight and scroll, Home's scroll and the
/// characters it has met, and the view the log was on while a stored pull
/// stood in for it. One field of `Gui`, holding its own subscriptions to
/// the session and the tick that sends a paused second asking.
pub struct Hist {
    pub store: Store,
    pub rail: RailUi,
    pub home_scroll: ScrollHandle,
    /// Every character the store has named as yours, most played first:
    /// what Home's chips offer.
    pub known: Vec<CharLine>,
    /// The log's view while a stored pull stood in for it: the store keeps
    /// no answer for some views, and the step back restores it.
    pub log_view: Option<View>,
    /// "Tonight", pinned by a test or a shot; the clock's otherwise.
    pub tonight_pin: Option<i64>,
    /// The store's state as `Status` last said it.
    status_seen: Option<HistoryStatus>,
    /// Home was offered at launch (once, after the first snapshot), and
    /// whether the log's pull was live at the last word from the session:
    /// a pull STARTING replaces Home with the meter.
    considered: bool,
    was_live: bool,
    _subscriptions: Vec<Subscription>,
    _tick: Task<()>,
}

/// The rail's own state: the trash toggle, the drawer's highlight, a row
/// to bring into sight, `H`'s wish for the earlier nights' heading, and its
/// list's scroll.
#[derive(Default)]
pub struct RailUi {
    pub hide_trash: bool,
    pub cursor: Option<Pull>,
    pub reveal: Option<Pull>,
    pub earlier: bool,
    pub scroll: ScrollHandle,
}

impl Hist {
    /// The readers at launch: the rail's newest page asked for at once, the
    /// session's replies and changes heard, and the tick started.
    pub fn new(cfg: &Config, session: &Entity<Session>, cx: &mut Context<Gui>) -> Self {
        let mut store = Store::new(cfg);
        let first = store.requests(Instant::now());
        session.update(cx, |s, _| {
            for msg in first {
                s.request(msg);
            }
        });
        let replies = cx.subscribe(session, |this, _, Reply(msg), cx| this.on_reply(msg, cx));
        let changes = cx.observe(session, |this, _, cx| this.history_observes(cx));
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |gui, cx| gui.history_tick(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            store,
            rail: RailUi::default(),
            home_scroll: ScrollHandle::new(),
            known: Vec::new(),
            log_view: None,
            tonight_pin: None,
            status_seen: None,
            considered: false,
            was_live: false,
            _subscriptions: vec![replies, changes],
            _tick: tick,
        }
    }

    /// Whose window it is by the store: the owner of its newest card —
    /// Home's answer when it has one, else the rail's pages' — whatever
    /// Home is scoped to.
    pub fn owner(&self) -> Option<Char> {
        if let Some(o) = &self.store.panels.owner {
            return Some(o.clone());
        }
        let card = self
            .store
            .earlier
            .cards
            .iter()
            .filter(|c| c.owner.is_some())
            .max_by_key(|c| c.start_utc_ms)?;
        let guid = card.owner.as_deref()?;
        let p = card.players.iter().find(|p| p.guid == guid)?;
        Some(Char {
            guid: p.guid.clone(),
            name: p.name.clone(),
            class: p.class,
            spec: p.spec,
        })
    }
}

/// [`Gui::fight`] from the window's two fields it reads, for a caller that
/// holds another field of the window mutably meanwhile.
pub(crate) fn fight_of<'a>(
    hist: &'a Hist,
    session: &'a Entity<Session>,
    cx: &'a App,
) -> &'a ClientState {
    match &hist.store.stored {
        Some(s) => &s.state,
        None => session.read(cx).state(),
    }
}

impl Gui {
    /// Send what the history readers asked for, on the window's connection.
    pub(crate) fn send_history(&mut self, msgs: Vec<ClientMsg>, cx: &mut Context<Self>) {
        if msgs.is_empty() {
            return;
        }
        self.session.update(cx, |s, _| {
            for msg in msgs {
                s.request(msg);
            }
        });
    }

    /// Home's paging runs only while Home is up: any way off it (a view
    /// tab, a death, the filter) stops it.
    fn sync_home(&mut self) {
        if self.place != Place::Home {
            self.hist.store.close_home();
        }
    }

    /// The tick: a second asking whose pause is over goes out.
    fn history_tick(&mut self, cx: &mut Context<Self>) {
        self.sync_home();
        let sent = self.hist.store.requests(Instant::now());
        if !sent.is_empty() {
            self.send_history(sent, cx);
            cx.notify();
        }
    }

    /// A reply of the daemon's to one of the readers.
    fn on_reply(&mut self, msg: &DaemonMsg, cx: &mut Context<Self>) {
        self.sync_home();
        let (changed, sent) = self.hist.store.on_reply(msg);
        self.send_history(sent, cx);
        // The store's word on a pin (`p`), told to the reader.
        if let DaemonMsg::History {
            answer: HistoryAnswer::Pinned { pinned, .. },
            ..
        } = msg
        {
            use wowdps_gui_logic::toast::{PINNED, UNPINNED};
            self.say(if *pinned { PINNED } else { UNPINNED }, cx);
        }
        // The rail's pages name the characters you played, as Home's
        // answers do: the picker's menu offers them without a visit to
        // Home.
        if changed.rail {
            let cards: Vec<&FightCard> = self.hist.store.earlier.cards.iter().collect();
            let seen = wowdps_gui_logic::home::character_lines(&cards, &[]);
            wowdps_gui_logic::home::remember(&mut self.hist.known, seen);
        }
        if changed.home {
            self.derive_home(cx);
        }
        // A stored pull's answer is the stage's snapshot: the window learns
        // from it as it does from the log's.
        if changed.stage {
            self.on_session(cx);
        }
        if changed.rail || changed.home || changed.stage {
            cx.notify();
        }
    }

    /// Whatever the session said, as the readers hear it: the store's
    /// state from `Status`, Home offered once after the first snapshot when
    /// nothing is live, and a pull starting replacing Home with the meter —
    /// one already live when Home was opened does not (the reader asked
    /// for Home).
    fn history_observes(&mut self, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let status = session.status().map(|s| s.history.clone());
        let snapshot = session.last_snapshot().is_some();
        let live = session.state().is_live();
        if let Some(h) = status
            && self.hist.status_seen.as_ref() != Some(&h)
        {
            self.hist.status_seen = Some(h.clone());
            if self.hist.store.on_status(&h) {
                cx.notify();
            }
        }
        if live && !self.hist.was_live && self.place == Place::Home {
            self.leave_home();
            cx.notify();
        }
        self.hist.was_live = live;
        if !self.hist.considered && snapshot {
            self.hist.considered = true;
            if self.cfg.home_on_start && !live {
                self.open_home(cx);
            }
        }
    }

    /// The fight the stage draws: the stored pull on it, else the tailed
    /// log's watched pull. Every stage renderer reads this, never the
    /// session's state directly, so a stored pull is drawn by the code that
    /// draws a live one.
    pub(crate) fn fight<'a>(&'a self, cx: &'a App) -> &'a ClientState {
        fight_of(&self.hist, &self.session, cx)
    }

    /// Act on the fight on the stage: a stored pull's own state, whose
    /// asks become the `GetFight` that answers them, else the log's.
    pub(crate) fn act_fight(
        &mut self,
        f: impl FnOnce(&mut ClientState) -> Vec<ClientMsg>,
        cx: &mut Context<Self>,
    ) {
        if self.hist.store.stored.is_some() {
            let sent = self.hist.store.act_stored(f);
            self.send_history(sent, cx);
            cx.notify();
        } else {
            self.session.update(cx, |s, cx| s.act(f, cx));
        }
    }

    /// Off the stored pull, back onto the log's own (`m`, the live pill):
    /// the view the log was on before the store stood in for it, when one
    /// was.
    pub(crate) fn leave_stored(&mut self) -> Option<View> {
        if self.hist.store.leave_stored() {
            self.hist.log_view.take()
        } else {
            None
        }
    }

    /// The stored card `p` pins: the stored pull's, or the log pull's once
    /// the rail holds its card — none before the store writes it.
    pub(crate) fn pin_card(&self, cx: &App) -> Option<FightCard> {
        let store = &self.hist.store;
        match &store.stored {
            Some(s) => s.card.clone().or_else(|| store.card(&s.fight_id).cloned()),
            None => {
                let state = self.session.read(cx).state();
                state.log_id().and_then(|log| {
                    let e = state.entries().get(state.segment_index())?;
                    let id = fight_id(log, e.row.start_ms, e.row.kind == SegmentKind::Overall);
                    store.card(&id).cloned()
                })
            }
        }
    }

    /// `p`: pin the pull on the stage, or let it go — its stored card's,
    /// a log pull's once the rail holds its card. Retention keeps a pinned
    /// card; the rail's star says so when the store answers.
    pub(crate) fn pin(&mut self, cx: &mut Context<Self>) {
        // A pull the store holds no card of says so rather than nothing.
        let Some(card) = self.pin_card(cx) else {
            self.say(wowdps_gui_logic::toast::NO_CARD, cx);
            return;
        };
        let req_id = self.hist.store.next_req();
        self.send_history(
            vec![ClientMsg::PinFight {
                req_id,
                fight_id: card.id,
                pinned: !card.pinned,
            }],
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_daemon::mock::MockDaemon;

    /// Every request the store sends, answered by the mock, until quiet.
    fn settle(store: &mut Store, mock: &mut MockDaemon, mut out: Vec<ClientMsg>) -> Changed {
        let mut all = Changed::default();
        for _ in 0..8 {
            let mut next = Vec::new();
            for msg in out.drain(..) {
                for reply in mock.handle(msg) {
                    let (c, more) = store.on_reply(&reply);
                    all.rail |= c.rail;
                    all.home |= c.home;
                    all.stage |= c.stage;
                    next.extend(more);
                }
            }
            if next.is_empty() {
                break;
            }
            out = next;
        }
        all
    }

    #[test]
    fn the_rail_reads_its_newest_page_at_launch() {
        let mut mock = MockDaemon::fixture().with_history();
        let mut store = Store::new(&Config::default());
        let first = store.requests(Instant::now());
        assert_eq!(first.len(), 1, "one page in flight");
        let changed = settle(&mut store, &mut mock, first);
        assert!(changed.rail);
        assert!(store.earlier.answered);
        assert!(
            !store.earlier.cards.is_empty(),
            "the fixture's stored fights"
        );
        assert!(
            store.requests(Instant::now()).is_empty(),
            "nothing more wanted"
        );
    }

    #[test]
    fn a_stored_pull_is_read_into_its_own_state() {
        let mut mock = MockDaemon::fixture().with_history();
        let mut store = Store::new(&Config::default());
        let first = store.requests(Instant::now());
        settle(&mut store, &mut mock, first);
        let id = store.earlier.cards[0].id.clone();
        let sent = store.open_stored(id.clone(), View::Damage, None);
        assert_eq!(sent.len(), 1, "one GetFight");
        let changed = settle(&mut store, &mut mock, sent);
        assert!(changed.stage);
        let s = store.stored.as_ref().expect("on the stage");
        assert_eq!(s.fight_id, id);
        assert!(
            !s.state.rows().is_empty(),
            "the store's rows, as a snapshot"
        );
        assert!(store.leave_stored());
        assert!(store.stored.is_none());
    }

    #[test]
    fn home_reads_its_week_one_page_at_a_time() {
        let mut mock = MockDaemon::fixture().with_history();
        let mut store = Store::new(&Config::default());
        let first = store.requests(Instant::now());
        settle(&mut store, &mut mock, first);
        let sent = store.open_home(None);
        assert!(
            sent.iter()
                .any(|m| matches!(m, ClientMsg::GetStatus { .. })),
            "the store's state is asked for"
        );
        let changed = settle(&mut store, &mut mock, sent);
        assert!(changed.home);
        let home = store.home.as_ref().expect("Home is up");
        assert!(home.answered && !home.cards.is_empty());
        store.close_home();
        assert!(store.home.is_none());
    }

    #[test]
    fn a_disabled_store_says_why() {
        let mut store = Store::new(&Config::default());
        let status = HistoryStatus {
            enabled: false,
            error: Some("no data dir".to_string()),
            dropped: 2,
            ..HistoryStatus::default()
        };
        assert!(store.on_status(&status));
        assert_eq!(store.disabled.as_deref(), Some("no data dir"));
        assert_eq!(store.dropped, 2);
        assert!(!store.on_status(&status), "nothing new the second time");
    }
}
