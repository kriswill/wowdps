//! The wowdps wire protocol: hand-rolled, zero-dependency, binary,
//! length-prefixed frames over a unix socket, plus the client library that
//! speaks it. Depends on `wowdps-model` only — never on the engine.

pub mod client;
pub mod msg;
pub mod state;
pub mod wire;

pub use client::{DaemonClient, SourceArg, ensure_daemon, socket_path};
pub use state::ClientState;

pub use msg::{
    Breakdown, ClientKind, ClientMsg, CompareSide, Cursor, DaemonMsg, ListEntry, LoadError,
    OverlayState, PROTO_VERSION, SegmentRef,
};
pub use wire::{DecodeError, MAX_FRAME};
