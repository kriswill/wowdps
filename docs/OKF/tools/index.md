# tools

The game-data generators and census scripts under tools/ — regenerated once per game patch from the local install through the wowdps-extract crate.

## Concepts

* [census-absorb-spells](census-absorb-spells.md) - Census of the shields real combat logs ABSORB with — the evidence beside the discovered absorb-spell table (CONTRACT.md R20, tools/extract/src/absorbgen.rs).
* [census-placed-spells](census-placed-spells.md) - Count, per log, every line of the spells the placed-spell table names (players' casts and summons of the placing spells, the auras and heals that tell where they stand), so gen-placed-spells can refuse an entry whose spells no real log writes.
* [census-proc-spells](census-proc-spells.md) - Census of how real combat logs tie each curated proc to its driver — the log-side evidence behind the curated proc table (CONTRACT.md R26, tools/extract/src/procgen.rs), beside the install-side evidence the generator reads from the client's own spell text.
* [census-role-spells](census-role-spells.md) - Census of the buffs real combat logs apply to PLAYERS — the evidence behind the curated role-spell table (CONTRACT.md R18, tools/extract/src/rolegen.rs).
* [codeql](codeql.md) - Runs GitHub's CodeQL code scanning locally the way the repository's default setup runs it on every PR (rust and actions, the default suites, the same query pack versions) and fails only on an alert missing from the committed baseline, tools/codeql-baseline.txt, as GitHub fails a PR only on an alert not already open on main.
* [dev-unit](dev-unit.md) - Installs and drives the wowdps-dev systemd user unit — the dev machine's live daemon, run from this checkout's own build and restarted only when the build it runs (or the overlay GUI it spawns) changes.
* [fetch-gpui-docs](fetch-gpui-docs.md) - Mirrors the GPUI / GPUI Kit documentation (Kit's pages, API digests rendered from docs.rs rustdoc JSON, and the published crate sources) into a gitignored docs/gpui/ for offline, greppable reading while working on the GPUI GUI (crates/gui, built as gui-new).
* [gen-absorb-spells](gen-absorb-spells.md) - Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from the LOCAL game install.
* [gen-class-spells](gen-class-spells.md) - Regenerate crates/core/src/class_spells.rs from the LOCAL game install.
* [gen-icons](gen-icons.md) - Build ~/.local/share/wowdps/class-icons.bin, the per-machine cache of the game's class crests and spec icons that every player disc wears: the 64 px art upscaled 4× by Real-ESRGAN, cropped inside its bevel and circle-masked into 208 px tiles, which the icon levels gui-logic's reader gains halve to the size each disc is drawn at.
* [gen-item-spells](gen-item-spells.md) - Regenerate crates/core/src/item_spells.rs from the LOCAL game install.
* [gen-keystone-timers](gen-keystone-timers.md) - Regenerate crates/core/src/keystone_timers.rs from the LOCAL game install.
* [gen-open-world-maps](gen-open-world-maps.md) - Regenerate crates/core/src/open_world_maps.rs from the LOCAL game install.
* [gen-placed-spells](gen-placed-spells.md) - Regenerate proto's table of what players place in the room (totems, gateways, rings, zones, statues, sigils, traps): the curated list proven by the client's spell names, its reach and duration read from the client's spell tables, and each entry backed by a real-log census, so the cut keeps their lines and the replay draws them.
* [gen-proc-spells](gen-proc-spells.md) - Regenerate crates/core/src/proc_spells.rs (+ proc_spells.expected.md) from the LOCAL game install.
* [gen-role-spells](gen-role-spells.md) - Regenerate crates/core/src/role_spells.rs (+ role_spells.expected.md) from the LOCAL game install.
* [gen-spell-icons](gen-spell-icons.md) - Regenerate the spell-icon cache from the LOCAL game install.
* [gen-spell-tips](gen-spell-tips.md) - Write the replay's spell tips (spell-tips.bin beside spell-icons.bin): for every spell the stored replays and the curated tables name, its tooltip's words (read at base values) and the facts the HUD's cast lanes ask (passive, channeled, empowered, on the GCD, its parent press, the specs whose logs omit that press: a census of the stored replays), from the local install; a per-machine cache, never committed.
* [gen-talent-art](gen-talent-art.md) - Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI's own artwork cropped from the client's texture atlases — per-spec pane background paintings, each hero tree's round medallion, and the golden medallion ring — the per-machine cache the GUI's talent viewer paints its panes with.
* [gen-talent-trees](gen-talent-trees.md) - Regenerate the talent-tree dataset from the LOCAL game install.
* [lib](lib.md) - What the tools/*.sh scripts share: the extractor built once, the schemas and keys a command needs, and the upscaler.
* [overlay-replay](overlay-replay.md) - Replays a real combat log, at speed, into an isolated daemon while an overlay follows it on a headless Hyprland output — the stand-in for a raid night without the game, and the measure of an overlay's cost under live load.
* [seed-creatures](seed-creatures.md) - Seed the history store's creatures.tsv from Wowhead's NPC data (the season's zone listings and a tooltip for each other NPC the stored replays hold), under the addon's sightings, so the replay rings a rare silver before the addon has seen it.
* [skills-update](skills-update.md) - Refreshes the vendored CodeRabbit skills (.claude/skills/autofix, and upstream's code-review installed as code-rabbit-review, from coderabbitai/skills) and re-applies this repo's patches over them, so a local fix to an upstream skill survives every refresh.
* [wowdps-db2](wowdps-db2.md) - The game's client tables as SQL: `wowdps-db2 Table... [-c SQL]` decodes each named table from the local install once per game build into ~/.cache/wowdps/db2/<build>/ (CSV and Parquet), then opens DuckDB in memory with a view per cached table — how this repo looks things up in the journal, creature, spell and map tables.
