# Devenv twin of flake.nix's devShells.default — the same environment, entered
# via devenv's native cd hook (trust once with `devenv allow`) instead of
# `nix develop`. Keep the two in sync when the toolchain changes.
{
  pkgs,
  lib,
  inputs,
  ...
}:
{
  # The toolchain comes from rust-toolchain.toml (nightly + components)
  # through rust-overlay — the same file and overlay the flake devShell
  # uses, so the two can never drift. rust-overlay is a devenv.yaml input
  # pinned to flake.lock's rev.
  languages.rust.enable = true;
  languages.rust.toolchainFile = ./rust-toolchain.toml;

  # Coverage: `cargo llvm-cov --workspace`; the llvm-cov / llvm-profdata it
  # drives come from the toolchain's own llvm-tools component (sysroot).
  packages = [
    pkgs.cargo-llvm-cov
    # `cargo audit` checks Cargo.lock against the RustSec advisory database.
    pkgs.cargo-audit
    # gawk drives the parser-independent fixture check
    # (crates/core/fixtures/verify.sh), like the flake shell.
    pkgs.gawk
    # okf (scaffold | index | validate | viz) over docs/OKF, the OKF
    # knowledge bundle — the nix-built CLI from the okflight input
    # (devenv.yaml), twin of the flake shell's.
    inputs.okf.packages.${pkgs.stdenv.hostPlatform.system}.okf
  ]
  # `wowdps gen-<name>` external dispatch: thin wrappers putting the repo's
  # tools/gen-*.sh on PATH as wowdps-gen-<name>, resolved against the live
  # checkout at run time (the scripts cargo-build into the repo), never a
  # store copy. Twin list in flake.nix's devShell.
  ++
    map
      (
        name:
        pkgs.writeShellScriptBin "wowdps-gen-${name}" ''
          exec "$(git rev-parse --show-toplevel)/tools/gen-${name}.sh" "$@"
        ''
      )
      [
        "class-spells"
        "keystone-timers"
        "item-spells"
        "icons"
        "spell-icons"
        "talent-trees"
      ]
  # The workspace's own binaries on PATH, built from the LIVE checkout on
  # every call. A dev shell that hands you a binary older than your last
  # commit is worse than no binary at all — the daemon silently kept running
  # a pre-R22 meter for exactly that reason — so each wrapper builds its own
  # `--bin` first (a no-op build is ~0.2 s) and only then runs it.
  # `WOWDPS_NO_BUILD=1` skips the build and runs whatever is already there.
  #
  # `exec` is load-bearing, not style: every binary here resolves its
  # siblings from `current_exe` first and PATH only as a fallback
  # (`tui/src/main.rs` find_bin, `gui/src/main.rs` daemon_bin,
  # `daemon/src/lib.rs` gui_bin, `mcp/src/bridge.rs`). Exec'ing replaces the
  # wrapper, so `current_exe` is `target/release/wowdps` and `wowdps history`
  # finds its sibling; a spawn would leave `current_exe` in /nix/store, where
  # no sibling exists, and send the daemon hunting on PATH with its stderr
  # already nulled. Cross-binary calls that miss still land on these wrappers
  # through PATH, which build lazily — one binary at a time, never the GUI's
  # tree for a `wowdps status`. Twin list in flake.nix's devShell.
  ++
    map
      (
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
            # stderr, never stdout: `wowdps mcp` speaks JSON-RPC on stdout
            # and `wowdps history sql --json` is piped into jq.
            cargo build --release --quiet --manifest-path "$root/Cargo.toml" \
              --bin ${name} >&2
          fi
          exec "$root/target/release/${name}" "$@"
        ''
      )
      [
        "wowdps"
        "wowdps-history"
        "wowdps-mcp"
        "wowdps-gui"
      ]
  # iced-layershell links libxkbcommon at build time (via
  # smithay-client-toolkit's pkg-config probe).
  ++ lib.optionals pkgs.stdenv.isLinux [
    pkgs.pkg-config
    pkgs.libxkbcommon
  ]
  # libduckdb for `wowdps-history` (system-linked, never `bundled`).
  ++ [ pkgs.duckdb ];

  # The iced GUI dlopens these at runtime (winit → wayland/xkbcommon,
  # wgpu → vulkan); on NixOS they are not on the default search path.
  # libduckdb likewise, for `cargo test -p wowdps-history`; the two
  # DUCKDB_* variables point the sys crate at nixpkgs' lib / dev outputs
  # (no .pc is shipped) — the flake's `duckdbEnv`, twinned.
  env = {
    LD_LIBRARY_PATH = lib.makeLibraryPath (
      [ (lib.getLib pkgs.duckdb) ]
      ++ lib.optionals pkgs.stdenv.isLinux [
        pkgs.wayland
        pkgs.libxkbcommon
        pkgs.vulkan-loader
        pkgs.libGL
      ]
    );
    DUCKDB_LIB_DIR = "${lib.getLib pkgs.duckdb}/lib";
    DUCKDB_INCLUDE_DIR = "${lib.getDev pkgs.duckdb}/include";
  };

  # The contract `devenv test` asserts — the environment must provide what the
  # flake devShell promises, not merely "evaluation didn't crash".
  enterTest = ''
    set -euo pipefail
    for tool in cargo rustc clippy-driver rustfmt rust-analyzer cargo-llvm-cov gawk okf \
                wowdps-gen-class-spells wowdps-gen-keystone-timers \
                wowdps-gen-item-spells wowdps-gen-icons wowdps-gen-spell-icons \
                wowdps-gen-talent-trees \
                wowdps wowdps-history wowdps-mcp wowdps-gui; do
      command -v "$tool" > /dev/null || {
        echo "devenv contract: $tool missing from PATH" >&2
        exit 1
      }
    done
    case "$(rustc --version)" in
      *nightly*) ;;
      *) echo "devenv contract: rustc is not the nightly rust-toolchain.toml names" >&2; exit 1 ;;
    esac
    # cargo-llvm-cov's llvm-cov / llvm-profdata: the toolchain's own
    # llvm-tools component, under the sysroot.
    for tool in llvm-cov llvm-profdata; do
      ls "$(rustc --print sysroot)"/lib/rustlib/*/bin/"$tool" > /dev/null 2>&1 || {
        echo "devenv contract: $tool missing from the toolchain sysroot" >&2
        exit 1
      }
    done
  ''
  + lib.optionalString pkgs.stdenv.isLinux ''
    pkg-config --exists xkbcommon || {
      echo "devenv contract: libxkbcommon not visible to pkg-config" >&2
      exit 1
    }
    [ -e "$DUCKDB_LIB_DIR/libduckdb.so" ] && [ -e "$DUCKDB_INCLUDE_DIR/duckdb.h" ] || {
      echo "devenv contract: DUCKDB_LIB_DIR / DUCKDB_INCLUDE_DIR do not point at libduckdb" >&2
      exit 1
    }
    for lib in libwayland-client.so libxkbcommon.so libvulkan.so libGL.so libduckdb.so; do
      found=0
      IFS=: read -ra dirs <<< "$LD_LIBRARY_PATH"
      for dir in "''${dirs[@]}"; do
        [ -e "$dir/$lib" ] && found=1 && break
      done
      [ "$found" = 1 ] || {
        echo "devenv contract: $lib not on LD_LIBRARY_PATH" >&2
        exit 1
      }
    done
  '';
}
