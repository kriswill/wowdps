---
type: Reference
title: OKF Profile
description: 'This bundle''s conventions on top of OKF v0.2 — the type registry, what is scaffolded from the sources versus authored, link style, provenance stamps, and the okf tooling.'
tags: [okf, meta]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:37:41-07:00 }
sources:
  - id: okf-spec
    resource: https://github.com/GoogleCloudPlatform/open-knowledge-format/blob/main/SPEC.md
    title: OKF SPEC.md (v0.2)
  - id: okflight
    resource: https://github.com/kriswill/okflight
    title: okflight — the okf CLI
---

This bundle follows OKF v0.2[^okf-spec] with the profile choices below, and
is maintained with the okf CLI[^okflight] configured by the repository's
`okflight.toml`. okf's built-in validation defaults reproduce these rules
exactly, so the `[profile]` section there sets nothing.

## What this bundle is for

`CONTRACT.md` is the binding interface spec and `CLAUDE.md` the working
instructions; `docs/` holds the specs and plans that produced each roadmap
step. This bundle is the **graph over all of that**: one node per crate,
per binding ruling, per game-data generator and per fixture, plus the
decisions, patterns and playbooks that explain why the code is shaped the
way it is. It never restates code — a node says what its source can't
(rationale, seams, gotchas) and links to everything it touches.

## Type registry

| Type | Directory | Scaffolded from | Authored part |
|---|---|---|---|
| Crate | `crates/` | Cargo workspace members: `Cargo.toml` description + the root `//!` comment | seams, dependency policy, which rulings it implements |
| Ruling | `rulings/` | `CONTRACT.md`'s rulings table (the call + detail per R#) | links to the crates, fixtures and tools that carry it |
| Tool | `tools/` | `tools/*.sh` leading comments | the tables each regenerates, cadence (once per game patch) |
| Fixture | `fixtures/` | hand-authored (a log has no comment header) | what the fixture exercises, which ruling and test gate it |
| Decision | `decisions/` | hand-authored | context / decision / consequences, commit hashes in prose |
| Pattern | `patterns/` | hand-authored | a named mechanism the repo is built on |
| Playbook | `playbooks/` | hand-authored | a recurring procedure |
| Reference | `.` (root) | hand-authored | bundle-level meta docs like this one |

The scaffold passes live in `_okflight/scripts/` (`crates.ts`,
`rulings.ts`, `tools.ts`); `okf scaffold` never overwrites, so a
scaffolded stub can be enriched by hand and survives re-runs. A new crate,
ruling or generator needs no edit to the passes.

## Profile rules

- **Required frontmatter:** `type`. `title`, `description`,
  `generated` are recommended (warnings; errors under
  `okf validate --strict`). `resource`, `tags`, `status` are
  encouraged.
- **`resource:`** is a repo-root-relative path to the concept's primary
  source (`crates/core`, `tools/gen-icons.sh`, `CONTRACT.md`) — never a
  URL. Decisions and playbooks may omit it.
- **`generated`** is `{ by: <actor>, at: <ISO-8601> }` (OKF v0.2 §5.2):
  scaffolded docs carry `okflight/<version>` and the source's last-commit
  date; hand-authored docs say `human:kris`; agent-written ones
  `<agent>/<version>`. Add `verified: { by, at }` only after checking the
  content against its sources. `status` is `stable` | `draft` |
  `deprecated` (never delete a superseded entry — deprecate it).
- **Links are file-relative** (`../crates/core.md`), never `/`-rooted,
  and may escape into the repository (`../../../CONTRACT.md`) — the
  validator checks these resolve. Outside the repository, full URLs only.
- **Sources** live in frontmatter `sources` (`id`, `resource`,
  `title`); per-claim attribution is a footnote whose label is a
  `sources[].id`. Commit hashes are cited inline as `\`abc1234\``.
- **Body headings are H2**; the frontmatter `title` is the H1.
- **`index.md` files are generated** by `okf index`; only the blurb
  above the first heading is hand-maintained.
- **`log.md` is bundle-scoped**: an entry lives in the `log.md` of the
  directory owning its subject (`decisions/log.md`, `crates/log.md`, …;
  created on first entry), newest first under `## YYYY-MM-DD`, leading
  with `**Creation**` / `**Update**` / `**Deprecation**`. The root
  `log.md` records bundle-level events only.

## Tooling

`okf` is on the dev-shell PATH (flake input `okf`, devenv twin);
outside the shell `nix run .#okf -- <cmd>` or
`bunx @kriswill/okflight <cmd>`.

```sh
okf scaffold   # stub docs for new crates / rulings / tools (never overwrites)
okf index      # regenerate index.md listings
okf validate   # conformance + links — must exit 0 before committing
okf viz        # docs/OKF/viz.html, the interactive graph (gitignored)
```

[^okf-spec]: OKF SPEC.md (v0.2)
[^okflight]: okflight — the okf CLI
