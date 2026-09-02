//! The JSON-RPC layer's edges, no daemon needed: malformed requests, the
//! modern-era `_meta` handling, `server/discover`, and what a tool call
//! says when the daemon cannot be reached or spawned. The environment is
//! sandboxed so the lazy bridge can neither find the user's daemon nor
//! start one.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use wowdps_mcp::{bridge::Bridge, json, json::Json, rpc};

fn sandbox() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let root = std::env::temp_dir().join(format!("wdm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("rt")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        unsafe {
            // No socket to find, no `wowdps` on the path to spawn.
            std::env::set_var("XDG_RUNTIME_DIR", root.join("rt"));
            std::env::set_var("PATH", root.join("bin"));
            std::env::set_var("WOWDPS_TALENTS", root.join("missing.json"));
        }
    });
}

fn drive(requests: &[&str]) -> Vec<Json> {
    sandbox();
    let mut bridge = Bridge::lazy();
    let input = requests.join("\n");
    let mut out = Vec::new();
    rpc::serve(input.as_bytes(), &mut out, &mut bridge).expect("serve");
    String::from_utf8(out)
        .expect("utf8")
        .lines()
        .map(|l| json::parse(l).expect("response parses"))
        .collect()
}

fn error_code(reply: &Json) -> Option<f64> {
    reply
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Json::as_f64)
}

fn tool_text(reply: &Json) -> &str {
    reply
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| match c {
            Json::Arr(items) => items.first(),
            _ => None,
        })
        .and_then(|b| b.get("text"))
        .and_then(Json::as_str)
        .expect("text content")
}

#[test]
fn protocol_faults_answer_with_json_rpc_errors() {
    let replies = drive(&[
        "",
        "   ",
        r#"{"jsonrpc":"2.0","id":1}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{}}"#,
        r#"{"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":7}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1999-01-01"}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
    ]);
    assert_eq!(
        replies.len(),
        5,
        "blank lines and notifications get nothing"
    );
    assert_eq!(error_code(&replies[0]), Some(-32600.0), "no method");
    assert!(
        replies[0]
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Json::as_str)
            .is_some_and(|m| m.contains("no method"))
    );
    assert_eq!(error_code(&replies[1]), Some(-32602.0), "no tool name");
    assert_eq!(
        error_code(&replies[2]),
        Some(-32600.0),
        "non-string version"
    );
    assert_eq!(
        error_code(&replies[3]),
        Some(-32022.0),
        "unsupported version"
    );
    let data = replies[3]
        .get("error")
        .and_then(|e| e.get("data"))
        .expect("data");
    assert_eq!(
        data.get("requested").and_then(Json::as_str),
        Some("1999-01-01")
    );
    assert!(matches!(data.get("supported"), Some(Json::Arr(v)) if v.len() == 5));
    assert_eq!(
        error_code(&replies[4]),
        Some(-32601.0),
        "ping is not modern"
    );
}

#[test]
fn the_modern_era_stamps_results_and_discover_is_era_free() {
    let replies = drive(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"},"name":"decode_talents","arguments":{"string":"x"}}}"#,
    ]);
    assert_eq!(replies.len(), 3);
    let discover = replies[0].get("result").expect("result");
    assert_eq!(
        discover.get("resultType").and_then(Json::as_str),
        Some("complete")
    );
    assert!(discover.get("supportedVersions").is_some());
    assert!(discover.get("ttlMs").is_some());
    assert!(
        discover
            .get("_meta")
            .and_then(|m| m.get("io.modelcontextprotocol/serverInfo"))
            .and_then(|s| s.get("name"))
            .and_then(Json::as_str)
            == Some("wowdps")
    );

    let list = replies[1].get("result").expect("result");
    assert_eq!(
        list.get("resultType").and_then(Json::as_str),
        Some("complete")
    );
    assert_eq!(
        list.get("cacheScope").and_then(Json::as_str),
        Some("public")
    );
    assert!(matches!(list.get("tools"), Some(Json::Arr(t)) if t.len() == 9));

    // A modern tool call: the reply's result is stamped; the tool itself
    // failed (no dataset in the sandbox) and says so as a tool error.
    let call = replies[2].get("result").expect("result");
    assert_eq!(
        call.get("resultType").and_then(Json::as_str),
        Some("complete")
    );
    assert_eq!(call.get("isError"), Some(&Json::Bool(true)));
    assert!(tool_text(&replies[2]).contains("gen-talent-trees.sh"));
}

#[test]
fn an_unreachable_daemon_is_a_tool_error_not_a_dead_transport() {
    let replies = drive(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"status"}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_fights"}}"#,
    ]);
    assert_eq!(replies.len(), 2, "the transport survives");
    for reply in &replies {
        assert_eq!(
            reply.get("result").and_then(|r| r.get("isError")),
            Some(&Json::Bool(true))
        );
        let text = tool_text(reply);
        assert!(text.contains("cannot reach or spawn the daemon"), "{text}");
    }
}
