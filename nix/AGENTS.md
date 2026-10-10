# nix (with flake.nix, devenv.nix, devenv.yaml)

The dev environment both shells import (`nix/dev/`), the flake's packages
and the home-manager and NixOS modules. Why and how:
`docs/OKF/decisions/devenv-is-the-reference-shell.md`.

## Rules

- **devenv is the reference shell,** locally and in CI; `nix develop`
  mirrors it, and both import `nix/dev/`. They must hand cargo identical
  inputs (the linker, `CC`, `PKG_CONFIG_PATH`, every variable a build script
  watches), or every switch between them rebuilds the whole tree.
  `PKG_CONFIG_PATH` is pinned and exported last by both.
- **The contract proves it.** An environment change passes `devenv test`
  and `nix develop -c wowdps-dev-contract`; a value both shells must share
  gets a check in `contract.nix`.
- **One concern per file:** `nix/dev/default.nix` assembles `wrappers.nix`
  (generator and workspace-binary wrappers, resolved against the live
  checkout), `env.nix` (DuckDB, the dlopened libraries, the pinned
  `PKG_CONFIG_PATH`, the linker and clang mirror, sccache) and
  `contract.nix`. Each shell file adds only what it alone plumbs.
- **Pins agree.** `devenv.yaml`'s nixpkgs, rust-overlay, okf and llm-agents
  pins match `flake.lock` (`update-locks.yml` keeps them so weekly).
- **The toolchain is declared once,** in `rust-toolchain.toml`; never pin a
  Rust version anywhere else.
- **The two modules move together.** `nix/home-manager.nix` and
  `nix/nixos.nix` install the same user unit (`wowdps daemon --linger`,
  `guiPackage` on its PATH).
- **Packages build from a filtered source** (crane, a dependency layer
  keyed on `Cargo.lock`). Add a path to the `lib.fileset` only when a build
  or a test reads it, so a docs edit outside it rebuilds nothing.
- **Old shells keep old values.** Restart a shell entered before an
  environment change.
- **Caches:** sccache never hits across worktrees. After a toolchain
  change, `cargo clean` before `cargo llvm-cov --workspace`.

```sh
devenv test                          # the contract, in devenv
nix develop -c wowdps-dev-contract   # the contract, in the flake shell
nix build .#wowdps                   # the daemon/TUI package
nix build .#wowdps-gui               # the GUI package (checks run on lavapipe)
```
