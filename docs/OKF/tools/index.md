# tools

The game-data generators and census scripts under tools/ — regenerated once per game patch from the local install through the wowdps-extract crate.

## Concepts

* [census-absorb-spells](census-absorb-spells.md) - Census of the shields real combat logs ABSORB with — the evidence beside the discovered absorb-spell table (CONTRACT.md R20, tools/extract/src/absorbgen.rs).
* [census-proc-spells](census-proc-spells.md) - Census of how real combat logs tie each curated proc to its driver — the log-side evidence behind the curated proc table (CONTRACT.md R26, tools/extract/src/procgen.rs), beside the install-side evidence the generator reads from the client's own spell text.
* [census-role-spells](census-role-spells.md) - Census of the buffs real combat logs apply to PLAYERS — the evidence behind the curated role-spell table (CONTRACT.md R18, tools/extract/src/rolegen.rs).
* [dev-unit](dev-unit.md) - Installs and drives the wowdps-dev systemd user unit — the dev machine's live daemon, run from this checkout's own build and restarted only when the build it runs (or the overlay GUI it spawns) changes.
* [fetch-gpui-docs](fetch-gpui-docs.md) - Mirrors the GPUI / GPUI Kit documentation (Kit's pages, API digests rendered from docs.rs rustdoc JSON, and the published crate sources) into a gitignored docs/gpui/ for offline, greppable reading while working on the GPUI GUI (crates/gui, built as gui-new).
* [gen-absorb-spells](gen-absorb-spells.md) - Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from the LOCAL game install.
* [gen-class-spells](gen-class-spells.md) - Regenerate crates/core/src/class_spells.rs from the LOCAL game install.
* [gen-icons](gen-icons.md) - Regenerate the class/spec icon cache from the LOCAL game install.
* [gen-item-spells](gen-item-spells.md) - Regenerate crates/core/src/item_spells.rs from the LOCAL game install.
* [gen-keystone-timers](gen-keystone-timers.md) - Regenerate crates/core/src/keystone_timers.rs from the LOCAL game install.
* [gen-proc-spells](gen-proc-spells.md) - Regenerate crates/core/src/proc_spells.rs (+ proc_spells.expected.md) from the LOCAL game install.
* [gen-role-spells](gen-role-spells.md) - Regenerate crates/core/src/role_spells.rs (+ role_spells.expected.md) from the LOCAL game install.
* [gen-spell-icons](gen-spell-icons.md) - Regenerate the spell-icon cache from the LOCAL game install.
* [gen-talent-art](gen-talent-art.md) - Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI's own artwork cropped from the client's texture atlases — per-spec pane background paintings, each hero tree's round medallion, and the golden medallion ring.
* [gen-talent-trees](gen-talent-trees.md) - Regenerate the talent-tree dataset from the LOCAL game install.
* [overlay-replay](overlay-replay.md) - Replays a real combat log, at speed, into an isolated daemon while an overlay follows it on a headless Hyprland output — the stand-in for a raid night without the game, and the measure of an overlay's cost under live load.
