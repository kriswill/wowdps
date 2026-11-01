# The reference dev shell, locally and in CI; flake.nix's devShells.default
# mirrors it. Entered via devenv's native cd hook (trust once with `devenv
# allow`). The environment ITSELF lives in nix/dev/, imported by both; what
# stays here is only what devenv plumbs differently: its Rust toolchain.
# Even the contract `devenv test` asserts is shared — it is an executable
# both shells carry, and it fails if the mirror drifts.
{
  pkgs,
  lib,
  ...
}:
let
  shared = import ./nix/dev { inherit pkgs lib; };
in
{
  # The toolchain comes from rust-toolchain.toml (nightly + components)
  # through rust-overlay — the same file and overlay the flake devShell
  # uses, so the two can never drift. rust-overlay is a devenv.yaml input
  # pinned to flake.lock's rev.
  languages.rust.enable = true;
  languages.rust.toolchainFile = ./rust-toolchain.toml;
  # devenv is the reference shell, locally and in CI: its languages.rust
  # links through Clang driving ld.lld (`clangLinker`, on by default on Linux)
  # and puts clang on PATH, whose setup hook sets CC=clang and CXX=clang++.
  # flake.nix mirrors both (nix/dev/env.nix, `rustLinker`), and the contract
  # fails if the mirror ever stops being exact.

  packages = shared.packages;

  env = shared.env;

  # Last (mkAfter), after every enterShell the languages.* modules add: the
  # pinned PKG_CONFIG_PATH (nix/dev/env.nix) — flake.nix runs the same hook
  # as mkShell's shellHook.
  enterShell = lib.mkAfter shared.shellHook;

  # The contract `devenv test` asserts. It lives in nix/dev/contract.nix as an
  # executable both shells carry (`nix develop -c wowdps-dev-contract` is the
  # flake half), because it checks what that file promises — a check sitting
  # in only one of the twins could only ever vouch for that one.
  enterTest = "wowdps-dev-contract";
}
