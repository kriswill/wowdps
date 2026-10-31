---
type: Decision
title: The Replay's Extractor Splits From The Game-Data Library
description: 'Before the replay leaves for a repository of its own, wowdps-extract splits in place: tools/extract keeps the game-data library and wowdps''s own generators, the store reader among them (a module stays when a crate on main, or a file wowdps''s crates read, needs it), tools/replay-extract (wowdps-replay-extract) takes the geometry, the floors, the portraits, the level posts, a pull''s cut, Warcraft Logs and the replay''s dev tools with the gpu and wcl features, and calls the library through pub items alone, and later the same day took the rubric''s drafter (gen-rubric) with the rubric; every output came out byte for byte.'
tags: [replay, tooling, dependencies, floors]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:30:00-07:00 }
sources:
  - id: library
    resource: ../../../tools/extract/AGENTS.md
    title: tools/extract/AGENTS.md — the library's surface the replay calls
  - id: replay
    resource: https://github.com/kriswill/wowdps-replay/blob/main/tools/replay-extract/AGENTS.md
    title: tools/replay-extract/AGENTS.md — the replay's extractor's rules
  - id: lib
    resource: ../../../tools/lib.sh
    title: tools/lib.sh — build_extract (the replay's copy adds build_replay_extract)
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md §Dependencies — extract
---

**Where:** [wowdps-extract](../crates/extract.md) and
wowdps-replay-extract, the scripts that
build them ([lib](../tools/lib.md)'s `build_extract`; the replay's copy
adds `build_replay_extract`), the dev
shell's tool wrappers (`nix/dev/wrappers.nix`) and
extract-check; checked with extract-check.

## Context

The replay leaves for a private repository of its own and will take
wowdps's crates by git dependency at one pinned revision (the replay's
split plan, "The extractor" and Step 0).
`wowdps-extract` on main is the game install's readers and wowdps's own
table generators; the replay branch had doubled it with the world's
geometry, the floors and their GPU path, the portraits, the rubric's
drafts, the stored pulls and their cut, Warcraft Logs and the replay's dev
tools. One crate cannot be split between two repositories, and the replay
cannot pin a crate the main repository does not hold.

## Decision

**Two crates, in place, the sort by who reads what.** A module stays in
`wowdps-extract` when a crate on main, or a file one of wowdps's crates
reads (a table, a cache, the rubric's base), needs it; everything else
moves to `tools/replay-extract`
(`wowdps-replay-extract`, its binary of the same name), as a library on
`wowdps-extract` with its own commands. The plan's first sort was
corrected by the modules' actual readers:

- `creature.rs` is the portrait renderer, not the creatures file: it
  moved with `portraitgen`, and took `m2`, `geom` and `chunk` with it.
- `classpowergen` (`class-power.bin`) and `tex-png` (the raid markers)
  feed only the replay spike: both moved, so `gen-class-power` and
  `gen-marker-icons` build the replay's binary.
- `spelltipgen` stays (gui-logic's `spell_tips` reads its file, a reader
  main gains with gui-logic's spell tips); the
  journal's reading it took from `rubricgen` moved into the library as
  `journal` first, so the spell tips build without the rubric drafter.
- `wowhead` (the daemon's `creatures.tsv`, through proto's codec),
  `placegen` (the `placed` table read by core's cut: the rubric's then,
  proto's since) and
  `db2sql` stay; the pictures (`img`, `png`, `deflate`, `atomic`) stay
  because `blp` and `icongen` need them.
- `rubricgen` (gen-rubric) stays, against the plan's first sort: the
  drafts it wrote were the rubric's base, which main then held under
  `crates/encounter-rubric/seasons/`, so their generator was public too.
  (Superseded the same day, below: it followed the rubric to the
  replay.)
  Its census reads the history store through `pulls`, which split: the
  store reader (`Fight`, its windows, `walk`, an encounter's positions)
  is the library's, and the positions and level posts files gen-floors
  and the stencil tracer take are the replay's `posts`. It reads the base
  rubric alone, never the curated set.
- The curated set's embed (`build.rs`, `curated`: the tuned and trash
  files under `curated/seasons/`, laid over the base as the replay lays
  them) is the replay's extractor's: gen-portraits' inventory and its
  test, gen-floors' default rubric, the scene and the cut's journal read
  it, all five in `wowdps-replay-extract`. `wowdps-extract` has no build
  script and embeds no curated file.
- `font.rs` (the contact sheets' labels) moved with `sheet`, but takes
  its Barlow from gui-logic (`fonts::BARLOW_MEDIUM`, the same file the
  GUI embeds, its license beside it) rather than by a path into
  `crates/gui-logic/fonts`, which will not resolve from another
  repository.

**The library through `pub` items alone.** The replay's half calls the
library only by public item, and the list is in the library's
`AGENTS.md`[^library], so a change to one is seen as the replay's. No
item changed visibility for it: what it calls was public already. The
journal's reading is a module of its own (`journal`), crate-private but
for its table list, so gen-spell-tips reads it without the drafter. The command line moved into the library
(`command`: `Args`, `Install`, `Cmd`, `Commands`), one parser for both
binaries, each naming itself in its usage and errors. `table::FILE_IDS`
stays whole in the library, since every table loads through `table::load`;
the replay's test holds both crates' lists to it, and nothing the
library's generators read is missing from it.

**Features and dependencies follow the code.** `gpu` (wgpu, pollster) and
`wcl` (ureq) moved to the replay's crate, so a build of
`wowdps-extract` was stdlib, core, the rubric and proto with no
features[^contract] (Superseded the same day, below: the rubric left
with the replay, and the crate is stdlib, core and proto). The replay's crate takes extract, core (its `cut`
and `wcl-pull` share the history store's index and replay cut), the
rubric, proto and gui-logic (the sheets' font). `portrait-crops.tsv` moved beside the replay's
manifest; gui-shared's `checkout::CROPS` and the rubric's crops test
follow it.

**Every script keeps its words.** The commands kept their names; a
replay script builds the replay's binary (`build_replay_extract`, in the
replay's copy of `tools/lib.sh`)[^lib],
the dev shell's `wowdps-sheet`, `wowdps-shot` and
`wowdps-portrait-audit` run it, and `wowdps-db2` and wowdps's generators
still run `wowdps-extract`[^replay].

## Consequences

- Nothing made moved. extract-check over a frozen store, the build before
  the split against the split (one binary against the two), CPU and GPU
  floors of nine encounters and Kings' Rest's levels, the rubric's
  drafts, the tables, icons, spell icons, talent art and trees, every
  portrait, the stencils, sheets and audit: 1,258 files identical, the 24
  floor sidecars differing in `made` alone and the one refusal log in its
  binary's name. Beside it, the spell tips, the class power, the placed
  table, the proc pairs, the level posts, the suggested turns and water,
  two scenes, a stored fight's cut and Ruby Life Pools' levels over the
  store's posts: 1,182 files identical but four sidecars' `made`. After
  rubricgen came back to the library, gen-rubric from `wowdps-extract`
  wrote the same 195 drafts and text sidecars with the same log; after
  the font moved, the sheets, the audit and the portraits came out the
  same.
- The editors' pin still names the crate before the split. At their next
  rev bump the map viewer and map editor take `wowdps-replay-extract`
  with `gpu` for the scene, the floors' files and the cut, and
  `wowdps-extract` for the install and its tables; the portrait editor
  writes the crops file at its new path through gui-shared.
- A table the replay starts reading needs its FileDataID in the
  library's manifest, a change on main once the replay has left.
- extract-check takes a build's `target/release` and runs each command
  from the binary that has it; one binary from before the split still
  runs every command.
- Later on 2026-10-30 the owner decided the encounter rubric leaves
  wowdps with the replay, base and curated alike (the split plan's "The
  rubric crate"). `rubricgen` and the gen-rubric command moved to
  the replay's extractor, which already took the rubric; the journal's
  reading and the `spelltip` readers it calls became `pub` for it, and
  the journal hands out facts of its own (`journal::SpellFacts`) that
  `rubricgen` makes the rubric's `Spell`, so `wowdps-extract` is
  stdlib, core and proto and names no rubric. `placegen` writes proto's
  `placed` table. gen-rubric from the replay's binary and from
  `wowdps-extract` at `2ee76e1` wrote the same drafts, text sidecars and
  log over a frozen store (174 files); gen-spell-tips' file and the
  placed table came out the same.

Landed on `wt/extract-split` as the journal module, the command line
into the library, the split, the dev shell's wrappers, rubricgen and the
store reader back in the library, and the sheets' font from gui-logic,
each its own commit. The library half came to main on 2026-10-30 (the
split's Step 0), after proto took the `placed` table and the rubric crate
left: `tools/extract` as the branch left it, wowdps's generators with
their scripts, gui-logic's `fonts::BARLOW_MEDIUM`, and `tools/lib.sh`
without `build_replay_extract`.

[^library]: `tools/extract/AGENTS.md`: "The library's surface is the replay's".
[^replay]: `tools/replay-extract/AGENTS.md`: its commands, on the library's command line.
[^lib]: `tools/lib.sh`: `build_extract`; the replay's copy adds `build_replay_extract`.
[^contract]: CONTRACT.md §Dependencies: extract.
