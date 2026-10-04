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
  # order. So it is derived here from the libraries the GUI's build scripts
  # probe and everything they propagate (fontconfig brings freetype and
  # expat, freetype brings zlib, bzip2, brotli and libpng), so a nixpkgs
  # bump that changes a propagation moves the pin with it. Both directories
  # a .pc file may live in are listed for each (zlib's is share/); pkg-config
  # skips the ones that do not exist. Exported LAST by both shells
  # (`shellHook`, after their own setup).
  pkgConfigRoots = lib.optionals pkgs.stdenv.hostPlatform.isLinux [
    pkgs.libxkbcommon
    pkgs.fontconfig
    pkgs.libxcb
  ];
  pkgConfigClosure =
    let
      node = drv: {
        key = (lib.getDev drv).outPath;
        dev = lib.getDev drv;
      };
    in
    map (n: n.dev) (
      builtins.genericClosure {
        startSet = map node pkgConfigRoots;
        operator = n: map node (n.dev.propagatedBuildInputs or [ ]);
      }
    );
  pinned = {
    PKG_CONFIG_PATH = lib.concatMapStringsSep ":" (
      dev: "${dev}/lib/pkgconfig:${dev}/share/pkgconfig"
    ) pkgConfigClosure;
  };

  # devenv's languages.rust links through Clang on Linux (`clangLinker`,
  # its default: it sidesteps GCC collect2's "Argument list too long" in
  # large Nix environments), and the clang it puts on PATH sets CC=clang and
  # CXX=clang++. On x86_64-unknown-linux-gnu, where rustc links with LLD out
  # of the box and stops adding its LLD flags once a driver is named, it
  # names its `devenv-rust-linker` script, which forces ld.lld; on any other
  # target, clang itself. The flake shell mirrors all of it: it adds the
  # same clang (flake.nix) and exports the same linker, the script written
  # out again with the same name and text so it builds to the same store
  # path. If devenv ever changes its choice, the paths part and the contract
  # says so.
  rustLinker =
    if pkgs.stdenv.hostPlatform.rust.rustcTarget == "x86_64-unknown-linux-gnu" then
      pkgs.writeShellScript "devenv-rust-linker" ''
        exec ${lib.getExe pkgs.clang} --ld-path=${pkgs.llvmPackages.bintools}/bin/ld.lld "$@"
      ''
    else
      lib.getExe pkgs.clang;
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
  # (the out dir, `-L dependency=`, the linker, the sysroot), so it hits
  # only at the SAME paths with the SAME toolchain: in CI, whose runner
  # builds at one path every run (a dependency a Cargo.lock change leaves
  # alone is a hit; a nixpkgs or rust-overlay bump moves the linker and the
  # sysroot and misses everything), and in a checkout that cargo rebuilds
  # without rustc's inputs changing (a `cargo clean`, a branch switched away
  # and back). A second worktree misses — SCCACHE_BASEDIRS did not normalise
  # Rust compiles (tested 2026-10-04).
  rustcWrapper = "${pkgs.sccache}/bin/sccache";

  env = duckdbEnv // {
    LD_LIBRARY_PATH = lib.makeLibraryPath libraries;
    RUSTC_WRAPPER = rustcWrapper;
  };
}
