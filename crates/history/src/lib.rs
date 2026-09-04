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
use wowdps_proto::json::Json;
use wowdps_proto::obj;

/// Where the lake lives: `$XDG_DATA_HOME/wowdps/history/v1`, else
/// `~/.local/share/wowdps/history/v1` — the daemon's default too.
/// The lake's data directories — one view each, and the only places a
/// read-only lake may touch.
pub const DIRS: [&str; 5] = ["fights", "rows", "details", "loadouts", "annotations"];

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
/// holds files (`fights`, `players` — the cards' player lines unnested —
/// `rows`, `loadouts`, `annotations`).
pub struct Lake {
    dir: PathBuf,
    conn: Connection,
    views: Vec<&'static str>,
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
        let glob = |sub: &str, ext: &str| {
            format!(
                "'{}/{sub}/*.{ext}'",
                self.dir.display().to_string().replace('\'', "''")
            )
        };
        if self.has_files("fights", "json") {
            self.conn
                .execute_batch(&format!(
                    "CREATE VIEW fights AS SELECT * FROM read_json({}, format = 'auto', \
                     union_by_name = true);\n\
                     CREATE VIEW players AS SELECT f.id AS fight_id, f.kind, f.name AS fight, \
                     f.start_utc_ms, f.duration_ms, f.success, f.aborted, \
                     f.encounter.id AS encounter_id, f.encounter.difficulty AS difficulty, \
                     unnest(f.players, recursive := true) FROM fights f;",
                    glob("fights", "json")
                ))
                .map_err(|e| e.to_string())?;
            self.views.push("fights");
            self.views.push("players");
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
        }
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
