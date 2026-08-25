//! `wowdps-mcp`: an MCP (Model Context Protocol) server over stdio, exposing
//! the daemon's fight data as tools an LLM harness can call — a third
//! frontend beside the TUI and GUI, and exactly as thin: model + proto only,
//! snapshots in, JSON out. `wowdps mcp` reaches it through the dispatcher's
//! external-command lookup.

pub mod bridge;
pub mod json;
pub mod rpc;
pub mod tools;
