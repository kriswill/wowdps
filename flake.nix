{
  description = "wowdps - WoW combat log TUI damage meter";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-darwin" ] (
          system: f nixpkgs.legacyPackages.${system}
        );
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = builtins.attrValues {
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
