# The development environment, declared ONCE for both shells that offer it:
# `nix develop` (flake.nix's devShells.default) and devenv's cd hook
# (devenv.nix). Those two used to be hand-kept twins — every wrapper, package
# and environment variable written out twice, with a comment on each copy
# asking the next person to remember the other one. This file is that
# agreement; each shell adds only what it alone can plumb (its Rust toolchain
# and its `okf`, which reach the two through different inputs).
#
# `duckdbEnv` is exported separately because flake.nix's PACKAGE build needs
# it too, and a package is not a shell.
{
  pkgs,
  lib ? pkgs.lib,
}:
let
  # `wowdps gen-<name>` external dispatch: the repo's tools/gen-*.sh on PATH
  # as wowdps-gen-<name>. Regenerate the game-data tables once per patch.
  genScripts = [
    "class-spells"
    "keystone-timers"
    "item-spells"
    "icons"
    "spell-icons"
    "talent-trees"
  ];

  # The workspace binaries, as the dispatcher's external-command lookup
  # names them (`wowdps history` → `wowdps-history`).
  binaries = [
    "wowdps"
    "wowdps-history"
    "wowdps-mcp"
    "wowdps-gui"
  ];

  # Resolved against the live checkout at run time, never a store copy: the
  # generators cargo-build into the repo and write per-machine caches.
  genWrapper =
    name:
    pkgs.writeShellScriptBin "wowdps-gen-${name}" ''
      exec "$(git rev-parse --show-toplevel)/tools/gen-${name}.sh" "$@"
    '';

  # The workspace's own binaries, built from the LIVE checkout on every call.
  # A dev shell that hands you a binary older than your last commit is worse
  # than no binary at all — the daemon silently kept running a pre-R22 meter
  # for exactly that reason — so each wrapper builds its own `--bin` first (a
  # no-op build is ~0.12 s) and only then runs it. `WOWDPS_NO_BUILD=1` skips
  # the build and runs whatever is already there.
  #
  # `exec` is load-bearing, not style: every binary here resolves its siblings
  # from `current_exe` first and PATH only as a fallback (`tui/src/main.rs`
  # find_bin, `gui/src/main.rs` daemon_bin, `daemon/src/lib.rs` gui_bin,
  # `mcp/src/bridge.rs`). Exec'ing replaces the wrapper, so `current_exe` is
  # `target/release/wowdps` and `wowdps history` finds its sibling; a spawn
  # would leave `current_exe` in /nix/store, where no sibling exists, and send
  # the daemon hunting on PATH with its stderr already nulled. Cross-binary
  # calls that miss still land on these wrappers through PATH, which build
  # lazily — one binary at a time, never the GUI's tree for a `wowdps status`.
  binWrapper =
    name:
    pkgs.writeShellScriptBin name ''
      set -eu
      root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
      # The shell stays loaded after you cd away; without this a
      # `wowdps status` from another checkout would build THAT project.
      if [ ! -e "$root/crates/core/fixtures/check.awk" ]; then
        echo "wowdps: run this from inside the wowdps checkout" >&2
        exit 127
      fi
      if [ -z "''${WOWDPS_NO_BUILD:-}" ]; then
        # stderr, never stdout: `wowdps mcp` speaks JSON-RPC on stdout and
        # `wowdps history sql --json` is piped into jq.
        cargo build --release --quiet --manifest-path "$root/Cargo.toml" \
          --bin ${name} >&2
      fi
      exec "$root/target/release/${name}" "$@"
    '';
in
rec {
  # Every command name the shells promise, so devenv's `enterTest` contract
  # is generated from the same list that creates the wrappers instead of
  # being a hand-copied third place to forget one.
  commands = map (name: "wowdps-gen-${name}") genScripts ++ binaries;

  # The history store's analytical reader (`wowdps-history`, reached as
  # `wowdps history`) links libduckdb from nixpkgs — SYSTEM-linked, never the
  # crate's `bundled` build (a ~15 minute C++ compile on CI). nixpkgs ships
  # no .pc for it, so the sys crate is pointed at the lib / dev outputs by
  # these two variables, in both shells and in the package build alike; the
  # crate version is pinned to the library's (crates/history/Cargo.toml).
  duckdbEnv = {
    DUCKDB_LIB_DIR = "${lib.getLib pkgs.duckdb}/lib";
    DUCKDB_INCLUDE_DIR = "${lib.getDev pkgs.duckdb}/include";
  };

  # Everything both shells install EXCEPT the Rust toolchain and okf, which
  # each reaches through its own input plumbing.
  packages =
    map genWrapper genScripts
    ++ map binWrapper binaries
    ++ [
      # Coverage: `cargo llvm-cov --workspace`; the llvm-cov / llvm-profdata
      # it drives come from the toolchain's own llvm-tools component
      # (sysroot), not from here.
      pkgs.cargo-llvm-cov
      # `cargo audit` checks Cargo.lock against the RustSec advisory database.
      pkgs.cargo-audit
      # gawk drives the parser-independent fixture check
      # (crates/core/fixtures/verify.sh), locally and in CI — the CI check
      # job runs inside this shell.
      pkgs.gawk
      # libduckdb for `wowdps-history` (see duckdbEnv).
      pkgs.duckdb
    ]
    # iced-layershell links libxkbcommon at build time (via
    # smithay-client-toolkit's pkg-config probe).
    ++ lib.optionals pkgs.stdenv.isLinux [
      pkgs.pkg-config
      pkgs.libxkbcommon
    ];

  # The iced GUI dlopens these at runtime (winit → wayland/xkbcommon, wgpu →
  # vulkan); on NixOS they are not on the default search path. libduckdb
  # likewise, for `cargo test -p wowdps-history`.
  env = duckdbEnv // {
    LD_LIBRARY_PATH = lib.makeLibraryPath (
      [ (lib.getLib pkgs.duckdb) ]
      ++ lib.optionals pkgs.stdenv.isLinux [
        pkgs.wayland
        pkgs.libxkbcommon
        pkgs.vulkan-loader
        pkgs.libGL
      ]
    );
  };
}
