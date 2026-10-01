#!/usr/bin/env bash
# Replay a real combat log, at speed, through an overlay on a headless
# Hyprland output: a stand-in for a raid night when there is no game, and
# the cost measure for either GUI's overlay under live load.
#
#   tools/overlay-replay.sh LOG GUI_BINARY [--speed N] [--name NAME] [--out DIR]
#
#   LOG          a WoWCombatLog-*.txt (one night; e.g. the design shots'
#                coiled-altar-night.txt under ~/.local/share/wowdps/design-shots)
#   GUI_BINARY   target/release/wowdps-gui, or another build of it
#   --speed N    how many times faster than the log's own clock (default 4)
#   --name NAME  the run's label in the results (default: the binary's name)
#   --out DIR    where results, screenshots and the isolated state go
#                (default: $XDG_CACHE_HOME/wowdps/replay/NAME)
#
# Isolation: the run starts its OWN daemon (the GUI binary's sibling
# `wowdps`) with its own runtime, config, data, cache and state dirs, so
# the dev daemon, its socket, its history store and the user's config are
# never touched. The runtime dir is a short /tmp path, since a unix socket's
# path must fit SUN_LEN; it links the session's Wayland and Hyprland
# sockets so the overlay still reaches the compositor. Only processes this
# script started are ever stopped.
#
# The overlay goes to a headless output (an existing HEADLESS-*, else one
# created and removed at the end); a surface that lands anywhere else is
# killed at once. A headless output hot-plugged while the game runs has
# glitched it, so the script refuses to run then.
#
# Results: idle CPU before any data, CPU and RSS every 30 s while the log
# streams, idle after (the staleness radar runs then), the overlay's stderr
# and panics, and screenshots (with grim) a minute in, halfway and at the
# end. Needs hyprctl, jq, awk, split; grim is optional.

set -euo pipefail

usage() { sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[ $# -ge 2 ] || usage
LOG=$(readlink -f "$1"); GUI=$(readlink -f "$2"); shift 2
SPEED=4; NAME=$(basename "$GUI"); OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --speed) SPEED=$2; shift 2 ;;
    --name) NAME=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    *) usage ;;
  esac
done
[ -f "$LOG" ] || { echo "no log at $LOG" >&2; exit 1; }
[ -x "$GUI" ] || { echo "no GUI binary at $GUI" >&2; exit 1; }
DAEMON=$(dirname "$GUI")/wowdps
[ -x "$DAEMON" ] || { echo "no daemon beside the GUI at $DAEMON" >&2; exit 1; }
for tool in hyprctl jq awk split; do
  command -v "$tool" >/dev/null || { echo "needs $tool" >&2; exit 1; }
done
if pgrep -f 'Wow(-64)?\.exe' >/dev/null; then
  echo "the game is running: a headless output hot-plugged now can glitch it" >&2
  exit 1
fi
OUT=${OUT:-${XDG_CACHE_HOME:-$HOME/.cache}/wowdps/replay/$NAME}
mkdir -p "$OUT"; OUT=$(readlink -f "$OUT")

# ---- the headless output -------------------------------------------------
CREATED=""
HEADLESS=$(hyprctl monitors -j | jq -r '[.[] | select(.name | startswith("HEADLESS"))][0].name // empty')
if [ -z "$HEADLESS" ]; then
  before=$(hyprctl monitors -j | jq -r '.[].name' | sort)
  hyprctl output create headless >/dev/null
  sleep 0.5
  HEADLESS=$(comm -13 <(echo "$before") <(hyprctl monitors -j | jq -r '.[].name' | sort) | head -1)
  [ -n "$HEADLESS" ] || { echo "could not create a headless output" >&2; exit 1; }
  CREATED=$HEADLESS
fi
X=$(hyprctl monitors -j | jq -r 'map(.x + .width) | max')
hyprctl eval "hl.monitor({ output='$HEADLESS', mode='1920x1080', position='${X}x0', scale=1 })" >/dev/null || true
GEOM=$(hyprctl monitors -j | jq -r --arg m "$HEADLESS" '.[] | select(.name==$m) | "\(.x),\(.y) \(.width)x\(.height)"')

# ---- the isolated world --------------------------------------------------
RUN=$(mktemp -d /tmp/wdr.XXXX)
chmod 700 "$RUN"
SESSION_RUN=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
ln -s "$SESSION_RUN/${WAYLAND_DISPLAY:-wayland-1}" "$RUN/${WAYLAND_DISPLAY:-wayland-1}"
ln -s "$SESSION_RUN/hypr" "$RUN/hypr"
W=$OUT/world
rm -rf "$W"; mkdir -p "$W/config/wowdps" "$W/data/wowdps" "$W/cache" "$W/state" "$W/logs" "$W/chunks" "$OUT/shots"
for f in class-icons.bin spell-icons.bin talents.json talent-art.bin; do
  src=${XDG_DATA_HOME:-$HOME/.local/share}/wowdps/$f
  [ -e "$src" ] && ln -s "$src" "$W/data/wowdps/$f"
done
cat > "$W/config/wowdps/config.toml" <<EOF
logs_dir = "$W/logs"
auto_overlay = false
monitor = "$HEADLESS"
EOF
export XDG_RUNTIME_DIR=$RUN XDG_CONFIG_HOME=$W/config XDG_DATA_HOME=$W/data \
  XDG_CACHE_HOME=$W/cache XDG_STATE_HOME=$W/state

# ---- the log in chunks, a chunk every 250 ms ----------------------------
# The log's span from its first and last stamps ("9/27/2026 18:58:54.564-7"),
# one day's wrap allowed for.
secs() { awk '{split($2, t, /[:.-]/); print t[1]*3600 + t[2]*60 + t[3]; exit}'; }
first=$(head -1 "$LOG" | secs); last=$(tail -1 "$LOG" | secs)
span=$(( last >= first ? last - first : last + 86400 - first ))
lines=$(wc -l < "$LOG")
ticks=$(awk -v s="$span" -v k="$SPEED" 'BEGIN { t = int(s / k * 4); print (t < 1 ? 1 : t) }')
per=$(( (lines + ticks - 1) / ticks ))
split -l "$per" -a 5 -d "$LOG" "$W/chunks/c"
TARGET=$W/logs/$(basename "$LOG")
case "$(basename "$TARGET")" in WoWCombatLog-*) ;; *) TARGET=$W/logs/WoWCombatLog-010126_000000.txt ;; esac
: > "$TARGET"

DPID=""; OPID=""; RPID=""
cleanup() {
  for p in $RPID $OPID $DPID; do kill "$p" 2>/dev/null || true; done
  rm -rf "$RUN"
  [ -n "$CREATED" ] && hyprctl output remove "$CREATED" >/dev/null 2>&1 || true
}
trap cleanup EXIT

R=$OUT/results.txt
: > "$R"
say() { echo "$*" | tee -a "$R"; }
say "$NAME: $(basename "$LOG") ($lines lines, ${span}s) at ${SPEED}x on $HEADLESS"

"$DAEMON" daemon --linger > "$OUT/daemon.out" 2>&1 &
DPID=$!
sleep 2
WOWDPS_OVERLAY_OUTPUT=$HEADLESS WOWDPS_OVERLAY_START_EXPANDED=1 "$GUI" --overlay > "$OUT/overlay.err" 2>&1 &
OPID=$!
sleep 3
on=$(hyprctl layers -j | jq -r --argjson p "$OPID" \
  'to_entries[] | .key as $m | .value.levels[][] | select(.pid==$p) | $m' | sort -u | tr '\n' ' ')
case "$on" in
  *"$HEADLESS"*) say "layer on: $on" ;;
  *) say "the overlay's surface is on '${on:-nothing}', not $HEADLESS: stopped"; exit 1 ;;
esac

tck=$(getconf CLK_TCK)
cpu() { awk '{print $14 + $15}' "/proc/$1/stat" 2>/dev/null; }
rss() { awk '/VmRSS/ {print int($2 / 1024)}' "/proc/$1/status" 2>/dev/null; }
sample() {
  local label=$1 n=$2 c0 d0 c1 d1
  c0=$(cpu "$OPID"); d0=$(cpu "$DPID"); sleep "$n"; c1=$(cpu "$OPID"); d1=$(cpu "$DPID")
  if [ -z "$c1" ]; then say "$label: the overlay is GONE"; return 1; fi
  say "$(awk -v l="$label" -v c="$((c1 - c0))" -v d="$((d1 - d0))" -v t="$tck" -v n="$n" -v r="$(rss "$OPID")" \
    'BEGIN { printf "%-12s overlay %5.2f %%  rss %4d MiB  daemon %5.2f %%", l, 100 * c / (t * n), r, 100 * d / (t * n) }')"
}
shot() { command -v grim >/dev/null && grim -g "$GEOM" "$OUT/shots/$NAME-$1.png" || true; }

sample idle-before 30
( for c in "$W"/chunks/c*; do cat "$c" >> "$TARGET"; sleep 0.25; done ) &
RPID=$!
t=0; half=$(( ticks / 8 ))
while kill -0 "$RPID" 2>/dev/null; do
  sample "live+${t}s" 30 || break
  t=$((t + 30))
  [ "$t" -eq 60 ] && shot 60s
  [ "$t" -ge "$half" ] && [ "$((t - 30))" -lt "$half" ] && shot half
done
RPID=""
sample idle-after 30
shot end
say "overlay stderr: $(wc -l < "$OUT/overlay.err") lines, $(grep -c panicked "$OUT/overlay.err" || true) panics"
say "live mean: $(awk '/^live/ {s += $3; n++} END { if (n) printf "%.2f %% over %d samples", s / n, n }' "$R")"
