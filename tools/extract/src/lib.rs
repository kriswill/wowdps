//! Dev-time game-data extraction (no runtime crate depends on this).
//!
//! Reads a client database (`.db2`, WDC5 format) plus its community schema
//! (`.dbd` from WoWDBDefs) and emits the table as CSV — the same shape
//! wago.tools serves, but computed locally from game files. Stdlib only,
//! like the rest of the workspace's non-GUI crates.
//!
//! The lower half is the local-install reader: `casc` (Data/data `.idx`
//! journals + archives), `blte` (chunk container, with `inflate` and
//! `salsa20` underneath), and `tact` (.build.info → build config →
//! encoding → root), which together turn a FileDataID or game path into
//! file bytes without touching the network.

pub mod bits;
pub mod blte;
pub mod casc;
pub mod dbd;
pub mod hash;
pub mod inflate;
pub mod salsa20;
pub mod table;
pub mod tact;
pub mod wdc5;
