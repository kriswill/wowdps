# Commit message convention — Conventional Commits v1.0.0

```
<type>[optional scope][!]: <description>

[optional body]

[optional footer(s)]
```

Spec, condensed:

- Subject is `type` + optional `(scope)` + optional `!` + `: ` + description. Type and scope are single nouns; description immediately follows the colon-space.
- `feat` = new feature, `fix` = bug fix. Other types allowed: `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`, `revert`.
- Body: free-form prose, one blank line after the description. Explains the why.
- Footers: one blank line after the body; `Token: value` (or `Token #value`), token uses `-` instead of spaces (`Reviewed-by`), except `BREAKING CHANGE`.
- Breaking change: `!` before the colon and/or a `BREAKING CHANGE: <description>` footer (uppercase, `BREAKING-CHANGE` synonymous). Either alone is valid; we use both.
- Types/scopes are case-insensitive per spec (except `BREAKING CHANGE`); we write them lowercase.
- `revert` commits: state what's reverted in the body/footer (e.g. `Refs: <hashes>`).

Repo specifics:

- Scopes: `model`, `core`, `proto`, `daemon`, `tui`, `gui`, `mcp`, `extract`, `fixtures`, `nix`, `ci`; omit for cross-cutting changes.
- Breaking here means: changes CONTRACT.md, bumps `PROTO_VERSION`, or alters fixture goldens.
- Description imperative, lowercase, no trailing period.

Examples: `feat(gui): render hero-tree medallions in the talent viewer` · `fix(core)!: count SPELL_ABSORBED toward the absorber (R3)` + `BREAKING CHANGE:` footer.
