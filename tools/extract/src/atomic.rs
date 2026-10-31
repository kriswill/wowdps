//! Writing a file whole or not at all. A reader that holds a cache open
//! (the GUI seeks into its icon and art caches, `gui-logic`'s `lazy_tiles`)
//! keeps the bytes its header describes while a regenerate renames a new
//! file over the name, where a write in place would hand it torn tiles.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The name a file is written under before it is renamed over `path`:
/// hidden, beside it (one filesystem, so the rename is atomic), and unique
/// to this process and this call, so no two writers share one.
pub fn tmp_beside(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.{}-{seq}.tmp", std::process::id()))
}

/// Rename a finished temporary file over `path`; on failure the temporary
/// file is removed.
pub fn rename_into(tmp: &Path, path: &Path) -> Result<(), String> {
    std::fs::rename(tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(tmp);
        format!("{}: {e}", path.display())
    })
}

/// Write `bytes` to `path` whole: into a temporary file beside it, then
/// renamed over it. A failed write leaves no temporary file behind.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), String> {
    let tmp = tmp_beside(path);
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    rename_into(&tmp, path)
}

/// Write `bytes` to `path` (whole, as `write_atomic` does) only when the
/// file does not hold them already; true when it wrote. A regenerate that
/// changes nothing leaves a repository file and its mtime alone, so what
/// watches it does not rebuild: a build script that walks a tree of
/// generated files reruns when anything under it is touched, and every
/// crate downstream with it.
pub fn write_if_changed(path: &Path, bytes: impl AsRef<[u8]>) -> Result<bool, String> {
    let bytes = bytes.as_ref();
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    write_atomic(path, bytes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atomic-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    }

    #[test]
    fn a_write_replaces_the_file_and_leaves_nothing_beside_it() {
        let d = scratch("replace");
        let p = d.join("cache.bin");
        write_atomic(&p, b"old").unwrap();
        // A reader that opened the old file keeps reading the old bytes.
        let mut held = std::fs::File::open(&p).unwrap();
        write_atomic(&p, b"newer").unwrap();
        let mut old = String::new();
        std::io::Read::read_to_string(&mut held, &mut old).unwrap();
        assert_eq!(old, "old");
        assert_eq!(std::fs::read(&p).unwrap(), b"newer");
        assert_eq!(names(&d), ["cache.bin"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_leaves_no_temporary_file() {
        let d = scratch("fail");
        // A directory where the file would go: the rename fails.
        let p = d.join("taken");
        std::fs::create_dir_all(p.join("inside")).unwrap();
        assert!(write_atomic(&p, b"x").is_err());
        assert_eq!(names(&d), ["taken"]);
        // A missing directory: the temporary file's own write fails.
        assert!(write_atomic(&d.join("none/file"), b"x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unchanged_file_is_left_alone() {
        let d = scratch("unchanged");
        let p = d.join("class_spells.rs");
        assert!(write_if_changed(&p, b"table").unwrap());
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!write_if_changed(&p, b"table").unwrap());
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
        assert!(write_if_changed(&p, b"table 2").unwrap());
        assert_eq!(std::fs::read(&p).unwrap(), b"table 2");
        assert_eq!(names(&d), ["class_spells.rs"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn temporary_names_are_hidden_and_never_repeat() {
        let p = Path::new("/x/talent-art.bin");
        let (a, b) = (tmp_beside(p), tmp_beside(p));
        assert_ne!(a, b);
        assert_eq!(a.parent(), p.parent());
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(".talent-art.bin.") && name.ends_with(".tmp"));
    }
}
