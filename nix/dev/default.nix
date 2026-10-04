# The development environment, declared ONCE for both shells that offer it:
# `nix develop` (flake.nix's devShells.default) and devenv's cd hook
# (devenv.nix). Those two used to be hand-kept twins — every wrapper, package
# and environment variable written out twice, with a comment on each copy
# asking the next person to remember the other one. This directory is that
# agreement, and each shell adds only what it alone can plumb: its Rust
# toolchain, and its `okf` (the two reach it through different inputs).
# devenv is the reference shell, locally and in CI; the flake shell also
# mirrors what devenv's languages.rust adds on its own (env.nix).
#
#   wrappers.nix  the commands about this repo — generators, workspace binaries
#   env.nix       DUCKDB_*, the dlopened libraries behind LD_LIBRARY_PATH, the
#                 pinned PKG_CONFIG_PATH, the flake's mirror of devenv's clang
#                 and linker, sccache
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
    inherit (env)
      pinned
      rustcWrapper
      rustLinker
      rustLinkerVar
      ;
  };
in
{
  # Every command name the shells promise, the contract checker included.
  commands = wrappers.names ++ [ "wowdps-dev-contract" ];

  # Re-exported for flake.nix's package build, which needs the two DUCKDB_*
  # variables without wanting a shell.
  inherit (env) duckdbEnv;
  inherit (env) env;

  # Run LAST by devenv (`enterShell`): it exports the pinned PKG_CONFIG_PATH
  # over what the shell's own setup wrote (env.nix says why).
  inherit (env) shellHook;

  # What the flake shell adds to mirror devenv, the reference shell: the
  # clang devenv's languages.rust puts on PATH (CC=clang, CXX=clang++), and
  # a shellHook that also exports devenv's linker script.
  inherit (env) flakeShellHook flakePackages;

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
      # The compiler cache RUSTC_WRAPPER names, on PATH for its
      # `--show-stats` (see env.nix).
      pkgs.sccache
    ]
    # The GUI's GPUI links libxkbcommon at build time, with libxcb for its
    # X11 backend, and fontconfig (font-kit's yeslogic-fontconfig-sys
    # probe).
    ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
      pkgs.pkg-config
      pkgs.libxkbcommon
      pkgs.fontconfig
      pkgs.libxcb
    ];
}
