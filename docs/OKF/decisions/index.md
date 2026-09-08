# decisions

Decision records — why the repository is the way it is.

## Concepts

* [Keep the knowledge bundle under docs/OKF](okf-bundle-under-docs.md) - The OKF bundle lives at docs/OKF, beside the specs and plans it links to, and its scaffold passes derive Crate, Ruling and Tool docs from the sources so the graph never drifts from CONTRACT.md or the workspace.
* [Keep Self-Harm Off The Damage Meter](self-harm-off-the-damage-meter.md) - Damage an actor deals to itself (own pets folded) is tallied as `self_harm` instead of landing on its Damage row, because a Brewmaster's Stagger ticks were 21% of his "damage" against every in-game meter's 0%.
* [Home Derives Client-Side From Fights](home-derives-from-fights.md) - The GUI's Home dashboard is derived in the client from `HistoryQuery::Fights` answers rather than from a new `Summary` query, and its list grows by scrolling rather than by a pager.
* [One Chrome Accent, Split By Luminance](one-accent-from-the-class-color.md) - The GUI's accent is derived from the class color, and the light/dark split that picks its ink is WCAG's crossover luminance.
