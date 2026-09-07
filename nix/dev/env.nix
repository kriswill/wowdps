# The environment variables a dev shell must export, and the native libraries
# behind them. Split out because `duckdbEnv` is not only a shell concern:
# flake.nix's PACKAGE build needs the same two variables, and a package is not
# a shell.
{
  pkgs,
  lib ? pkgs.lib,
}:
rec {
  # The history store's analytical reader (`wowdps-history`, reached as
  # `wowdps history`) links libduckdb from nixpkgs — SYSTEM-linked, never the
  # crate's `bundled` build (a ~15 minute C++ compile on CI). nixpkgs ships no
  # .pc for it, so the sys crate is pointed at the lib / dev outputs by these
  # two variables; the crate version is pinned to the library's
  # (crates/history/Cargo.toml).
  duckdbEnv = {
    DUCKDB_LIB_DIR = "${lib.getLib pkgs.duckdb}/lib";
    DUCKDB_INCLUDE_DIR = "${lib.getDev pkgs.duckdb}/include";
  };

  # Dlopened at runtime, so nothing links them and nothing on the build side
  # would notice they are missing: the iced GUI reaches for wayland/xkbcommon
  # (winit) and vulkan/GL (wgpu), and `cargo test -p wowdps-history` reaches
  # for libduckdb. On NixOS none of them are on the default search path.
  libraries = [
    (lib.getLib pkgs.duckdb)
  ]
  ++ lib.optionals pkgs.stdenv.isLinux [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.vulkan-loader
    pkgs.libGL
  ];

  env = duckdbEnv // {
    LD_LIBRARY_PATH = lib.makeLibraryPath libraries;
  };
}
