#!/usr/bin/env bash
# The dev daemon as a systemd user unit that follows the checkout.
#
#   tools/dev-unit.sh install            write + enable + start the units
#   tools/dev-unit.sh profile [debug|release]   show or switch the build the
#                                        daemon runs (restarts it if running)
#   tools/dev-unit.sh status             profile, gui, units, `wowdps status`
#   tools/dev-unit.sh uninstall          stop, disable, remove the units
#
# Unit entry points (not for hands): run | stop-daemon | reload.
#
# Model: wowdps-dev.service execs target/<profile>/wowdps daemon --linger
# from THIS checkout, stamping the binaries it started from: the daemon and
# the GUI it spawns as the overlay, config.toml's `gui_binary` (default
# wowdps-gui). wowdps-dev.path fires wowdps-dev-reload on any write to
# target/{debug,release}/wowdps{,-gui,-gui-new}; reload waits for the writes
# to settle, and restarts the service only when the ACTIVE profile's stamp
# changed — so a debug build never bounces a release daemon, a build of the
# GUI the daemon does not spawn bounces nothing, and a `systemctl --user stop
# wowdps-dev` stays stopped.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
env_file="${XDG_CONFIG_HOME:-$HOME/.config}/wowdps/dev-unit.env"
config_file="${XDG_CONFIG_HOME:-$HOME/.config}/wowdps/config.toml"
run_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/wowdps"
stamp_file="$run_dir/dev-unit.stamp"
units=(wowdps-dev.service wowdps-dev.path wowdps-dev-reload.service)

profile() {
  local p=release
  # shellcheck disable=SC1090
  [ -r "$env_file" ] && . "$env_file"
  echo "${WOWDPS_PROFILE:-$p}"
}

bin_dir() { echo "$root/target/$(profile)"; }

# The daemon's reading of `gui_binary` (crates/daemon/src/config.rs): the
# last non-empty `gui_binary = "…"` above the first [section], else
# wowdps-gui. Escapes are beyond this reader: a value holding a quote or a
# backslash reads as the default here.
gui_binary() {
  local v=""
  [ -r "$config_file" ] && v="$(awk '
    /^[[:space:]]*\[/ { exit }
    /^[[:space:]]*gui_binary[[:space:]]*=/ {
      s = $0
      sub(/^[^=]*=[[:space:]]*/, "", s)
      if (match(s, /^"[^"\\]+"[[:space:]]*(#.*)?$/)) {
        sub(/"[[:space:]]*(#.*)?$/, "", s)
        v = substr(s, 2)
      }
    }
    END { print v }' "$config_file")"
  echo "${v:-wowdps-gui}"
}

# Where the daemon finds it (Config::gui_bin): a value with a `/` as written,
# a bare name beside the daemon. A bare name with no sibling is spawned from
# $PATH, which is not this checkout's to stamp or watch.
gui_path() {
  local g; g="$(gui_binary)"
  case "$g" in
    */*) echo "$g" ;;
    *) echo "$(bin_dir)/$g" ;;
  esac
}

# Path, inode, size and mtime of the daemon and the GUI it spawns — the path
# too, so switching `gui_binary` changes the stamp even between two missing
# binaries. A missing GUI stamps as "-": the daemon still runs, the overlay
# just cannot spawn from here.
stamp() {
  local b
  for b in "$(bin_dir)/wowdps" "$(gui_path)"; do
    echo "$b $(stat -c '%i:%s:%Y' "$b" 2>/dev/null || echo -)"
  done | paste -sd' '
}

daemon_running() {
  WOWDPS_NO_BUILD=1 "$1" status >/dev/null 2>&1
}

# Ask any running daemon (unit's or self-spawned) to stop, then wait for it
# to RELEASE THE LOCK — `stop` is async, the socket vanishes before the
# process does, and the daemon holds an flock on wowdps-v<N>.lock until it
# exits. Then take down orphan overlays: a tick-driven client with no daemon
# spawns one on its own, and it beat the unit's daemon to the lock once.
lock_free() {
  local f
  for f in "$run_dir"/wowdps-v*.lock; do
    [ -e "$f" ] || continue
    flock -n "$f" true 2>/dev/null || return 1
  done
}

stop_daemon() {
  local bin="$1"
  if [ -x "$bin" ] && daemon_running "$bin"; then
    WOWDPS_NO_BUILD=1 "$bin" stop >/dev/null 2>&1 || true
  fi
  for _ in $(seq 1 30); do
    lock_free && break
    sleep 0.5
  done
  if ! lock_free; then
    echo "dev-unit: a daemon still holds the lock after 15 s" >&2
    return 1
  fi
  # Either GUI's overlay: one a `gui_binary` switch left behind is an orphan
  # too.
  pkill -f 'wowdps-gui(-new)? --overlay$' 2>/dev/null || true
}

cmd_run() {
  local p bin; p="$(profile)"; bin="$(bin_dir)/wowdps"
  if [ ! -x "$bin" ]; then
    echo "dev-unit: no $p daemon at $bin — cargo build ${p/release/--release} --bin wowdps first" >&2
    exit 78
  fi
  local gui; gui="$(gui_path)"
  if [ ! -x "$gui" ]; then
    case "$(gui_binary)" in
      */*) echo "dev-unit: gui_binary $gui is not an executable; the overlay will not spawn" >&2 ;;
      *) echo "dev-unit: no $p $(gui_binary) beside the daemon; the overlay spawns from \$PATH if one is there, else not until one is built" >&2 ;;
    esac
  fi
  stop_daemon "$bin"
  # Lost the lock anyway (a client re-spawned in the gap): exit 1 so systemd
  # retries — the daemon itself exits 0 on "already running".
  lock_free || { echo "dev-unit: lock taken while starting, retrying" >&2; exit 1; }
  mkdir -p "$(dirname "$stamp_file")"
  stamp >"$stamp_file"
  echo "dev-unit: starting $p daemon $bin" >&2
  exec "$bin" daemon --linger
}

cmd_reload() {
  systemctl --user is-active --quiet wowdps-dev.service || exit 0
  local d; d="$(bin_dir)"
  # Settle: cargo unlinks then hardlinks, and a link step can take seconds.
  local prev="" cur
  for _ in $(seq 1 30); do
    cur="$(stamp)"
    if [ "$cur" = "$prev" ]; then break; fi
    prev="$cur"; sleep 2
  done
  [ -x "$d/wowdps" ] || exit 0              # mid-rebuild; the path unit re-fires
  [ "$cur" != "$(cat "$stamp_file" 2>/dev/null || true)" ] || exit 0
  echo "dev-unit: $(profile) binaries changed, restarting the daemon" >&2
  systemctl --user restart wowdps-dev.service
}

cmd_install() {
  mkdir -p "$unit_dir" "$(dirname "$env_file")"
  [ -e "$env_file" ] || echo "WOWDPS_PROFILE=release" >"$env_file"
  for u in "${units[@]}"; do
    sed "s|@ROOT@|$root|g" "$root/tools/dev-unit/$u" >"$unit_dir/$u"
  done
  systemctl --user daemon-reload
  systemctl --user enable wowdps-dev.path wowdps-dev.service
  systemctl --user start wowdps-dev.path
  if [ "${1:-}" = "--no-start" ]; then
    echo "dev-unit: installed and enabled; start with: systemctl --user start wowdps-dev"
  else
    systemctl --user start wowdps-dev.service
    cmd_status
  fi
}

cmd_uninstall() {
  systemctl --user disable --now wowdps-dev.path wowdps-dev.service 2>/dev/null || true
  for u in "${units[@]}"; do rm -f "$unit_dir/$u"; done
  systemctl --user daemon-reload
  echo "dev-unit: removed"
}

cmd_profile() {
  case "${1:-}" in
    "") profile; return ;;
    debug|release) ;;
    *) echo "usage: $0 profile [debug|release]" >&2; exit 2 ;;
  esac
  mkdir -p "$(dirname "$env_file")"
  echo "WOWDPS_PROFILE=$1" >"$env_file"
  echo "dev-unit: profile = $1"
  if systemctl --user is-active --quiet wowdps-dev.service; then
    systemctl --user restart wowdps-dev.service
    cmd_status
  fi
}

cmd_status() {
  echo "profile: $(profile)  ($(bin_dir))"
  local gui; gui="$(gui_path)"
  echo "gui:     $(gui_binary)  ($gui$([ -x "$gui" ] || echo ", missing"))"
  for u in wowdps-dev.service wowdps-dev.path; do
    local en ac
    en="$(systemctl --user is-enabled "$u" 2>/dev/null)" || en="${en:-absent}"
    ac="$(systemctl --user is-active "$u" 2>/dev/null)" || true
    printf '%-24s %s / %s\n' "$u" "$en" "$ac"
  done
  local bin; bin="$(bin_dir)/wowdps"
  [ -x "$bin" ] && WOWDPS_NO_BUILD=1 "$bin" status || true
}

case "${1:-}" in
  install)     cmd_install "${2:-}" ;;
  uninstall)   cmd_uninstall ;;
  profile)     cmd_profile "${2:-}" ;;
  status)      cmd_status ;;
  run)         cmd_run ;;
  stop-daemon) stop_daemon "$(bin_dir)/wowdps" ;;
  reload)      cmd_reload ;;
  *) sed -n '2,10p' "$0"; exit 2 ;;
esac
