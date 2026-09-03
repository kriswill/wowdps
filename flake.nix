{
  description = "wowdps - WoW combat log damage meter overlay and log-parsing daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # The Rust toolchain: rust-toolchain.toml names the channel (nightly)
    # and components; the overlay's locked manifest set fixes the exact
    # nightly date, so flake.lock pins it. devenv.yaml mirrors this input.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-darwin" ] (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
      # One toolchain for the dev shell AND the package build, straight from
      # rust-toolchain.toml — the single declaration (devenv.nix reads the
      # same file), so the two can never drift.
      toolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      # The history store's analytical reader (`wowdps-history`, reached as
      # `wowdps history`) links libduckdb from nixpkgs — SYSTEM-linked, never
      # the crate's `bundled` build (a ~15 minute C++ compile on CI). nixpkgs
      # ships no .pc for it, so the sys crate is pointed at the lib / dev
      # outputs by these two variables, in the dev shell (devenv.nix twin)
      # and in the package build alike; the crate version is pinned to the
      # library's (crates/history/Cargo.toml).
      duckdbEnv = pkgs: {
        DUCKDB_LIB_DIR = "${pkgs.lib.getLib pkgs.duckdb}/lib";
        DUCKDB_INCLUDE_DIR = "${pkgs.lib.getDev pkgs.duckdb}/include";
      };
    in
    {
      # The daemon + TUI binary (`wowdps`) plus its siblings — the MCP server
      # (`wowdps-mcp`, reached as `wowdps mcp`) and the history reader
      # (`wowdps-history`, `wowdps history`): pure Rust except libduckdb,
      # which the history binary finds through its rpath. Packaging
      # `wowdps-gui` (wayland/vulkan runtime wrapping) is a follow-up; until
      # then the overlay supervisor finds `wowdps-gui` on PATH (see
      # nix/home-manager.nix).
      packages = forAllSystems (
        pkgs:
        let
          toolchain = toolchainFor pkgs;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        rec {
          wowdps = rustPlatform.buildRustPackage (
            {
              pname = "wowdps";
              version = "0.1.0";
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = [
                "-p"
                "wowdps-tui"
                "-p"
                "wowdps-mcp"
                "-p"
                "wowdps-history"
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
                "-p"
                "wowdps-mcp"
                "-p"
                "wowdps-history"
              ];
              nativeBuildInputs = [ pkgs.autoPatchelfHook ];
              buildInputs = [ (pkgs.lib.getLib pkgs.duckdb) ];
              # The one native library in the closure, on the one binary
              # that needs it; the tests need it on the load path too.
              LD_LIBRARY_PATH = "${pkgs.lib.getLib pkgs.duckdb}/lib";
              meta.mainProgram = "wowdps";
            }
            // duckdbEnv pkgs
          );
          default = wowdps;
        }
      );

      homeManagerModules = rec {
        wowdps = { pkgs, ... }: {
          imports = [ ./nix/home-manager.nix ];
          services.wowdps.package =
            nixpkgs.lib.mkDefault
              self.packages.${pkgs.stdenv.hostPlatform.system}.wowdps;
        };
        default = wowdps;
      };

      # Same user unit for NixOS configs that skip home-manager.
      nixosModules = rec {
        wowdps = { pkgs, ... }: {
          imports = [ ./nix/nixos.nix ];
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
            # `wowdps gen-<name>` external dispatch: thin wrappers putting the
            # repo's tools/gen-*.sh on PATH as wowdps-gen-<name>, resolved
            # against the live checkout at run time (the scripts cargo-build
            # into the repo), never a store copy. Twin list in devenv.nix.
            map (
              name:
              pkgs.writeShellScriptBin "wowdps-gen-${name}" ''
                exec "$(git rev-parse --show-toplevel)/tools/gen-${name}.sh" "$@"
              ''
            ) [ "class-spells" "keystone-timers" "item-spells" "icons" "spell-icons" "talent-trees" ]
            ++ [
              # rustc, cargo, clippy, rustfmt, rust-analyzer, rust-src and
              # llvm-tools — everything rust-toolchain.toml lists.
              (toolchainFor pkgs)
              # Coverage: `cargo llvm-cov --workspace`; the llvm-cov /
              # llvm-profdata it drives come from the toolchain's sysroot.
              pkgs.cargo-llvm-cov
              # `cargo audit` checks Cargo.lock against the RustSec advisory database.
              pkgs.cargo-audit
              # gawk drives the parser-independent fixture check
              # (crates/core/fixtures/verify.sh), locally and in CI — the
              # CI check job runs inside this shell.
              pkgs.gawk
            ]
            # iced-layershell links libxkbcommon at build time (via
            # smithay-client-toolkit's pkg-config probe).
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.pkg-config
              pkgs.libxkbcommon
            ]
            # libduckdb for `wowdps-history` (see duckdbEnv above).
            ++ [ pkgs.duckdb ];
          # The iced GUI dlopens these at runtime (winit → wayland/xkbcommon,
          # wgpu → vulkan); on NixOS they are not on the default search path.
          # libduckdb likewise, for `cargo test -p wowdps-history`.
          env = {
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
              [ (pkgs.lib.getLib pkgs.duckdb) ]
              ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
                pkgs.wayland
                pkgs.libxkbcommon
                pkgs.vulkan-loader
                pkgs.libGL
              ]
            );
          }
          // duckdbEnv pkgs;
        };
      });
    };
}
