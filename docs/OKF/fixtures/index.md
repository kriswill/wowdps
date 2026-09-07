# fixtures

The committed synthetic combat logs under crates/core/fixtures/ — each exercises named rulings, carries hand-computed goldens recomputed independently by check.awk, and is gated by a test.

## Concepts

* [arena.txt](arena.md) - Arena matches as named win/loss Encounter segments — arenas zone in at difficulty 0, so R10 never sees them.
* [corrupt.txt](corrupt.md) - The negative control — a deliberately damaged log that the parser-independent check must FAIL on.
* [instance.txt](instance.md) - A completed keystone with suspend/resume on zoning and city combat between visits — the R10 visit machinery.
* [relog.txt](relog.md) - Two mid-log COMBAT_LOG_VERSION boundaries — the pet-owner map must reset at each, and the open visit suspends.
* [sample.txt](sample.md) - The canonical synthetic advanced-format log — two encounters and trash inside one raid visit, three players and a pet, every modeled event type — with hand-computed golden totals.
* [shields.txt](shields.md) - A Discipline Priest's Power Word: Shield ledger in every state — partial waste, running-total refreshes up and down, re-apply while open, an over-absorb, a pre-pull shield seen only by its absorb, one open at the kill — plus Ice Barrier, Blood Shield, an excluded stagger and a non-shield trailer.
* [spans.txt](spans.md) - Aura spans with caster and target — Shield Block refreshed with no apply and open at the kill, overlapping Shield Wall and Pain Suppression for the union, externals, Time Warp, support buffs, a trinket proc, and a pre-pull aura in the dead zone.
* [support.txt](support.md) - An Augmentation Evoker buffing a Mage, a Warrior and a pet, plus a Holy Priest with shields, overheal, a self-heal and an NPC-sourced heal — support attribution and the healing split.
* [taken.txt](taken.md) - Three players, every miss kind, staggered hits, a pet hit before its summon and a guardian that staggers itself — the destination-side Taken view, per-player mitigation and the R22 self-harm split.
