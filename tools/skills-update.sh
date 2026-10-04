#!/usr/bin/env bash
# Refresh the vendored CodeRabbit skills, then lay this repo's patches over
# them.
#
# .claude/skills/{autofix,code-review} are coderabbitai/skills as
# `bunx skills add` copied them (skills-lock.json records the source and
# hash). Local fixes never live in those files alone: each is a patch in
# .claude/skills/patches/, so a refresh cannot lose one. The script
# installs the upstream files exactly as first added (Claude Code only,
# copied) into a scratch project, applies every patch there in name order,
# and only then replaces the checkout's skills and lock. A failed install
# or a patch that no longer applies leaves the checkout untouched and the
# scratch project in place: upstream moved under the patch, so rebase it
# against the new files there, or delete it once upstream carries the fix.
#
# Not `bunx skills update`: it reinstalls through `skills add -y` with no
# agent and no --copy, which under -y means every agent installed on the
# machine, symlinked from .agents/skills, and git apply will not write
# through a symlink.
#
# usage: tools/skills-update.sh
# Network: github.com (the skills repo) and the npm registry (bunx skills).
set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD
skills=(autofix code-review)

# Its own repository, so git apply resolves the patches' paths from it.
stage=$(mktemp -d)
trap 'echo "skills-update: stopped; the checkout is untouched (scratch project: $stage)" >&2' ERR
git init -q "$stage"
cp skills-lock.json "$stage/"
(cd "$stage" && bunx skills add coderabbitai/skills --skill "${skills[@]}" \
  -a claude-code --copy -y)

shopt -s nullglob
for patch in "$root"/.claude/skills/patches/*.patch; do
  git -C "$stage" apply "$patch"
  echo "applied ${patch#"$root"/}"
done

for skill in "${skills[@]}"; do
  rm -rf ".claude/skills/$skill"
  cp -R "$stage/.claude/skills/$skill" .claude/skills/
done
cp "$stage/skills-lock.json" skills-lock.json
rm -rf "$stage"
