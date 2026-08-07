//! Dev-time game-data extraction (no runtime crate depends on this).
//!
//! Reads a client database (`.db2`, WDC5 format) plus its community schema
//! (`.dbd` from WoWDBDefs) and emits the table as CSV — the same shape
//! wago.tools serves, but computed locally from game files. Stdlib only,
//! like the rest of the workspace's non-GUI crates. First slice of the
//! local-install extractor; BLTE/CASC reading (to pull `.db2` files out of
//! the game's own storage) builds on top of this.

pub mod bits;
pub mod dbd;
pub mod table;
pub mod wdc5;
