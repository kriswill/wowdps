---
type: Crate
title: wowdps-mcp
description: 'wowdps-mcp binary: MCP server exposing fight data to LLM harnesses.'
resource: crates/mcp
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T16:04:26-07:00 }
---

`wowdps-mcp`: an MCP (Model Context Protocol) server over stdio, exposing the daemon's fight data as tools an LLM harness can call — a third frontend beside the TUI and GUI, and exactly as thin: model + proto only, snapshots in, JSON out. `wowdps mcp` reaches it through the dispatcher's external-command lookup.

## Source

- Manifest: [`crates/mcp/Cargo.toml`](../../../crates/mcp/Cargo.toml)
- Root: [`crates/mcp/src/lib.rs`](../../../crates/mcp/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
