# .github

CI and dependency automation. Rationale:
`docs/OKF/decisions/devenv-is-the-reference-shell.md`.

## Rules

- **Checks run on PRs.** `workflows/ci.yml` runs on pull requests only. On
  main only `workflows/cache-warm.yml` runs (and `workflows/audit.yml` on a
  lockfile change and weekly).
- **Main warms, PRs read.** `cache-warm.yml` compiles exactly what PR CI
  compiles, gates nothing and runs no test; it is the only writer of both
  caches. PR runs restore with `save-if: false` and
  `SCCACHE_GHA_RW_MODE=READ_ONLY`. Keep the two jobs' setup and the
  rust-cache `shared-key` identical.
- **Every check runs in devenv,** and the environment contract (`devenv
  test`, then `nix develop -c wowdps-dev-contract`) runs before any cargo
  step.
- **Every action is pinned to a commit SHA,** its version in a trailing
  comment for Dependabot; no checkout persists its credentials.
- **`[skip ci]`** anywhere in a head commit's message skips PR CI.
