---
type: Tool
title: fetch-gpui-docs
description: 'Mirrors the GPUI / GPUI Kit documentation (Kit''s pages, API digests rendered from docs.rs rustdoc JSON, and the published crate sources) into a gitignored docs/gpui/ for offline, greppable reading while working on the GPUI GUI (crates/gui, built as gui-new).'
resource: tools/fetch-gpui-docs.sh
tags: [tool, docs, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T20:40:00-07:00 }
sources:
  - id: guide
    resource: ../../gpui/README.md
    title: docs/gpui/README.md — the guide to the mirror and what it confirmed
---

Mirror the GPUI / GPUI Kit documentation into `docs/gpui/` (gitignored; only
the guide[^guide] and `.gitignore` are committed): `kit/` (every English page
of gpui-kit.com as the site's own Markdown, through its `llms.txt` index),
`api/` (one digest per crate: signatures and doc comments, one file per
source file, an `INDEX.md` of every item path) and `src/` (the published
crate sources). `usage: tools/fetch-gpui-docs.sh [kit-version]` (default: the
newest gpui-kit). Needs curl, jq, zstd and tar, and the network
(gpui-kit.com, crates.io, static.crates.io, docs.rs). Takes about 10 s and
~26 MB, and replaces the mirror.

## Source

- Script: [`tools/fetch-gpui-docs.sh`](../../../tools/fetch-gpui-docs.sh)
- Digest renderer: [`tools/rustdoc-digest.jq`](../../../tools/rustdoc-digest.jq)

## Seams

- **Which GPUI.** The script reads `gpui-pre`'s exact pin from the Kit
  version's own dependency list on crates.io, so `api/` and `src/` always
  match the Kit being mirrored. crates.io's `gpui` (0.2.2, docs.rs/gpui/latest)
  is digested for reference only.
- **Digests come from docs.rs's rustdoc JSON** (`format_version` 61 when
  written). jq has no forward references, so the type renderer is one
  recursive dispatcher. A newer format may need the renderer updated; the
  symptom is a jq error, not a wrong digest. Generated `Styled` method
  families (`h_0` … `h_neg_80`) collapse to one greppable line.
- **The mirror is gitignored, so ripgrep skips it.** Pass the path
  explicitly, or use `grep -rn`.
- **When to run it:** on every GPUI Kit bump, before reading the release's
  changes — a step in [Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md).
  The prose is CC BY 4.0 / Apache-2.0, another reason it stays out of git.

[^guide]: `docs/gpui/README.md`: which GPUI, the reading order, findings checked against the sources.
