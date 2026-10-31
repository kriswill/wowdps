//! The extractor's commands (`main.rs` lists them), on the library's
//! command line (`wowdps_extract::command`: the argument parser `args` and
//! the install every command opens, `install`), by what they make —
//! committed tables (`tablegen`), per-machine caches (`caches`), the
//! client's tables and files as they are (`db2`) and the history store's
//! creatures file seeded from Wowhead (`creatures`).

pub use wowdps_extract::command::{Cmd, args, at, cmd, data_path, install, make_dir, read_text};

pub mod caches;
pub mod creatures;
pub mod db2;
pub mod tablegen;
