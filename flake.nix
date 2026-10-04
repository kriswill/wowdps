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
    # crane: the two-derivation Rust build (see `packages`).
    crane.url = "github:ipetkov/crane";
    # okf, the knowledge-bundle CLI (docs/OKF, okflight.toml), ships from
    # FlakeHub (kriswill/okflight, public); "0" tracks the 0.x release
    # series — `nix flake update okf` moves to the newest release.
    # devenv.yaml mirrors this input.
    okf = {
      url = "https://flakehub.com/f/kriswill/okflight/0";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # The CodeRabbit CLI (`coderabbit`, alias `cr`: an AI review of the
    # working tree or a branch, `coderabbit review --agent`) — a prebuilt,
    # unfree binary nixpkgs does not carry, packaged (and updated daily) by
    # numtide's llm-agents.nix. devenv.yaml mirrors this input.
    # It keeps its OWN nixpkgs: its package set needs a newer one than this
    # repo pins (following ours, it does not evaluate).
    llm-agents.url = "github:numtide/llm-agents.nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      crane,
      okf,
      llm-agents,
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
      # okf, the knowledge-bundle CLI over docs/OKF (okflight.toml), from the
      # okflight input — on the dev-shell PATH and exported as `.#okf`.
      okfFor = pkgs: okf.packages.${pkgs.stdenv.hostPlatform.system}.okf;
      # The CodeRabbit CLI, from the llm-agents input — on the dev-shell PATH.
      coderabbitFor = pkgs: llm-agents.packages.${pkgs.stdenv.hostPlatform.system}.coderabbit-cli;
      # BOTH dev shells' contents, declared once: the wrappers, the packages
      # and the environment that `nix develop` and devenv.nix each hand you.
      # Importing the same file is what keeps the twins from drifting; each
      # shell adds only what it alone plumbs (its toolchain, and the CLIs it
      # reaches through its own inputs: okf, coderabbit).
      devShellFor = pkgs: import ./nix/dev { inherit pkgs; };
      # The two DUCKDB_* variables the history reader's sys crate needs — the
      # PACKAGE build wants them too, and a package is not a shell.
      duckdbEnv = pkgs: (devShellFor pkgs).duckdbEnv;
    in
    {
      # The daemon + TUI binary (`wowdps`) plus its siblings — the MCP server
      # (`wowdps-mcp`, reached as `wowdps mcp`) and the history reader
      # (`wowdps-history`, `wowdps history`): pure Rust except libduckdb,
      # which the history binary finds through its rpath. `wowdps-gui` is
      # its own package with its own dependency layer (GPUI is most of the
      # compile and the daemon never needs it): the binary is wrapped so the
      # libraries GPUI dlopens (wayland-client, vulkan-loader, EGL/GL) sit
      # on LD_LIBRARY_PATH, and both modules put it on the daemon's PATH by
      # default, which is how the overlay supervisor finds it.
      #
      # Built with crane in two derivations so CI never recompiles the
      # dependency tree: `wowdps-deps` compiles every dependency crate
      # against a source with the workspace's own crates stubbed out (its
      # hash depends on the Cargo manifests + lockfile only, so it is a
      # binary-cache download on every PR that leaves Cargo.lock alone),
      # and `wowdps` builds and tests just the workspace crates on top.
      packages = forAllSystems (
        pkgs:
        let
          lib = pkgs.lib;
          craneLib = (crane.mkLib pkgs).overrideToolchain (toolchainFor pkgs);
          # Only what the build and its tests read. `src = ./.` would hash
          # the whole checkout, so a README or docs edit would rebuild the
          # binary from scratch; this set changes only when the Rust
          # sources, the fixtures, or the one doc a test parses do.
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./rust-toolchain.toml
              ./clippy.toml
              ./crates
              ./tools/extract
              # crates/daemon embeds the addon with include_str!.
              ./addon
              # crates/history/tests/parity.rs executes every recipe in it.
              ./docs/history-queries.md
            ];
          };
          commonArgs = {
            pname = "wowdps";
            version = "0.1.0";
            inherit src;
            strictDeps = true;
            # Only the three shipped binaries.
            cargoExtraArgs = "-p wowdps-tui -p wowdps-mcp -p wowdps-history";
            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            buildInputs = [ (lib.getLib pkgs.duckdb) ];
            # The one native library in the closure, on the one binary
            # that needs it; the tests need it on the load path too.
            LD_LIBRARY_PATH = "${lib.getLib pkgs.duckdb}/lib";
          }
          // duckdbEnv pkgs;
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          # The GUI: GPUI Kit over model + proto + gui-logic. It LINKS
          # libxkbcommon(-x11), libxcb and fontconfig (font-kit's pkg-config
          # probe); wayland-client, vulkan and EGL/GL are dlopened and only
          # need to be findable at run time, which the wrapper below
          # provides. crates/gui/build.rs also bakes the build-time
          # LD_LIBRARY_PATH into the RUNPATH, so one list serves both (nix's
          # fixup then shrinks the RUNPATH to what the binary links, which
          # is why the package still runs through its wrapper).
          guiLibraries = lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.wayland
            pkgs.libxkbcommon
            pkgs.vulkan-loader
            pkgs.libGL
          ];
          guiArgs = {
            pname = "wowdps-gui";
            version = "0.1.0";
            inherit src;
            strictDeps = true;
            cargoExtraArgs = "-p wowdps-gui";
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.makeWrapper
            ];
            buildInputs =
              guiLibraries
              ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
                pkgs.libxcb
                pkgs.fontconfig
              ];
            LD_LIBRARY_PATH = lib.makeLibraryPath guiLibraries;
          };
          guiArtifacts = craneLib.buildDepsOnly guiArgs;
        in
        rec {
          wowdps = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = lib.concatStringsSep " " (
                map (c: "-p wowdps-${c}") [
                  "model"
                  "core"
                  "proto"
                  "daemon"
                  "tui"
                  "mcp"
                  "history"
                ]
              );
              meta.mainProgram = "wowdps";
            }
          );
          default = wowdps;
          wowdps-deps = cargoArtifacts;
          wowdps-gui = craneLib.buildPackage (
            guiArgs
            // {
              cargoArtifacts = guiArtifacts;
              # Kit's click-through harness and the real-text headless
              # context; nothing opens a display. The real-text tests open a
              # headless window, and GPUI's headless renderer wants a wgpu
              # adapter even when nothing is captured: in the sandbox, Mesa's
              # lavapipe (software Vulkan). They shape text in the system UI
              # face too, which the sandbox lacks: DejaVu Sans, one of GPUI's
              # fallbacks, through a fontconfig file cosmic-text's fontdb
              # reads. The render guard and the shots are ignored tests, run
              # by hand (crates/gui/SHOTS.md).
              cargoTestExtraArgs = "-p wowdps-gui";
              preCheck = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                export VK_DRIVER_FILES=${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.uname.processor}.json
                export VK_ICD_FILENAMES=$VK_DRIVER_FILES
                export FONTCONFIG_FILE=${pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; }}
              '';
              postInstall = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                wrapProgram $out/bin/wowdps-gui \
                  --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath guiLibraries}
              '';
              meta.mainProgram = "wowdps-gui";
            }
          );
          wowdps-gui-deps = guiArtifacts;
        }
      );

      homeManagerModules = rec {
        wowdps = { pkgs, ... }: {
          imports = [ ./nix/home-manager.nix ];
          services.wowdps.package =
            nixpkgs.lib.mkDefault
              self.packages.${pkgs.stdenv.hostPlatform.system}.wowdps;
          services.wowdps.guiPackage =
            nixpkgs.lib.mkDefault
              self.packages.${pkgs.stdenv.hostPlatform.system}.wowdps-gui;
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
          services.wowdps.guiPackage =
            nixpkgs.lib.mkDefault
              self.packages.${pkgs.stdenv.hostPlatform.system}.wowdps-gui;
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
          packages = (devShellFor pkgs).packages ++ [
            # rustc, cargo, clippy, rustfmt, rust-analyzer, rust-src and
            # llvm-tools — everything rust-toolchain.toml lists. devenv reads
            # the same file through its own `languages.rust`.
            (toolchainFor pkgs)
            # okf (scaffold | index | validate | viz) over docs/OKF, the OKF
            # knowledge bundle — see .claude/skills/knowledge-bundle. It
            # reaches this shell through the flake's own input and devenv's
            # through devenv.yaml's, which is why it is not in the shared file.
            (okfFor pkgs)
            # coderabbit / cr, the CodeRabbit review CLI — the same split as
            # okf: this shell's llm-agents input, devenv's mirrored one.
            (coderabbitFor pkgs)
          ];
          env = (devShellFor pkgs).env;
        };
      });
    };
}
