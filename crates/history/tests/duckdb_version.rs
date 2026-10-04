//! The DuckDB pin holds the library it links. The crate is SYSTEM-linked to
//! nixpkgs' libduckdb (CONTRACT.md §Dependencies), and its version encodes
//! the library version it was written against: `=1.10505.0` is DuckDB
//! 1.5.5. Dependabot bumps the crate on its own, and `nix flake update`
//! moves the library on its own, so either can leave the two apart, and
//! nothing else fails when they drift. This does, by name, on the PR that
//! moved one of them.

#![allow(clippy::expect_used, clippy::panic)]

/// The DuckDB version a `duckdb` crate version encodes: `1.10505.0` →
/// `1.5.5` (the middle number is major·10000 + minor·100 + patch).
fn encoded(crate_version: &str) -> Option<String> {
    let n: u32 = crate_version.split('.').nth(1)?.parse().ok()?;
    Some(format!("{}.{}.{}", n / 10_000, n / 100 % 100, n % 100))
}

#[test]
fn the_crate_version_encodes_the_library_version() {
    assert_eq!(encoded("1.10505.0").as_deref(), Some("1.5.5"));
    assert_eq!(encoded("1.10504.0").as_deref(), Some("1.5.4"));
    assert_eq!(encoded("1.11200.0").as_deref(), Some("1.12.0"));
    assert_eq!(encoded("1.x.0"), None);
}

#[test]
fn the_duckdb_pin_matches_the_linked_library() {
    let pin = include_str!("../Cargo.toml")
        .lines()
        .find_map(|l| l.trim().strip_prefix("duckdb = "))
        .and_then(|rest| rest.split('"').nth(1))
        .map(|v| v.trim_start_matches('='))
        .expect("Cargo.toml pins duckdb as `duckdb = { version = \"=…\" … }`");
    let want = encoded(pin).unwrap_or_else(|| panic!("cannot read the pin {pin:?}"));

    let conn = duckdb::Connection::open_in_memory().expect("an in-memory DuckDB");
    let linked: String = conn
        .query_row("select version()", [], |r| r.get(0))
        .expect("select version()");
    let got: String = linked
        .trim_start_matches('v')
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    assert_eq!(
        got, want,
        "the duckdb crate is pinned to ={pin} (DuckDB {want}), but the linked \
         library is {linked} (nixpkgs' pkgs.duckdb). The crate system-links that \
         library and must move with it: hold a crate bump until `nix flake \
         update` brings DuckDB {want}, or after a nixpkgs bump move the pin in \
         crates/history/Cargo.toml to the library's version"
    );
}
