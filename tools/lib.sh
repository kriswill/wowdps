#!/usr/bin/env bash
# What the tools/*.sh scripts share: the extractor built once, the schemas
# and keys a command needs, and the upscaler. A script sets `root` to the
# checkout and sources it: `. "$root/tools/lib.sh"`.
#
#   build_extract [ARGS]           cargo-build wowdps-extract (release) from
#                                  the checkout and set $extract to it (under
#                                  $CARGO_TARGET_DIR where that is set, as a
#                                  worktree's own target must be): the
#                                  generators of wowdps's own tables and caches
#   fetch_dbds COMMAND DIR         the WoWDBDefs schema of every table
#                                  COMMAND reads, into DIR: the list is the
#                                  extractor's own (`$extract tables
#                                  COMMAND`), so no script keeps one
#   fetch_keys FILE                the wowdev TACTKeys list into FILE
#   find_upscaler                  print realesrgan-ncnn-vulkan's path: from
#                                  $PATH, else built from nixpkgs; nothing
#                                  when there is neither
#   upscale_4x UPSCALER IN OUT     every picture in IN, 4x by Real-ESRGAN's
#                                  illustration model, into OUT
#
# The network is used for the schemas and keys alone, unless they are
# given: $WOWDPS_DBD_DIR (a directory of <Table>.dbd files) and
# $WOWDPS_TACT_KEYS (a TACTKeys file) are copied instead, so a regenerate
# can run offline, or against schemas pinned by hand.

: "${root:?tools/lib.sh: set root to the checkout before sourcing it}"

# Its arguments are cargo's; most callers pass none.
# shellcheck disable=SC2120
build_extract() {
    cargo build -q --release --manifest-path "$root/Cargo.toml" -p wowdps-extract "$@"
    extract="${CARGO_TARGET_DIR:-$root/target}/release/wowdps-extract"
}

fetch_dbds() {
    local cmd=$1 dir=$2 tables t
    tables=$("$extract" tables "$cmd") || return 1
    for t in $tables; do
        if [ -n "${WOWDPS_DBD_DIR:-}" ]; then
            cp "$WOWDPS_DBD_DIR/$t.dbd" "$dir/$t.dbd" ||
                { echo "no $t.dbd in $WOWDPS_DBD_DIR" >&2; return 1; }
        else
            curl -sfL "https://raw.githubusercontent.com/wowdev/WoWDBDefs/master/definitions/$t.dbd" \
                -o "$dir/$t.dbd" || { echo "failed to fetch $t.dbd" >&2; return 1; }
        fi
    done
}

fetch_keys() {
    if [ -n "${WOWDPS_TACT_KEYS:-}" ]; then
        cp "$WOWDPS_TACT_KEYS" "$1" || { echo "no TACT keys at $WOWDPS_TACT_KEYS" >&2; return 1; }
    else
        curl -sfL "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt" \
            -o "$1" || { echo "failed to fetch TACTKeys" >&2; return 1; }
    fi
}

find_upscaler() {
    local esrgan store
    esrgan=$(command -v realesrgan-ncnn-vulkan || true)
    if [ -z "$esrgan" ] && command -v nix >/dev/null; then
        echo "building realesrgan-ncnn-vulkan from nixpkgs..." >&2
        store=$(nix build --no-link --print-out-paths nixpkgs#realesrgan-ncnn-vulkan 2>/dev/null || true)
        [ -n "$store" ] && esrgan="$store/bin/realesrgan-ncnn-vulkan"
    fi
    if [ -n "$esrgan" ] && [ -x "$esrgan" ]; then
        echo "$esrgan"
    fi
}

upscale_4x() {
    "$1" -i "$2" -o "$3" -n realesrgan-x4plus-anime -s 4 -f png
}
