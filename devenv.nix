# The reference dev shell, locally and in CI; flake.nix's devShells.default
# mirrors it. Entered via devenv's native cd hook (trust once with `devenv
# allow`). The environment ITSELF lives in nix/dev/, imported by both; what
# stays here is only what devenv plumbs differently: its Rust toolchain and
# its okf and llm-agents inputs. Even the contract `devenv test` asserts is
# shared — it is an executable both shells carry, and it fails if the mirror
# drifts.
{
  pkgs,
  lib,
  inputs,
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

  packages = shared.packages ++ [
    # okf (scaffold | index | validate | viz) over docs/OKF, the OKF
    # knowledge bundle — the nix-built CLI from the okflight input
    # (devenv.yaml). The flake shell reaches the same CLI through its own
    # input, which is why this one package is not in nix/dev/.
    inputs.okf.packages.${pkgs.stdenv.hostPlatform.system}.okf
    # coderabbit / cr, the CodeRabbit review CLI (`coderabbit review
    # --agent`), from the llm-agents input — the flake shell reaches it
    # through its own, as it does okf.
    inputs.llm-agents.packages.${pkgs.stdenv.hostPlatform.system}.coderabbit-cli
  ];

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
