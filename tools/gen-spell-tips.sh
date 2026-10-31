#!/usr/bin/env bash
# Regenerate the replay's spell tips from the LOCAL game install.
#
# Every spell the history store's replays name, and every spell the
# curated tables in crates/core/src list, with what the replay's tooltips
# say of it and what its cast lanes ask of it: its name, school, cast
# time, range, cooldown, cost, the duration of the aura it applies,
# whether it is passive, channeled or empowered, whether it starts the
# global cooldown, the spell whose effect it is (its trigger
# parent, or the spell its description embeds, followed to the press made
# or to the passive that made it), its description with its values
# resolved at base values, the specs whose logs omit the press it is the
# effect of (a census of the stored replays), and for an encounter spell
# whose description says little, the journal's words. Written to
# $XDG_DATA_HOME/wowdps/spell-tips.bin beside spell-icons.bin: a
# PER-MACHINE CACHE of Blizzard's words, never committed. gui-logic's
# spell_tips, the reader it gains with its spell tips, reads it; a reader
# without the file shows names alone.
#
# A rerun finds the file current when the build, the spells asked for and
# the stored players' casts and auras have not changed since, and decodes
# no table (about 4 s: the store's replays are read to know which spells
# they name); new fights in the store or a new build regenerate it (about
# 7 s, the spell tables decoded). Network is used only for the WoWDBDefs
# schemas and the wowdev TACTKeys list.
#
# usage: tools/gen-spell-tips.sh [wow-dir] [--store DIR] [--force] [-o FILE]
#   wow-dir: folder holding .build.info and Data/. When omitted the tool
#   locates the install itself ($WOWDPS_WOW_DIR, the wowdps config's
#   logs_dir, or a scan of Steam compatdata prefixes).
#   --store: the history store (default $XDG_DATA_HOME/wowdps/history/v1;
#   a frozen copy of one works too).
#   --force: regenerate a file the stamp says is current.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build_extract
fetch_dbds gen-spell-tips "$work"
fetch_keys "$work/tactkeys.txt"

"$extract" gen-spell-tips "$@" --dbd-dir "$work" --keys "$work/tactkeys.txt"
