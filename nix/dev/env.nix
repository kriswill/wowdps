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
  # would notice they are missing: the GUI's GPUI reaches for wayland-client
  # and vulkan/EGL (wgpu), libxkbcommon rides along for the binary's baked
  # RUNPATH (crates/gui/build.rs), and `cargo test -p wowdps-history`
  # reaches for libduckdb. On NixOS none of them are on the default search
  # path.
  libraries = [
    (lib.getLib pkgs.duckdb)
  ]
  ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.vulkan-loader
    pkgs.libGL
  ];

  # The twins must hand cargo the same strings. Cargo decides a crate is
  # fresh by fingerprinting its INPUTS, keeps ONE fingerprint per crate in
  # target/, and counts as inputs both the linker (it rides `-C linker` on
  # EVERY crate's rustc) and each variable a build script names in
  # `rerun-if-env-changed` (the fontconfig, freetype, xkbcommon and cc-built
  # crates under GPUI watch CC and PKG_CONFIG_PATH). When the shells
  # disagreed, every switch between devenv and `nix develop` rebuilt the
  # whole tree — 567 crates, about 65 s each way (measured 2026-10-04),
  # however identical the output. devenv is the reference, locally and in
  # CI; the flake shell mirrors it, and the contract checks the mirror.

  # PKG_CONFIG_PATH is the one value neither shell can be left to compute:
  # each pkg-config hook lists what its own packages bring, and devenv's
  # brings two it does not need (valgrind, bash-interactive) in another
  # order. Pinned here, in the order mkShell's hook listed them, and
  # exported LAST by both shells (`shellHook`, after their own setup).
  pinned = {
    PKG_CONFIG_PATH = lib.concatStringsSep ":" (
      lib.optionals pkgs.stdenv.hostPlatform.isLinux [
        "${lib.getDev pkgs.libxkbcommon}/lib/pkgconfig"
        "${lib.getDev pkgs.fontconfig}/lib/pkgconfig"
        "${lib.getDev pkgs.freetype}/lib/pkgconfig"
        "${lib.getDev pkgs.zlib}/share/pkgconfig"
        "${lib.getDev pkgs.bzip2}/lib/pkgconfig"
        "${lib.getDev pkgs.brotli}/lib/pkgconfig"
        "${lib.getDev pkgs.libpng}/lib/pkgconfig"
        "${lib.getDev pkgs.expat}/lib/pkgconfig"
        "${lib.getDev pkgs.libxcb}/lib/pkgconfig"
      ]
    );
  };

  # devenv's languages.rust links through Clang driving ld.lld on Linux
  # (`clangLinker`, its default: it sidesteps GCC collect2's "Argument list
  # too long" in large Nix environments, and forces LLD because naming a
  # driver stops rustc adding its default LLD flags), and the clang it puts
  # on PATH sets CC=clang and CXX=clang++. The flake shell mirrors both: it
  # adds the same clang (flake.nix) and exports this script, devenv's own
  # `devenv-rust-linker` written out again with the same name and text, so
  # it builds to the same store path. If devenv ever changes its script, the
  # paths part and the contract says so.
  rustLinker = pkgs.writeShellScript "devenv-rust-linker" ''
    exec ${lib.getExe pkgs.clang} --ld-path=${pkgs.llvmPackages.bintools}/bin/ld.lld "$@"
  '';
  rustLinkerVar = "CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_LINKER";

  # Both shells run this last (devenv's `enterShell`, mkShell's `shellHook`).
  shellHook = lib.optionalString (pinned.PKG_CONFIG_PATH != "") ''
    export PKG_CONFIG_PATH=${lib.escapeShellArg pinned.PKG_CONFIG_PATH}
  '';

  # What only the flake shell adds, to mirror devenv's languages.rust.
  flakeShellHook =
    shellHook
    + lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
      export ${rustLinkerVar}=${rustLinker}
    '';
  flakePackages = lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.clang ];

  # sccache in front of rustc. Where cargo's fingerprint says "rebuild",
  # sccache hashes the actual compiler invocation and inputs and hands back
  # the artifact it already has — the output-keyed cache cargo itself does
  # not keep. It cannot cache build scripts, proc-macros or binaries, so it
  # softens a rebuild rather than removing it. Its keys hold absolute paths
  # (the out dir, `-L dependency=`), so it hits only at the SAME paths:
  # in CI, whose runner builds at one path every run and whose GitHub
  # Actions backend outlives a target/ cache miss, and in a checkout that
  # cargo rebuilds without rustc's inputs changing (a `cargo clean`, a
  # branch switched away and back). A second worktree misses —
  # SCCACHE_BASEDIRS did not normalise Rust compiles (tested 2026-10-04).
  rustcWrapper = "${pkgs.sccache}/bin/sccache";

  env = duckdbEnv // {
    LD_LIBRARY_PATH = lib.makeLibraryPath libraries;
    RUSTC_WRAPPER = rustcWrapper;
  };
}
