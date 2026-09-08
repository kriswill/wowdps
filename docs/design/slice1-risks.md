# Slice 1 — adversarial risk review

Scope reviewed: the "no wire change" slice — spec-accented theme/token module;
nav shell (icon tab bar, jump chips, two-tone title, stat cards); a client-side
Home dashboard built from `HistoryQuery::Fights` answers; the `?` shortcut
sheet; a row filter; `~` to Home with `Esc` walking one level up.

Sources read: `docs/design/gui-window-features.html` (§2, Layout A, the view
map), `CLAUDE.md`, `CONTRACT.md`, `crates/gui/src/*`, `crates/proto/src/{state,
msg,history,client,wire}.rs`, `crates/daemon/src/{history,hub,config}.rs`,
`crates/model/src/lib.rs`, `crates/tui/tests/keybind_parity.rs`.

Severity counts: **7 blocker · 12 serious · 6 minor** (25 findings).

---

## A. Data honesty — every number on Layout A

The reference shapes are `FightCard` (`crates/proto/src/history.rs:262-305`),
`CardPlayer` (`:184-257`), `KeyInfo` (`:172-179`), `KeyBoss` (`:309-315`) and
`HistoryAnswer::Fights { cards, total }` (`crates/proto/src/msg.rs:334-338`).

### 1. `season score 2 748` is invented. **BLOCKER**

Evidence: no rating/score field exists anywhere on `FightCard` or `CardPlayer`
(`history.rs:184-305`). Mythic+ rating is a Blizzard server-side number; the
combat log never carries it and `crates/core` never derives one. The same
number reappears per-character in panel 6 ("Characters … 2 748 / 2 431 / …").

This is exactly the failure mode CLAUDE.md/CONTRACT.md are built against: a
plausible-looking number with no provenance. Any client-side "score" we invent
will be compared by the user against the in-game score and will be wrong.

Mitigation: delete the card, or replace it with something the store actually
knows and label it as ours (e.g. "keys logged: 74" or "median key level: +12").
Never call it "season score".

### 2. `raid bosses 7 / 8` — the denominator does not exist. **BLOCKER**

Evidence: cards carry `encounter: Option<Encounter>` = `{id, difficulty,
group_size}` (`model/src/lib.rs:82-86`) for bosses you *fought*. There is no
raid→boss-roster table in `model`, `proto` or `core`; `difficulty_name`
(`model/src/lib.rs:91`) is the only encounter-side lookup that exists. "N of 8"
requires a hand-maintained per-tier encounter list that nothing in the repo has
and no generator produces.

Mitigation: render "7 bosses killed this tier" (numerator only), or ship a
generated encounter→instance table as its own change (a `tools/gen-*.sh` in the
style of `keystone_timers.rs`) before this card exists. Do not hardcode 8.

### 3. `62%` best-pull percentage — real, but only for raid bosses and only on
post-R16 cards. **SERIOUS**

Evidence: `FightCard::best_pct`'s doc comment says "Reserved for ruling R16 …
never written yet" (`history.rs:298-299`) — **the comment is stale**: the daemon
does write it (`daemon/src/history.rs:2630 best_pct: seg.best_pct()`), and
`meter.rs:905` implements it. But `meter.rs:362` notes the health tracking is
"Empty off raid bosses", so keys and dungeons are always `None`, and every card
written before R16 landed is `None` too.

Mitigation: render the row without a percentage when `best_pct` is `None`
(blank, not "0%" and not "100%"). Also fix the stale doc comment in the same
change.

### 4. `rank in role #3 of 9` — the grader lives in a crate the GUI cannot
depend on. **SERIOUS**

Evidence: role-relative ranking is `crates/mcp/src/grade.rs:111 pub fn
grade(card, guid) -> Option<Grade>` with `rank: Option<usize>` (`:79`, computed
at `:167`). `crates/gui/Cargo.toml` depends on `wowdps-model` and
`wowdps-proto` only, deliberately ("A pure client: model + proto only"). The
GUI cannot call `grade`.

Also note the semantic mismatch: `grade`'s `rank` is *within one fight's
roster* ("#3 of the 9 DPS on that pull"), not a season standing. Layout A puts
it in the "Me · season" card, where a reader will read it as a season rank.

Mitigation: move `grade.rs` into `wowdps-proto` (it is stdlib-only, so the
dependency policy allows it) and have mcp re-export it — one shared grader, one
definition. Then label the Home card "median rank in role, this season" and
compute it as a median of per-fight ranks, or drop the card from slice 1.

### 5. `78th pct` / per-role percentile. **SERIOUS (honesty)**

The doc itself concedes "Percentile is against your own history, stated as
such — we have no ladder". That is the right instinct, but a card reading
"78th pct" next to "dps · Tranqster" will be read as a ladder percentile by
every user, including the one who wrote the doc, six months from now.

Mitigation: word it as the comparison it is — "better than 78% of your own
pulls this season" — or cut the panel. A one-word label will not survive.

### 6. Per-dungeon "best level + N runs · M timed" — derivable, with two
caveats. **MINOR**

Derivable: `kind == FightKind::Key`, `key.level`, `pars_ms` and `official_ms`
are all on the card (`history.rs:271,281-283`); "timed" is
`official_ms <= pars_ms.0`, which is R10's own definition, not a guess.

Caveats: (a) `key.level` is `Option` ("`None` on an unkeyed visit",
`history.rs:176`) — those runs must be excluded, not counted as level 0;
(b) `pars_ms` is `Option` and `keystone_timers::pars_ms` is `pub(crate)` in
core (`core/src/keystone_timers.rs:12`), so a card written for a dungeon the
generated table did not know has no par at all and its timed verdict is
**unknown**, not "over". Render it as a dash.

Dungeon *name*: `card.name` is `seg.name` (`daemon/src/history.rs:2609`), so
names come from the log, fine — but `KeyInfo.map_id` has no name table, so any
grouping must key on `map_id` and display `name`, and two logs that spell a
zone differently will split a row.

### 7. `Ulgrax the Devourer M · 4:12` — derivable. **OK**

`encounter.difficulty` → `difficulty_name` (`model/src/lib.rs:91`), `success`,
`duration_ms`, all on the card.

### 8. `this week 18 pulls · 2 kills` — derivable client-side. **MINOR**

`aborted` must be excluded (the field's own doc: "listed, never counted as a
pull", `history.rs:286-288`). Week bucketing must be done by the client; note
the daemon's own `Progression`/`Trend` bucketing takes a `local_cutover_hour`
so a raid night never straddles midnight (`msg.rs:259-262`). A naive
client-side UTC week will disagree with the daemon's own answer for the same
data — two numbers, one truth. Reuse `bucket_start`'s rule or accept the drift
explicitly.

### 9. `median / best effective dps` — derivable. **OK**

`CardPlayer::effective_dps(duration_ms)` and `effective()` are in *proto*
(`history.rs:514,526`), reachable from the GUI. `role()` (`:481`) too. Good.

### 10. `deaths / pull 0.4` — derivable. **OK** (exclude `aborted`).

### 11. `fights stored 1 284` — derivable, but not from `Fights`. **MINOR**

`HistoryAnswer::Fights` carries `total` (matches before limit), which is the
count *after filters* — usable. The honest global number is
`HistoryStatus::fights` (`msg.rs:505`), which arrives only in `DaemonMsg::
Status`, and `ClientState` throws `Status` away (`state.rs:706`) and the GUI
never sends `GetStatus`. See finding 20.

### 12. `best week +14% dps` — derivable, but not from a `Fights` answer alone.
**MINOR**

Doable client-side by bucketing cards, but the daemon already answers exactly
this as `HistoryQuery::Trend { bucket: Week, measure: EffectiveDps }`
(`msg.rs:270-278`). Two implementations of "week" will drift (see finding 8).
"+14%" also needs a stated baseline — vs the previous week? vs the season
median? The mockup does not say, which means the implementer will pick one.

### 13. The character list is **not** derivable from `Fights`. **BLOCKER**

Evidence: a card names exactly one `owner: Option<String>`
(`history.rs:292-293`), filled in at answer time from the store's single
`self.owner()` (`daemon/src/history.rs:1733-1741`). `CardPlayer` has no
"is me" flag — `logged` means "a COMBATANT_INFO named this player"
(`history.rs:192-194`), which is true for all 20 raiders. So "the four
characters you play" cannot be recovered: every card either names the one
current owner, or the GUI would have to guess from the 20-player roster.

The daemon *does* know: `Config::history_characters` is a `Vec<String>` of
"Name-Realm" (`daemon/src/config.rs:33`). It is not on the wire.

Partial mitigation that keeps "no wire change": the GUI's own `Config` has
`#[serde(flatten)] extra: toml::Table` (`gui/src/config.rs:65`) holding every
key it does not own — including `history_characters`. The GUI can read the
names out of `extra` and match them against `CardPlayer::name`. That is a
back-channel through a shared config file, it breaks if the user never set
`history_characters` (the inference path — `HistoryStatus::owner_inferred`),
and it matches by name not guid. Workable for a first cut; say so out loud in
the code, or cut the panel and scope Home to the one owner.

### 14. Season scoping is fine. **OK**

`HistoryQuery::Fights { since_utc_ms }` exists (`msg.rs:243`). A season as a
named date range in config needs no wire change — see finding 18 for the
config-file trap.

### 15. `daemon live · 3 clients` in the Home chrome. **SERIOUS**

Needs `DaemonMsg::Status`, which the GUI neither requests nor keeps
(`state.rs:706`, and `ClientState::initial_request` is just the Watch,
`state.rs:117-119`). This is new client plumbing outside `ClientState` — the
same shape as the v19 `GetLoadout`/`pending_loadout` pair in `window.rs:106-108`.
Not a wire change, but not free either.

### 16. `Recent · last 24 h` with pin markers. **OK**

`Fights { since_utc_ms, sort: Newest }` plus `card.pinned` (`history.rs:297`).
The one derivable-and-honest panel on the page.

---

## B. Cost

### 17. An unbounded `Fights` answer can exceed `MAX_FRAME` and kill the
session in a reconnect loop. **BLOCKER**

Evidence chain:

- `HistoryQuery::Fights.limit: u32` has **no upper cap** daemon-side — only
  `let limit = if limit == 0 { 50 } else { limit as usize }`
  (`daemon/src/history.rs:1725`). A client asking for `u32::MAX` gets the whole
  store.
- Wire size per card: `put_card` (`msg.rs:1450-1492`) + `put_card_player`
  (`:1370-1414`). A `CardPlayer` is ~186 bytes of fixed scalars plus guid and
  name strings — call it ~230 B. A 20-player raid card is therefore ~4.7 KB;
  a 5-player key card ~1.4 KB plus its `bosses[]`.
- A season of ~1 300 cards at a mixed ~8 players/card is **~2.5 MB in one
  frame**; a raid-heavy 1 300 cards is ~6 MB; ~3 500 raid cards crosses
  **`MAX_FRAME = 16 MiB`** (`wire.rs:11`).
- Over the cap, the send side only `debug_assert!`s (`wire.rs:197`) — in a
  release build it emits the oversized frame anyway. The receiving reader
  rejects it (`wire.rs:236`, `read_frame` → `InvalidData`), the client reader
  thread breaks out of its loop and sets `disconnected = true`
  (`client.rs:295-306`), `reconnect_if_dead` respawns and re-declares the watch
  (`client.rs:242-259`) — and Home asks again. **Infinite reconnect loop, no
  error message.**

Mitigation (all three): cap `limit` daemon-side (e.g. 500 per page, and clamp
rather than truncate silently), make Home page with `after_id` instead of
asking for everything, and turn `wire::frame`'s `debug_assert` into a real
refusal so an oversized frame can never be written.

### 18. Home queries share one bounded 64-slot queue with live fight writes —
a Home refresh can lose a real fight. **BLOCKER**

Evidence: `HistoryReq` carries both `Store(Box<ClosedFight>)` and
`Query { .. }` on the same channel (`daemon/src/history.rs:132-155`), the
channel is `sync_channel(QUEUE)` with `QUEUE = 64` (`:50, :275`), and
`HistoryLink::send` is a deliberately lossy `try_send` that increments
`dropped` and hands the request back on a full queue (`:216-227`). One history
thread services it serially, and answering `Fights` **clones every matching
card** (`:1733-1742 .cloned()`).

So: a Home that re-queries on a timer, or on every `HistoryChanged`
(`msg.rs:647`, which fires per closed fight), can keep 64 slots busy while a
pull ends — and the `Store` for that pull is dropped. The fight is never
written to the store. That is silent data loss caused by a read-only dashboard.

Mitigation: Home issues **one** query per explicit open plus a debounced
refresh (≥ 5 s) on `HistoryChanged`, never on the 100 ms `TICK`
(`window.rs:22`). Better still: give queries their own channel, or prioritise
`Store` over `Query` in the history thread's receive loop.

### 19. A disabled store and a cold store are indistinguishable — Home renders
zeros either way. **BLOCKER**

Evidence: when the store is off or the queue is full, `forward_history`
synthesises `HistoryAnswer::Fights { cards: vec![], total: 0 }`
(`daemon/src/hub.rs:148-158`). The client cannot tell "history_enabled = false",
"queue was full, your request was thrown away", and "you have no fights yet"
apart. A dashboard of confident zeros is precisely the dishonest-number failure
this project cares about.

Mitigation: Home must gate on `HistoryStatus` (`enabled`, `error`, `fights`,
`importing`) before rendering any number — which means finding 15's status
plumbing is a *prerequisite*, not a nice-to-have. Render an explicit empty
state ("the history store is disabled — set `history_enabled = true`") rather
than 0.

### 20. History replies are never coalesced. **SERIOUS**

`Inbox::push` coalesces `Snapshot`, `SegmentList` and `CompareSnapshot`; every
other message — including `DaemonMsg::History` — goes to the never-dropped
`control` VecDeque (`client.rs:132-156`). Repeated Home refreshes therefore
*queue*, each holding a full `Vec<FightCard>` in client memory until drained.
Combined with finding 17's multi-megabyte answers, a few stacked refreshes are
tens of megabytes in the inbox.

Mitigation: one in-flight `req_id` at a time (the `pending_loadout` pattern,
`window.rs:106-108`), drop replies whose id is not the current one.

### 21. A large history reply serialises behind / ahead of the 10 Hz snapshot
stream on the session's single writer. **MINOR / suspicion**

I did not read `daemon/src/server.rs`'s writer thread closely enough to state
this as fact. Worth measuring: a ~3 MB frame on the same writer as the meter
snapshots will visibly stutter a live overlay/window during a pull.

---

## C. The "no wire change" claim

### 22. True for the queries; false for the slice. **SERIOUS**

Genuinely needs no new wire message: season scoping (`since_utc_ms`), the
recent panel, the key panel, the raid panel (`Progression` already exists), the
me panel (`Trend` already exists), the filter, the `?` sheet, the theme tokens.

Quietly needs something new:

- **Character list** — finding 13. No wire field says which players are me.
  The config back-channel avoids a wire bump; that is a workaround, not
  "already derivable".
- **Daemon status / store liveness** — findings 15 and 19. No new *message*
  (`GetStatus` exists), but new client plumbing and, arguably, a new
  `ClientState` responsibility since `state.rs:706` currently discards it.
- **`grade`** — finding 4. No wire change, but a crate move.
- **`Screen::Home`** — `Screen` has exactly three variants (`model/src/lib.rs:
  1280-1287`) and is the shared client state machine used by the TUI and the
  overlay. Adding a variant is a model + proto + TUI + overlay change. Keeping
  Home window-local (the `talents` pattern, `window.rs:102-104`) avoids all of
  it and is the right call — but then Home is invisible to `ClientState` and
  `Esc`/`~` must be handled in `window.rs`, not in the state machine.
- **`CONTRACT.md` §"Client state & behavior"** enumerates the keymap by hand
  (`CONTRACT.md:498-516`). `~ m H / ?` are a contract edit in the same PR.

---

## D. iced reality (iced 0.14, `crates/gui/Cargo.toml:24`)

### 23. Gradients are already solved — the doc is over-cautious. **MINOR**

`view.rs:1384-1389` builds an `iced::gradient::Linear` and sets it as a
`container::Style` background. No canvas needed for linear ramps (radial is
another matter, and nothing in Layout A needs one). The two-tone title, the
gradient active tab and the gradient bar fills are all `container` styling.

### 24. There is no `button` widget anywhere in the GUI, and no `text_input::
focus()` call. The filter box will eat keys or lose them. **BLOCKER**

Evidence: `grep` over `crates/gui/src` finds zero uses of `iced::widget::
button`; every interactive surface is a `mouse_area` (`view.rs:7`,
`overlay.rs`, `talents.rs:29`). And nothing calls `text_input::focus` — the
talent viewer's `text_input` (`talents.rs:1271`) is focused by clicking it.

Meanwhile the keymap runs off a **global** `keyboard::listen()` subscription
(`window.rs:434`) whose handler is a flat if/else chain on `modified_key`
(`window.rs:294-345`). So a filter box opened with `/`:

- is not focused, so keystrokes go nowhere visible; and
- even once focused, the global listener still fires — typing "q" in the filter
  quits the app, "d" switches to Damage, "K" jumps to Deaths.

The talent viewer only survives this because `window.rs:309-320` has an
explicit "the viewer swallows the meter keymap" branch.

Mitigation: every new modal (filter, `?` sheet, Home) needs (a) a state flag,
(b) its own early branch in the `window.rs:294` chain that swallows everything
except its own escape hatch, and (c) for the filter, a `Task` issuing
`text_input::focus(id)` on open. Budget this properly — it is the single
easiest way to ship a GUI that quits when you type.

### 25. A sortable header is real work; a tab bar and a modal are not.
**MINOR**

No sortable-table widget exists in iced 0.14; it is `mouse_area(container(
row![...]))` per header cell plus a sort key in `Gui`. The modal sheet is fine:
`stack` is already imported and used (`view.rs:7`). The tab bar is
`row![mouse_area(...)]`. None of this needs a canvas.

### 26. A fixed type scale will break the overlay, which multiplies every size
by its own zoom. **SERIOUS**

Evidence: the overlay imports and calls `view.rs` helpers directly —
`DIM, GREEN, OVERLAY_DRILL_COLS, RED, YELLOW, overlay_drill_row, overlay_row,
recap_row` (`overlay.rs:40-42`) plus `scroll_clear`, `header_tag`,
`spell_breadcrumb`, `spell_stats(r, view, z)`, `spell_target_list`,
`drill_mitigation_line`, `enemy_split`, `team_divider(9.0 * z)`, `rate_label`,
`school_color` (`overlay.rs:374,1477-1889`). Note the `z` threaded through
those signatures: the overlay renders at scale 1.0 and applies its own zoom by
multiplying sizes, deliberately, because iced_layershell 0.19's custom
`scale_factor` breaks pointer hit-testing (`docs/tracing.md:110-120`, bug 2).

A token module exporting absolute px sizes and being applied inside those
shared helpers will therefore be zoom-blind in the overlay. Any token that
touches a shared helper must be a *ratio* the caller multiplies by `z`, or the
helper must take the token set as a parameter — which changes its signature and
every overlay call site.

### 27. The window's theme is a hardcoded `Theme::TokyoNight`
(`window.rs:249-251`). **MINOR**

Fine to keep. Note that `iced::theme::Palette` has only a handful of slots
(background/text/primary/success/danger); a spec accent with `--spec`,
`--spec-2`, on-accent ink and the luminance flip does not fit it. Build the
token set as our own struct (the `inverted_metrics` luminance rule at
`view.rs:1327-1333` is already the right primitive to generalise) and pass it
explicitly, rather than trying to express it as an iced `Theme`.

### 28. The overlay must not grow a Home screen. **MINOR**

`KeyboardInteractivity::None` (per CLAUDE.md) means the overlay has no keys at
all; `~`, `/`, `?` are meaningless there. Keep the nav shell strictly
window-side, and make sure nothing in the shared helpers assumes a tab bar
exists.

---

## E. Keymap collisions

### 29. `~ m H / ?` are all free — and every one of them **fails the keybind
parity test** if added to `gui/src/keys.rs`. **BLOCKER**

Free: neither keymap binds `~`, `m`, `H`, `/` or `?`. `h` is Healing and `T`/
`K` are shifted views (`gui/src/keys.rs:41-58`, `tui/src/keys.rs:11-31`), so
shift-`H` does not collide with `h`.

But `crates/tui/tests/keybind_parity.rs` reads both keymap *source files* and
asserts every GUI char binding is either mirrored in the TUI or is in
`GUI_ONLY_CHARS = [("v","PickCompare"), ("g","ToggleGraph")]` (`:23`, enforced
at `:170-178` with the message "not a contract-sanctioned GUI-only binding
(R12: v/g only)"). Adding `~`/`m`/`H`/`/`/`?` to `keys.rs` breaks
`cargo test -p wowdps-tui` immediately.

Mitigation: bind them **outside `keys.rs`**, in the `window.rs:294` handler,
exactly as lowercase `t` is today (`window.rs:321-324`; `keys.rs`'s own test
asserts `ch("t") == None`, `:96`). The parity test never sees them and the
mirror stays intact. Then update `CONTRACT.md:498-516` to list them as
window-only, the way `v`/`g` are listed.

### 30. `Esc` semantics: safe at the list level, dangerous elsewhere.
**SERIOUS**

`ClientState::apply_list` has **no `Action::Back` arm** (`state.rs:772-789`) —
Esc on the segment list is currently a no-op. So "Esc from the fight list goes
to Home" can be implemented purely window-side without touching the shared
state machine or the TUI. Good.

What is *not* safe: `Action::Back`'s meter behaviour is a documented four-level
walk (`state.rs:855-875`, `CONTRACT.md:516-520`), returned as `ClientMsg`s.
Do not reinterpret it. Home must sit *above* `Screen::List` as a window-local
overlay, and Esc must only be intercepted when `screen == List && !home_open`.

### 31. `~` is not reachable on every keyboard layout. **MINOR / suspicion**

iced delivers `Key::Character("~")`, which on a US layout is shift-backtick. On
layouts where `~` is a dead key (many European ones) the `KeyPressed` may
arrive as the dead key or not at all. I have not tested this.

Mitigation: bind a second, layout-stable key (e.g. `Named::Home`, or `g` is
taken so `Escape`-from-list per finding 30 carries the load), and treat `~` as
the convenience binding.

---

## F. Scope

### 32. This is three PRs, not one. **SERIOUS**

Weighing the findings above: the token module alone touches every shared
`view.rs` helper and the overlay's `z` threading (finding 26). The nav shell
plus the filter plus `?` all require restructuring the `window.rs:294` keyboard
chain and adding focus management (finding 24). Home requires new non-
`ClientState` client plumbing for both `History` and `Status` replies, request
correlation, paging, empty/disabled states, and the grade move — and it is the
half whose *shape* the design doc itself says "will move around before it
settles".

A natural split:

1. **Tokens + two-tone title + spec accent.** Pure rendering, no new state, no
   new keys. Ships visible quality on day one. Must land the `z`-ratio decision
   (finding 26) here, because everything after depends on it.
2. **Nav shell + `?` sheet + filter.** One PR that owns the keyboard
   restructuring, the modal pattern and focus management once, correctly.
3. **Home.** After 1 and 2, and after the status/paging/grade prerequisites are
   in.

---

## Cut list

Remove from slice 1, in order of how much trouble they save:

1. **The season score card and the per-character scores** (findings 1, 13).
   Invented number, and the character list it sits in is not derivable. Cutting
   both removes a blocker and a data-honesty violation at once.
2. **`raid bosses N / 8`** (finding 2). The denominator needs a generated
   encounter roster that does not exist. Ship the numerator or nothing.
3. **The "Season progress by role" percentile panel** (finding 5). No ladder,
   no honest wording that fits in a card.
4. **`rank in role`** (finding 4) until `grade.rs` lives in proto. Duplicating
   the grader in the GUI guarantees the two answers diverge.
5. **The whole Home dashboard from slice 1** (findings 17, 18, 19, 20, 32).
   It is the only piece that can lose a user's fight data (18) or wedge the
   client in a reconnect loop (17), and it depends on status plumbing (19) that
   is itself unbuilt. Land the theme and the nav shell first; Home earns its
   own PR with paging, debouncing, an in-flight `req_id`, a daemon-side `limit`
   cap and a real empty/disabled state.
6. **`~` as the only Home binding** (finding 31). Keep it, but do not rely on
   it alone.

What I would keep in slice 1: the token module (with `z`-aware ratios), the
two-tone title, the spec accent and gradient bars, the icon tab bar, the `?`
sheet, and the filter — provided the keyboard restructuring in finding 24 is
budgeted as real work rather than assumed.

## Biggest single blocker

**Finding 18** — Home's history queries share one bounded, deliberately lossy
64-slot channel with the live `Store` writes (`daemon/src/history.rs:50,132-155,
216-227,275`). A read-only dashboard that refreshes while a pull ends can cause
that pull to never be written to the history store. A dashboard about your
fights must not be able to delete one.
