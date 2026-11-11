#!/usr/bin/env bash
# Regenerate the games' emblems the window's edition switch draws, from
# the LOCAL game launcher's own files.
#
# The launcher's play selector shows each game with its own emblem: the
# current expansion's for Retail, Forever's for Forever. Both are SVG
# documents the launcher keeps in its content-addressed cache, named by
# its catalog of the game (rules in tools/extract/src/emblemgen.rs). They
# are copied as they are into $XDG_DATA_HOME/wowdps/edition-emblems.bin, a
# PER-MACHINE cache the GUI reads at runtime and rasterizes at the size it
# draws. A game whose emblem the launcher's cache does not hold is left
# out, and the switch names it alone.
#
# No network, no game files: the launcher's cache is read and nothing
# else. Rerun it when the launcher shows a new emblem (a new expansion).
#
# Like class-icons.bin this is Blizzard's art and lives OUTSIDE the
# repository on purpose; a machine without it draws each game's name
# alone.
#
# usage: tools/gen-emblems.sh [wow-dir] [--launcher DIR] [-o FILE]
#   wow-dir: folder holding .build.info; the launcher's folder is found
#   beside it, under the same Wine prefix (or %LOCALAPPDATA% on Windows).
#   When omitted the tool locates the install itself ($WOWDPS_WOW_DIR, the
#   wowdps config's logs_dir, or a scan of the install roots).
#   --launcher DIR: the launcher's local data folder (the one holding its
#   Cache/), where it is not found beside the install (macOS among them).
#   -o: the cache's path (default $XDG_DATA_HOME/wowdps/edition-emblems.bin).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

build_extract
"$extract" gen-emblems "$@"
