//! `wowdps-mcp`: an MCP (Model Context Protocol) server over stdio, exposing
//! the daemon's fight data as tools an LLM harness can call — a third
//! frontend beside the TUI and GUI, and exactly as thin: model + proto only,
//! snapshots in, JSON out. `wowdps mcp` reaches it through the dispatcher's
//! external-command lookup.

pub mod bridge;
pub mod grade;
pub mod rpc;
pub mod tools;

// The grading core, at the crate root for the lake's parity gate.
pub use grade::{DPS_FLOOR, DPS_TOP_FLOOR, Grade, Measure};

// The JSON value and the talent codec live in wowdps-proto (shared with the
// GUI's talent viewer); re-exported so this crate's modules and tests keep
// their `crate::json` / `crate::talents` paths.
pub use wowdps_proto::{json, obj, talents};
