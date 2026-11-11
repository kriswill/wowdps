//! The window's edition (the editions plan's "The window"). One idea: **the
//! window shows one game at a time, and by default the one you are
//! playing.** The switch lists the games; picking the game the window
//! would follow anyway follows (`EditionRef::Follow`), picking the other
//! pins it, and a pin whose game becomes the live one follows again once
//! the live game moves on — so the window keeps showing what you play. The
//! reader never sees Follow or a pin, only whether the game shown is the
//! one being played (the live dot on the switch).
//!
//! The live meter rides `ClientState::set_edition`; the rail's stored
//! pages, Home and the census's switch ride the history readers
//! (`history::Store::rescope`); the character Home opens on, its class and
//! the dates Home counts from are the edition's own config keys
//! (`config::KeysOf`). With one edition known nothing here changes what
//! the window asks or draws.
//!
//! The words and the census are gui-logic's `editions`; this module only
//! routes the choice.

use std::time::Instant;

use gpui_kit::{App, Context};
use wowdps_gui_logic::config::KeysOf;
use wowdps_gui_logic::editions::{self, Entry, Face};
use wowdps_gui_logic::home::Season;
use wowdps_gui_logic::rail::Pull;
use wowdps_model::Edition;
use wowdps_proto::EditionRef;

use super::{Gui, Place};

impl Gui {
    /// The live edition: following, the one the daemon last answered the
    /// log's state in, else the one `Status` names live; pinned, the one
    /// `Status` names live.
    pub(crate) fn live_edition(&self, cx: &App) -> Option<Edition> {
        let named = match self.edition {
            EditionRef::Follow => self.session.read(cx).state().edition(),
            EditionRef::Pin(_) => None,
        };
        named
            .or(self.hist.store.census.live())
            .map(Edition::counts_as)
    }

    /// The edition the window is in now: the pinned one, or the live one —
    /// and while nothing names that, the one it was in.
    pub(crate) fn edition_now(&self, cx: &App) -> Edition {
        editions::window_edition(self.edition, self.live_edition(cx), self.hist.edition)
    }

    /// Whose config keys the window reads: its edition's, falling back to
    /// the top-level ones while it is the only edition known.
    pub(crate) fn keys(&self) -> KeysOf {
        KeysOf::new(self.hist.edition, !self.hist.store.census.several())
    }

    /// The edition a fight is named against in its header: the window's,
    /// while it knows several; `None` while it knows one.
    pub(crate) fn header_edition(&self) -> Option<Edition> {
        self.hist
            .store
            .census
            .several()
            .then_some(self.hist.edition)
    }

    /// The switch's entries, `None` while one edition or none is known.
    pub(crate) fn edition_entries(&self) -> Option<Vec<Entry>> {
        editions::entries(&self.hist.store.census, self.hist.edition)
    }

    /// The switch's face: the game shown.
    pub(crate) fn edition_face(&self) -> Face {
        editions::face(&self.hist.store.census, self.hist.edition)
    }

    /// The daemon has answered the meter since the window last told it
    /// where to look: until then the state's empty list is the window's
    /// own reset, not the daemon's word.
    fn answered(&self, cx: &App) -> bool {
        let Some(asked) = self.hist.asked_at else {
            return true;
        };
        self.session
            .read(cx)
            .last_snapshot()
            .is_some_and(|at| at > asked)
    }

    /// What the stage says when the game shown has no fight to show — the
    /// daemon answered with no pull of its log (a followed log once it is
    /// listed: while it is still read, the stage says so as ever) and the
    /// store holds none — once the window knows several games: its
    /// heading and how to get a fight. `None` otherwise.
    pub(crate) fn empty_game(&self, cx: &App) -> Option<(String, String)> {
        let census = &self.hist.store.census;
        if !census.several() || self.hist.store.stored.is_some() || !self.answered(cx) {
            return None;
        }
        let state = self.session.read(cx).state();
        let shown = self.hist.edition;
        let seen = census.seen(shown);
        let listed = !seen.source || state.source.is_some();
        let here = state.edition().map(Edition::counts_as) == Some(shown);
        (here && listed && state.segment_count() == 0 && seen.stored == 0)
            .then(|| editions::empty_stage(&census.name(shown)))
    }

    /// Bring every reader, and what the window keeps per edition, to the
    /// edition it is in now: the rail's stored pages and Home ask for that
    /// edition's fights once several are known, Home counts from its dates
    /// and opens on its character, and the chrome wears its character's
    /// class. A pin whose game became the live one follows again once the
    /// live game moves on. Called on every word from the session and the
    /// readers; it does nothing while nothing moved.
    pub(crate) fn sync_edition(&mut self, cx: &mut Context<Self>) {
        self.catch_up(cx);
        let now = self.edition_now(cx);
        let several = self.hist.store.census.several();
        let scoped = editions::history_edition(several, now);
        let moved = now != self.hist.edition;
        self.hist.edition = now;
        let season = Season::for_edition(&self.cfg, self.keys());
        if self.hist.store.rescope(scoped, season, moved) || moved {
            let scope = self.cfg.character_in(self.keys()).map(str::to_string);
            if let Some(home) = self.hist.store.home.as_mut() {
                home.scope = scope;
            }
            let sent = self.hist.store.requests(Instant::now());
            self.send_history(sent, cx);
            self.derive_home(cx);
            cx.notify();
        }
        if moved {
            // Another game's characters: the class chrome is learned anew,
            // from that game's remembered class meanwhile.
            self.repaint(cx);
        }
        // A menu whose entries went away closes rather than hold the keys
        // invisibly.
        if self.cards.edition && self.edition_entries().is_none() {
            self.cards.edition = false;
            cx.notify();
        }
        self.land_pinned(cx);
    }

    /// A pin whose game is the live one: noted, and once the live game
    /// moves on, the window follows again — it shows what you play from
    /// then on. Until then a pin and following show the same thing, so
    /// nothing is reset while the reader looks.
    fn catch_up(&mut self, cx: &mut Context<Self>) {
        let EditionRef::Pin(pinned) = self.edition else {
            self.hist.caught_up = false;
            return;
        };
        match self.hist.store.census.live() {
            Some(live) if live == pinned.counts_as() => self.hist.caught_up = true,
            Some(_) if self.hist.caught_up => {
                self.hist.caught_up = false;
                self.set_choice(EditionRef::Follow, cx);
            }
            _ => {}
        }
    }

    /// A press on one of the switch's games: the window shows it. The game
    /// the window would follow anyway is followed, any other pinned; the
    /// game already shown changes nothing. The live meter starts over on
    /// the game's own list and lands on its newest pull; a stored pull on
    /// the stage gives way to it, as `m` does.
    pub(crate) fn pick_edition(&mut self, pick: Edition, cx: &mut Context<Self>) {
        self.cards.edition = false;
        let pick = pick.counts_as();
        if pick != self.edition_now(cx) {
            // The live game by the daemon's freshest word: its frames while
            // following, which can run a second ahead of `Status`.
            let follows_to = self
                .live_edition(cx)
                .or_else(|| (self.edition == EditionRef::Follow).then_some(self.hist.edition));
            let choice = editions::choice_for(pick, follows_to);
            self.set_choice(choice, cx);
            self.sync_edition(cx);
        }
        cx.notify();
    }

    /// Tell the meter which edition to look in, off a stored pull.
    fn set_choice(&mut self, choice: EditionRef, cx: &mut Context<Self>) {
        if choice == self.edition {
            return;
        }
        self.edition = choice;
        self.hist.caught_up = false;
        self.hist.asked_at = Some(std::time::Instant::now());
        // A pinned game with no log of its own lands on its newest stored
        // pull once the rail has it.
        self.hist.landing = matches!(choice, EditionRef::Pin(_));
        let view = self.leave_stored();
        self.session.update(cx, |s, cx| {
            s.act(
                |st| {
                    st.uninspect();
                    if let Some(v) = view {
                        st.view = v;
                    }
                    st.set_edition(choice)
                },
                cx,
            )
        });
    }

    /// A pinned game no log of which is followed puts its newest stored
    /// pull on the stage, once the daemon has answered the pin with its
    /// empty meter and the rail has the pull: the window shows the game
    /// picked, never a blank. A game whose log is followed shows that log.
    pub(crate) fn land_pinned(&mut self, cx: &mut Context<Self>) {
        let EditionRef::Pin(pinned) = self.edition else {
            return;
        };
        if !self.hist.landing || self.place != Place::Fights || self.hist.store.stored.is_some() {
            return;
        }
        if self.hist.store.census.seen(pinned).source {
            self.hist.landing = false;
            return;
        }
        if !self.answered(cx) {
            return;
        }
        let state = self.session.read(cx).state();
        if state.edition().map(Edition::counts_as) != Some(pinned.counts_as()) {
            return;
        }
        if state.segment_count() > 0 {
            self.hist.landing = false;
            return;
        }
        let rail = self.rail(cx);
        let stored = |trash: bool| {
            rail.lines()
                .find(|l| matches!(l.pull, Pull::Stored(_)) && l.trash == trash)
                .map(|l| l.pull.clone())
        };
        if let Some(pull) = stored(false).or_else(|| stored(true)) {
            self.hist.landing = false;
            self.go_pull(pull, cx);
        }
    }
}

#[cfg(test)]
mod tests;
