---
type: Decision
title: Give The Map A Vocabulary Per Level And Per Area
description: 'Deprecated 2026-10-30: the encounter rubric''s map vocabulary (a dungeon level as an instance file''s [level.<ui_map>], areas, paths, transitions and the render''s framing) left wowdps with the rubric for the private wowdps-replay repository.'
tags: [replay, rubric, design]
status: deprecated
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:00:00-07:00 }
sources:
  - id: replay
    resource: https://github.com/kriswill/wowdps-replay
    title: wowdps-replay — the private repository the replay and the encounter rubric live in
---

**Deprecated 2026-10-30.** This decision gave the
[encounter rubric](../crates/encounter-rubric.md) what a map editor
writes: a dungeon level read as the same map an arena's is, and inside
any map the areas the camera knows, the paths between them, what the
replay does while the log is quiet, and the render's framing tweaks, all
given by the base. It built on
[Resolve An Encounter Through An Ordered Layer Stack](encounter-rubric-layers.md).

The rubric left wowdps with the replay for the private wowdps-replay
repository[^replay], and the decision went with it; this record's history
is in the log and in git.

[^replay]: wowdps-replay — the private repository the replay and the encounter rubric live in
