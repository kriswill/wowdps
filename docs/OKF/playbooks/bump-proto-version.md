---
type: Playbook
title: Bump PROTO_VERSION And Deploy It
description: 'The steps for a wire-shape change (CONTRACT.md, the golden bytes and the version that renames the socket) and for bringing the dev machine''s live daemon, mcp servers and overlay onto it without a stale socket or a respawn race.'
tags: [proto, daemon, deploy]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T17:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the wire protocol section and its PROTO_VERSION heading
  - id: tracing
    resource: ../../tracing.md
    title: docs/tracing.md — the dev daemon unit
---

**Where:** [wowdps-proto](../crates/proto.md), [wowdps-daemon](../crates/daemon.md),
[dev-unit](../tools/dev-unit.md).

## When

Any change a client could see on the wire: a message's or a type's shape,
a tag, an enum's codes, a field's encoding — anything the golden bytes in
`crates/proto/tests/codec.rs` would catch. A meter fix that moves no shape
keeps the version, however much it changes the numbers[^contract].

## The change

1. Change the shape (`crates/proto/src/msg.rs` and its codec), CONTRACT.md's
   wire section and the golden bytes in `crates/proto/tests/codec.rs` in one
   change.
2. Bump `PROTO_VERSION` once for the branch, however many shapes it moves,
   and the number in CONTRACT.md's wire heading with it. The socket name
   embeds the version (`wowdps-v<N>.sock`, its lockfile `wowdps-v<N>.lock`
   beside it), so an old client never talks to a new daemon.
3. A shape the history store writes is not the wire: readers must still
   accept the old files, and the cards are rewritten after deploy
   (`wowdps history regrade`).
4. Record the decision when the bump carries a ruling or a policy, and
   update the crate docs it touches.

## Deploying it on the dev machine

1. **Ask first.** The user may be mid-pull; a restart drops the live meter
   and any coach's monitoring.
2. **Build release in the dev shell.** The `wowdps-dev` unit's path watch
   restarts the daemon and its overlay when the active profile's binaries
   change; otherwise `systemctl --user restart wowdps-dev`[^tracing].
3. **The new binary cannot stop the old daemon**: its stop goes to the new
   socket name. Let the unit's restart replace the old process, then remove
   the stale `$XDG_RUNTIME_DIR/wowdps/wowdps-v<old>.sock` and `.lock`.
4. **Restart every client**: each Claude session's mcp server and any
   windowed GUI look for the old socket until restarted.
5. **Never run `wowdps status` while the unit restarts.** A one-shot client
   spawns a non-linger daemon when none answers and races the unit. Check
   with `systemctl --user show wowdps-dev -p ActiveState -p MainPID`.
6. **A unit stuck in start-limit-hit** behind a self-spawned daemon
   recovers in one command:
   `systemctl --user reset-failed wowdps-dev; kill <fresh overlay pid>; target/release/wowdps stop; systemctl --user start wowdps-dev`.
   Query the overlay's pid fresh; never reuse one from earlier output.
7. **Regrade** when a stored shape moved.

[^contract]: CONTRACT.md's wire protocol section: the socket name, the framing and the bump rule.
[^tracing]: docs/tracing.md, "The dev daemon unit".
