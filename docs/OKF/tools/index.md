# tools

The game-data generators and census scripts under tools/ — regenerated once per game patch from the local install through the wowdps-extract crate.

## Concepts

* [census-absorb-spells](census-absorb-spells.md) - Census of the shields real combat logs ABSORB with — the evidence beside the discovered absorb-spell table (CONTRACT.md R20, tools/extract/src/absorbgen.rs).
* [census-role-spells](census-role-spells.md) - Census of the buffs real combat logs apply to PLAYERS — the evidence behind the curated role-spell table (CONTRACT.md R18, tools/extract/src/rolegen.rs).
* [gen-absorb-spells](gen-absorb-spells.md) - Regenerate crates/core/src/absorb_spells.rs (+ absorb_spells.expected.md) from the LOCAL game install.
* [gen-class-spells](gen-class-spells.md) - Regenerate crates/core/src/class_spells.rs from the LOCAL game install.
* [gen-icons](gen-icons.md) - Regenerate the class/spec icon cache from the LOCAL game install.
* [gen-item-spells](gen-item-spells.md) - Regenerate crates/core/src/item_spells.rs from the LOCAL game install.
* [gen-keystone-timers](gen-keystone-timers.md) - Regenerate crates/core/src/keystone_timers.rs from the LOCAL game install.
* [gen-role-spells](gen-role-spells.md) - Regenerate crates/core/src/role_spells.rs (+ role_spells.expected.md) from the LOCAL game install.
* [gen-spell-icons](gen-spell-icons.md) - Regenerate the spell-icon cache from the LOCAL game install.
* [gen-talent-art](gen-talent-art.md) - Regenerate ~/.local/share/wowdps/talent-art.bin: the talent UI's own artwork cropped from the client's texture atlases — per-spec pane background paintings, each hero tree's round medallion, and the golden medallion ring.
* [gen-talent-trees](gen-talent-trees.md) - Regenerate the talent-tree dataset from the LOCAL game install.
