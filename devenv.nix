# Devenv twin of flake.nix's devShells.default — the same environment, entered
# via devenv's native cd hook (trust once with `devenv allow`) instead of
# `nix develop`. The environment ITSELF lives in nix/dev-shell.nix, imported by
# both, so the two can no longer drift; what stays here is only what devenv
# plumbs differently — its Rust toolchain, its okf input, and the contract
# `devenv test` asserts.
{
  pkgs,
  lib,
  inputs,
  ...
}:
let
  shared = import ./nix/dev-shell.nix { inherit pkgs lib; };
in
{
  # The toolchain comes from rust-toolchain.toml (nightly + components)
  # through rust-overlay — the same file and overlay the flake devShell
  # uses, so the two can never drift. rust-overlay is a devenv.yaml input
  # pinned to flake.lock's rev.
  languages.rust.enable = true;
  languages.rust.toolchainFile = ./rust-toolchain.toml;

  packages = shared.packages ++ [
    # okf (scaffold | index | validate | viz) over docs/OKF, the OKF
    # knowledge bundle — the nix-built CLI from the okflight input
    # (devenv.yaml). The flake shell reaches the same CLI through its own
    # input, which is why this one package is not in nix/dev-shell.nix.
    inputs.okf.packages.${pkgs.stdenv.hostPlatform.system}.okf
  ];

  env = shared.env;

  # The contract `devenv test` asserts — the environment must provide what the
  # flake devShell promises, not merely "evaluation didn't crash". The wrapper
  # names come from the same list that builds them, so a new generator or
  # binary cannot be added without this check covering it.
  enterTest = ''
    set -euo pipefail
    for tool in cargo rustc clippy-driver rustfmt rust-analyzer cargo-llvm-cov gawk okf \
                ${lib.concatStringsSep " " shared.commands}; do
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
