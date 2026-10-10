# tools

Dev-time tools: the game-data generators (`gen-*.sh`) and their censuses
(`census-*.sh`), the extractor `wowdps-extract` (`tools/extract/`), the dev
unit, the overlay replay, the GPUI docs fetcher and the skills updater.
Each script's header says what it does and how to run it; each has a doc
under `docs/OKF/tools/`, and the extractor's structure is
`docs/OKF/crates/extract.md`.

## Rules

- **Extracted game art never lands in the repository.** The art
  generators write per-machine caches under `~/.local/share/wowdps/`
  (`class-icons.bin`, `spell-icons.bin`, `talents.json`, `talent-art.bin`),
  and every reader draws without them.
- **Generated tables are never hand-edited.** The committed tables
  (`crates/core/src/*_spells.rs`, `keystone_timers.rs`,
  `open_world_maps.rs`) hold factual identifiers only and are regenerated
  once per game patch. A generator change reruns it and commits the table
  with it.
- **Generators read the local install** (`$WOWDPS_WOW_DIR`, the config's
  `logs_dir`, or a Steam compatdata scan; or pass the folder holding
  `.build.info`). The network serves only WoWDBDefs schemas and TACT keys.
  Output is deterministic per build.
- **Curated tables need two proofs.** `role_spells` and `proc_spells` are
  hand lists in `tools/extract/src/rolegen.rs` and `procgen.rs` that the
  generator validates against the client's own data, plus a committed
  real-log census (`tools/*-census.csv` from `census-*.sh`). Rerun the
  census when curating. A census reads real logs: commit counts, never
  names.
- **The extractor stays stdlib-only** (plus `wowdps-core`); a new
  dependency needs a CONTRACT.md sign-off.
- **A new script** carries a header comment (the bundle scaffolds from it):
  run `okf scaffold`, then bring `docs/OKF/tools/<name>.md` up to the
  bundle's quality bar.
- **`dev-unit.sh` drives the live daemon** (root `AGENTS.md`, "The live
  daemon"). **`skills-update.sh`** is the only way to refresh the vendored
  skills in `.claude/skills/`.
- **`overlay-replay.sh` replays real logs:** its inputs and results hold
  real names and stay outside the checkout.

```sh
tools/gen-class-spells.sh      # class_spells.rs (R8)
tools/gen-keystone-timers.sh   # keystone_timers.rs (R10)
tools/gen-open-world-maps.sh   # open_world_maps.rs (R10)
tools/gen-item-spells.sh       # item_spells.rs (R12; SpellEffect is big, be patient)
tools/gen-role-spells.sh       # role_spells.rs (R18, curated)
tools/gen-absorb-spells.sh     # absorb_spells.rs (R20)
tools/gen-proc-spells.sh       # proc_spells.rs (R26, curated)
tools/gen-icons.sh             # class-icons.bin (per-machine)
tools/gen-spell-icons.sh       # spell-icons.bin (per-machine, ~58 MiB)
tools/gen-talent-trees.sh      # talents.json (R14, per-machine)
tools/gen-talent-art.sh        # talent-art.bin (per-machine, ~60 MiB)
tools/extract/verify.sh                    # DB2 decoder vs wago.tools (network)
tools/extract/verify.sh --game "$WOW_DIR"  # tables from the install's CASC
tools/dev-unit.sh install | status | profile debug | profile release | uninstall
tools/fetch-gpui-docs.sh [kit-version]     # the gitignored docs/gpui mirror
tools/skills-update.sh
```

`cargo run -q -p wowdps-extract` with no arguments prints the extractor's
CLI (`tools/extract/src/main.rs`); its `fetch` pulls any file from local
CASC storage by FileDataID, network-free.
