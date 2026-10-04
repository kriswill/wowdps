---
type: Decision
title: No Forked Or Patched GPUI
description: 'The GPUI GUI (built as gui-new, now wowdps-gui) builds on stock gpui-pre through gpui-kit, pinned exactly — never a fork, a vendored copy or a [patch] of any gpui-pre or Kit crate — because gpui-pre moves about weekly and a carried patch is a tax on every bump; a capability GPUI lacks is designed around or contributed upstream.'
tags: [gui, dependencies]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T20:40:00-07:00 }
sources:
  - id: gpui
    resource: ../../gpui/README.md
    title: The GPUI reference — the third-party harness survey and the 0.3.7 findings
  - id: spec
    resource: ../../spec-gui-new.md
    title: gui-new specification — §2 non-negotiable 2, §7.2 the edge strip, §9 testing
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md §Dependencies — no [patch], no fork
---

**Where:** every crate that names GPUI. Today that is
[`wowdps-gui`](../crates/gui.md), built as gui-new
([Rebuild The GUI On GPUI Kit](gui-on-gpui.md)); the rule is in the
dependency policy[^contract].

## Context

`gpui-pre` is a crates.io snapshot of Zed's GPUI, published about weekly,
and GPUI Kit pins one exactly. Most GPUI automation found on GitHub on
2026-09-30 carries GPUI changes of its own[^gpui]:
- `themixednuts/gpui-mcp` needs a 1,514-line patch over gpui-pre;
- `stefan-siebert/gpui-mcp` needs a Zed fork plus a gpui-component fork;
- `stolinski/gpui-agent` needs a patched gpui-pre 0.3.5;
- the `gpui-ce` community fork exposes an accessibility tree for tests.

Each of these has to be re-applied, and possibly re-designed, on every
snapshot. The user ruled out `gpui-ce` on sight, and the rule was widened
to every patch.

## Decision

- **Stock crates only.** No fork, no vendored copy, and no `[patch]` of any
  `gpui-pre*` or Kit crate, in any profile.
- **Design around a gap.** gpui-pre cannot change a layer surface's margin
  after creation, so the overlay surface spans its edge's length and moves
  its content and input region instead of itself[^spec].
- **Use what the vendor ships.** The click-through test harness is Kit's own
  `TestWindowExt`, not a patched driver.
- **Contribute upstream.** A capability worth having goes to Zed or Kit as a
  PR (a runtime `set_margin` for layer surfaces is the first candidate) and
  is adopted on a later bump.

## Consequences

Some designs are more roundabout than a patch would make them: a
full-length transparent strip, input regions tracked by hand. Tests can
observe only what Kit's harness exposes; there is no visible-text getter,
and a canvas is one target. In return a GPUI bump is a `cargo update`, the
test suite and the guard, with nothing to rebase.

The replay's rotated floor is another case: GPUI draws images
axis-aligned only, so the floor is resampled on the CPU rather than
patching a transform into the image sprite
([The Replay's Room Turns To The User, Without Edges](replay-room-turns-without-edges.md)).

The rule is a strong
lean, not an absolute ban: an exception would be its own decision record
with the cost written down. Landed in the policy in `4d33cee`.

[^gpui]: `docs/gpui/README.md`, "Click-through tests need no screenshots" and the third-party table.
[^spec]: `docs/spec-gui-new.md` §2 (non-negotiable 2), §7.2 (the edge strip), §9 (testing).
[^contract]: `CONTRACT.md` §Dependencies: "No `[patch]` of any `gpui-pre*` or Kit crate and no fork of GPUI".
