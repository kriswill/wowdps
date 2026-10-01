//! The talent viewer's logic (R14), moved out of the iced viewer so both
//! GUIs draw one: a player's tree from the per-machine dataset
//! (`talents.json`, read through `wowdps_proto::talents`, the mcp talent
//! tools' own codec), laid out into the game's three panes, and the
//! viewer's state machine over it.
//!
//! Input is a paste: either a bare in-game import string, or a whole
//! SimulationCraft addon export (`simc`), which also brings every saved
//! loadout, the equipped gear, the bag items and the currency lines. A
//! parsed simc paste is persisted per character under
//! `$XDG_DATA_HOME/wowdps/simc/`, so opening the viewer on a meter row
//! whose player has pasted before shows their build immediately.
//!
//! v19: opening on a meter row also asks the daemon for the player's
//! COMBATANT_INFO loadout (`GetLoadout`); when it lands, the logged build —
//! talents and equipped gear, the ones actually used in the watched fight —
//! wins over any stored paste ([`Viewer::adopt_logged`]), with simc loadout
//! chips one click away. Logged builds are never persisted; the daemon
//! re-answers on every open.
//!
//! The panes mirror the game's — class, spec, hero — split the way the
//! in-game frame does it: hero nodes carry `subTreeId`, and the class/spec
//! halves divide at the midpoint of the remaining nodes' grid x.

#[cfg(any(test, feature = "test-support"))]
pub mod fixture;
mod geometry;
mod model;
mod viewer;

pub use geometry::*;
pub use model::*;
pub use viewer::*;

use wowdps_proto::json::Json;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static DATASET: std::cell::Cell<Option<&'static Json>> = const { std::cell::Cell::new(None) };
}

/// Test hook: answer every dataset read on this thread from `dataset`
/// instead of the per-machine `talents.json` (`None` restores it). A GUI's
/// tests pair it with `simc::use_dir_on_this_thread`, so no test reads or
/// writes `~/.local/share/wowdps`.
#[cfg(any(test, feature = "test-support"))]
pub fn use_dataset_on_this_thread(dataset: Option<&'static Json>) {
    DATASET.with(|d| d.set(dataset));
}

/// Whether this thread's dataset is a test's (the hook above is set): a
/// GUI's art readers stay away from the per-machine caches then, so every
/// render exercises its "no cache" path.
pub fn under_test() -> bool {
    #[cfg(any(test, feature = "test-support"))]
    let hooked = DATASET.with(std::cell::Cell::get).is_some();
    #[cfg(not(any(test, feature = "test-support")))]
    let hooked = false;
    hooked
}

/// The talent dataset (R14), through the test hook.
pub fn load_dataset() -> Result<&'static Json, String> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(ds) = DATASET.with(std::cell::Cell::get) {
        return Ok(ds);
    }
    wowdps_proto::talents::load()
}

#[cfg(test)]
mod tests;
