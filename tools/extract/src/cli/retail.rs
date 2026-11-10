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

/// Refuse the places a command would write when `install` reads another
/// product than Retail's and one of them is Retail's: its `-o` when `out`
/// resolves to a committed table or twin or to a data-home cache (whatever
/// its spelling, `..` or symlinks), or `beside`, the file it writes next
/// to `out` and named from it (a table's review twin), likewise; or, with
/// no `-o`, its default `place`, which is always Retail's. Retail's
/// product writes wherever it did.
pub fn retail_only(
    install: &Install,
    out: Option<&Path>,
    beside: Option<&Path>,
    place: &str,
) -> Result<(), String> {
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
            let own = retail_place(out, &cwd, &caches).map(|found| (found, None));
            let twin = beside.and_then(|b| retail_place(b, &cwd, &caches).map(|f| (f, Some(b))));
            match own.or(twin) {
                Some((found, None)) => format!("-o {}: {found}", out.display()),
                Some((found, Some(b))) => format!(
                    "-o {} writes {} beside it: {found}",
                    out.display(),
                    b.display()
                ),
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

/// `path`, made absolute against `cwd`, where the file system would put
/// it: walked a component at a time as the kernel walks it, each one
/// that exists resolved with its symlinks (so the path so far is always
/// the real one, and a `..` steps to its real parent), each one that does
/// not exist kept by name, as the directory a command creates there will
/// be. A symlink above a missing component, or a `..` after one, so lands
/// where the write would.
fn resolved(path: &Path, cwd: &Path) -> PathBuf {
    let mut real = PathBuf::new();
    for c in cwd.join(path).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                real.pop();
            }
            Component::Normal(name) => {
                real.push(name);
                if let Ok(p) = real.canonicalize() {
                    real = p;
                }
            }
            root => real.push(root),
        }
    }
    real
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

    /// A symlink above a component that does not exist yet still counts:
    /// `link/missing/../talents.json`, with `link` the data home's wowdps
    /// directory, is where the write lands once `missing` is made. And a
    /// data home reached through a symlink, its `wowdps/` not made yet, is
    /// the same place by either spelling, from either side.
    #[test]
    fn a_symlink_above_a_missing_component_is_followed() {
        let dir = scratch("missing");
        let home = dir.join("data/wowdps");
        std::fs::create_dir_all(&home).unwrap();
        let caches: Vec<PathBuf> = caches::CACHES.iter().map(|n| home.join(n)).collect();
        std::os::unix::fs::symlink(&home, dir.join("s")).unwrap();
        let out = dir.join("s/missing/../talents.json");
        assert_eq!(
            retail_place(&out, Path::new("/"), &caches),
            Some("the data home's talents.json".into())
        );
        // `..` after a missing component, then into a symlinked directory.
        let src = checkout().join("crates/core/src");
        std::os::unix::fs::symlink(&src, dir.join("src")).unwrap();
        assert_eq!(
            retail_place(&dir.join("gone/../src/role_spells.rs"), Path::new("/"), &[]),
            Some("the committed crates/core/src/role_spells.rs".into())
        );

        let real = dir.join("realhome");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, dir.join("linkhome")).unwrap();
        let by = |home: &str| -> Vec<PathBuf> {
            caches::CACHES
                .iter()
                .map(|n| dir.join(home).join("wowdps").join(n))
                .collect()
        };
        for (cached, written) in [("linkhome", "realhome"), ("realhome", "linkhome")] {
            let out = dir.join(written).join("wowdps/talents.json");
            assert_eq!(
                retail_place(&out, Path::new("/"), &by(cached)),
                Some("the data home's talents.json".into()),
                "caches under {cached}, -o under {written}"
            );
        }
        assert!(!real.join("wowdps").exists(), "nothing made");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Retail's product writes wherever it did; another is refused no
    /// `-o`, an `-o` that is Retail's, and an `-o` whose twin beside it
    /// would be, in words that read right for each, and let through to
    /// anywhere else.
    #[test]
    fn only_another_product_is_refused_and_only_retails_places() {
        let root = checkout();
        let committed = root.join("crates/core/src/class_spells.rs");
        let place = "the committed crates/core/src/class_spells.rs";
        assert!(retail_only(&install("wow"), None, None, place).is_ok());
        assert!(retail_only(&install("wow"), Some(&committed), None, place).is_ok());
        let tail = "is Retail's (product wow); product wowf needs -o naming another place";
        assert_eq!(
            retail_only(&install("wowf"), None, None, place).unwrap_err(),
            format!("{place} {tail}")
        );
        let e = retail_only(&install("wowf"), Some(&committed), None, place).unwrap_err();
        assert_eq!(e, format!("-o {}: {place} {tail}", committed.display()));

        let stem = root.join("crates/core/src/proc_spells");
        let twin = root.join("crates/core/src/proc_spells.expected.md");
        let e = retail_only(&install("wowf"), Some(&stem), Some(&twin), place).unwrap_err();
        assert_eq!(
            e,
            format!(
                "-o {} writes {} beside it: the committed crates/core/src/proc_spells.expected.md {tail}",
                stem.display(),
                twin.display()
            )
        );
        assert!(retail_only(&install("wow"), Some(&stem), Some(&twin), place).is_ok());

        let dir = scratch("allowed");
        let elsewhere = dir.join("class_spells.rs");
        let beside = dir.join("class_spells.expected.md");
        assert!(retail_only(&install("wowf"), Some(&elsewhere), Some(&beside), place).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
