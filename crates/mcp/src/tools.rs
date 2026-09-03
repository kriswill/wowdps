//! The MCP tool surface: what an LLM can ask the meter. Every tool answers
//! with one JSON document in a text content block — compact, self-labeled,
//! and stable in shape, so a harness can reason over fights without ever
//! seeing the wire protocol.

use crate::bridge::Bridge;
use crate::json::Json;
use crate::obj;

use wowdps_model::{
    GearItem, Loadout, Mark, Row, SegmentId, SegmentInfo, SegmentKind, Spec, Timeline, View,
};
use wowdps_proto::history::{FightCard, FightKind};
use wowdps_proto::{
    Cursor, FightSort, HistoryAnswer, HistoryQuery, ListEntry, OverlayState, SegmentRef,
    TrendBucket,
};

/// The DPS curve resolution in tool output: coarse enough to stay small,
/// fine enough to show a cooldown window or a death's dent.
const CURVE_BUCKET_MS: u32 = 10_000;

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema for `arguments`, as a serialized object.
    pub schema: Json,
}

pub fn catalog() -> Vec<Tool> {
    let segment_id = || {
        obj! {
            "type": Json::str("integer"),
            "description": Json::str(
                "Fight id from list_fights. Omit for the live/most recent fight.",
            ),
        }
    };
    let view = || {
        obj! {
            "type": Json::str("string"),
            "enum": Json::Arr(
                ["damage", "healing", "interrupts", "crowd_control", "dispels", "deaths"]
                    .iter().map(|s| Json::str(*s)).collect(),
            ),
            "description": Json::str("Which meter to read. Default: damage."),
        }
    };
    let player = |what: &str| {
        obj! {
            "type": Json::str("string"),
            "description": Json::str(format!(
                "{what}: a player name (as shown in fight rows) or GUID key."
            )),
        }
    };
    let mut tools = vec![
        Tool {
            name: "status",
            description: "Daemon and game state: what log is followed, whether the game is \
                          running, and whether a fight is happening right now.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {},
            },
        },
        Tool {
            name: "list_fights",
            description: "Every fight the log holds: encounters (kill/wipe), arena matches \
                          (win/loss), trash stretches, and per-visit Overall totals for \
                          dungeons/raids (keystone runs include their par timers). Returns \
                          ids for the other tools.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {},
            },
        },
        Tool {
            name: "fight",
            description: "One fight's meter: per-player totals, per-second rates, activity \
                          share and crit rate for the chosen view. The place to start for \
                          performance questions.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "segment_id": segment_id(),
                    "view": view(),
                    "top": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Only the top N rows. Default: all."),
                    },
                },
            },
        },
        Tool {
            name: "breakdown",
            description: "One player's fight in depth: per-ability rows (hits, crit rate, \
                          average hit), per-target rows, and a DPS curve over the fight with \
                          trinket uses/procs and consumables marked on it. With view=deaths \
                          the per-ability rows are that player's death recap (R9): the last \
                          hits they took, with remaining health after each.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "segment_id": segment_id(),
                    "player": player("The player to drill into"),
                    "view": view(),
                },
                "required": Json::Arr(vec![Json::str("player")]),
            },
        },
        Tool {
            name: "history",
            description: "Stored fights from past sessions (the history store keeps every \
                          raid boss, arena match and keystone run's overall — with every \
                          raid member's name and build from your own combat log — across \
                          logins). Filter by encounter id / difficulty / player / kind / \
                          since; sort newest, fastest (kills only — fastest with limit 1 \
                          is the best kill) or by the owner's DPS. Ids here are stable \
                          fight ids (strings), not list_fights' per-run integers.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "encounter": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("ENCOUNTER_START encounter id."),
                    },
                    "difficulty": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Difficulty id (e.g. 14 Normal, 15 Heroic, 16 Mythic raids)."),
                    },
                    "player": player("Only fights this player was in"),
                    "kind": obj! {
                        "type": Json::str("string"),
                        "enum": Json::Arr(
                            ["encounter", "arena", "key", "overall", "trash"]
                                .iter().map(|s| Json::str(*s)).collect(),
                        ),
                    },
                    "since_utc_ms": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Only fights starting at or after this UTC epoch ms."),
                    },
                    "sort": obj! {
                        "type": Json::str("string"),
                        "enum": Json::Arr(
                            ["newest", "fastest", "owner_per_sec"]
                                .iter().map(|s| Json::str(*s)).collect(),
                        ),
                        "description": Json::str("Default: newest."),
                    },
                    "limit": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Max fights returned. Default 50."),
                    },
                },
            },
        },
        Tool {
            name: "progression",
            description: "Pulls-to-kill progression on one boss + difficulty from the \
                          history store: total pulls, kills, the first kill, a per-night \
                          breakdown and the median kill time.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "encounter": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("ENCOUNTER_START encounter id."),
                    },
                    "difficulty": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Difficulty id."),
                    },
                },
                "required": Json::Arr(vec![Json::str("encounter"), Json::str("difficulty")]),
            },
        },
        Tool {
            name: "trend",
            description: "One player's damage or healing per second over time from the \
                          history store — one point per fight, or per UTC day / week \
                          (per_sec averaged, amounts summed). Scope with spec, encounter \
                          and difficulty; since_utc_ms scopes to a game build's era.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "player": player("Whose trend"),
                    "spec": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Blizzard spec id; omit for every spec."),
                    },
                    "encounter": obj! { "type": Json::str("integer") },
                    "difficulty": obj! { "type": Json::str("integer") },
                    "view": obj! {
                        "type": Json::str("string"),
                        "enum": Json::Arr(vec![Json::str("damage"), Json::str("healing")]),
                        "description": Json::str("Default: damage."),
                    },
                    "bucket": obj! {
                        "type": Json::str("string"),
                        "enum": Json::Arr(vec![Json::str("none"), Json::str("day"), Json::str("week")]),
                        "description": Json::str("Default: none (one point per fight)."),
                    },
                    "since_utc_ms": obj! { "type": Json::str("integer") },
                    "limit": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("Max points. Default 50."),
                    },
                },
                "required": Json::Arr(vec![Json::str("player")]),
            },
        },
        Tool {
            name: "stored_fight",
            description: "One stored fight by its history fight id: the same rows `fight` \
                          returns for a live fight, and with `player` the same breakdown \
                          `breakdown` returns (from the details tier — kills, bests and \
                          pinned fights keep it; the death recap for view deaths).",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "fight_id": obj! {
                        "type": Json::str("string"),
                        "description": Json::str("A fight id from `history`."),
                    },
                    "view": view(),
                    "player": player("Drill into this player"),
                },
                "required": Json::Arr(vec![Json::str("fight_id")]),
            },
        },
        Tool {
            name: "pin_fight",
            description: "Protect a stored fight from retention (or release it). Pinned \
                          fights keep their details tier forever.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "fight_id": obj! { "type": Json::str("string") },
                    "pinned": obj! {
                        "type": Json::str("boolean"),
                        "description": Json::str("Default true."),
                    },
                },
                "required": Json::Arr(vec![Json::str("fight_id")]),
            },
        },
        Tool {
            name: "loadout",
            description: "One player's actual build as the combat log recorded it \
                          (COMBATANT_INFO): spec, talents and equipped gear with item \
                          levels, enchants, gems and bonus ids. Talents come named \
                          through the local talent dataset with an in-game import \
                          string when the dataset knows the spec, raw \
                          node/entry/rank picks otherwise (rank 0 = a granted node). \
                          The game logs a build only inside instances (raids, \
                          dungeons, arenas); the answer is the latest one logged \
                          at or before this fight, so an open-world fight after \
                          an instance carries that instance's build (stale if the \
                          player respecced or regeared since), and logged: false \
                          means none has fired yet in this log.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "segment_id": segment_id(),
                    "player": player("The player whose build to fetch"),
                },
                "required": Json::Arr(vec![Json::str("player")]),
            },
        },
        Tool {
            name: "talent_tree",
            description: "One spec's talent tree from the local game data: nodes with \
                          positions, ranks, choice entries, spell ids/names/icons, hero \
                          subtrees, point gates, and the node walk order of the in-game \
                          import string. Needs the per-machine dataset from \
                          tools/gen-talent-trees.sh.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "spec_id": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str(
                            "ChrSpecialization id (e.g. 266 = Demonology Warlock).",
                        ),
                    },
                },
                "required": Json::Arr(vec![Json::str("spec_id")]),
            },
        },
        Tool {
            name: "decode_talents",
            description: "Decode an in-game talent import/export string (the one from the \
                          talent pane or a SimC export's talents= line) into the chosen \
                          spec, hero tree, and every selected node with ranks and choice \
                          picks, named from local game data.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "string": obj! {
                        "type": Json::str("string"),
                        "description": Json::str("The talent import string."),
                    },
                },
                "required": Json::Arr(vec![Json::str("string")]),
            },
        },
        Tool {
            name: "encode_talents",
            description: "Build an in-game talent import string from a spec id and a list \
                          of node selections ({node_id, ranks?, choice_index?}); ranks \
                          default to the node's maximum. The game client validates the \
                          result on import.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "spec_id": obj! {
                        "type": Json::str("integer"),
                        "description": Json::str("ChrSpecialization id of the loadout."),
                    },
                    "selections": obj! {
                        "type": Json::str("array"),
                        "description": Json::str(
                            "Selected nodes: [{node_id, ranks?, choice_index?}].",
                        ),
                        "items": obj! {
                            "type": Json::str("object"),
                            "properties": obj! {
                                "node_id": obj! { "type": Json::str("integer") },
                                "ranks": obj! { "type": Json::str("integer") },
                                "choice_index": obj! { "type": Json::str("integer") },
                            },
                            "required": Json::Arr(vec![Json::str("node_id")]),
                        },
                    },
                },
                "required": Json::Arr(vec![Json::str("spec_id"), Json::str("selections")]),
            },
        },
        Tool {
            name: "compare",
            description: "Two players of one fight side by side: totals, per-ability tables \
                          and both DPS curves on one clock — the tool for 'why is their \
                          number bigger than mine'.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "segment_id": segment_id(),
                    "a": player("One side"),
                    "b": player("The other side"),
                },
                "required": Json::Arr(vec![Json::str("a"), Json::str("b")]),
            },
        },
    ];
    // Ad hoc SQL exists only where the `wowdps-history` binary does: an
    // absent binary means no tool, not a tool that always fails.
    if history_bin().is_some() {
        tools.push(Tool {
            name: "history_sql",
            description: "Ad hoc SQL (DuckDB) over the history store's files, via the \
                          wowdps-history binary. Views: fights (one row per stored fight: \
                          id, kind, name, encounter{id,difficulty,group_size}, key, \
                          start_utc_ms, duration_ms, success, aborted, build, owner, \
                          pinned, players[]), players (one row per player per fight: \
                          fight_id, encounter_id, difficulty, guid, name, class, spec, \
                          damage, dps, healing, hps, deaths, enemy), rows (the six views' \
                          meter rows + death recaps), details (breakdowns + timelines for \
                          kills/bests/pins), loadouts, annotations. Read-only, offline; \
                          returns {columns, rows}.",
            schema: obj! {
                "type": Json::str("object"),
                "properties": obj! {
                    "query": obj! {
                        "type": Json::str("string"),
                        "description": Json::str("A DuckDB SQL statement over the views above."),
                    },
                },
                "required": Json::Arr(vec![Json::str("query")]),
            },
        });
    }
    tools
}

/// Run one tool. `Err` is a tool-level failure (bad args, no such fight) —
/// reported to the harness as `isError`, never as a protocol fault.
pub fn call(bridge: &mut Bridge, name: &str, args: &Json) -> Result<Json, String> {
    match name {
        "status" => status(bridge),
        "list_fights" => list_fights(bridge),
        "fight" => fight(bridge, args),
        "breakdown" => breakdown(bridge, args),
        "loadout" => loadout(bridge, args),
        "compare" => compare(bridge, args),
        // v20: the history store's fixed questions.
        "history" => history(bridge, args),
        "progression" => progression(bridge, args),
        "trend" => trend(bridge, args),
        "stored_fight" => stored_fight(bridge, args),
        "pin_fight" => pin_fight(bridge, args),
        "history_sql" => history_sql(args),
        // The talent tools read the per-machine dataset, never the daemon.
        "talent_tree" => crate::talents::tree_view(crate::talents::load()?, arg_spec_id(args)?),
        "decode_talents" => {
            let string = args
                .get("string")
                .and_then(Json::as_str)
                .ok_or("decode_talents requires a string")?;
            crate::talents::decode(crate::talents::load()?, string)
        }
        "encode_talents" => {
            let selections = match args.get("selections") {
                Some(Json::Arr(s)) => s.clone(),
                _ => return Err("encode_talents requires a selections array".into()),
            };
            crate::talents::encode(crate::talents::load()?, arg_spec_id(args)?, &selections)
        }
        other => Err(format!("no such tool {other:?}")),
    }
}

// ---- the tools --------------------------------------------------------------

fn status(bridge: &mut Bridge) -> Result<Json, String> {
    let s = bridge.status()?;
    let (_, active, _) = bridge.segments()?;
    Ok(obj! {
        "daemon": Json::str("running"),
        "source": opt_str(s.source),
        "game_running": Json::Bool(s.game_running),
        "fight_active": Json::Bool(active),
        "clients": Json::u64(s.clients as u64),
        "overlay": Json::str(match s.overlay {
            OverlayState::Absent => "absent".to_string(),
            OverlayState::Visible => "visible".to_string(),
            OverlayState::Hidden => "hidden".to_string(),
            OverlayState::Failed(e) => format!("failed: {e}"),
        }),
        "history": obj! {
            "enabled": Json::Bool(s.history.enabled),
            "fights": Json::u64(u64::from(s.history.fights)),
            "importing": Json::u64(u64::from(s.history.importing)),
            "dropped": Json::u64(u64::from(s.history.dropped)),
            "owner_inferred": Json::Bool(s.history.owner_inferred),
            "error": opt_str(s.history.error),
        },
    })
}

// ---- v20: the history store -----------------------------------------------------

fn arg_u32(args: &Json, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Json::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn arg_i64(args: &Json, key: &str) -> Option<i64> {
    args.get(key).and_then(Json::as_i64)
}

/// A `player` argument as a guid: a "Player-…" key passes through; a name
/// is looked up among the store's cards (case-insensitive).
fn history_guid(bridge: &mut Bridge, args: &Json, key: &str) -> Result<Option<String>, String> {
    let Some(who) = args.get(key).and_then(Json::as_str) else {
        return Ok(None);
    };
    if who.starts_with("Player-") {
        return Ok(Some(who.to_string()));
    }
    let cards = match bridge.history(HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: None,
        since_utc_ms: None,
        kind: None,
        sort: FightSort::Newest,
        limit: 500,
    })? {
        HistoryAnswer::Fights(cards) => cards,
        _ => Vec::new(),
    };
    let want = who.to_lowercase();
    cards
        .iter()
        .flat_map(|c| c.players.iter())
        .find(|p| {
            p.name.to_lowercase() == want || p.name.to_lowercase().starts_with(&format!("{want}-"))
        })
        .map(|p| Some(p.guid.clone()))
        .ok_or_else(|| format!("no stored fight has a player named {who:?}"))
}

fn history(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let guid = history_guid(bridge, args, "player")?;
    let kind = match args.get("kind").and_then(Json::as_str) {
        None => None,
        Some(k) => {
            Some(FightKind::parse(&k.to_lowercase()).ok_or_else(|| format!("unknown kind {k:?}"))?)
        }
    };
    let sort = match args
        .get("sort")
        .and_then(Json::as_str)
        .map(str::to_lowercase)
        .as_deref()
    {
        None | Some("newest") => FightSort::Newest,
        Some("fastest") => FightSort::Fastest,
        Some("owner_per_sec") => FightSort::OwnerPerSec,
        Some(other) => return Err(format!("unknown sort {other:?}")),
    };
    let answer = bridge.history(HistoryQuery::Fights {
        encounter: arg_u32(args, "encounter"),
        difficulty: arg_u32(args, "difficulty"),
        guid,
        since_utc_ms: arg_i64(args, "since_utc_ms"),
        kind,
        sort,
        limit: arg_u32(args, "limit").unwrap_or(0),
    })?;
    let HistoryAnswer::Fights(cards) = answer else {
        return Err("unexpected answer".to_string());
    };
    Ok(obj! {
        "count": Json::u64(cards.len() as u64),
        "fights": Json::Arr(cards.iter().map(card_json).collect()),
    })
}

fn progression(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let encounter = arg_u32(args, "encounter").ok_or("progression requires encounter")?;
    let difficulty = arg_u32(args, "difficulty").ok_or("progression requires difficulty")?;
    let answer = bridge.history(HistoryQuery::Progression {
        encounter,
        difficulty,
    })?;
    let HistoryAnswer::Progression {
        pulls,
        kills,
        first_kill,
        nights,
        median_kill_ms,
    } = answer
    else {
        return Err("unexpected answer".to_string());
    };
    Ok(obj! {
        "encounter": Json::u64(u64::from(encounter)),
        "difficulty": Json::u64(u64::from(difficulty)),
        "pulls": Json::u64(u64::from(pulls)),
        "kills": Json::u64(u64::from(kills)),
        "first_kill": first_kill.as_deref().map_or(Json::Null, card_json),
        "median_kill": median_kill_ms.map_or(Json::Null, |ms| Json::str(wowdps_model::fmt::duration(ms))),
        "median_kill_ms": median_kill_ms.map_or(Json::Null, |ms| Json::num(ms as f64)),
        "nights": Json::Arr(nights.iter().map(|n| obj! {
            "date": Json::str(utc_date(n.day_utc_ms)),
            "day_utc_ms": Json::num(n.day_utc_ms as f64),
            "pulls": Json::u64(u64::from(n.pulls)),
            "kill": Json::Bool(n.kill),
            "best_pct": n.best_pct.map_or(Json::Null, |p| Json::u64(u64::from(p))),
        }).collect()),
    })
}

fn trend(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let guid = history_guid(bridge, args, "player")?.ok_or("trend requires player")?;
    let view = match arg_view(args)? {
        View::Healing => View::Healing,
        _ => View::Damage,
    };
    let bucket = match args
        .get("bucket")
        .and_then(Json::as_str)
        .map(str::to_lowercase)
        .as_deref()
    {
        None | Some("none") => TrendBucket::None,
        Some("day") => TrendBucket::Day,
        Some("week") => TrendBucket::Week,
        Some(other) => return Err(format!("unknown bucket {other:?}")),
    };
    let answer = bridge.history(HistoryQuery::Trend {
        guid: guid.clone(),
        spec: arg_u32(args, "spec"),
        encounter: arg_u32(args, "encounter"),
        difficulty: arg_u32(args, "difficulty"),
        view,
        bucket,
        since_utc_ms: arg_i64(args, "since_utc_ms"),
        limit: arg_u32(args, "limit").unwrap_or(0),
    })?;
    let HistoryAnswer::Trend(points) = answer else {
        return Err("unexpected answer".to_string());
    };
    Ok(obj! {
        "player": Json::str(guid),
        "view": Json::str(if view == View::Healing { "healing" } else { "damage" }),
        "points": Json::Arr(points.iter().map(|p| obj! {
            "date": Json::str(utc_date(p.bucket_utc_ms)),
            "bucket_utc_ms": Json::num(p.bucket_utc_ms as f64),
            "fight_id": Json::str(p.fight_id.clone()),
            "spec": p.spec.and_then(Spec::from_id).map_or(Json::Null, |s| Json::str(s.name())),
            "amount": Json::u64(p.amount),
            "per_sec": Json::num(round1(p.per_sec)),
            "duration_ms": Json::num(p.duration_ms as f64),
            "fights": Json::u64(u64::from(p.n)),
        }).collect()),
    })
}

fn stored_fight(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let fight_id = args
        .get("fight_id")
        .and_then(Json::as_str)
        .ok_or("stored_fight requires fight_id")?
        .to_string();
    let view = arg_view(args)?;
    // Resolve the drill against the fight's own players, not the whole store.
    let drill = match args.get("player").and_then(Json::as_str) {
        None => None,
        Some(who) if who.starts_with("Player-") => Some(who.to_string()),
        Some(who) => {
            let Some(f) = bridge.stored_fight(fight_id.clone(), view, None)? else {
                return Ok(not_stored(&fight_id));
            };
            let want = who.to_lowercase();
            let guid = f
                .card
                .players
                .iter()
                .find(|p| {
                    p.name.to_lowercase() == want
                        || p.name.to_lowercase().starts_with(&format!("{want}-"))
                })
                .map(|p| p.guid.clone())
                .ok_or_else(|| format!("no player named {who:?} in that fight"))?;
            Some(guid)
        }
    };
    let Some(f) = bridge.stored_fight(fight_id.clone(), view, drill.clone())? else {
        return Ok(not_stored(&fight_id));
    };
    let mut o = vec![
        ("fight".to_string(), card_json(&f.card)),
        ("view".to_string(), Json::str(view_name(view))),
        (
            "rows".to_string(),
            Json::Arr(
                f.rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| meter_row(i, r, view, f.card.duration_ms))
                    .collect(),
            ),
        ),
    ];
    if let Some(guid) = drill {
        let player = f
            .rows
            .iter()
            .find(|r| r.key == guid)
            .map(player_ident)
            .unwrap_or_else(|| obj! { "key": Json::str(guid.clone()) });
        o.push(("player".to_string(), player));
        match f.breakdown {
            Some(b) => {
                let (spells_key, targets_key) = if view == View::Deaths {
                    ("death_recap", "attackers")
                } else {
                    ("by_ability", "by_target")
                };
                o.push((
                    spells_key.to_string(),
                    Json::Arr(b.by_spell.iter().map(|r| ability_row(r, view)).collect()),
                ));
                o.push((
                    targets_key.to_string(),
                    Json::Arr(b.by_target.iter().map(|r| ability_row(r, view)).collect()),
                ));
                if let Some(tl) = &b.timeline {
                    o.push(("timeline".to_string(), timeline_json(tl)));
                }
            }
            None => o.push((
                "note".to_string(),
                Json::str(
                    "no breakdown stored for this fight/view: the details tier is kept for \
                     kills, bests and pinned fights (damage and healing), and the death \
                     recap only for players who died",
                ),
            )),
        }
    }
    Ok(Json::Obj(o))
}

fn pin_fight(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let fight_id = args
        .get("fight_id")
        .and_then(Json::as_str)
        .ok_or("pin_fight requires fight_id")?
        .to_string();
    let pinned = args.get("pinned").and_then(Json::as_bool).unwrap_or(true);
    let now = bridge.pin_fight(fight_id.clone(), pinned)?;
    Ok(obj! {
        "fight_id": Json::str(fight_id),
        "pinned": Json::Bool(now),
    })
}

/// The `wowdps-history` binary, if this machine has one: `$WOWDPS_HISTORY_BIN`,
/// else a sibling of this binary, else on `$PATH`. Absent = the
/// `history_sql` tool is not registered at all.
pub fn history_bin() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("WOWDPS_HISTORY_BIN") {
        let p = std::path::PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("wowdps-history")))
        .filter(|p| p.is_file());
    sibling.or_else(|| {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join("wowdps-history"))
                .find(|p| p.is_file())
        })
    })
}

/// Ad hoc SQL over the lake, by shelling out to `wowdps-history sql --json`
/// — DuckDB never links into this stdlib-only process.
fn history_sql(args: &Json) -> Result<Json, String> {
    let query = args
        .get("query")
        .and_then(Json::as_str)
        .ok_or("history_sql requires a query")?;
    let bin = history_bin().ok_or("wowdps-history is not installed on this machine")?;
    let out = std::process::Command::new(&bin)
        .args(["sql", query, "--json"])
        .output()
        .map_err(|e| format!("cannot run {}: {e}", bin.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "wowdps-history: {}",
            err.trim().trim_start_matches("wowdps-history: ")
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    crate::json::parse(text.trim()).map_err(|e| format!("wowdps-history output: {e}"))
}

fn not_stored(fight_id: &str) -> Json {
    obj! {
        "fight_id": Json::str(fight_id),
        "stored": Json::Bool(false),
        "note": Json::str("no such stored fight — evicted by retention, or never stored"),
    }
}

fn view_name(view: View) -> &'static str {
    match view {
        View::Damage => "damage",
        View::Healing => "healing",
        View::Interrupts => "interrupts",
        View::CrowdControl => "crowd_control",
        View::Dispels => "dispels",
        View::Deaths => "deaths",
    }
}

/// A fight card, reshaped for a reader: dates spelled out, names next to ids.
fn card_json(c: &FightCard) -> Json {
    obj! {
        "id": Json::str(c.id.clone()),
        "kind": Json::str(c.kind.as_str()),
        "name": Json::str(c.name.clone()),
        "encounter": c.encounter.map_or(Json::Null, |e| obj! {
            "id": Json::u64(u64::from(e.id)),
            "difficulty": Json::u64(u64::from(e.difficulty)),
            "group_size": Json::u64(u64::from(e.group_size)),
        }),
        "instance": c.key.as_ref().map_or(Json::Null, |k| obj! {
            "map_id": Json::u64(u64::from(k.map_id)),
            "difficulty": Json::u64(u64::from(k.difficulty)),
            "key_level": k.level.map_or(Json::Null, |l| Json::u64(u64::from(l))),
            "completed": k.completed.map_or(Json::Null, Json::Bool),
        }),
        "date": Json::str(utc_datetime(c.start_utc_ms)),
        "start_utc_ms": Json::num(c.start_utc_ms as f64),
        "duration": Json::str(wowdps_model::fmt::duration(c.duration_ms)),
        "duration_ms": Json::num(c.duration_ms as f64),
        "official_ms": c.official_ms.map_or(Json::Null, |m| Json::num(m as f64)),
        "result": if c.aborted {
            Json::str("aborted")
        } else {
            result_name(c.success, c.kind == FightKind::Arena)
        },
        "build": Json::str(format!("{}.{}.{}", c.build.0, c.build.1, c.build.2)),
        "owner": opt_str(c.owner.clone()),
        "pinned": Json::Bool(c.pinned),
        "best_pct": c.best_pct.map_or(Json::Null, |p| Json::u64(u64::from(p))),
        "players": Json::Arr(c.players.iter().map(|p| obj! {
            "name": Json::str(p.name.clone()),
            "key": Json::str(p.guid.clone()),
            "class": p.class.map_or(Json::Null, |c| Json::str(format!("{c:?}"))),
            "spec": p.spec.map_or(Json::Null, |s| Json::str(s.name())),
            "damage": Json::u64(p.damage),
            "dps": Json::num(round1(p.dps)),
            "healing": Json::u64(p.healing),
            "hps": Json::num(round1(p.hps)),
            "deaths": Json::u64(u64::from(p.deaths)),
            "enemy": Json::Bool(p.enemy),
        }).collect()),
    }
}

/// `YYYY-MM-DD` of a UTC epoch (civil-from-days, Howard Hinnant's algorithm).
fn utc_date(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(86_400_000));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DD HH:MM UTC`.
fn utc_datetime(ms: i64) -> String {
    let day_ms = ms.rem_euclid(86_400_000);
    format!(
        "{} {:02}:{:02} UTC",
        utc_date(ms),
        day_ms / 3_600_000,
        (day_ms / 60_000) % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn list_fights(bridge: &mut Bridge) -> Result<Json, String> {
    let (entries, active, source) = bridge.segments()?;
    let fights = entries
        .iter()
        .map(|ListEntry { id, row }| {
            let mut o = vec![
                ("id".to_string(), Json::u64(id.0)),
                ("kind".to_string(), Json::str(kind_name(row.kind))),
                ("name".to_string(), Json::str(row.name.clone())),
                (
                    "duration".to_string(),
                    Json::str(wowdps_model::fmt::duration(row.duration_ms)),
                ),
                ("duration_ms".to_string(), Json::num(row.duration_ms as f64)),
                ("result".to_string(), result_name(row.success, row.arena)),
            ];
            if row.live {
                o.push(("live".to_string(), Json::Bool(true)));
            }
            if let Some(visit) = row.instance {
                o.push(("visit".to_string(), Json::u64(visit as u64)));
            }
            if let Some((par, plus2, plus3)) = row.pars_ms {
                o.push((
                    "keystone_pars_ms".to_string(),
                    Json::Arr(vec![
                        Json::num(par as f64),
                        Json::num(plus2 as f64),
                        Json::num(plus3 as f64),
                    ]),
                ));
            }
            Json::Obj(o)
        })
        .collect();
    Ok(obj! {
        "source": opt_str(source),
        "fight_active": Json::Bool(active),
        "fights": Json::Arr(fights),
    })
}

fn fight(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let segment = arg_segment(args);
    let view = arg_view(args)?;
    let top_n = args
        .get("top")
        .and_then(Json::as_u64)
        .map(|n| n.min(u32::MAX as u64) as u32);
    let snap = bridge.snapshot(Cursor::Segment {
        segment,
        view,
        top_n,
        drill: None,
        spell: None,
    })?;
    let rows = snap
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| meter_row(i, r, view, snap.info.duration_ms))
        .collect();
    Ok(obj! {
        "fight": fight_info(snap.id, &snap.info),
        "view": Json::str(wowdps_model::fmt::view_name(view)),
        "rows": Json::Arr(rows),
        "total_rows": Json::u64(snap.total_rows as u64),
    })
}

fn breakdown(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let segment = arg_segment(args);
    let view = arg_view(args)?;
    let (segment, key, row) = resolve_player(bridge, segment, view, args, "player")?;
    let snap = bridge.snapshot(Cursor::Segment {
        segment,
        view,
        top_n: None,
        drill: Some(key),
        spell: None,
    })?;
    let bd = snap
        .breakdown
        .ok_or("daemon sent no breakdown for the drilled player")?;
    let mut out = vec![
        ("fight".to_string(), fight_info(snap.id, &snap.info)),
        (
            "view".to_string(),
            Json::str(wowdps_model::fmt::view_name(view)),
        ),
        ("player".to_string(), player_ident(&row)),
        (
            if view == View::Deaths {
                "death_recap".to_string()
            } else {
                "by_ability".to_string()
            },
            Json::Arr(bd.by_spell.iter().map(|r| ability_row(r, view)).collect()),
        ),
        (
            "by_target".to_string(),
            Json::Arr(bd.by_target.iter().map(|r| ability_row(r, view)).collect()),
        ),
    ];
    if let Some(tl) = &bd.timeline {
        out.push(("timeline".to_string(), timeline_json(tl)));
    }
    Ok(Json::Obj(out))
}

fn compare(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let segment = arg_segment(args);
    // Compare is damage-only (R12); resolve names against the damage meter.
    let (segment, a_key, _) = resolve_player(bridge, segment, View::Damage, args, "a")?;
    let (_, b_key, _) = resolve_player(bridge, segment, View::Damage, args, "b")?;
    let (info, a, b) = bridge.compare(segment, a_key, b_key)?;
    let side = |s: &wowdps_proto::CompareSide| {
        obj! {
            "player": player_ident(&s.total),
            "total": Json::u64(s.total.amount),
            "per_sec": Json::num(round1(s.total.per_sec)),
            "share_pct": Json::num(round1(s.total.pct)),
            "abilities": Json::Arr(
                s.spells.iter().map(|r| ability_row(r, View::Damage)).collect(),
            ),
            "timeline": timeline_json(&s.timeline),
        }
    };
    Ok(obj! {
        "fight": fight_info(None, &info),
        "a": side(&a),
        "b": side(&b),
    })
}

fn loadout(bridge: &mut Bridge, args: &Json) -> Result<Json, String> {
    let segment = arg_segment(args);
    // The build doesn't belong to a view; damage rows list everyone who
    // contributed, which is where a coach's questions start anyway — but a
    // healer who never swung is only on the healing rows, so try those too.
    let (segment, key, row) = match resolve_player(bridge, segment, View::Damage, args, "player") {
        Ok(hit) => hit,
        Err(damage_err) => resolve_player(bridge, segment, View::Healing, args, "player")
            .map_err(|_| damage_err)?,
    };
    let Some(l) = bridge.loadout(segment, key)? else {
        return Ok(obj! {
            "player": player_ident(&row),
            "logged": Json::Bool(false),
            "note": Json::str(
                "no COMBATANT_INFO for this player at or before this fight — the \
                 game logs a build only inside instances (raids, dungeons, arenas), \
                 and none has fired for them yet in this log",
            ),
        });
    };
    Ok(obj! {
        "player": player_ident(&row),
        "logged": Json::Bool(true),
        "spec_id": l.spec_id.map(|s| Json::u64(s as u64)).unwrap_or(Json::Null),
        "talents": talents_json(&l),
        "gear": gear_json(&l.gear),
    })
}

/// The logged picks, named through the talent dataset when it knows the spec
/// — the same picks→encode→decode path the GUI's "from combat log" view
/// takes, so validation and warnings match it exactly. Raw (node, entry,
/// rank) triples otherwise; rank 0 marks a granted node.
fn talents_json(l: &Loadout) -> Json {
    let raw = |note: String| {
        obj! {
            "picks": Json::Arr(
                l.talents
                    .iter()
                    .map(|p| obj! {
                        "node_id": Json::u64(p.node_id as u64),
                        "entry_id": Json::u64(p.entry_id as u64),
                        "rank": Json::u64(p.rank as u64),
                    })
                    .collect(),
            ),
            "note": Json::str(note),
        }
    };
    let Some(spec_id) = l.spec_id.map(u64::from).filter(|&s| s != 0) else {
        return raw("the log carried no spec id, so the picks cannot be named".to_string());
    };
    let picks: Vec<(u32, u32, u32)> = l
        .talents
        .iter()
        .map(|p| (p.node_id, p.entry_id, p.rank))
        .collect();
    match named_talents(spec_id, &picks) {
        Ok(doc) => doc,
        Err(e) => raw(format!("{e} — raw picks only")),
    }
}

fn named_talents(spec_id: u64, picks: &[(u32, u32, u32)]) -> Result<Json, String> {
    let dataset = crate::talents::load()?;
    let shaped = crate::talents::picks_to_selections(dataset, spec_id, picks)?;
    let selections = match shaped.get("selections") {
        Some(Json::Arr(s)) => s.clone(),
        _ => Vec::new(),
    };
    let mut warnings = match shaped.get("warnings") {
        Some(Json::Arr(w)) => w.clone(),
        _ => Vec::new(),
    };
    let encoded = crate::talents::encode(dataset, spec_id, &selections)?;
    let string = encoded
        .get("string")
        .and_then(Json::as_str)
        .ok_or("encode produced no string")?
        .to_string();
    let mut doc = crate::talents::decode(dataset, &string)?;
    if let Json::Obj(fields) = &mut doc {
        for (k, v) in fields.iter_mut() {
            if k == "warnings" {
                // The pick-shaping warnings (skipped/clamped picks) come
                // first: they explain anything odd the decode then reports.
                if let Json::Arr(w) = v {
                    warnings.append(w);
                }
                *v = Json::Arr(std::mem::take(&mut warnings));
            }
        }
        fields.push(("import_string".to_string(), Json::str(string)));
    }
    Ok(doc)
}

/// COMBATANT_INFO's gear dump is positional — the standard inventory-slot
/// order (same table as the GUI's inventory view). Labels apply only when
/// the count fits; an unexpected shape gets unlabeled rows rather than lies.
const GEAR_SLOTS: [&str; 18] = [
    "head",
    "neck",
    "shoulder",
    "shirt",
    "chest",
    "waist",
    "legs",
    "feet",
    "wrist",
    "hands",
    "finger 1",
    "finger 2",
    "trinket 1",
    "trinket 2",
    "back",
    "main hand",
    "off hand",
    "tabard",
];

/// Ids only — the log carries no item names. Zeroed tuples are empty slots.
fn gear_json(gear: &[GearItem]) -> Json {
    let labeled = gear.len() <= GEAR_SLOTS.len();
    let ids = |v: &[u32]| Json::Arr(v.iter().map(|&x| Json::u64(x as u64)).collect());
    let mut items = Vec::new();
    let (mut ilvl_sum, mut ilvl_n) = (0u64, 0u64);
    for (i, g) in gear.iter().enumerate() {
        if g.item_id == 0 {
            continue;
        }
        let slot = if labeled {
            GEAR_SLOTS.get(i).copied().unwrap_or("")
        } else {
            ""
        };
        if g.ilvl > 0 && slot != "shirt" && slot != "tabard" {
            ilvl_sum += g.ilvl as u64;
            ilvl_n += 1;
        }
        let mut o = Vec::new();
        if !slot.is_empty() {
            o.push(("slot".to_string(), Json::str(slot)));
        }
        o.push(("item_id".to_string(), Json::u64(g.item_id as u64)));
        o.push(("ilvl".to_string(), Json::u64(g.ilvl as u64)));
        if !g.enchants.is_empty() {
            o.push(("enchants".to_string(), ids(&g.enchants)));
        }
        if !g.bonus_ids.is_empty() {
            o.push(("bonus_ids".to_string(), ids(&g.bonus_ids)));
        }
        if !g.gems.is_empty() {
            o.push(("gems".to_string(), ids(&g.gems)));
        }
        items.push(Json::Obj(o));
    }
    let mut out = vec![("items".to_string(), Json::Arr(items))];
    if ilvl_n > 0 {
        // Plain mean of the stat-bearing slots (shirt/tabard excluded) —
        // close enough to coach with, not the game's 2H-weighted formula.
        out.push((
            "avg_ilvl".to_string(),
            Json::num(round1(ilvl_sum as f64 / ilvl_n as f64)),
        ));
    }
    Json::Obj(out)
}

// ---- argument plumbing ------------------------------------------------------

fn arg_spec_id(args: &Json) -> Result<u64, String> {
    args.get("spec_id")
        .and_then(Json::as_u64)
        .ok_or_else(|| "spec_id (a ChrSpecialization id) is required".to_string())
}

fn arg_segment(args: &Json) -> SegmentRef {
    match args.get("segment_id").and_then(Json::as_u64) {
        Some(id) => SegmentRef::Id(SegmentId(id)),
        None => SegmentRef::Live,
    }
}

fn arg_view(args: &Json) -> Result<View, String> {
    let Some(name) = args.get("view").and_then(Json::as_str) else {
        return Ok(View::Damage);
    };
    // Case-insensitive so the echoed display name ("Damage") round-trips.
    match name.to_lowercase().as_str() {
        "damage" => Ok(View::Damage),
        "healing" => Ok(View::Healing),
        "interrupts" => Ok(View::Interrupts),
        "crowd_control" | "crowd control" => Ok(View::CrowdControl),
        "dispels" => Ok(View::Dispels),
        "deaths" => Ok(View::Deaths),
        other => Err(format!(
            "unknown view {other:?} (damage, healing, interrupts, crowd_control, dispels, deaths)"
        )),
    }
}

/// A player argument may be the row key (GUID) or the displayed name; the
/// cursor wants the key, so look the name up in the fight's own rows.
///
/// Also returns the segment to use for every follow-up request: `Live`
/// re-resolves on each round trip, so a pull opening between two calls would
/// pair this fight's player with the next fight's data. The snapshot carries
/// the id it resolved to, and the caller pins to it.
fn resolve_player(
    bridge: &mut Bridge,
    segment: SegmentRef,
    view: View,
    args: &Json,
    arg: &str,
) -> Result<(SegmentRef, String, Row), String> {
    let Some(who) = args.get(arg).and_then(Json::as_str) else {
        return Err(format!("{arg:?} is required: a player name or GUID"));
    };
    let snap = bridge.snapshot(Cursor::Segment {
        segment,
        view,
        top_n: None,
        drill: None,
        spell: None,
    })?;
    let found = snap
        .rows
        .iter()
        .find(|r| r.key == who || r.label == who)
        .or_else(|| {
            let lower = who.to_lowercase();
            snap.rows
                .iter()
                .find(|r| r.label.to_lowercase().starts_with(&lower))
        });
    let pinned = snap.id.map(SegmentRef::Id).unwrap_or(segment);
    match found {
        Some(r) => Ok((pinned, r.key.clone(), r.clone())),
        None => Err(format!(
            "no player {:?} in this fight's {} rows; it has: {}",
            who,
            wowdps_model::fmt::view_name(view),
            snap.rows
                .iter()
                .map(|r| r.label.clone())
                .collect::<Vec<_>>()
                .join(", "),
        )),
    }
}

// ---- shaping ----------------------------------------------------------------

fn kind_name(kind: SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Encounter => "encounter",
        SegmentKind::Trash => "trash",
        SegmentKind::Overall => "overall",
    }
}

fn result_name(success: Option<bool>, arena: bool) -> Json {
    match (success, arena) {
        (Some(true), false) => Json::str("kill"),
        (Some(false), false) => Json::str("wipe"),
        (Some(true), true) => Json::str("win"),
        (Some(false), true) => Json::str("loss"),
        (None, _) => Json::Null,
    }
}

fn fight_info(id: Option<SegmentId>, info: &SegmentInfo) -> Json {
    let mut o = vec![
        (
            "id".to_string(),
            id.map(|i| Json::u64(i.0)).unwrap_or(Json::Null),
        ),
        ("kind".to_string(), Json::str(kind_name(info.kind))),
        ("name".to_string(), Json::str(info.name.clone())),
        (
            "duration".to_string(),
            Json::str(wowdps_model::fmt::duration(info.duration_ms)),
        ),
        (
            "duration_ms".to_string(),
            Json::num(info.duration_ms as f64),
        ),
        ("result".to_string(), result_name(info.success, info.arena)),
    ];
    if info.live {
        o.push(("live".to_string(), Json::Bool(true)));
    }
    if let Some(visit) = info.instance {
        o.push(("visit".to_string(), Json::u64(visit as u64)));
    }
    Json::Obj(o)
}

fn player_ident(r: &Row) -> Json {
    obj! {
        "name": Json::str(r.label.clone()),
        "key": Json::str(r.key.clone()),
        "class": r.class.map(|c| Json::str(format!("{c:?}"))).unwrap_or(Json::Null),
        "spec": r.spec.map(|s| Json::str(s.name())).unwrap_or(Json::Null),
    }
}

/// One meter row. `amount` is damage/healing for those views, an event count
/// for the rest; `extra` is overkill (damage) or overheal (healing).
fn meter_row(rank: usize, r: &Row, view: View, _dur_ms: i64) -> Json {
    let mut o = vec![
        ("rank".to_string(), Json::u64(rank as u64 + 1)),
        ("player".to_string(), Json::str(r.label.clone())),
        (
            "class".to_string(),
            r.class
                .map(|c| Json::str(format!("{c:?}")))
                .unwrap_or(Json::Null),
        ),
        (
            "spec".to_string(),
            r.spec.map(|s| Json::str(s.name())).unwrap_or(Json::Null),
        ),
        ("amount".to_string(), Json::u64(r.amount)),
        ("share_pct".to_string(), Json::num(round1(r.pct))),
    ];
    if view.is_rate() {
        o.push(("per_sec".to_string(), Json::num(round1(r.per_sec))));
        o.push(("crit_pct".to_string(), Json::num(round1(r.crit_pct()))));
        o.push((
            if view == View::Healing {
                "overheal".to_string()
            } else {
                "overkill".to_string()
            },
            Json::u64(r.extra),
        ));
    }
    o.push(("events".to_string(), Json::u64(r.count)));
    Json::Obj(o)
}

/// One breakdown row (an ability, a target — or a death-recap event, which
/// additionally reports remaining health).
fn ability_row(r: &Row, view: View) -> Json {
    let mut o = vec![
        ("name".to_string(), Json::str(r.label.clone())),
        ("amount".to_string(), Json::u64(r.amount)),
        ("share_pct".to_string(), Json::num(round1(r.pct))),
        ("hits".to_string(), Json::u64(r.count)),
    ];
    if view.is_rate() {
        o.push(("crit_pct".to_string(), Json::num(round1(r.crit_pct()))));
        if let Some(avg) = r.amount.checked_div(r.count) {
            o.push(("avg_hit".to_string(), Json::u64(avg)));
        }
        o.push(("per_sec".to_string(), Json::num(round1(r.per_sec))));
    }
    if let Some((hp, max)) = r.hp {
        o.push((
            "health_after".to_string(),
            obj! { "current": Json::u64(hp), "max": Json::u64(max) },
        ));
    }
    Json::Obj(o)
}

/// A fight timeline, compacted: per-10s DPS points plus the item markers.
fn timeline_json(tl: &Timeline) -> Json {
    obj! {
        "bucket_secs": Json::num(CURVE_BUCKET_MS as f64 / 1000.0),
        "dps": Json::Arr(
            curve(tl).into_iter().map(|d| Json::num(d.round())).collect(),
        ),
        "marks": Json::Arr(tl.marks.iter().map(mark_json).collect()),
    }
}

/// Re-bucket the fine grid onto `CURVE_BUCKET_MS` and convert to a rate.
fn curve(tl: &Timeline) -> Vec<f64> {
    if tl.bucket_ms == 0 || tl.buckets.is_empty() {
        return Vec::new();
    }
    let per = (CURVE_BUCKET_MS / tl.bucket_ms).max(1) as usize;
    tl.buckets
        .chunks(per)
        .map(|chunk| {
            let sum: u64 = chunk.iter().sum();
            let span_secs = chunk.len() as f64 * tl.bucket_ms as f64 / 1000.0;
            if span_secs > 0.0 {
                sum as f64 / span_secs
            } else {
                0.0
            }
        })
        .collect()
}

fn mark_json(m: &Mark) -> Json {
    let kind = match m.kind {
        wowdps_model::MarkKind::TrinketUse => "trinket_use",
        wowdps_model::MarkKind::TrinketProc => "trinket_proc",
        wowdps_model::MarkKind::Consumable => "consumable",
        wowdps_model::MarkKind::External => "external_buff",
    };
    let mut o = vec![
        (
            "at_secs".to_string(),
            Json::num((m.at_ms as f64 / 100.0).round() / 10.0),
        ),
        ("kind".to_string(), Json::str(kind)),
        ("label".to_string(), Json::str(m.label.clone())),
    ];
    if m.dur_ms > 0 {
        o.push((
            "active_secs".to_string(),
            Json::num((m.dur_ms as f64 / 100.0).round() / 10.0),
        ));
    }
    Json::Obj(o)
}

fn opt_str(s: Option<String>) -> Json {
    s.map(Json::Str).unwrap_or(Json::Null)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::{Class, MarkKind, Spec, TalentPick};

    fn keys(j: &Json) -> Vec<&str> {
        match j {
            Json::Obj(o) => o.iter().map(|(k, _)| k.as_str()).collect(),
            other => panic!("not an object: {other:?}"),
        }
    }

    #[test]
    fn marks_and_timelines_compact_onto_the_coarse_grid() {
        let m = |kind, dur_ms| Mark {
            at_ms: 12_345,
            kind,
            label: "Sigil".to_string(),
            spell_id: 1,
            dur_ms,
        };
        let cases = [
            (MarkKind::TrinketUse, 0, "trinket_use"),
            (MarkKind::TrinketProc, 15_000, "trinket_proc"),
            (MarkKind::Consumable, 0, "consumable"),
            (MarkKind::External, 30_000, "external_buff"),
        ];
        for (kind, dur, name) in cases {
            let j = mark_json(&m(kind, dur));
            assert_eq!(j.get("kind").and_then(Json::as_str), Some(name));
            assert_eq!(j.get("at_secs").and_then(Json::as_f64), Some(12.3));
            assert_eq!(
                j.get("active_secs").and_then(Json::as_f64),
                (dur > 0).then_some(dur as f64 / 1000.0)
            );
        }

        // 1 s buckets re-bucketed to 10 s: a partial last chunk keeps its
        // own span.
        let tl = Timeline {
            bucket_ms: 1000,
            buckets: (0..15).map(|_| 1000).collect(),
            marks: vec![m(MarkKind::Consumable, 0)],
        };
        let j = timeline_json(&tl);
        assert_eq!(j.get("bucket_secs").and_then(Json::as_f64), Some(10.0));
        let dps: Vec<f64> = match j.get("dps") {
            Some(Json::Arr(v)) => v.iter().filter_map(Json::as_f64).collect(),
            _ => panic!("no dps"),
        };
        assert_eq!(dps, vec![1000.0, 1000.0]);
        assert!(matches!(j.get("marks"), Some(Json::Arr(v)) if v.len() == 1));
        // No grid at all: no curve.
        assert!(curve(&Timeline::default()).is_empty());
    }

    #[test]
    fn fights_rows_and_results_are_labelled_by_kind_and_view() {
        assert_eq!(result_name(Some(true), true), Json::str("win"));
        assert_eq!(result_name(Some(false), true), Json::str("loss"));
        assert_eq!(result_name(None, true), Json::Null);

        let info = SegmentInfo {
            kind: SegmentKind::Overall,
            name: "Key".to_string(),
            start_ms: 0,
            duration_ms: 61_000,
            success: Some(true),
            live: true,
            instance: Some(2),
            pars_ms: None,
            arena: false,
            encounter: None,
        };
        let f = fight_info(Some(SegmentId(4)), &info);
        assert_eq!(f.get("id").and_then(Json::as_u64), Some(4));
        assert_eq!(f.get("kind").and_then(Json::as_str), Some("overall"));
        assert_eq!(f.get("duration").and_then(Json::as_str), Some("1:01"));
        assert_eq!(f.get("live"), Some(&Json::Bool(true)));
        assert_eq!(f.get("visit").and_then(Json::as_u64), Some(2));
        assert_eq!(fight_info(None, &info).get("id"), Some(&Json::Null));

        let r = Row {
            key: "Player-1".to_string(),
            label: "Ana".to_string(),
            amount: 1000,
            extra: 50,
            count: 4,
            crits: 1,
            per_sec: 12.34,
            pct: 33.333,
            class: Some(Class::Mage),
            spec: Spec::from_id(63),
            hp: Some((10, 100)),
            ..Row::default()
        };
        let healing = meter_row(0, &r, View::Healing, 1000);
        assert!(keys(&healing).contains(&"overheal"), "{healing:?}");
        assert_eq!(healing.get("crit_pct").and_then(Json::as_f64), Some(25.0));
        let deaths = meter_row(2, &r, View::Deaths, 1000);
        assert!(!keys(&deaths).contains(&"per_sec"));
        assert_eq!(deaths.get("rank").and_then(Json::as_u64), Some(3));
        assert_eq!(deaths.get("spec").and_then(Json::as_str), Some("Fire"));

        let recap = ability_row(&r, View::Deaths);
        assert_eq!(
            recap
                .get("health_after")
                .and_then(|h| h.get("max"))
                .and_then(Json::as_u64),
            Some(100)
        );
        assert!(!keys(&recap).contains(&"avg_hit"));
        let hit = ability_row(&r, View::Damage);
        assert_eq!(hit.get("avg_hit").and_then(Json::as_u64), Some(250));
        assert_eq!(
            player_ident(&r).get("class").and_then(Json::as_str),
            Some("Mage")
        );
        assert_eq!(kind_name(SegmentKind::Trash), "trash");
        assert_eq!(opt_str(None), Json::Null);
    }

    #[test]
    fn gear_is_labelled_by_slot_only_when_the_shape_fits() {
        let item = |id, ilvl| GearItem {
            item_id: id,
            ilvl,
            enchants: vec![7],
            bonus_ids: vec![8, 9],
            gems: vec![10],
        };
        let mut gear = vec![item(0, 0); 18];
        gear[0] = item(100, 600);
        gear[3] = item(101, 1); // shirt: never counts toward the average
        gear[15] = item(102, 620);
        let j = gear_json(&gear);
        let items = match j.get("items") {
            Some(Json::Arr(v)) => v.clone(),
            _ => panic!("no items: {j:?}"),
        };
        assert_eq!(items.len(), 3, "empty slots are skipped");
        assert_eq!(items[0].get("slot").and_then(Json::as_str), Some("head"));
        assert_eq!(
            items[2].get("slot").and_then(Json::as_str),
            Some("main hand")
        );
        assert!(matches!(items[0].get("enchants"), Some(Json::Arr(v)) if v.len() == 1));
        assert!(matches!(items[0].get("gems"), Some(Json::Arr(v)) if v.len() == 1));
        assert_eq!(j.get("avg_ilvl").and_then(Json::as_f64), Some(610.0));

        // An unexpected count gets honest, unlabeled rows.
        let odd = vec![item(1, 10); 19];
        let j = gear_json(&odd);
        let items = match j.get("items") {
            Some(Json::Arr(v)) => v.clone(),
            _ => panic!("no items: {j:?}"),
        };
        assert_eq!(items.len(), 19);
        assert!(items.iter().all(|i| i.get("slot").is_none()));
    }

    /// Without a spec id, or without a dataset to name them, the picks are
    /// returned raw with a note saying why.
    #[test]
    fn talents_fall_back_to_raw_picks_with_a_reason() {
        let pick = TalentPick {
            node_id: 5,
            entry_id: 6,
            rank: 0,
        };
        let no_spec = Loadout {
            spec_id: None,
            talents: vec![pick],
            gear: Vec::new(),
        };
        let j = talents_json(&no_spec);
        assert!(
            j.get("note")
                .and_then(Json::as_str)
                .is_some_and(|n| n.contains("no spec id")),
            "{j:?}"
        );
        assert!(matches!(j.get("picks"), Some(Json::Arr(p)) if p.len() == 1));

        // Point the dataset somewhere empty so naming cannot succeed.
        // Env is process-global; this is the only test in this binary
        // touching it.
        unsafe { std::env::set_var("WOWDPS_TALENTS", "/nonexistent/wowdps-talents.json") };
        let with_spec = Loadout {
            spec_id: Some(71),
            ..no_spec.clone()
        };
        let j = talents_json(&with_spec);
        assert!(
            j.get("note")
                .and_then(Json::as_str)
                .is_some_and(|n| n.ends_with("raw picks only")),
            "{j:?}"
        );
        // A zero spec id is "no spec" too.
        let zero = Loadout {
            spec_id: Some(0),
            ..no_spec
        };
        assert!(
            talents_json(&zero)
                .get("note")
                .and_then(Json::as_str)
                .is_some_and(|n| n.contains("no spec id"))
        );
    }

    #[test]
    fn argument_errors_never_reach_the_daemon() {
        let mut bridge = Bridge::lazy();
        let err = call(&mut bridge, "no_such_tool", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("no such tool"), "{err}");
        let err = call(&mut bridge, "encode_talents", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("selections array"), "{err}");
        let err = call(&mut bridge, "decode_talents", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("requires a string"), "{err}");
        // v20: the history tools' required arguments fail the same way.
        let err = call(&mut bridge, "progression", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("requires encounter"), "{err}");
        let err = call(&mut bridge, "stored_fight", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("requires fight_id"), "{err}");
        let err = call(&mut bridge, "pin_fight", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("requires fight_id"), "{err}");
        let err = call(&mut bridge, "trend", &Json::Obj(Vec::new())).unwrap_err();
        assert!(err.contains("requires player"), "{err}");
        assert_eq!(catalog().len(), 14);
    }
}
