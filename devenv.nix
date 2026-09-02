# Devenv twin of flake.nix's devShells.default — the same environment, entered
# via devenv's native cd hook (trust once with `devenv allow`) instead of
# `nix develop`. Keep the two in sync when the toolchain changes.
{ pkgs, lib, ... }:
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
    # gawk drives the parser-independent fixture check
    # (crates/core/fixtures/verify.sh), like the flake shell.
    pkgs.gawk
  ]
  # `wowdps gen-<name>` external dispatch: thin wrappers putting the repo's
  # tools/gen-*.sh on PATH as wowdps-gen-<name>, resolved against the live
  # checkout at run time (the scripts cargo-build into the repo), never a
  # store copy. Twin list in flake.nix's devShell.
  ++ map (
    name:
    pkgs.writeShellScriptBin "wowdps-gen-${name}" ''
      exec "$(git rev-parse --show-toplevel)/tools/gen-${name}.sh" "$@"
    ''
  ) [ "class-spells" "keystone-timers" "item-spells" "icons" "spell-icons" "talent-trees" ]
  # iced-layershell links libxkbcommon at build time (via
  # smithay-client-toolkit's pkg-config probe).
  ++ lib.optionals pkgs.stdenv.isLinux [
    pkgs.pkg-config
    pkgs.libxkbcommon
  ];

  # The iced GUI dlopens these at runtime (winit → wayland/xkbcommon,
  # wgpu → vulkan); on NixOS they are not on the default search path.
  env = lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath [
      pkgs.wayland
      pkgs.libxkbcommon
      pkgs.vulkan-loader
      pkgs.libGL
    ];
  };

  # The contract `devenv test` asserts — the environment must provide what the
  # flake devShell promises, not merely "evaluation didn't crash".
  enterTest = ''
    set -euo pipefail
    for tool in cargo rustc clippy-driver rustfmt rust-analyzer cargo-llvm-cov gawk \
                wowdps-gen-class-spells wowdps-gen-keystone-timers \
                wowdps-gen-item-spells wowdps-gen-icons wowdps-gen-spell-icons \
                wowdps-gen-talent-trees; do
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
    for lib in libwayland-client.so libxkbcommon.so libvulkan.so libGL.so; do
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
