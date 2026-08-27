//! MCP over stdio: newline-delimited JSON-RPC 2.0. One request line in, one
//! response line out; notifications get nothing. Protocol faults answer with
//! JSON-RPC errors; tool-level failures answer with `isError` content, which
//! is what a harness shows the model.
//!
//! This server is dual-era. The spec split at 2026-07-28: "legacy" revisions
//! (2025-11-25 and earlier) open with an `initialize` handshake, while
//! "modern" revisions (2026-07-28 on) are stateless — every request carries
//! its protocol version in `params._meta`, and `server/discover` replaces the
//! handshake as the way to learn what a server speaks. Era is decided per
//! request, from the request alone: the modern `_meta` version key selects
//! modern semantics, anything else is served as legacy. That is legal for a
//! dual-era server (it MAY serve both eras concurrently), and safe here
//! because our surface — the tools — is identical under every revision we
//! list, so no negotiated state needs to survive between requests.

use std::io::{BufRead, Write};

use crate::bridge::Bridge;
use crate::json::{self, Json};
use crate::obj;
use crate::tools;

/// Handshake-era revisions this server implements. The stdio framing and the
/// tools surface are identical across all four (2025-11-25's additions —
/// auth, icons, elicitation forms, sampling tools — touch nothing we serve),
/// so a client offering any of them is answered in kind; an unknown offer
/// gets our latest legacy revision, which per spec is how a server declines
/// a revision it hasn't verified.
const LEGACY_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", LATEST_LEGACY];
const LATEST_LEGACY: &str = "2025-11-25";

/// Stateless-era revisions: requests wear `_meta` and there is no handshake.
const MODERN_VERSIONS: &[&str] = &["2026-07-28"];

/// `_meta` keys the modern era defines (spec-reserved prefix).
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// UnsupportedProtocolVersionError (spec-reserved JSON-RPC code).
const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;

const INSTRUCTIONS: &str = "Live and historical World of Warcraft combat metering. Start with \
     list_fights (or status for liveness), then fight/breakdown/compare \
     for per-player analysis. Amounts are raw totals; per_sec is the \
     rate over the fight's duration.";

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

    // `server/discover` is the cross-era compatibility probe — a dual-era
    // client sends it before knowing what we speak, so it must be answered
    // whatever (or no) version the request wears; the answer itself carries
    // our version list. Every other method dispatches on era first.
    if method == "server/discover" {
        return Some(ok_reply(id, discover_result()));
    }
    Some(match modern_version(&params) {
        Some(requested) => handle_modern(id, method, requested, &params, bridge),
        None => handle_legacy(id, method, &params, bridge),
    })
}

/// The modern per-request version marker, when the request wears one.
fn modern_version(params: &Json) -> Option<&str> {
    params.get("_meta")?.get(META_PROTOCOL_VERSION)?.as_str()
}

// ---- modern era (2026-07-28): stateless, per-request `_meta` ---------------

fn handle_modern(
    id: Json,
    method: &str,
    requested: &str,
    params: &Json,
    bridge: &mut Bridge,
) -> Json {
    if !MODERN_VERSIONS.contains(&requested) {
        return unsupported_version_reply(id, requested);
    }
    match method {
        "tools/list" => ok_reply(id, modernize(cacheable(tools_list_result()))),
        "tools/call" => modernize_reply(tools_call_reply(id, params, bridge)),
        // `ping` (and the rest of the handshake-era utility surface) was
        // removed in 2026-07-28; a modern request to it is an unknown method.
        other => error_reply(id, -32601, &format!("method {other:?} not found")),
    }
}

fn unsupported_version_reply(id: Json, requested: &str) -> Json {
    obj! {
        "jsonrpc": Json::str("2.0"),
        "id": id,
        "error": obj! {
            "code": Json::num(UNSUPPORTED_PROTOCOL_VERSION as f64),
            "message": Json::str("Unsupported protocol version"),
            "data": obj! {
                "supported": supported_versions(),
                "requested": Json::str(requested),
            },
        },
    }
}

/// Everything we speak, newest first — modern revisions a client retries
/// with in `_meta`, then the legacy revisions reachable via `initialize`.
fn supported_versions() -> Json {
    Json::Arr(
        MODERN_VERSIONS
            .iter()
            .chain(LEGACY_VERSIONS.iter().rev())
            .map(|v| Json::str(*v))
            .collect(),
    )
}

fn discover_result() -> Json {
    let mut result = obj! {
        "resultType": Json::str("complete"),
        "supportedVersions": supported_versions(),
        "capabilities": obj! { "tools": Json::Obj(Vec::new()) },
        "instructions": Json::str(INSTRUCTIONS),
    };
    push_server_info(&mut result);
    cacheable(result)
}

/// Stamp a modern result: the required `resultType` (ours are always final —
/// we never issue multi-round-trip interim results) plus the recommended
/// server identity in `_meta`.
fn modernize(result: Json) -> Json {
    let Json::Obj(mut fields) = result else {
        return result;
    };
    if !fields.iter().any(|(k, _)| k == "resultType") {
        fields.insert(0, ("resultType".to_string(), Json::str("complete")));
    }
    let mut result = Json::Obj(fields);
    push_server_info(&mut result);
    result
}

/// Modernize the `result` inside a finished JSON-RPC reply (errors pass
/// through untouched — JSON-RPC error objects carry no result fields).
fn modernize_reply(reply: Json) -> Json {
    let Json::Obj(mut fields) = reply else {
        return reply;
    };
    for (key, value) in &mut fields {
        if key == "result" {
            *value = modernize(std::mem::replace(value, Json::Null));
        }
    }
    Json::Obj(fields)
}

fn push_server_info(result: &mut Json) {
    if let Json::Obj(fields) = result {
        fields.push((
            "_meta".to_string(),
            Json::Obj(vec![(META_SERVER_INFO.to_string(), server_info())]),
        ));
    }
}

/// The modern list results are `CacheableResult`s: `ttlMs`/`cacheScope` are
/// required. Our catalog is fixed for the life of the process and identical
/// for every caller, so a long public TTL is honest.
fn cacheable(result: Json) -> Json {
    let Json::Obj(mut fields) = result else {
        return result;
    };
    fields.push(("ttlMs".to_string(), Json::num(3_600_000.0)));
    fields.push(("cacheScope".to_string(), Json::str("public")));
    Json::Obj(fields)
}

// ---- legacy era (2025-11-25 and earlier): `initialize` handshake -----------

fn handle_legacy(id: Json, method: &str, params: &Json, bridge: &mut Bridge) -> Json {
    let result = match method {
        "initialize" => Ok(initialize_result(params)),
        "ping" => Ok(Json::Obj(Vec::new())),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => return tools_call_reply(id, params, bridge),
        other => Err((-32601, format!("method {other:?} not found"))),
    };
    match result {
        Ok(result) => ok_reply(id, result),
        Err((code, msg)) => error_reply(id, code, &msg),
    }
}

fn initialize_result(params: &Json) -> Json {
    // Echo a known client revision, else answer with our latest legacy —
    // echoing an arbitrary string would claim support for a revision we've
    // never seen, and a modern revision cannot be spoken through a handshake.
    let version = params
        .get("protocolVersion")
        .and_then(Json::as_str)
        .filter(|v| LEGACY_VERSIONS.contains(v))
        .unwrap_or(LATEST_LEGACY);
    obj! {
        "protocolVersion": Json::str(version),
        "capabilities": obj! { "tools": Json::Obj(Vec::new()) },
        "serverInfo": server_info(),
        "instructions": Json::str(INSTRUCTIONS),
    }
}

// ---- the era-independent tool surface --------------------------------------

fn server_info() -> Json {
    obj! {
        "name": Json::str("wowdps"),
        "version": Json::str(env!("CARGO_PKG_VERSION")),
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
