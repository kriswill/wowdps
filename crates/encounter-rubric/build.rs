//! Embeds every file under the crate's `seasons/` directory: the seasons'
//! TOML, so a binary carries the rubric it was built with. A file added,
//! changed or removed there rebuilds this crate. A dot-file or directory
//! (an editor's lock or swap file) is passed over, as `Rubric::from_dir`
//! passes it over in a tree on disk.
//!
//! The tree is the base alone: a hand-tuned `<id>-<slug>.toml` or an
//! instance's `trash.toml` belongs under the repository's
//! `curated/seasons/`. One found here (an editor on an old pin saved it
//! where the set lay before it moved) is said as a warning naming it,
//! since it would be embedded as the base's own curated tier; the crate's
//! tests refuse it (`the_embedded_base_holds_no_tuned_or_trash_file`).

use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.file_name().as_encoded_bytes().first() == Some(&b'.') {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "toml") {
            out.push(p);
        }
    }
}

/// Whether a path under `seasons/` names a curated file: an instance's
/// `trash.toml`, or an encounter's tuned `<id>-<slug>.toml` (a draft is
/// `<id>-<slug>.draft.toml`). The rule `FileRole::of` reads paths by.
fn curated_file(rel: &str) -> bool {
    match *rel.split('/').collect::<Vec<_>>().as_slice() {
        [_, _, "trash.toml"] => true,
        [_, _, "instance.toml"] => false,
        [_, _, file] => file.strip_suffix(".toml").is_some_and(|stem| {
            !stem.ends_with(".draft")
                && stem
                    .split('-')
                    .next()
                    .is_some_and(|id| id.parse::<u32>().is_ok())
        }),
        _ => false,
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("seasons");
    println!("cargo::rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    for f in &files {
        let rel = f
            .strip_prefix(&root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        if curated_file(&rel) {
            println!(
                "cargo::warning=rubric: seasons/{rel} is a tuned or trash file in the base; it belongs under curated/seasons/{rel}"
            );
        }
    }
    let mut code = String::from("pub static FILES: &[(&str, &str)] = &[\n");
    for f in &files {
        let rel = f
            .strip_prefix(&root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let abs = f.canonicalize().unwrap_or_else(|_| f.clone());
        code.push_str(&format!(
            "    ({rel:?}, include_str!({:?})),\n",
            abs.to_string_lossy()
        ));
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("embedded.rs");
    if let Err(e) = std::fs::write(&out, code) {
        println!("cargo::warning=rubric: cannot write {}: {e}", out.display());
    }
}
