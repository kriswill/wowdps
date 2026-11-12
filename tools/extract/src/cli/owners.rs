//! The outputs each edition of the game owns, which another edition's run
//! never takes: the committed tables and their review twins in a checkout,
//! which the meter is built from, each made from one edition's client
//! (`tablegen::committed`), and the caches in the data home, which the GUI
//! and the mcp tools read, all Retail's. A run writes a place it owns or
//! one no edition does, and nothing else (`owned_only`), checked before
//! the command reads anything.

use std::path::{Component, Path, PathBuf};

use super::install::Install;
use super::{caches, data_path, tablegen};
use wowdps_core::model::Edition;
use wowdps_extract::game::default_product;

/// A file at a wowdps checkout's root that only a checkout holds: what
/// the dev shell's wrappers know a checkout by.
pub const CHECKOUT_MARK: &str = "crates/core/fixtures/check.awk";

/// How the refusal names a cache in the data home.
pub fn cache_place(name: &str) -> String {
    format!("the data home's {name}")
}

/// Refuse the places a command would write that another product owns: its
/// `-o` when `out` resolves to a committed table or twin made from another
/// edition's client, or, for any product but Retail's, to a data-home cache
/// (whatever its spelling, `..` or symlinks); `beside`, the file it writes
/// next to `out` and named from it (a table's review twin), likewise; or,
/// with no `-o`, its default `place`, which is Retail's, for any product
/// but Retail's. A run writes its own edition's places and any place no
/// edition owns.
pub fn owned_only(
    install: &Install,
    out: Option<&Path>,
    beside: Option<&Path>,
    place: &str,
) -> Result<(), String> {
    let refuse = |what: String, owner: Edition| {
        Err(format!(
            "{what} is {}'s (product {}); product {} needs -o naming another place",
            owner.label(),
            owner.product().unwrap_or_default(),
            install.product
        ))
    };
    let Some(out) = out else {
        return if install.product == default_product() {
            Ok(())
        } else {
            refuse(place.to_string(), Edition::Retail)
        };
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let caches: Vec<PathBuf> = caches::CACHES
        .iter()
        .filter_map(|name| data_path(name).ok())
        .collect();
    let own = owned_places(out, &cwd, &caches)
        .into_iter()
        .map(|f| (f, None));
    let twin = beside.into_iter().flat_map(|b| {
        owned_places(b, &cwd, &caches)
            .into_iter()
            .map(move |f| (f, Some(b)))
    });
    for (found, b) in own.chain(twin) {
        if found.owner.product() == Some(install.product.as_str()) {
            continue;
        }
        let what = match b {
            None => format!("-o {}: {}", out.display(), found.place),
            Some(b) => format!(
                "-o {} writes {} beside it: {}",
                out.display(),
                b.display(),
                found.place
            ),
        };
        return refuse(what, found.owner);
    }
    Ok(())
}

/// An output an edition owns: how a refusal names it, and the edition.
#[derive(Debug, PartialEq, Eq)]
struct Owned {
    place: String,
    owner: Edition,
}

/// The owned outputs `out` (relative to `cwd`) reaches: a committed table
/// or review twin of a wowdps checkout, its edition's, or one of `caches`,
/// Retail's, compared as the file system resolves them. `out` reaches both
/// what it resolves to and where a write to it lands, which differ when its
/// last component is a symlink: the writers rename a finished file over the
/// name, which replaces the link rather than its target, so a committed
/// table replaced by a link in its checkout is still hit, and a link from
/// one edition's table to another's reaches both.
fn owned_places(out: &Path, cwd: &Path, caches: &[PathBuf]) -> Vec<Owned> {
    let places = [Some(resolved(out, cwd)), landing(out, cwd)];
    places
        .iter()
        .flatten()
        .filter_map(|out| owned_at(out, cwd, caches))
        .collect()
}

/// The owned output at `out`, a path already resolved, if any.
fn owned_at(out: &Path, cwd: &Path, caches: &[PathBuf]) -> Option<Owned> {
    for (owner, rel) in tablegen::committed() {
        // The checkout `out` would lie in, were it this file of one.
        let root = out.ancestors().nth(rel.components().count());
        if out.ends_with(&rel) && root.is_some_and(|r| r.join(CHECKOUT_MARK).is_file()) {
            let place = format!("the committed {}", rel.display());
            return Some(Owned { place, owner });
        }
    }
    for cache in caches {
        if resolved(cache, cwd) == out {
            let name = cache.file_name().unwrap_or_default().to_string_lossy();
            let place = cache_place(&name);
            return Some(Owned {
                place,
                owner: Edition::Retail,
            });
        }
    }
    None
}

/// The first place `out` reaches, as `owned_places` names it, for the tests.
#[cfg(test)]
fn place_of(out: &Path, cwd: &Path, caches: &[PathBuf]) -> Option<String> {
    owned_places(out, cwd, caches)
        .into_iter()
        .next()
        .map(|o| o.place)
}

/// Where a rename onto `path` (relative to `cwd`) lands: its directory
/// resolved, with its own name kept, link or not. None where it names no
/// file (it ends in `..`).
fn landing(path: &Path, cwd: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    Some(resolved(path.parent()?, cwd).join(name))
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
        let dir = std::env::temp_dir().join(format!("wowdps-owners-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn the_committed_files_are_every_table_and_twin() {
        let root = checkout();
        let committed = tablegen::committed();
        assert_eq!(
            committed.len(),
            24,
            "Retail's 8 tables and 4 review twins, Forever's 8 tables and 4 review twins"
        );
        for (_, rel) in &committed {
            assert!(root.join(rel).is_file(), "{}", rel.display());
        }
        assert!(root.join(CHECKOUT_MARK).is_file());
    }

    /// A committed file is its edition's: Forever's under its directory,
    /// Retail's beside it of the same name, and a cache Retail's.
    #[test]
    fn a_committed_file_is_found_with_its_edition() {
        let root = checkout();
        let owner = |rel: &str| {
            let found = owned_places(&root.join(rel), Path::new("/"), &[]);
            found.into_iter().next().map(|o| o.owner)
        };
        assert_eq!(
            owner("crates/core/src/class_spells.rs"),
            Some(Edition::Retail)
        );
        assert_eq!(
            owner("crates/core/src/forever/class_spells.rs"),
            Some(Edition::Forever)
        );
        assert_eq!(
            owner("crates/core/src/forever/open_world_maps.rs"),
            Some(Edition::Forever)
        );
        assert_eq!(
            owner("crates/core/src/forever/role_spells.rs"),
            Some(Edition::Forever)
        );
        assert_eq!(owner("crates/core/src/forever/meter.rs"), None);
        assert_eq!(owner("crates/core/src/forever.rs"), None);
    }

    /// A committed file is found whatever spells it: absolute, relative
    /// to the checkout or to its own directory, through `..`, through a
    /// symlinked directory; and every table's and twin's, not one alone.
    #[test]
    fn a_committed_file_is_found_however_it_is_spelled() {
        let root = checkout();
        let src = root.join("crates/core/src");
        let found = |out: &str, cwd: &Path| place_of(Path::new(out), cwd, &[]);
        let class = Some("the committed crates/core/src/class_spells.rs".to_string());
        let abs = src.join("class_spells.rs");
        assert_eq!(place_of(&abs, Path::new("/"), &[]), class);
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

    /// A committed file that is itself a symlink in its checkout is found
    /// too: it resolves to the link's target, but the writer renames its
    /// file over the name, which replaces the link where the committed file
    /// is. The target is no committed file for it; a data-home cache that
    /// is a link is found as well.
    #[test]
    fn a_committed_file_replaced_by_a_symlink_is_found() {
        let dir = scratch("linked");
        let tree = dir.join("checkout");
        let mark = tree.join(CHECKOUT_MARK);
        std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
        std::fs::write(&mark, "").unwrap();
        let src = tree.join("crates/core/src");
        std::fs::create_dir_all(&src).unwrap();
        let target = dir.join("elsewhere.rs");
        std::fs::write(&target, "x").unwrap();
        for name in ["class_spells.rs", "role_spells.expected.md"] {
            std::os::unix::fs::symlink(&target, src.join(name)).unwrap();
            let want = Some(format!("the committed crates/core/src/{name}"));
            assert_eq!(place_of(&src.join(name), Path::new("/"), &[]), want);
            assert_eq!(place_of(Path::new(name), &src, &[]), want);
            let linked = dir.join("src");
            let _ = std::fs::remove_file(&linked);
            std::os::unix::fs::symlink(&src, &linked).unwrap();
            assert_eq!(place_of(&linked.join(name), Path::new("/"), &[]), want);
        }
        assert_eq!(place_of(&target, Path::new("/"), &[]), None);

        let home = dir.join("data/wowdps");
        std::fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(&target, home.join("talents.json")).unwrap();
        let caches: Vec<PathBuf> = caches::CACHES.iter().map(|n| home.join(n)).collect();
        assert_eq!(
            place_of(&home.join("talents.json"), Path::new("/"), &caches),
            Some("the data home's talents.json".into())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Anywhere else is no edition's: a file of the same name elsewhere, a
    /// path shaped like a checkout's that is no checkout, a path in no
    /// directory that exists, a source file of the checkout no generator
    /// writes.
    #[test]
    fn elsewhere_is_no_editions() {
        let dir = scratch("elsewhere");
        let none = |out: &Path| place_of(out, Path::new("/"), &[]);
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
        let found = |out: PathBuf| place_of(&out, Path::new("/"), &caches);
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
            place_of(&out, Path::new("/"), &caches),
            Some("the data home's talents.json".into())
        );
        // `..` after a missing component, then into a symlinked directory.
        let src = checkout().join("crates/core/src");
        std::os::unix::fs::symlink(&src, dir.join("src")).unwrap();
        assert_eq!(
            place_of(&dir.join("gone/../src/role_spells.rs"), Path::new("/"), &[]),
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
                place_of(&out, Path::new("/"), &by(cached)),
                Some("the data home's talents.json".into()),
                "caches under {cached}, -o under {written}"
            );
        }
        assert!(!real.join("wowdps").exists(), "nothing made");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Retail's product writes wherever it did but on another edition's
    /// table; another is refused no `-o`, an `-o` that is Retail's, and an
    /// `-o` whose twin beside it would be, in words that read right for
    /// each, and let through to anywhere else.
    #[test]
    fn another_products_place_is_refused_and_anywhere_else_is_not() {
        let root = checkout();
        let committed = root.join("crates/core/src/class_spells.rs");
        let place = "the committed crates/core/src/class_spells.rs";
        assert!(owned_only(&install("wow"), None, None, place).is_ok());
        assert!(owned_only(&install("wow"), Some(&committed), None, place).is_ok());
        let tail = "is Retail's (product wow); product wowf needs -o naming another place";
        assert_eq!(
            owned_only(&install("wowf"), None, None, place).unwrap_err(),
            format!("{place} {tail}")
        );
        let e = owned_only(&install("wowf"), Some(&committed), None, place).unwrap_err();
        assert_eq!(e, format!("-o {}: {place} {tail}", committed.display()));

        let stem = root.join("crates/core/src/proc_spells");
        let twin = root.join("crates/core/src/proc_spells.expected.md");
        let e = owned_only(&install("wowf"), Some(&stem), Some(&twin), place).unwrap_err();
        assert_eq!(
            e,
            format!(
                "-o {} writes {} beside it: the committed crates/core/src/proc_spells.expected.md {tail}",
                stem.display(),
                twin.display()
            )
        );
        assert!(owned_only(&install("wow"), Some(&stem), Some(&twin), place).is_ok());

        let dir = scratch("allowed");
        let elsewhere = dir.join("class_spells.rs");
        let beside = dir.join("class_spells.expected.md");
        for product in ["wowf", "wow"] {
            let i = install(product);
            assert!(
                owned_only(&i, Some(&elsewhere), Some(&beside), place).is_ok(),
                "{product}"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Forever's committed table is Forever's own: its run writes it, by
    /// any spelling, and Retail's is refused it, as is a product neither
    /// edition names; Forever's run is still refused Retail's.
    #[test]
    fn each_editions_table_is_written_by_its_own_run_alone() {
        let root = checkout();
        let forever = root.join("crates/core/src/forever/class_spells.rs");
        let retail = root.join("crates/core/src/class_spells.rs");
        let place = "the committed crates/core/src/class_spells.rs";
        assert!(owned_only(&install("wowf"), Some(&forever), None, place).is_ok());
        let spelled = root.join("crates/core/src/../src/forever/./class_spells.rs");
        assert!(owned_only(&install("wowf"), Some(&spelled), None, place).is_ok());
        let tail = "is Forever's (product wowf); product wow needs -o naming another place";
        let e = owned_only(&install("wow"), Some(&forever), None, place).unwrap_err();
        assert_eq!(
            e,
            format!(
                "-o {}: the committed crates/core/src/forever/class_spells.rs {tail}",
                forever.display()
            )
        );
        let e = owned_only(&install("wowx"), Some(&forever), None, place).unwrap_err();
        assert!(
            e.ends_with("is Forever's (product wowf); product wowx needs -o naming another place"),
            "{e}"
        );
        let e = owned_only(&install("wowf"), Some(&retail), None, place).unwrap_err();
        assert!(
            e.ends_with("is Retail's (product wow); product wowf needs -o naming another place"),
            "{e}"
        );
        // A product with no table of its own still has no default place.
        assert!(owned_only(&install("wowx"), None, None, place).is_err());
    }

    /// A link from one edition's committed table to another's reaches both:
    /// what it resolves to and where a write to it lands. Neither edition's
    /// run may write through it.
    #[test]
    fn a_link_between_two_editions_tables_is_refused_to_both() {
        let dir = scratch("cross");
        let tree = dir.join("checkout");
        let mark = tree.join(CHECKOUT_MARK);
        std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
        std::fs::write(&mark, "").unwrap();
        let src = tree.join("crates/core/src");
        std::fs::create_dir_all(src.join("forever")).unwrap();
        std::fs::write(src.join("class_spells.rs"), "x").unwrap();
        let link = src.join("forever/class_spells.rs");
        std::os::unix::fs::symlink(src.join("class_spells.rs"), &link).unwrap();
        let owners: Vec<Edition> = owned_places(&link, Path::new("/"), &[])
            .into_iter()
            .map(|o| o.owner)
            .collect();
        assert_eq!(owners, [Edition::Retail, Edition::Forever]);
        let place = "the committed crates/core/src/class_spells.rs";
        for product in ["wow", "wowf"] {
            assert!(
                owned_only(&install(product), Some(&link), None, place).is_err(),
                "{product}"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
