---
type: Tool
title: gen-spell-tips
description: 'Write the replay''s spell tips (spell-tips.bin beside spell-icons.bin): for every spell the stored replays and the curated tables name, its tooltip''s words (read at base values) and the facts the HUD''s cast lanes ask (passive, channeled, empowered, on the GCD, its parent press, the specs whose logs omit that press: a census of the stored replays), from the local install; a per-machine cache, never committed.'
resource: tools/gen-spell-tips.sh
tags: [tool, replay, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:30:00-07:00 }
sources:
  - id: script
    resource: ../../../tools/gen-spell-tips.sh
    title: tools/gen-spell-tips.sh — the manual, in its header
  - id: gen
    resource: ../../../tools/extract/src/spelltipgen.rs
    title: spelltipgen.rs — the spells asked for, the facts, root_of, the file's writer
  - id: tip
    resource: ../../../tools/extract/src/spelltip.rs
    title: spelltip.rs — the description's substitution the talent viewer uses too
  - id: journal
    resource: ../../../tools/extract/src/journal.rs
    title: journal.rs — journal_texts, the reading gen-rubric gives an ability's text
  - id: reader
    resource: ../crates/gui-logic.md
    title: wowdps-gui-logic — spell_tips, the file's reader that gui-logic gains with its spell tips (its words, pressed/passive_success)
---

`tools/gen-spell-tips.sh [wow-dir] [--store DIR] [--force] [-o FILE]`
builds [wowdps-extract](../crates/extract.md) and runs
`gen-spell-tips`[^script]. It writes `$XDG_DATA_HOME/wowdps/spell-tips.bin`,
beside [gen-spell-icons](gen-spell-icons.md)' `spell-icons.bin`: what the
replay spike's tooltips say of a spell and what the followed unit's cast
lanes ask of it
(Show The Followed Unit's HUD At The Replay Map's Foot,
phase 3). Blizzard's words, so a per-machine cache like the icons: the
spike draws a name-only tooltip and phase 2's lanes without it.

## The spells asked for

- **Every spell the history store's replays name** (`--store`, the live
  store unless it says; a frozen one works): each event's and placed
  row's, read offline as [seed-creatures](seed-creatures.md) reads the
  tier, a boss row's encounter id and a death left out.
- **Every spell the curated tables list:** R8's class spells, R12's item
  spells, R18's role spells, R20's absorbs and R26's procs, read from the
  committed `crates/core/src/*_spells.rs` the extractor is built from
  (`include_str!`; core's lookups are crate-private).
- **Each one's parent**, so an effect mark can say its name.

On 2026-10-11: 22,702 asked for (3,908 from 595 replay files, 1,711 from
51 pull directories, 19,597 from the tables), 23,329 records written, 468
asked for with no name in the client (server-side spells). The pull
directories (`--pulls`) left the inputs later that day, when the replay
spike came to read the store alone: their fights are stored ones, the
three from Warcraft Logs imported.

## What a record holds

From the base rows (DifficultyID 0, else the first)[^gen]: name; school
mask; cast time; range (the larger of `RangeMax_0` and `_1`); cooldown
(the larger of SpellCooldowns' two, else the charge category's
recharge, with the charges where more than one); the duration of the aura
it applies (-1 until cancelled); the cost line; flags: passive
(`Attributes_0` 0x40), its parent passive, channeled (`Attributes_1` 0x4,
but not an empowered spell), a class spell (R8), on the global cooldown
(`StartRecoveryTime` > 0), empowered (listed in `SpellEmpower`); the
parent; the description through `spelltip::collect` at base values[^tip]
(a stat-scaled value reads 0, as the talent viewer shows it, and so does
a stat an expression names; inside a `${…}` a duration is its seconds
and a product with a 0 in it is 0: Feral Frenzy's "5*0*6 sec/2 Bleed
damage" reads "0 Bleed damage"); the specs whose logs omit its press
(below); and, where the description is empty or under six words, the
encounter journal's text for the spell (or a parent two levels up), read as
gen-rubric reads an ability's (`journal::journal_texts`[^journal],
section by spell id, base rows, no difficulty's branch).

## The parent

A spell's parent is the spell whose description its own embeds
(`$@spelldesc<id>` leading it: the game's own tooltip naming its source),
else the spells whose effects trigger it (`SpellEffect.EffectTriggerSpell`,
every difficulty). The trigger column alone does not link Feral Frenzy's
damage spell (274838) to its button (274837): the button's effect is a
periodic dummy the server scripts. The walk goes up to the roots; where
one is active (named as the spell is, then nearest, then lowest id) the
parent is the first active spell on the way there, the press (Fire
Breath's burn is described as Fire Breath, which a hero talent's active
spell triggers: the parent is Fire Breath, which the log writes, not the
root); where every root is passive, a passive root, flagged.

## The omitted presses

An effect mark on a lane stands for a press the log never writes (a
guardian druid's Feral Frenzy). Whether the log omits a press is a fact
of the logs, not of the spell tables, so the generator counts it over the
stored replays it reads for their spells (`players_of`,
`omitted_presses`)[^gen]: for an effect whose parent is active, of its
own name and on a cooldown, and each spec of the players who put it on,
the fights where that player logged no cast of the parent's name against
those where they did. A spec with more fights without than with omits the
press; the record lists its spec ids (format 2). The class would not do:
ferals press Feral Frenzy and the log writes it, guardians and
restoration druids never; one frost death knight's Remorseless Winter in
the whole store is a cast. On the store of 2026-10-11 (8,863 players'
fights that put an aura on), two effects are listed: Feral Frenzy's
damage for guardian and restoration druids (6 and 25 fights to 0) and
Remorseless Winter's slow for frost death knights (42 to 1); Shield
Block (4 to 153), Judgment's debuff, Dancing Rune Weapon, Shield of the
Righteous, Greater Invisibility, Arcane Surge, Colossus Smash,
Bladestorm and Summon Infernal are presses their specs log, put on now and
then by a proc. The census joins the stamp, so new fights regenerate the
file.

## Checked on the data

- **What a press is.** The lane drops a cast success that is a passive's
  (gui-logic's `passive_success`: the spell passive, or its parent passive
  and the spell not pressed). "Pressed" (a class spell, on the GCD, a
  cooldown or a cost) is what tells Mortal Strike, Thunder Blast or an
  Execute a talent swaps in, all reached by a passive's chain, from
  Shadowy Apparition, Crusading Strikes, Soul Fragment or a hero talent's
  proc, which have none of the four: a census of the store's 1,055
  player instants by spell.
- **No trinket proc logs a success with a passive parent.** The trinket
  spells the store's players log are their uses (`ItemEffect.TriggerType`
  0), presses, and stay.
- **An empower's success is its hold's start.** Fire Breath's burn lands
  0.7 to 1.1 s after its success in a stored run: the log does not say
  how long it was held, so no channel is drawn for one.
- **Journal words are rare.** 135 NPC spells in the store have a terse
  description; nearly all are trash's, which the journal does not list.
  Three records carry the journal's words.

## Cost

- A fresh run (or a new build, or new fights in the store): about 7 s on
  the dev machine, 6 s of it inside the extractor decoding the spell
  tables (SpellEffect 629k rows, Spell and SpellName 414k).
- A rerun with the same build and spells: about 4 s, the store's replays
  read and the install opened, no table decoded (the file's stamp is the
  rules, the build, the spells asked for and the omitted presses'
  census, hashed). `--force` decodes anyway.
- The file: 3.9 MiB for 23,329 spells. Deterministic: two runs write the
  same bytes, and a file already holding them is left alone.

## Source

- Script: [`tools/gen-spell-tips.sh`](../../../tools/gen-spell-tips.sh)
- The file's layout: [replay-assets.md](../../replay-assets.md) §5 and
  `spelltipgen.rs`' header; the reader: `spell_tips`, which gui-logic
  gains with its spell tips[^reader].

[^script]: `tools/gen-spell-tips.sh`: the manual, in its header.
[^gen]: `spelltipgen.rs`: `store_spells`, `players_of`, `curated_spells`, `parents_of`, `root_of`, `omitted_presses`, `generate`, `encode`.
[^tip]: `spelltip.rs`: `collect`, `eval_expr`, `seconds_in`; values that scale with a stat read 0 at base values, as in the talent viewer.
[^journal]: `journal.rs`: `journal_texts` over the journal's sections and the `Spells` gen-rubric reads too (moved out of `rubricgen` when the replay's extractor split off, 2026-10-30).
[^reader]: `crates/gui-logic/src/spell_tips.rs`, which gui-logic gains with its spell tips: `SpellTips`, `SpellTip::facts_line`, `pressed`, `passive_success`, `omits_press_for`.
