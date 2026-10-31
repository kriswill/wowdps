# tools

Dev-time tools: the game-data generators (`gen-*.sh`) and their censuses
(`census-*.sh`), the creatures seed, the dev unit, the overlay replay, the
GPUI docs fetcher, the skills updater and the local CodeQL check. The
extractor behind most of them is `tools/extract/` (`wowdps-extract`: the
game-data library and wowdps's own tables and caches), with its own
`AGENTS.md`. The replay's tools (the rubric's drafts, the floors,
portraits, level posts, cuts, Warcraft Logs, the extract and spike checks)
live in the private wowdps-replay repository
(https://github.com/kriswill/wowdps-replay), and the editors (the map
editor, the portrait editor and the map viewer) in wowdps-editors
(https://github.com/kriswill/wowdps-editors). Each script's header says
what it does and how to run it.

## Rules

- **Extracted game art never lands in the repository.** The art
  generators write per-machine caches under `~/.local/share/wowdps/`
  (`class-icons.bin`, `spell-icons.bin`, `spell-tips.bin`, `talents.json`,
  `talent-art.bin`), and every reader draws without them.
- **Generated files are never hand-edited.** The committed tables
  (`crates/core/src/*_spells.rs`, `keystone_timers.rs`,
  `open_world_maps.rs`, `crates/proto/src/placed/placed_spells.rs`) hold
  factual identifiers only and are regenerated once per game patch. A generator change reruns it and
  commits its output with it.
- **Every generator sources `tools/lib.sh`:** it builds the extractor once
  (`build_extract`), fetches the schemas the command itself names (`$extract tables
  COMMAND`) and the TACT keys, and finds the upscaler. Never keep a table
  list in a script.
- **Generators read the local install** (`$WOWDPS_WOW_DIR`, the config's
  `logs_dir`, or a Steam compatdata scan; or pass the folder holding
  `.build.info`). The network serves only WoWDBDefs schemas and TACT keys.
  Output is deterministic per build.
- **Curated tables need two proofs.** `role_spells`, `proc_spells` and
  `placed_spells` are hand lists in `tools/extract/src/` (`rolegen.rs`,
  `procgen.rs`, `placegen.rs`) that the generator checks against the
  client's own data, plus a committed real-log census (`tools/*-census.csv`
  from `census-*.sh`). Rerun the census when curating, and commit counts,
  never names. A new `placed` entry changes the replay cut: the stored
  replays take it when they are cut again (a replay format bump sends
  every one through the daemon's rewrite queue).
- **Real names stay on the machine.** The history store,
  `overlay-replay.sh` runs and censuses read real logs: their outputs live
  under `~/.local/share/wowdps/` or a scratch directory, never in the
  checkout. Report codes never enter the repository.
- **The seed lies under the addon.** `seed-creatures` writes Wowhead's
  classifications into the history store's `creatures.tsv` as rows seen
  at 0, so every sighting the addon records wins, and caches every answer
  under `~/.cache/wowdps/wowhead/` (two requests a second at most; a
  rerun fetches nothing it holds). Neither the answers nor the file enter
  the repository. Until the running daemon's build folds the file on disk
  in before each merge, a seed written under it needs a unit restart
  (asked first), or its next merge drops the seed.
- **Check a change by what it makes.** An extractor change reruns the
  generators it reaches on the base and the change, each built in its own
  worktree and target, and compares their outputs byte for byte; a change
  to the library's `pub` surface (`tools/extract/AGENTS.md`) or to the
  replay cut is checked again by wowdps-replay's `tools/extract-check.sh`
  and `tools/spike-check.sh` when its pin moves to it. Every difference
  must be meant.
- **A new script** carries a header comment that says what it does and
  how to run it.
- **`dev-unit.sh` drives the live daemon** (root `AGENTS.md`, "The live
  daemon"). **`skills-update.sh`** is the only way to refresh the vendored
  skills in `.claude/skills/`.
- **`codeql.sh` mirrors GitHub's CodeQL run** (rust and actions, the
  default suites, pinned query packs) and fails only on an alert missing
  from `codeql-baseline.txt`, which `--accept` rewrites. Run it as
  `wowdps-codeql`: the dev shells carry the CLI.

## Game tables and caches

```sh
tools/gen-class-spells.sh      # class_spells.rs (R8)
tools/gen-keystone-timers.sh   # keystone_timers.rs (R10)
tools/gen-open-world-maps.sh   # open_world_maps.rs (R10)
tools/gen-item-spells.sh       # item_spells.rs (R12; SpellEffect is big, be patient)
tools/gen-role-spells.sh       # role_spells.rs (R18, curated)
tools/gen-absorb-spells.sh     # absorb_spells.rs (R20)
tools/gen-proc-spells.sh       # proc_spells.rs (R26, curated)
tools/gen-placed-spells.sh     # proto's placed_spells.rs (what the replay cut keeps of placed things, curated)
tools/gen-icons.sh             # class-icons.bin (upscaled 4x; --no-upscale)
tools/gen-spell-icons.sh       # spell-icons.bin (~58 MiB)
tools/gen-talent-trees.sh      # talents.json (R14)
tools/gen-talent-art.sh        # talent-art.bin (~60 MiB)
tools/gen-spell-tips.sh        # spell-tips.bin (gui-logic's spell tips; ~7 s, 4 s when current)
tools/seed-creatures.sh        # NPC classifications from Wowhead into the store's creatures.tsv, under the addon's
```

## Checks and the rest

```sh
tools/extract/verify.sh [--game "$WOW_DIR"]    # the DB2 decoder vs wago.tools (network)
tools/dev-unit.sh install | status | profile debug | profile release | uninstall
tools/fetch-gpui-docs.sh [kit-version]         # the gitignored docs/gpui mirror
tools/skills-update.sh
wowdps-codeql [--accept] [rust|actions]        # tools/codeql.sh, gated on codeql-baseline.txt
```
