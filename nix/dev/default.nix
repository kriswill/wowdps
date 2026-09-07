# The development environment, declared ONCE for both shells that offer it:
# `nix develop` (flake.nix's devShells.default) and devenv's cd hook
# (devenv.nix). Those two used to be hand-kept twins — every wrapper, package
# and environment variable written out twice, with a comment on each copy
# asking the next person to remember the other one. This directory is that
# agreement, and each shell adds only what it alone can plumb: its Rust
# toolchain, and its `okf` (the two reach it through different inputs).
#
#   wrappers.nix  the commands about this repo — generators, workspace binaries
#   env.nix       DUCKDB_* and the dlopened libraries behind LD_LIBRARY_PATH
#   contract.nix  what a shell must deliver, as a runnable check
{
  pkgs,
  lib ? pkgs.lib,
}:
let
  wrappers = import ./wrappers.nix { inherit pkgs; };
  env = import ./env.nix { inherit pkgs lib; };
  contract = import ./contract.nix {
    inherit pkgs lib;
    commands = wrappers.names;
  };
in
{
  # Every command name the shells promise, the contract checker included.
  commands = wrappers.names ++ [ "wowdps-dev-contract" ];

  # Re-exported for flake.nix's package build, which needs the two DUCKDB_*
  # variables without wanting a shell.
  inherit (env) duckdbEnv;
  inherit (env) env;

  # Everything both shells install EXCEPT the Rust toolchain and okf.
  packages =
    wrappers.packages
    ++ [
      contract
      # Coverage: `cargo llvm-cov --workspace`; the llvm-cov / llvm-profdata
      # it drives come from the toolchain's own llvm-tools component
      # (sysroot), not from here.
      pkgs.cargo-llvm-cov
      # `cargo audit` checks Cargo.lock against the RustSec advisory database.
      pkgs.cargo-audit
      # gawk drives the parser-independent fixture check
      # (crates/core/fixtures/verify.sh), locally and in CI — the CI check job
      # runs inside this shell.
      pkgs.gawk
      # libduckdb for `wowdps-history` (see env.nix).
      pkgs.duckdb
    ]
    # iced-layershell links libxkbcommon at build time (via
    # smithay-client-toolkit's pkg-config probe).
    ++ lib.optionals pkgs.stdenv.isLinux [
      pkgs.pkg-config
      pkgs.libxkbcommon
    ];
}
