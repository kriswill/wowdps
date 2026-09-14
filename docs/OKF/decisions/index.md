# decisions

Decision records — why the repository is the way it is.

## Concepts

* [Guilds Come From An Addon, Joined At Read](guilds-come-from-an-addon.md) - The combat log never names a guild, so wowdps ships a tiny in-game addon that writes every raid member's guild to its SavedVariables; the daemon files those as affiliations and joins them onto cards when it answers, never storing a guild on a card, and only ever updates an addon the user installed.
* [Home Derives Client-Side From Fights](home-derives-from-fights.md) - The GUI's Home dashboard is derived in the client from `HistoryQuery::Fights` answers rather than from a new `Summary` query, and its list grows by scrolling rather than by a pager — with the daemon giving reads their own quota so a dashboard can never cost the user a stored fight.
* [Keep the knowledge bundle under docs/OKF](okf-bundle-under-docs.md) - The OKF bundle lives at docs/OKF, beside the specs and plans it links to, and its scaffold passes derive Crate, Ruling and Tool docs from the sources so the graph never drifts from CONTRACT.md or the workspace.
* [One Chrome Accent, Split By Luminance](one-accent-from-the-class-color.md) - The GUI's accent is derived from the class color, and the light/dark split that picks its ink is WCAG's crossover luminance (0.179) rather than a tuned constant, because the split exists to keep text legible on thirteen fixed colors nobody gets to choose.
* [One Table For Every Row List, And History As A Window-Local Browser](one-table-for-every-row-list.md) - GUI slice 2: every list of `Row`s the window draws goes through one table primitive whose headings, cells and total row are generated from the same column list; the History screen is window-local like Home and renders a stored fight on that same table; the `?` sheet is keyed on the surface; the R21 matrix derives level 0 per debuff.
* [Keep Self-Harm Off The Damage Meter](self-harm-off-the-damage-meter.md) - Damage an actor deals to itself (own pets folded) is tallied as `self_harm` instead of landing on its Damage row, because a Brewmaster's Stagger ticks were 21% of his "damage" against every in-game meter's 0%.
