---
type: Crate
title: wowdps-extract
description: DB2/CASC extractor generating wowdps game-data tables from a local WoW install.
resource: tools/extract
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-05T00:53:27-07:00 }
---

Dev-time game-data extraction (no runtime crate depends on this). Reads a client database (`.db2`, WDC5 format) plus its community schema (`.dbd` from WoWDBDefs) and emits the table as CSV — the same shape wago.tools serves, but computed locally from game files. Stdlib only, like the rest of the workspace's non-GUI crates. The lower half is the local-install reader: `casc` (Data/data `.idx` journals + archives), `blte` (chunk container, with `inflate` and `salsa20` underneath), and `tact` (.build.info → build config → encoding → root), which together turn a FileDataID or game path into file bytes without touching the network.

## Seams

- **Tables by FileDataID.** The live build's root manifest carries no name
  hashes, so a table is fetched by FileDataID, and WoWDBDefs'
  `manifest.json` maps table names to them. A `--file` lookup by name fails
  on such a build.
- **The hotfix cache is not read.** Rows the server pushes after a client
  build live in `_retail_/Cache/ADB/<locale>/DBCache.bin`, and every
  generator sees only the CASC tables. Season membership doesn't need it,
  but journal text and spell tuning drift there. Overlaying it is an open
  item of
  [Scope The Replay's Game Data To The Season The Client Names](../decisions/replay-data-from-the-season.md).
- **Generators write per-machine caches** (`*.bin`, `talents.json`) or
  committed tables (`*.rs`). Extracted art never lands in the repository.
- **Planned replay generators.** An encounters manifest and the floors,
  current season only:
  [Draw The Replay's Room From The Game's Own Floors](../decisions/replay-floors-from-game-files.md).
  `blp.rs` already decodes the map tiles, the minimap tiles and the journal
  portraits they need.

## Source

- Manifest: [`tools/extract/Cargo.toml`](../../../tools/extract/Cargo.toml)
- Root: [`tools/extract/src/lib.rs`](../../../tools/extract/src/lib.rs)
- Binaries: `wowdps-extract`

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
