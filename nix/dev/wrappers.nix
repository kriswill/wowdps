# The commands the dev shells put on PATH that are ABOUT this repo: the
# game-data generators, the workspace's own binaries and the extractor's
# dev-time tool. All three kinds resolve against the live checkout at run time
# rather than a store copy, which is the whole reason they are wrappers and not
# packages. The replay and its tools left for the private wowdps-replay
# repository (https://github.com/kriswill/wowdps-replay), and the map and
# portrait editors for wowdps-editors
# (https://github.com/kriswill/wowdps-editors); their own dev shells wrap
# them.
{ pkgs }:
let
  # `wowdps gen-<name>` external dispatch: the repo's tools/gen-*.sh on PATH
  # as wowdps-gen-<name>. Run once per game patch to regenerate the tables
  # (class_spells, keystone_timers, item_spells) and the per-machine art and
  # talent caches.
  generators = [
    "class-spells"
    "keystone-timers"
    "item-spells"
    "icons"
    "spell-icons"
    "talent-trees"
    # The spell tips and the placed table.
    "spell-tips"
    "placed-spells"
  ];

  # The workspace binaries, as the dispatcher's external-command lookup names
  # them (`wowdps history` → `wowdps-history`).
  binaries = [
    "wowdps"
    "wowdps-history"
    "wowdps-mcp"
    "wowdps-gui"
  ];

  # The generators cargo-build into the repo and write per-machine caches, so
  # they must run from the checkout, never from a store copy of themselves.
  genWrapper =
    name:
    pkgs.writeShellScriptBin "wowdps-gen-${name}" ''
      exec "$(git rev-parse --show-toplevel)/tools/gen-${name}.sh" "$@"
    '';

  # Built from the LIVE checkout on every call. A dev shell that hands you a
  # binary older than your last commit is worse than no binary at all — the
  # daemon silently kept running a pre-R22 meter for exactly that reason — so
  # each wrapper builds its own `--bin` first (a no-op build is ~0.12 s) and
  # only then runs it. `WOWDPS_NO_BUILD=1` skips the build and runs whatever
  # is already there.
  #
  # `exec` is load-bearing, not style: every binary here resolves its siblings
  # from `current_exe` first and PATH only as a fallback (`tui/src/main.rs`
  # find_bin, `gui/src/main.rs` daemon_bin, `daemon/src/lib.rs` gui_bin,
  # `mcp/src/bridge.rs`). Exec'ing replaces the wrapper, so `current_exe` is
  # `target/release/wowdps` and `wowdps history` finds its sibling; a spawn
  # would leave `current_exe` in /nix/store, where no sibling exists, and send
  # the daemon hunting on PATH with its stderr already nulled. Cross-binary
  # calls that miss still land on these wrappers through PATH, which build
  # lazily — one binary at a time, never the GUI's tree for a `wowdps status`.
  binWrapper =
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
        # stderr, never stdout: `wowdps mcp` speaks JSON-RPC on stdout and
        # `wowdps history sql --json` is piped into jq.
        cargo build --release --quiet --manifest-path "$root/Cargo.toml" \
          --bin ${name} >&2
      fi
      # Where cargo put it: under CARGO_TARGET_DIR when that is set (a
      # worktree's own target), else the checkout's.
      exec "''${CARGO_TARGET_DIR:-$root/target}/release/${name}" "$@"
    '';
  # The dev-time tools the extractor carries beside its generators, each a
  # subcommand of its binary (`bin`) under a name of its own: built from the
  # live checkout like the binaries, then exec'd with the arguments as given.
  tools = {
    # The game's tables as SQL: decoded into a per-build cache once, then
    # DuckDB with a view per table (tools/extract/src/db2sql.rs).
    "wowdps-db2" = {
      bin = "wowdps-extract";
      subcommand = "sql";
    };
  };

  toolWrapper =
    name: t:
    pkgs.writeShellScriptBin name ''
      set -eu
      root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
      if [ ! -e "$root/crates/core/fixtures/check.awk" ]; then
        echo "${name}: run this from inside the wowdps checkout" >&2
        exit 127
      fi
      if [ -z "''${WOWDPS_NO_BUILD:-}" ]; then
        # The release build the gen-* scripts use, so neither rebuilds the
        # other's.
        cargo build --release --quiet --manifest-path "$root/Cargo.toml" \
          -p ${t.bin} --bin ${t.bin} >&2
      fi
      exec "''${CARGO_TARGET_DIR:-$root/target}/release/${t.bin}" ${t.subcommand} "$@"
    '';
in
{
  packages =
    map genWrapper generators
    ++ map binWrapper binaries
    ++ pkgs.lib.mapAttrsToList toolWrapper tools;

  # The names, for the contract check — generated from the same lists that
  # build the wrappers, so the check cannot fall behind them the way a
  # hand-copied list does.
  names =
    map (name: "wowdps-gen-${name}") generators
    ++ binaries
    ++ builtins.attrNames tools;

}
