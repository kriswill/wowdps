---
type: Crate
title: wowdps-gui-new
description: 'Deprecated: the name the GPUI GUI was built under (crates/gui-new, binary wowdps-gui-new) beside the iced wowdps-gui; at the cutover it became crates/gui and took the wowdps-gui name, so its record is wowdps-gui.'
resource: crates/gui
tags: [crate, gui]
status: deprecated
generated: { by: claude-code/opus-5.5, at: 2026-10-01T11:32:09-07:00 }
sources:
  - id: plan
    resource: ../../plan-gui-new.md
    title: The implementation plan — phases 1–4 as built, and phase 5's cutover note
---

The GPUI rebuild of the window and the overlay lived here while it ran
beside the iced GUI: `crates/gui-new`, binary `wowdps-gui-new`, reached as
`wowdps gui-new`, spawned as the overlay only when config `gui_binary`
named it, and built per package (its own CI steps, the flake's
`.#wowdps-gui-new` with its own crane layer, the modules'
`guiNewPackage`)[^plan]. The cutover deleted the iced crate and moved this
one into `crates/gui` as package and binary `wowdps-gui`; the second
package, the wrapper, the option and the per-package CI steps went with it
([Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md)).

Everything this record described — the `Session`, keys in contexts, the
overlay's edge strip, the window as one entity over `Gui::fight`, one
scrollbar, the shot tests — now belongs to [wowdps-gui](gui.md).

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).

[^plan]: `docs/plan-gui-new.md`: phases 1–4 as built; phase 5's "As built: the cutover".
