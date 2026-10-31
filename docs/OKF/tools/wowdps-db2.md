---
type: Tool
title: wowdps-db2
description: 'The game''s client tables as SQL: `wowdps-db2 Table... [-c SQL]` decodes each named table from the local install once per game build into ~/.cache/wowdps/db2/<build>/ (CSV and Parquet), then opens DuckDB in memory with a view per cached table — how this repo looks things up in the journal, creature, spell and map tables.'
resource: tools/extract/src/db2sql.rs
tags: [tool, game-data, dev-shell]
status: draft
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:30:00-07:00 }
sources:
  - id: db2sql
    resource: ../../../tools/extract/src/db2sql.rs
    title: db2sql.rs — the manifest, the cache and the statements
  - id: cli
    resource: ../../../tools/extract/src/cli/db2.rs
    title: cli/db2.rs — the `sql` subcommand
  - id: wowdbdefs
    resource: https://github.com/wowdev/WoWDBDefs
    title: WoWDBDefs — table schemas and manifest.json (names to FileDataIDs)
---

`wowdps-db2` (the dev shell's name for `wowdps-extract sql`) answers
questions about the game's data in SQL. Without it, each question took
four hand steps: find the table's FileDataID, fetch its `.dbd` schema,
then `fetch` and `csv` through [`wowdps-extract`](../crates/extract.md),
then load the CSVs into DuckDB[^cli].

```sh
wowdps-db2 --find display                  # tables whose name holds "display"
wowdps-db2 JournalEncounter JournalEncounterCreature   # interactive, both views
wowdps-db2 -c "select ID, Name_lang from JournalEncounter where DungeonEncounterID = 3455"
wowdps-db2 --tables                        # what this build's cache holds
wowdps-db2 --csv CreatureDisplayInfo       # the CSV's path, for awk and friends
```

## Seams

- **Found by name, read from the install.** A name resolves through
  WoWDBDefs' `manifest.json`, any case, to its `db2FileDataID`[^wowdbdefs].
  The bytes come from the install's own CASC storage, so the table is the
  build the game runs. A name missing from the manifest fetches the
  manifest again once before it fails.
- **One cache per build.** `~/.cache/wowdps/db2/<build>/csv/` and
  `parquet/` hold every table ever asked for, and a patch starts a new
  directory[^db2sql]. Beside the builds sit `manifest.json`, `tactkeys.txt`
  (both fetched again after a week) and `dbd/` (fetched once). A schema
  that lacks this build's layout is fetched again once. `--refresh`
  re-fetches all of it for the tables named.
- **Views, not a database file.** Each session is DuckDB in memory with a
  view per Parquet file, so two sessions never contend for a lock, and a
  table asked for once is there in every later session. Parquet is typed
  from every row (`sample_size = -1`): a column a sample would call text
  stays a number.
- **What it calls.** `curl` for the manifest, schemas and keys (the
  system's, as the `gen-*` scripts use), and `duckdb` for the Parquet copy
  and the session, which it `exec`s so the shell is yours (the dev shells
  dropped it when DuckDB was bundled, 2026-10-23: put one on `PATH`). The
  crate stays stdlib-only:
  [Keep The Dev Tools In The Extractor, Calling The Shell's CLIs](../decisions/dev-tools-in-the-extractor.md).
- **Not the hotfix cache.** As with every generator, rows the server
  pushed after the build are not read (`DBCache.bin`).

[^db2sql]: `tools/extract/src/db2sql.rs`: `Manifest`, `cache_root`, `build_dir`, `to_parquet`, `views`, `cached`, `fetch`.
[^cli]: `tools/extract/src/cli/db2.rs`, `fn sql`, which reads a table through `command/install.rs`'s `fetch_db2` and `read_dbd` and the build through `game::build_info`.
[^wowdbdefs]: WoWDBDefs, `manifest.json` and `definitions/<Table>.dbd`.
