---
type: Tool
title: skills-update
description: Refreshes the vendored CodeRabbit skills (.claude/skills/autofix, and upstream's code-review installed as code-rabbit-review, from coderabbitai/skills) and re-applies this repo's patches over them, so a local fix to an upstream skill survives every refresh.
resource: tools/skills-update.sh
tags: [tool, dev]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-03T18:21:54-07:00 }
---

`tools/skills-update.sh` reinstalls the two skills exactly as they were
first added, into a scratch project, applies every
`.claude/skills/patches/*.patch` there in name order, and only then
replaces the checkout's skill folders and `skills-lock.json`. The install
is `bunx skills add coderabbitai/skills --skill autofix code-review -a
claude-code --copy -y`: Claude Code only, copied rather than symlinked.
`skills-lock.json` keeps upstream's hash, so it never records a patch.
A skill can be installed under a local name (`as` in the script):
upstream's `code-review` lands as `.claude/skills/code-rabbit-review`,
so it sits beside Claude Code's own `/code-review` (the code-review
plugin) instead of shadowing it; the patches still address upstream's
paths, and the lock keeps upstream's names.
Needs the network (github.com and the npm registry).

## Source

- Script: [`tools/skills-update.sh`](../../../tools/skills-update.sh)
- Patches: [`.claude/skills/patches/`](../../../.claude/skills/patches/)
- Lock: [`skills-lock.json`](../../../skills-lock.json)

## Seams

- **Patches, never bare edits.** A change made only to a vendored skill
  file is lost on the next refresh, because a copy install wipes each
  skill's folder first. A local fix lives as a patch, with a note above its
  first diff header (git apply skips it) saying why it exists and when to
  drop it. The first one, `autofix-push-gate.patch`, stops autofix from
  posting its PR success comment when the fix commit was never pushed.
  `code-rabbit-review-name.patch` renames the CodeRabbit review skill in
  its frontmatter and drops upstream's claim to every review request.
- **Not `bunx skills update`.** For each project skill it runs
  `skills add -y` with no agent and no `--copy`. Under `-y` that means every
  agent installed on the machine, symlinked from `.agents/skills`, and git
  apply will not write through a symlink.
- **All or nothing.** The checkout changes only after the install and
  every patch have succeeded in the scratch project, so Claude Code never
  loads a refreshed skill without its patches. Even then the new folders
  and lock are first copied beside their targets (a swap directory under
  `.claude/`, outside `skills/`) and swapped in by rename, so a failed copy
  changes nothing either. A rename failing mid-swap (a full disk, a
  permission changed under it) is not rolled back: the swap directory is
  kept with the skills it replaced, named on stderr, to restore by hand. When a patch no longer
  applies, the script stops with git's reason and keeps the scratch
  project, named on stderr. Upstream moved under the patch: rebase it
  against the new files there, or delete it once upstream carries the fix.
