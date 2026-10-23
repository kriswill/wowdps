---
type: Tool
title: codeql
description: Runs GitHub's CodeQL code scanning locally the way the repository's default setup runs it on every PR (rust and actions, the default suites, the same query pack versions) and fails only on an alert missing from the committed baseline, tools/codeql-baseline.txt, as GitHub fails a PR only on an alert not already open on main.
resource: tools/codeql.sh
tags: [tool, dev, ci]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-23T15:00:00-07:00 }
sources:
  - id: default-setup
    resource: https://docs.github.com/en/code-security/code-scanning/enabling-code-scanning/configuring-default-setup-for-code-scanning
    title: GitHub Docs — configuring default setup for code scanning
  - id: sarif
    resource: https://docs.github.com/en/code-security/code-scanning/integrating-with-code-scanning/sarif-support-for-code-scanning
    title: GitHub Docs — SARIF support for code scanning (partialFingerprints)
---

`wowdps-codeql [--accept] [rust|actions]...` is the dev shells' wrapper
(`nix/dev/codeql.nix`) around `tools/codeql.sh`, run from the live
checkout like the `wowdps-gen-*` generators. With no language it runs
both. CI never calls it: CodeQL's CI run is the repository's default
setup, a repository setting with no workflow file, which scans every
PR[^default-setup]. This is that run on the dev machine, before the push.
Exit 0 means no new alert, 1 new alerts, 2 a usage error or a CodeQL
failure (the step's log tail is printed).

## What it mirrors

- **Languages and suites.** GitHub's default setup here runs `rust` and
  `actions` with the `default` query suite and the CLI's default threat
  model (`remote`). The suites are
  `codeql/rust-queries:codeql-suites/rust-code-scanning.qls` and
  `codeql/actions-queries:codeql-suites/actions-code-scanning.qls`, both
  in build mode `none`: the Rust extractor loads the Cargo workspace
  itself and needs no build.
- **Pack versions, pinned.** `codeql/rust-queries@0.1.45` and
  `codeql/actions-queries@0.6.38`, the versions GitHub's run used on
  2026-10-23, at the same commit (the SARIF GitHub keeps names them under
  `runs[0].tool.extensions`; fetch one with `gh api -H 'Accept:
  application/sarif+json' repos/kriswill/wowdps/code-scanning/analyses/<id>`).
  Pinned, every machine runs the same queries and a pack release never
  shows up as new local alerts; move the pins when GitHub's run moves.
- **The CLI differs, the queries do not.** The dev shells carry nixpkgs'
  `codeql` 2.27.1; GitHub ran 2.27.3. The queries come from the packs, and
  on 2026-10-23 the local run reported exactly the ten alerts GitHub had
  open on main.

## The baseline

`tools/codeql-baseline.txt` lists the alerts accepted as open, one per
line: the rule, the SARIF `partialFingerprints.primaryLocationLineHash`
and the path, tab-separated, under a `#` header. The rule and hash
identify an alert[^sarif]: the hash follows the flagged line's text, so an
alert keeps it when lines above it move (its `:N` suffix counts identical
lines within a file); the path is for people. A run prints every alert
(`rule  path:line  message`), how many baseline alerts are still open, and
then any alert the baseline lacks, which fails it.

`--accept` rewrites the baseline from the run's alerts and exits 0. It
replaces only the lines of the languages it ran (a rule is named
`<language>/...`), so `wowdps-codeql actions --accept` keeps the Rust
lines. An alert fixed since is dropped by the next `--accept`; until then
the run reports it as no longer open. Seeded 2026-10-23 with main's ten: 4
`rust/hard-coded-cryptographic-value` (the CASC decoders' fixed Salsa20
nonces, `tools/extract/src/{salsa20,blte}.rs`) and 6
`rust/cleartext-logging` (in `crates/daemon`'s history store, engine and
production test, and in `crates/tui`). The same day, #128 moved the
production test's two alerts down 11 lines; their hashes held.

## Seams

- **A staged copy, not the checkout.** GitHub scans a fresh checkout, which
  holds no ignored file. The Rust extractor reads every `.rs` under its
  source root, ignored or not: run on the main checkout it extracted the
  304 tracked files cleanly and 696 more "with errors", 639 from the
  gitignored `docs/gpui` mirror ([fetch-gpui-docs](fetch-gpui-docs.md),
  vendored sources with no manifest) and 57 build-script outputs under
  `target/`. So each run first copies the tree as git sees it (`git
  ls-files --cached --others --exclude-standard`: tracked files plus
  untracked ones no `.gitignore` covers) to `target/codeql/src/` and
  extracts that. A new file is scanned before it is added; an ignored one
  never is.
- **The standard library comes from stable sources.** The Rust extractor
  models std from `RUST_SRC_PATH`, and without it reads the toolchain's
  own `rust-src`: the nightly `rust-toolchain.toml` names, which the
  nixpkgs CLI cannot resolve. On nightly-2026-10-19 the database came out
  degraded (28 % of calls with a call target, against CodeQL's 50 %
  threshold) and 7 of main's 10 alerts were missing: the six
  cleartext-logging ones and one nonce. The wrapper sets `RUST_SRC_PATH`
  to nixpkgs' stable library source (`rustPlatform.rustLibSrc`, 1.98.1 on
  the 2026-10-23 pin), which gives the ten and a clean database. It is the
  source CodeQL reads std from, not a toolchain: nothing builds with it,
  and it moves with nixpkgs. An older devenv exported that same path from
  its `languages.rust` and the current one exports none, which is why a
  shell entered before the upgrade found ten and a fresh one three.
- **A degraded database fails the run.** CodeQL still analyzes a database
  it could not resolve, and the missing alerts would read as a pass. It
  flags one in the SARIF (`rust/diagnostic/database-quality`), and the
  script exits 2 on that flag.
- **Fresh databases.** Each language's database is recreated on every run
  (`--overwrite`): a stale database reports on code that has since changed,
  which is worse than a slow run. Everything lives under `target/codeql/`
  (gitignored): the staged source, and per language the database, the
  SARIF file and the CodeQL log.
- **Packs from the registry, once.** The packs are not bundled with the
  nixpkgs CLI. The script resolves each suite first (`codeql resolve
  queries`, offline, under a second) and only when that fails runs `codeql
  pack download`, which fetches from GitHub's container registry
  anonymously (about 2 s) into `~/.codeql/packages/`; a machine that has
  them never touches the network. `codeql resolve qlpacks` is no test: it
  does not list that package cache.
- **The all-platforms banner.** nixpkgs packages the deprecated
  all-platforms CLI zip, which prints a deprecation warning on every call
  unless `CODEQL_ALLOW_ALL_PLATFORMS_DIST=true`; the script sets it.
- **Every core.** Extraction and analysis run with `--threads=0`.
- **The unfree allowance.** `codeql` is the one unfree package the shells
  take from nixpkgs, allowed by name in `nix/dev/codeql.nix`'s own nixpkgs
  import ([devenv Is The Reference Shell](../decisions/devenv-is-the-reference-shell.md),
  "The parcels"). `wowdps-dev-contract` checks that `codeql` and
  `wowdps-codeql` are on both shells' PATH.
- **Disk.** An analyzed Rust database, with its evaluation cache, takes
  about 2.7 GB under `target/codeql/`; `cargo clean` removes it.

## Timings

Measured 2026-10-23 on the dev machine (24 cores) under `nice -n 19`, the
query packs already compiled:

- **Rust:** 4m06s to 4m10s a run over three runs: about 2 min to create
  the database (1m37s of it the extractor loading the workspace and the
  standard library) and 2 min to analyze.
- **Actions:** 5 s.
- **A whole run,** entering devenv included: 254 s.
- **The first analysis on a machine** also compiles the queries: 9 min in
  a single-threaded run made before the script passed `--threads=0`; later
  runs reuse the compiled cache. The pack download is about 2 s.

## Source

- Script: [`tools/codeql.sh`](../../../tools/codeql.sh)
- Baseline: [`tools/codeql-baseline.txt`](../../../tools/codeql-baseline.txt)
- Wrapper and CLI: [`nix/dev/codeql.nix`](../../../nix/dev/codeql.nix)

[^default-setup]: GitHub Docs — configuring default setup for code scanning
[^sarif]: GitHub Docs — SARIF support for code scanning (partialFingerprints)
