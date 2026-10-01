//! What the wowdps GUIs share and neither draws: the logic the iced GUI
//! (`crates/gui`) and its GPUI successor (`crates/gui-new`) both run while
//! they coexist, and the successor keeps after the cutover.
//!
//! Code arrives here MOVED from `crates/gui`, never copied: each module left
//! that crate in a commit of its own, which re-imports it, so one config
//! writer, one keymap and one takeover socket serve both GUIs. Nothing here
//! names a UI framework; a piece that draws stays in its GUI
//! (`docs/spec-gui-new.md` §5).

pub mod config;
pub mod fold;
pub mod history;
pub mod hypr;
pub mod keys;
pub mod simc;
pub mod single;
pub mod table;
pub mod theme;
pub mod tree;
