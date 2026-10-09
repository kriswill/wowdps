//! Embeds every file under the repository's `rubric/` directory: the
//! seasons' TOML, so a binary carries the rubric it was built with. A file
//! added, changed or removed there rebuilds this crate.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "toml") {
            out.push(p);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("../../rubric");
    println!("cargo::rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
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
