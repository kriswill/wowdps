# Devenv twin of flake.nix's devShells.default — the same environment, entered
# via devenv's native cd hook (trust once with `devenv allow`) instead of
# `nix develop`. Keep the two in sync when the toolchain changes.
{ pkgs, lib, ... }:
{
  # nixpkgs' stable rust toolchain — rustc, cargo, clippy, rustfmt,
  # rust-analyzer — the same channel the flake devShell uses.
  languages.rust.enable = true;

  # iced-layershell links libxkbcommon at build time (via
  # smithay-client-toolkit's pkg-config probe).
  packages = lib.optionals pkgs.stdenv.isLinux [
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
    for tool in cargo rustc clippy-driver rustfmt rust-analyzer; do
      command -v "$tool" > /dev/null || {
        echo "devenv contract: $tool missing from PATH" >&2
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
