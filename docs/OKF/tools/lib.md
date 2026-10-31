---
type: Tool
title: lib
description: 'What the tools/*.sh scripts share: the extractor built once, the schemas and keys a command needs, and the upscaler.'
resource: tools/lib.sh
tags: [tool, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-30T19:50:00-07:00 }
---

`tools/lib.sh` is sourced, never run: a script sets `root` to the
checkout and sources it (`. "$root/tools/lib.sh"`). Every generator here
does, and [census-proc-spells](census-proc-spells.md) builds through it.

- `build_extract [ARGS]` builds `wowdps-extract` (release) from
  the checkout and sets `$extract` to it, under `$CARGO_TARGET_DIR` where
  that is set (since 2026-10-10: a worktree builds into a target of its
  own, never a shared one, and the script must run that build). Its
  replay twin, `build_replay_extract`, left with the replay's scripts for
  wowdps-replay (2026-10-30).
- `fetch_dbds COMMAND DIR` fetches the WoWDBDefs schema of every table
  COMMAND reads into DIR. The list is the extractor's own
  (`$extract tables COMMAND`, from each command's `Cmd::tables`), so
  no script keeps a list that must match a generator's `TABLES`.
- `fetch_keys FILE` fetches the wowdev TACTKeys list.
- `find_upscaler` prints `realesrgan-ncnn-vulkan`'s path, from `$PATH` or
  built from nixpkgs, or nothing; `upscale_4x UPSCALER IN OUT` runs its
  illustration model over a directory, as [gen-icons](gen-icons.md) does
  between its whole pass (`--whole-only`) and its cut.

## Seams

- **Offline.** The network is used for the schemas and keys alone, and
  not even that when they are given: `$WOWDPS_DBD_DIR` (a directory of
  `<Table>.dbd` files) and `$WOWDPS_TACT_KEYS` (a TACTKeys file) are copied
  instead, which is how a regenerate runs without the network or against
  schemas pinned by hand.
- **The build is the scripts' own.** It is the release build the dev
  shell's `wowdps-*` tool wrappers use (`nix/dev/wrappers.nix`), so neither
  rebuilds the other's.

## Source

- Script: [`tools/lib.sh`](../../../tools/lib.sh)
- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).
