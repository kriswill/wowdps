# crates/encounter-rubric (wowdps-encounter-rubric)

The seasonal encounter rubric: per-encounter TOML under
`seasons/<season>/<instance>/`, embedded at build time and resolved for one
difficulty at one tier, plus the replay's art formats (`floor`,
`portraits`) and the generated `placed` table. The schema, key by key:
`README.md`. Seams: `docs/OKF/crates/encounter-rubric.md`. Layering:
`docs/OKF/decisions/encounter-rubric-layers.md`.

## Rules

- **Main carries the base alone.** This crate's `seasons/` holds drafts,
  `instance.toml` and `season.toml` files, nothing else. The curated set
  (the hand-tuned `<id>-<slug>.toml` files and the instances' `trash.toml`
  files) is private: the replay keeps it in its own tree,
  `curated/seasons/<season>/<instance>/`, embeds it and lays it over this
  crate's base as the bundle `curated`
  (`docs/OKF/decisions/curated-set-leaves-the-rubric-tree.md`). Never commit
  a tuned or trash file under `seasons/`: `build.rs` warns on one and
  `the_embedded_base_holds_no_tuned_or_trash_file` fails. A tree that holds
  them still reads (`Curated(Embedded)`, `from_dir`), so the crate's
  readers keep working for a checkout laid out the old way.
- **The base stands alone.** Every encounter resolves and passes `check` at
  `Tier::Base` with no curated file; the embedded test resolves every
  encounter at every tier. A tuned file adds and refines; nothing the base
  needs may live only there.
- **Maps are base.** An encounter's map and view (its stencil, `ppy`,
  glows, rooms, places and the events they break on, its areas, paths and
  transition), its `travel` and any NPC the draft lacks or has wrong
  (`shares` among them) live in its instance's `instance.toml`
  (`[encounter.<id>.map]`, `.view`, `.travel`, `.npc`), so the base alone
  draws every room whole. A curated or user set may still amend an arena's
  map and view (the layer stack lets a set amend anything), never a
  level's.
- **Levels are the base's alone.** A dungeon level (`[level.<ui_map>]`:
  the same `Map`, a `View`, `travel`) lives in the base's instance file;
  a set's instance file giving one is refused, and a set's `[defaults]`
  skip levels. So do a dungeon's creatures and their enemy forces (the
  instance file's `[npc]` and `forces`): identities are base, what the
  trash does is the curated `trash.toml`'s, which gives no map, view,
  travel, phase or room and no clock from its start. The embedded test holds every level clean, and every
  court's encounter posted on its level, but for the exceptions it names.
- **Drafts and `placed` are generated** by `tools/gen-rubric.sh` (drafts
  into `seasons/`, never touching a tuned file) and
  `tools/gen-placed-spells.sh` (`src/placed/placed_spells.rs`, curated and
  census-proven). Both come to main with the extractor's library half;
  until then they run on the replay branch and their output is committed
  here. Never hand-edit a `*.draft.toml` or `src/placed/placed_spells.rs`:
  regenerate and bring the result over.
- **A mechanic is a kind of entry, not boss code.** A new behavior is a
  schema entry (with its README section and its `check`) that every boss
  can use; never special-case a boss in a reader.
- **Typos are errors.** Every struct is `deny_unknown_fields`; `check`
  reports a name an encounter uses but lacks, and an `only` naming no
  difficulty. A new schema field joins `write`'s `ORDER`, is `Gated` and is
  resolved, or the crate does not build.
- **Values inherit, presence does not.** Overrides follow the client's
  difficulty fallback chain (`difficulty::CHAINS`); an `only` list is exact.
- **Versioned.** Every file carries `schema = N`; `migrate` brings older
  files forward and refuses newer ones. `Rubric::version()` hashes every
  laid file.
- **Blizzard's words stay on the machine.** An ability's `text` never goes
  in a committed file; journal text lives in the per-machine sidecars under
  `$XDG_DATA_HOME/wowdps/rubric-text/`.
- **Embedded, so a change is a rebuild** (`build.rs` walks `seasons/`),
  but for a tree read at runtime: `with_curated_dir` (`with_curated_tree`
  over a walk) reads a tree's tuned and trash files (a checkout's
  `curated/seasons/`) in the first curated place (`Origin::Dir`), never
  its base files; `from_dir` (`from_tree`) reads a whole tree (a
  checkout's `seasons/`, its base) in the build's place (`base_origin` is
  `Origin::Dir`), refused whole on any file or subject that does not read.
  The replay lays a checkout's curated tree over its base so. Every layer's paths
  are `<season>/<instance>/<file>` relative to `seasons/`; user files
  under `$XDG_CONFIG_HOME/wowdps/rubric/` mirror it, and a bad one is
  named in `user_errors()` and left out, never fatal.
- **A map's turn is the rubric's alone.** A raid encounter's
  `[encounter.<id>.view] turn`, a level's `[level.<ui_map>.view] turn`,
  and an area's `turn` over them; unsaid, north up. `turn::room_turn`
  proposes one (the extractor's `suggest-turns`, the map editor's
  suggestion); nothing reads it to draw.
- **A place's swirl is drawn by `swirl::disc` alone** (or its bands,
  `swirl::rows`), in the replay and the editors alike, so their discs
  match; neither reimplements the twist, the levels or the rim.
- **One reader and writer per art format.** `floor` (a render's `.txt`
  sidecar: `Sidecar`, `Floors::scan`, `floor::slug`, and the `params` hash
  a render is stamped with and `Floors::stale` judges it by) and
  `portraits` (the crops file, `tools/replay-extract/portrait-crops.tsv`
  where the replay's extractor lives, and the portraits cache's
  `index.tsv`): the replay's extractor writes them and its GUI reads them
  through this crate alone.
- **The one engine-side crate naming serde/toml** (plus proto for
  `proto::dirs`). Readers go through it and never name serde/toml; the
  daemon reads `placed` through core's `PlacedTable`.
- **The replay checks a change too.** This crate's tests hold the base
  alone; the curated set is read and checked over it by the replay's own
  tests, and a rubric change the replay draws is held there by its dumps
  and shots before and after, at both tiers.

```sh
cargo test -p wowdps-encounter-rubric
```
