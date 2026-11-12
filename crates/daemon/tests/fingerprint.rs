//! The sweep's source fingerprint (`build.rs`, `build/fingerprint.rs`) over
//! trees of its own: a module split into files stays hashed, a missing unit
//! or a stale exclusion fails, deciding code outside the hashed set fails,
//! test code is cut and production code never is, and stray entries (dot-
//! files, dangling links, other files) move nothing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "../build/fingerprint.rs"]
mod fingerprint;

use std::path::{Path, PathBuf};

use fingerprint::{Unit, check_deciding, hashed, strip_tests};

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "wowdps-fingerprint-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn names(root: &Path, sources: &[Unit], left_out: &[&str]) -> Vec<String> {
    hashed(root, sources, left_out)
        .unwrap()
        .files
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}

fn print(root: &Path, sources: &[Unit]) -> u64 {
    fingerprint::fingerprint(&hashed(root, sources, &[]).unwrap().files)
}

#[test]
fn a_module_split_into_files_stays_hashed() {
    let t = Temp::new("split");
    t.write("src/history.rs", "pub mod retention;\nfn wants() {}\n");
    let units = [Unit::Module("src/history")];
    assert_eq!(names(&t.0, &units, &[]), ["src/history.rs"]);
    // Watched by its folder, which sees a split-off directory appear.
    let watched: Vec<PathBuf> = hashed(&t.0, &units, &[])
        .unwrap()
        .watched
        .into_iter()
        .collect();
    assert_eq!(watched, [t.0.join("src")]);
    let before = print(&t.0, &units);
    // Retention moves into a file of its own: hashed with the module.
    t.write("src/history/retention.rs", "fn retain() {}\n");
    // (Paths sort by component: `history` before `history.rs`.)
    assert_eq!(
        names(&t.0, &units, &[]),
        ["src/history/retention.rs", "src/history.rs"]
    );
    let split = print(&t.0, &units);
    assert_ne!(split, before);
    t.write(
        "src/history/retention.rs",
        "fn retain() { /* changed */ }\n",
    );
    assert_ne!(print(&t.0, &units), split, "a change in the split-off file");
    // A module as a folder alone (`mod.rs`) counts too.
    let m = Temp::new("modrs");
    m.write("src/loader/mod.rs", "fn load_for() {}\n");
    assert_eq!(
        names(&m.0, &[Unit::Module("src/loader")], &[]),
        ["src/loader/mod.rs"]
    );
}

#[test]
fn a_unit_or_an_exclusion_that_names_nothing_fails() {
    let t = Temp::new("missing");
    t.write("src/history.rs", "fn wants() {}\n");
    t.write("core/src/lib.rs", "pub mod cli;\n");
    t.write("core/src/cli.rs", "fn main() {}\n");
    assert!(hashed(&t.0, &[Unit::Module("src/history")], &[]).is_ok());
    let gone = hashed(&t.0, &[Unit::Module("src/loader")], &[]);
    assert!(gone.is_err_and(|e| e.contains("src/loader")));
    let gone = hashed(&t.0, &[Unit::Tree("model/src")], &[]);
    assert!(gone.is_err_and(|e| e.contains("model/src")));
    // A tree of no `.rs` file holds no source either.
    t.write("empty/src/README.md", "words\n");
    assert!(hashed(&t.0, &[Unit::Tree("empty/src")], &[]).is_err());
    let units = [Unit::Tree("core/src")];
    assert_eq!(
        names(&t.0, &units, &["core/src/cli.rs"]),
        ["core/src/lib.rs"]
    );
    let stale = hashed(&t.0, &units, &["core/src/tail.rs"]);
    assert!(stale.is_err_and(|e| e.contains("core/src/tail.rs")));
}

#[test]
fn deciding_code_outside_the_hashed_set_fails_the_build() {
    let t = Temp::new("deciding");
    t.write(
        "src/history.rs",
        "fn wants_meta() {}\n\n#[cfg(test)]\nmod tests {\n    fn store_impl() {}\n}\n",
    );
    t.write("src/hub.rs", "fn protected() {}\n");
    let files = hashed(&t.0, &[Unit::Module("src/history")], &[])
        .unwrap()
        .files;
    assert!(check_deciding(&files, &["fn wants_meta("]).is_ok());
    // In a file the set does not hash.
    assert!(check_deciding(&files, &["fn protected("]).is_err_and(|e| e.contains("protected")));
    // Only in test code, which is not hashed.
    assert!(check_deciding(&files, &["fn store_impl("]).is_err());
}

#[test]
fn test_code_is_cut_and_production_code_never_is() {
    // A formatted test module, mid-file and at the end: both cut, the
    // production code around them kept.
    let text = "fn a() {}\n\n#[cfg(test)]\nmod inner {\n    #[test]\n    fn t() {\n        let x = 1;\n\n    }\n}\n\nfn b() {}\n\n#[cfg(test)]\nmod tests {\n    fn u() {}\n}\n";
    let (kept, out) = strip_tests(text);
    assert_eq!(kept, "fn a() {}\n\n\nfn b() {}\n\n");
    assert!(out.is_empty());

    // A line in the first column inside the block (a raw string, an
    // unformatted file): kept whole, and what follows it too.
    for text in [
        "#[cfg(test)]\nmod tests {\n    const S: &str = r\"\nfn looks_like_code() {}\n\";\n}\nfn b() {}\n",
        "#[cfg(test)]\nmod tests {\n    fn t() {}\n    }\nfn production() {}\n}\n",
    ] {
        assert_eq!(strip_tests(text).0, text);
    }

    // Anything but `#[cfg(test)]` then `mod x {`: kept.
    for text in [
        "#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    fn t() {}\n}\n",
        "#[cfg(test)]\nfn helper() {}\n",
        "#[cfg(not(test))]\nmod live {\n    fn t() {}\n}\n",
        "#[cfg(test)]\npub mod tests {\n    fn t() {}\n}\n",
    ] {
        assert_eq!(strip_tests(text).0, text);
    }

    // An out-of-line test module: its name, so its file is left out.
    let (kept, out) = strip_tests("fn a() {}\n#[cfg(test)]\nmod tests;\n");
    assert_eq!(kept, "fn a() {}\n#[cfg(test)]\nmod tests;\n");
    assert_eq!(out, ["tests"]);
}

#[test]
fn an_out_of_line_test_module_is_left_out_with_its_children() {
    let t = Temp::new("outofline");
    t.write("src/lib.rs", "pub mod replay;\n");
    t.write(
        "src/replay.rs",
        "pub mod placed;\n#[cfg(test)]\nmod tests;\n",
    );
    t.write("src/replay/placed.rs", "fn cut() {}\n");
    t.write("src/replay/tests.rs", "fn t() {}\n");
    t.write("src/replay/tests/more.rs", "fn t2() {}\n");
    assert_eq!(
        names(&t.0, &[Unit::Tree("src")], &[]),
        ["src/lib.rs", "src/replay/placed.rs", "src/replay.rs"]
    );
    // Its edits move nothing.
    let before = print(&t.0, &[Unit::Tree("src")]);
    t.write("src/replay/tests.rs", "fn t() { /* another test */ }\n");
    assert_eq!(print(&t.0, &[Unit::Tree("src")]), before);
}

#[test]
fn stray_entries_move_nothing() {
    let t = Temp::new("stray");
    t.write("src/meter.rs", "fn feed() {}\n");
    t.write("src/history.rs", "fn wants() {}\n");
    let units = [Unit::Tree("src"), Unit::Module("src/history")];
    let before = print(&t.0, &units);
    // An editor's lock (a dangling link), a dot-file, a dot-folder, a
    // link to nowhere, notes and data.
    std::os::unix::fs::symlink("user@host.1234:5678", t.0.join("src/.#meter.rs")).unwrap();
    t.write("src/.scratch.rs", "fn not_built() {}\n");
    t.write("src/.cache/x.rs", "fn cached() {}\n");
    std::os::unix::fs::symlink("nowhere.rs", t.0.join("src/gone.rs")).unwrap();
    t.write("src/NOTES.md", "notes\n");
    t.write("src/table.tsv", "1\t2\n");
    t.write("src/history.rs.orig", "fn old() {}\n");
    assert_eq!(print(&t.0, &units), before);
    assert_eq!(names(&t.0, &units, &[]), ["src/history.rs", "src/meter.rs"]);
    // A real source still does.
    t.write("src/parser.rs", "fn parse_line() {}\n");
    assert_ne!(print(&t.0, &units), before);
}

#[test]
fn the_fingerprint_is_the_names_and_the_text() {
    let a = Temp::new("order-a");
    let b = Temp::new("order-b");
    // The same tree written in two orders: one fingerprint.
    a.write("src/x.rs", "fn x() {}\n");
    a.write("src/y.rs", "fn y() {}\n");
    b.write("src/y.rs", "fn y() {}\n");
    b.write("src/x.rs", "fn x() {}\n");
    let units = [Unit::Tree("src")];
    assert_eq!(print(&a.0, &units), print(&b.0, &units));
    // A rename moves it; so does a byte of production code; a test module's
    // edit does not.
    b.write("src/z.rs", "fn y() {}\n");
    let _ = std::fs::remove_file(b.0.join("src/y.rs"));
    assert_ne!(print(&a.0, &units), print(&b.0, &units));
    a.write(
        "src/x.rs",
        "fn x() {}\n\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n",
    );
    let base = print(&a.0, &units);
    a.write(
        "src/x.rs",
        "fn x() {}\n\n#[cfg(test)]\nmod tests {\n    fn t() {}\n\n    fn u() {}\n}\n",
    );
    assert_eq!(print(&a.0, &units), base, "a test module edited");
    a.write(
        "src/x.rs",
        "fn x() { 1; }\n\n#[cfg(test)]\nmod tests {\n    fn t() {}\n\n    fn u() {}\n}\n",
    );
    assert_ne!(print(&a.0, &units), base, "production code changed");
}
