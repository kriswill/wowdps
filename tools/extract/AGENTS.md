# tools/extract (wowdps-extract)

The dev-time game-data extractor and library. It reads the local install
(CASC, BLTE, DB2 with WoWDBDefs schemas) and writes wowdps's committed
tables and its per-machine caches (the icons, the talents, the spell
tips), seeds the history store's creatures file, reads the store's fights
(`pulls`) and the journal (`journal`), and carries the dev-time
`wowdps-db2`. No runtime crate depends on it. The replay's extractor
(`wowdps-replay-extract`, in the private wowdps-replay repository: the
encounter rubric's drafts, the floors, the world's geometry, the
portraits, the level posts, a pull's cut, Warcraft Logs, the stencils,
sheets and shots) is built on it as a library, by git dependency at a
pinned revision. Structure: `docs/OKF/crates/extract.md`. The scripts
that drive it: `tools/AGENTS.md`.

## Rules

- **Stdlib-only,** plus core and proto (CONTRACT.md §Dependencies), with
  no features and no build script: the GPU and HTTPS dependencies, the
  encounter rubric and its drafter (gen-rubric) and the curated set's
  embed are the replay's extractor's. Anything else needs a sign-off. The
  dev tools stay stdlib-only by calling the shell's CLIs (`curl`,
  `duckdb`).
- **One command line.** The library's `command` module is what both
  extractors' commands run on: `command/args.rs` is the one parser, and a
  word no command takes (an unknown flag above all) is an error;
  `command/install.rs` opens the install and its tables; `Commands` is a
  binary's list, usage and dispatch. Commands live under each binary's
  `cli/`, each module listing its `COMMANDS`. A command names the tables
  it reads once (`Cmd::tables`); `wowdps-extract tables COMMAND` (and the
  replay's `wowdps-replay-extract tables COMMAND`) prints them for
  `tools/lib.sh`.
- **Tables by FileDataID,** through WoWDBDefs' manifest
  (`table::FILE_IDS`, the replay's extractor's tables beside this
  crate's: both load through `table::load`). The hotfix cache
  (`DBCache.bin`) is not read.
- **One writer per shared format.** Per-machine paths go through
  `wowdps_proto::dirs`. The creatures file `seed-creatures` seeds goes
  through `wowdps_proto::creatures` (`parse`, `merge`, `render`). The spell
  tips (`gen-spell-tips`, `spelltipgen.rs`) are written here alone and
  read by gui-logic's `spell_tips` (the reader it gains with its spell
  tips), as the spell icons are: a layout change moves both and the
  version in both headers. The `placed` table (`gen-placed-spells`,
  `placegen.rs`) is proto's (`wowdps_proto::placed`), read by the
  daemon's replay cut and the replay. A 2x atlas sheet's members share
  their 1x twins' elements: cut a scale's member through `Atlas::committed` and
  `Atlas::cut`, not by element.
- **Inputs are stored fights.** The history store's cards and replay
  tier are read through `pulls.rs` (`Fight::read`, `walk`, `windows`: a
  boss pull whole, a keystone run's boss fights each over its own
  stretch), here and in the replay's extractor (gen-rubric's census among
  its readers); a replay this build cannot decode is passed over.
- **The journal is read once, here.** `journal.rs` reads the Encounter
  Journal's section tree and the spell tables through the client's
  difficulty fallback chain; gen-spell-tips takes the journal's words for
  spells from it, and the replay's gen-rubric drafts an encounter's
  abilities and `[spell]` table from it. Its facts are its own
  (`SpellFacts`), never a rubric type: the replay's extractor turns them
  into the rubric's `Spell`.
- **Blizzard's words and pixels stay on the machine.** Spell
  descriptions and the journal's words for spells go to `spell-tips.bin`,
  art to the caches.
- **Look before you fetch.** Use `wowdps-db2` (any game table as SQL,
  cached per build under `~/.cache/wowdps/db2/<build>/`) before any hand
  fetch, CSV or DuckDB session.
- **Gates.** The DB2 decoder: `tools/extract/verify.sh` against wago.tools'
  export (network; `--game` reads the install's own CASC). Any change:
  rerun the generators it reaches on the base and the change, each built
  in its own worktree and target, and compare their outputs byte for
  byte; wowdps-replay's `tools/extract-check.sh` checks the library again
  through the replay's extractor when its pin moves to the change.
- **The library's surface is the replay's.** `wowdps-replay-extract`
  calls this crate through `pub` items alone, and only these; a change to
  one is a change to the replay's extractor, built and checked with it in
  wowdps-replay (its `tools/extract-check.sh`) before its pin moves. Make
  an item public only when it calls it:
  - `command`: `Cmd`, `cmd`, `tables_cmd`, `Commands`, `at`, `read_text`,
    `make_dir`, `data_path`; `args::{Args, Usage, usage_error}`;
    `install::{Install, load_tables}`
  - `game::Game`: `fetch_fdid`, `build`
  - `table`: `Tables` (`get`, `find`), `Csv` (`col`), `cell`, `int`,
    `num`, `text`, `parse_csv`, `file_id`, `FILE_IDS`; and `load_refetching`,
    which the map editor calls
  - `blp::{decode, decode_level, levels}`; `png::{read, write, encode,
    decode, Decoded}`; `img::Image` with `bilinear_texel_wrap`,
    `bilinear_uv_wrap`, `bilinear_over_black`, `blend_rgb8`, `blend_rgbf`,
    `disc_coverage`, `resample_premultiplied`, `downscale_box`,
    `downscale_drawn`; `raw::zero::{byte, f32_at, u16_at, u32_at,
    vec3_at}` and `raw::be::{i16_at, u16_at, u32_at}`
  - `atomic::{write_atomic, write_if_changed, tmp_beside, rename_into}`
  - `artgen::{Atlas, Committed, ATLAS_TABLES}` (`Atlas::new`, `crop`,
    `committed`, `cut`)
  - `pulls::{Fight, Window, Walked, walk, encounter_positions}`: a stored
    fight read (`Fight::read`; its `id`, `card` and `cut`), its windows
    (`windows`, `Window::holds`)
  - `journal`: `Section` (its fields, `is_stage`, `is_creature`,
    `is_ability`), `sections`, `spell_names`, `Spells` (`new`, `descs`,
    `chain`, `triggers`, `facts`, `ctx`), `SpellFacts`, `join`, for
    gen-rubric (the replay's `rubricgen`); `spelltip::{Ctx, journal,
    ids_in, spell_refs}`, which its texts are read with
  - every generator's table list (`TABLES`, `TABLE`), for the replay's
    test that holds both crates' lists to `FILE_IDS`

```sh
cargo test -p wowdps-extract
cargo run -q -p wowdps-extract                   # no arguments: its CLI
wowdps-db2 JournalEncounter -c "select ..."      # --find WORD, --tables, --csv
```
