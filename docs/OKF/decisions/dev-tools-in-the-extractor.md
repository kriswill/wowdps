---
type: Decision
title: Keep The Dev Tools In The Extractor, Calling The Shell's CLIs
description: 'The repeatable steps of the portrait work (game tables as SQL, contact sheets, window shots, the dark-render audit) became extractor subcommands with dev-shell names rather than scratch scripts or a new crate; since the split only the SQL one (wowdps-db2) is wowdps-extract''s, the rest the replay''s extractor''s, and each crate stays stdlib-only by calling CLIs: curl and a duckdb on PATH here, lswt and grim in the replay''s.'
tags: [dev-shell, tooling, dependencies]
status: draft
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:30:00-07:00 }
sources:
  - id: wrappers
    resource: ../../../nix/dev/wrappers.nix
    title: wrappers.nix — the `tools` table of dev-shell names
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md §Dependencies — the extractor's stdlib-only line
---

**Where:** [`wowdps-extract`](../crates/extract.md):
[wowdps-db2](../tools/wowdps-db2.md); and, in the private wowdps-replay
repository, `wowdps-sheet`, `wowdps-shot` and `wowdps-portrait-audit`.

**Update 2026-10-30:** `sheet`, `shot` and `portrait-audit` (with `font`)
are subcommands of `wowdps-replay-extract` since the replay's extractor
split off, and left with the replay for wowdps-replay; `sql` stays in
`wowdps-extract`, the one of the four on main, its wrapper with it. The
rule holds for both crates; the `gpu` and `wcl` features are the replay
extractor's, and `wowdps-extract` has none
([The Replay's Extractor Splits From The Game-Data Library](replay-extract-splits-from-extract.md)).

## Context

The NPC portrait work (the replay's decision "Wear The Journal's
Portrait, Else Render The Model") ran on throwaway shell that a later
session would write again:

- about fifteen table dumps, each with its FileDataID looked up, its
  schema fetched, `fetch`, `csv`, and a duckdb join
- eight magick montages
- a brightness ranking
- window captures

The role-spell curation loop does the same table dumps by hand. Each
`gen-*.sh` fetches its own schemas and keys into a temporary directory.

## Decision

**Subcommands of the extractor, named in the dev shell.** `sql`, `sheet`,
`shot` and `portrait-audit` became subcommands of `wowdps-extract`, then
the repository's dev-time tool crate, which holds the CASC reader and the
PNG codec they need. Since the split `sql` is the one left in
`wowdps-extract`; `sheet`, `shot` and `portrait-audit`, with the portrait
index they read, are `wowdps-replay-extract`'s. `nix/dev/wrappers.nix`'s
`tools` table gives each its `wowdps-*` name (on main, `wowdps-db2`
alone). Each builds the release extractor from the live checkout, as the
`gen-*` scripts do, so neither rebuilds the other's, and then `exec`s
it[^wrappers]. A crate of its own would have depended on the extractor
for all of that.

**Stdlib-only, by calling CLIs.** Network, SQL and the compositor are
outside the standard library, and the extractor's dependency line allows
nothing outside the workspace[^contract]. So `sql` calls `curl` for the
WoWDBDefs manifest, schemas and TACT keys (the system's, as every
`gen-*` script already does) and `duckdb` for Parquet and the session,
and the replay's `shot` calls `lswt` and `grim` for the window. Main's
dev shells carry none of the three: `duckdb` must be on `PATH` (the
shells dropped it when DuckDB was bundled), and the replay's shell adds
`lswt` and `grim` on Linux. A tool run without one says which CLI is
missing.

**Labels without a font dependency.** The sheet's labels are drawn by a
small TrueType rasterizer (`font.rs`, the replay's extractor's) from the
GUI's bundled OFL Barlow (gui-logic's `fonts::BARLOW_MEDIUM`), read
unmodified, rather than by a system font or an image tool.

**The crops leave the Rust source.** The hand cuts moved from a `CROPS`
table in `portraitgen.rs` to a `portrait-crops.tsv` beside the replay
extractor's manifest, embedded the same way and checked by the same
tests. The portrait editor writes
the file itself ("Save to repo"), so installing a round of crops is no
longer a Rust splice.

## Consequences

- One command per question, and a cache that outlives the session:
  `~/.cache/wowdps/db2/<build>/` keeps every table ever asked for.
- **Watch:** the tools rely on the shells' CLIs. A shell change that drops
  `duckdb`, `lswt` or `grim` breaks them at run time, not at build time;
  the contract checks the wrappers' names, not those CLIs. It happened:
  since DuckDB was bundled into wowdps-history (2026-10-23) neither shell
  carries the `duckdb` CLI, so `wowdps-db2` needs one on `PATH`.
- **Not done here:** the `gen-*` scripts still fetch their own schemas and
  keys on every run. The db2 cache could serve them too.

[^wrappers]: `nix/dev/wrappers.nix`, `tools` and `toolWrapper`; the replay's dev shell adds `lswt` and `grim` for `wowdps-shot`.
[^contract]: CONTRACT.md §Dependencies: the extractor is stdlib-only (its `gpu` and `wcl` features, when it had them, went with the replay's extractor on 2026-10-30).
