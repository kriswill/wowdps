---
type: Decision
title: Keep The Curated Set Out Of The Rubric Crate's Tree
description: 'On the replay branch the hand-tuned encounter files and the instances'' trash files moved from crates/encounter-rubric/seasons/ to curated/seasons/ at the repository''s root, in the same layout; the rubric crate embeds the base alone, as on main, and the replay (and the extractor''s replay tools) embed the set and lay it over that base as one bundle, so the replay''s move to its own repository takes the set as a directory and the crate it leaves behind is main''s; the replay''s --seasons reads a checkout''s two trees.'
tags: [replay, rubric, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-30T20:40:00-07:00 }
sources:
  - id: agents
    resource: ../../../crates/encounter-rubric/AGENTS.md
    title: The rubric crate's AGENTS.md — its seasons/ the base alone
  - id: build
    resource: ../../../crates/encounter-rubric/build.rs
    title: The rubric crate's build.rs — embeds seasons/ alone, warns on a tuned or trash file there
  - id: guard
    resource: ../../../crates/encounter-rubric/src/tests.rs
    title: The rubric crate's tests — the_embedded_base_holds_no_tuned_or_trash_file
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md) (its
`seasons/` the base alone, `build.rs` warning on a tuned or trash file
left there and a test refusing one[^build][^guard]; `with_curated_tree`),
and on the replay branch wowdps-replay (`build.rs`, `curated`,
`pulls::Tree`), wowdps-replay-extract (`build.rs`, `curated`; first in
wowdps-extract) and wowdps-gui-shared (`checkout::CURATED`), checked with
the replay's spike-check. Step 2 of the replay's split in place, after the
replay was made a crate; the layers are
[Resolve An Encounter Through An Ordered Layer Stack](encounter-rubric-layers.md)'s.
Main took the crate at this state (its schema, layer stack and base
season files) in Step 0 of the replay's move to its own repository.

## Context

The replay is leaving for a repository of its own, and the plan keeps the
rubric crate in wowdps: main already holds it as the public base, the
daemon links it for the generated `placed` table, and the replay will lay
its curated set over it from a pinned revision (the split's plan,
`docs/plan-replay-split.md` on the replay branch, "The rubric crate",
option (a), recommended and assumed). On the branch the
curated set (the hand-tuned `<id>-<slug>.toml` files beside their drafts
and the instances' `trash.toml` files, 22 files) lay inside the crate's
`seasons/`, embedded by its build script and read as
`Curated(Embedded)`. So the crate on the branch was not the crate on main,
the set could not move without picking files out of a crate's tree, and
every reader of `Rubric::embedded()` read the set whether it meant to.

## Decision

**The set lives at `curated/seasons/`, the base in the crate.** Every file
the crate's own classifier (`FileRole`) calls a tuned encounter file or a
trash file moved, each a rename, to the same path under the repository's
`curated/seasons/`; the drafts, `instance.toml` and `season.toml` stayed.
The crate's `seasons/` is then main's kind of tree, its tests and goldens
(`resolved-maps.txt`, `resolved-levels.txt`) passing over the base alone
and unchanged[^agents].

**The replay embeds the set and lays it as a bundle.** Its build script
writes an `include_str!` per file of `curated/seasons/`, and
`curated::rubric` lays them over `Rubric::embedded()` with
`Rubric::with_curated("curated", …)` (`crates/replay/src/curated.rs`:
`files`, `rubric`, `ORIGIN`): the bundle stands where
the embedded tuned files stood, so every encounter, trash and level
resolves as before at every tier, which its tests hold against the files
read as one tree as they were (`from_files` of base and set together), key
by key with their provenance (`Curated(Bundle("curated"))` where it was
`Curated(Embedded)`) (`crates/replay/src/curated/tests.rs`). What the rubric crate's tests checked of the
tuned and trash files (every encounter and trash checked at every tier,
Ruby Life Pools' trash) moved with them. The extractor, whose replay tools
read the rubric as the replay does (gen-portraits' inventory of NPCs, the
floors' and the scene's maps, the cut's journal), embeds the same set the
same way and reads it where it read `Rubric::embedded()`. (Update
2026-10-30: the embed lives in the replay's extractor,
wowdps-replay-extract, where all five
readers are since the extractor split; the public library embeds no
curated file, and its gen-rubric reads the base alone: "The Replay's
Extractor Splits From The Game-Data Library", the replay's record.)

**`--seasons` names a checkout.** It reads the checkout's base from
`crates/encounter-rubric/seasons` whole (`from_tree`) and lays its
`curated/seasons` over it in the first curated place (`with_curated_tree`,
added so the walk that fingerprints the tree is the one laid); a tuned
or trash file left in such a checkout's base refuses it, named, since
the set laid over it would pass the file over unseen; a checkout's base
named itself, as before, reads its checkout where that holds the set,
and any other `seasons/` tree is read whole as before (`crates/replay/src/pulls.rs`: `Tree::of`,
`Walked`, `Held::read`). gui-shared
names the set's place in a checkout (`checkout::CURATED`) beside the
base's (`SEASONS`).

## Consequences

- Nothing resolved moved: spike-check's 128 dump files and 228 shots over
  a frozen store came out byte for byte against the baseline at
  `d2f9e78`, and `--dump` under `--seasons` (the old single tree with the
  old build, the checkout by its root and by its base with the new) came
  out identical over 56 dumps. `Rubric::version()` changed, as it folds
  the bundle's name and files in another order: the control file's
  `.done` and stderr say another version for the same rubric.
- The editors save a tuned or trash file to `curated/seasons/` now
  (`checkout::CURATED`); their own reads of `embedded_files()` hold the
  base alone. That is their change to make when they bump their pin.
- gen-rubric writes drafts into the crate's tree as before; its draft
  header's "beside it" still names the tuned file's old place.
- When the replay moves, `curated/` goes whole as `curated/seasons/`, and
  the rubric crate stays as main has it. The two build scripts that
  embed the set (the replay's, the replay's extractor's) become the
  replay repository's.

Landed on `wt/replay-crate` (for `spike/replay`) after `8537bc1` (the
replay made a crate) and `cd1f90e` (`with_curated_tree`), in the commit
that moves the files.

[^agents]: `crates/encounter-rubric/AGENTS.md`: its `seasons/` the base alone.
[^build]: `crates/encounter-rubric/build.rs`: `curated_file`, the warning naming the file and where it belongs.
[^guard]: `crates/encounter-rubric/src/tests.rs`: `the_embedded_base_holds_no_tuned_or_trash_file`.
