#!/usr/bin/env bash
# Seed the history store's creatures file (creatures.tsv, the NPC
# classifications the replay's rings read) from Wowhead's NPC data, so a
# rare or a rare elite rings silver before the wowdps addon has seen it in
# game. It classifies every hostile NPC of the store's replay tier and
# every NPC of the season's dungeon and raid zone listings (the zone table
# is in tools/extract/src/wowhead.rs), then lays those rows under the
# file's own: a seed row is seen_unix 0 and never a lieutenant (Wowhead has
# no lieutenant flag and no `minus` rank), so any sighting the addon
# records wins, and a rerun replaces only seed rows. The fetches go through
# curl, two a second at most, and every answer is cached under
# ~/.cache/wowdps/wowhead/, so a rerun asks only for what it lacks and an
# interrupted run resumes. Nothing fetched enters the repository.
#
#   tools/seed-creatures.sh [--store DIR] [-o FILE] [--cache DIR] [--dry-run]
#
# `--store` is the history store's directory ($XDG_DATA_HOME/wowdps/history/v1
# by default), whose creatures.tsv it writes unless `-o` names another
# file; `--dry-run` fetches and reports without writing. The daemon reads
# the file when it opens the store; until its build folds the file on disk
# in before each merge, restart the dev unit after seeding under a live
# daemon (`systemctl --user restart wowdps-dev`, asked first), or its next
# merge of the addon's records writes the file without the seed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
. "$root/tools/lib.sh"

build_extract
"$extract" seed-creatures "$@"
