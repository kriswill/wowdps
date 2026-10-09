# fixtures

The committed synthetic combat logs under crates/core/fixtures/ — each exercises named rulings, carries hand-computed goldens recomputed independently by check.awk, and is gated by a test.

## Concepts

* [arena.txt](arena.md) - Arena matches as named win/loss Encounter segments — arenas zone in at difficulty 0, so R10 never sees them.
* [corrupt.txt](corrupt.md) - The negative control — a deliberately damaged log that the parser-independent check must FAIL on.
* [instance.txt](instance.md) - A completed keystone with suspend/resume on zoning and city combat between visits — the R10 visit machinery.
* [relog.txt](relog.md) - Two mid-log COMBAT_LOG_VERSION boundaries — the pet-owner map must reset at each, and the open visit suspends.
* [replay.txt](replay.md) - The replay cut's fixture (R29, v45) — two boss pulls with world markers placed and replaced before the first, posts on two floors, every event kind a replay draws (a ranged hit, a miss, a hit from no source, a failed cast, a Feign Death, a creature down unconscious, a rez), a warlock known by R8, and what players place: a gateway, a totem's summon, touches, a create, a totem destroyed.
* [sample.txt](sample.md) - The canonical synthetic advanced-format log — two encounters and trash inside one raid visit, three players and a pet, every modeled event type — with hand-computed golden totals.
* [shields.txt](shields.md) - A Discipline Priest's Power Word: Shield ledger in every state — partial waste, running-total refreshes up and down, re-apply while open, an over-absorb, a pre-pull shield seen only by its absorb, one open at the kill — plus Ice Barrier, Blood Shield, an excluded stagger and a non-shield trailer.
* [spans.txt](spans.md) - Aura spans with caster and target — Shield Block refreshed with no apply and open at the kill, overlapping Shield Wall and Pain Suppression for the union, externals, Time Warp, support buffs, a trinket proc, and a pre-pull aura in the dead zone.
* [support.txt](support.md) - An Augmentation Evoker buffing a Mage, a Warrior and a pet, plus a Holy Priest with shields, overheal, a self-heal and an NPC-sourced heal (v44: two of the Priest's heals partly eaten by a heal-absorb) — support attribution and the healing split.
* [taken.txt](taken.md) - Three players, every miss kind, staggered hits, a pet hit before its summon and a guardian that staggers itself — the destination-side Taken view, per-player mitigation and the R22 self-harm split — plus a trash pull built for the combat clock and friendly fire.
* [tree.txt](tree.md) - The ability tree's fixture — a Destruction Warlock's Sayaad and Infernal summoned mid-pull, Wither's hit and tick under two ids, two trinkets' procs, and every cast the passive gate turns away; a Priest's one-id Shadow Word: Pain hit and ticks and Renew's instant heal and ticks; an Evoker's empowered Fire Breath and Dream Breath (v44).
