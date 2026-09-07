---
okf_version: '0.2'
---

# OKF

The wowdps knowledge bundle — a graph over the repository's crates and their
seams, the binding rulings of `CONTRACT.md`, the fixtures that gate them, the
game-data generators, and the decisions, patterns and playbooks behind them,
structured as an [Open Knowledge Format](https://github.com/GoogleCloudPlatform/open-knowledge-format/blob/main/SPEC.md)
v0.2 bundle. Conventions and tooling: the [OKF Profile](okf-profile.md). This
is the authored-rationale layer; it complements `CLAUDE.md` (working
instructions), `CONTRACT.md` (the binding spec) and `docs/*.md` (specs and
plans per roadmap step).

## Concepts

* [OKF Profile](okf-profile.md) - This bundle's conventions on top of OKF v0.2 — the type registry, what is scaffolded from the sources versus authored, link style, provenance stamps, and the okf tooling.

## Subdirectories

* [crates](crates/index.md) - The Cargo workspace members — the engine, the wire protocol, the daemon, the three frontends, the MCP server, the history reader and the DB2/CASC extractor — one doc each, scaffolded from Cargo.toml and the crate-root comment.
* [decisions](decisions/index.md) - Decision records — why the repository is the way it is.
* [fixtures](fixtures/index.md) - The committed synthetic combat logs under crates/core/fixtures/ — each exercises named rulings, carries hand-computed goldens recomputed independently by check.awk, and is gated by a test.
* [patterns](patterns/index.md) - Named architectural mechanisms the repository is built on — the daemon/client split, lazy segment loading, generated game-data tables, per-machine art caches.
* [playbooks](playbooks/index.md) - Operational how-tos for recurring tasks — regenerating game-data tables per game patch, bumping PROTO_VERSION, adding a ruling, running the real-log gates.
* [rulings](rulings/index.md) - CONTRACT.md's binding rulings R1–R22 — what counts as damage, healing, absorbs, segments, pets, visits, taken, spans, support, shields — one doc per ruling, scaffolded from the rulings table.
* [tools](tools/index.md) - The game-data generators and census scripts under tools/ — regenerated once per game patch from the local install through the wowdps-extract crate.
