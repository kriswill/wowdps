//! Home (the prototype's `.home`): the reader's week, over the daemon's
//! history store.
//!
//! Home is window-local, exactly like the talent viewer — never a `Screen`
//! variant, so `ClientState` (and with it the TUI and the overlay) never
//! learns it exists. Everything on it is derived here, client-side, from
//! the `HistoryQuery::Fights` answers the daemon already serves: each
//! card's "me" is the scoped character's row on it, or — scoped to all of
//! them — the row of the card's owner.
//!
//! It opens on the question a reader asks after a raid, "how did last night
//! go for me": the night's pulls with their rank in the role, and how that
//! rank moved across the night. Under it, the week: keys against their
//! timers, raid progress with one dot per pull, and key throughput across
//! characters. Scope is a chip row — all characters, or one — never a lock:
//! the config's `character` is only the scope Home opens on.
//!
//! Two rules run through the whole module:
//!
//! - **A number we cannot derive is not drawn.** No card carries a raid's
//!   boss roster or a Mythic+ rating, so there is no "N / 8" and no score;
//!   a wipe nobody saw the boss's health on says "No kill", never "0%".
//! - **Paging is transport.** The reader never sees a page: Home asks for
//!   the store's newest cards, one request in flight at a time, until the
//!   week is in hand.

// The model is gui-logic's (`home`); this module draws it.
pub(crate) use wowdps_gui_logic::home::*;

use iced::Element;

use crate::nav;
use crate::theme;

mod charts;
mod panels;

/// A character's colour as iced draws it: their class's, or the third ink
/// for a character whose class nobody saw (gui-logic's `Char::ink`).
pub(crate) trait CharInk {
    fn color(&self) -> iced::Color;
}

impl CharInk for Char {
    fn color(&self) -> iced::Color {
        let c = self.ink(wowdps_gui_logic::theme::GOLD.window.ink_3);
        iced::Color::from_rgba(c.r, c.g, c.b, c.a)
    }
}

/// A character line as the picker wears it.
pub(crate) fn char_pick(c: &CharLine) -> nav::CharPick {
    nav::CharPick {
        guid: c.guid.clone(),
        name: c.name.clone(),
        class: c.class,
        spec: c.spec,
        fights: c.fights,
        last_local_ms: (c.last_local_ms != 0).then_some(c.last_local_ms),
    }
}

// ---- the screen -------------------------------------------------------------

/// The facts the screen needs besides the panels, cheap to clone into the
/// `responsive` closure that lays it out (`Home` is not `Clone`, and the
/// closure is called again on every resize).
#[derive(Debug, Clone)]
pub(crate) struct Meta {
    /// The store is on and has answered: there is a week to show. Off, or
    /// not heard from yet, the screen says which instead of drawing panels
    /// that say "none" of what nobody read.
    pub settled: bool,
    pub stalled: bool,
    /// Whose week: a guid, or `None` for every character.
    pub scope: Option<String>,
    /// The line that tells a disabled store from a cold one from a
    /// degraded one ([`state_line`]).
    pub state_line: Option<String>,
    pub hide_realms: bool,
    /// The night it is now, which words how long ago the lead night was.
    pub tonight: i64,
    /// A pressed chip's edge.
    pub accent: theme::Accent,
}

impl Meta {
    pub(crate) fn of(home: &Home, hide_realms: bool, tonight: i64, accent: theme::Accent) -> Self {
        Self {
            settled: home.answered && home.disabled_reason.is_none(),
            stalled: home.stalled(),
            scope: home.scope.clone(),
            state_line: state_line(home),
            hide_realms,
            tonight,
            accent,
        }
    }
}

/// The whole screen: the title and its scope chips, the night, and the
/// week's panels in as many columns as the width holds. `chars` are the
/// characters the window knows you play — from this week's cards and every
/// page of the store it has read — which the chips offer, by name.
pub(crate) fn screen(
    home: &Home,
    panels: &Panels,
    chars: &[CharLine],
    accent: theme::Accent,
    hide_realms: bool,
    tonight: i64,
) -> Element<'static, crate::window::Message> {
    let meta = Meta::of(home, hide_realms, tonight, accent);
    let panels = panels.clone();
    let mut chars: Vec<CharLine> = chars
        .iter()
        .filter(|c| !c.guid.is_empty())
        .cloned()
        .collect();
    // The chips stand still: by name, not by how much each was played.
    chars.sort_by_key(|c| crate::fold::fold(&c.name));
    // The layout is a function of the width, which only the layout knows;
    // `responsive` is how a widget tree gets to ask.
    iced::widget::responsive(move |size| panels::laid_out(&meta, &panels, &chars, size.width))
        .into()
}

#[cfg(test)]
mod tests;
