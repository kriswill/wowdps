---
type: Decision
title: DuckDB Is Bundled, Not System-Linked
description: 'wowdps-history compiles DuckDB from the duckdb crate''s own source on every platform (the `bundled` feature, with `json` and `parquet`) instead of linking nixpkgs'' libduckdb, because the crate pin had to equal `pkgs.duckdb`, nixpkgs lagged a release, and the pin could not reach the libduckdb-sys that closed three RustSec advisories; the price is a C++ compile of about 350 units, repeated whenever libduckdb-sys''s build inputs move.'
tags: [history, dependencies, nix, build, ci]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-23T10:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md §Dependencies — the history crate's line
  - id: nixpkgs-bump
    resource: https://github.com/NixOS/nixpkgs/pull/568519
    title: 'NixOS/nixpkgs#568519 — duckdb: 1.5.5 -> 1.5.6 (open since 2026-09-30)'
  - id: rustsec-0367
    resource: https://rustsec.org/advisories/RUSTSEC-2026-0367.html
    title: RUSTSEC-2026-0367 — zip, SUID/SGID bits preserved during extraction
  - id: rustsec-0368
    resource: https://rustsec.org/advisories/RUSTSEC-2026-0368.html
    title: RUSTSEC-2026-0368 — zip, extracted symlinks may point outside the destination
  - id: rustsec-0369
    resource: https://rustsec.org/advisories/RUSTSEC-2026-0369.html
    title: RUSTSEC-2026-0369 — zip, Unix permissions applied through symlinks
  - id: spec
    resource: ../../spec-history-store.md
    title: The history store spec — §3, the engine table and the original policy
---

**Where:** [`wowdps-history`](../crates/history.md) (its one `duckdb`
dependency line), `nix/dev/` and `flake.nix` (the system-link wiring this
removed), and CONTRACT.md §Dependencies.[^contract] It reverses the
"system-linked, never bundled" half of the 2026-09-02 sign-off in the
history store spec;[^spec] the sign-off itself (DuckDB as the one
analytical engine, in the `wowdps-history` binary only, never the daemon)
stands.

## Context

DuckDB was signed off on 2026-09-02 as a SYSTEM-linked dependency: the
`duckdb` crate built against nixpkgs' `libduckdb` (the dev shell and the
package set `DUCKDB_LIB_DIR` / `DUCKDB_INCLUDE_DIR`, the package patched the
rpath), because the spec measured `bundled` at 70 s locally and guessed
10–20 minutes on CI.[^spec] The crate's version encodes the DuckDB version
it wraps (`1.10505.0` is DuckDB 1.5.5), so the pin had to equal
`pkgs.duckdb`, and from 2026-10-03 `tests/duckdb_version.rs` failed by name
when the two drifted.

That coupling put the pin at nixpkgs' pace. nixpkgs stayed on 1.5.5; its
1.5.5 → 1.5.6 bump has been open since 2026-09-30.[^nixpkgs-bump] Every
dependabot DuckDB bump went red until nixpkgs caught up. Then `cargo audit`
flagged zip 6.0.0, a build dependency of libduckdb-sys 1.10505.0, for
RUSTSEC-2026-0367, -0368 and -0369;[^rustsec-0367][^rustsec-0368][^rustsec-0369]
libduckdb-sys 1.10506.x moved to zip 9, but the pin could not follow while
the library it named was a release behind. A macOS contributor's PR (#111)
had already needed a macOS-only `cfg` split to bundle there, since macOS has
no nixpkgs library to point at.

## Decision

- **Signed off 2026-10-23.** The user approved bundling in the session that
  made this change ("go, bundle it everywhere").
- **Bundled on every platform, one dependency line.** `duckdb =
  "=1.10506.1"` (DuckDB 1.5.6) with `default-features = false` and
  `features = ["bundled", "json", "parquet"]`. libduckdb-sys compiles the
  DuckDB source it ships with `cc` and links it statically, so the library
  is the crate's by construction. No `[target.'cfg(…)']` split.
- **JSON and Parquet as features, ICU absent.** The `cc` bundled path
  compiles `core_functions` plus the extensions its features name, and only
  `json` and `parquet` are offered there; ICU needs `bundled-cmake` and a
  duckdb-rs source checkout. No history query uses ICU, so
  `tests/parity.rs`' `network_access_is_off` now asserts JSON and Parquet
  loaded, and nothing else changed in the views. The mcp's `history_sql`
  description says there are no time zone functions and points at
  `start_local_ms`, or `start_utc_ms + tz_min * 60000`.
- **Offline still, with the compiled-in defaults against it.** The `cc`
  bundled build compiles `DUCKDB_EXTENSION_AUTOINSTALL_DEFAULT=1` and
  `DUCKDB_EXTENSION_AUTOLOAD_DEFAULT=1` in (nixpkgs' CMake build left both
  off), so the `SET`s `Lake::open_with` runs are now the only guard.
  `network_access_is_off` asserts both settings read false, and that a
  query needing ICU fails and leaves ICU unloaded on the reading and the
  writable lake alike. DuckDB's extension-function stub tries a load
  whatever `autoload_known_extensions` says; what keeps it off the network
  is `autoinstall_known_extensions = false`, the repository path that does
  not exist and the lake's empty extension directory.
- **No debug info for DuckDB in debug builds.** The workspace's
  `[profile.dev.package] libduckdb-sys = { debug = false }` hands the build
  script `DEBUG=false`: cc-rs drops `-g` and the build script defines
  `NDEBUG`, so DuckDB's own assertions are off in a debug build as in a
  release one. Measured on a clean debug rebuild: OUT_DIR 1.8 GB → 782 MB,
  `libduckdb.a` and the rlib 951 MB → 412 MB each, the parity test binary
  594 MB → 361 MB, the debug `wowdps-history` 503 MB → 271 MB, the
  build-script run 55.5 s → 48.9 s.
- **The system-link wiring is gone.** `duckdbEnv` (the two `DUCKDB_*`
  variables), `pkgs.duckdb` on the shell's package list and on
  `LD_LIBRARY_PATH`, the contract's libduckdb checks, the package's
  `buildInputs`, `LD_LIBRARY_PATH` and `autoPatchelfHook` (it existed to
  point the history binary's rpath at libduckdb), and
  `tests/duckdb_version.rs`: with the library compiled from the crate's own
  source there is nothing left to hold together.
- **The compiler is already there.** The dev shell's clang/clang++ (`CC`,
  `CXX`, put there by devenv's `languages.rust` and mirrored by the flake
  shell, per [devenv Is The Reference Shell](devenv-is-the-reference-shell.md))
  compile it; the crane package uses stdenv's C++ compiler.
- **Not DuckDB 2.0 here.** `1.20000.0` (DuckDB 2.0, a new storage format
  and C API) is its own change. It is a semver-minor step from `1.10506`,
  so `.github/dependabot.yml` ignores `duckdb` and `libduckdb-sys` from
  `1.20000.0` on; the cargo group would offer it otherwise.

## Consequences

Measured 2026-10-23 on a 24-core machine with this exact recipe (before
the debug-info override above): the DuckDB compile is 352 C++ units; debug
58–61 s wall (about 960 CPU-s), release 91 s wall (at most about 1,700
CPU-s). The crane `wowdps-deps` layer, which compiles every dependency in
release, took 148 s here; it has no sccache, and any Cargo.lock change
pays it again. On CI's 4-vCPU runners the first run of this change, cold
for the bundled compile, took 1,312 s for the `fmt + clippy + test +
fixtures` job (main: about 5 minutes warm, about 16 after a lock bump)
and 1,372 s for the `nix build (daemon/TUI package)` job (main: 3–5
minutes); the deps layer for that Cargo.lock then sat in FlakeHub Cache.
The release `wowdps-history` binary grows from 3 MB to 54 MB, the debug
one from 164 MB to 503 MB (271 MB with the override).

sccache does not make the compile a one-time cost. cc-rs puts
`RUSTC_WRAPPER` (the dev shell's sccache) in front of the C++ compiler, but
the build script unpacks DuckDB's source into libduckdb-sys's OUT_DIR and
compiles it by absolute path, and sccache keys on that path. The objects
are reused only while OUT_DIR is unchanged (a `cargo clean`, a branch
switched away and back); OUT_DIR's hash moves with any version change in
libduckdb-sys's build-dependency closure (cc, serde, syn, libc, flate2,
tar, ureq and rustls, zip and the rest), and the whole compile repeats
with a miss. A lock update that moves any of them makes CI pay the cold
compile again.

Dependabot may bump the crate freely within DuckDB 1.x now, and `cargo
audit` reports no vulnerabilities. The stable clippy canary in CI still
excludes the history crate: it runs uncached, and a cold DuckDB compile
would cost it several minutes for a crate the `ci` job already lints.

[^contract]: CONTRACT.md §Dependencies, the history crate's line.
[^nixpkgs-bump]: The nixpkgs DuckDB 1.5.5 → 1.5.6 bump, open since 2026-09-30.
[^rustsec-0367]: zip: SUID/SGID bits preserved during extraction.
[^rustsec-0368]: zip: extracted symlinks whose targets escape the destination.
[^rustsec-0369]: zip: Unix permissions applied through symlinks.
[^spec]: The history store spec's §3: the engine table and the policy it signed off.
