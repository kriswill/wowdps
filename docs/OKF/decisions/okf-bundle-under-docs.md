---
type: Decision
title: Keep the knowledge bundle under docs/OKF
description: The OKF bundle lives at docs/OKF, beside the specs and plans it links to, and its scaffold passes derive Crate, Ruling and Tool docs from the sources so the graph never drifts from CONTRACT.md or the workspace.
tags: [okf, meta]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:37:41-07:00 }
sources:
  - id: okflight
    resource: https://github.com/kriswill/okflight
    title: okflight — the okf CLI
---

**Where:** [OKF Profile](../okf-profile.md), [okflight.toml](../../../okflight.toml).

## Context

The repository's rationale was spread across `CONTRACT.md` (binding but
flat), `CLAUDE.md` (working instructions), a dozen `docs/*.md` specs and
plans, and commit bodies. Nothing linked them, so "which fixture gates R18"
or "why does the GUI depend on model + proto only" meant reading everything.

## Decision

Adopt OKF v0.2 through okflight[^okflight] with the bundle at `docs/OKF` —
inside `docs/` so relative links to the specs and plans stay short, and
named after the format so the directory is self-describing. Three scaffold
passes derive the catalog layer: crates from the Cargo workspace, rulings
from `CONTRACT.md`'s table, tools from the `tools/*.sh` headers. The
authored layer (decisions, patterns, playbooks, fixtures) is written by
hand. `okf` reaches the dev shell as a flake input (FlakeHub, `0`
series) with a devenv twin, and the `knowledge-bundle` skill under
`.claude/skills/` teaches the maintenance loop.

## Consequences

A new crate, ruling or generator becomes a node on the next
`okf scaffold`; the graph can't drift from `CONTRACT.md`'s table because
the ruling docs are regenerated from it. Scaffolded docs are placeholders
until enriched — the entry quality bar in the skill applies. `viz.html` is
generated and gitignored.

[^okflight]: okflight — the okf CLI
