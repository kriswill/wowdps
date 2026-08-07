{
  description = "wowdps - WoW combat log damage meter overlay and log-parsing daemon";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-darwin" ] (
          system: f nixpkgs.legacyPackages.${system}
        );
    in
    {
      # The daemon + TUI binary (`wowdps`): pure Rust, no GUI native deps.
      # Packaging `wowdps-gui` (wayland/vulkan runtime wrapping) is a
      # follow-up; until then the overlay supervisor finds `wowdps-gui` on
      # PATH (see nix/home-manager.nix).
      packages = forAllSystems (pkgs: rec {
        wowdps = pkgs.rustPlatform.buildRustPackage {
          pname = "wowdps";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "wowdps-tui"
          ];
          cargoTestFlags = [
            "-p"
            "wowdps-model"
            "-p"
            "wowdps-core"
            "-p"
            "wowdps-proto"
            "-p"
            "wowdps-daemon"
            "-p"
            "wowdps-tui"
          ];
          meta.mainProgram = "wowdps";
        };
        default = wowdps;
      });

      homeManagerModules = rec {
        wowdps = { pkgs, ... }: {
          imports = [ ./nix/home-manager.nix ];
          services.wowdps.package =
            nixpkgs.lib.mkDefault
              self.packages.${pkgs.stdenv.hostPlatform.system}.wowdps;
        };
        default = wowdps;
      };

      # treefmt's logger probes the terminal on startup (OSC 10/11 color +
      # cursor-position queries); the replies race its exit and leak into
      # the shell as `rgb:...` garbage. Deny it a TTY and it never asks.
      formatter = forAllSystems (
        pkgs:
        pkgs.writeShellScriptBin "treefmt-no-tty" ''
          set -o pipefail
          ${pkgs.lib.getExe pkgs.nixfmt-tree} "$@" 2>&1 | cat
        ''
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages =
            builtins.attrValues {
              inherit (pkgs)
                cargo
                rustc
                clippy
                rustfmt
                rust-analyzer
                ;
            }
            # iced-layershell links libxkbcommon at build time (via
            # smithay-client-toolkit's pkg-config probe).
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.pkg-config
              pkgs.libxkbcommon
            ];
          # The iced GUI dlopens these at runtime (winit → wayland/xkbcommon,
          # wgpu → vulkan); on NixOS they are not on the default search path.
          env = pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.wayland
              pkgs.libxkbcommon
              pkgs.vulkan-loader
              pkgs.libGL
            ];
          };
        };
      });
    };
}
