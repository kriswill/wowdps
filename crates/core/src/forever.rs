//! Forever's generated tables (`Edition::Forever`, the install's `wowf`
//! product), beside Retail's at the crate root: the same generators run
//! against that edition's client (`tools/gen-*.sh --product wowf`), never
//! edited by hand. `crate::tables` picks them for a Forever fight.

pub(crate) mod class_spells;
pub(crate) mod item_spells;
pub(crate) mod keystone_timers;
pub(crate) mod open_world_maps;
