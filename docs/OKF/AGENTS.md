# docs/OKF

The knowledge bundle: an Open Knowledge Format v0.2 graph over the crates,
CONTRACT.md's rulings, the fixtures, the tools, and the decisions, patterns
and playbooks behind them. Conventions: `okf-profile.md`. The maintenance
loop is the `knowledge-bundle` skill.

## Rules

- **Read `okf-profile.md` before authoring.** Frontmatter needs `type`;
  `title`, `description` and `generated` are expected. Body headings are
  H2. Links are file-relative and must resolve.
- **Scaffolded docs** (crates, rulings, tools) come from the sources via
  `okf scaffold`, which never overwrites; enrich a stub by hand. Never
  `scaffold --force` an enriched doc.
- **`index.md` listings are generated** (`okf index`); only the blurb above
  the first heading is hand-written.
- **Log every change** in the owning directory's `log.md`, newest first
  under `## YYYY-MM-DD`, led by `**Creation**`, `**Update**` or
  `**Deprecation**`. Deprecate a superseded entry; never delete it.
- **CONTRACT.md stays the binding text.** A ruling doc links to it and never
  competes with it.
- **Validate before committing:** `nix develop -c okf validate` must exit 0.
