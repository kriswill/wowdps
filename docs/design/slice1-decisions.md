# Slice 1 — decisions

Arbitration between `slice1-spec.md` (designer) and `slice1-risks.md` (devil's
advocate). Where they disagree, **this file wins**. Everything here was checked
against the source, with the evidence named.

## 1. A read must never be able to drop a fight (risks #18) — BLOCKING, fix first

Confirmed: `HistoryReq::Query` and `HistoryReq::Store` ride the same 64-slot
channel (`crates/daemon/src/history.rs:50`, `HistoryReq::Query` at ~:153), and
`HistoryLink::send` is a lossy `try_send` that counts the drop and hands the
request back (`history.rs:216-227`). A Home dashboard that queries in a loop
can therefore fill the queue and cause a closing pull's `Store` to be dropped.

`HistoryLink::reply` already establishes the precedent that some requests must
not ride the lossy path (`history.rs:229-236`).

**Fix, daemon-internal, no wire change, as the first commit:** give the read
path (`Query` / `Fight`) its own bounded channel, or its own reserved quota, so
that reads can never consume the slots a `Store` needs. Keep "dropping beats
stalling" for writes — that trade-off is deliberate and stays. Add a test that
fills the read path and proves a `Store` still lands.

**Plus GUI discipline, not instead of it:** at most one query in flight; no
timer-driven refresh; refresh only on Home open, on an explicit user action,
and on a debounced `HistoryChanged` *while Home is visible*.

## 2. Bound every answer (risks: `MAX_FRAME`)

The GUI always sends `limit: 200` and pages sequentially, stopping as soon as
it has `total` or 5 pages (1 000 cards), whichever comes first. Also clamp the
`Fights` limit daemon-side (internal, no wire change) so no client can ask for
a frame the reader will reject — `wire::frame` only `debug_assert!`s, so a
release build would emit an oversized frame and produce a silent reconnect
loop.

## 3. Disabled, empty and dropped must look different

`hub.rs:143-158` synthesises an *empty* answer both when the store is disabled
and when the queue rejected the request, so "no cards" is ambiguous. `Status`
already carries what disambiguates it: `HistoryStatus { enabled, fights,
dropped, error }` (`proto/src/msg.rs:501-514`).

Home sends `GetStatus` on open and renders three distinct states: **disabled**
(name the `error`), **empty** ("no fights stored yet"), **degraded**
(`dropped > 0` — say so). Never confident zeros.

## 4. Cuts and corrections to Layout A

| Panel / number | Decision |
| --- | --- |
| Season score | **Cut the card.** No score field exists anywhere in the store. Do not dash it, do not compute a lookalike — remove it. |
| Raid `7 / 8` | Reword to **"N down · M seen"**. There is no raid→boss roster in the repo, so the denominator would be invented. |
| "Best week" | Keep only if derivable from card timestamps + the owner's own numbers; otherwise cut it, same rule as the score. |
| `best_pct` | **Usable.** The doc comment at `proto/src/history.rs:298` says "never written yet" but `daemon/src/history.rs:2630` writes it. Fix the stale comment in its own commit. |
| Characters | **Keep.** Derivable as the distinct card `owner` guids, unioned with `history_characters` from `~/.config/wowdps/config.toml` (`daemon/src/config.rs:33,151`) — the GUI already reads that same file. |
| Percentile | Against the owner's own history, labelled as such on screen. We have no ladder. |

The designer's `unknowable_numbers_render_as_em_dash` test stays, and covers
whatever survives as a dash after these cuts.

## 5. Keymap: bind in `window.rs`, not `keys.rs`

`~ m H / ?` in `gui/src/keys.rs` would fail `crates/tui/tests/keybind_parity.rs`
(`GUI_ONLY_CHARS` is `v`/`g`). Lowercase `t` already shows the way: window-local
bindings live in `window.rs`. Do that, and no `CONTRACT.md` change is needed —
verify the parity test is green rather than assuming it.

## 6. The filter box must swallow the keymap while focused

The keymap is a global `keyboard::listen()` (`window.rs:434`), so typing `q`
into an unswallowed filter quits the app. The talent viewer already solves
exactly this — follow that precedent, and add a test that types `q` into a
focused filter and asserts the app is still alive.

## 7. API points, now resolved (were "verify" in the spec)

- Gradient backgrounds exist: `Background::Gradient(Gradient)` with
  `From<Linear> for Gradient` (`iced_core-0.14.0/src/background.rs:10,33`), and
  `iced_tiny_skia-0.14.0` renders them (`engine.rs:193`, `geometry.rs:434`), so
  they survive the headless render tests.
- Focus exists: `iced::widget::operation::focus(id) -> Task<T>`
  (`iced_runtime-0.14.0/src/widget/operation.rs:65`). The "click-to-focus only"
  fallback is not needed.

## 8. Scope

Slice 1 is: the daemon read/write split (§1), the limit clamp (§2), `theme.rs`,
`nav.rs`, `home.rs`, the `?` sheet, the filter, and the `~`/Esc chain. The
throughput table, cast timeline, uptime lanes, stack matrix, mitigation cards,
death navigator and gear grid are **not** in this slice.
