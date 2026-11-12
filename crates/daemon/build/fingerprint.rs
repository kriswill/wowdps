//! The sweep's source fingerprint, its machinery: `build.rs` names what is
//! hashed (and what decides) and runs it; `tests/fingerprint.rs` runs it
//! over trees of its own. See `build.rs` for what is hashed and why.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A unit of hashed source, relative to the crate's directory.
pub enum Unit {
    /// A whole directory.
    Tree(&'static str),
    /// A module: `<path>.rs` or `<path>/mod.rs`, and all under `<path>/`.
    Module(&'static str),
}

/// What [`hashed`] found: every hashed file by its name relative to the
/// crate, sorted, with its text less its test modules; and the folders a
/// build script watches for them.
pub struct Hashed {
    pub files: Vec<(String, String)>,
    pub watched: BTreeSet<PathBuf>,
}

/// Every file of `sources` under `root` but `left_out`, read and cut of its
/// test code. A unit holding no source or a left-out file that is not one
/// of them is an error: a rename the lists must follow.
pub fn hashed(root: &Path, sources: &[Unit], left_out: &[&str]) -> Result<Hashed, String> {
    let mut files = BTreeSet::new();
    let mut watched = BTreeSet::new();
    for unit in sources {
        let found = match unit {
            Unit::Tree(dir) => {
                let path = root.join(dir);
                watched.insert(path.clone());
                collect(&path, &mut files)?
            }
            Unit::Module(module) => {
                let (file, dir) = (root.join(format!("{module}.rs")), root.join(module));
                // The folder the module sits in, which always exists: a
                // path that does not would rerun this on every build, and
                // the folder sees the module's file or directory appear.
                watched.insert(dir.parent().unwrap_or(root).to_path_buf());
                let mut any = false;
                if regular(&file) {
                    files.insert(file);
                    any = true;
                }
                any | collect(&dir, &mut files)?
            }
        };
        if !found {
            let name = match unit {
                Unit::Tree(p) | Unit::Module(p) => p,
            };
            return Err(format!(
                "swept: `{name}` in build.rs's SOURCES holds no source; follow its rename"
            ));
        }
    }
    for file in left_out {
        let path = root.join(file);
        if !files.remove(&path) {
            return Err(format!(
                "swept: `{file}` in build.rs's LEFT_OUT is not a hashed source; follow its rename"
            ));
        }
    }
    // Read every file, cut its test modules, and leave out the files of
    // its out-of-line test modules (`#[cfg(test)] mod x;`).
    let mut texts = Vec::new();
    let mut tests = BTreeSet::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| format!("swept: cannot read {}: {e}", file.display()))?;
        let (kept, out_of_line) = strip_tests(&text);
        let dir = module_dir(file);
        for name in out_of_line {
            tests.insert(dir.join(format!("{name}.rs")));
            tests.insert(dir.join(&name));
        }
        texts.push((file.clone(), kept));
    }
    let files = texts
        .into_iter()
        .filter(|(file, _)| !tests.iter().any(|t| file.starts_with(t)))
        .map(|(file, text)| {
            let name = file.strip_prefix(root).unwrap_or(&file);
            (name.to_string_lossy().into_owned(), text)
        })
        .collect();
    Ok(Hashed { files, watched })
}

/// Each of `deciding` must be in the hashed text: one that is not moved
/// somewhere the fingerprint cannot see (or was renamed).
pub fn check_deciding(files: &[(String, String)], deciding: &[&str]) -> Result<(), String> {
    match deciding
        .iter()
        .find(|needle| !files.iter().any(|(_, text)| text.contains(**needle)))
    {
        Some(needle) => Err(format!(
            "swept: `{needle}` decides what a read of a log stores, and is not in the \
             sweep's hashed sources: add the unit it moved into to build.rs's SOURCES \
             (or follow its rename in DECIDING)"
        )),
        None => Ok(()),
    }
}

/// FNV-1a over each file's name and text, in order.
pub fn fingerprint(files: &[(String, String)]) -> u64 {
    let mut hash = Fnv::new();
    for (name, text) in files {
        hash.write(name.as_bytes());
        hash.write(&[0]);
        hash.write(&(text.len() as u64).to_le_bytes());
        hash.write(text.as_bytes());
    }
    hash.0
}

/// A regular file (through a symlink), not a dot-file.
fn regular(path: &Path) -> bool {
    !dotted(path) && std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

fn dotted(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// Every `.rs` regular file under `dir`, dot-entries and directory links
/// skipped; whether any was found.
fn collect(dir: &Path, out: &mut BTreeSet<PathBuf>) -> Result<bool, String> {
    // Only a folder that is not there is absent: one that cannot be read
    // fails, rather than leaving its sources out unseen.
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("swept: cannot list {}: {e}", dir.display())),
    };
    let mut any = false;
    for entry in entries {
        let entry = entry.map_err(|e| format!("swept: cannot list {}: {e}", dir.display()))?;
        let path = entry.path();
        if dotted(&path) {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|e| format!("swept: cannot stat {}: {e}", path.display()))?;
        if kind.is_dir() {
            any |= collect(&path, out)?;
        } else if path.extension().is_some_and(|x| x == "rs") && regular(&path) {
            out.insert(path);
            any = true;
        }
    }
    Ok(any)
}

/// Where a file's child modules live: beside it for `mod.rs`, `lib.rs`
/// and `main.rs`, else in the directory named for it.
fn module_dir(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or(file).to_path_buf();
    match file.file_name().and_then(|n| n.to_str()) {
        Some("mod.rs" | "lib.rs" | "main.rs") => parent,
        _ => file.with_extension(""),
    }
}

/// `text` less a top-level `#[cfg(test)] mod x { … }` block that is the
/// file's last item, and the names of its `#[cfg(test)] mod x;`
/// declarations. The block is cut only as rustfmt writes one: every line
/// between its head and a `}` alone in the first column blank or indented,
/// and nothing but blank lines after that `}` (clippy's
/// `items_after_test_module` holds every test module there). Any other
/// block is kept whole.
///
/// This reads lines, not Rust. A production string or comment cannot be
/// cut (its closing quote or `*/` would follow the `}`). What could be is
/// the end of an item rustfmt leaves alone — a `macro_rules!` body, an item
/// under `#[rustfmt::skip]` — that closes the file and holds first-column
/// `#[cfg(test)]` and `mod x {` lines of its own. No hashed file has one.
pub fn strip_tests(text: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut kept = String::with_capacity(text.len());
    let mut out_of_line = Vec::new();
    let mut i = 0;
    while let Some(line) = lines.get(i) {
        if line.trim_end() == "#[cfg(test)]"
            && let Some(next) = lines.get(i + 1).map(|l| l.trim_end())
            && let Some(rest) = next.strip_prefix("mod ")
        {
            if let Some(name) = rest.strip_suffix(';').filter(|n| ident(n)) {
                out_of_line.push(name.to_string());
            } else if rest.strip_suffix(" {").is_some_and(ident) {
                let mut j = i + 2;
                while lines
                    .get(j)
                    .is_some_and(|l| l.trim().is_empty() || l.starts_with([' ', '\t']))
                {
                    j += 1;
                }
                let last = lines
                    .get(j + 1..)
                    .is_some_and(|rest| rest.iter().all(|l| l.trim().is_empty()));
                if last && lines.get(j).is_some_and(|l| l.trim_end() == "}") {
                    i = j + 1;
                    continue;
                }
            }
        }
        kept.push_str(line);
        i += 1;
    }
    (kept, out_of_line)
}

fn ident(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// FNV-1a, as the store hashes everything.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
