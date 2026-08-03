//! The `wowdps` binary is daemon + TUI client in one, so it links the engine
//! transitively — but the TUI client code itself must never touch it. The
//! compiler can't enforce that inside one binary; this grep keeps it honest.

#[test]
fn tui_sources_never_name_the_engine_modules() {
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let forbidden = [
        "wowdps_core::parser",
        "wowdps_core::meter",
        "wowdps_core::index",
        "wowdps_core::tail",
        "wowdps_core::app",
        "wowdps_core::model",
        "wowdps_core::testkit",
    ];
    for entry in std::fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for name in forbidden {
            assert!(
                !text.contains(name),
                "{} names {name}: the TUI renders daemon snapshots, it does not run the engine",
                path.display()
            );
        }
    }
}
