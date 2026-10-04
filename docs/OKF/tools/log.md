# Log

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
