//! MCP over stdio: newline-delimited JSON-RPC 2.0. One request line in, one
//! response line out; notifications get nothing. Protocol faults answer with
//! JSON-RPC errors; tool-level failures answer with `isError` content, which
//! is what a harness shows the model.

use std::io::{BufRead, Write};

use crate::bridge::Bridge;
use crate::json::{self, Json};
use crate::obj;
use crate::tools;

/// The MCP revision this server implements. Echoed back if the client asks
/// for something else — stdio framing and the tools surface are identical
/// across the revisions that exist.
const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve<R: BufRead, W: Write>(
    input: R,
    mut output: W,
    bridge: &mut Bridge,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle_line(&line, bridge) {
            output.write_all(reply.to_line().as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// `None` means "say nothing" (a notification, or a parse failure with no id
/// to answer to isn't one — JSON-RPC answers those with id null).
fn handle_line(line: &str, bridge: &mut Bridge) -> Option<Json> {
    let req = match json::parse(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_reply(
                Json::Null,
                -32700,
                &format!("parse error: {e}"),
            ));
        }
    };
    let id = req.get("id").cloned();
    let Some(method) = req.get("method").and_then(Json::as_str) else {
        return Some(error_reply(
            id.unwrap_or(Json::Null),
            -32600,
            "no method in request",
        ));
    };
    let params = req.get("params").cloned().unwrap_or(Json::Obj(Vec::new()));

    // Notifications (no id) are fire-and-forget by spec.
    let id = id?;

    let result = match method {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(Json::Obj(Vec::new())),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => return Some(tools_call_reply(id, &params, bridge)),
        other => Err((-32601, format!("method {other:?} not found"))),
    };
    Some(match result {
        Ok(result) => ok_reply(id, result),
        Err((code, msg)) => error_reply(id, code, &msg),
    })
}

fn initialize_result(params: &Json) -> Json {
    // Echo a known client revision, else answer with ours.
    let version = params
        .get("protocolVersion")
        .and_then(Json::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    obj! {
        "protocolVersion": Json::str(version),
        "capabilities": obj! { "tools": Json::Obj(Vec::new()) },
        "serverInfo": obj! {
            "name": Json::str("wowdps"),
            "version": Json::str(env!("CARGO_PKG_VERSION")),
        },
        "instructions": Json::str(
            "Live and historical World of Warcraft combat metering. Start with \
             list_fights (or status for liveness), then fight/breakdown/compare \
             for per-player analysis. Amounts are raw totals; per_sec is the \
             rate over the fight's duration.",
        ),
    }
}

fn tools_list_result() -> Json {
    Json::Obj(vec![(
        "tools".to_string(),
        Json::Arr(
            tools::catalog()
                .into_iter()
                .map(|t| {
                    obj! {
                        "name": Json::str(t.name),
                        "description": Json::str(t.description),
                        "inputSchema": t.schema,
                    }
                })
                .collect(),
        ),
    )])
}

fn tools_call_reply(id: Json, params: &Json, bridge: &mut Bridge) -> Json {
    let Some(name) = params.get("name").and_then(Json::as_str) else {
        return error_reply(id, -32602, "tools/call needs a tool name");
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(Json::Obj(Vec::new()));
    let (text, is_error) = match tools::call(bridge, name, &args) {
        Ok(v) => (v.to_line(), false),
        Err(e) => (e, true),
    };
    let mut result = vec![(
        "content".to_string(),
        Json::Arr(vec![obj! {
            "type": Json::str("text"),
            "text": Json::Str(text),
        }]),
    )];
    if is_error {
        result.push(("isError".to_string(), Json::Bool(true)));
    }
    ok_reply(id, Json::Obj(result))
}

fn ok_reply(id: Json, result: Json) -> Json {
    obj! {
        "jsonrpc": Json::str("2.0"),
        "id": id,
        "result": result,
    }
}

fn error_reply(id: Json, code: i32, message: &str) -> Json {
    obj! {
        "jsonrpc": Json::str("2.0"),
        "id": id,
        "error": obj! {
            "code": Json::num(code as f64),
            "message": Json::str(message),
        },
    }
}
