# CodeQL, GitHub's code-scanning engine, as a LOCAL check: the CLI on PATH
# and `wowdps-codeql`, which runs tools/codeql.sh from the live checkout. CI
# never calls it: GitHub's CodeQL default setup (the repository's settings,
# no workflow file) scans every PR, and the script mirrors that run.
{
  pkgs,
  lib ? pkgs.lib,
}:
let
  # The CLI is unfree, so nixpkgs refuses to evaluate it without an
  # allowance. This import is the only place one is given, by name and for
  # this package alone: the nixpkgs both shells evaluate everything else
  # from (flake.nix's import, devenv's input) stays free-only, so nothing
  # else unfree can arrive beside it. Same source and system as `pkgs`, so
  # it builds to the same store path; the cost is one more evaluation of
  # nixpkgs.
  unfree = [ "codeql" ];
  allowed = import pkgs.path {
    inherit (pkgs.stdenv.hostPlatform) system;
    config.allowUnfreePredicate = pkg: builtins.elem (lib.getName pkg) unfree;
  };
  inherit (allowed) codeql;

  # The script lives in the checkout, like the generators: an edit to it
  # takes effect without re-entering the shell. The wrapper carries what the
  # script runs, so it works whatever the PATH.
  check = pkgs.writeShellApplication {
    name = "wowdps-codeql";
    runtimeInputs = [
      codeql
      pkgs.jq
      pkgs.gnutar
      pkgs.gawk
    ];
    text = ''
      root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
      # The shell stays loaded after you cd away (wrappers.nix).
      if [ ! -e "$root/crates/core/fixtures/check.awk" ]; then
        echo "wowdps-codeql: run this from inside the wowdps checkout" >&2
        exit 127
      fi
      # The Rust extractor models the standard library from RUST_SRC_PATH,
      # and without it reads the toolchain's own rust-src: the nightly
      # rust-toolchain.toml names, newer than this CLI can resolve. On
      # nightly-2026-10-19 it resolved 28 % of calls, flagged the database
      # as degraded and missed 7 of main's 10 alerts. nixpkgs' stable
      # library source reads clean (1.98.1 on the 2026-10-23 pin). It is
      # what CodeQL reads std from, not a toolchain: nothing builds with it.
      export RUST_SRC_PATH=${pkgs.rustPlatform.rustLibSrc}
      exec "$root/tools/codeql.sh" "$@"
    '';
  };
in
{
  packages = [
    codeql
    check
  ];

  # For the contract, as wrappers.nix hands it its names.
  names = [
    "codeql"
    "wowdps-codeql"
  ];
}
