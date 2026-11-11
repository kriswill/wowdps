//! The window's edition (the editions plan's "The window"): the switch's
//! choice — Follow, the default, or one edition pinned — and what follows
//! from it. The live meter rides `ClientState::set_edition`; the rail's
//! stored pages, Home and the census's switch ride the history readers
//! (`history::Store::rescope`); the character Home opens on, its class and
//! the dates Home counts from are the edition's own config keys. With one
//! edition known nothing here changes what the window asks or draws.
//!
//! The words and the census are gui-logic's `editions`; this module only
//! routes the choice.

use std::time::Instant;

use gpui_kit::{App, Context};
use wowdps_gui_logic::editions::{self, Entry, Face};
use wowdps_gui_logic::home::Season;
use wowdps_model::Edition;
use wowdps_proto::EditionRef;

use super::Gui;

impl Gui {
    /// The edition the window is in now: the pinned one, or the live one
    /// the daemon last answered the log's state in.
    pub(crate) fn edition_now(&self, cx: &App) -> Edition {
        editions::window_edition(self.edition, self.session.read(cx).state().edition())
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
    pub(crate) fn edition_entries(&self, cx: &App) -> Option<Vec<Entry>> {
        let live = self.session.read(cx).state().edition();
        editions::entries(&self.hist.store.census, self.edition, live)
    }

    /// The switch's face.
    pub(crate) fn edition_face(&self, cx: &App) -> Face {
        let live = self.session.read(cx).state().edition();
        editions::face(&self.hist.store.census, self.edition, live)
    }

    /// Bring every reader, and what the window keeps per edition, to the
    /// edition it is in now: the rail's stored pages and Home ask for that
    /// edition's fights once several are known, Home counts from its dates
    /// and opens on its character, and the chrome wears its character's
    /// class. Called on every word from the session and the readers; it
    /// does nothing while nothing moved.
    pub(crate) fn sync_edition(&mut self, cx: &mut Context<Self>) {
        let now = self.edition_now(cx);
        let several = self.hist.store.census.several();
        let scoped = editions::history_edition(several, now);
        let moved = now != self.hist.edition;
        self.hist.edition = now;
        let season = Season::for_edition(&self.cfg, now);
        let rescoped = self.hist.store.rescope(scoped, season);
        if rescoped || moved {
            if let Some(home) = self.hist.store.home.as_mut() {
                home.scope = self.cfg.character_in(now).map(str::to_string);
            }
            let sent = self.hist.store.requests(Instant::now());
            self.send_history(sent, cx);
            self.derive_home(cx);
            cx.notify();
        }
        if moved {
            // Another edition's characters: the class chrome is learned
            // anew, from that edition's remembered class meanwhile.
            self.repaint(cx);
        }
    }

    /// A press on one of the switch's entries: the window in that edition
    /// (or following the game again). The live meter starts over on the
    /// edition's own list and lands on its newest pull; a stored pull on
    /// the stage gives way to it, as `m` does.
    pub(crate) fn pick_edition(&mut self, choice: EditionRef, cx: &mut Context<Self>) {
        self.cards.edition = false;
        let choice = match choice.canonical() {
            EditionRef::Pin(e) => EditionRef::Pin(e.counts_as()),
            EditionRef::Follow => EditionRef::Follow,
        };
        if choice != self.edition {
            self.edition = choice;
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
            self.sync_edition(cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
