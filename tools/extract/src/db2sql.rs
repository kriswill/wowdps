//! The game's tables as SQL (`wowdps-extract sql`, `wowdps-db2` in the
//! dev shell): a table named as WoWDBDefs names it is found by its
//! FileDataID in WoWDBDefs' `manifest.json`, read from the install's own
//! CASC storage with its `.dbd` schema, decoded to CSV, copied to Parquet
//! by DuckDB, and kept per game build under
//! `$XDG_CACHE_HOME/wowdps/db2/<build>/`. Each session opens DuckDB in
//! memory with a view per table the build's cache holds, so a table asked
//! for once is there in every later session, and no two sessions share a
//! lock. The manifest, the schemas and the TACT keys are fetched with
//! `curl` and kept beside the builds (the manifest and keys refreshed after
//! a week); nothing else touches the network.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// WoWDBDefs' manifest of every table: its name and FileDataID.
pub const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/manifest.json";
/// A table's schema, by its name.
pub const DBD_URL: &str = "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/";
/// The community's TACT keys, for the encrypted parts of a table.
pub const KEYS_URL: &str = "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt";
/// How old the manifest and the keys may grow before they are fetched again.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Every table WoWDBDefs knows a FileDataID for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub tables: Vec<(String, u32)>,
}

/// The string value of `"key": "…"` in one flat JSON object.
fn field<'a>(object: &'a str, key: &str) -> Option<&'a str> {
    let at = object.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = object
        .get(at..)?
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    match rest.strip_prefix('"') {
        Some(s) => s.split('"').next(),
        None => rest.split([',', '}', '\n']).next().map(str::trim),
    }
}

impl Manifest {
    /// WoWDBDefs' `manifest.json`: an array of flat objects, each with a
    /// `tableName` and (for a table the client ships) a `db2FileDataID`.
    pub fn parse(json: &str) -> Manifest {
        let mut tables: Vec<(String, u32)> = json
            .split('{')
            .skip(1)
            .filter_map(|o| {
                let o = o.split('}').next()?;
                let name = field(o, "tableName")?;
                let fdid = field(o, "db2FileDataID")?.parse().ok()?;
                Some((name.to_string(), fdid))
            })
            .collect();
        tables.sort();
        Manifest { tables }
    }

    /// A table by its name, any case: its name as WoWDBDefs spells it and
    /// its FileDataID.
    pub fn find(&self, name: &str) -> Option<(&str, u32)> {
        self.tables
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(n, f)| (n.as_str(), *f))
    }

    /// Every table whose name holds `word`, any case.
    pub fn search(&self, word: &str) -> Vec<(&str, u32)> {
        let word = word.to_lowercase();
        self.tables
            .iter()
            .filter(|(n, _)| n.to_lowercase().contains(&word))
            .map(|(n, f)| (n.as_str(), *f))
            .collect()
    }
}

/// The cache: `$XDG_CACHE_HOME/wowdps/db2`, else `~/.cache/wowdps/db2`
/// (`wowdps_proto::dirs`).
pub fn cache_root() -> Result<PathBuf, String> {
    wowdps_proto::dirs::cache_home()
        .map(|d| d.join("wowdps").join("db2"))
        .ok_or_else(|| "no XDG_CACHE_HOME or HOME".to_string())
}

/// A build's place in the cache: its version with anything but digits,
/// letters, dots and dashes made a dash.
pub fn build_dir(root: &Path, build: &str) -> PathBuf {
    let safe: String = build
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if safe.chars().all(|c| c == '.') {
        return root.join("unknown");
    }
    root.join(safe)
}

/// Whether `path` is missing or older than `age`.
pub fn stale(path: &Path, age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|e| e > age)
}

/// `text` as a single-quoted SQL string.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// `name` as a quoted SQL identifier.
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The statement that copies a table's CSV into Parquet, its columns typed
/// from every row (`sample_size = -1`: a column a sample would call text
/// stays a number).
pub fn to_parquet(csv: &Path, parquet: &Path) -> String {
    format!(
        "copy (select * from read_csv({}, header = true, sample_size = -1)) to {} (format parquet);",
        quoted(&csv.to_string_lossy()),
        quoted(&parquet.to_string_lossy())
    )
}

/// The statements that open a session: a view per table, named as the
/// table is, over its Parquet file.
pub fn views(tables: &[(String, PathBuf)]) -> String {
    tables
        .iter()
        .map(|(name, parquet)| {
            format!(
                "create view {} as select * from read_parquet({});",
                ident(name),
                quoted(&parquet.to_string_lossy())
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The tables a build's cache holds: each `parquet/<Name>.parquet`.
pub fn cached(build: &Path) -> Vec<(String, PathBuf)> {
    let dir = build.join("parquet");
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path
                .file_name()?
                .to_str()?
                .strip_suffix(".parquet")?
                .to_string();
            Some((name, path))
        })
        .collect();
    out.sort();
    out
}

/// Fetch `url` into `dest` with curl, whole or not at all.
pub fn fetch(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = crate::atomic::tmp_beside(dest);
    let status = std::process::Command::new("curl")
        .args(["-sfL", "--max-time", "120", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .map_err(|e| format!("curl: {e} (the gen-* scripts need it too)"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{url}: curl failed ({status})"));
    }
    crate::atomic::rename_into(&tmp, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"[
  {
    "tableName": "Achievement",
    "tableHash": "D2EE2CA7",
    "dbcFileDataID": 841607,
    "db2FileDataID": 1260179
  },
  {
    "tableName": "Achievement_Criteria",
    "tableHash": "E3614CCD"
  },
  {
    "tableName": "CreatureDisplayInfo",
    "tableHash": "BFDAF9F1",
    "dbcFileDataID": 801605,
    "db2FileDataID": 1108759
  },
  {"tableName": "CreatureDisplayInfoExtra", "db2FileDataID": 1264997}
]"#;

    #[test]
    fn the_manifest_names_each_table_the_client_ships() {
        let m = Manifest::parse(JSON);
        // A table with no db2 file is not one the install can give.
        assert_eq!(m.tables.len(), 3);
        assert_eq!(
            m.find("creaturedisplayinfo"),
            Some(("CreatureDisplayInfo", 1108759))
        );
        assert_eq!(m.find("Achievement_Criteria"), None);
        assert_eq!(m.find("Creature"), None);
        assert_eq!(
            m.search("DISPLAY"),
            vec![
                ("CreatureDisplayInfo", 1108759),
                ("CreatureDisplayInfoExtra", 1264997)
            ]
        );
    }

    #[test]
    fn statements_quote_what_they_name() {
        let csv = Path::new("/c/it's/T.csv");
        let pq = Path::new("/c/T.parquet");
        assert_eq!(
            to_parquet(csv, pq),
            "copy (select * from read_csv('/c/it''s/T.csv', header = true, sample_size = -1)) \
             to '/c/T.parquet' (format parquet);"
        );
        let v = views(&[
            ("Map".into(), PathBuf::from("/c/Map.parquet")),
            ("A\"b".into(), PathBuf::from("/c/x.parquet")),
        ]);
        assert_eq!(
            v,
            "create view \"Map\" as select * from read_parquet('/c/Map.parquet'); \
             create view \"A\"\"b\" as select * from read_parquet('/c/x.parquet');"
        );
    }

    #[test]
    fn a_build_is_its_own_directory() {
        let root = Path::new("/c");
        assert_eq!(
            build_dir(root, "12.0.5.66380"),
            Path::new("/c/12.0.5.66380")
        );
        assert_eq!(build_dir(root, "../x y"), Path::new("/c/..-x-y"));
        assert_eq!(build_dir(root, ".."), Path::new("/c/unknown"));
        assert_eq!(build_dir(root, ""), Path::new("/c/unknown"));
        assert!(stale(Path::new("/no/such/file"), MAX_AGE));
    }
}
