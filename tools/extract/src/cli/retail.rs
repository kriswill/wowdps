//! Retail's outputs, which another product's never take: the committed
//! tables and their review twins in a checkout, which the meter is built
//! from, and the caches in the data home, which the GUI and the mcp tools
//! read. A table or cache made from another edition's client is written
//! only where `-o` names another place (`retail_only`), checked before
//! the command reads anything.

use std::path::{Component, Path, PathBuf};

use super::install::Install;
use super::{caches, data_path, tablegen};
use wowdps_extract::game::default_product;

/// A file at a wowdps checkout's root that only a checkout holds: what
/// the dev shell's wrappers know a checkout by.
const CHECKOUT_MARK: &str = "crates/core/fixtures/check.awk";

/// How the refusal names a cache in the data home.
pub fn cache_place(name: &str) -> String {
    format!("the data home's {name}")
}

/// Refuse the place a command would write when `install` reads another
/// product than Retail's and that place is one of Retail's: its `-o` when
/// `out` resolves to a committed table or twin or to a data-home cache
/// (whatever its spelling, `..` or symlinks), or, with no `-o`, its
/// default `place`, which is always Retail's. Retail's product writes
/// wherever it did.
pub fn retail_only(install: &Install, out: Option<&Path>, place: &str) -> Result<(), String> {
    let retail = default_product();
    if install.product == retail {
        return Ok(());
    }
    let what = match out {
        None => place.to_string(),
        Some(out) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let caches: Vec<PathBuf> = caches::CACHES
                .iter()
                .filter_map(|name| data_path(name).ok())
                .collect();
            match retail_place(out, &cwd, &caches) {
                Some(found) => format!("-o {}: {found}", out.display()),
                None => return Ok(()),
            }
        }
    };
    Err(format!(
        "{what} is Retail's (product {retail}); product {} needs -o naming another place",
        install.product
    ))
}

/// The Retail output `out` (relative to `cwd`) resolves to, if any: a
/// committed table or review twin of a wowdps checkout, or one of
/// `caches`, compared as the file system resolves them.
fn retail_place(out: &Path, cwd: &Path, caches: &[PathBuf]) -> Option<String> {
    let out = resolved(out, cwd);
    for rel in tablegen::committed() {
        // The checkout `out` would lie in, were it this file of one.
        let root = out.ancestors().nth(rel.components().count());
        if out.ends_with(&rel) && root.is_some_and(|r| r.join(CHECKOUT_MARK).is_file()) {
            return Some(format!("the committed {}", rel.display()));
        }
    }
    for cache in caches {
        if resolved(cache, cwd) == out {
            let name = cache.file_name().unwrap_or_default().to_string_lossy();
            return Some(cache_place(&name));
        }
    }
    None
}

/// `path`, made absolute against `cwd`, as the file system names it: its
/// own symlinks resolved where it exists, else its directory's with its
/// name rejoined, else its `.` and `..` folded by name alone (nothing of
/// it exists to resolve).
fn resolved(path: &Path, cwd: &Path) -> PathBuf {
    let abs = cwd.join(path);
    if let Ok(p) = abs.canonicalize() {
        return p;
    }
    if let (Some(dir), Some(name)) = (abs.parent(), abs.file_name())
        && let Ok(dir) = dir.canonicalize()
    {
        return dir.join(name);
    }
    let mut folded = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            other => folded.push(other),
        }
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::Args;
    use crate::cli::{Cmd, cmd};

    /// This checkout's root, whose committed files the tests name (read,
    /// never written).
    fn checkout() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wowdps-retail-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn the_committed_files_are_every_table_and_twin() {
        let root = checkout();
        let committed = tablegen::committed();
        assert_eq!(committed.len(), 12, "8 tables, 4 review twins");
        for rel in &committed {
            assert!(root.join(rel).is_file(), "{}", rel.display());
        }
        assert!(root.join(CHECKOUT_MARK).is_file());
    }

    /// A committed file is found whatever spells it: absolute, relative
    /// to the checkout or to its own directory, through `..`, through a
    /// symlinked directory; and every table's and twin's, not one alone.
    #[test]
    fn a_committed_file_is_found_however_it_is_spelled() {
        let root = checkout();
        let src = root.join("crates/core/src");
        let found = |out: &str, cwd: &Path| retail_place(Path::new(out), cwd, &[]);
        let class = Some("the committed crates/core/src/class_spells.rs".to_string());
        let abs = src.join("class_spells.rs");
        assert_eq!(retail_place(&abs, Path::new("/"), &[]), class);
        assert_eq!(found("crates/core/src/class_spells.rs", &root), class);
        assert_eq!(found("class_spells.rs", &src), class);
        assert_eq!(
            found("crates/core/src/../src/class_spells.rs", &root),
            class
        );
        assert_eq!(
            found("./crates/core/../core/src/./class_spells.rs", &root),
            class
        );
        assert_eq!(
            found("crates/core/src/item_spells.rs", &root),
            Some("the committed crates/core/src/item_spells.rs".into())
        );
        assert_eq!(
            found("crates/core/src/proc_spells.expected.md", &root),
            Some("the committed crates/core/src/proc_spells.expected.md".into())
        );
        assert_eq!(
            found("crates/proto/src/placed/placed_spells.rs", &root),
            Some("the committed crates/proto/src/placed/placed_spells.rs".into())
        );
        let dir = scratch("link");
        std::os::unix::fs::symlink(&src, dir.join("src")).unwrap();
        assert_eq!(found("src/class_spells.rs", &dir), class);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Anywhere else is not Retail's: a file of the same name elsewhere, a
    /// path shaped like a checkout's that is no checkout, a path in no
    /// directory that exists, a source file of the checkout no generator
    /// writes.
    #[test]
    fn elsewhere_is_not_retails() {
        let dir = scratch("elsewhere");
        let none = |out: &Path| retail_place(out, Path::new("/"), &[]);
        assert_eq!(none(&dir.join("class_spells.rs")), None);
        let shaped = dir.join("crates/core/src");
        std::fs::create_dir_all(&shaped).unwrap();
        assert_eq!(
            none(&shaped.join("class_spells.rs")),
            None,
            "no checkout mark"
        );
        assert_eq!(
            none(&dir.join("absent/crates/core/src/class_spells.rs")),
            None
        );
        assert_eq!(none(&checkout().join("crates/core/src/lib.rs")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A cache in the data home is found however it is spelled, whether
    /// its directory exists or not; another file beside it is not one.
    #[test]
    fn a_data_home_cache_is_found_however_it_is_spelled() {
        let dir = scratch("caches");
        let home = dir.join("data/wowdps");
        let caches: Vec<PathBuf> = caches::CACHES.iter().map(|n| home.join(n)).collect();
        let found = |out: PathBuf| retail_place(&out, Path::new("/"), &caches);
        for exists in [false, true] {
            if exists {
                std::fs::create_dir_all(&home).unwrap();
                std::fs::write(home.join("spell-icons.bin"), b"x").unwrap();
            }
            assert_eq!(
                found(home.join("spell-icons.bin")),
                Some("the data home's spell-icons.bin".into()),
                "exists {exists}"
            );
            assert_eq!(
                found(home.join("../wowdps/talents.json")),
                Some("the data home's talents.json".into()),
                "exists {exists}"
            );
            assert_eq!(found(home.join("other.bin")), None, "exists {exists}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    const CMD: Cmd = cmd("cmd", "cmd [-o OUT]", |_| Ok(()));

    fn install(product: &str) -> Install {
        let words = vec!["--product".to_string(), product.to_string()];
        Install::files(&mut Args::new("bin", words, &CMD)).unwrap()
    }

    /// Retail's product writes wherever it did; another is refused no
    /// `-o` and an `-o` that is Retail's, in words that read right for
    /// both, and let through to anywhere else.
    #[test]
    fn only_another_product_is_refused_and_only_retails_places() {
        let committed = checkout().join("crates/core/src/class_spells.rs");
        let place = "the committed crates/core/src/class_spells.rs";
        assert!(retail_only(&install("wow"), None, place).is_ok());
        assert!(retail_only(&install("wow"), Some(&committed), place).is_ok());
        let tail = "is Retail's (product wow); product wowf needs -o naming another place";
        assert_eq!(
            retail_only(&install("wowf"), None, place).unwrap_err(),
            format!("{place} {tail}")
        );
        let e = retail_only(&install("wowf"), Some(&committed), place).unwrap_err();
        assert_eq!(e, format!("-o {}: {place} {tail}", committed.display()));
        let dir = scratch("allowed");
        assert!(retail_only(&install("wowf"), Some(&dir.join("class_spells.rs")), place).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
