//! What the wowdps GUI computes without drawing: config, Hyprland IPC, the
//! keymap, the history pages, the cache readers, and every model, word and
//! geometry its surfaces lay out. The GUI (`crates/gui`, on GPUI) draws
//! from it; the TUI's keybind-parity test reads its chord table.
//!
//! Code arrived here MOVED from the iced GUI, never copied, while that GUI
//! and its GPUI successor coexisted (`docs/spec-gui-new.md` §5): one config
//! writer, one keymap and one takeover socket served both, and the iced
//! pictures proved each move. Nothing here names a UI framework; a piece
//! that draws stays in the GUI.

pub mod axis;
pub mod config;
pub mod deaths;
pub mod drill;
pub mod fight_head;
pub mod fold;
pub mod fonts;
pub mod glyph;
pub mod graph;
pub mod history;
pub mod home;
pub mod hypr;
pub mod icons;
pub mod inspect;
pub mod keys;
pub mod labels;
pub mod lazy_tiles;
pub mod output;
pub mod palette;
#[cfg(any(test, feature = "test-support"))]
pub mod raid;
pub mod rail;
pub mod reveal;
pub mod ribbon;
pub mod sibling;
pub mod simc;
pub mod single;
pub mod spell_icons;
pub mod surface;
pub mod table;
pub mod talent_art;
pub mod talents;
pub mod theme;
pub mod timeline;
pub mod toast;
pub mod tree;
