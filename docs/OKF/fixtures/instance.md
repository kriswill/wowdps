---
type: Fixture
title: instance.txt
description: 'A completed keystone with suspend/resume on zoning and city combat between visits — the R10 visit machinery.'
resource: crates/core/fixtures/instance.txt
tags: [fixture]
status: stable
generated: { by: human:kris, at: 2026-09-05T17:38:40-07:00 }
---

ZONE_CHANGE and CHALLENGE_MODE_* lines drive instance visits; the fixture proves a new key is a new visit, zoning suspends and resumes, and the synthetic Overall segment sums its members' durations.

## Rulings exercised

- [R10 Visits & Overall](../rulings/r10.md), [R11 Meaningful segments](../rulings/r11.md).

## Gate

`crates/core/tests/instance.rs`. Beside the fixture it replays inline R10
shapes: a key whose door logged 0, a key joined mid-run, and a raid whose
difficulty was switched outside so its door logged 0. For that raid the
meter, the scanner, every lazy slice, the live tail and every checkpoint
resume must agree ([the decision](../decisions/a-pull-zones-in.md)).

## Source

- Log: [`crates/core/fixtures/instance.txt`](../../../crates/core/fixtures/instance.txt)
- Format: [`FORMAT-NOTES.md`](../../../crates/core/fixtures/FORMAT-NOTES.md); parser-independent check: [`check.awk`](../../../crates/core/fixtures/check.awk) via [`verify.sh`](../../../crates/core/fixtures/verify.sh).
