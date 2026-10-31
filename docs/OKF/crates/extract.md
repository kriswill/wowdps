---
type: Crate
title: wowdps-extract
description: DB2/CASC extractor generating wowdps game-data tables from a local WoW install, and the game-data library the replay's extractor builds on.
resource: tools/extract
tags: [crate]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:30:00-07:00 }
---

Dev-time game-data extraction (no runtime crate depends on this). Reads a client database (`.db2`, WDC5 format) plus its community schema (`.dbd` from WoWDBDefs) and emits the table as CSV — the same shape wago.tools serves, but computed locally from game files. Stdlib only, like the rest of the workspace's non-GUI crates, with no features. The lower half is the local-install reader: `casc` (Data/data `.idx` journals + archives), `blte` (chunk container, with `inflate` and `salsa20` underneath), and `tact` (.build.info → build config → encoding → root), which together turn a FileDataID or game path into file bytes without touching the network. Above it, the generators of wowdps's own tables and caches, and the library wowdps-replay-extract builds on.

## Seams

- **Tables by FileDataID.** The live build's root manifest carries no name
  hashes, so a table is fetched by FileDataID, and WoWDBDefs'
  `manifest.json` maps table names to them. A `--file` lookup by name fails
  on such a build. `table::FILE_IDS` holds the id of every table a
  generator reads, this crate's and the replay's extractor's alike (each
  loads its tables through `table::load`), each generator's `TABLES` its
  names; a table is decoded
  straight into cells (`table::decode`, what `parse_csv` reads back from
  `write_csv`'s text) and handed over as `table::Tables`.
- **The hotfix cache is not read.** Rows the server pushes after a client
  build live in `_retail_/Cache/ADB/<locale>/DBCache.bin`, and every
  generator sees only the CASC tables. Season membership doesn't need it,
  but journal text and spell tuning drift there. Overlaying it is an open
  item of
  [Scope The Replay's Game Data To The Season The Client Names](../decisions/replay-data-from-the-season.md).
- **Generators write per-machine caches** (`*.bin`, `talents.json`) or
  committed tables (`*.rs`). Extracted art never lands in the repository.
- **Pictures in one place.** `img.rs` holds the `Image` every decoder
  hands out and what the generators do to one: crops, the disc mask, three
  reductions (alpha averaged in, clear pixels left out, premultiplied),
  three bilinear samplers (texel indices wrapped, texture coordinates
  wrapped, pixel coordinates over black) and two blends, each rule kept
  apart where the arithmetic differs. `png::read` and `png::write` take an
  `Image` from and to a file, compressed by `deflate.rs`, the counterpart
  of `inflate.rs`. `blp.rs` decodes a texture at any mip level
  (`decode_level`).
- **The journal and its spells.** `journal.rs` reads the Encounter
  Journal's section tree (`Section`, `sections`), every spell's name and
  the spell tables by spell id through the client's difficulty fallback
  chain (`Spells`: `chain`, `facts`, `ctx`), and the journal's words for a
  spell (`journal_texts`, a listing's text by `join`). gen-spell-tips takes
  its journal words from here, and the replay's
  gen-rubric drafts an encounter's abilities,
  their text and its `[spell]` table from the same reading through the
  `pub` items. It was `rubricgen`'s until 2026-10-30, lifted out so the
  spell tips read it without the drafter. A spell's facts are the
  journal's own `SpellFacts`, not the rubric's `Spell`: since the rubric
  left wowdps with the replay (2026-10-30) this crate names no rubric, and
  `rubricgen`, now wowdps-replay-extract's, makes
  the one from the other.
- **The store's fights.** `pulls.rs` is the store reader: a stored
  fight's card and replay tier (`Fight::read`), its encounters as windows
  (a boss pull whole, a keystone run's boss fights each over its own
  stretch), a walk over the store passing over what this build cannot
  decode (`walk`, `Walked`), and an encounter's players' positions
  (`encounter_positions`). The replay's extractor reads the store through
  it (gen-rubric's census among its readers); the positions and level
  posts files are its own (`posts`).
- **Formats shared with their readers.** The XDG directories come from
  `wowdps_proto::dirs`, the creatures file seed-creatures writes from
  `wowdps_proto::creatures`, and `placed_spells.rs` (gen-placed-spells) is
  [wowdps-proto](proto.md)'s `placed` table, read by the daemon's replay
  cut (through [wowdps-core](core.md)'s `PlacedTable`) and the replay.
  `spell-tips.bin` is written here alone and read by `spell_tips`, the
  reader [wowdps-gui-logic](gui-logic.md) gains with its spell tips.
- **One command line, the library's.** The binary's commands live under
  `cli/`, a module by what they make: `tablegen` (the committed tables, one
  runner for all eight, with `proc-pairs` and `placed-spells-ids` for the
  censuses), `caches`, `db2` and `creatures`. Each lists its
  commands (`COMMANDS`: name, usage, runner) beside the code that reads
  their flags, and `main.rs` gathers the lists into a `Commands`, which
  makes the usage text, answers `tables` and dispatches. What a command
  runs on is the library's `command` module, which the replay's extractor
  runs its own commands on too: `command/args.rs` is the one parser: an `Args`
  that gives its words up as a command takes them, so a word no one took
  — an unknown flag above all — is an error naming it with the command's
  usage under its binary's name. `command/install.rs` is `Install` (`--wow-dir` or the one word left,
  `--keys`, and `--dbd-dir` for a command that reads tables), which opens
  the install and reads its tables, each through `table::load` (the
  library's; the map editor opens tables with `table::load_refetching`,
  which fetches a schema that will not decode this build again once, as
  `wowdps-db2` does for its cache). A command names the tables it reads
  (`Cmd::tables`) once: its runner, given its `Args`, reads them through
  `Install::open_tables`, and `wowdps-extract tables COMMAND` prints them:
  `tools/lib.sh` fetches their schemas by that list, so no script keeps
  one of its own.
- **Dev-time tools beside the generators.** Each a subcommand with a
  `wowdps-*` name in the dev shell, or a script's:
  - `db2sql.rs` is [wowdps-db2](../tools/wowdps-db2.md): any game table as
    SQL, cached per build.
  - `wowhead.rs` is [seed-creatures](../tools/seed-creatures.md): Wowhead's
    NPC classifications, by zone listing and by tooltip, laid under the
    addon's sightings in the history store's `creatures.tsv` through
    [wowdps-proto](proto.md)'s `creatures` codec.
  - `spelltipgen.rs` is [gen-spell-tips](../tools/gen-spell-tips.md): the
    replay's spell tips for every spell the store's replays and the
    curated tables name, the description through
    `spelltip` (read at base values: an expression's durations in
    seconds, a stat as 0, a product with a 0 in it 0; `eval_expr`, the
    talent viewer's too) and the journal's words through
    `journal::journal_texts`, each spell's parent walked up its
    description and trigger rows (`root_of`), and the specs whose logs
    omit an effect's press, counted over the store's replays
    (`players_of`, `omitted_presses`); `spell_tips`, the reader
    gui-logic gains with its spell tips, reads the file.

  They stay stdlib-only by calling `curl` and `duckdb`:
  [Keep The Dev Tools In The Extractor, Calling The Shell's CLIs](../decisions/dev-tools-in-the-extractor.md).
- **The library the replay's extractor builds on.** The floors, the
  world's geometry, the portraits, the HUD's class resources, the level
  posts, a stored pull's cut, Warcraft Logs, the stencils, the sheets and
  the shots are wowdps-replay-extract's
  (2026-10-30; in the private wowdps-replay repository since, where it
  takes this crate by git dependency at a pinned revision), with the
  `gpu` and `wcl` features, and so is the
  rubric's drafter (gen-rubric, `rubricgen`, moved there with the rubric
  the same day), so a build of this crate is stdlib, core and proto, with
  no features. That crate calls this one through `pub` items alone: the
  install (`game`), the tables and their schemas (`table`, `dbd`,
  `wdc5`), the pictures (`blp`, `img`, `png`), the atlases
  (`artgen::Atlas`), the store's fights (`pulls`), the journal
  (`journal`, `spelltip`'s text readers), the per-machine files
  (`atomic`) and the command line (`command`); `tools/extract/AGENTS.md` lists the items, so
  a change to one is seen as the replay's
  ([The Replay's Extractor Splits From The Game-Data Library](../decisions/replay-extract-splits-from-extract.md)).

## Source

- Manifest: [`tools/extract/Cargo.toml`](../../../tools/extract/Cargo.toml)
- Root: [`tools/extract/src/lib.rs`](../../../tools/extract/src/lib.rs)
- Binaries: `wowdps-extract`

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
