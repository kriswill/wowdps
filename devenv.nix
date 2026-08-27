# Devenv twin of flake.nix's devShells.default — the same environment, entered
# via devenv's native cd hook (trust once with `devenv allow`) instead of
# `nix develop`. Keep the two in sync when the toolchain changes.
{ pkgs, lib, ... }:
{
  # nixpkgs' stable rust toolchain — rustc, cargo, clippy, rustfmt,
  # rust-analyzer — the same channel the flake devShell uses.
  languages.rust.enable = true;

  # Coverage: `cargo llvm-cov --workspace`. nixpkgs' rustc ships no
  # llvm-tools-preview component, so point cargo-llvm-cov at nixpkgs' LLVM.
  packages = [
    pkgs.cargo-llvm-cov
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
  env = {
    LLVM_COV = "${pkgs.llvm}/bin/llvm-cov";
    LLVM_PROFDATA = "${pkgs.llvm}/bin/llvm-profdata";
  }
  // lib.optionalAttrs pkgs.stdenv.isLinux {
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
    for tool in cargo rustc clippy-driver rustfmt rust-analyzer cargo-llvm-cov \
                wowdps-gen-class-spells wowdps-gen-keystone-timers \
                wowdps-gen-item-spells wowdps-gen-icons wowdps-gen-spell-icons \
                wowdps-gen-talent-trees; do
      command -v "$tool" > /dev/null || {
        echo "devenv contract: $tool missing from PATH" >&2
        exit 1
      }
    done
    for var in LLVM_COV LLVM_PROFDATA; do
      [ -x "''${!var}" ] || {
        echo "devenv contract: \$$var does not point at an executable" >&2
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
