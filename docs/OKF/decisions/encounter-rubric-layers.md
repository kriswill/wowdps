---
type: Decision
title: Resolve An Encounter Through An Ordered Layer Stack
description: 'Deprecated 2026-10-30: how the encounter rubric resolves an encounter through an ordered stack of base, curated and user sources left wowdps with the rubric for the private wowdps-replay repository.'
tags: [replay, rubric, design]
status: deprecated
generated: { by: claude-code/opus-5.5, at: 2026-10-30T23:00:00-07:00 }
sources:
  - id: replay
    resource: https://github.com/kriswill/wowdps-replay
    title: wowdps-replay — the private repository the replay and the encounter rubric live in
---

**Deprecated 2026-10-30.** This decision was about the
[encounter rubric](../crates/encounter-rubric.md)'s resolver: each
encounter read through an ordered stack of sources in three tiers (the
base's season, instance, draft and drawing files, then every curated set
in the order laid, then the user's own files), with a provenance per key
path, so the curated layer could ship apart from the public base and a
user could override any key on their own machine.

The rubric left wowdps with the replay for the private wowdps-replay
repository[^replay], and the decision went with it; this record's history
is in the log and in git.

[^replay]: wowdps-replay — the private repository the replay and the encounter rubric live in
