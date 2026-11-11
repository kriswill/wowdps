//! Home (plan step 3.5; the iced window's `home.rs`, the prototype's
//! `.home`): "You, this week", the reader's week over the daemon's history
//! store. Window-local like the talent viewer — no `Screen` variant, so
//! `ClientState` never learns it exists.
//!
//! Everything on it is derived by gui-logic's `home` from the
//! `HistoryQuery::Fights` answers `history::Store` pages in, one request in
//! flight at a time, until the week is in hand. Scope is a chip row — all
//! characters, or one — never a lock: the config's `character` is only the
//! scope Home opens on.

mod charts;
mod panels;

#[cfg(test)]
mod tests;

pub use panels::view;

use gpui_kit::{App, Context};
use wowdps_gui_logic::config::Config;
use wowdps_gui_logic::home::{characters_by_edition, derive};
use wowdps_gui_logic::rail::log_instances;
use wowdps_model::Edition;
use wowdps_proto::history::FightCard;

use super::{Gui, Place};

impl Gui {
    /// Open Home on the scope it opened on last, asking for its first page
    /// and for the store's state.
    pub(crate) fn open_home(&mut self, cx: &mut Context<Self>) {
        self.place = Place::Home;
        if self.hist.store.home.is_none() {
            let scope = self.cfg.character_in(self.keys()).map(str::to_string);
            let sent = self.hist.store.open_home(scope);
            self.send_history(sent, cx);
        }
        cx.notify();
    }

    /// Home stands aside for the stage: its paging stops, its panels stay
    /// for the picker and the next time.
    pub(crate) fn leave_home(&mut self) {
        self.place = Place::Fights;
        self.hist.store.close_home();
    }

    /// Scope Home to `guid` (`None`: every character of yours) — a chip, a
    /// pick of the picker's menu — opening it so scoped when it is not up.
    /// The scope is Home's, and the one it opens on next time in the
    /// edition the window is in: the config's `character` (Retail's, or the
    /// edition's own) says no more than that. Whose window it is, its
    /// chrome and the "you" on a meter follow the character played.
    pub(crate) fn scope_home(&mut self, guid: Option<String>, cx: &mut Context<Self>) {
        // The one key, over what is on disk: the launch-time copy would put
        // back an overlay drag or a zoom saved since.
        let keys = self.keys();
        if self.cfg.character_in(keys) != guid.as_deref() {
            self.cfg.set_character_in(keys, guid.clone());
            let stored = guid.clone();
            Config::store(move |c| c.set_character_in(keys, stored));
        }
        self.open_home(cx);
        if let Some(home) = self.hist.store.home.as_mut() {
            home.scope = guid;
        }
        self.derive_home(cx);
        cx.notify();
    }

    /// Derive the panels from whatever Home holds now, and remember the
    /// characters they name.
    pub(crate) fn derive_home(&mut self, cx: &App) {
        let Some(home) = self.hist.store.home.as_ref() else {
            return;
        };
        // The tailed log's visits name a raid night still going, whose Σ
        // card the store has yet to write.
        let state = self.session.read(cx).state();
        let log = log_instances(state.entries(), state.log_id());
        // `history_characters` names Retail's characters: another edition's
        // chips are its own store's alone, once the window knows several.
        let configured =
            if self.hist.edition == Edition::Retail || !self.hist.store.census.several() {
                self.cfg.history_characters()
            } else {
                Vec::new()
            };
        let panels = derive(
            &home.cards,
            home.scope.as_deref(),
            &self.hist.store.season,
            &configured,
            &log,
        );
        let cards: Vec<&FightCard> = home.cards.iter().collect();
        let by = characters_by_edition(&cards);
        self.hist.remember(by);
        self.hist.store.panels = panels;
    }
}
