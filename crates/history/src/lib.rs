//! The history store's analytical reader (roadmap item 1, spec §10): DuckDB
//! over the lake the daemon writes under `$XDG_DATA_HOME/wowdps/history/v1/`.
//! The daemon answers the fixed questions from its card index; this crate
//! answers the ad hoc ones in SQL, and the parity gate in `tests/parity.rs`
//! keeps the two readers honest against the same files.
//!
//! The engine opens in memory with autoinstall / autoload off and the
//! configuration locked, so it never touches the network; `materialize`
//! writes `cache.duckdb` beside the lake, a file only this binary ever opens
//! (DuckDB's single-writer lock never crosses a process).

use std::path::{Path, PathBuf};

use duckdb::types::Value;
use duckdb::{Config, Connection};
use wowdps_model::Spec;
use wowdps_proto::json::Json;
use wowdps_proto::obj;

/// The lake's data directories — one view each, and the only places a
/// read-only lake may touch.
pub const DIRS: [&str; 5] = ["fights", "rows", "details", "loadouts", "annotations"];

/// The grader's floors (roadmap item 1a, step 1), the same numbers as
/// `wowdps_mcp::DPS_FLOOR` / `DPS_TOP_FLOOR` — the binary cannot link the
/// mcp crate (CONTRACT: model + proto + duckdb), so `tests/parity.rs`
/// asserts the two pairs are equal instead. A same-role player below
/// `DPS_FLOOR` × the median of the OTHER same-role players, or below
/// `DPS_TOP_FLOOR` × the top one, is not a data point: `role_ranks` drops
/// them and counts them in `excluded`.
pub const DPS_FLOOR: f64 = 0.10;
pub const DPS_TOP_FLOOR: f64 = 0.01;

/// `CASE p.spec WHEN <id> THEN '<role>' … END` over every spec, so a lake
/// written before cards carried `role` still answers role queries.
fn role_case() -> String {
    let mut sql = String::from("CASE p.spec");
    for spec in Spec::ALL {
        sql.push_str(&format!(
            " WHEN {} THEN '{}'",
            spec.id(),
            spec.role().name()
        ));
    }
    sql.push_str(" END");
    sql
}

/// `CASE p.spec WHEN 1473 THEN true ELSE false END` — R19's support flag,
/// derived from the spec the way `role` is (`Spec::support`: Augmentation
/// only). No card stores it; a specless player reads false.
fn support_case() -> String {
    let ids: Vec<String> = Spec::ALL
        .iter()
        .filter(|s| s.support())
        .map(|s| s.id().to_string())
        .collect();
    format!(
        "CASE WHEN p.spec IN ({}) THEN true ELSE false END",
        ids.join(", ")
    )
}

/// Every field of a stored `Row` (`wowdps_proto::history::row_json`) as
/// columns off the struct `alias`, in the codec's own order. `as_guid`
/// renames `key` to `guid` — a meter row's key IS the player's guid, and
/// on the by-ability / by-attacker drills it is the ability or the
/// attacker's name and stays `key`.
fn row_cols(alias: &str, as_guid: bool) -> String {
    const FIELDS: [&str; 14] = [
        "label", "amount", "extra", "count", "crits", "per_sec", "pct", "class", "spec", "hp",
        "gain", "spell_id", "enemy", "school",
    ];
    let mut sql = if as_guid {
        format!("{alias}.key AS guid")
    } else {
        format!("{alias}.key AS key")
    };
    for f in FIELDS {
        sql.push_str(&format!(", {alias}.{f} AS {f}"));
    }
    sql
}

/// Where the lake lives: `$XDG_DATA_HOME/wowdps/history/v1`, else
/// `~/.local/share/wowdps/history/v1` — the daemon's default too.
pub fn default_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("wowdps/history/v1"))
}

/// A query's answer: column names and rows of JSON values.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Json>>,
}

impl Table {
    /// `{"columns": [...], "rows": [[...], ...]}`.
    pub fn to_json(&self) -> Json {
        obj! {
            "columns": Json::Arr(self.columns.iter().map(|c| Json::str(c.clone())).collect()),
            "rows": Json::Arr(self.rows.iter().map(|r| Json::Arr(r.clone())).collect()),
        }
    }

    /// One object per row, keyed by column.
    pub fn to_objects(&self) -> Json {
        Json::Arr(
            self.rows
                .iter()
                .map(|r| {
                    Json::Obj(
                        self.columns
                            .iter()
                            .cloned()
                            .zip(r.iter().cloned())
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    /// Tab-separated text with a header line.
    pub fn to_text(&self) -> String {
        let mut out = self.columns.join("\t");
        out.push('\n');
        for r in &self.rows {
            let cells: Vec<String> = r
                .iter()
                .map(|v| match v {
                    Json::Str(s) => s.clone(),
                    Json::Null => String::new(),
                    other => other.to_line(),
                })
                .collect();
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        out
    }
}

/// The lake, opened: an in-memory DuckDB with one view per directory that
/// holds files (`fights`; `players` — the cards' player lines unnested,
/// `role` filled in from the spec when the card predates it; `role_ranks`
/// — the daemon's role-relative grader over `players`; `rows`, `details`,
/// `loadouts`, `annotations`; R17's `taken` / `mitigation` /
/// `taken_spells` / `taken_sources`, R19's `support` / `support_targets`
/// and R18's `uptime` / `coarse`, each defined only when the lake's own
/// files carry the shape that view needs).
pub struct Lake {
    dir: PathBuf,
    conn: Connection,
    views: Vec<&'static str>,
    /// Whether any stored card carries `role` on its players (cards
    /// written before roadmap item 1a step 1 do not).
    players_have_role: bool,
    /// R17 (step 2b): whether the cards' player struct carries the tank
    /// measures (`taken` / `mitigated` / `prevented` / `dtps` /
    /// `mitigated_pct`). A card written before step 2b carries none of
    /// them, and `union_by_name` only gives the struct the fields once ONE
    /// card in the lake does.
    players_have_taken: bool,
    /// Whether any rows file carries a usable `mitigation` list — false on
    /// a lake whose rows all predate step 2b, and false too when every
    /// file's list is empty (DuckDB then types it JSON, not a struct list).
    rows_have_mitigation: bool,
    /// R19 (step 3b): whether the cards' player struct carries the healing
    /// split and the support scalars (`overheal` / `absorbed` /
    /// `support_given` / `support_received` / `healed_received` /
    /// `self_healed`). A PR #19 card carries none, and `effective_dps_sql`
    /// then folds `damage` alone.
    players_have_support: bool,
    /// R18 (step 4b): whether the cards' player struct carries the span
    /// scalars (`am_uptime_ms` / `externals_given` / `externals_given_ms`
    /// / `externals_received` / `externals_received_ms`). A PR #23 card
    /// carries none, and `am_uptime_pct_sql` then reads 0.
    players_have_spans: bool,
    /// R18 (step 4b): whether any rows file carries the `uptime` KEY at all
    /// (a list, empty or not) — the honest denominator for
    /// `rows_without_uptime`: a fight with no role aura writes `[]`, which
    /// is a stored answer, not a missing one, even though the `uptime`
    /// view cannot be typed off it.
    rows_have_uptime_key: bool,
}

impl Lake {
    /// Read-only towards the rest of the machine, as every reader is
    /// promised: file access is fenced to the lake's own directories, so an
    /// ad hoc query — the MCP `history_sql` tool hands an LLM's SQL here
    /// verbatim — can neither `COPY` out nor `read_text` in.
    pub fn open(dir: &Path) -> Result<Self, String> {
        Self::open_with(dir, false)
    }

    /// Keeps file access for the one writer, `materialize` (ATTACH).
    pub fn open_writable(dir: &Path) -> Result<Self, String> {
        Self::open_with(dir, true)
    }

    fn open_with(dir: &Path, external: bool) -> Result<Self, String> {
        let cfg = Config::default()
            .threads(2)
            .map_err(|e| e.to_string())?
            .max_memory("256MB")
            .map_err(|e| e.to_string())?;
        let conn = Connection::open_in_memory_with_flags(cfg).map_err(|e| e.to_string())?;
        // Offline by construction: nothing auto-installs or auto-loads, an
        // explicit INSTALL can only look in a repository that does not
        // exist (never the network, never ~/.duckdb), and LOAD can only find
        // what sits in the lake's own (empty) extension directory. JSON,
        // Parquet and ICU are statically linked in the nixpkgs build, so
        // nothing here needs an extension anyway.
        let quoted = |p: PathBuf| p.display().to_string().replace('\'', "''");
        conn.execute_batch(&format!(
            "SET autoinstall_known_extensions = false;\n\
             SET autoload_known_extensions = false;\n\
             SET custom_extension_repository = '{}';\n\
             SET extension_directory = '{}';",
            quoted(dir.join(".no-extension-repository")),
            quoted(dir.join(".extensions")),
        ))
        .map_err(|e| e.to_string())?;
        let mut lake = Self {
            dir: dir.to_path_buf(),
            conn,
            views: Vec::new(),
            players_have_role: false,
            players_have_taken: false,
            rows_have_mitigation: false,
            players_have_support: false,
            players_have_spans: false,
            rows_have_uptime_key: false,
        };
        lake.define_views()?;
        // A view re-reads its files on every query, so file access cannot
        // simply be switched off: instead it is fenced to the lake's own
        // data directories (plus the empty extension directory, which
        // `duckdb_extensions()` lists). Anything else on the machine —
        // `COPY … TO` out, `read_text` in — is a permission error, and the
        // setting is locked in. `materialize` keeps full access for its
        // ATTACH beside the lake.
        let access = if external {
            String::new()
        } else {
            let dirs = DIRS
                .iter()
                .map(|d| format!("'{}'", quoted(dir.join(d))))
                .chain(std::iter::once(format!(
                    "'{}'",
                    quoted(dir.join(".extensions"))
                )))
                .collect::<Vec<_>>()
                .join(", ");
            format!("SET allowed_directories = [{dirs}];\nSET enable_external_access = false;\n")
        };
        lake.conn
            .execute_batch(&format!("{access}SET lock_configuration = true;"))
            .map_err(|e| e.to_string())?;
        Ok(lake)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The views this lake defined — a directory with no files defines none
    /// (DuckDB binds a view's glob at creation).
    pub fn views(&self) -> &[&'static str] {
        &self.views
    }

    fn has_files(&self, sub: &str, ext: &str) -> bool {
        std::fs::read_dir(self.dir.join(sub))
            .map(|d| {
                d.flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == ext))
            })
            .unwrap_or(false)
    }

    fn define_views(&mut self) -> Result<(), String> {
        let root = self.dir.display().to_string().replace('\'', "''");
        let glob = move |sub: &str, ext: &str| format!("'{root}/{sub}/*.{ext}'");
        if self.has_files("fights", "json") {
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW fights AS SELECT * FROM read_json({}, format = 'auto', \
                     union_by_name = true);",
                    glob("fights", "json")
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("fights");
            // `role` is derived from `spec` here exactly as the codec does
            // on read (`from_json` ignores the stored field: the spec is
            // the truth). The stored value is never SELECTed — a card
            // written before roadmap item 1a step 1 has no `role` field
            // at all, `union_by_name` gives the struct one as soon as ONE
            // card carries it, and a lake whose every stored role is null
            // (an arena card, an R8-failed roster) has DuckDB sniff it as
            // JSON rather than VARCHAR, which no coalesce can survive. The
            // probe only says whether there is a field to EXCLUDE (and
            // lets `cards_without_role` count).
            self.players_have_role = self
                .sql(
                    "SELECT role FROM (SELECT unnest(players, recursive := true) FROM fights) \
                     LIMIT 0",
                )
                .is_ok();
            let exclude = if self.players_have_role {
                "EXCLUDE (role)"
            } else {
                ""
            };
            // R17 (step 2b), the same probe on the same reasoning: the tank
            // measures ride the card's player struct, so a lake of PR #16
            // cards has no such field to SELECT and `mitigated_pct_sql`
            // could not bind. The four are written together, so one probe
            // covers them; `mitigated_pct` is derived and written beside
            // them, and it is kept as the STORED column so parity can hold
            // it against the computed one.
            self.players_have_taken = self
                .sql(
                    "SELECT taken, mitigated, prevented, dtps FROM \
                     (SELECT unnest(players, recursive := true) FROM fights) LIMIT 0",
                )
                .is_ok();
            // The model's one formula (`wowdps_model::mitigated_pct`):
            // mitigated over everything swung with an amount, 0 when
            // nothing was. A card that predates the measures reads 0 here,
            // exactly as `CardPlayer::from_json` does — and since `p.*`
            // then has no stored `mitigated_pct` to offer, the computed 0
            // is named `mitigated_pct` as well, so `SELECT mitigated_pct
            // FROM players` answers on any lake.
            let pct_sql = if self.players_have_taken {
                ", CASE WHEN coalesce(p.taken, 0) + coalesce(p.prevented, 0) = 0 THEN 0.0 \
                 ELSE coalesce(p.mitigated, 0) * 100.0 \
                 / (coalesce(p.taken, 0) + coalesce(p.prevented, 0)) END AS mitigated_pct_sql"
            } else {
                ", CAST(0.0 AS DOUBLE) AS mitigated_pct, CAST(0.0 AS DOUBLE) AS mitigated_pct_sql"
            };
            // R19 (step 3b): the healing split and the support scalars ride
            // the player struct too, written together — one probe. The six
            // come through `p.*`; `effective_dps` is derived on the card
            // (`CardPlayer::effective_dps`) and written beside them, kept
            // as the STORED column so parity can hold it against the one
            // computed here.
            self.players_have_support = self
                .sql(
                    "SELECT overheal, absorbed, support_given, support_received, \
                     healed_received, self_healed FROM \
                     (SELECT unnest(players, recursive := true) FROM fights) LIMIT 0",
                )
                .is_ok();
            // The model's one fold (`wowdps_model::effective`): `damage −
            // received + given`, clamped at 0 (R19's ruling on a share that
            // exceeds the damage it folds against), over the card's
            // duration by the meter's own per-second arithmetic — `amount
            // as f64 / (duration_ms as f64 / 1000.0)`, the form
            // `CardPlayer::effective_dps` uses, so the two agree bit for
            // bit rather than to a rounding. The coalesce is what makes a
            // pre-3b card (no scalars) read its `dps`; a lake with no such
            // card at all has no scalar column to coalesce, so the
            // numerator is `damage` alone and, since `p.*` then offers no
            // stored `effective_dps`, a NULL one is named so `SELECT
            // effective_dps FROM players` answers on any lake.
            let (effective, stored_effective) = if self.players_have_support {
                (
                    "greatest(0, coalesce(p.damage, 0) - coalesce(p.support_received, 0) \
                     + coalesce(p.support_given, 0))",
                    "",
                )
            } else {
                (
                    "coalesce(p.damage, 0)",
                    ", CAST(NULL AS DOUBLE) AS effective_dps",
                )
            };
            let effective_sql = format!(
                "{stored_effective}, CASE WHEN f.duration_ms > 0 \
                 THEN CAST({effective} AS DOUBLE) / (CAST(f.duration_ms AS DOUBLE) / 1000.0) \
                 ELSE 0.0 END AS effective_dps_sql"
            );
            // R18 (step 4b): the span scalars ride the player struct too,
            // written together — one probe. The five come through `p.*`;
            // `am_uptime_pct` is derived on the card
            // (`CardPlayer::am_uptime_pct`) and written beside them, kept
            // as the STORED column so parity can hold it against the one
            // computed here: `am_uptime_ms as f64 * 100.0 / duration_ms as
            // f64` in that order, DOUBLE first (the 3b DECIMAL trap), so
            // the two agree bit for bit. On a lake with no such card the
            // same synthesis as `pct_sql`: both pct columns exist and read
            // 0, the scalars do not exist at all — exactly as 2b's `taken`.
            self.players_have_spans = self
                .sql(
                    "SELECT am_uptime_ms, externals_given, externals_given_ms, \
                     externals_received, externals_received_ms FROM \
                     (SELECT unnest(players, recursive := true) FROM fights) LIMIT 0",
                )
                .is_ok();
            let am_sql = if self.players_have_spans {
                ", CASE WHEN f.duration_ms > 0 \
                 THEN CAST(coalesce(p.am_uptime_ms, 0) AS DOUBLE) * 100.0 \
                      / CAST(f.duration_ms AS DOUBLE) \
                 ELSE 0.0 END AS am_uptime_pct_sql"
            } else {
                ", CAST(0.0 AS DOUBLE) AS am_uptime_pct, CAST(0.0 AS DOUBLE) AS am_uptime_pct_sql"
            };
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW players AS SELECT f.id AS fight_id, f.kind, f.name AS fight, \
                     f.start_utc_ms, f.duration_ms, f.success, f.aborted, \
                     f.encounter.id AS encounter_id, f.encounter.difficulty AS difficulty, \
                     p.* {exclude}, {} AS role, {} AS support{pct_sql}{effective_sql}{am_sql} \
                     FROM fights f, unnest(f.players) AS u(p);",
                    role_case(),
                    support_case(),
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("players");
            // The daemon's grader in SQL (`wowdps_mcp::grade`): friendly
            // DPS ranked by effective dps among DPS (R19, step 3b — one
            // measure for the whole role, no "fight has support" predicate:
            // without support scalars it IS dps, so a pre-3b card ranks
            // exactly as it did under v22), healers by hps among healers,
            // both under the floors. Every same-role player is in the pool
            // — a zero-output row still moves the median of the others and
            // is then dropped by the floors, exactly as the daemon does it;
            // tanks have no measure yet and are not here.
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW role_ranks AS \
                     WITH m AS (\
                       SELECT fight_id, guid, name, role, spec, \
                              CASE role WHEN 'healer' THEN hps ELSE effective_dps_sql END \
                                AS measure \
                       FROM players WHERE NOT enemy AND role IN ('dps', 'healer')\
                     ), f AS (\
                       SELECT a.*, \
                              max(measure) OVER pool AS top, \
                              count(*) OVER pool AS pool_size, \
                              (SELECT median(b.measure) FROM m b \
                               WHERE b.fight_id = a.fight_id AND b.role = a.role \
                                 AND b.guid <> a.guid) AS others_median \
                       FROM m a WINDOW pool AS (PARTITION BY fight_id, role)\
                     ) \
                     SELECT fight_id, guid, name, role, spec, measure, \
                            CASE role WHEN 'healer' THEN 'hps' ELSE 'effective_dps' END \
                              AS rank_measure, \
                            rank() OVER w AS rank, \
                            count(*) OVER w AS count, \
                            median(measure) OVER w AS median, \
                            pool_size - count(*) OVER w AS excluded \
                     FROM f \
                     WHERE (others_median IS NULL OR measure >= others_median * {DPS_FLOOR}) \
                       AND measure >= top * {DPS_TOP_FLOOR} \
                     WINDOW w AS (PARTITION BY fight_id, role ORDER BY measure DESC \
                                  RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING);"
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("role_ranks");
        }
        if self.has_files("rows", "json") {
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW rows AS SELECT * FROM read_json({}, format = 'auto', \
                     union_by_name = true);",
                    glob("rows", "json")
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("rows");
            self.define_taken_views();
            self.define_support_views();
            self.define_span_views();
        }
        if self.has_files("details", "json") {
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW details AS SELECT * FROM read_json({}, format = 'auto', \
                     union_by_name = true);",
                    glob("details", "json")
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("details");
        }
        if self.has_files("loadouts", "json") {
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW loadouts AS SELECT * FROM read_json({}, format = 'auto', \
                     union_by_name = true);",
                    glob("loadouts", "json")
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("loadouts");
        }
        if self.has_files("annotations", "ndjson") {
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW annotations AS SELECT filename, * FROM read_json({}, \
                     format = 'newline_delimited', union_by_name = true, filename = true);",
                    glob("annotations", "ndjson")
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("annotations");
        }
        Ok(())
    }

    /// Define `name` as `sql` only if the lake's files really carry the
    /// shape it needs. `union_by_name` types a key that no file carries as
    /// NULL and an always-empty list as JSON, and neither survives a struct
    /// field reference — so the view is created, probed with `LIMIT 0`, and
    /// dropped again when the probe cannot bind. `false` = not defined,
    /// which is the honest answer for an un-regraded lake, not an error.
    fn probe_view(&mut self, name: &'static str, sql: &str, typed: &[&str]) -> bool {
        if self
            .conn
            .execute_batch(&format!("CREATE VIEW {name} AS {sql};"))
            .is_err()
        {
            return false;
        }
        // Binding is not enough. DuckDB types a list that is `[]` in every
        // file as JSON, and JSON answers a struct field reference (`t.key`)
        // with more JSON instead of failing — so the view would define,
        // select, and hand every reader untyped columns. `typed` names the
        // columns that carry the shape's meaning (never one like `hp` or
        // `class`, legitimately null on every Taken row and JSON for it):
        // if DuckDB could not infer a real type for those, the files do
        // not carry the shape and the view is not this lake's. The rule is
        // "does not START with JSON", not "is not JSON": a LIST column that
        // is `[]` in every file types as `JSON[]` (step 4b's `taken10`),
        // which an exact comparison would wave through.
        let inferred = self
            .sql(&format!("DESCRIBE SELECT * FROM {name}"))
            .map(|t| {
                typed.iter().all(|col| {
                    t.rows.iter().any(|r| {
                        r.first().and_then(Json::as_str) == Some(col)
                            && r.get(1)
                                .and_then(Json::as_str)
                                .is_some_and(|ty| !ty.starts_with("JSON"))
                    })
                })
            })
            .unwrap_or(false);
        if !inferred || self.sql(&format!("SELECT * FROM {name} LIMIT 0")).is_err() {
            let _ = self.conn.execute_batch(&format!("DROP VIEW {name};"));
            return false;
        }
        self.views.push(name);
        true
    }

    /// R17 (step 2b): the Taken meter rows and the mitigation record with
    /// both its drills, unnested out of the rows tier. Every one of the
    /// four is probed: 0 of the real lake's rows files carried any of this
    /// before `regrade`, and a mixed lake carries it in some files only.
    fn define_taken_views(&mut self) {
        let has_taken = self.probe_view(
            "taken",
            &format!(
                "SELECT r.id AS fight_id, {} FROM rows r, unnest(r.views.taken) AS u(t)",
                row_cols("t", true)
            ),
            &["guid", "amount"],
        );
        // The Taken row amount the mitigated pct divides by: the meter's
        // own row when the rows tier carries it, else the card's copy of
        // the same number. The daemon writes both together with the
        // mitigation list, so a lake with neither has no `mitigation` view
        // to define and the probe below says so; a player with no Taken
        // row at all (only missed) still coalesces to 0 and the
        // zero-denominator guard keeps the pct a number.
        let (join, taken_expr) = if has_taken {
            (
                " LEFT JOIN taken tk ON tk.fight_id = m.fight_id AND tk.guid = m.guid",
                "coalesce(tk.amount, 0)",
            )
        } else {
            (
                " LEFT JOIN players pl ON pl.fight_id = m.fight_id AND pl.guid = m.guid",
                "coalesce(pl.taken, 0)",
            )
        };
        let misses: Vec<String> = wowdps_model::MissKind::ALL
            .iter()
            .map(|k| format!("m.rec.misses.{} AS {}", k.name(), k.name()))
            .collect();
        let miss_sum: Vec<String> = wowdps_model::MissKind::ALL
            .iter()
            .map(|k| format!("m.rec.misses.{}", k.name()))
            .collect();
        // `mitigated` and `mitigated_pct` are the model's own
        // (`Mitigation::mitigated`, `wowdps_model::mitigated_pct`) — one
        // column each, so no reader has to reassemble them.
        self.rows_have_mitigation = self.probe_view(
            "mitigation",
            &format!(
                "WITH mit AS (\
                   SELECT r.id AS fight_id, x.guid AS guid, x.record AS rec, x.other AS o, \
                          x.other_sources AS os \
                   FROM rows r, unnest(r.mitigation) AS u(x)\
                 ), j AS (\
                   SELECT m.fight_id, m.guid, \
                          m.rec.absorbed AS absorbed, m.rec.blocked AS blocked, \
                          m.rec.absorbed_full AS absorbed_full, \
                          m.rec.blocked_full AS blocked_full, \
                          m.rec.stagger AS stagger, m.rec.stagger_ticked AS stagger_ticked, \
                          {}, ({}) AS misses, \
                          m.o.amount AS other_amount, m.o.extra AS other_extra, \
                          m.o.count AS other_count, m.o.n AS other_n, \
                          m.os.amount AS other_sources_amount, \
                          m.os.extra AS other_sources_extra, \
                          m.os.count AS other_sources_count, m.os.n AS other_sources_n, \
                          {taken_expr} AS taken \
                   FROM mit m{join}\
                 ) \
                 SELECT j.*, \
                        absorbed_full + blocked_full AS prevented, \
                        absorbed + blocked + absorbed_full + blocked_full AS mitigated, \
                        CASE WHEN taken + absorbed_full + blocked_full = 0 THEN 0.0 \
                             ELSE (absorbed + blocked + absorbed_full + blocked_full) * 100.0 \
                                  / (taken + absorbed_full + blocked_full) END AS mitigated_pct \
                 FROM j",
                misses.join(", "),
                miss_sum.join(" + "),
            ),
            &["guid", "absorbed", "other_amount", "other_sources_amount"],
        );
        for name in ["taken_spells", "taken_sources"] {
            self.probe_view(
                name,
                &format!(
                    "SELECT r.id AS fight_id, x.guid AS guid, {} \
                     FROM rows r, unnest(r.mitigation) AS u(x), unnest(x.{name}) AS v(s)",
                    row_cols("s", false)
                ),
                &["key", "amount"],
            );
        }
    }

    /// R19 (step 3b): the supporters' blocks out of the rows tier — one
    /// `support` row per fight × supporter with the four share sums, and
    /// their per-target table as `support_targets` (the meter's
    /// `support_targets` rows: `target` is the buffed owner's guid, and the
    /// damage-shaped row's `amount` / `extra` / `count` are named for what
    /// they hold — `damage` / `healing` / `lines` — never `extra` /
    /// `count`). Both probed: the list is `[]` on every Augmentation-less
    /// fight and absent on a pre-3b rows file, and DuckDB types either
    /// shape as JSON when no file carries a block.
    fn define_support_views(&mut self) {
        self.probe_view(
            "support",
            "SELECT r.id AS fight_id, s.guid AS guid, \
                    s.given.damage AS given_damage, s.given.healing AS given_healing, \
                    s.received.damage AS received_damage, \
                    s.received.healing AS received_healing \
             FROM rows r, unnest(r.support) AS u(s)",
            &["guid", "given_damage", "received_damage"],
        );
        self.probe_view(
            "support_targets",
            "SELECT r.id AS fight_id, s.guid AS guid, t.key AS target, t.label AS name, \
                    t.amount AS damage, t.extra AS healing, t.count AS lines, \
                    t.class AS class, t.spec AS spec \
             FROM rows r, unnest(r.support) AS u(s), unnest(s.targets) AS v(t)",
            &["guid", "target", "damage"],
        );
    }

    /// R18 (step 4b): the aura-uptime rollup and the coarse series out of
    /// the rows tier. `uptime` is one row per fight × TARGET × cell —
    /// `guid` is the buffed player, `src` the caster, `kind` the mark
    /// kind's NAME (`external`, `active_mitigation`, `support_buff`, …) —
    /// so "externals given, to whom" is `WHERE src = ? AND kind =
    /// 'external'`. `coarse` is one row per fight × friendly player with
    /// the 10 s `taken10` / `heal10` lists (cast to `BIGINT[]`: an
    /// all-empty list column types `JSON[]`, and the cast is what gives an
    /// aura-less lake typed columns) and the mark list, unnested per
    /// query. Both probed: the lists are `[]` on a fight with no role aura
    /// and absent on a pre-4b rows file, and neither shape types.
    fn define_span_views(&mut self) {
        self.rows_have_uptime_key = self.sql("SELECT uptime FROM rows LIMIT 0").is_ok();
        self.probe_view(
            "uptime",
            "SELECT r.id AS fight_id, x.guid AS guid, c.spell_id AS spell_id, \
                    c.label AS label, c.kind AS kind, c.src AS src, c.count AS count, \
                    c.total_ms AS total_ms \
             FROM rows r, unnest(r.uptime) AS u(x), unnest(x.cells) AS v(c)",
            &["guid", "spell_id", "total_ms"],
        );
        self.probe_view(
            "coarse",
            "SELECT r.id AS fight_id, c.guid AS guid, c.taken10::BIGINT[] AS taken10, \
                    c.heal10::BIGINT[] AS heal10, c.marks AS marks \
             FROM rows r, unnest(r.coarse) AS u(c)",
            &["guid", "taken10"],
        );
    }

    /// Run one statement and collect its result.
    pub fn sql(&self, query: &str) -> Result<Table, String> {
        self.sql_with(query, &[])
    }

    /// `query` with `?` placeholders bound to `params` in order — the way to
    /// get a string literal through three quoting layers, and the way a
    /// tool hands an LLM's values to SQL without splicing them in. JSON
    /// scalars only: null, bool, number, string.
    pub fn sql_with(&self, query: &str, params: &[Json]) -> Result<Table, String> {
        let values: Vec<Value> = params.iter().map(param_value).collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare(query).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(duckdb::params_from_iter(values.iter()))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let mut columns: Vec<String> = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if columns.is_empty() {
                let stmt = row.as_ref();
                columns = stmt.column_names();
            }
            let mut cells = Vec::with_capacity(columns.len());
            for i in 0..columns.len() {
                let v: Value = row.get(i).map_err(|e| e.to_string())?;
                cells.push(value_json(v));
            }
            out.push(cells);
        }
        if columns.is_empty() {
            columns = stmt.column_names();
        }
        Ok(Table { columns, rows: out })
    }

    /// The fixed questions in SQL — the parity gate's other half.
    pub fn best_kill(&self, encounter: u32, difficulty: u32) -> Result<Table, String> {
        self.sql(&format!(
            "SELECT id, name, duration_ms, start_utc_ms FROM fights \
             WHERE encounter.id = {encounter} AND encounter.difficulty = {difficulty} \
             AND success AND NOT aborted ORDER BY duration_ms, start_utc_ms LIMIT 1"
        ))
    }

    pub fn progression(&self, encounter: u32, difficulty: u32) -> Result<Table, String> {
        self.sql(&format!(
            "SELECT (start_utc_ms // 86400000) * 86400000 AS day_utc_ms, count(*) AS pulls, \
             bool_or(coalesce(success, false)) AS kill FROM fights \
             WHERE encounter.id = {encounter} AND encounter.difficulty = {difficulty} \
             AND NOT aborted GROUP BY day_utc_ms ORDER BY day_utc_ms"
        ))
    }

    pub fn trend(&self, guid: &str, healing: bool, limit: u32) -> Result<Table, String> {
        let col = if healing { "hps" } else { "dps" };
        self.sql(&format!(
            "SELECT fight_id, start_utc_ms, spec, {col} AS per_sec, duration_ms FROM players \
             WHERE guid = '{}' AND NOT aborted ORDER BY start_utc_ms DESC LIMIT {limit}",
            guid.replace('\'', "''")
        ))
    }

    /// Counts per directory — what the lake costs.
    pub fn stats(&self) -> Json {
        let count = |sub: &str| -> (u64, u64) {
            std::fs::read_dir(self.dir.join(sub))
                .map(|d| {
                    d.flatten().fold((0u64, 0u64), |(n, bytes), e| {
                        (n + 1, bytes + e.metadata().map(|m| m.len()).unwrap_or(0))
                    })
                })
                .unwrap_or((0, 0))
        };
        let mut o = Vec::new();
        for sub in DIRS {
            let (n, bytes) = count(sub);
            o.push((
                sub.to_string(),
                obj! { "files": Json::u64(n), "bytes": Json::u64(bytes) },
            ));
        }
        obj! {
            "dir": Json::str(self.dir.display().to_string()),
            "views": Json::Arr(self.views.iter().map(|v| Json::str(*v)).collect()),
            "directories": Json::Obj(o),
            "cards_without_role": Json::u64(self.cards_without_role()),
            "cards_without_taken": Json::u64(self.cards_without_taken()),
            "rows_without_mitigation": Json::u64(self.rows_without_mitigation()),
            "cards_without_overheal": Json::u64(self.cards_without_overheal()),
            "cards_without_am_uptime": Json::u64(self.cards_without_am_uptime()),
            "rows_without_uptime": Json::u64(self.rows_without_uptime()),
        }
    }

    /// R18 (step 4b): cards written before the span scalars — some player
    /// has a spec and no stored `am_uptime_ms` (the key absent; a stored 0
    /// is present) — what `regrade` would fill in. 0 on a fresh lake;
    /// every card on a PR #23 one. `players` reads such a card's
    /// `am_uptime_pct_sql` as 0, which is "not recorded", never "no
    /// mitigation".
    fn cards_without_am_uptime(&self) -> u64 {
        if !self.views.contains(&"fights") {
            return 0;
        }
        let stored = if self.players_have_spans {
            "p.am_uptime_ms IS NULL"
        } else {
            "true"
        };
        self.sql(&format!(
            "SELECT count(*) FROM fights WHERE list_bool_or(list_transform(players, \
             p -> p.spec IS NOT NULL AND {stored}))"
        ))
        .ok()
        .and_then(|t| t.rows.first()?.first()?.as_u64())
        .unwrap_or(0)
    }

    /// Rows files with no `uptime` key — every one of them when no file
    /// carries the key, else the ones `union_by_name` filled with NULL. An
    /// empty list is NOT counted: a fight with no role aura stores `[]`,
    /// and that is its answer (the `uptime` view may still be undefined
    /// when every file's list is empty — `views` says so).
    fn rows_without_uptime(&self) -> u64 {
        if !self.views.contains(&"rows") {
            return 0;
        }
        let query = if self.rows_have_uptime_key {
            "SELECT count(*) FROM rows WHERE uptime IS NULL"
        } else {
            "SELECT count(*) FROM rows"
        };
        self.sql(query)
            .ok()
            .and_then(|t| t.rows.first()?.first()?.as_u64())
            .unwrap_or(0)
    }

    /// R19 (step 3b): cards written before the healing split and the
    /// support scalars — some player has a spec and no stored `overheal`
    /// (the key absent; a stored 0 is present) — what `regrade` would fill
    /// in. 0 on a fresh lake; every card on a PR #19 one. `players` reads
    /// such a card's `effective_dps_sql` as its `dps`, but nothing can
    /// recover its overheal or its support from the card alone.
    fn cards_without_overheal(&self) -> u64 {
        if !self.views.contains(&"fights") {
            return 0;
        }
        let stored = if self.players_have_support {
            "p.overheal IS NULL"
        } else {
            "true"
        };
        self.sql(&format!(
            "SELECT count(*) FROM fights WHERE list_bool_or(list_transform(players, \
             p -> p.spec IS NOT NULL AND {stored}))"
        ))
        .ok()
        .and_then(|t| t.rows.first()?.first()?.as_u64())
        .unwrap_or(0)
    }

    /// R17 (step 2b): cards written before the tank measures — some player
    /// has a spec and no stored `taken` — what `regrade` would fill in. 0
    /// on a fresh lake; every card on a PR #16 one. Unlike `role`, nothing
    /// derives these from the card alone, so the count is the whole story
    /// of what `history` / `trend` cannot answer yet.
    fn cards_without_taken(&self) -> u64 {
        if !self.views.contains(&"fights") {
            return 0;
        }
        let stored = if self.players_have_taken {
            "p.taken IS NULL"
        } else {
            "true"
        };
        self.sql(&format!(
            "SELECT count(*) FROM fights WHERE list_bool_or(list_transform(players, \
             p -> p.spec IS NOT NULL AND {stored}))"
        ))
        .ok()
        .and_then(|t| t.rows.first()?.first()?.as_u64())
        .unwrap_or(0)
    }

    /// Rows files with no `mitigation` key — every one of them when the
    /// `mitigation` view could not be defined at all, else the ones
    /// `union_by_name` filled with NULL.
    fn rows_without_mitigation(&self) -> u64 {
        if !self.views.contains(&"rows") {
            return 0;
        }
        let query = if self.rows_have_mitigation {
            "SELECT count(*) FROM rows WHERE mitigation IS NULL"
        } else {
            "SELECT count(*) FROM rows"
        };
        self.sql(query)
            .ok()
            .and_then(|t| t.rows.first()?.first()?.as_u64())
            .unwrap_or(0)
    }

    /// Cards written before players carried `role` — some player has a
    /// spec and no stored role — what `regrade --kind all` would rewrite;
    /// 0 on a fresh lake. The `players` view answers for them from the
    /// spec regardless.
    fn cards_without_role(&self) -> u64 {
        if !self.views.contains(&"fights") {
            return 0;
        }
        let stored = if self.players_have_role {
            "p.role IS NULL"
        } else {
            "true"
        };
        self.sql(&format!(
            "SELECT count(*) FROM fights WHERE list_bool_or(list_transform(players, \
             p -> p.spec IS NOT NULL AND {stored}))"
        ))
        .ok()
        .and_then(|t| t.rows.first()?.first()?.as_u64())
        .unwrap_or(0)
    }

    /// One fight, self-contained: card + rows + details + annotations.
    pub fn export(&self, fight_id: &str) -> Result<Json, String> {
        let read = |sub: &str, ext: &str| -> Option<Json> {
            let text =
                std::fs::read_to_string(self.dir.join(sub).join(format!("{fight_id}.{ext}")))
                    .ok()?;
            if ext == "ndjson" {
                Some(Json::Arr(
                    text.lines()
                        .filter_map(|l| wowdps_proto::json::parse(l).ok())
                        .collect(),
                ))
            } else {
                wowdps_proto::json::parse(&text).ok()
            }
        };
        let card = read("fights", "json").ok_or_else(|| format!("no stored fight {fight_id}"))?;
        Ok(obj! {
            "fight": card,
            "rows": read("rows", "json").unwrap_or(Json::Null),
            "details": read("details", "json").unwrap_or(Json::Null),
            "annotations": read("annotations", "ndjson").unwrap_or(Json::Arr(Vec::new())),
        })
    }

    /// Copy every view into `cache.duckdb` beside the lake, so a repeated
    /// question costs a table scan instead of a JSON parse.
    pub fn materialize(&self) -> Result<PathBuf, String> {
        let target = self.dir.join("cache.duckdb");
        let tmp = self.dir.join("cache.duckdb.tmp");
        let _ = std::fs::remove_file(&tmp);
        let path = tmp.display().to_string().replace('\'', "''");
        self.conn
            .execute_batch(&format!("ATTACH '{path}' AS cache;"))
            .map_err(|e| e.to_string())?;
        for view in &self.views {
            self.conn
                .execute_batch(&format!(
                    "CREATE OR REPLACE TABLE cache.{view} AS SELECT * FROM {view};"
                ))
                .map_err(|e| e.to_string())?;
        }
        self.conn
            .execute_batch("DETACH cache;")
            .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        Ok(target)
    }
}

/// A JSON scalar as a DuckDB parameter value.
fn param_value(v: &Json) -> Result<Value, String> {
    Ok(match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Str(s) => Value::Text(s.clone()),
        Json::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                Value::BigInt(*n as i64)
            } else {
                Value::Double(*n)
            }
        }
        other => return Err(format!("parameter {} is not a scalar", other.to_line())),
    })
}

/// A DuckDB value as JSON. Integers that fit stay numbers; 64-bit ones
/// beyond 2^53 and everything exotic become strings, never a lossy float.
pub fn value_json(v: Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Boolean(b) => Json::Bool(b),
        Value::TinyInt(n) => Json::num(n),
        Value::SmallInt(n) => Json::num(n),
        Value::Int(n) => Json::num(n),
        Value::BigInt(n) => {
            if n.abs() < (1i64 << 53) {
                Json::num(n as f64)
            } else {
                Json::str(n.to_string())
            }
        }
        Value::UTinyInt(n) => Json::num(n),
        Value::USmallInt(n) => Json::num(n),
        Value::UInt(n) => Json::num(n),
        Value::UBigInt(n) => {
            if n < (1u64 << 53) {
                Json::num(n as f64)
            } else {
                Json::str(n.to_string())
            }
        }
        // `sum()` over an integer column is a HUGEINT — every Σ the parity
        // gate takes lands here, so the same exactness rule as the 64-bit
        // ones: a number while an f64 holds it, else its decimal text.
        Value::HugeInt(n) => {
            if n.unsigned_abs() < (1u128 << 53) {
                Json::num(n as f64)
            } else {
                Json::str(n.to_string())
            }
        }
        Value::Float(f) => Json::num(f),
        Value::Double(f) => Json::num(f),
        Value::Text(s) => Json::str(s),
        Value::List(items) => Json::Arr(items.into_iter().map(value_json).collect()),
        Value::Struct(fields) => Json::Obj(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), value_json(v.clone())))
                .collect(),
        ),
        other => Json::str(format!("{other:?}")),
    }
}
