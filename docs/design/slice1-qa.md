# Slice 1 — independent QA

Reviewer: QA pass over `932f16e..5207df7` (branch `feat/ui-design-update`),
against `docs/design/slice1-decisions.md` (binding) and
`docs/design/slice1-risks.md`. Nothing was fixed; everything below is for the
coder or lead.

Toolchain runs (all inside `nix develop -c`):

| run | result |
| --- | --- |
| `cargo test --workspace` #1 | **ok** — 1134 passed, 0 failed, 11 ignored (61 suites) |
| `cargo test --workspace` #2 | **FAILED** — 1 flake, `wowdps-daemon overlay::tests::supervisor_edges_no_spawner_returning_game_and_real_stderr` |
| `cargo test --workspace` #3 | **ok** — 1134 passed, 0 failed, 11 ignored |
| `cargo clippy --workspace --all-targets` | clean, exit 0, zero warnings |
| `cargo fmt --all -- --check` | clean, exit 0 |

---

## Verdict per item

| # | Item | Verdict |
| --- | --- | --- |
| 1 | No wire change | **PASS** |
| 2 | §1 reads cannot evict a write | **PASS with a serious defect** (finding 2) |
| 3 | §2 the cap | **PASS** (finding 3, minor) |
| 4 | §9 infinite scroll | **PASS** |
| 5 | §3/§4 data honesty | **FAIL** (finding 5 serious, finding 6 serious) |
| 6 | §5/§6 keymap + filter swallow | **PASS** (finding 8, minor) |
| 7 | Luminance deviation | **PASS on the deviation, FAIL on one class** (finding 9 serious) |
| 8 | Test quality / flake claim | **PASS on the flake claim**; findings 11–13 |

---

## 1. No wire change — PASS

`git diff --stat 932f16e..HEAD` touches exactly one file under `crates/proto/`:
`crates/proto/src/history.rs`, `+4 -1`, and the whole hunk is the `best_pct`
doc comment (decisions §4's "fix the stale comment in its own commit", done as
`7d77c30`). Untouched: `crates/proto/src/msg.rs` (so no `PROTO_VERSION` bump,
no new `ClientMsg`/`DaemonMsg` variant, no codec change), `crates/proto/src/
wire.rs` (frame layout), `crates/proto/tests/codec.rs` (golden bytes — not in
the diffstat at all).

No severity.

---

## 2. §1 read/write split — the mechanism holds; the `dropped` counter now lies. **SERIOUS**

**The mechanism is sound.** `crates/daemon/src/history.rs:58` `READ_QUOTA =
QUEUE / 2` (= 32 of 64). `HistoryLink::send` does
`self.reads.fetch_add(1, AcqRel) >= READ_QUOTA` — `fetch_add` returns the
*prior* value and every racing caller gets a distinct one, so at most 32
callers ever pass; a refused caller decrements again. The thread calls
`read_done()` (`history.rs:384-386`) after `recv()` and before `handle`, so
what the quota bounds is channel occupancy, not handler latency. Conclusion:
**reads can never occupy more than 32 of the 64 slots**, so a `Store` always
has ≥ 32 free slots. I tried to break it and could not — the atomic is
correctly ordered and the refund paths (quota refusal, `TrySendError`) both
decrement.

**On "does it hold when the write half is also full?"** — it does not, by
design: 32 backed-up `Store`s plus 32 reads fills the queue and the 33rd
`Store` is dropped. Decisions §1 explicitly keeps "dropping beats stalling"
for writes, and writes still get all 64 slots when no reads are queued, so
this is within spec. Worth stating out loud in the doc comment, though: the
guarantee is a *floor of 32*, not "a Store is never dropped".

**The test is not vacuous.** `crates/daemon/tests/history.rs:586
a_flood_of_reads_cannot_drop_a_store` sends 500 undrained `Query`s, then
asserts every `Store` from the fixture lands. Without the quota, 500 reads
fill all 64 slots and every `Store` fails — so the test does catch the
regression it names.

It is, however, **weak at the boundary**: `sample.txt` yields only ~3 closed
fights, so it proves 3 writes fit in 32 free slots, not 32. A version that
pushes `QUEUE/2` writes after the flood would actually pin the reserved half.

**The defect.** `count_drop()` (`history.rs:266-270`) is called on a
quota-refused *read* and increments the same `HistoryStatus::dropped` field
that `crates/proto/src/msg.rs:506` documents as *"**Writes** dropped because
the hub → history queue was full."* After this branch, a client polling the
store inflates a counter whose whole contract is "you lost fight data" — and
decisions §3 wires exactly that counter to Home's **degraded** banner
(`home.rs:845`, "N request(s) the daemon dropped — this is not the whole
story"). A dashboard that scrolls hard will accuse the daemon of losing the
user's fights. This is the same class of dishonest-number failure §3 exists to
prevent, introduced by §1's fix.

**Want changed:** separate counters — `dropped` stays writes-only, add a
reads-refused counter (daemon-internal or a later wire bump), and make Home's
degraded line read only the write counter. Failing that, at minimum fix
`msg.rs:506`'s doc and re-word the Home banner, because right now the code and
the contract disagree.

---

## 3. §2 the cap — enforced daemon-side; arithmetic is safe with margin. **MINOR**

**Daemon-side, not client politeness.** The clamp is inside `Store::fights`
(`crates/daemon/src/history.rs:1780-1786`):
`let limit = if limit == 0 { 50 } else { limit as usize }.min(FIGHTS_CAP);`
with `FIGHTS_CAP = 500` (`:62`). That is the answer builder every `Fights`
query routes through (`:1656`), so no client can opt out. `total` stays
unclamped. The GUI's `PAGE = 200` (`home.rs:35`) is *additional* politeness,
not the enforcement. Test `a_fights_answer_is_capped_however_much_the_client_
asks_for` sends `limit: u32::MAX` and asserts `got.len() == cap` — genuine.

**Arithmetic.** Counting the wire fields in `put_card_player`
(`crates/proto/src/msg.rs`): 15×u64 + 3×f64 + 4×u32 + 1×u16 + 1×u8 + 2×bool =
165 B fixed, + 3 `put_opt` (~27 B) + 2 strings (guid ~40 B, name ~24 B, plus
length prefixes) ≈ **~270 B per player**. `put_card`'s own scalars are ~130 B
plus its name/id strings and `bosses[]`.

- 20-player raid card: ~5.6 KB → 500 cards = **2.8 MB** (17 % of `MAX_FRAME`)
- 40-player card: ~11 KB → 500 cards = **5.5 MB** (34 %)
- 5-player key card + 10 bosses: ~2.3 KB → 500 cards = **1.2 MB**

So the capped answer is comfortably under `MAX_FRAME = 16 MiB` for anything a
raid or key produces — roughly 3× margin at the worst realistic shape.

**The residual (minor).** The cap bounds card *count*, not answer *bytes*. A
card's `players` vec is not itself bounded — a `Trash` segment in a crowded
capital city or a large battleground can record hundreds of players. At ~200
players/card, 500 cards is ~30 MB and crosses `MAX_FRAME`. Decisions §2 asked
only for the clamp, and `wire::frame`'s `debug_assert!` was deliberately left
alone (risks #17's third mitigation was not adopted into §2), so this is
in-spec — but the path to a silent release-build reconnect loop is still open,
just narrower.

**Want changed:** either a byte budget rather than a card budget, or (cheaper)
turn `wire::frame`'s `debug_assert!` into a real refusal so the failure mode is
an error rather than a reconnect loop. Not a merge blocker.

---

## 4. §9 infinite scroll — PASS

**No pager anywhere.** `grep -rn "pager\|next_page\|prev_page\|page_number\|
load more\|Load More\|LoadMore\|NextPage\|PrevPage" crates/gui/src/` returns
only two *comments* (`window.rs:323`, `home.rs:625`) saying there is no pager.
`crates/gui/src/nav.rs` has no page control. The only visible affordances are
the `scrollable` (`home.rs:826-829`) and, at the burst ceiling, a plain
sentence — `home.rs:817-823`, "scroll for more of the store" — which §9
explicitly permits ("say so in the list's footer in words").

**No `relative_offset`, short content guarded.** `grep` for `relative_offset`
in `crates/gui/src/` hits only two comments explaining why it is avoided.
`home.rs:877-886 wants_more` computes from `content_bounds().height`,
`bounds().height` and `absolute_offset().y` (lifted into `ScrollAt`,
`home.rs:860-874`, because `Viewport` has no public constructor — a good call
that makes the trigger testable) and returns `false` outright when
`content_h <= view_h`.

**Fetch is gated at one choke point.** `home.rs:89-92`:
`if self.pending.is_some() || self.complete() || self.pages >= MAX_PAGES {
return None }`. Both the open gesture and `Message::HomeScrolled`
(`window.rs:638-650`) go through `next_request`, so a scroll burst cannot
issue concurrent queries. `absorb` (`home.rs:122-124`) drops any reply whose
id is not the outstanding one, which also closes risks #20's stacking-replies
hole. `complete()` stops the asking, and the pending affordance is gated on
`!home.complete() && home.pending.is_none()`.

**Would the tests catch a regression?** Yes, both.
`a_second_scroll_while_a_query_is_out_sends_nothing` (`window.rs:1386-1418`)
asserts request count 1 then 0 across two identical scroll messages — delete
the `pending.is_some()` guard and it fails immediately.
`scrolling_to_the_bottom_asks_for_more_without_a_button` (`window.rs:1359-
1383`) asserts a `GetHistory` results from the scroll alone — delete the
`on_scroll` wiring and it fails. `a_short_list_never_asks_for_more`
(`window.rs:1421-1430`) is a pure-function test with the divide-by-zero case
and both long-list ends. Nothing vacuous here.

No severity.

---

## 5. §3 — the three states render differently, but `dropped`/`disabled` are read once at launch and then never refresh. **SERIOUS**

The rendering half is right: `state_line` (`home.rs:833-852`) returns four
distinct outcomes — disabled (names the `error`), not-yet-answered ("reading
the history store…"), degraded (`dropped > 0`, names the count), or `None` —
and `screen` pushes it above every number (`home.rs:652-654`). Empty panels
dash with a reason rather than zeroing (`home.rs:378-388`: "Nothing matched:
three dashes with the reason, not three zeros").

**The defect:** `GetStatus` is sent exactly once, from `Gui::new`
(`window.rs:146`, `req_id: 0`). `grep -n "GetStatus" crates/gui/src/window.rs`
returns that single line — Home does **not** send it on open, contrary to
decisions §3 ("Home sends `GetStatus` on open"). And the daemon never
broadcasts `Status` unsolicited: `crates/daemon/src/hub.rs:319-332` pushes it
only in reply to `ClientMsg::GetStatus`. So `state.history_dropped` and
`state.history_disabled` are frozen at the value they had at window
construction. A store that gets disabled mid-session, or (much more likely,
per finding 2) a `dropped` counter that rises while the user browses, is
invisible. The degraded banner is effectively dead code in practice.

**Want changed:** send `GetStatus` in `open_home` (and on the debounced
`HistoryChanged` refresh), as §3 specifies.

**Test gap, same finding:** `the_screen_renders_loading_empty_and_populated`
(`home.rs:1096-1131`) calls `state_line` *directly* for the disabled and
degraded cases and only does `let _ = render(screen(...))` — with no
assertion — for loading and empty. If `screen()` stopped pushing `state_line`
onto the body, every assertion in that test still passes. Nothing in the
suite proves the three states are *visibly* distinct on screen.

---

## 6. §4 — the cut numbers are genuinely gone. **PASS** (one minor)

- **Season score** — cut, not dashed. `grep -rn "score" crates/gui/src/home.rs
  crates/gui/src/nav.rs`: the only hit outside comments is
  `nav.rs:481 Stat::unknown("season score", "not in the log")`, which is
  *sample data inside the `stat_cards` widget unit test*, never rendered by
  Home. `home.rs:966-971` asserts the card cannot come back:
  `assert!(!panels.top.iter().any(|s| s.label.contains("score")))`.
- **`N / 8`** — no `/ 8` denominator anywhere; `home.rs:465` documents why and
  the raid panel counts kills only.
- **Ladder percentile** — no `percentile`/`pct of`/`ladder` string in
  `home.rs` or `nav.rs`. Cut entirely (§4 would have allowed an own-history
  version; cutting is the safer subset).
- **Best week** — cut. No `best_week` in the derivation.
- **No faked zeros.** `grep -n "unwrap_or(0\|unwrap_or_default()\|map_or(0"
  crates/gui/src/home.rs` yields two hits, both benign and neither a displayed
  number: `:364` a timestamp seed for the week window, `:476` a grouping key.
  Every displayed unknown goes through `DASH` (`home.rs:608, 662, 731`) or
  `Stat::unknown`. `best_pct` is handled honestly at `home.rs:715-721`:
  `(None, None) => "no kill · N pulls"` with the comment "Never '100%'".
- `me_panel` filters `v > 0.0` out of the sample rather than counting a sat-out
  pull as zero performance (`home.rs:527-532`) — correct.

**Minor:** `home.rs:541` computes the "median" as `values[len/2]`, which for an
even count is the upper-middle element, not the mean of the two middles. The
label says "median". Cosmetic, but it is a number with a name.

**Minor:** decisions §4 said Characters should be "distinct card `owner` guids
**unioned with** `history_characters` from the config". Only the owner half is
implemented (`character_lines`, `home.rs:557-583`). The code comment is honest
about the trade-off, so this is a documented scope reduction rather than a lie.

---

## 7. §5/§6 keymap and swallow — PASS (one minor)

**§5.** `crates/tui/tests/keybind_parity.rs` is green in all three runs. It is
still meaningful: its extractors key on the literal `=> Action::`
(`keybind_parity.rs:36, 66`), and the new `keys.rs` additions are a `BINDINGS`
data table whose lines read `keys: "d",` — no `=> Action::`, so they are
correctly invisible to the parity scan rather than accidentally suppressed.
`GUI_ONLY_CHARS` was not touched. The new window-local keys (`~ m ? /`) are
bound in `window.rs:507-530`, exactly as §5 required, and `keys.rs:318-330
bindings_table_covers_every_action_key` holds the advertised table against
`action_for` for every non-`window_local` row.

**§6, the swallow is real.** `window.rs:492-506`: while `state.filter_focused`
the `else if` chain terminates in a `match` that handles only Escape and
Enter and does nothing else — control never reaches `action_for`, so `q`
cannot quit. `FocusFilter` also issues
`iced::widget::operation::focus(nav::filter_id())` (`window.rs:517`), so the
field is genuinely focused rather than click-only. The filter is wired for
real (`view.rs:150-166 filtered_indexed`, called at `view.rs:457, 565`), and
keeps original row indices so a filtered click still selects the right player.

The test `the_meter_keymap_is_swallowed_while_the_filter_has_focus`
(`window.rs:1487-1499`) is well built: it presses `q` while focused and
asserts `!quit`, **then unfocuses and presses `q` again and asserts `quit`**.
That positive control means the test cannot pass by the keymap being broken
altogether — delete the swallow branch and the first assertion fires.

**Minor:** `keys::zoom_for` is checked *before* the swallow branches
(`window.rs:467`), so `ctrl +` / `ctrl -` / `ctrl 0` still zoom while the
filter has focus. `zoom_for` returns `None` without a control modifier
(`keys.rs:192-194`), so bare `+`/`-`/`0` type normally. Defensible, but it is
one un-swallowed path and worth a deliberate comment.

---

## 8. The luminance threshold — the deviation is correct, one class still fails WCAG AA, and the test will not catch it. **SERIOUS**

I recomputed WCAG relative luminance (`(x+0.055)/1.055)^2.4`, 0.2126/0.7152/
0.0722) and contrast `(L1+0.05)/(L2+0.05)` for all 13 `Class::rgb` values
(`crates/model/src/lib.rs:456-468`) against the ink `theme::accent` actually
produces (`INK_DARK` 0.043/0.047/0.063, `INK_LIGHT` 0.95/0.95/0.98):

| class | luminance | `light` @0.179 | ink | ink : base | other ink |
| --- | --- | --- | --- | --- | --- |
| Warrior | 0.3655 | true | dark | 7.74 | 2.27 |
| Paladin | 0.4153 | true | dark | 8.66 | 2.03 |
| Hunter | 0.5635 | true | dark | 11.42 | 1.54 |
| Rogue | 0.8696 | true | dark | 17.12 | 1.03 |
| Priest | 1.0000 | true | dark | 19.55 | 1.11 |
| DeathKnight | 0.1297 | false | light | 5.26 | 3.35 |
| **Shaman** | **0.1681** | **false** | **light** | **4.33** | **4.06** |
| Mage | 0.4790 | true | dark | 9.85 | 1.79 |
| Warlock | 0.2893 | true | dark | 6.32 | 2.78 |
| Monk | 0.7379 | true | dark | 14.67 | 1.20 |
| Druid | 0.3570 | true | dark | 7.58 | 2.32 |
| DemonHunter | 0.1412 | false | light | 4.94 | 3.56 |
| Evoker | 0.2310 | true | dark | 5.23 | 3.36 |

**The deviation is right and should be endorsed.** At the spec's 0.6 threshold
six classes flip to light ink at catastrophic contrast: Hunter 1.54:1,
Mage 1.79:1, Paladin 2.03:1, Warrior 2.27:1, Druid 2.32:1, Warlock 2.78:1.
The coder's 0.179 is WCAG's own black/white crossover and is strictly better.
The module doc at `theme.rs:29-38` describes this accurately.

**The failure: Shaman (`0x0070DD`), 4.33:1 — below WCAG AA's 4.5:1 for normal
text.** And note the "other ink" column: Shaman's best achievable contrast
with *either* ink is 4.33:1, so no threshold tweak fixes it. The accent colour
itself is the problem; it needs `darken(base, ~0.1)` (or a slightly lighter
ink) before it clears AA. DemonHunter (4.94) and Evoker (5.23) pass but sit
close. Heading-on-`PANEL` contrasts are all ≥ 4.67, comfortably over the
test's 3.0 floor.

**Would the coder's test catch it? No.** The test is named
`every_class_is_legible` (`theme.rs:250`, not `every_class_has_a_legible_
accent`), and it asserts `c >= 3.5`, not 4.5:

```
assert!(c >= 3.5, "{class:?} ink on base is only {c:.2}:1");
```

Shaman's 4.33 sails past a 3.5 floor. The threshold the test enforces is below
every published accessibility bar (AA large text is 3.0, AA normal text is
4.5), so the test cannot flag the one class that actually fails.

**Want changed:** raise the assertion to `>= 4.5` and adjust Shaman's accent
(darken the base for chrome use only — `Class::rgb` itself is a game constant
and must not move) until it passes. This is the finding I would most want
fixed, because it is a shipped-and-tested legibility claim that is false.

---

## 9. Test quality — the vacuous ones. **MINOR**

Tests from the diff that would still pass with the feature stubbed or deleted:

1. **`home::tests::the_screen_renders_loading_empty_and_populated`**
   (`home.rs:1096`) — the loading and empty halves are `let _ = render(...)`
   with no assertion, and the disabled/degraded halves call `state_line`
   directly. Stub `screen()` to return an empty container and the test still
   passes. See finding 5.
2. **`nav::tests::the_chrome_pieces_render`** and the two `_ = ui.snapshot(...)`
   calls in `stat_cards_render_a_headline_and_an_em_dash` — "does not panic"
   smoke tests. The `ui.find(...)` assertions in the latter *are* real.
3. **`theme::tests::every_class_is_legible`** — passes today with a class below
   AA (finding 8). Not vacuous, but its floor is set below the standard it is
   named for.
4. **`window::tests::a_short_list_never_asks_for_more`** — tests
   `home::wants_more` as a pure function. Delete the `on_scroll` wiring and it
   still passes. (Covered by its sibling test, so acceptable.)

The rest are good. The scroll pair, the filter-swallow test (with its positive
control), `a_stale_answer_is_dropped_and_pages_dedupe`,
`bindings_table_covers_every_action_key`, and both new daemon tests all fail
when the thing they name is removed.

---

## 10. The flake — the coder's claim is correct, but I hit a different one. **MINOR**

**The coder's claim, verified.** `gui overlay::tests::styles_and_theme_are_
fixed` (`crates/gui/src/overlay.rs:3560-3571`) asserts
`start_view().is_none()` — "no AUTOVIEW in the test environment".
`start_view` reads `WOWDPS_OVERLAY_AUTOVIEW` (`overlay.rs:389`), and sibling
tests in the *same test binary* set and remove that variable
(`overlay.rs:2411, 2425, 2461-2463, 2481`). Process-global env mutated from
parallel test threads is a genuine pre-existing race. The branch's
contribution: `crates/gui/src/overlay.rs` is **not in the diffstat at all**,
and `git diff 932f16e..HEAD -- crates/gui/ | grep "set_var\|remove_var\|
AUTOVIEW"` returns nothing. So the branch did not cause it. It did add ~25
tests to that binary, which changes thread scheduling and can raise the odds
of a latent race firing — worth saying, but the diagnosis stands.

**A second, unrelated flake I hit on run #2** (not seen on runs #1 or #3):

```
overlay::tests::supervisor_edges_no_spawner_returning_game_and_real_stderr
panicked at crates/daemon/src/overlay.rs:575:
the child never died: Failed("spawning /tmp/.../spew: Text file busy (os error 26)")
```

`ETXTBSY`: the test writes an executable script and execs it while a write
handle is still open elsewhere in the process. `crates/daemon/src/overlay.rs`
is also untouched by this branch, so this too is pre-existing. Both flakes are
in the same family (test-binary-global state) and both are worth a separate
ticket; neither should block this merge.

---

## Summary of findings by severity

**Serious**

- **F2** — `HistoryStatus::dropped` now counts refused *reads* as well as lost
  *writes* (`history.rs:266-270` vs `msg.rs:506`), and Home renders that
  counter as "the daemon dropped requests — this is not the whole story".
  §1's fix broke §3's honesty guarantee.
- **F5** — `GetStatus` is sent once at window construction (`window.rs:146`)
  and the daemon never broadcasts `Status` (`hub.rs:319`), so the
  disabled/degraded state never refreshes. §3 required it on Home open.
- **F8** — Shaman's accent gives 4.33:1 on-accent contrast, below WCAG AA
  4.5:1, and `every_class_is_legible` asserts only `>= 3.5` so it does not
  catch it.

**Minor**

- **F3** — the cap bounds cards, not bytes; a pathological many-player card
  can still exceed `MAX_FRAME`. `wire::frame` is still only a
  `debug_assert!`.
- **F2b** — `a_flood_of_reads_cannot_drop_a_store` proves 3 writes fit, not 32;
  it does not pin the reserved half.
- **F6a** — "median" is `values[len/2]`, not a true even-count median.
- **F6b** — Characters omits the `history_characters` config union §4 asked for.
- **F7** — `ctrl +/-/0` are not swallowed by the focused filter.
- **F9** — four tests would survive stubbing (listed above).
- **F10** — two pre-existing env/`ETXTBSY` test-binary races, one of which I
  reproduced; separate ticket.

## The one thing to fix before merge

**Finding 8** — raise `every_class_is_legible`'s floor from 3.5 to WCAG AA's
4.5 and darken the Shaman chrome accent until it passes. It is the only
finding where the branch ships a *tested* claim ("every class is legible")
that is measurably false, and legibility of the class accent is the whole
premise of `theme.rs`. Finding 2 (the `dropped` counter conflation) is a close
second and is the more insidious of the two, because it makes a data-loss
warning fire on a scroll.
