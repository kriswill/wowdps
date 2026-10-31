---
type: Decision
title: Keep The Extractor's And The GUI's Shared Formats In Rubric And Proto
description: 'The files the extractor writes and the GUI examples read (a floor''s placement sidecar, the portrait crops and index) have one reader and writer each in wowdps-encounter-rubric, and every per-machine path resolves through proto::dirs; no new crate, the user''s call when the replay spike''s tools were refactored.'
tags: [dependencies, replay, tooling]
status: draft
generated: { by: claude-code/opus-5.5, at: 2026-10-07T21:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md §Dependencies — the rubric's and the extractor's lines
  - id: floor
    resource: ../../../crates/encounter-rubric/src/floor.rs
    title: floor.rs — Sidecar, Floors::scan and slug
  - id: portraits
    resource: ../../../crates/encounter-rubric/src/portraits.rs
    title: portraits.rs — the crops and index files
  - id: dirs
    resource: ../../../crates/proto/src/dirs.rs
    title: dirs.rs — the XDG directories
---

**Where:** [`wowdps-encounter-rubric`](../crates/encounter-rubric.md),
[`wowdps-proto`](../crates/proto.md),
[`wowdps-extract`](../crates/extract.md) and the GUI's two examples, the
replay spike and the portrait editor ([`wowdps-gui`](../crates/gui.md)).

## Context

The replay spike's tools grew their formats one reader at a time. A floor's
`.txt` placement sidecar was written by
gen-floors and read seven ways: a key-value map in
the stencil tracer and six hand parsers in the spike, which also read every
sidecar in the floors cache twice per load and scanned the directory
unsorted where the tracer sorted it. The writer quoted strings with Rust's
`{:?}` and no reader unescaped them. The portrait crops
(`tools/replay-extract/portrait-crops.tsv`) and the portraits cache's `index.tsv`
had three parsers between gen-portraits, its
audit and the portrait editor, the editor naming 22 columns by hand. The XDG
directories were resolved in 17 places, some reading an empty
`$XDG_DATA_HOME` as unset and some using it as the path, so the extractor
could write a cache the spike would never find.

## Decision

**The formats live where both sides already reach.** The extractor and the
GUI both depend on the rubric crate, and an encounter's map and its NPC keys
are rubric concepts. So `wowdps_encounter_rubric::floor` holds the sidecar: a typed
`Sidecar` whose `to_text` is byte-identical to what gen-floors wrote,
`Floors::scan` reading the cache once in name order, and `floor::slug`[^floor].
`wowdps_encounter_rubric::portraits` holds the crops and index files, with every
index column named[^portraits]. Both round-trip every real file on the
machine they were written on, and fixtures under `crates/encounter-rubric/fixtures/`
hold the round trip in tests.

**One home for the directories.** `wowdps_proto::dirs` resolves the data,
cache, state and config homes, an empty or relative variable counting as
unset as the spec says[^dirs]. Proto is stdlib-only, so the extractor takes
it as a normal dependency and the rubric takes it for its text sidecars;
CONTRACT.md's dependency lines say so[^contract]. `talents::data_path`,
which claimed to be the only copy, is now `dirs::data_path`.

**No new crate.** A stdlib-only crate for every per-machine asset format
was the other choice: a cleaner boundary, at the cost of a new entry in the
dependency policy, CLAUDE.md and this bundle. The user chose rubric and
proto on 2026-10-07.

## Consequences

The spike and the extractor agree on one format, one directory order and
one escaping rule. A floor's sidecar is read once per load. An empty or
relative XDG variable now lands under `$HOME` everywhere these paths go,
where before gen-rubric's journal text could land under the working
directory. The daemon, gui-logic and core still resolve their own
directories and were left alone, as outside the spike's refactor. The GPUI
helpers both examples share (pictures from PNGs, the theme's colors, the
fonts and window, small settings files written atomically through
gui-logic's `write_atomic`) lived in `crates/gui/examples/shared/`, since
the GUI crate has no library target the examples could import; since
2026-10-13 they are a crate of their own,
wowdps-gui-shared.

Landed in commits `2dfca1c`, `21e08d5`, `684f62e` and `e86baf8` (the
formats, the directories and the examples' helpers), then `db7e935`,
`51b54a9` and `702ee62` (the extractor's side) and `ca08cb9` (the rubric's
text sidecars).

[^floor]: `crates/encounter-rubric/src/floor.rs`: `Sidecar`, `Sidecar::to_text`, `Floors::scan`, `slug`.
[^portraits]: `crates/encounter-rubric/src/portraits.rs`: `Portrait`, `Crop`, `parse_crops`, `crops_tsv`, `IndexRow`, `index_tsv`, `parse_index`.
[^dirs]: `crates/proto/src/dirs.rs`: `data_home`, `cache_home`, `state_home`, `config_home`, `data_path`, `Base::resolve`.
[^contract]: CONTRACT.md §Dependencies: the rubric's and the extractor's lines.
