//! The single place the TUI binds to the parser/meter implementation.
//!
//! `app.rs`, `ui.rs` and `main.rs` import their domain types from here and never
//! name `meter`/`parser` directly, so swapping the implementation is a one-line
//! change. It was `stub.rs` through milestone 1; core's real modules landed at
//! milestone 2.
//!
//! What the TUI relies on beyond the contract's field lists:
//! - `View: Copy + Clone + PartialEq + Debug`
//! - `SegmentKind: PartialEq + Debug`
//! - `Row`/`Segment` fields public exactly as named in CONTRACT.md
//! - `parse_line` returning `Option<LogLine>` with a monotonic `ts_ms`
pub use crate::meter::*;
pub use crate::parser::*;
