---
type: Tool
title: gen-icons
description: 'Build ~/.local/share/wowdps/class-icons.bin, the per-machine cache of the game''s class crests and spec icons that every player disc wears: the 64 px art upscaled 4× by Real-ESRGAN, cropped inside its bevel and circle-masked into 208 px tiles, which the icon levels gui-logic''s reader gains halve to the size each disc is drawn at.'
resource: tools/gen-icons.sh
tags: [tool, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:30:00-07:00 }
sources:
  - id: script
    resource: ../../../tools/gen-icons.sh
    title: gen-icons.sh — the two passes round the upscaler
  - id: icongen
    resource: ../../../tools/extract/src/icongen.rs
    title: icongen.rs — the crests' FileDataIDs, the crop, the mask and the file layout
  - id: reader
    resource: ../../../crates/gui-logic/src/icons.rs
    title: gui-logic's icons.rs — the lazy reader, and the levels it gains
  - id: esrgan
    resource: https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan
    title: Real-ESRGAN ncnn Vulkan — the upscaler (MIT), from nixpkgs when not on PATH
---

`tools/gen-icons.sh [wow-dir] [--no-upscale]` (`wowdps gen-icons` in the dev
shell) runs [`wowdps-extract`](../crates/extract.md)'s `gen-icons` round an
upscaler, as gen-portraits does: a pass that writes the
icons whole, then the one that writes the cache[^script]:

1. **Whole.** The thirteen `classicon_*` crests and every
   `ChrSpecialization.SpellIconFileID`, decoded from BLP, each written at
   its native 64 px (`--whole DIR --whole-only`, `<fdid>.png`, no cache).
   Without the upscaler this pass is skipped.
2. **The upscale.** Real-ESRGAN's illustration model
   (`realesrgan-x4plus-anime`) takes them to 256 px on the GPU[^esrgan].
3. **The cut.** `--upscaled DIR` builds the cache from the upscales: each
   cropped 6/64 per side and circle-masked, 208 px[^icongen]. Without the
   upscaler, or with `--no-upscale`, the tiles are the game's own pixels,
   52 px.

The result is about 9 MiB, renamed into place so a running GUI keeps
reading the file it opened. Like every icon bin it is extracted art and
stays out of the repository; without it the GUI draws class-colored discs.

## Seams

- **The crop.** Every icon wears a 4 px bevel (a light rim over a dark
  line). Under a circle mask it showed as pale arcs at the disc's four
  edges, and the upscaler sharpens it into a bright ring, so it is cut
  before the mask. Six of 64 rather than the bevel's four, because 208
  halves evenly to 13.
- **Levels, not one size.** GPUI samples an image bilinearly with no
  mipmaps, so a 208 px tile on an 18 px meter disc would shimmer. With
  the icon levels it gains, the [gui-logic](../crates/gui-logic.md) reader
  is asked for the size a disc is DRAWN, in device pixels (the zoomed size times the display's scale
  factor), and serves the smallest halving that covers it (208, 104, 52,
  26 or 13), alpha-weighted, each level made once and read lazily[^reader].
  The replay spike, which paints its own discs, holds every level of a
  spec and picks at paint by the same rule.
- **Format v1 throughout.** The file layout never changed; only its
  `icon_px` grew from 32. A build from before reads the 208 px tiles too
  (its cap is 256) and draws them unhalved: blurred art, never discs.
- **All or nothing.** `--upscaled` fails on any icon whose upscale is
  missing or not exactly 4×: one cache never mixes tile sizes. The script
  then falls back to the game's pixels.

[^script]: gen-icons.sh — the two passes round the upscaler
[^icongen]: icongen.rs — the crests' FileDataIDs, the crop, the mask and the file layout
[^reader]: gui-logic's icons.rs — the lazy reader, and the levels it gains
[^esrgan]: Real-ESRGAN ncnn Vulkan — the upscaler (MIT), from nixpkgs when not on PATH
