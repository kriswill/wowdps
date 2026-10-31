# Log

## 2026-10-30

- **Creation** — wowdps's generators that the replay branch grew, brought
  with the extractor's library half (Step 0 of the replay's move to its
  own repository): [gen-spell-tips](gen-spell-tips.md) (`spell-tips.bin`,
  for the `spell_tips` reader gui-logic gains with its spell tips),
  [gen-placed-spells](gen-placed-spells.md) and its census
  [census-placed-spells](census-placed-spells.md) (proto's `placed`
  table), [seed-creatures](seed-creatures.md) (the store's
  `creatures.tsv` from Wowhead), [wowdps-db2](wowdps-db2.md) (the game's
  tables as SQL) and [lib](lib.md), the scripts' shared helpers, without
  the replay's `build_replay_extract`.

- **Update** — every `gen-*.sh` and `census-proc-spells.sh` sources
  [lib](lib.md) and fetches the schemas its command names
  (`wowdps-extract tables COMMAND`), so no script keeps a table list;
  [gen-icons](gen-icons.md) upscales the art 4x round the extractor (208 px
  tiles, `--no-upscale` for the game's own 52 px), and
  [gen-talent-art](gen-talent-art.md)'s record says how its atlases are cut.

## 2026-10-23

- **Creation** — [codeql](codeql.md): GitHub's CodeQL default-setup run
  (rust and actions, the default suites, pinned query packs) as a local
  check, `wowdps-codeql`, that fails only on an alert missing from the
  committed baseline (`tools/codeql-baseline.txt`, seeded with main's ten);
  the extractor reads std from nixpkgs' stable library source, because the
  nightly's own `rust-src` degraded the database and hid 7 of the 10, and a
  degraded database fails the run; the CLI is the one unfree package the
  shells take from nixpkgs, allowed by name
  ([decision](../decisions/devenv-is-the-reference-shell.md)).

## 2026-10-16

- **Update** — [dev-unit](dev-unit.md): stopping a daemon takes down orphan
  overlays first and re-sends `stop` to any daemon that answers while it
  waits for the lock, so a reconnecting overlay no longer holds the lock
  with a fresh daemon through the wait.

## 2026-10-04

- **Creation** — [gen-open-world-maps](gen-open-world-maps.md): Map.db2's
  open-world maps (InstanceType 0) into `crates/core/src/open_world_maps.rs`,
  read by R10's door rule ([decision](../decisions/an-open-world-door-is-zoned-out.md)).

## 2026-10-03

- **Update** — [skills-update](skills-update.md): installs upstream's
  `code-review` as `code-rabbit-review` (`as` in the script, a frontmatter
  patch), so Claude Code's own `/code-review` plugin is no longer shadowed;
  the project no longer turns that plugin off.
- **Creation** — [skills-update](skills-update.md): refreshes the vendored
  CodeRabbit skills and re-applies this repo's patches over them, starting
  with autofix's push gate.

## 2026-10-01

- **Creation** — [overlay-replay](overlay-replay.md): a night's log replayed at
  speed into an isolated daemon while an overlay follows it on a headless
  output — the raid-week stand-in and the overlay cost measure.
- **Creation** — [dev-unit](dev-unit.md): the dev daemon's systemd unit, never
  scaffolded when the script landed in plan step 0.3.
- **Update** — [fetch-gpui-docs](fetch-gpui-docs.md): the mirror now serves
  the GPUI GUI as `crates/gui`, gui-new's name since the cutover.

## 2026-09-30

- **Creation** — [fetch-gpui-docs](fetch-gpui-docs.md): the gitignored GPUI /
  GPUI Kit mirror under `docs/gpui/` (Kit's pages, rustdoc-JSON digests,
  crate sources) behind [the gui-new decision](../decisions/gui-on-gpui.md).
- **Creation** — [gen-proc-spells](gen-proc-spells.md) and
  [census-proc-spells](census-proc-spells.md): the curated talent-proc table
  for [R26](../rulings/r26.md) step 4, proven by the client's text and a
  real-log census.

## 2026-09-29

- **Update** — [gen-item-spells](gen-item-spells.md) also emits each trinket
  spell's single owning item and its ItemSparse name (`trinket_of`, for
  [R26](../rulings/r26.md)); the regenerated table is build 12.1.0.
