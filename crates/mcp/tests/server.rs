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
    start_daemon_on(tmp, FIXTURE)
}

fn start_daemon_on(tmp: &Temp, log: &str) -> PathBuf {
    start_daemon_with(tmp, log, |_| {})
}

fn start_daemon_with(tmp: &Temp, log: &str, tweak: impl FnOnce(&mut DaemonOptions)) -> PathBuf {
    let socket = tmp.0.join("test.sock");
    let mut opts = DaemonOptions {
        socket: socket.clone(),
        lockfile: tmp.0.join("test.lock"),
        source: SourceSpec::File(PathBuf::from(log)),
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
        history: None,
    };
    tweak(&mut opts);
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
        Some("2025-11-25"),
        "an unknown client revision gets our latest legacy, not an echo"
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
            "history",
            "progression",
            "trend",
            "stored_fight",
            "pin_fight",
            "loadout",
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

    // ---- the modern era (2026-07-28): stateless, per-request _meta ----------
    let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}"#;
    let replies = drive(
        &mut bridge,
        &[
            // The bare probe a dual-era client opens with — no _meta at all.
            r#"{"jsonrpc":"2.0","id":20,"method":"server/discover"}"#,
            &format!(r#"{{"jsonrpc":"2.0","id":21,"method":"tools/list","params":{{{meta}}}}}"#),
            // A version we don't speak must name what we do.
            r#"{"jsonrpc":"2.0","id":22,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2099-01-01"}}}"#,
            // ping was removed from the modern revision.
            &format!(r#"{{"jsonrpc":"2.0","id":23,"method":"ping","params":{{{meta}}}}}"#),
            &format!(
                r#"{{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{{"name":"status","arguments":{{}},{meta}}}}}"#
            ),
            // The version key worn but mistyped is a protocol fault, not a
            // legacy client — it must never be silently served legacy.
            r#"{"jsonrpc":"2.0","id":25,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":20260728}}}"#,
        ],
    );

    let discover = replies[0].get("result").expect("discover result");
    assert_eq!(str_of(discover, "resultType"), "complete");
    let versions: Vec<&str> = match discover.get("supportedVersions") {
        Some(Json::Arr(items)) => items.iter().filter_map(Json::as_str).collect(),
        other => panic!("no supportedVersions: {other:?}"),
    };
    assert_eq!(
        versions,
        [
            "2026-07-28",
            "2025-11-25",
            "2025-06-18",
            "2025-03-26",
            "2024-11-05"
        ],
        "everything we speak, newest first"
    );
    assert!(
        discover
            .get("capabilities")
            .and_then(|c| c.get("tools"))
            .is_some()
    );
    assert!(
        discover
            .get("_meta")
            .and_then(|m| m.get("io.modelcontextprotocol/serverInfo"))
            .is_some(),
        "discover carries serverInfo in _meta"
    );
    assert!(discover.get("ttlMs").is_some() && discover.get("cacheScope").is_some());

    let modern_list = replies[1].get("result").expect("modern tools/list");
    assert_eq!(str_of(modern_list, "resultType"), "complete");
    assert!(
        modern_list.get("ttlMs").is_some() && modern_list.get("cacheScope").is_some(),
        "modern list results are CacheableResults"
    );
    assert!(matches!(modern_list.get("tools"), Some(Json::Arr(_))));

    let unsupported = replies[2].get("error").expect("unsupported version error");
    assert_eq!(
        unsupported.get("code").and_then(Json::as_f64),
        Some(-32022.0)
    );
    let data = unsupported.get("data").expect("error data");
    assert_eq!(str_of(data, "requested"), "2099-01-01");
    assert!(
        matches!(data.get("supported"), Some(Json::Arr(items)) if !items.is_empty()),
        "the error names the versions we do speak"
    );

    assert_eq!(
        replies[3]
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Json::as_f64),
        Some(-32601.0),
        "modern ping is an unknown method"
    );

    let modern_call = replies[4].get("result").expect("modern tools/call");
    assert_eq!(str_of(modern_call, "resultType"), "complete");
    assert!(
        modern_call
            .get("_meta")
            .and_then(|m| m.get("io.modelcontextprotocol/serverInfo"))
            .is_some()
    );
    assert!(
        !is_error(&replies[4]),
        "status answers under the modern era"
    );

    assert_eq!(
        replies[5]
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Json::as_f64),
        Some(-32600.0),
        "a non-string _meta version is a protocol fault, not a legacy request"
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
    // The live row names its difficulty — Heroic and Mythic share a boss
    // name, and a coach must never have to know that 15 means Heroic.
    let enc = kill
        .get("encounter")
        .expect("live rows carry the encounter");
    assert_eq!(enc.get("id").and_then(Json::as_u64), Some(3130));
    assert_eq!(enc.get("difficulty").and_then(Json::as_u64), Some(15));
    assert_eq!(str_of(enc, "difficulty_name"), "Heroic");
    let kill_id = num_of(kill, "id") as u64;
    // The history store's stable id rides on closed rows; the same fight
    // answers to it wherever segment_id is accepted.
    let history_id = str_of(kill, "history_id").to_string();
    assert!(history_id.ends_with("-1785182700000"), "{history_id}");
    let by_fight_id = drive(
        &mut bridge,
        &[&call_line(
            10,
            "fight",
            &format!(r#"{{"fight_id":{history_id:?}}}"#),
        )],
    );
    let doc = tool_doc(&by_fight_id[0]);
    assert_eq!(
        doc.get("fight").map(|f| num_of(f, "id") as u64),
        Some(kill_id)
    );
    assert_eq!(
        doc.get("fight")
            .map(|f| str_of(f, "history_id").to_string()),
        Some(history_id)
    );
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
        doc.get("fight")
            .and_then(|f| f.get("encounter"))
            .map(|e| str_of(e, "difficulty_name").to_string()),
        Some("Heroic".to_string()),
        "the fight header names the difficulty too"
    );
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
              }, {
                "treeId": 11, "classId": 1, "className": "Warrior",
                "specs": [{"specId": 71, "name": "Arms", "role": 2}],
                "currencies": [{"index": 0, "id": 701}, {"index": 1, "id": 702}],
                "subTrees": [],
                "nodeOrder": [91024, 91025, 91026],
                "nodes": [
                  {"id": 91024, "type": "single", "posX": 0, "posY": 0, "maxRanks": 1,
                   "entries": [{"id": 124871, "spellId": 2001, "name": "Mortal Strike", "maxRanks": 1}]},
                  {"id": 91025, "type": "single", "posX": 0, "posY": 100, "maxRanks": 1,
                   "entries": [{"id": 124872, "spellId": 2002, "name": "Overpower", "maxRanks": 1}]},
                  {"id": 91026, "type": "single", "posX": 0, "posY": 200, "maxRanks": 1,
                   "entries": [{"id": 124873, "spellId": 2003, "name": "Slam", "maxRanks": 1}]}
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

    // ---- loadout: the fixture's COMBATANT_INFO named through the dataset ----
    let replies = drive(&mut bridge, &[&call_line(6, "list_fights", "{}")]);
    let list = tool_doc(&replies[0]);
    let kill_id = fights(&list)
        .iter()
        .find(|f| str_of(f, "name") == "The Ashen Warden")
        .map(|f| num_of(f, "id") as u64)
        .expect("the fixture's first encounter is listed");

    let replies = drive(
        &mut bridge,
        &[
            &call_line(
                7,
                "loadout",
                &format!("{{\"segment_id\":{kill_id},\"player\":\"Thraxx\"}}"),
            ),
            &call_line(8, "loadout", "{\"player\":\"Nobody\"}"),
        ],
    );
    let doc = tool_doc(&replies[0]);
    assert_eq!(
        doc.get("player").map(|p| str_of(p, "name").to_string()),
        Some("Thraxx-Nebula-US".to_string())
    );
    assert_eq!(doc.get("logged"), Some(&Json::Bool(true)));
    assert_eq!(num_of(&doc, "spec_id"), 71.0);

    let talents = doc.get("talents").expect("talents");
    assert_eq!(str_of(talents, "spec"), "Arms");
    assert!(
        !str_of(talents, "import_string").is_empty(),
        "the named path carries an import string"
    );
    let Some(Json::Arr(sels)) = talents.get("selections") else {
        panic!("no selections: {}", talents.to_line());
    };
    assert_eq!(sels.len(), 3, "all three fixture picks resolve");
    let granted = sels
        .iter()
        .find(|s| num_of(s, "node_id") == 91026.0)
        .expect("the rank-0 pick is selected");
    assert_eq!(
        granted.get("granted"),
        Some(&Json::Bool(true)),
        "rank 0 in the log means granted: {}",
        granted.to_line()
    );
    assert_eq!(str_of(granted, "name"), "Slam");
    assert!(
        matches!(talents.get("warnings"), Some(Json::Arr(w)) if w.is_empty()),
        "{}",
        talents.to_line()
    );

    let gear = doc.get("gear").expect("gear");
    let Some(Json::Arr(items)) = gear.get("items") else {
        panic!("no gear items: {}", gear.to_line());
    };
    assert_eq!(items.len(), 2, "the fixture equips two items");
    let head = &items[0];
    assert_eq!(str_of(head, "slot"), "head");
    assert_eq!(num_of(head, "item_id"), 212446.0);
    assert_eq!(num_of(head, "ilvl"), 639.0);
    assert!(matches!(head.get("bonus_ids"), Some(Json::Arr(b)) if b.len() == 2));
    assert!(
        matches!(items[1].get("gems"), Some(Json::Arr(g)) if g.len() == 1),
        "the second item's gem survives"
    );
    assert_eq!(num_of(gear, "avg_ilvl"), 639.0);

    // An unknown player is a tool-level error, exactly like breakdown's.
    assert!(is_error(&replies[1]), "{}", replies[1].to_line());
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

const INSTANCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/instance.txt");

/// The tool's error text, out of the content block.
fn error_text(reply: &Json) -> &str {
    assert!(is_error(reply), "expected a tool error: {reply:?}");
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

/// A dead segment id fails the load and the failure reaches the tool as
/// an error naming the fix; missing or bad arguments never touch the
/// daemon at all.
#[test]
fn load_failures_and_bad_arguments_are_tool_errors() {
    let tmp = Temp::new("errors");
    let socket = start_daemon(&tmp);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");
    let replies = drive(
        &mut bridge,
        &[
            &call_line(1, "fight", r#"{"segment_id": 999999}"#),
            &call_line(2, "breakdown", r#"{"segment_id": 999999, "player": "x"}"#),
            &call_line(
                3,
                "compare",
                r#"{"a": "Thraxx", "b": "Mírelle", "segment_id": 999999}"#,
            ),
            &call_line(4, "breakdown", "{}"),
            &call_line(5, "fight", r#"{"view": "bogus"}"#),
            &call_line(6, "loadout", r#"{"player": "Nobody-Here"}"#),
        ],
    );
    assert_eq!(replies.len(), 6);
    for reply in &replies[..3] {
        let text = error_text(reply);
        assert!(text.contains("no such segment"), "{text}");
    }
    assert!(error_text(&replies[3]).contains("\"player\" is required"));
    assert!(error_text(&replies[4]).contains("unknown view \"bogus\""));
    let text = error_text(&replies[5]);
    assert!(text.contains("no player \"Nobody-Here\""), "{text}");
}

/// R10 through the tools: a keystone visit lists its par timers and its
/// visit ordinal, and the Overall's fight header names the visit too.
#[test]
fn keystone_visits_list_their_par_timers() {
    let tmp = Temp::new("keystone");
    let socket = start_daemon_on(&tmp, INSTANCE);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");
    let replies = drive(&mut bridge, &[&call_line(1, "list_fights", "{}")]);
    let doc = tool_doc(&replies[0]);
    let keyed: Vec<&Json> = fights(&doc)
        .iter()
        .filter(|f| f.get("keystone_pars_ms").is_some())
        .collect();
    assert!(
        !keyed.is_empty(),
        "the instance fixture holds a keyed visit"
    );
    let overall = keyed[0];
    assert_eq!(str_of(overall, "kind"), "overall");
    assert!(overall.get("visit").is_some());
    match overall.get("keystone_pars_ms") {
        Some(Json::Arr(pars)) => {
            assert_eq!(pars.len(), 3);
            let ms: Vec<f64> = pars.iter().filter_map(Json::as_f64).collect();
            assert!(ms[0] > ms[1] && ms[1] > ms[2], "par > +2 > +3: {ms:?}");
        }
        other => panic!("pars: {other:?}"),
    }

    let id = num_of(overall, "id") as u64;
    let replies = drive(
        &mut bridge,
        &[&call_line(
            2,
            "fight",
            &format!(r#"{{"segment_id": {id}}}"#),
        )],
    );
    let doc = tool_doc(&replies[0]);
    let fight = doc.get("fight").expect("fight header");
    assert_eq!(str_of(fight, "kind"), "overall");
    assert!(fight.get("visit").is_some());
}

/// The death recap (R9) through the tool, a player whose build was never
/// logged, and the bridge's own argument check.
#[test]
fn death_recaps_unlogged_builds_and_bridge_argument_checks() {
    let tmp = Temp::new("recap");
    let socket = start_daemon(&tmp);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");
    let replies = drive(&mut bridge, &[&call_line(1, "list_fights", "{}")]);
    let doc = tool_doc(&replies[0]);
    let kill = fights(&doc)
        .iter()
        .find(|f| str_of(f, "name") == "The Ashen Warden")
        .expect("the kill");
    let id = num_of(kill, "id") as u64;
    let replies = drive(
        &mut bridge,
        &[&call_line(
            2,
            "breakdown",
            &format!(r#"{{"segment_id": {id}, "player": "Mírelle", "view": "deaths"}}"#),
        )],
    );
    let doc = tool_doc(&replies[0]);
    assert_eq!(str_of(&doc, "view"), "Deaths");
    let recap = match doc.get("death_recap") {
        Some(Json::Arr(items)) => items,
        other => panic!("no death_recap: {other:?}"),
    };
    assert!(!recap.is_empty(), "Mírelle died on the kill");
    assert!(
        recap.iter().any(|r| r.get("health_after").is_some()),
        "recap rows carry remaining health"
    );

    assert_eq!(
        bridge.snapshot(wowdps_proto::Cursor::List).err().as_deref(),
        Some("snapshot() takes a segment cursor")
    );

    // A log with no COMBATANT_INFO at all: the player is found but never
    // logged a build.
    let tmp2 = Temp::new("unlogged");
    let socket = start_daemon_on(&tmp2, INSTANCE);
    let stream = UnixStream::connect(&socket).expect("connect");
    let mut bridge = Bridge::over(stream).expect("handshake");
    let replies = drive(
        &mut bridge,
        &[&call_line(3, "loadout", r#"{"player": "Ana-Realm"}"#)],
    );
    let doc = tool_doc(&replies[0]);
    assert_eq!(doc.get("logged"), Some(&Json::Bool(false)));
    assert!(str_of(&doc, "note").contains("no COMBATANT_INFO"));
    assert_eq!(
        doc.get("player")
            .and_then(|p| p.get("key"))
            .and_then(Json::as_str),
        Some("Player-1-A")
    );
}

/// The overlay's every wording in `status`: a connected overlay is visible,
/// then hidden by its own report; a supervisor whose spawn fails reports
/// the failure — and a daemon that goes away mid-session is a tool error.
#[test]
fn status_words_the_overlay_and_a_lost_daemon() {
    use wowdps_proto::{ClientKind, ClientMsg, DaemonClient};

    let tmp = Temp::new("overlay");
    let socket = start_daemon(&tmp);
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("hs");
    let overlay_stream = UnixStream::connect(&socket).expect("connect");
    let mut overlay = DaemonClient::over(overlay_stream, ClientKind::Overlay).expect("hs");
    let status = |bridge: &mut Bridge| {
        let replies = drive(bridge, &[&call_line(1, "status", "{}")]);
        tool_doc(&replies[0])
    };
    let doc = status(&mut bridge);
    assert_eq!(str_of(&doc, "overlay"), "visible");
    assert_eq!(num_of(&doc, "clients"), 2.0);

    overlay.send(&ClientMsg::VisibilityChanged { visible: false });
    let deadline = Instant::now() + DEADLINE;
    loop {
        let doc = status(&mut bridge);
        if str_of(&doc, "overlay") == "hidden" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the manual hide never registered"
        );
        thread::sleep(Duration::from_millis(10));
    }

    // A supervisor set to spawn a gui that does not exist, woken by a
    // "game" whose process pattern is this very test binary.
    let me = std::env::current_exe()
        .expect("exe")
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let tmp2 = Temp::new("spawnfail");
    let socket = start_daemon_with(&tmp2, FIXTURE, move |opts| {
        opts.game_pattern = Some(me);
        opts.auto_overlay = true;
        opts.gui_bin = Some(tmp2_gui());
    });
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("hs");
    let deadline = Instant::now() + DEADLINE;
    loop {
        let doc = status(&mut bridge);
        if doc.get("game_running") == Some(&Json::Bool(true)) {
            let overlay = str_of(&doc, "overlay");
            assert!(overlay.starts_with("failed: spawning "), "{overlay}");
            break;
        }
        assert!(Instant::now() < deadline, "the game watcher never saw us");
        thread::sleep(Duration::from_millis(50));
    }

    // Shut the daemon down behind the bridge's back.
    use std::io::Write as _;
    UnixStream::connect(&socket)
        .expect("connect")
        .write_all(&ClientMsg::Shutdown.encode())
        .expect("shutdown");
    let deadline = Instant::now() + DEADLINE;
    while socket.exists() {
        assert!(Instant::now() < deadline, "daemon never left");
        thread::sleep(Duration::from_millis(10));
    }
    let replies = drive(&mut bridge, &[&call_line(9, "status", "{}")]);
    let text = error_text(&replies[0]);
    assert!(text.contains("daemon connection lost"), "{text}");
}

fn tmp2_gui() -> PathBuf {
    PathBuf::from("/nonexistent/wowdps-gui-for-tests")
}

// ---- v20: the history store's tools --------------------------------------------

fn history_opts(tmp: &Temp) -> wowdps_daemon::history::HistoryOptions {
    wowdps_daemon::history::HistoryOptions {
        dir: tmp.0.join("history"),
        store_trash: false,
        keep_per_encounter: 200,
        keep_details_per_encounter: 10,
        characters: Vec::new(),
        cache_dir: None,
    }
}

/// Poll `status` until the store holds `fights` cards with nothing importing.
fn wait_for_store(bridge: &mut Bridge, fights: u64) {
    let deadline = Instant::now() + DEADLINE;
    let mut last = Json::Null;
    while Instant::now() < deadline {
        let reply = drive(bridge, &[&call_line(1, "status", "{}")]);
        let doc = tool_doc(&reply[0]);
        let h = doc.get("history").cloned().unwrap_or(Json::Null);
        if h.get("fights").and_then(Json::as_u64) == Some(fights)
            && h.get("importing").and_then(Json::as_u64) == Some(0)
        {
            return;
        }
        last = h;
        thread::sleep(Duration::from_millis(20));
    }
    panic!("store never reached {fights} fights: {last:?}");
}

#[test]
fn history_tools_answer_over_the_store() {
    let tmp = Temp::new("history");
    let opts = history_opts(&tmp);
    let socket = start_daemon_with(&tmp, FIXTURE, |o| o.history = Some(opts));
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("bridge");
    wait_for_store(&mut bridge, 2);

    // Everything, newest first.
    let reply = drive(&mut bridge, &[&call_line(2, "history", "{}")]);
    assert!(!is_error(&reply[0]), "{:?}", reply[0]);
    let doc = tool_doc(&reply[0]);
    assert_eq!(doc.get("count").and_then(Json::as_u64), Some(2));
    let all = fights(&doc);
    assert_eq!(str_of(&all[0], "name"), "Verkath the Hollow");
    assert_eq!(str_of(&all[0], "result"), "wipe");
    assert_eq!(str_of(&all[1], "result"), "kill");
    assert!(str_of(&all[0], "date").ends_with(" UTC"));
    assert_eq!(str_of(&all[0], "build"), "12.0.0");
    // Default `players: me`: no roster, its size, and `me` — null here,
    // since one log alone cannot name the owner (spec §9).
    assert_eq!(doc.get("total").and_then(Json::as_u64), Some(2));
    assert_eq!(
        doc.get("next_after_id").and_then(Json::as_str),
        Some(str_of(&all[1], "id")),
        "the cursor for the next page is the last id answered"
    );
    assert_eq!(all[1].get("players"), Some(&Json::Null));
    assert_eq!(all[1].get("roster_size").and_then(Json::as_u64), Some(3));
    assert_eq!(all[1].get("me"), Some(&Json::Null));
    // `players: all`: the roster, each row with a role.
    let reply = drive(
        &mut bridge,
        &[&call_line(2, "history", r#"{"players":"all"}"#)],
    );
    let doc = tool_doc(&reply[0]);
    let all = fights(&doc);
    let players = match all[1].get("players") {
        Some(Json::Arr(p)) => p.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(players.len(), 3);
    assert!(
        players.iter().all(|p| matches!(
            p.get("role").and_then(Json::as_str),
            Some("dps" | "healer" | "tank")
        )),
        "{players:?}"
    );
    let guid = str_of(&players[0], "key").to_string();
    let name = str_of(&players[0], "name").to_string();

    // The best kill: fastest, limit 1.
    let reply = drive(
        &mut bridge,
        &[&call_line(3, "history", r#"{"sort":"fastest","limit":1}"#)],
    );
    let doc = tool_doc(&reply[0]);
    let best = fights(&doc);
    assert_eq!(best.len(), 1);
    assert_eq!(str_of(&best[0], "name"), "The Ashen Warden");
    let kill_id = str_of(&best[0], "id").to_string();

    // Filters: by encounter id, by player name, by an unknown kind.
    let reply = drive(
        &mut bridge,
        &[
            &call_line(4, "history", r#"{"encounter":3131}"#),
            &call_line(5, "history", &format!(r#"{{"player":"{name}"}}"#)),
            &call_line(6, "history", r#"{"kind":"key"}"#),
            &call_line(7, "history", r#"{"kind":"raid"}"#),
        ],
    );
    assert_eq!(fights(&tool_doc(&reply[0])).len(), 1);
    assert_eq!(fights(&tool_doc(&reply[1])).len(), 2);
    assert_eq!(fights(&tool_doc(&reply[2])).len(), 0);
    assert!(is_error(&reply[3]));

    // Progression on the kill's boss.
    let reply = drive(
        &mut bridge,
        &[&call_line(
            8,
            "progression",
            // The difficulty by name: the same question as `"difficulty":15`.
            r#"{"encounter":3130,"difficulty":"heroic"}"#,
        )],
    );
    let doc = tool_doc(&reply[0]);
    assert_eq!(doc.get("pulls").and_then(Json::as_u64), Some(1));
    assert_eq!(doc.get("kills").and_then(Json::as_u64), Some(1));
    assert_eq!(
        str_of(&doc.get("first_kill").cloned().unwrap(), "id"),
        kill_id
    );
    // References, not cards: no roster rides along; best_kill is the
    // fastest kill, here the only one.
    assert!(doc.get("first_kill").unwrap().get("players").is_none());
    assert_eq!(
        str_of(&doc.get("best_kill").cloned().unwrap(), "id"),
        kill_id
    );
    assert_eq!(str_of(&doc, "median_kill"), "1:00");
    match doc.get("nights") {
        Some(Json::Arr(n)) => {
            assert_eq!(n.len(), 1);
            assert_eq!(n[0].get("kill"), Some(&Json::Bool(true)));
            assert_eq!(n[0].get("best_pct").and_then(Json::as_u64), Some(0), "R16");
            assert_eq!(
                str_of(&n[0], "date"),
                "2026-07-28",
                "UTC-4 evening → next UTC day"
            );
        }
        other => panic!("{other:?}"),
    }

    // Trend for that player, per fight and per day.
    let reply = drive(
        &mut bridge,
        &[
            &call_line(9, "trend", &format!(r#"{{"player":"{guid}"}}"#)),
            &call_line(
                10,
                "trend",
                &format!(r#"{{"player":"{name}","bucket":"day"}}"#),
            ),
            &call_line(11, "trend", r#"{"player":"Nobody-Here"}"#),
        ],
    );
    let per_fight = tool_doc(&reply[0]);
    let per_day = tool_doc(&reply[1]);
    let count = |d: &Json| match d.get("points") {
        Some(Json::Arr(p)) => p.len(),
        other => panic!("{other:?}"),
    };
    assert_eq!(count(&per_fight), 2);
    assert_eq!(count(&per_day), 1);
    assert!(
        is_error(&reply[2]),
        "unknown player is an error, not an empty trend"
    );

    // stored_fight == fight for the same boss: identical rows.
    let reply = drive(&mut bridge, &[&call_line(12, "list_fights", "{}")]);
    let live = tool_doc(&reply[0]);
    let seg = fights(&live)
        .iter()
        .find(|f| str_of(f, "name") == "The Ashen Warden")
        .and_then(|f| f.get("id"))
        .and_then(Json::as_u64)
        .expect("the kill is listed");
    let reply = drive(
        &mut bridge,
        &[
            &call_line(13, "fight", &format!(r#"{{"segment_id":{seg}}}"#)),
            &call_line(
                14,
                "stored_fight",
                &format!(r#"{{"fight_id":"{kill_id}"}}"#),
            ),
        ],
    );
    let live_rows = tool_doc(&reply[0]).get("rows").cloned();
    let stored = tool_doc(&reply[1]);
    assert_eq!(
        stored.get("rows").cloned(),
        live_rows,
        "same rows, same shape"
    );
    assert_eq!(
        str_of(&stored.get("fight").cloned().unwrap(), "id"),
        kill_id
    );

    // Drilled: the kill keeps its details tier; deaths give the recap.
    let reply = drive(
        &mut bridge,
        &[
            &call_line(
                15,
                "stored_fight",
                &format!(r#"{{"fight_id":"{kill_id}","player":"{name}"}}"#),
            ),
            &call_line(16, "stored_fight", r#"{"fight_id":"nope"}"#),
        ],
    );
    let drilled = tool_doc(&reply[0]);
    assert!(matches!(drilled.get("by_ability"), Some(Json::Arr(a)) if !a.is_empty()));
    assert!(drilled.get("timeline").is_some());
    let missing = tool_doc(&reply[1]);
    assert_eq!(missing.get("stored"), Some(&Json::Bool(false)));

    // Pin it, and see it pinned.
    let reply = drive(
        &mut bridge,
        &[
            &call_line(17, "pin_fight", &format!(r#"{{"fight_id":"{kill_id}"}}"#)),
            &call_line(18, "history", r#"{"sort":"fastest","limit":1}"#),
            &call_line(19, "pin_fight", r#"{"fight_id":"nope","pinned":true}"#),
        ],
    );
    assert_eq!(tool_doc(&reply[0]).get("pinned"), Some(&Json::Bool(true)));
    assert_eq!(
        fights(&tool_doc(&reply[1]))[0].get("pinned"),
        Some(&Json::Bool(true))
    );
    assert_eq!(tool_doc(&reply[2]).get("pinned"), Some(&Json::Bool(false)));
}

#[test]
fn history_tools_answer_empty_without_a_store() {
    let tmp = Temp::new("nohistory");
    let socket = start_daemon(&tmp);
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("bridge");
    let reply = drive(
        &mut bridge,
        &[
            &call_line(1, "status", "{}"),
            &call_line(2, "history", "{}"),
            &call_line(3, "progression", r#"{"encounter":1,"difficulty":1}"#),
            &call_line(4, "trend", r#"{"player":"Player-1-A"}"#),
            &call_line(5, "stored_fight", r#"{"fight_id":"x"}"#),
            &call_line(6, "pin_fight", r#"{"fight_id":"x"}"#),
        ],
    );
    let status = tool_doc(&reply[0]);
    assert_eq!(
        status.get("history").and_then(|h| h.get("enabled")),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        tool_doc(&reply[1]).get("count").and_then(Json::as_u64),
        Some(0)
    );
    assert_eq!(
        tool_doc(&reply[2]).get("pulls").and_then(Json::as_u64),
        Some(0)
    );
    assert!(matches!(tool_doc(&reply[3]).get("points"), Some(Json::Arr(p)) if p.is_empty()));
    assert_eq!(tool_doc(&reply[4]).get("stored"), Some(&Json::Bool(false)));
    assert_eq!(tool_doc(&reply[5]).get("pinned"), Some(&Json::Bool(false)));
}
