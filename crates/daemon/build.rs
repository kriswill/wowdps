//! The sweep's source fingerprint (`swept::SOURCES`): a hash of every Rust
//! source that decides which fights a read of a log yields and the history
//! store keeps. The sweep's manifest answers to it, so a build that changes
//! any of them reads every log once on its first start, and one that
//! changes none (the hub, the engine, a client) reads nothing again. No
//! one has to remember to bump a number: the code is the version.
//!
//! Whole trees where the deciding code lives throughout (model, core: the
//! parser, the scanner, the meter, the replay cut an import meters through,
//! the generated tables), single files elsewhere. A file the storing path
//! moves into joins the list. Only `.rs` files count: the packages' source
//! filter keeps those and leaves Markdown out, so every build of one
//! checkout agrees.

use std::path::{Path, PathBuf};

/// Relative to this crate's directory.
const SOURCES: &[&str] = &[
    "../model/src",
    "../core/src",
    "../proto/src/history.rs",
    "src/history.rs",
    "src/loader.rs",
    "src/cache.rs",
    "src/swept.rs",
];

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let mut hash = Fnv::new();
    let mut files = Vec::new();
    for source in SOURCES {
        let path = root.join(source);
        println!("cargo:rerun-if-changed={}", path.display());
        if !path.exists() {
            // Never quietly constant: say so, and let the missing tree's
            // name move the hash at least.
            println!(
                "cargo:warning=swept: {source} is missing; the sweep's fingerprint cannot see it"
            );
        }
        hash.write(source.as_bytes());
        hash.write(&[0]);
        collect(&path, &mut files);
    }
    files.sort();
    for file in &files {
        let name = file.strip_prefix(&root).unwrap_or(file);
        hash.write(name.to_string_lossy().as_bytes());
        hash.write(&[0]);
        match std::fs::read(file) {
            Ok(bytes) => {
                hash.write(&(bytes.len() as u64).to_le_bytes());
                hash.write(&bytes);
            }
            Err(_) => hash.write(b"unreadable"),
        }
    }
    println!("cargo:rustc-env=WOWDPS_SWEEP_SOURCES={:016x}", hash.0);
}

/// Every `.rs` file at or under `path`.
fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|x| x == "rs") {
        out.push(path.to_path_buf());
    }
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
