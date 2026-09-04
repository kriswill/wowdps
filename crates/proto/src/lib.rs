//! The wowdps wire protocol: hand-rolled, zero-dependency, binary,
//! length-prefixed frames over a unix socket, plus the client library that
//! speaks it. Depends on `wowdps-model` only — never on the engine.
//!
//! Also home to the shared client-side extras every frontend may need
//! without touching the daemon: the hand-rolled JSON value (`json`) and the
//! talent dataset + import-string codec (`talents`, ruling R14) — here
//! rather than in one frontend so mcp and gui read the same code.
//! `history` is the on-disk record codec of the history store (roadmap
//! item 1): the daemon writes these documents, the readers parse them.

pub mod client;
pub mod history;
pub mod json;
pub mod msg;
pub mod state;
pub mod talents;
pub mod wire;

pub use client::{DaemonClient, SourceArg, ensure_daemon, socket_path};
pub use state::ClientState;

pub use msg::{
    Breakdown, ClientKind, ClientMsg, CompareSide, Cursor, DaemonMsg, FightSort, HistoryAnswer,
    HistoryQuery, HistoryStatus, ListEntry, LoadError, Night, OverlayState, PROTO_VERSION,
    SegmentRef, StoredFight, TrendBucket, TrendPoint, is_loading_status, loading_status,
};
pub use wire::{DecodeError, MAX_FRAME};
