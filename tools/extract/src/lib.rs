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
//!
//! Beside the generators sit two dev-time tools: `db2sql` (the game's
//! tables as SQL: wowdps-db2) and `wowhead` (seed-creatures: NPC
//! classifications seeded into the history store's `creatures.tsv`). They
//! stay stdlib-only by calling CLIs: `curl` (as the `gen-*` scripts do),
//! and `duckdb` from the dev shell.
//!
//! It is also the library the replay's extractor builds on
//! (`wowdps-replay-extract`, in the private wowdps-replay repository): the
//! install, the client tables with their schemas, the pictures, the
//! journal, the store's fights, the per-machine caches and the command line
//! (`command`), through their `pub` items alone; the list is
//! `tools/extract/AGENTS.md`'s.

pub mod absorbgen;
pub mod artgen;
pub mod atomic;
pub mod bits;
pub mod blp;
pub mod blte;
pub mod casc;
pub mod classgen;
pub mod command;
pub mod db2sql;
pub mod dbd;
pub mod deflate;
pub mod game;
pub mod hash;
pub mod icongen;
pub mod img;
pub mod inflate;
pub mod itemgen;
pub mod journal;
pub mod keystonegen;
pub mod mapgen;
pub mod placegen;
pub mod png;
pub mod procgen;
pub mod pulls;
pub mod raw;
pub mod rolegen;
pub mod salsa20;
pub mod spellicongen;
pub mod spelltip;
pub mod spelltipgen;
pub mod table;
pub mod tact;
pub mod talentgen;
pub mod wdc5;
pub mod wowhead;
