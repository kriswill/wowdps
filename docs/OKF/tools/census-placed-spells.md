---
type: Tool
title: census-placed-spells
description: 'Count, per log, every line of the spells the placed-spell table names (players'' casts and summons of the placing spells, the auras and heals that tell where they stand), so gen-placed-spells can refuse an entry whose spells no real log writes.'
resource: tools/census-placed-spells.sh
tags: [tool, game-data, replay]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-07T23:40:00-07:00 }
sources:
  - id: script
    resource: ../../../tools/census-placed-spells.sh
    title: tools/census-placed-spells.sh — the manual, in its header
  - id: csv
    resource: ../../../tools/placed-spells-census.csv
    title: The committed census
---

`tools/census-placed-spells.sh -o tools/placed-spells-census.csv LOG...`
asks the extractor which spells to count (`wowdps-extract
placed-spells-ids`, from the curated list), then greps each log
once[^script]. A row is an event, a spell id and the name the log wrote,
with a count per log[^csv]:

- `cast`: a player's SPELL_CAST_SUCCESS;
- `summon`: a player's SPELL_SUMMON or SPELL_CREATE;
- `aura`: SPELL_AURA_APPLIED on anyone (Wind Rush lands on pets and
  friendly NPCs as well as players);
- `heal`: SPELL_HEAL or SPELL_PERIODIC_HEAL.

[gen-placed-spells](gen-placed-spells.md) requires each entry's placing
spell as a cast or summon, and every spell it is found by as an aura or
heal. It also checks the name the log wrote, and prints the counts in the
review twin.

The committed census (2026-10-07) is 25 logs: the eight the
[role-spell census](census-role-spells.md) reads, every log a replay spike
pull was cut from, the night of 2026-10-07, two Mythic+ nights with Ring
of Peace in them, and the one with a Black Ox Statue.

[^script]: `tools/census-placed-spells.sh`'s header.
[^csv]: `tools/placed-spells-census.csv`: `event,id,name`, then one column per log.
