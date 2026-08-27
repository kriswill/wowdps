//! End-to-end: a real daemon on a temp socket over the committed fixture, a
//! real `Bridge` over a real stream, and MCP request lines through
//! `rpc::serve` — asserting tool output against the fixture's hand-computed
//! golden totals (sample.expected.tsv).

// In tests a panic IS the failure mechanism (clippy.toml's intent). The
// helper fns below sit outside #[test] items, which clippy 1.98's
// allow-*-in-tests no longer exempts, so this integration-test crate says
// it explicitly.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::tail::SourceSpec;
use wowdps_daemon::{DaemonOptions, run};
use wowdps_mcp::{bridge::Bridge, json, json::Json, rpc};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(10);

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("wowdps-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        assert!(std::fs::create_dir_all(&p).is_ok(), "mkdir {p:?}");
        Temp(p)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Boot a lingering daemon on the fixture; returns its socket path (the
/// daemon thread dies with the process — linger keeps it from idle-exiting
/// under a slow test runner).
fn start_daemon(tmp: &Temp) -> PathBuf {
    let socket = tmp.0.join("test.sock");
    let opts = DaemonOptions {
        socket: socket.clone(),
        lockfile: tmp.0.join("test.lock"),
        source: SourceSpec::File(PathBuf::from(FIXTURE)),
        linger: true,
        idle_grace: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        version: "test".to_string(),
        cache_dir: None,
        game_pattern: None,
        loader_workers: 2,
        auto_overlay: false,
        overlay_exit_grace: Duration::ZERO,
        gui_bin: None,
    };
    let (tx, _rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(opts));
    });
    let deadline = Instant::now() + DEADLINE;
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon never bound {socket:?}");
        thread::sleep(Duration::from_millis(5));
    }
    socket
}

/// Feed request lines through the server; parse each response line back.
fn drive(bridge: &mut Bridge, requests: &[&str]) -> Vec<Json> {
    let input = requests.join("\n");
    let mut out = Vec::new();
    rpc::serve(input.as_bytes(), &mut out, bridge).expect("serve");
    String::from_utf8(out)
        .expect("utf8 output")
        .lines()
        .map(|l| json::parse(l).expect("response parses"))
        .collect()
}

fn call_line(id: u32, tool: &str, args: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#
    )
}

/// The tool result's JSON document, parsed out of the text content block.
fn tool_doc(reply: &Json) -> Json {
    let text = reply
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| match c {
            Json::Arr(items) => items.first(),
            _ => None,
        })
        .and_then(|b| b.get("text"))
        .and_then(Json::as_str)
        .expect("text content");
    json::parse(text).expect("tool output is JSON")
}

fn is_error(reply: &Json) -> bool {
    reply
        .get("result")
        .and_then(|r| r.get("isError"))
        .map(|e| *e == Json::Bool(true))
        .unwrap_or(false)
}

fn fights(doc: &Json) -> &[Json] {
    match doc.get("fights") {
        Some(Json::Arr(items)) => items,
        other => panic!("no fights array: {other:?}"),
    }
}

fn str_of<'j>(v: &'j Json, key: &str) -> &'j str {
    v.get(key).and_then(Json::as_str).expect(key)
}

fn num_of(v: &Json, key: &str) -> f64 {
    v.get(key).and_then(Json::as_f64).expect(key)
}

#[test]
fn the_whole_surface_over_a_real_daemon() {
    let tmp = Temp::new("surface");
    let socket = start_daemon(&tmp);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");

    // ---- handshake + catalog ------------------------------------------------
    let replies = drive(
        &mut bridge,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"no/such"}"#,
            "not json at all",
            r#"{"jsonrpc":"2.0","id":5,"method":"initialize","params":{"protocolVersion":"2099-01-01","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        ],
    );
    assert_eq!(replies.len(), 6, "notification must get no reply");

    let init = replies[0].get("result").expect("init result");
    assert_eq!(
        init.get("protocolVersion").and_then(Json::as_str),
        Some("2025-03-26"),
        "a known client revision is echoed"
    );
    let reinit = replies[5].get("result").expect("re-init result");
    assert_eq!(
        reinit.get("protocolVersion").and_then(Json::as_str),
        Some("2025-06-18"),
        "an unknown client revision gets our latest, not an echo"
    );
    assert!(
        init.get("capabilities")
            .and_then(|c| c.get("tools"))
            .is_some()
    );

    let tools = replies[1]
        .get("result")
        .and_then(|r| r.get("tools"))
        .cloned()
        .expect("tools");
    let names: Vec<String> = match &tools {
        Json::Arr(items) => items
            .iter()
            .map(|t| str_of(t, "name").to_string())
            .collect(),
        _ => panic!("tools is not an array"),
    };
    assert_eq!(
        names,
        [
            "status",
            "list_fights",
            "fight",
            "breakdown",
            "talent_tree",
            "decode_talents",
            "encode_talents",
            "compare"
        ]
    );

    assert_eq!(
        replies[2].get("result"),
        Some(&Json::Obj(Vec::new())),
        "ping"
    );
    assert_eq!(
        replies[3]
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Json::as_f64),
        Some(-32601.0)
    );
    assert_eq!(
        replies[4]
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Json::as_f64),
        Some(-32700.0)
    );

    // ---- list_fights: the fixture's shape -----------------------------------
    let replies = drive(&mut bridge, &[&call_line(10, "list_fights", "{}")]);
    let list = tool_doc(&replies[0]);
    let kill = fights(&list)
        .iter()
        .find(|f| str_of(f, "name") == "The Ashen Warden")
        .expect("the fixture's first encounter is listed");
    assert_eq!(str_of(kill, "result"), "kill");
    assert_eq!(num_of(kill, "duration_ms"), 60000.0);
    let kill_id = num_of(kill, "id") as u64;
    let wipe = fights(&list)
        .iter()
        .find(|f| str_of(f, "name") == "Verkath the Hollow")
        .expect("the wipe is listed");
    assert_eq!(str_of(wipe, "result"), "wipe");
    assert!(
        fights(&list).iter().any(|f| str_of(f, "kind") == "overall"),
        "the raid visit's Overall row is listed"
    );

    // ---- fight: rows match the hand-computed goldens ------------------------
    let replies = drive(
        &mut bridge,
        &[&call_line(
            11,
            "fight",
            &format!("{{\"segment_id\":{kill_id}}}"),
        )],
    );
    let doc = tool_doc(&replies[0]);
    assert_eq!(
        doc.get("fight").map(|f| str_of(f, "name").to_string()),
        Some("The Ashen Warden".to_string())
    );
    let rows = match doc.get("rows") {
        Some(Json::Arr(rows)) => rows.clone(),
        other => panic!("no rows: {other:?}"),
    };
    assert_eq!(rows.len(), 3, "three players in the fixture");
    let top = &rows[0];
    assert_eq!(str_of(top, "player"), "Thraxx-Nebula-US");
    assert_eq!(num_of(top, "amount"), 185370.0, "golden total (R1-R5)");
    assert_eq!(num_of(top, "per_sec"), 3089.5, "golden dps");
    assert_eq!(num_of(top, "share_pct"), 50.8);
    assert_eq!(num_of(top, "overkill"), 5200.0);
    let second = &rows[1];
    assert_eq!(
        num_of(second, "amount"),
        167200.0,
        "pet damage folds into the owner (R5)"
    );

    // ---- breakdown: drill by displayed name, timeline present ---------------
    let replies = drive(
        &mut bridge,
        &[&call_line(
            12,
            "breakdown",
            &format!("{{\"segment_id\":{kill_id},\"player\":\"Thraxx\"}}"),
        )],
    );
    let doc = tool_doc(&replies[0]);
    assert_eq!(
        doc.get("player").map(|p| str_of(p, "name").to_string()),
        Some("Thraxx-Nebula-US".to_string())
    );
    let abilities = match doc.get("by_ability") {
        Some(Json::Arr(a)) if !a.is_empty() => a.clone(),
        other => panic!("no abilities: {other:?}"),
    };
    let total: f64 = abilities.iter().map(|a| num_of(a, "amount")).sum();
    assert_eq!(total, 185370.0, "abilities sum to the meter row");
    let tl = doc
        .get("timeline")
        .expect("damage drill carries the timeline");
    assert!(matches!(tl.get("dps"), Some(Json::Arr(d)) if !d.is_empty()));

    // ---- compare: two sides, one clock --------------------------------------
    let replies = drive(
        &mut bridge,
        &[&call_line(
            13,
            "compare",
            &format!("{{\"segment_id\":{kill_id},\"a\":\"Thraxx\",\"b\":\"Kael\"}}"),
        )],
    );
    let doc = tool_doc(&replies[0]);
    let a = doc.get("a").expect("side a");
    let b = doc.get("b").expect("side b");
    assert_eq!(num_of(a, "total"), 185370.0);
    assert_eq!(num_of(b, "total"), 167200.0);
    assert!(matches!(a.get("abilities"), Some(Json::Arr(s)) if !s.is_empty()));
    assert!(
        matches!(b.get("timeline").and_then(|t| t.get("dps")), Some(Json::Arr(d)) if !d.is_empty())
    );

    // ---- tool-level failures are isError, not protocol faults ---------------
    let replies = drive(
        &mut bridge,
        &[
            &call_line(20, "fight", "{\"segment_id\":999999}"),
            &call_line(
                21,
                "breakdown",
                &format!("{{\"segment_id\":{kill_id},\"player\":\"Nobody\"}}"),
            ),
            &call_line(22, "fight", "{\"view\":\"nonsense\"}"),
            &call_line(23, "no_such_tool", "{}"),
        ],
    );
    for reply in &replies {
        assert!(is_error(reply), "{}", reply.to_line());
        assert!(reply.get("error").is_none(), "tool failures are results");
    }
}

/// The talent tools end to end: a synthetic dataset file (no Blizzard data
/// in the repo), `WOWDPS_TALENTS` pointing at it, and encode→decode driven
/// through the full rpc surface.
#[test]
fn talent_tools_over_a_fixture_dataset() {
    let tmp = Temp::new("talents");
    let dataset = tmp.0.join("talents.json");
    assert!(
        std::fs::write(
            &dataset,
            r#"{
              "build": "12.1.0.69497",
              "trees": [{
                "treeId": 10, "classId": 8, "className": "Mage",
                "specs": [{"specId": 62, "name": "Arcane", "role": 2},
                          {"specId": 63, "name": "Fire", "role": 2}],
                "currencies": [{"index": 0, "id": 601}, {"index": 1, "id": 602}],
                "subTrees": [{"id": 77, "name": "Sunfury", "specs": [62, 63]}],
                "nodeOrder": [1, 2],
                "nodes": [
                  {"id": 1, "type": "single", "posX": 0, "posY": 0, "maxRanks": 2,
                   "entries": [{"id": 101, "spellId": 1001, "name": "Filler", "maxRanks": 2}]},
                  {"id": 2, "type": "choice", "posX": 0, "posY": 100, "maxRanks": 1,
                   "visibleFor": [62],
                   "entries": [{"id": 131, "spellId": 1031, "name": "Left", "maxRanks": 1},
                               {"id": 132, "spellId": 1032, "name": "Right", "maxRanks": 1}]}
                ]
              }]
            }"#,
        )
        .is_ok()
    );
    // Process-global, but no other test reads it.
    unsafe { std::env::set_var("WOWDPS_TALENTS", &dataset) };

    let socket = start_daemon(&tmp);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");

    let replies = drive(
        &mut bridge,
        &[
            &call_line(1, "talent_tree", "{\"spec_id\":63}"),
            &call_line(
                2,
                "encode_talents",
                "{\"spec_id\":62,\"selections\":[{\"node_id\":1,\"ranks\":1},{\"node_id\":2,\"choice_index\":1}]}",
            ),
            &call_line(3, "decode_talents", "{\"string\":\"no such string\"}"),
            &call_line(4, "talent_tree", "{\"spec_id\":9999}"),
        ],
    );

    // Fire's view drops the Arcane-only choice node.
    let tree = tool_doc(&replies[0]);
    assert_eq!(str_of(&tree, "class"), "Mage");
    assert_eq!(str_of(&tree, "spec"), "Fire");
    assert!(matches!(tree.get("nodes"), Some(Json::Arr(n)) if n.len() == 1));

    let encoded = tool_doc(&replies[1]);
    assert_eq!(str_of(&encoded, "build"), "12.1.0.69497");
    let string = str_of(&encoded, "string").to_string();

    // Bad input and unknown specs are tool-level errors.
    assert!(is_error(&replies[2]), "{}", replies[2].to_line());
    assert!(is_error(&replies[3]), "{}", replies[3].to_line());

    let replies = drive(
        &mut bridge,
        &[&call_line(
            5,
            "decode_talents",
            &format!("{{\"string\":{}}}", Json::str(&string).to_line()),
        )],
    );
    let decoded = tool_doc(&replies[0]);
    assert_eq!(str_of(&decoded, "spec"), "Arcane");
    let Some(Json::Arr(sels)) = decoded.get("selections") else {
        panic!("no selections: {}", decoded.to_line());
    };
    assert_eq!(sels.len(), 2);
    assert_eq!(num_of(&sels[0], "ranks"), 1.0);
    assert_eq!(str_of(&sels[1], "name"), "Right");
    assert!(
        matches!(decoded.get("warnings"), Some(Json::Arr(w)) if w.is_empty()),
        "{}",
        decoded.to_line()
    );
}

/// The CONTRACT.md R14 gate: a REAL in-game exported string must decode
/// against the real per-machine dataset and re-encode byte-identically.
/// Blizzard-derived data stays out of the repo, so the string comes from
/// the environment (matching the WOWDPS_REAL_LOG pattern). Run with:
/// `WOWDPS_REAL_TALENT_STRING=C... cargo test -p wowdps-mcp -- --ignored real_talent --nocapture`
/// The dataset is the default `$XDG_DATA_HOME/wowdps/talents.json` (or
/// `$WOWDPS_TALENTS`), regenerated by tools/gen-talent-trees.sh.
#[test]
#[ignore = "needs WOWDPS_REAL_TALENT_STRING holding a real in-game export"]
fn real_talent_string_round_trips_byte_identically() {
    use wowdps_mcp::talents;

    let string = std::env::var("WOWDPS_REAL_TALENT_STRING").expect("set WOWDPS_REAL_TALENT_STRING");
    let dataset = talents::load().expect("talent dataset (run tools/gen-talent-trees.sh)");

    let decoded = talents::decode(dataset, &string).expect("decode");
    let warnings = decoded.get("warnings").cloned().expect("warnings");
    assert_eq!(
        warnings,
        Json::Arr(Vec::new()),
        "a real string must decode warning-free: {}",
        warnings.to_line()
    );

    // Rebuild the encoder's selections from the decode output verbatim.
    let Some(Json::Arr(sels)) = decoded.get("selections") else {
        panic!("no selections: {}", decoded.to_line());
    };
    let selections: Vec<Json> = sels
        .iter()
        .map(|s| {
            let mut o = vec![(
                "node_id".to_string(),
                s.get("node_id").cloned().expect("node_id"),
            )];
            if s.get("granted") == Some(&Json::Bool(true)) {
                o.push(("granted".to_string(), Json::Bool(true)));
            } else if let Some(r) = s.get("ranks") {
                o.push(("ranks".to_string(), r.clone()));
            }
            if let Some(c) = s.get("choice_index") {
                o.push(("choice_index".to_string(), c.clone()));
            }
            Json::Obj(o)
        })
        .collect();

    let spec_id = decoded.get("spec_id").and_then(Json::as_u64).expect("spec");
    let encoded = talents::encode(dataset, spec_id, &selections).expect("encode");
    assert_eq!(
        encoded.get("string").and_then(Json::as_str),
        Some(string.as_str()),
        "decode→encode must reproduce the exported string byte for byte"
    );
    eprintln!(
        "round-tripped {} chars for spec {spec_id} ({} selections)",
        string.len(),
        selections.len()
    );
}

#[test]
fn status_answers_without_a_cursor() {
    let tmp = Temp::new("status");
    let socket = start_daemon(&tmp);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");
    let replies = drive(&mut bridge, &[&call_line(1, "status", "{}")]);
    let doc = tool_doc(&replies[0]);
    assert_eq!(str_of(&doc, "daemon"), "running");
    assert!(str_of(&doc, "source").contains("sample.txt"));
    assert_eq!(doc.get("game_running"), Some(&Json::Bool(false)));
}
