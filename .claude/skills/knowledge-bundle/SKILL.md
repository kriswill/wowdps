---
name: knowledge-bundle
description: Maintain the docs/OKF knowledge bundle — the repo's authored knowledge graph (crates, CONTRACT.md rulings, fixtures, game-data tools, decisions, patterns, playbooks). Use after adding a crate, ruling, fixture or tools/*.sh generator, when making a non-obvious design decision worth recording, or when asked to "update the knowledge bundle", "add a decision record", "validate the bundle", or "regenerate the knowledge graph".
---

# Maintaining the docs/OKF bundle

`docs/OKF/` is an [Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/open-knowledge-format/blob/main/SPEC.md)
bundle: markdown concept docs with YAML frontmatter, cross-linked into a
graph. It exists so rationale survives outside commit bodies and chat
history — **keep it current as part of any change, not as an afterthought.**
All conventions live in `docs/OKF/okf-profile.md`; read it before authoring.

## Commands (okf — nix-built from github:kriswill/okflight)

In the dev shell (`nix develop` / devenv) these are on `PATH` as `okf <cmd>`;
outside it, `nix run .#okf -- <cmd>` or `bunx @kriswill/okflight <cmd>`:

```sh
okf scaffold   # stub docs for new crates / rulings / tools (never overwrites)
okf index      # regenerate index.md listings (blurbs above the first heading are preserved)
okf validate   # conformance + link check; must exit 0 before committing
okf viz        # regenerate docs/OKF/viz.html, the interactive graph (gitignored)
```

The okf CLI is generic; THIS repo's scaffolding lives in
`docs/OKF/_okflight/scripts/` — `main.ts` plus one pass per scaffolded type
(`crates.ts` over the Cargo workspace members, `rulings.ts` over
`CONTRACT.md`'s rulings table, `tools.ts` over `tools/*.sh`), wired via
`okflight.toml [scaffold] script` and run through the injected
`ScaffoldContext` (vendored types: `scaffold-api.d.ts`). A new crate, ruling
row or generator script needs no edit to the passes; a new *kind* of source
is a new pass file.

## When to update what

| Change you just made | Bundle action |
|---|---|
| Added a crate, a CONTRACT.md ruling row, or a `tools/*.sh` script | `scaffold` then `index`; then bring the stub up to the quality bar below — scaffolded output is a placeholder, not an entry |
| Amended an existing ruling's text in CONTRACT.md | Re-run `scaffold --force` for that doc is NOT safe (it drops enrichment) — edit `docs/OKF/rulings/r<N>.md`'s call/detail by hand, or regenerate and re-apply the links |
| Added or changed a fixture under `crates/core/fixtures/` | Hand-write / update `docs/OKF/fixtures/<name>.md` (rulings exercised, the gating test, invariants) |
| Removed or renamed a component | Delete/rename its doc, fix inbound links (`validate` finds them), `index` |
| Made a non-obvious decision (anything deserving a long commit body — a PROTO_VERSION bump, a dependency-policy exception, a new generated table) | Add `docs/OKF/decisions/<slug>.md` (template below); cite commit hashes; link affected concepts both ways |
| Changed how a core mechanism works (segmentation, lazy loading, the hub, the history store, caches) | Update or add the matching `docs/OKF/patterns/*.md` |
| New recurring procedure | Add `docs/OKF/playbooks/<slug>.md` |
| Any of the above | Append a log entry to the **owning directory's** `log.md` (`decisions/log.md`, `crates/log.md`, `fixtures/log.md`, … — create on first entry) under today's `## YYYY-MM-DD`, newest first, `**Creation**`/`**Update**`/`**Deprecation**` lead, links relative to that file; then `index` + `validate` |

## Entry quality checklist

- **Description** = what it *is* + how *this repo* uses it, in one
  sentence. No name-restating filler.
- **Body** says what the source can't: seams, deliberate deviations,
  invariants, gotchas. Delete anything that restates the description or
  the code. CONTRACT.md stays the binding text — a ruling doc links to it,
  never competes with it.
- **`sources`** (frontmatter) lists what the doc leans on, each with an
  `id`, cited from the body by footnote (`…[^okf-spec]`). Commit hashes stay
  in prose as `` `abc1234` ``. Verify every URL resolves.
- **`generated`** is `{ by: <actor>, at: <ISO-8601> }` — `human:kris` when
  authored by hand, `<agent>/<version>` when an agent writes it. Add
  `verified: { by: human:kris, at }` only after checking the content
  against its sources; `status: deprecated` (never delete) when an entry
  stops being current.
- **Cross-links** — every concept the body names is a link: the crate that
  implements a ruling, the fixture that gates it, the tool that generates
  its table, related decisions; backlink from the target when the
  relationship is load-bearing. Aim for ≥2 edges beyond the scaffold.
- **Touched a component whose doc is still a stub?** Upgrade it in the same
  change.

## Decision-record template

```markdown
---
type: Decision
title: <Short Imperative Title>
description: <one sentence — what was decided and the key why.>
tags: [<topic>]
status: stable
generated: { by: human:kris, at: <ISO-8601 now> }
sources:
  - id: <short-id>
    resource: <URL or ../path/to/reference.md>
    title: <what it is>
---

**Where:** [<concept>](../crates/<name>.md).

## Context

<the problem/constraint that forced a choice>

## Decision

<what was chosen, and the mechanics that make it work>

## Consequences

<what got better, what to watch out for — per the reference.[^<short-id>]>
Landed in commits `<hash>`.

[^<short-id>]: <what it is>
```

## Profile rules that trip people up

- Frontmatter requires `type`; `title`, `description`, `generated` are
  recommended. Every footnote `[^x]` must match a `sources[].id`. A
  single-quoted YAML string doubles its apostrophes (`'a Priest''s'`).
- Links are **file-relative** (`../rulings/r18.md`) — never `/`-rooted;
  they may escape into the repo (`../../../CONTRACT.md`) but must resolve.
- Body section headings are **H2**; no H1 in concept bodies (frontmatter
  `title` is the H1).
- Never hand-edit generated `index.md` listing sections — only the blurb
  above the first heading; `viz.html` is generated and gitignored.
- The root `docs/OKF/log.md` records bundle-level events only (new
  directories or types, root-level docs, bundle-wide sweeps). Everything
  else logs in its directory's own `log.md`.
