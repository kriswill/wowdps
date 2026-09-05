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
            "regrade_fights",
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
    // Step 3b (plan S4): the roster carries every player the fight's support
    // ledger names — sample.txt's second encounter trails a supporter guid
    // (Player-1168-0A1B2C04) that has no meter row anywhere, so its card
    // holds four players, not three; Σ effective over a card must equal
    // Σ damage.
    assert_eq!(all[1].get("roster_size").and_then(Json::as_u64), Some(4));
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
    // Step 3b (plan S4): the fourth row is the supporter sample.txt's
    // RANGE_DAMAGE_SUPPORT pair trails with — a guid with no name, spec or
    // meter row anywhere, carried so Σ effective over the card is Σ damage.
    assert_eq!(players.len(), 4, "{players:?}");
    let (supporter, named): (Vec<&Json>, Vec<&Json>) = players
        .iter()
        .partition(|p| str_of(p, "key") == "Player-1168-0A1B2C04");
    assert_eq!(supporter.len(), 1, "{players:?}");
    assert_eq!(supporter[0].get("role"), Some(&Json::Null));
    assert_eq!(supporter[0].get("damage").and_then(Json::as_u64), Some(0));
    assert_eq!(
        supporter[0].get("support_given").and_then(Json::as_u64),
        Some(29_400)
    );
    assert_eq!(f64_of(supporter[0], "effective_dps"), 490.0);
    assert_eq!(named.len(), 3);
    assert!(
        named.iter().all(|p| matches!(
            p.get("role").and_then(Json::as_str),
            Some("dps" | "healer" | "tank")
        )),
        "{players:?}"
    );
    let guid = str_of(named[0], "key").to_string();
    let name = str_of(named[0], "name").to_string();

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
    assert!(
        error_text(&reply[1]).contains("no stored fight nope"),
        "{:?}",
        reply[1]
    );
    // The tier answered, and what it can serve.
    assert_eq!(str_of(&drilled, "tier"), "details");
    // A boss drill on a plain encounter: nothing to drill into, said so.
    let reply = drive(
        &mut bridge,
        &[&call_line(
            16,
            "stored_fight",
            &format!(r#"{{"fight_id":"{kill_id}","boss":"Vexamus"}}"#),
        )],
    );
    assert!(
        error_text(&reply[0]).contains("no member bosses"),
        "{:?}",
        reply[0]
    );
    // Seven views plus the deaths recap, the Taken drill (rows tier, R17)
    // and the damage/healing drills of the details tier.
    assert!(matches!(drilled.get("available_views"), Some(Json::Arr(a)) if a.len() == 10));

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
    assert!(
        error_text(&reply[4]).contains("no stored fight x"),
        "{:?}",
        reply[4]
    );
    assert_eq!(tool_doc(&reply[5]).get("pinned"), Some(&Json::Bool(false)));
}

/// The `me` row of the newest fight with the store's owner set to `name`.
fn me_of_owner(tag: &str, name: &str) -> Json {
    let tmp = Temp::new(tag);
    let mut opts = history_opts(&tmp);
    opts.characters = vec![name.to_string()];
    let socket = start_daemon_with(&tmp, FIXTURE, |o| o.history = Some(opts));
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("bridge");
    wait_for_store(&mut bridge, 2);
    let reply = drive(
        &mut bridge,
        &[&call_line(2, "history", r#"{"sort":"fastest"}"#)],
    );
    assert!(!is_error(&reply[0]), "{:?}", reply[0]);
    let doc = tool_doc(&reply[0]);
    let all = fights(&doc);
    assert_eq!(str_of(&all[0], "name"), "The Ashen Warden");
    all[0].get("me").cloned().unwrap_or(Json::Null)
}

fn f64_of(row: &Json, key: &str) -> f64 {
    match row.get(key) {
        Some(Json::Num(n)) => *n,
        other => panic!("{key}: {other:?}"),
    }
}

#[test]
fn healer_owner_is_ranked_among_healers_by_hps() {
    // Mírelle (Discipline) is the fixture's only healer: the trivial
    // one-healer case, asserted explicitly — rank 1 of 1, the median is her
    // own HPS, and (nobody else heals on the kill) her share is all of it.
    let me = me_of_owner("owner-healer", "Mírelle");
    assert_eq!(str_of(&me, "name"), "Mírelle-Nebula-US");
    assert_eq!(str_of(&me, "role"), "healer");
    assert_eq!(str_of(&me, "rank_measure"), "hps");
    assert_eq!(me.get("rank").and_then(Json::as_u64), Some(1));
    assert_eq!(me.get("rank_count").and_then(Json::as_u64), Some(1));
    assert_eq!(me.get("rank_excluded").and_then(Json::as_u64), Some(0));
    let hps = f64_of(&me, "hps");
    assert!(hps > 0.0, "{me:?}");
    assert_eq!(f64_of(&me, "rank_median"), hps);
    assert_eq!(f64_of(&me, "rank_share"), 100.0);
    // The legacy DPS-pool block never ranked a healer and still does not;
    // its pool numbers describe the fight's two DPS as before.
    assert_eq!(me.get("rank_dps"), Some(&Json::Null));
    assert_eq!(me.get("dps_count").and_then(Json::as_u64), Some(2));
    assert_eq!(me.get("dps_excluded").and_then(Json::as_u64), Some(0));
    assert!(
        me.get("dps_median")
            .is_some_and(|m| !matches!(m, Json::Null))
    );
}

#[test]
fn dps_owner_generic_block_equals_the_legacy_block() {
    // Thraxx (Arms) is DPS-role: the role-relative block IS the old DPS
    // block, key for key, and every legacy key is populated.
    let me = me_of_owner("owner-dps", "Thraxx");
    assert_eq!(str_of(&me, "name"), "Thraxx-Nebula-US");
    assert_eq!(str_of(&me, "role"), "dps");
    // Step 3b: the generic block is labelled by what it ranks — effective
    // dps. Nobody supported Thraxx, so his own number is his dps bit for
    // bit, and so are his rank, count, exclusions and share (Σ effective =
    // Σ damage)…
    assert_eq!(str_of(&me, "rank_measure"), "effective_dps");
    assert_eq!(me.get("effective_dps"), me.get("dps"));
    assert_eq!(me.get("support"), Some(&Json::Bool(false)));
    assert_eq!(me.get("support_received").and_then(Json::as_u64), Some(0));
    for (generic, legacy) in [
        ("rank", "rank_dps"),
        ("rank_count", "dps_count"),
        ("rank_excluded", "dps_excluded"),
        ("rank_share", "dps_share"),
    ] {
        assert_eq!(
            me.get(generic),
            me.get(legacy),
            "{generic} vs {legacy}: {me:?}"
        );
    }
    // …but the fixture's RANGE_DAMAGE_SUPPORT pair moves 29 400 of Kael'thar's
    // 167 200 to a supporter (sample.expected.md's addendum), so the pool's
    // median differs between the two blocks: raw (185 370 + 167 200) / 2
    // over 60 s against effective (185 370 + 137 800) / 2. This is the one
    // key the two blocks may disagree on for an unbuffed player.
    assert_eq!(f64_of(&me, "dps_median"), 2938.1);
    assert_eq!(f64_of(&me, "rank_median"), 2693.1);
    assert!(matches!(me.get("rank_dps"), Some(Json::Num(_))), "{me:?}");
    assert_eq!(me.get("dps_count").and_then(Json::as_u64), Some(2));
    assert!(matches!(me.get("dps_median"), Some(Json::Num(_))));
    assert!(matches!(me.get("dps_share"), Some(Json::Num(_))));
    assert!(f64_of(&me, "dps_share") < 100.0);
}

// ---- v22 (R17, step 2b): the tank side ------------------------------------------

const TAKEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");

/// A daemon over `taken.txt` with a store, optionally owned by `owner`.
/// Only the encounter is stored (`store_trash: false`), so the store settles
/// at one card.
fn taken_daemon(tag: &str, owner: Option<&str>) -> (Temp, Bridge) {
    let tmp = Temp::new(tag);
    let mut opts = history_opts(&tmp);
    opts.characters = owner.map(|o| vec![o.to_string()]).unwrap_or_default();
    let socket = start_daemon_with(&tmp, TAKEN, |o| o.history = Some(opts));
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("bridge");
    wait_for_store(&mut bridge, 1);
    (tmp, bridge)
}

/// The live Taken meter over `taken.txt`, its drill, and the same drill out
/// of the store — the numbers are `taken.expected.md`'s, recomputed there
/// from the log by `check.awk`.
#[test]
fn the_taken_view_reads_the_tank_side_live_and_stored() {
    let (_tmp, mut bridge) = taken_daemon("taken-view", None);

    let reply = drive(&mut bridge, &[&call_line(1, "list_fights", "{}")]);
    let listed = tool_doc(&reply[0]);
    let boss = fights(&listed)
        .iter()
        .find(|f| str_of(f, "name") == "Taken Test Boss")
        .cloned()
        .expect("the encounter is listed");
    let seg = boss.get("id").and_then(Json::as_u64).expect("id");
    let fight_id = str_of(&boss, "history_id").to_string();

    // The meter: taken per player, DTPS as per_sec, absorbed as the extra.
    let reply = drive(
        &mut bridge,
        &[&call_line(
            2,
            "fight",
            &format!(r#"{{"segment_id":{seg},"view":"taken"}}"#),
        )],
    );
    assert!(!is_error(&reply[0]), "{:?}", reply[0]);
    let doc = tool_doc(&reply[0]);
    assert_eq!(str_of(&doc, "view"), "Taken");
    let rows = match doc.get("rows") {
        Some(Json::Arr(r)) => r.clone(),
        other => panic!("{other:?}"),
    };
    let taken_of = |name: &str| {
        rows.iter()
            .find(|r| str_of(r, "player").starts_with(name))
            .cloned()
            .unwrap_or_else(|| panic!("no row for {name}: {rows:?}"))
    };
    let durgan = taken_of("Durgan");
    assert_eq!(durgan.get("amount").and_then(Json::as_u64), Some(84_000));
    assert_eq!(num_of(&durgan, "per_sec"), 1400.0, "84 000 over 60 s");
    assert_eq!(
        durgan.get("absorbed").and_then(Json::as_u64),
        Some(12_000),
        "a Taken row's extra is what was absorbed of it, never overkill"
    );
    assert_eq!(str_of(&durgan, "role"), "tank");
    assert_eq!(
        taken_of("Zenlí").get("amount").and_then(Json::as_u64),
        Some(70_200)
    );
    assert_eq!(
        taken_of("Pyralis").get("amount").and_then(Json::as_u64),
        Some(52_000),
        "both pet hits fold onto their owner"
    );

    // The drill: what hit the tank, who hit them, and the mitigation split.
    let reply = drive(
        &mut bridge,
        &[
            &call_line(
                3,
                "breakdown",
                &format!(r#"{{"segment_id":{seg},"player":"Durgan","view":"taken"}}"#),
            ),
            &call_line(
                4,
                "stored_fight",
                &format!(r#"{{"fight_id":"{fight_id}","player":"Durgan","view":"taken"}}"#),
            ),
        ],
    );
    assert!(!is_error(&reply[0]), "{:?}", reply[0]);
    let live = tool_doc(&reply[0]);
    let by_target = match live.get("by_target") {
        Some(Json::Arr(t)) => t.clone(),
        other => panic!("{other:?}"),
    };
    let from_boss = by_target
        .iter()
        .find(|r| str_of(r, "name") == "Taken Test Boss")
        .cloned()
        .expect("the boss dealt it");
    assert_eq!(from_boss.get("amount").and_then(Json::as_u64), Some(84_000));
    let m = live.get("mitigation").cloned().expect("mitigation object");
    assert_eq!(m.get("absorbed").and_then(Json::as_u64), Some(12_000));
    assert_eq!(m.get("blocked").and_then(Json::as_u64), Some(18_000));
    assert_eq!(m.get("blocked_full").and_then(Json::as_u64), Some(55_000));
    assert_eq!(m.get("prevented").and_then(Json::as_u64), Some(55_000));
    assert_eq!(m.get("mitigated").and_then(Json::as_u64), Some(85_000));
    assert_eq!(
        num_of(&m, "mitigated_pct"),
        61.2,
        "85 000 / (84 000 + 55 000)"
    );
    assert_eq!(m.get("stagger").and_then(Json::as_u64), Some(0));
    let misses = m.get("misses").cloned().expect("misses");
    assert_eq!(
        misses.get("total").and_then(Json::as_u64),
        Some(5),
        "BLOCK, PARRY, DODGE, MISS, MISS"
    );
    assert_eq!(misses.get("block").and_then(Json::as_u64), Some(1));
    assert_eq!(
        misses.get("evade"),
        None,
        "only the kinds that happened are listed"
    );
    assert_eq!(
        m.get("by_ability_other").and_then(Json::as_u64),
        Some(0),
        "a boss pull folds nothing: by_ability sums to the taken row"
    );

    // The stored drill is the live one, key for key.
    assert!(!is_error(&reply[1]), "{:?}", reply[1]);
    let stored = tool_doc(&reply[1]);
    for key in ["by_ability", "by_target", "mitigation", "player"] {
        assert_eq!(
            stored.get(key),
            live.get(key),
            "{key}: stored_fight must answer exactly what breakdown does"
        );
    }
}

/// The monk's stagger pair and the mage's full absorb: the two mitigation
/// shapes that are not the warrior's block.
#[test]
fn stagger_and_full_absorbs_show_up_in_the_mitigation_object() {
    let (_tmp, mut bridge) = taken_daemon("taken-shapes", None);
    // The live fight is the trailing Trash stretch; the encounter is named.
    let reply = drive(&mut bridge, &[&call_line(1, "list_fights", "{}")]);
    let seg = fights(&tool_doc(&reply[0]))
        .iter()
        .find(|f| str_of(f, "name") == "Taken Test Boss")
        .and_then(|f| f.get("id"))
        .and_then(Json::as_u64)
        .expect("the encounter is listed");
    let drill = |bridge: &mut Bridge, id: u32, who: &str| {
        let reply = drive(
            bridge,
            &[&call_line(
                id,
                "breakdown",
                &format!(r#"{{"segment_id":{seg},"player":"{who}","view":"taken"}}"#),
            )],
        );
        assert!(!is_error(&reply[0]), "{:?}", reply[0]);
        tool_doc(&reply[0])
    };
    // Zenlí: taken 70 200, mitigated 28 000 (25 000 absorbed + 3 000 full),
    // stagger 25 000 of which 10 000 was ticked back out.
    let m = drill(&mut bridge, 5, "Zenlí")
        .get("mitigation")
        .cloned()
        .expect("mitigation");
    assert_eq!(m.get("absorbed").and_then(Json::as_u64), Some(25_000));
    assert_eq!(m.get("absorbed_full").and_then(Json::as_u64), Some(3_000));
    assert_eq!(m.get("mitigated").and_then(Json::as_u64), Some(28_000));
    assert_eq!(m.get("stagger").and_then(Json::as_u64), Some(25_000));
    assert_eq!(m.get("stagger_ticked").and_then(Json::as_u64), Some(10_000));
    assert_eq!(num_of(&m, "mitigated_pct"), 38.3, "28 000 / 73 200");
    // Pyralis: five misses of five different kinds, 21 000 prevented.
    let m = drill(&mut bridge, 6, "Pyralis")
        .get("mitigation")
        .cloned()
        .expect("mitigation");
    assert_eq!(m.get("prevented").and_then(Json::as_u64), Some(21_000));
    assert_eq!(m.get("mitigated").and_then(Json::as_u64), Some(26_000));
    let misses = m.get("misses").cloned().expect("misses");
    assert_eq!(misses.get("total").and_then(Json::as_u64), Some(5));
    for kind in ["immune", "absorb", "deflect", "reflect", "resist"] {
        assert_eq!(
            misses.get(kind).and_then(Json::as_u64),
            Some(1),
            "{kind}: {misses:?}"
        );
    }
}

/// The `me` row of `taken.txt`'s only stored fight, owned by `name`.
fn taken_me(tag: &str, name: &str) -> Json {
    let (_tmp, mut bridge) = taken_daemon(tag, Some(name));
    let reply = drive(&mut bridge, &[&call_line(2, "history", "{}")]);
    assert!(!is_error(&reply[0]), "{:?}", reply[0]);
    let doc = tool_doc(&reply[0]);
    let all = fights(&doc);
    assert_eq!(str_of(&all[0], "name"), "Taken Test Boss");
    all[0].get("me").cloned().unwrap_or(Json::Null)
}

#[test]
fn a_tank_owner_reads_the_card_measures_and_the_tank_pair() {
    // Durgan is Protection Warrior: unranked, but with his own numbers and
    // the co-tank beside him, heaviest first.
    let me = taken_me("taken-owner-tank", "Durgan");
    assert_eq!(str_of(&me, "role"), "tank");
    assert_eq!(me.get("rank_measure"), Some(&Json::Null));
    assert_eq!(me.get("rank"), Some(&Json::Null));
    assert_eq!(me.get("taken").and_then(Json::as_u64), Some(84_000));
    assert_eq!(me.get("mitigated").and_then(Json::as_u64), Some(85_000));
    assert_eq!(me.get("prevented").and_then(Json::as_u64), Some(55_000));
    assert_eq!(f64_of(&me, "mitigated_pct"), 61.2);
    assert_eq!(f64_of(&me, "dtps"), 1400.0);
    let pair = match me.get("tank_pair") {
        Some(Json::Arr(p)) => p.clone(),
        other => panic!("no tank_pair: {other:?}"),
    };
    assert_eq!(pair.len(), 2, "the fixture's warrior and monk");
    assert!(str_of(&pair[0], "name").starts_with("Durgan"));
    assert!(str_of(&pair[1], "name").starts_with("Zenlí"));
    assert_eq!(pair[0].get("taken").and_then(Json::as_u64), Some(84_000));
    assert_eq!(pair[1].get("taken").and_then(Json::as_u64), Some(70_200));
    assert_eq!(f64_of(&pair[1], "dtps"), 1170.0);
}

#[test]
fn a_non_tank_owner_gets_the_measures_but_no_tank_pair() {
    let me = taken_me("taken-owner-dps", "Pyralis");
    assert_eq!(str_of(&me, "role"), "dps");
    assert_eq!(
        me.get("tank_pair"),
        None,
        "only a tank gets a co-tank block"
    );
    assert_eq!(me.get("taken").and_then(Json::as_u64), Some(52_000));
    assert_eq!(f64_of(&me, "dtps"), 866.7);
    assert_eq!(str_of(&me, "rank_measure"), "effective_dps");
}

#[test]
fn trend_takes_a_measure_and_defaults_it_by_role() {
    let (_tmp, mut bridge) = taken_daemon("taken-trend", None);
    let one = |doc: &Json, key: &str| -> f64 {
        match doc.get("points") {
            Some(Json::Arr(p)) if p.len() == 1 => num_of(&p[0], key),
            other => panic!("{other:?}"),
        }
    };
    let reply = drive(
        &mut bridge,
        &[
            &call_line(2, "trend", r#"{"player":"Durgan"}"#),
            &call_line(3, "trend", r#"{"player":"Durgan","measure":"dtps"}"#),
            &call_line(4, "trend", r#"{"player":"Pyralis"}"#),
            &call_line(5, "trend", r#"{"player":"Pyralis","view":"healing"}"#),
            &call_line(6, "trend", r#"{"player":"Durgan","measure":"bogus"}"#),
        ],
    );
    // A tank's default measure is what he turned away.
    let tank = tool_doc(&reply[0]);
    assert_eq!(str_of(&tank, "measure"), "mitigated_pct");
    assert_eq!(one(&tank, "mitigated_pct"), 61.2);
    // …and the named measure wins, naming its own field.
    let dtps = tool_doc(&reply[1]);
    assert_eq!(str_of(&dtps, "measure"), "dtps");
    assert_eq!(one(&dtps, "dtps"), 1400.0);
    // …and `per_sec` stays as an alias of the same value: the wow-coach
    // skill reads `points[].per_sec`.
    assert_eq!(one(&dtps, "per_sec"), 1400.0);
    // A DPS player defaults to effective DPS (step 3b — dps bit for bit on a
    // fight without an Augmentation); `view` still maps onto hps for a
    // release.
    assert_eq!(str_of(&tool_doc(&reply[2]), "measure"), "effective_dps");
    assert_eq!(str_of(&tool_doc(&reply[3]), "measure"), "hps");
    assert!(
        error_text(&reply[4]).contains("unknown measure"),
        "{:?}",
        reply[4]
    );
}

#[test]
fn history_filters_fights_by_the_subjects_role() {
    let (_tmp, mut bridge) = taken_daemon("taken-role", Some("Durgan"));
    let reply = drive(
        &mut bridge,
        &[
            &call_line(2, "history", r#"{"role":"tank"}"#),
            &call_line(3, "history", r#"{"role":"healer"}"#),
            &call_line(4, "history", r#"{"role":"paladin"}"#),
        ],
    );
    let tank = tool_doc(&reply[0]);
    assert_eq!(tank.get("total").and_then(Json::as_u64), Some(1));
    assert_eq!(str_of(&fights(&tank)[0], "name"), "Taken Test Boss");
    assert_eq!(
        tank.get("role_applied"),
        Some(&Json::Bool(true)),
        "the owner is the subject, so the filter applied"
    );
    assert_eq!(tank.get("note"), None);
    let healer = tool_doc(&reply[1]);
    assert_eq!(
        healer.get("total").and_then(Json::as_u64),
        Some(0),
        "the owner never healed this fixture"
    );
    assert!(matches!(healer.get("fights"), Some(Json::Arr(f)) if f.is_empty()));
    assert!(
        error_text(&reply[2]).contains("unknown role"),
        "{:?}",
        reply[2]
    );
}

/// Without an owner and without `player` there is no subject: the daemon
/// skips the role filter, and the answer says so instead of pretending.
#[test]
fn history_role_without_a_subject_is_reported_not_applied() {
    let (_tmp, mut bridge) = taken_daemon("taken-role-nosubject", None);
    let reply = drive(
        &mut bridge,
        &[
            &call_line(2, "history", r#"{"role":"tank"}"#),
            &call_line(3, "history", r#"{"role":"tank","player":"Durgan"}"#),
            &call_line(4, "history", r#"{"role":"healer","player":"Durgan"}"#),
            &call_line(5, "history", "{}"),
        ],
    );
    let unapplied = tool_doc(&reply[0]);
    assert_eq!(
        unapplied.get("total").and_then(Json::as_u64),
        Some(1),
        "the filter was a no-op: the fight is still listed"
    );
    assert_eq!(unapplied.get("role_applied"), Some(&Json::Bool(false)));
    assert_eq!(
        str_of(&unapplied, "note"),
        "role filter needs a subject: pass player, or set history_characters"
    );
    // Naming the player makes them the subject, owner or not.
    let named = tool_doc(&reply[1]);
    assert_eq!(named.get("role_applied"), Some(&Json::Bool(true)));
    assert_eq!(named.get("note"), None);
    assert_eq!(named.get("total").and_then(Json::as_u64), Some(1));
    let healer = tool_doc(&reply[2]);
    assert_eq!(healer.get("role_applied"), Some(&Json::Bool(true)));
    assert_eq!(healer.get("total").and_then(Json::as_u64), Some(0));
    assert_eq!(healer.get("note"), None, "an empty page needs no note");
    // No role asked: neither key appears.
    let plain = tool_doc(&reply[3]);
    assert_eq!(plain.get("role_applied"), None);
    assert_eq!(plain.get("note"), None);
}

// ---- v23 (R19, step 3b): support and the healing split ---------------------------

const SUPPORT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/support.txt");

/// A daemon over `support.txt` with a store, optionally owned by `owner`.
/// Only the encounter is stored (`store_trash: false`), so the store settles
/// at one card. Every number below is `support.expected.md`'s, recomputed
/// there from the log by `check.awk`.
fn support_daemon(tag: &str, owner: Option<&str>) -> (Temp, Bridge) {
    let tmp = Temp::new(tag);
    let mut opts = history_opts(&tmp);
    opts.characters = owner.map(|o| vec![o.to_string()]).unwrap_or_default();
    let socket = start_daemon_with(&tmp, SUPPORT, |o| o.history = Some(opts));
    let mut bridge = Bridge::over(UnixStream::connect(&socket).expect("connect")).expect("bridge");
    wait_for_store(&mut bridge, 1);
    (tmp, bridge)
}

/// The `me` row of `support.txt`'s only stored fight, owned by `name`.
fn support_me(tag: &str, name: &str) -> Json {
    let (_tmp, mut bridge) = support_daemon(tag, Some(name));
    let reply = drive(&mut bridge, &[&call_line(2, "history", "{}")]);
    assert!(!is_error(&reply[0]), "{:?}", reply[0]);
    let doc = tool_doc(&reply[0]);
    let all = fights(&doc);
    assert_eq!(str_of(&all[0], "name"), "Support Test Boss");
    all[0].get("me").cloned().unwrap_or(Json::Null)
}

fn u64_of(row: &Json, key: &str) -> u64 {
    match row.get(key) {
        Some(Json::Num(n)) => *n as u64,
        other => panic!("{key}: {other:?}"),
    }
}

#[test]
fn an_augmentation_owner_is_graded_by_effective_dps_and_flagged_support() {
    // Vessyra (Augmentation): 69 500 raw damage, 23 900 given, 7 500 received
    // (the self-supported Bombardments proc, once) → 85 900 effective over
    // 60 s. Both grades rank her last of the three DPS — 85 900 against the
    // Mage's 269 350 and the Warrior's 227 250 by effective, 69 500 against
    // 271 000 and 242 000 raw — but on different numbers: the generic
    // median is the Warrior's effective 227 250 / 60, the legacy one his
    // raw 242 000 / 60, and her share climbs from 11.9% to 14.7%.
    let me = support_me("support-owner-evoker", "Vessyra");
    assert_eq!(str_of(&me, "name"), "Vessyra-Nebula-US");
    assert_eq!(str_of(&me, "spec"), "Augmentation");
    assert_eq!(str_of(&me, "role"), "dps");
    assert_eq!(me.get("support"), Some(&Json::Bool(true)));
    assert_eq!(u64_of(&me, "damage"), 69_500);
    assert_eq!(f64_of(&me, "dps"), 1158.3);
    assert_eq!(u64_of(&me, "support_given"), 23_900);
    assert_eq!(u64_of(&me, "support_received"), 7_500);
    assert_eq!(f64_of(&me, "effective_dps"), 1431.7);
    assert_eq!(u64_of(&me, "healed_received"), 10_000);
    assert_eq!(u64_of(&me, "self_healed"), 0);
    assert_eq!(u64_of(&me, "overheal"), 0);
    assert_eq!(u64_of(&me, "absorbed"), 0);
    assert_eq!(str_of(&me, "rank_measure"), "effective_dps");
    assert_eq!(me.get("rank").and_then(Json::as_u64), Some(3));
    assert_eq!(me.get("rank_count").and_then(Json::as_u64), Some(3));
    assert_eq!(me.get("rank_excluded").and_then(Json::as_u64), Some(0));
    assert_eq!(f64_of(&me, "rank_median"), 3787.5);
    assert_eq!(f64_of(&me, "rank_share"), 14.7);
    // The legacy block is raw dps: 69 500 ranks last among 271 000 / 242 000.
    assert_eq!(me.get("rank_dps").and_then(Json::as_u64), Some(3));
    assert_eq!(me.get("dps_count").and_then(Json::as_u64), Some(3));
    assert_eq!(f64_of(&me, "dps_median"), 4033.3);
    assert_eq!(f64_of(&me, "dps_share"), 11.9);
    assert_eq!(me.get("tank_pair"), None);
}

#[test]
fn a_buffed_mage_owner_ranks_first_on_effective_dps_below_its_raw_dps() {
    // Ignatia (Fire): 271 000 raw with 1 650 of it an Augmentation's shares
    // (the Water Elemental's 90 included) → 269 350 effective; first either
    // way, and not a support spec.
    let me = support_me("support-owner-mage", "Ignatia");
    assert_eq!(str_of(&me, "name"), "Ignatia-Nebula-US");
    assert_eq!(me.get("support"), Some(&Json::Bool(false)));
    assert_eq!(u64_of(&me, "damage"), 271_000);
    assert_eq!(f64_of(&me, "dps"), 4516.7);
    assert_eq!(u64_of(&me, "support_given"), 0);
    assert_eq!(u64_of(&me, "support_received"), 1_650);
    assert_eq!(f64_of(&me, "effective_dps"), 4489.2);
    assert!(f64_of(&me, "effective_dps") < f64_of(&me, "dps"));
    assert_eq!(u64_of(&me, "healed_received"), 5_000, "the heal on her pet");
    assert_eq!(str_of(&me, "rank_measure"), "effective_dps");
    assert_eq!(me.get("rank").and_then(Json::as_u64), Some(1));
    assert_eq!(me.get("rank_dps").and_then(Json::as_u64), Some(1));
    assert_eq!(f64_of(&me, "rank_share"), 46.2);
    assert_eq!(f64_of(&me, "dps_share"), 46.5);
}

#[test]
fn a_healer_owner_reads_the_healing_split_and_the_self_healed_pair() {
    // Seraphíne (Holy Priest): 88 000 healing of which 15 000 absorbs and
    // 16 000 overhealing; both Renew ticks were on herself.
    let me = support_me("support-owner-priest", "Seraphíne");
    assert_eq!(str_of(&me, "role"), "healer");
    assert_eq!(me.get("support"), Some(&Json::Bool(false)));
    assert_eq!(u64_of(&me, "healing"), 88_000);
    assert_eq!(f64_of(&me, "hps"), 1466.7);
    assert_eq!(u64_of(&me, "overheal"), 16_000);
    assert_eq!(u64_of(&me, "absorbed"), 15_000);
    assert_eq!(u64_of(&me, "self_healed"), 13_000);
    assert_eq!(u64_of(&me, "healed_received"), 13_000);
    assert_eq!(u64_of(&me, "support_given"), 0);
    assert_eq!(u64_of(&me, "support_received"), 0);
    assert_eq!(f64_of(&me, "effective_dps"), 0.0);
    assert_eq!(str_of(&me, "rank_measure"), "hps");
    assert_eq!(me.get("rank").and_then(Json::as_u64), Some(1));
    // The legacy block describes the three DPS, raw, and never ranks her.
    assert_eq!(me.get("rank_dps"), Some(&Json::Null));
    assert_eq!(me.get("dps_count").and_then(Json::as_u64), Some(3));
    assert_eq!(f64_of(&me, "dps_median"), 4033.3);
}

#[test]
fn the_roster_and_a_peer_carry_the_support_scalars() {
    let (_tmp, mut bridge) = support_daemon("support-roster", Some("Vessyra"));
    let reply = drive(
        &mut bridge,
        &[
            &call_line(2, "history", r#"{"players":"all"}"#),
            &call_line(3, "history", r#"{"players":"Brakkar"}"#),
        ],
    );
    let all = tool_doc(&reply[0]);
    let players = match fights(&all)[0].get("players") {
        Some(Json::Arr(p)) => p.clone(),
        other => panic!("no roster: {other:?}"),
    };
    assert_eq!(players.len(), 4);
    let row = |name: &str| {
        players
            .iter()
            .find(|p| str_of(p, "name").starts_with(name))
            .cloned()
            .unwrap_or_else(|| panic!("{name} on the roster"))
    };
    let w = row("Brakkar");
    assert_eq!(u64_of(&w, "support_received"), 14_750);
    assert_eq!(f64_of(&w, "effective_dps"), 3787.5);
    assert_eq!(u64_of(&w, "healed_received"), 50_000, "the NPC heal counts");
    assert_eq!(u64_of(&w, "self_healed"), 0);
    assert_eq!(w.get("support"), Some(&Json::Bool(false)));
    let e = row("Vessyra");
    assert_eq!(u64_of(&e, "support_given"), 23_900);
    assert_eq!(e.get("support"), Some(&Json::Bool(true)));
    assert_eq!(e.get("me"), Some(&Json::Bool(true)));
    let h = row("Seraph");
    assert_eq!(u64_of(&h, "overheal"), 16_000);
    assert_eq!(u64_of(&h, "absorbed"), 15_000);
    // v25 (review S2): every roster row carries the three span keys too —
    // support.txt has no external and no active mitigation, so all zeros.
    for p in &players {
        assert_eq!(f64_of(p, "am_uptime_pct"), 0.0, "{p:?}");
        for key in ["externals_given", "externals_received"] {
            let e = p.get(key).unwrap_or_else(|| panic!("{key} on {p:?}"));
            assert_eq!(e.get("count").and_then(Json::as_u64), Some(0));
            assert_eq!(e.get("secs").and_then(Json::as_f64), Some(0.0));
        }
    }
    // The peer row is the `me` shape: graded by effective, second of three.
    let peer = fights(&tool_doc(&reply[1]))[0]
        .get("peer")
        .cloned()
        .expect("peer");
    assert!(str_of(&peer, "name").starts_with("Brakkar"));
    assert_eq!(str_of(&peer, "rank_measure"), "effective_dps");
    assert_eq!(peer.get("rank").and_then(Json::as_u64), Some(2));
    assert_eq!(f64_of(&peer, "effective_dps"), 3787.5);
    assert_eq!(f64_of(&peer, "dps"), 4033.3);
    assert_eq!(u64_of(&peer, "support_received"), 14_750);
}

#[test]
fn stored_fight_drills_a_supporters_targets() {
    let (_tmp, mut bridge) = support_daemon("support-stored", None);
    let reply = drive(&mut bridge, &[&call_line(2, "history", "{}")]);
    let id = str_of(&fights(&tool_doc(&reply[0]))[0], "id").to_string();
    let reply = drive(
        &mut bridge,
        &[
            &call_line(
                3,
                "stored_fight",
                &format!(r#"{{"fight_id":"{id}","player":"Vessyra"}}"#),
            ),
            &call_line(
                4,
                "stored_fight",
                &format!(r#"{{"fight_id":"{id}","player":"Brakkar"}}"#),
            ),
            &call_line(
                5,
                "stored_fight",
                &format!(r#"{{"fight_id":"{id}","player":"Seraphíne"}}"#),
            ),
            &call_line(6, "stored_fight", &format!(r#"{{"fight_id":"{id}"}}"#)),
        ],
    );
    for r in &reply {
        assert!(!is_error(r), "{r:?}");
    }
    // The supporter: everything given, the one self-supported proc
    // received, and every buffed player as a target — herself included.
    let doc = tool_doc(&reply[0]);
    let s = doc.get("support").expect("the Evoker has a support block");
    assert_eq!(u64_of(s.get("given").unwrap(), "damage"), 23_900);
    assert_eq!(u64_of(s.get("given").unwrap(), "healing"), 2_100);
    assert_eq!(u64_of(s.get("received").unwrap(), "damage"), 7_500);
    assert_eq!(u64_of(s.get("received").unwrap(), "healing"), 0);
    let targets = match s.get("targets") {
        Some(Json::Arr(t)) => t.clone(),
        other => panic!("targets: {other:?}"),
    };
    let target = |name: &str| {
        targets
            .iter()
            .find(|t| str_of(t, "name").starts_with(name))
            .cloned()
            .unwrap_or_else(|| panic!("{name} among {targets:?}"))
    };
    let m = target("Ignatia");
    assert_eq!(u64_of(&m, "damage"), 1_650);
    assert_eq!(u64_of(&m, "healing"), 0);
    assert_eq!(u64_of(&m, "lines"), 5);
    assert_eq!(str_of(&m, "spec"), "Fire");
    let w = target("Brakkar");
    assert_eq!(u64_of(&w, "damage"), 14_750);
    assert_eq!(u64_of(&w, "lines"), 5);
    // A heal share is keyed on the heal's SOURCE like a damage share on the
    // hit's: the Fate Mirror line (l.39) is the Priest's Flash Heal on the
    // Warrior, so its 2 000 is a share of HER healing — `check.awk` says so
    // too (support_received_heal: the Priest 2 100, the Warrior 0).
    assert_eq!(u64_of(&w, "healing"), 0);
    let e = target("Vessyra");
    assert_eq!(u64_of(&e, "damage"), 7_500);
    assert_eq!(u64_of(&e, "lines"), 1, "the self-supported proc");
    let h = target("Seraph");
    assert_eq!(u64_of(&h, "damage"), 0);
    assert_eq!(
        u64_of(&h, "healing"),
        2_100,
        "Fate Mirror 2 000 + Shifting Sands 100"
    );
    let sum: u64 = targets.iter().map(|t| u64_of(t, "damage")).sum();
    assert_eq!(sum, 23_900, "the targets partition what was given");
    let sum: u64 = targets.iter().map(|t| u64_of(t, "healing")).sum();
    assert_eq!(sum, 2_100);
    // A buffed player: received only, no targets.
    let doc = tool_doc(&reply[1]);
    let s = doc.get("support").expect("Brakkar received support");
    assert_eq!(u64_of(s.get("given").unwrap(), "damage"), 0);
    assert_eq!(u64_of(s.get("given").unwrap(), "healing"), 0);
    assert_eq!(u64_of(s.get("received").unwrap(), "damage"), 14_750);
    assert_eq!(u64_of(s.get("received").unwrap(), "healing"), 0);
    assert!(matches!(s.get("targets"), Some(Json::Arr(t)) if t.is_empty()));
    // The healer's block: the heal shares of her own heals, nothing else.
    let doc = tool_doc(&reply[2]);
    let s = doc.get("support").expect("the Priest's heal shares");
    assert_eq!(u64_of(s.get("received").unwrap(), "healing"), 2_100);
    assert_eq!(u64_of(s.get("received").unwrap(), "damage"), 0);
    assert_eq!(u64_of(s.get("given").unwrap(), "damage"), 0);
    assert!(matches!(s.get("targets"), Some(Json::Arr(t)) if t.is_empty()));
    // No drill, no block.
    assert_eq!(tool_doc(&reply[3]).get("support"), None);
}

#[test]
fn trend_defaults_a_dps_player_to_effective_dps_and_keeps_raw_dps() {
    let (_tmp, mut bridge) = support_daemon("support-trend", None);
    let one = |doc: &Json, key: &str| -> f64 {
        match doc.get("points") {
            Some(Json::Arr(p)) if p.len() == 1 => num_of(&p[0], key),
            other => panic!("{other:?}"),
        }
    };
    let reply = drive(
        &mut bridge,
        &[
            &call_line(2, "trend", r#"{"player":"Ignatia"}"#),
            &call_line(3, "trend", r#"{"player":"Ignatia","measure":"dps"}"#),
            &call_line(
                4,
                "trend",
                r#"{"player":"Vessyra","measure":"effective_dps"}"#,
            ),
            &call_line(5, "trend", r#"{"player":"Ignatia","view":"damage"}"#),
            &call_line(6, "trend", r#"{"player":"Seraphíne"}"#),
        ],
    );
    for r in &reply {
        assert!(!is_error(r), "{r:?}");
    }
    // A DPS player's default is effective: the Mage's 269 350 over 60 s,
    // named by the measure and under the `per_sec` alias alike.
    let eff = tool_doc(&reply[0]);
    assert_eq!(str_of(&eff, "measure"), "effective_dps");
    assert_eq!(one(&eff, "effective_dps"), 4489.2);
    assert_eq!(one(&eff, "per_sec"), 4489.2);
    assert_eq!(one(&eff, "amount"), 269_350.0);
    // Raw dps stays reachable by name…
    let raw = tool_doc(&reply[1]);
    assert_eq!(str_of(&raw, "measure"), "dps");
    assert_eq!(one(&raw, "dps"), 4516.7);
    assert_eq!(one(&raw, "amount"), 271_000.0);
    // …the Evoker's effective line is her contribution…
    let evoker = tool_doc(&reply[2]);
    assert_eq!(one(&evoker, "effective_dps"), 1431.7);
    assert_eq!(one(&evoker, "amount"), 85_900.0);
    // …the deprecated view alias still means raw dps, and a healer's default
    // is untouched.
    assert_eq!(str_of(&tool_doc(&reply[3]), "measure"), "dps");
    assert_eq!(str_of(&tool_doc(&reply[4]), "measure"), "hps");
}
