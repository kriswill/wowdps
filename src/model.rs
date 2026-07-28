//! The single place the TUI binds to the parser/meter implementation.
//!
//! SWAP POINT (milestone 2): once `core` lands `parser.rs`/`meter.rs` on main,
//! replace the `pub use` below with
//!
//! ```ignore
//! pub use crate::meter::*;
//! pub use crate::parser::*;
//! ```
//!
//! then delete `src/stub.rs` and its `mod stub;` in `main.rs`. Nothing in
//! `app.rs`, `ui.rs` or `main.rs` refers to `stub` directly.
//!
//! What the TUI relies on beyond the contract's field lists:
//! - `View: Copy + Clone + PartialEq + Debug`
//! - `SegmentKind: PartialEq + Debug`
//! - `Row`/`Segment` fields public exactly as named in CONTRACT.md
//! - `parse_line` returning `Option<LogLine>` with a monotonic `ts_ms`
pub use crate::stub::*;
