# tools

Dev-time tools: the game-data generators (`gen-*.sh`) and their censuses
(`census-*.sh`), the creatures seed, the dev unit, the overlay replay, the
GPUI docs fetcher and the local CodeQL check. The extractor behind most of
them is `tools/extract/` (`wowdps-extract`: the game-data library and
wowdps's own tables and caches), with its own `AGENTS.md`. The replay's tools (the rubric's drafts, the floors,
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
  `talent-art.bin`, `edition-emblems.bin`), and every reader draws without
  them.
- **Generated files are never hand-edited.** The committed tables
  (`crates/core/src/*_spells.rs`, `keystone_timers.rs`,
  `open_world_maps.rs`, `crates/proto/src/placed/placed_spells.rs`, and
  Forever's under `crates/core/src/forever/`) hold factual identifiers
  only and are regenerated once per game patch. A generator change reruns
  it and commits its output with it.
- **Every generator sources `tools/lib.sh`:** it builds the extractor once
  (`build_extract`), fetches the schemas the command itself names (`$extract tables
  COMMAND`) and the TACT keys, and finds the upscaler. Never keep a table
  list in a script.
- **Generators read the local install** (`$WOWDPS_WOW_DIR`, the config's
  `logs_dir`, or a Steam compatdata scan; or pass the folder holding
  `.build.info`). The network serves only WoWDBDefs schemas and TACT keys.
  Output is deterministic per build. `gen-emblems` reads the launcher's own
  files instead, found beside the install (`--launcher DIR` names them):
  its play selector's emblem of each game, as the launcher keeps it.
- **One edition at a time, Retail's by default.** The product read is
  `.build.info`'s `wow` row unless `--product ROW` or `$WOWDPS_PRODUCT`
  names another (Forever's `wowf`). Each committed table and review twin
  is one edition's (`tablegen::committed`): Retail's beside the code,
  Forever's class, item, keystone, open-world, absorb, role-span and
  role-signal tables under `crates/core/src/forever/` and its placed table
  beside Retail's in proto (`forever_placed_spells.rs`) (the role signals are Forever's
  alone: a run of another product needs `-o`). A table script passes
  `--checkout "$root"`, so a run with no `-o` writes its own edition's
  committed table, and one whose edition has none of that table needs
  `-o`; a census-backed table's run reads its own edition's committed
  census under it (Retail's `tools/*-census.csv`, Forever's
  `tools/forever/*-census.csv`, but the role signals' Forever-only
  `tools/role-signals-census.csv`), unless `--census` names another. The extractor refuses
  any product's output on another edition's committed file, Retail's
  included, however `-o` spells it (`cli/owners.rs`). The caches in the
  data home are Retail's by default: another product's needs `-o`, and
  its readers look for it in a folder named for its edition beside
  Retail's (`forever/talents.json`, `forever/spell-icons.bin`,
  `forever/spell-tips.bin`, `forever/talent-art.bin`;
  `proto::dirs::in_edition`), never in Retail's. The class crests
  (`class-icons.bin`) are drawn by class and spec alone, as every game
  names them, and are not per edition.
  The games' emblems are the one cache of every game's: `gen-emblems`
  reads no product's client and takes no `--product`.
  What a non-Retail run writes names the product beside the build.
- **Curated tables need two proofs.** `role_spells`, `proc_spells`,
  `placed_spells` and Forever's `role_signals` are hand lists in
  `tools/extract/src/` (`rolegen.rs`, `procgen.rs`, `placegen.rs`,
  `signalgen.rs`, and Forever's role spans in `spangen.rs`; Forever's lists
  are curated by name and expanded to every id the client proves, since
  each of its ranks is an id of its own) that the generator checks against the client's own data,
  plus a committed real-log census (`tools/*-census.csv` from
  `census-*.sh`). Rerun the census when curating, and commit counts,
  never names: a census labels its logs `log 1`, `log 2`, … in the order
  given, since a log's file name says when it was played. A new `placed` entry changes the replay cut: the stored
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
  daemon").
- **`codeql.sh` mirrors GitHub's CodeQL run.** CI's scan is GitHub's
  default setup (the repository's settings, no workflow file;
  `.github/AGENTS.md`), run on every PR. `wowdps-codeql [--accept]
  [rust|actions]` runs the same languages (rust and actions), the default
  suites and the pinned query packs locally and, as GitHub fails a PR only
  on an alert not open on main, fails only on an alert missing from
  `codeql-baseline.txt`; `--accept` rewrites that baseline. Run it as
  `wowdps-codeql`: the dev shells carry the CLI. A run takes minutes on
  every core.

## Game tables and caches

```sh
tools/gen-class-spells.sh      # class_spells.rs (R8)
tools/gen-keystone-timers.sh   # keystone_timers.rs (R10)
tools/gen-open-world-maps.sh   # open_world_maps.rs (R10)
tools/gen-item-spells.sh       # item_spells.rs (R12; SpellEffect is big, be patient)
tools/gen-class-spells.sh --product wowf   # and the other three: Forever's, crates/core/src/forever/
tools/gen-role-spells.sh       # role_spells.rs (R18, curated; --product wowf: Forever's, by name)
tools/gen-absorb-spells.sh     # absorb_spells.rs (R20; --product wowf: Forever's, over tools/forever/'s census)
tools/gen-proc-spells.sh       # proc_spells.rs (R26, curated)
tools/gen-placed-spells.sh     # proto's placed_spells.rs (what the replay cut keeps of placed things, curated; --product wowf: Forever's)
tools/gen-role-signals.sh      # Forever's forever/role_signals.rs (R30, curated by name; Retail has none)
tools/gen-icons.sh             # class-icons.bin (upscaled 4x; --no-upscale)
tools/gen-spell-icons.sh       # spell-icons.bin (~58 MiB)
tools/gen-talent-trees.sh      # talents.json (R14)
tools/gen-talent-trees.sh --product wowf -o ~/.local/share/wowdps/forever/talents.json   # Forever's, where the talent tools read it
tools/gen-spell-icons.sh --product wowf -o ~/.local/share/wowdps/forever/spell-icons.bin  # and talent-art, spell-tips: Forever's, where the GUI reads them
tools/gen-talent-art.sh        # talent-art.bin (~60 MiB)
tools/gen-spell-tips.sh        # spell-tips.bin (gui-logic's spell tips; ~7 s, 4 s when current)
tools/gen-emblems.sh           # edition-emblems.bin (each game's emblem, from the launcher's own files)
tools/seed-creatures.sh        # NPC classifications from Wowhead into the store's creatures.tsv, under the addon's
```

## Checks and the rest

```sh
tools/extract/verify.sh [--game "$WOW_DIR"] [--product ROW]   # the DB2 decoder vs wago.tools (network)
tools/dev-unit.sh install | status | profile debug | profile release | uninstall
tools/fetch-gpui-docs.sh [kit-version]         # the gitignored docs/gpui mirror
wowdps-codeql [--accept] [rust|actions]        # tools/codeql.sh, gated on codeql-baseline.txt
```
