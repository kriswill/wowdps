# playbooks

Operational how-tos for recurring tasks — regenerating game-data tables per game patch, bumping PROTO_VERSION, adding a ruling, running the real-log gates.

## Concepts

* [Bump PROTO_VERSION And Deploy It](bump-proto-version.md) - The steps for a wire-shape change (CONTRACT.md, the golden bytes and the version that renames the socket) and for bringing the dev machine's live daemon, mcp servers and overlay onto it without a stale socket or a respawn race.
