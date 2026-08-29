//! The MCP tool surface: what an LLM can ask the meter. Every tool answers
//! with one JSON document in a text content block — compact, self-labeled,
//! and stable in shape, so a harness can reason over fights without ever
//! seeing the wire protocol.

use crate::bridge::Bridge;
use crate::json::Json;
use crate::obj;

use wowdps_model::{
    GearItem, Loadout, Mark, Row, SegmentId, SegmentInfo, SegmentKind, Timeline, View,
};
use wowdps_proto::{Cursor, ListEntry, OverlayState, SegmentRef};

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
    vec![
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
            name: "loadout",
            description: "One player's actual build as the combat log recorded it \
                          (COMBATANT_INFO): spec, talents and equipped gear with item \
                          levels, enchants, gems and bonus ids. Talents come named \
                          through the local talent dataset with an in-game import \
                          string when the dataset knows the spec, raw \
                          node/entry/rank picks otherwise (rank 0 = a granted node). \
                          The game logs a build only inside instances (raids, \
                          dungeons, arenas) — elsewhere the answer is logged: false.",
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
    ]
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
    })
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
    let (key, row) = resolve_player(bridge, segment, view, args, "player")?;
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
    let (a_key, _) = resolve_player(bridge, segment, View::Damage, args, "a")?;
    let (b_key, _) = resolve_player(bridge, segment, View::Damage, args, "b")?;
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
    // contributed, which is where a coach's questions start anyway.
    let (key, row) = resolve_player(bridge, segment, View::Damage, args, "player")?;
    let Some(l) = bridge.loadout(segment, key)? else {
        return Ok(obj! {
            "player": player_ident(&row),
            "logged": Json::Bool(false),
            "note": Json::str(
                "no COMBATANT_INFO for this player in this fight — the game logs a \
                 build only inside instances (raids, dungeons, arenas)",
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
fn resolve_player(
    bridge: &mut Bridge,
    segment: SegmentRef,
    view: View,
    args: &Json,
    arg: &str,
) -> Result<(String, Row), String> {
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
    match found {
        Some(r) => Ok((r.key.clone(), r.clone())),
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
