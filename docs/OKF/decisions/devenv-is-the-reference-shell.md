---
type: Decision
title: devenv Is The Reference Shell, And The Flake Shell Mirrors It
description: 'devenv is the dev shell for local builds and CI; the flake''s `nix develop` shell mirrors what devenv''s languages.rust adds (clang as CC/CXX, the linker it names — on x86_64 its Clang+LLD script, rebuilt to the same store path), PKG_CONFIG_PATH is pinned in both, sccache wraps rustc, and the contract fails if the two ever hand cargo different inputs — because cargo fingerprints those inputs and rebuilt all 567 crates on every switch while they drifted.'
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
- **The linker.** It exports the linker devenv names, by devenv's own rule:
  on `x86_64-unknown-linux-gnu` its linker script (Clang driving `ld.lld`),
  written again with the same name and text so it builds to the same store
  path (measured: `7rv8s9…-devenv-rust-linker` in both); on any other
  target, clang itself.
- **pkg-config.** `PKG_CONFIG_PATH` is the one value neither shell can be
  left to compute. It is pinned and exported last by both: mkShell's
  `shellHook`, devenv's `enterShell` (`lib.mkAfter`). The pin is derived
  from the libraries the GUI's build scripts probe (xkbcommon, fontconfig,
  xcb) and everything they propagate, so a nixpkgs bump that changes a
  propagation moves it.

**sccache wraps rustc in both** (`RUSTC_WRAPPER`). Where cargo's
fingerprint says "rebuild", sccache returns the artifact for an identical
compiler invocation. It cannot cache build scripts, proc-macros or binaries,
so it softens a rebuild rather than removing it. Its keys hold absolute
paths, the linker and the sysroot, so it hits only at the same paths with
the same toolchain: in CI, whose runner builds at one path every run, and
in a checkout that cargo rebuilds without rustc's inputs changing (a `cargo
clean`, a branch switched away and back). A second worktree misses;
`SCCACHE_BASEDIRS` did not normalise Rust compiles (tested 2026-10-04).

**The contract checks the mirror.** `wowdps-dev-contract` asserts the
pinned `PKG_CONFIG_PATH`, `CC=clang` and `CXX=clang++`, the linker's exact
path, and sccache[^contract]. In devenv, the linker check compares the
mirror's path against devenv's own, so a devenv upgrade that changes its
choice fails the contract instead of silently costing rebuilds. The
Linux-only values are checked on Linux alone.

**CI runs in devenv,** and checks the environment first: `devenv test`, then
`nix develop -c wowdps-dev-contract`, before any cargo step. Two cargo
caches are layered[^ci]:

- **`Swatinem/rust-cache`,** for the whole `target/`. Its `shared-key`
  carries a hash of the locks and the environment's sources (`flake.lock`,
  `devenv.lock`, `devenv.yaml`, `devenv.nix`, `nix/dev/`). Its own key
  follows rustc's version and `Cargo.lock` but not nixpkgs, whose glibc the
  cached build scripts are linked against (#79 after #80). And an unchanged
  key is never saved again, so an environment change would otherwise leave
  every PR a stale `target/` for good. The action's `key` input is ignored
  once `shared-key` is set. It asks rustc for its version in the dev shell
  (`cmd-format: devenv shell -- {0}`): outside it, the runner's rustup
  downloads that day's nightly to answer, and the key moved daily.
- **sccache's GitHub Actions backend,** per crate. It serves a dependency a
  `Cargo.lock` change leaves alone, where `target/` restores only in part.
  A nixpkgs or rust-overlay bump misses it entirely, as it must. A
  first-party `actions/github-script` step exports the cache service's
  variables to it, as `mozilla-actions/sccache-action` would, without
  downloading a second sccache.

Every action in every workflow is pinned to a commit SHA, its version in a
trailing comment for Dependabot to move, and no checkout persists its
credentials.

**Main warms both caches for every PR.** GitHub scopes an Actions cache to
the ref that wrote it. A PR's entries are readable by that PR alone, and
main's scope is the only one every PR can read. With checks on PRs only
(the repository's rule), every new PR started cold.

`cache-warm.yml` is the one exception, by the user's call (2026-10-04). It
runs on each push to main (not docs-only ones), sets up exactly as the `ci`
job does, and compiles what that job compiles: clippy's check artifacts,
and the test binaries without running them. It gates on nothing. Both jobs
share one rust-cache `shared-key`, and only main writes either cache: PR
runs restore rust-cache with `save-if: false` and read sccache with
`SCCACHE_GHA_RW_MODE=READ_ONLY`. A PR-scoped entry would serve that PR
alone while crowding main's out of the 10 GB budget.

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
[^ci]: `.github/workflows/ci.yml`: the ci job's devenv install, the cache-service step, rust-cache's shared-key and cmd-format, and the contract step; `.github/workflows/cache-warm.yml`: the main-only warm.
