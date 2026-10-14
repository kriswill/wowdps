# crates/encounter-rubric (wowdps-encounter-rubric)

The seasonal encounter rubric: per-encounter TOML under
`seasons/<season>/<instance>/`, embedded at build time and resolved for one
difficulty at one tier, plus the replay's art formats (`floor`,
`portraits`) and the generated `placed` table. The schema, key by key:
`README.md`. Seams: `docs/OKF/crates/encounter-rubric.md`. Layering:
`docs/OKF/decisions/encounter-rubric-layers.md`.

## Rules

- **Main carries the base alone.** The hand-tuned `<id>-<slug>.toml` files
  are the curated layer and never land in this repository (the crate still
  reads one laid beside its draft).
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
  skip levels. The embedded test holds every level clean, and every
  court's encounter posted on its level, but for the exceptions it names.
- **Drafts and `placed` are generated** on the replay branch
  (`tools/gen-rubric.sh`, `tools/gen-placed-spells.sh` live there) and
  committed here. Never hand-edit a `*.draft.toml` or
  `src/placed/placed_spells.rs`: regenerate there and bring the result over.
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
  but for a tuned file: `with_curated_dir` reads a checkout's tuned files
  at runtime in the embedded ones' place (`Origin::Dir`), never its base
  files. Every layer's paths are `<season>/<instance>/<file>` relative to
  `seasons/`; user files under `$XDG_CONFIG_HOME/wowdps/rubric/` mirror it,
  and a bad one is named in `user_errors()` and left out, never fatal.
- **One reader and writer per art format.** `floor` (a render's `.txt`
  sidecar: `Sidecar`, `Floors::scan`, `floor::slug`, and the `params` hash
  a render is stamped with and `Floors::stale` judges it by) and
  `portraits` (the crops file and the portraits cache's `index.tsv`): the
  replay branch's extractor writes them and its GUI reads them through
  this crate alone.
- **The one engine-side crate naming serde/toml** (plus proto for
  `proto::dirs`). Readers go through it and never name serde/toml; the
  daemon reads `placed` through core's `PlacedTable`.

```sh
cargo test -p wowdps-encounter-rubric
```
