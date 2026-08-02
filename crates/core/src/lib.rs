//! wowdps-core: everything that is not a screen.
//!
//! Parsing (`parser`), aggregation (`meter`), the startup index (`index`),
//! log following (`tail`) and the UI-agnostic application state machine
//! (`app`). Frontends — the ratatui TUI, the iced GUI — depend on this crate
//! and bind to the domain types through `model`.

pub mod app;
pub mod cli;
pub mod fmt;
pub mod index;
pub mod meter;
pub mod model;
pub mod parser;
pub mod tail;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
