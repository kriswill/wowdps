//! wowdps-core: the engine. Parsing (`parser`), aggregation (`meter`), the
//! startup index (`index`) and log following (`tail`) — everything between
//! bytes on disk and domain rows. Only the daemon runs this; frontends are
//! pure clients binding to `wowdps-model` types over `wowdps-proto`.

pub(crate) mod class_spells;
pub mod cli;
pub(crate) mod keystone_timers;
pub use wowdps_model::fmt;
pub mod index;
pub mod meter;
pub mod model;
pub mod parser;
pub mod tail;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
