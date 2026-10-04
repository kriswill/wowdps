---
type: Decision
title: devenv Is The Reference Shell, And The Flake Shell Mirrors It
description: 'devenv is the dev shell for local builds and CI; the flake''s `nix develop` shell mirrors what devenv''s languages.rust adds (clang as CC/CXX, its Clang+LLD linker script, rebuilt to the same store path), PKG_CONFIG_PATH is pinned in both, sccache wraps rustc, and the contract fails if the two ever hand cargo different inputs — because cargo fingerprints those inputs and rebuilt all 567 crates on every switch while they drifted.'
tags: [nix, build, ci]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-04T16:30:00-07:00 }
sources:
  - id: env
    resource: ../../../nix/dev/env.nix
    title: nix/dev/env.nix — the pinned values, the linker mirror, sccache
  - id: contract
    resource: ../../../nix/dev/contract.nix
    title: nix/dev/contract.nix — wowdps-dev-contract, run in both shells
  - id: ci
    resource: ../../../.github/workflows/ci.yml
    title: The CI workflow — devenv, the two-shell contract, the cargo caches
---

**Where:** `nix/dev/` (the environment both shells import), `devenv.nix`,
`flake.nix`'s `devShells.default`, and `.github/workflows/ci.yml`. The
replay spike's builds exposed it ([A Fight Replay Captured Once And
Interpreted Per Tier](fight-replay-captured-once.md)), but it touches every
crate.

## Context

The repository carried two dev shells: `nix develop` and devenv. They were
called twins that "can no longer drift", because both import `nix/dev`. They
did drift, in what each shell's own machinery adds:

- **The C compiler.** devenv's `languages.rust` puts clang on PATH, so
  `CC=clang`; mkShell left `CC=gcc`.
- **The pkg-config path.** Each hook lists its own packages: devenv adds
  valgrind and bash-interactive, in another order.
- **The Rust linker.** devenv points cargo at its `devenv-rust-linker` (Clang
  driving `ld.lld`); the flake shell named none.

Cargo decides freshness from a fingerprint of a crate's inputs, never its
output, and keeps one fingerprint per crate in `target/`. Two kinds of input
matter here:

- **The linker,** which rides `-C linker` on every rustc call.
- **Every variable a build script names in `rerun-if-env-changed`.** The
  fontconfig, freetype, xkbcommon and cc-built crates under GPUI watch `CC`
  and `PKG_CONFIG_PATH`.

So each switch between the shells rebuilt the whole tree: 567 crates, about
65 s, measured 2026-10-04, however identical the output.

## Decision

**devenv is the reference shell, locally and in CI.** The flake shell copies
it, with no third choice imposed on both[^env]:

- **The compiler.** It adds the same clang, whose setup hook sets
  `CC=clang` and `CXX=clang++`.
- **The linker.** It exports devenv's linker script, written again with the
  same name and text, so it builds to the same store path. Measured:
  `7rv8s9…-devenv-rust-linker` in both.
- **pkg-config.** `PKG_CONFIG_PATH` is the one value neither shell can be
  left to compute. It is pinned (the nine directories the GUI's build
  needs) and exported last by both: mkShell's `shellHook`, devenv's
  `enterShell`.

**sccache wraps rustc in both** (`RUSTC_WRAPPER`). Where cargo's
fingerprint says "rebuild", sccache returns the artifact for an identical
compiler invocation. It cannot cache build scripts, proc-macros or binaries,
so it softens a rebuild rather than removing it. Its keys hold absolute
paths, so it hits only at the same paths: in CI, whose runner builds at one
path every run, and in a checkout that cargo rebuilds without rustc's inputs
changing (a `cargo clean`, a branch switched away and back). A second
worktree misses; `SCCACHE_BASEDIRS` did not normalise Rust compiles (tested
2026-10-04).

**The contract checks the mirror.** `wowdps-dev-contract` asserts the
pinned `PKG_CONFIG_PATH`, `CC=clang` and `CXX=clang++`, the linker script's
exact path, and sccache[^contract]. In devenv, the linker check compares the
mirror's store path against devenv's own, so a devenv upgrade that changes
its script fails the contract instead of silently costing rebuilds.

**CI runs in devenv,** and checks the environment first: `devenv test`, then
`nix develop -c wowdps-dev-contract`, before any cargo step. Two cargo
caches are layered[^ci]:

- **`Swatinem/rust-cache`,** keyed on `flake.lock` and `devenv.lock`, for
  the whole `target/`.
- **sccache's GitHub Actions backend,** per crate, which outlives a key
  change. Its token comes from `mozilla-actions/sccache-action`, whose own
  binary and annotation go unused.

## Consequences

**Measured after the change.** Alternating devenv, a clean `nix develop`,
devenv and `nix develop` again recompiled 15, 0, 0 and 0 crates.

**Old shells keep old values.** A shell entered before an environment
change keeps its values until it is restarted. devenv's live reload, with
its evaluation cache, did not pick this change up.

**The flake shell's job narrows.** It is now a faithful mirror for people
without devenv, and the input for `.#wowdps` and `.#wowdps-gui`. The
package builds (crane) never used either shell.

Landed on `build/devenv-reference-shell`.

[^env]: `nix/dev/env.nix`: `pinned`, `rustLinker` (the mirror), `flakeShellHook`, `flakePackages`, `rustcWrapper`.
[^contract]: `nix/dev/contract.nix`: the shared-values block and the Linux linker/compiler checks.
[^ci]: `.github/workflows/ci.yml`: the ci job's devenv install, sccache-action, rust-cache key and contract step.
