# Devenv twin of flake.nix's devShells.default — the same environment, entered
# via devenv's native cd hook (trust once with `devenv allow`) instead of
# `nix develop`. The environment ITSELF lives in nix/dev-shell.nix, imported by
# both, so the two can no longer drift; what stays here is only what devenv
# plumbs differently: its Rust toolchain and its okf input. Even the contract
# `devenv test` asserts is shared — it is an executable both shells carry.
{
  pkgs,
  lib,
  inputs,
  ...
}:
let
  shared = import ./nix/dev-shell.nix { inherit pkgs lib; };
in
{
  # The toolchain comes from rust-toolchain.toml (nightly + components)
  # through rust-overlay — the same file and overlay the flake devShell
  # uses, so the two can never drift. rust-overlay is a devenv.yaml input
  # pinned to flake.lock's rev.
  languages.rust.enable = true;
  languages.rust.toolchainFile = ./rust-toolchain.toml;

  packages = shared.packages ++ [
    # okf (scaffold | index | validate | viz) over docs/OKF, the OKF
    # knowledge bundle — the nix-built CLI from the okflight input
    # (devenv.yaml). The flake shell reaches the same CLI through its own
    # input, which is why this one package is not in nix/dev-shell.nix.
    inputs.okf.packages.${pkgs.stdenv.hostPlatform.system}.okf
  ];

  env = shared.env;

  # The contract `devenv test` asserts. It lives in nix/dev-shell.nix as an
  # executable both shells carry (`nix develop -c wowdps-dev-contract` is the
  # flake half), because it checks what that file promises — a check sitting
  # in only one of the twins could only ever vouch for that one.
  enterTest = "wowdps-dev-contract";
}
