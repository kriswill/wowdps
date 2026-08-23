# Contributing

Thanks for your interest! A few things about this codebase are non-negotiable
and knowing them up front will save you a rewritten PR.

## CONTRACT.md is binding

[`CONTRACT.md`](CONTRACT.md) fixes the public signatures of `parser`, `meter`
and `index`, the semantic rulings R1–R13 (what counts as damage, absorb
attribution, segment boundaries, pet attribution, class inference, …) and the
wire protocol. The fixture golden values are computed from the rulings, and
`crates/proto/tests/codec.rs` pins the wire encodings byte-for-byte.

Changing any of these means changing CONTRACT.md, the fixtures/golden bytes,
and the code **together**, in one PR — and a wire-shape change means bumping
`PROTO_VERSION`. A PR that changes behavior without touching the contract, or
vice versa, will be asked to reconcile them.

## Dependency policy

- `crates/model`: zero dependencies.
- `crates/core`, `crates/proto`, `crates/daemon`, `tools/extract`: stdlib only.
- Approved elsewhere: ratatui + crossterm (tui); iced + iced_layershell +
  serde/toml (gui).
- No chrono (timestamps are hand-parsed), no tokio (threads + channels), no
  serde outside the gui.

New dependencies anywhere need a strong case.

## No panics in production code

The workspace denies `unwrap`, `expect`, indexing, `panic!` and friends via
clippy lints — a meter that aborts on a malformed log line is worse than one
that skips it, and the daemon outlives every client that could restart it.
Tests are exempt (`clippy.toml`); there a panic IS the failure mechanism.

## Before you open a PR

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
crates/core/fixtures/verify.sh   # gawk recomputes golden totals, no parser
```

CI runs exactly these plus the corrupt-fixture negative control and a
`nix build .#wowdps` of the flake package.

Building the GUI needs Wayland system libraries — on NixOS use `nix develop`
(or devenv). The daemon/TUI is pure Rust and builds anywhere.

## LLM-assisted contributions

Welcome — this whole project was built with Claude Code. See the README's
"AI use" section for the rules; in short: the project's standards apply in
full, no copyrighted or license-laundered material, and you own what you
submit — review it, test it, be able to explain it.

## Licensing

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as MIT OR Apache-2.0, without any additional
terms or conditions.

Never commit extracted game assets. Generated tables (`class_spells.rs`,
`item_spells.rs`, `keystone_timers.rs`) contain only factual identifiers and
are regenerated per game patch by `tools/gen-*.sh`; artwork lives in
per-machine caches under `~/.local/share/wowdps/` and stays out of the repo.
