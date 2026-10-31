---
type: Tool
title: gen-talent-art
description: 'Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI''s own artwork cropped from the client''s texture atlases — per-spec pane background paintings, each hero tree''s round medallion, and the golden medallion ring — the per-machine cache the GUI''s talent viewer paints its panes with.'
resource: tools/gen-talent-art.sh
tags: [tool, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-07T15:10:00-07:00 }
sources:
  - id: script
    resource: ../../../tools/gen-talent-art.sh
    title: gen-talent-art.sh — the schemas, the TACT keys and one extractor run
  - id: artgen
    resource: ../../../tools/extract/src/artgen.rs
    title: artgen.rs — Atlas, the crops and the talent-art.bin layout
  - id: reader
    resource: ../../../crates/gui-logic/src/talent_art.rs
    title: talent_art.rs — the GUI's lazy seek-per-tile reader
---

`tools/gen-talent-art.sh [wow-dir]` fetches the WoWDBDefs schemas for its
four tables and the TACT keys, then runs
[`wowdps-extract`](../crates/extract.md)'s `gen-talent-art`[^script]. The
textures come from the local install's CASC storage. Like the icon bins,
the output is extracted art and stays out of the repository; the talent
viewer draws plain panels without it.

## Seams

- **Three kinds of tile, one file.** Each spec's pane painting
  (`talents-background-<class>-<spec>`, halved from 1612 × 774), each hero
  tree's medallion and the golden ring round it, as `(kind, id)` entries
  over raw RGBA at recorded offsets. The GUI seeks one tile at a time and
  never reads the whole 60 MiB[^artgen][^reader].
- **Names by token, never by locale.** A pane's element name is built from
  hardcoded English spec tokens. The obvious source,
  `ChrSpecialization.Name_lang`, is localized, so on a non-enUS install
  every background would land in `missing`. A hero medallion skips names
  altogether: `TraitSubTree.UiTextureAtlasElementID` names its element.
- **One cropper, shared.** `artgen::Atlas` resolves an element by name or
  id to its committed rectangle and cuts it from its sheet, decoding each
  sheet once. gen-portraits cuts the replay's boss
  medallion rings with it too, one of them the same gold ring this file
  holds[^artgen].

[^script]: `tools/gen-talent-art.sh`; the schemas it fetches are those `wowdps-extract tables gen-talent-art` names, `artgen::TABLES`.
[^artgen]: `tools/extract/src/artgen.rs`: the module doc's layout, `SPEC_TOKEN`, `Atlas` and `generate`.
[^reader]: `crates/gui-logic/src/talent_art.rs`: the index read once, tiles on demand.
