#!/usr/bin/env bash
# Refresh the vendored CodeRabbit skills, then lay this repo's patches over
# them.
#
# .claude/skills/{autofix,code-rabbit-review} are coderabbitai/skills as
# `bunx skills add` copied them (skills-lock.json records the source and
# hash, under upstream's names) — `code-review` installed as
# code-rabbit-review, so it never shadows Claude Code's own /code-review.
# Local fixes never live in those files alone: each is a patch in
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
# Upstream's names, as `skills add` installs them and the patches address
# them; `as` maps one this repo installs under another name.
skills=(autofix code-review)
declare -A as=([code-review]=code-rabbit-review)
named() { echo "${as[$1]:-$1}"; }

# Its own repository, so git apply resolves the patches' paths from it.
stage=$(mktemp -d)
stopped() {
  if ((${backups:-0})); then
    echo "skills-update: stopped mid-swap; the skills it replaced are in" \
      "$swap (scratch project: $stage)" >&2
  else
    echo "skills-update: stopped; the checkout is untouched (scratch" \
      "project: $stage)" >&2
  fi
}
trap stopped ERR
git init -q "$stage"
cp skills-lock.json "$stage/"
(cd "$stage" && bunx skills add coderabbitai/skills --skill "${skills[@]}" \
  -a claude-code --copy -y)

shopt -s nullglob
for patch in "$root"/.claude/skills/patches/*.patch; do
  git -C "$stage" apply "$patch"
  echo "applied ${patch#"$root"/}"
done

# Copy the results beside their targets, then swap them in by rename, so a
# failed copy leaves the checkout as it was. The swap directory sits on the
# checkout's filesystem but outside .claude/skills, where Claude Code would
# load a half-copied skill. Once an old skill has been moved into it, it
# holds the only copy, so a failure from then on keeps it to restore from
# by hand; nothing rolls back on its own.
swap=$(mktemp -d .claude/skills-update.XXXXXX)
backups=0
trap '((backups)) || rm -rf "$swap"' EXIT
for skill in "${skills[@]}"; do
  cp -R "$stage/.claude/skills/$skill" "$swap/$(named "$skill")"
done
cp "$stage/skills-lock.json" "$swap/skills-lock.json"
for skill in "${skills[@]}"; do
  to=$(named "$skill")
  # Its local name, and upstream's — what an install before the rename
  # left — both moved aside.
  for old in "$to" "$skill"; do
    if [[ -e ".claude/skills/$old" ]]; then
      mv ".claude/skills/$old" "$swap/$old.old"
      backups=1
    fi
  done
  mv "$swap/$to" ".claude/skills/$to"
done
mv "$swap/skills-lock.json" skills-lock.json
rm -rf "$swap" "$stage"
