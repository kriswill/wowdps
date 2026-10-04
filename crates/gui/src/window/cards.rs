//! The cards over everything (plan step 3.6; the command palette is its
//! own): the `?` sheet, the ⚙ options card, the character menu, and the
//! toast — the iced window's `nav::shortcut_sheet`, `view::options_panel`,
//! `nav::character_menu` and `view::toast_card`, drawn from the same
//! gui-logic words, and the Esc walk that backs out of whatever is up.
//!
//! The three menus are modal for the keys: while one is up the window's
//! root leaves the meter's key context for [`keys::MODAL`], so none of the
//! meter's bindings fire, and its key-down listener closes the menu on any
//! key — which does nothing else. The zoom chords (bound with no context)
//! still zoom, and Ctrl K still opens the palette, which replaces the menu,
//! as in the iced window. For the pointer the sheet and the character menu
//! hang over a scrim that takes the press that closes them; the ⚙ card has
//! none, and the pointer leaving it closes it.

mod menu;
mod options;
mod sheet;
mod toast;

#[cfg(test)]
mod tests;

pub use menu::view as menu;
pub use options::view as options;
pub use sheet::view as sheet;
pub use toast::view as toast;

use std::time::Duration;

use gpui_kit::base::{Keyframe, Keyframes, Timing, TransitionId, animate_keyframes};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, KeyDownEvent, Task, Window};
use wowdps_gui_logic::keys::{Inert, Surface, inert_keys};
use wowdps_gui_logic::labels::display_name;
use wowdps_gui_logic::theme::{Chrome, Def, class_accent};
use wowdps_gui_logic::toast::{TOAST_FOR, pinned_player, stored_refusal};
use wowdps_model::{Action, Screen, View};

use super::w::{Fit, W};
use super::{Gui, Place};
use crate::theme;

/// A passing word over the stage, and the task that takes it away.
pub struct Toast {
    pub words: String,
    /// Which word this is: a new one replaces the old and enters afresh.
    pub seq: u64,
    _clear: Task<()>,
}

/// What the cards keep in the window: the toast up, the pin it last
/// said a word for, and the sheet's scroll.
#[derive(Default)]
pub struct CardsUi {
    pub toast: Option<Toast>,
    seq: u64,
    /// The first pick of the comparison as the window last saw it: a new
    /// pin says so, and the pair forming takes the word back.
    pin_seen: Option<String>,
    pub sheet_scroll: crate::scrollbar::Scroll,
}

/// A card's edge. iced draws a container's border inside its padding;
/// GPUI's sits outside it, so a card here pads by the iced padding less
/// its edge, and its content stands where the iced window's does.
pub(super) const BORDER: f32 = 1.0;

/// The cards' entrance (a delight): a fade and a rise of a few pixels,
/// eased out. Under reduced motion a card stands where it rests from its
/// first frame — the iced window's pixels.
const ENTER: Duration = Duration::from_millis(160);
const RISE: f32 = 6.0;

/// `el` entering: its opacity and its rise at this frame's point of the
/// entrance keyed by `id`, which starts the first frame the id is drawn.
pub(super) fn enter<E: Styled>(
    id: impl Into<TransitionId>,
    el: E,
    w: &W,
    window: &mut Window,
    cx: &mut App,
) -> E {
    let t = Keyframes::try_new([Keyframe::new(0.0, 0.0_f32), Keyframe::new(1.0, 1.0)])
        .ok()
        .map(|frames| animate_keyframes(id, &frames, Timing::new(ENTER), window, cx).value)
        .unwrap_or(1.0);
    if t >= 1.0 {
        return el;
    }
    let eased = 1.0 - (1.0 - t).powi(3);
    el.opacity(eased).relative().top(w.z(RISE * (1.0 - eased)))
}

/// Which of the three menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Options,
    Sheet,
    Picker,
}

impl Gui {
    /// Is a menu up — the sheet, the ⚙ card or the character menu? Then
    /// the keys are its: any key closes it and does nothing else.
    pub(crate) fn modal(&self) -> bool {
        self.cards.options || self.cards.sheet || self.cards.picker
    }

    /// Close every menu.
    pub(crate) fn close_menus(&mut self, cx: &mut Context<Self>) {
        self.cards.options = false;
        self.cards.sheet = false;
        self.cards.picker = false;
        cx.notify();
    }

    /// Open `menu`, or close it when it is the one up. One menu at a time,
    /// and the keys come to the window's root, wherever they were (the
    /// row filter's field): the menu's keys are any key.
    pub(crate) fn toggle_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        let open = !match menu {
            Menu::Options => self.cards.options,
            Menu::Sheet => self.cards.sheet,
            Menu::Picker => self.cards.picker,
        };
        self.cards.options = open && menu == Menu::Options;
        self.cards.sheet = open && menu == Menu::Sheet;
        self.cards.picker = open && menu == Menu::Picker;
        if open {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// A key while a menu is up: it closes the menu, and nothing else.
    pub(crate) fn menu_key(&mut self, _: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.modal() {
            self.close_menus(cx);
            cx.stop_propagation();
        }
    }

    // ---- the ⚙ card: each choice its own key in the config file ----------

    pub(crate) fn set_show_ranks(&mut self, on: bool, cx: &mut Context<Self>) {
        self.cfg.show_ranks = on;
        wowdps_gui_logic::config::Config::store(|c| c.show_ranks = on);
        cx.notify();
    }

    pub(crate) fn set_hide_realms(&mut self, on: bool, cx: &mut Context<Self>) {
        self.cfg.hide_realms = on;
        wowdps_gui_logic::config::Config::store(|c| c.hide_realms = on);
        cx.notify();
    }

    /// The chrome: the theme's own accent, or the owner's class when the
    /// window knows it — the character played last, else the class the
    /// config remembers of them.
    pub(crate) fn set_chrome(&mut self, chrome: Chrome, cx: &mut Context<Self>) {
        let name = chrome.name().to_string();
        self.cfg.chrome = name.clone();
        wowdps_gui_logic::config::Config::store(|c| c.chrome = name);
        self.repaint(cx);
    }

    /// The theme, by name: written to the config alone (the overlay, which
    /// watches the file, follows), and the window repainted in it with the
    /// chrome it had.
    pub(crate) fn set_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.cfg.theme == name {
            return;
        }
        self.cfg.theme = name.to_string();
        let stored = name.to_string();
        wowdps_gui_logic::config::Config::store(|c| c.theme = stored);
        self.repaint(cx);
    }

    /// The theme the config names, from the registry the window started
    /// with (the config's own `[themes]` included).
    pub(crate) fn theme_def(&self, cx: &App) -> Def {
        theme::Themes::global(cx).named(&self.cfg.theme).clone()
    }

    /// Apply the configured theme and chrome. A class chrome wears the
    /// owner's class when the window knows it — learned once and held: a
    /// class chrome picked now wears it, and the session's next word keeps
    /// it.
    fn repaint(&mut self, cx: &mut Context<Self>) {
        let chrome = self.cfg.chrome();
        let class = self
            .played(cx)
            .and_then(|p| p.class)
            .or_else(|| self.cfg.character_class());
        let accent = match chrome {
            Chrome::Class => class.map(class_accent),
            Chrome::Theme => None,
        };
        self.learned = chrome == Chrome::Class && class.is_some();
        let def = self.theme_def(cx);
        theme::apply(&def, accent, cx);
        cx.notify();
    }

    // ---- the character menu ------------------------------------------------

    /// The follow item's press (it stands only while a character is
    /// picked): the pick let go, so the picker names the character played
    /// again and Home, when it is up, every character — staying where the
    /// window is, as All characters' chip does on Home. It says what
    /// following means; the window's "you" never left it.
    pub(crate) fn picker_follow(&mut self, cx: &mut Context<Self>) {
        self.cards.picker = false;
        if self.cfg.character.is_some() {
            self.cfg.character = None;
            wowdps_gui_logic::config::Config::store_character(None);
        }
        if let Some(home) = self.hist.store.home.as_mut() {
            home.scope = None;
        }
        self.derive_home(cx);
        self.say(wowdps_gui_logic::home::FOLLOW_NOTE, cx);
    }

    /// A character's press: Home scoped to them (`scope_home`, which opens
    /// it).
    pub(crate) fn pick_character(&mut self, guid: String, cx: &mut Context<Self>) {
        self.cards.picker = false;
        self.scope_home(Some(guid), cx);
    }

    // ---- toasts ---------------------------------------------------------------

    /// A passing word over the stage (`.toast`), gone after
    /// [`TOAST_FOR`] or when another replaces it.
    pub(crate) fn say(&mut self, words: &str, cx: &mut Context<Self>) {
        self.cards_ui.seq += 1;
        let seq = self.cards_ui.seq;
        let clear = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            let _ = this.update(cx, |gui, cx| {
                if gui.cards_ui.toast.as_ref().is_some_and(|t| t.seq == seq) {
                    gui.cards_ui.toast = None;
                    cx.notify();
                }
            });
        });
        self.cards_ui.toast = Some(Toast {
            words: words.to_string(),
            seq,
            _clear: clear,
        });
        cx.notify();
    }

    /// A pin made just now says so for a moment (the prototype's toast);
    /// the pair it asks for, or the pin's end, takes the word back. Run
    /// after every gesture and every word from the session.
    pub(crate) fn pin_watch(&mut self, cx: &mut Context<Self>) {
        let app = self.fight(cx);
        let pin = app.compare_picks().first().cloned();
        let screen = app.screen;
        let before = self.cards_ui.pin_seen.take();
        match &pin {
            Some((key, label))
                if screen == Screen::Meter && before.as_deref() != Some(key.as_str()) =>
            {
                let name = if self.cfg.hide_realms {
                    display_name(label).to_string()
                } else {
                    label.clone()
                };
                self.say(&pinned_player(&name), cx);
            }
            Some(_) if screen != Screen::Compare => {}
            Some(_) => self.drop_toast(cx),
            None if before.is_some() => self.drop_toast(cx),
            None => {}
        }
        self.cards_ui.pin_seen = pin.map(|(key, _)| key);
    }

    fn drop_toast(&mut self, cx: &mut Context<Self>) {
        if self.cards_ui.toast.take().is_some() {
            cx.notify();
        }
    }

    /// What a stored pull says to `action` instead of asking the store for
    /// what it does not keep — a comparison, the enemies' view, an
    /// ability's own curve. `true` when it was refused (and said so).
    pub(crate) fn refuse(&mut self, action: Action, cx: &mut Context<Self>) -> bool {
        let Some(words) = self.refusal(action, cx) else {
            return false;
        };
        // Home stands over the stage: the word would go unseen there.
        if self.place == Place::Fights {
            self.say(words, cx);
        }
        true
    }

    fn refusal(&self, action: Action, cx: &App) -> Option<&'static str> {
        self.hist.store.stored.as_ref()?;
        let app = self.fight(cx);
        // R26: Enter on a group of the ability tree folds it, stored or not.
        let folds = super::inspector::model::tree_lines(app, &self.insp)
            .and_then(|lines| {
                super::inspector::model::tree_keyed(app, &self.insp, &lines)
                    .and_then(|at| lines.get(at).map(|l| l.opens.is_none()))
            })
            .unwrap_or(false);
        stored_refusal(action, app.inspecting(), folds)
    }

    // ---- the sheet's surface -----------------------------------------------

    /// Is the rail a drawer over the stage now (at 1180 px and under)?
    pub(crate) fn drawer_open(&self) -> bool {
        self.cards.rail && Fit::of(self.width) != Fit::Wide
    }

    /// Which surface is showing: what the `?` sheet lists the keys of.
    pub(crate) fn surface(&self, cx: &App) -> Surface {
        Surface::of(
            self.talents.is_some(),
            self.drawer_open(),
            self.place == Place::Home,
            self.fight(cx),
        )
    }

    /// The keys the sheet dims: listed, as they work on the surface, but
    /// not on the pull on the stage.
    pub(crate) fn inert_keys(&self, cx: &App) -> Vec<&'static str> {
        let app = self.fight(cx);
        let beside = Fit::of(self.width) != Fit::Narrow;
        inert_keys(Inert {
            covered: self.place == Place::Home || self.talents.is_some(),
            stored: self.hist.store.stored.is_some(),
            inspecting: app.inspecting(),
            pinnable: self.stage_card(cx).is_some(),
            deaths_table_beside: beside
                && app.view == View::Deaths
                && app.raid().is_some()
                && !app.inspecting(),
            narrow: !beside,
        })
    }

    // ---- Esc ----------------------------------------------------------------

    /// The command palette's place at the head of the Esc walk: over
    /// everything, it goes first. Its field answers its own Esc
    /// (`PalClose`); this is the Esc that reaches the window with the card
    /// up and the field not holding the keys.
    fn palette_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.pal.is_none() {
            return false;
        }
        self.close_palette(window, cx);
        true
    }

    /// Esc, one level up, in the iced window's order. The talent viewer
    /// and the menus answer their own Esc before it reaches here (the
    /// viewer holds the keys; a menu takes any key); then the palette, the
    /// rail's drawer, and Home — where the chain ends — then on the stage
    /// the filter's text (wherever the field shows), the inspector's
    /// ability, its keys, the comparison, and Home. `true` when answered.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.palette_escape(window, cx) {
            return true;
        }
        if self.drawer_open() {
            self.close_drawer(cx);
            return true;
        }
        if self.place == Place::Home {
            // Home is the front door every Esc leads to: it stays.
            return true;
        }
        let app = self.fight(cx);
        match app.screen {
            // No pull on the stage yet (an empty log): Home.
            Screen::List => {
                self.open_home(cx);
                return true;
            }
            Screen::Meter | Screen::Compare => {}
        }
        let comparing = app.screen == Screen::Compare || !app.compare_picks().is_empty();
        let ability = app.drill_spell().is_some();
        let narrow = Fit::of(self.width) == Fit::Narrow;
        let pushed = app.inspecting() && narrow;
        let keys_shown = app.inspecting() && (app.screen == Screen::Meter || pushed);
        // The filter's text goes first wherever the field shows: only a
        // narrow window's pushed inspector covers it, and there its keys
        // come back first — Esc never clears text the reader cannot see.
        if !self.filter_text.is_empty() && !pushed {
            self.clear_filter(window, cx);
        } else if ability || keys_shown {
            self.act(|s| s.apply(Action::Back), cx);
        } else if comparing {
            // Keys held by a pair beside the meter lit nothing: they go with
            // it, not as an Esc of their own that seemed to do nothing.
            self.act(
                |s| {
                    s.uninspect();
                    s.clear_compare()
                },
                cx,
            );
        } else if self.insp.wide && !narrow {
            // A widened inspector narrows back beside the meter before the
            // chain leaves the pull.
            self.toggle_wide(cx);
        } else {
            self.open_home(cx);
        }
        true
    }
}
