#!/usr/bin/env bash
# Mirror the GPUI / GPUI Kit documentation into docs/gpui/ for offline,
# greppable reading while building crates/gui-new.
#
# Three parts, all written under docs/gpui/ and ignored by git (only
# docs/gpui/README.md is committed — it is the guide to what lands here):
#
#   kit/   every English page of https://gpui-kit.com (docs, components,
#          gpui-base, shell), fetched as the site's own Markdown through the
#          index it publishes for LLMs (llms.txt); kit/INDEX.md is that index.
#   api/   one digest per crate, rendered by tools/rustdoc-digest.jq from the
#          rustdoc JSON docs.rs builds — the same items as the docs.rs pages,
#          signatures + doc comments, one file per source file, INDEX.md
#          listing every item path.
#   src/   the published crate sources (static.crates.io), for what the API
#          docs cannot answer: the Wayland backend, examples, tests.
#
# The gpui snapshot GPUI Kit builds on is NOT crates.io's `gpui` (0.2.2, last
# published 2025-10) but `gpui-pre`, a snapshot of Zed's GPUI that Kit pins
# exactly; the script reads that pin from Kit's own dependency list so the
# api/ and src/ copies always match the Kit version mirrored. `gpui` 0.2.2 is
# digested too, for reference only.
#
# usage: tools/fetch-gpui-docs.sh [kit-version]   (default: newest gpui-kit)
# Network: gpui-kit.com, crates.io, static.crates.io, docs.rs. Needs curl,
# jq, zstd, tar. Re-running replaces the mirror.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/docs/gpui"
ua='wowdps-fetch-gpui-docs (github.com/kriswill/wowdps)'
api_crates=(gpui-pre gpui-pre-platform gpui-kit gpui-base gpui-component)
src_crates=(gpui-pre gpui-pre-linux gpui-pre-wgpu gpui-pre-platform gpui-pre-macros gpui-kit gpui-base gpui-component)
legacy_gpui=0.2.2   # docs.rs/gpui/latest — crates.io's own gpui, digested for reference

cratesio() { curl -sSfL -A "$ua" "https://crates.io/api/v1/crates/$1"; }

kit="${1:-$(cratesio gpui-kit | jq -r .crate.max_version)}"
pre=$(cratesio "gpui-kit/$kit/dependencies" \
    | jq -r '.dependencies[] | select(.crate_id == "gpui-pre" and .kind == "normal") | .req' \
    | tr -d '=^ ')
[ -n "$pre" ] || { echo "gpui-kit $kit declares no gpui-pre dependency" >&2; exit 1; }
echo "gpui-kit $kit pins gpui-pre $pre"

# Kit's crates move in lockstep with gpui-kit; the gpui-pre-* family with gpui-pre.
version_of() { case "$1" in gpui-pre*) echo "$pre" ;; gpui) echo "$legacy_gpui" ;; *) echo "$kit" ;; esac; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

rm -rf "$out/kit" "$out/api" "$out/src"
mkdir -p "$out/kit" "$out/api" "$out/src"

# ---- kit: the site's Markdown pages -----------------------------------
curl -sSfL -A "$ua" https://gpui-kit.com/llms.txt -o "$work/llms.txt"
grep -v 'zh-CN' "$work/llms.txt" > "$out/kit/INDEX.md"
grep -oE '\]\(/[^)]+\.md\)' "$out/kit/INDEX.md" | sed -E 's/^\]\(//; s/\)$//' | sort -u > "$work/pages"
echo "kit: $(wc -l < "$work/pages") pages"
while read -r page; do
    mkdir -p "$out/kit$(dirname "$page")"
    printf 'url = "https://gpui-kit.com%s"\noutput = "%s"\n' "$page" "$out/kit$page"
done < "$work/pages" > "$work/curl.cfg"
curl -sSfL -A "$ua" --parallel --parallel-max 4 --retry 3 -K "$work/curl.cfg"

# ---- api: rustdoc JSON -> Markdown digests ----------------------------
digest() { # crate version
    local c=$1 v=$2 dir="$out/api/$1-$2"
    curl -sSfL -A "$ua" "https://docs.rs/crate/$c/$v/json" -o "$work/$c.json.zst"
    zstd -dqf "$work/$c.json.zst" -o "$work/$c.json"
    jq -r -f "$root/tools/rustdoc-digest.jq" "$work/$c.json" > "$work/$c.digest"
    mkdir -p "$dir"
    awk -v dir="$dir" -v crate="$c" -v ver="$v" '
        /^@@@FILE@@@ / { file = substr($0, 12) ".md"; out = dir "/" file
                         d = out; sub(/\/[^\/]*$/, "", d); system("mkdir -p \"" d "\"")
                         files[++n] = file; next }
        /^@@@INDEX@@@ / { split(substr($0, 13), kv, "\t")
                          idx[file] = idx[file] "- `" kv[2] "` (" kv[1] ")\n"; next }
        { print > out }
        END {
            ix = dir "/INDEX.md"
            print "# " crate " " ver " — API digest\n" > ix
            print "From docs.rs rustdoc JSON (https://docs.rs/" crate "/" ver "), rendered by" > ix
            print "tools/rustdoc-digest.jq. One file per source file; every item is listed" > ix
            print "under the file that holds it, by its defining path (GPUI re-exports most" > ix
            print "of them at the crate root, so `gpui::elements::div::Div` is `gpui::Div`).\n" > ix
            for (i = 1; i <= n; i++) printf "## [%s](%s)\n\n%s\n", files[i], files[i], idx[files[i]] > ix
        }' "$work/$c.digest"
    echo "api: $c $v — $(grep -c '^@@@INDEX@@@' "$work/$c.digest") items"
}
for c in "${api_crates[@]}"; do digest "$c" "$(version_of "$c")"; done
digest gpui "$legacy_gpui"

# ---- src: published crate sources -------------------------------------
for c in "${src_crates[@]}"; do
    v=$(version_of "$c")
    curl -sSfL -A "$ua" "https://static.crates.io/crates/$c/$c-$v.crate" | tar xz -C "$out/src"
    echo "src: $c $v"
done

printf 'gpui-kit %s\ngpui-pre %s\ngpui %s\nfetched %s\n' "$kit" "$pre" "$legacy_gpui" "$(date -u +%FT%TZ)" > "$out/VERSIONS"
echo "done: $out"
