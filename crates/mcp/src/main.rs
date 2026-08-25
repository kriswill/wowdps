//! Binary entry: connect (spawning the daemon on demand), then speak MCP on
//! stdin/stdout until the harness hangs up. Diagnostics go to stderr — stdout
//! belongs to the protocol.

use std::io::{BufReader, Write as _};

use wowdps_mcp::{bridge::Bridge, rpc};

const USAGE: &str = "\
wowdps-mcp - MCP server exposing wowdps fight data to LLM harnesses

Usage:
  wowdps-mcp    speak MCP (stdio transport) — normally run as `wowdps mcp`

Registers as an MCP server, e.g. for Claude Code:
  claude mcp add wowdps -- wowdps mcp

Tools: status, list_fights, fight, breakdown, compare. The daemon is spawned
on demand and owns the combat log; this process only reshapes its snapshots.";

fn main() {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return;
        }
        Some(other) => {
            eprintln!("wowdps-mcp: unknown argument {other:?}\n\n{USAGE}");
            std::process::exit(2);
        }
    }

    let mut bridge = match Bridge::connect() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("wowdps-mcp: cannot reach or spawn the daemon: {e}");
            std::process::exit(1);
        }
    };

    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    if let Err(e) = rpc::serve(BufReader::new(stdin), stdout, &mut bridge) {
        eprintln!("wowdps-mcp: {e}");
        let _ = std::io::stderr().flush();
        std::process::exit(1);
    }
}
