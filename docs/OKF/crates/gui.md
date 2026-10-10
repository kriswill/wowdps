---
type: Crate
title: wowdps-gui
description: 'The wowdps GUI on Zed''s GPUI through GPUI Kit — the meter window and the wlr-layer-shell overlay, a pure client of model, proto and gui-logic; built as crates/gui-new beside the iced GUI, it took this crate''s place at the cutover.'
resource: crates/gui
tags: [crate, gui]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T17:30:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: The GUI's specification — §3 platform, §6 architecture and themes, §9 testing, §10 the cutover
  - id: plan
    resource: ../../plan-gui-new.md
    title: The implementation plan — phases 1–5 and each step's As-built note
  - id: gpui
    resource: ../../gpui/README.md
    title: The GPUI reference — which GPUI, findings checked against the 0.3.7 sources
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Shots, the render guard and the chrome budget
---

A pure rendering client of the daemon, drawn as the meter window or, with
`--overlay`, a layer surface over the game. Like every frontend it depends
on [`wowdps-model`](model.md) and [`wowdps-proto`](proto.md), plus the
framework-free [`wowdps-gui-logic`](gui-logic.md), never the engine, so it
cannot parse a log. It was rebuilt on GPUI as `crates/gui-new`
([the deprecated record](gui-new.md)) beside the iced GUI that held this
name, reproducing every overlay guard state and design-shot state, and took
the name at the cutover[^plan] — [Rebuild The GUI On GPUI Kit](../decisions/gui-on-gpui.md),
[No Forked Or Patched GPUI](../decisions/no-gpui-forks.md). The daemon's
overlay supervisor spawns it by config `gui_binary`, default `wowdps-gui`
([wowdps-daemon](daemon.md)).

## Source

- Manifest: [`crates/gui/Cargo.toml`](../../../crates/gui/Cargo.toml)
- Root: [`crates/gui/src/main.rs`](../../../crates/gui/src/main.rs)
- Binaries: `wowdps-gui` (`wowdps gui` through the dispatcher)
- Design record: [`docs/design/window-redesign.html`](../../design/window-redesign.html) — the prototype whose Tokens, sizes and behavior the window implements
- Review loop: [`crates/gui/SHOTS.md`](../../../crates/gui/SHOTS.md) — the shot tests and the overlay render guard

## Seams

**`Session`** (`session.rs`) is one daemon link and its `ClientState` as an
entity; `pump` is all of its work — reconnect without waiting, drain,
apply, send what the state asks for, and notify on any non-empty drain
(not on `snapshot_gen`, which `CompareSnapshot` and `SegmentList` never
move). The app runs it every 100 ms; the link is a trait, so tests drive the
daemon's mock through the same `pump` with no timer[^spec]. One-shot
answers (`Loadout`, `History`, `Fight`, `HistoryChanged`) come back as
`Reply` events to whoever asked. `main.rs` makes the link before GPUI
starts, because `DaemonClient::connect` may spawn a daemon and wait.

**Keys are GPUI actions in contexts.** [gui-logic](gui-logic.md)'s chord
table, which [the TUI's parity test](tui.md) iterates, is registered once:
every meter action rides one `Do(Action)` on `Meter && !Input`, so no meter
key fires while a text field has the keys; the window-local gestures are
`Go(Gesture)`; menus switch the root to `Modal`, and a field's own keys bind
on `Filter > Input` or `Palette > Input`.

**One theme definition at a time.** gui-logic's `theme::Def` (`navy`, `onyx`,
`frost`, or a config's own — [themes as config data](../decisions/themes-as-config-data.md))
feeds Kit's `Theme` slot by slot and the app's `Look`, so no surface draws a
literal color, face, corner or effect — [the Look decision](../decisions/gold-chrome-and-a-look-per-surface.md)
generalized; the ⚙ card switches it and the overlay follows the config; class chrome is an accent over any theme
([one accent](../decisions/one-accent-from-the-class-color.md)). Kit's
styled `Input` decides its own placeholder color and text size, so the
palette and the row filter use `window/field.rs`, Kit's unstyled input
dressed in the window's tokens.

**The window** (`window.rs` and `window/`) is one entity, `Gui`, over the
fight on the stage (`Gui::fight` — the log's `ClientState` or a stored
pull's own, fed from `GetFight` — with every gesture through `act`). The
redesign's surfaces are all here, each on its decision: the top bar and one
pull rail ([the Rail decision](../decisions/one-pull-rail.md)), one fight
header over the meter ([the Header decision](../decisions/one-fight-header-over-the-meter.md)),
the ribbon and "you" off the daemon's `mine`
([the Wire decision](../decisions/raid-timeline-and-mine-on-the-wire.md),
[R25](../rulings/r25.md)), one column model for every row list
([One Table](../decisions/one-table-for-every-row-list.md)), the inspector
beside the meter ([the Inspector decision](../decisions/inspector-beside-the-meter.md))
(widened over the stage, its graph's window scoping its lists —
[the Window decision](../decisions/a-window-scopes-the-drill.md))
with [R26](../rulings/r26.md)'s ability tree and stacked graph
([the Ability Tree decision](../decisions/ability-tree-in-the-inspector.md)),
Home derived from `Fights` answers ([decision](../decisions/home-derives-from-fights.md)),
and the command palette and cards ([the Redesign decision](../decisions/window-redesign.md)).
The inspector is a pure model built once a frame (`window/inspector/model.rs`)
and drawn by its views; its graph, matrices and chips are components that
bring only a text measure and their paint to gui-logic's geometry. GPUI has
no app scale factor, so every window size goes through `w.z(…)`.

**The overlay** (`overlay.rs`, `overlay/`) draws what the iced overlay drew
in the theme's overlay palette. It opens through GPUI's `cx.open_window`,
never Kit's Root (which adds a 20 px shadow inset on a layer surface)[^gpui].
gpui-pre cannot move a layer surface after creation, so the surface spans
its whole edge, the content sits at an offset inside it, and the input
region is exactly the content; "hidden" is a 1 px strip with an empty
region. It never asks for a zero length (a viewport protocol error under
GPUI), and its output is the game's monitor, found by recomputing GPUI's
UUIDv5 of the output name.

**Animation is finite.** GPUI's animation frame re-renders the whole view,
and the window is one: an endless pulse idled it at 5.7 % of a core, three
pulses per live pull at 0.4 %. Every delight (bars easing, the tab
underline, a zoom gliding, a card's entrance, the talent ripples) settles
to the reviewed pixels under `cx.reduce_motion()`, which every shot test
sets.

**Reviewed headless.** Kit's `TestWindowExt` clicks through the real views
over the daemon's mock on every `cargo test`; tests that depend on text
metrics run in a `HeadlessAppContext` with the real cosmic-text system,
which needs a wgpu adapter (the GPU, or Mesa's lavapipe in CI and the nix
sandbox). Ignored shot tests render each surface at the prototype's sizes,
and `overlay_render_guard` compares the overlay's states, rendered without
the art caches, with committed PNGs within a tolerance[^shots].

**Libraries.** It links libxkbcommon(-x11), libxcb and fontconfig, and
dlopens wayland-client, vulkan and EGL; `build.rs` bakes the dev shell's
`LD_LIBRARY_PATH` into the RUNPATH, and the flake's `.#wowdps-gui` (its
own crane dependency layer) runs through a wrapper, which both modules put
on the daemon's `PATH`.


## As built, surface by surface

What follows is the GUI as built, surface by surface: the record agents
once carried in their instructions, kept here so the instructions can stay
rules. Paths are under `crates/gui/src/`; a module called gui-logic's is
under `crates/gui-logic/src/`. The prototype
(`docs/design/window-redesign.html`) and the decisions linked above are the
why; this is the what.

## The two frontends

The **window** (`window.rs`, its pieces under `window/`) and the
**overlay** (`overlay.rs` and `overlay/`, `--overlay`) are two frontends,
each over its own `ClientState` held in a `Session` (above) — thin clients
like the TUI. `pump` runs on a 100 ms `TICK`, with `GetStatus` once a
second because the daemon never broadcasts it; tests call `pump`
themselves and run no timer, so the executor parks. Every UI change goes
through `Session::act`, and a one-shot answer (`Loadout`, `History`,
`Fight`, `HistoryChanged`) comes back as a `Reply` event, matched by
`req_id`, to whoever asked through `Session::request`. `main.rs` makes the
daemon link BEFORE GPUI starts (`DaemonClient::connect` may spawn a daemon
and wait, a wait no frame may take), registers the keymap and the bundled
faces, applies the theme and opens one surface.

Config lives at `~/.config/wowdps/config.toml`
(`crates/gui-logic/src/config.rs`, the one writer: every save atomic, and a
gesture's key written alone through `Config::store` / `store_character` /
`store_character_class`, never a launch-time copy over an overlay drag).
The two surfaces share no renderer: the overlay draws the iced overlay's
pixels in the theme's overlay palette, the window the redesign's Tokens
(*Chrome and type*, below).

The design record is `docs/design/window-redesign.html`: an interactive
prototype built from a real Heroic Coiled Altar kill (25 players, six
deaths, one Heroism), the critique that drove it (ten findings ranked by
what each costs after a pull), the six-step build plan (Look, Header,
Inspector, Rail, Wire, Home) and the Tokens section every window color and
size comes from. Its CSS values and JS behavior are the spec — window code
cites its selectors (`.fhead`, `.rail`, `.insp`, `.pal` …) — and each
step's rationale is a decision in this bundle
([window-redesign](../decisions/window-redesign.md) links them all).
Reference renders of the prototype live outside the repository, under
`~/.local/share/wowdps/design-shots/reference/` (`wide-*` 1440×900,
`tile-*` 960×880, `narrow-*` 460×860, captured at 1.25×).

## Keys, mirrored from the TUI

The TUI (`ui.rs` renders `ClientState`, TestBackend tests against
`daemon::mock`; `tests/no_engine.rs` greps that tui sources never name engine
modules) keeps every pre-redesign semantic: each `ClientState` capability the
window added is OPT-IN (`set_follow`, `open_death`, `select_player`) and the
TUI calls none. GUI keybinds mirror the TUI's through ONE chord table,
gui-logic's `keys::ACTIONS` (`Chord` → `Action`), and
`crates/tui/tests/keybind_parity.rs` iterates that table against the TUI's
match arms and fails on a binding the TUI lacks. The GUI registers the table
as GPUI `KeyBinding`s once at start (`crates/gui/src/keys.rs`): every meter
action rides ONE action, `keys::Do(Action)`, in the `Meter && !Input` context
— GPUI's `!` reads the whole context stack, so no meter key fires while a
text field has the keys — and the zoom chords ride `ZoomTo`, window-wide. A
typed character binds as itself (`?`, `~`, `+`; `K` is Shift+K). Every
window-only gesture (`t`, `p`, Ctrl K, `/`, `m`, `~`, `H`, `?`, `f`) is a
`keys::Go(Gesture)` the window answers itself, never through the table, and
is listed in `keys::BINDINGS` with `window_local: true`. Two window-local rows
are extra READINGS of keys the table also answers: ← → (older/newer segment
there; the window's second meaning steps a recap's deaths) and the Deaths
table's `j`/`k` (down/up there; the window walks the deaths in order, each
step its recap). The window's other key contexts: `Modal` (a menu is up),
`Filter` (the row filter), `Palette` (the command palette) and `Talents` (the
talent viewer), each below.

## Comparison (R12)

R12's comparison is the GUI's alone. The pair is `ClientState`'s (at most two
picked players, a third replaces the older). The OVERLAY draws it as
`Screen::Compare` (`overlay/panel/compare.rs`): clicking a meter row's class
icon picks that player, the second pick opens two per-spell tables (hits /
crit% / average) each over a timeline graph (`overlay/graph.rs`, from
gui-logic's `graph` model) — rolling DPS or cumulative (a click: the overlay
has no keyboard), with vertical bars for trinket uses, trinket procs and
consumables — the surface grown to `COMPARE_MIN` while comparing. The WINDOW
draws the same pair inside its inspector (below): `v` or a class icon pins,
the next move makes the pair. Either way both curves share one y-scale and
one x-range — per-side scaling would make every pair look identical, which is
the one thing a comparison must not do. v29: a comparison is about the VIEW it
was opened from (`Cursor::Compare.view`, echoed by `CompareSnapshot` and read
back as `ClientState::compare_view`) — on Taken the tables list the abilities
that HIT them ("hit by"), the header's rate is dtps, the curve is
`Segment::taken_timeline` (so consumables, externals and mitigation spans
still mark it) and each side's R17 mitigation record sits under its table;
the view keys re-ask for the same pair. The drag-zoom window scopes the
tables on Damage and (v38) Healing — `Segment::spells_in` reads their sparse
per-spell series — and the daemon echoes `range: None` on other views rather
than pairing a zoomed graph with full-fight tables. Hovering a spell row lights the SAME ability in BOTH lists
(`Overlay::spell_hover`, the window's `InspState::spell_hover`), matched by
by-spell KEY because the two lists are sorted independently. Every list that
answers the mouse wears its surface's one hover fill (the window's `hover`
token), fainter than the selection's mark and borderless, so a hover can sit
on the selected row without arguing with it. The window keeps the pointer's
position in `Gui::row_hover` (`RowHover::Meter` / `Death(i)` on the Deaths
table) and the inspector's in `InspState::hover` (pane, line — a pair draws
two lists side by side, so the pane is part of the answer); it is drawn and
never sent anywhere.

## The window's layout

A top bar over a body: the pull rail beside (or, at
1180 px and under, over) either Home or the STAGE — a pull's fight header,
ribbon, view tabs, meter and the inspector beside it. `Gui` (`window.rs`) is
the root entity: the log's `Session`, the place on show, the open cards, the
filter and sort, the pointer, the inspector's state and the history reads.
The breakpoints are the prototype's, gui-logic's `theme::TILE_WINDOW` (1180)
and `theme::NARROW_WINDOW` (820), inclusive at both and read at zoom 1
(`Gui::width`, as the last frame laid it out; `Fit::of`). GPUI has no app
scale factor, so the window zooms by hand: every window renderer draws
through `window/w.rs`'s `W` — the tokens, type scale, pitches, accent, faces,
the zoom (a size is `w.z(…)`) and the width — and Ctrl = / Ctrl - / Ctrl 0
step 0.1 over 0.5–3.0, written alone through `Config::store`. A pull is
drawn from the stage's `ClientState` — `Gui::fight()`, the tailed log's or a
stored pull's own, and `Gui::act` acts on it — so one set of renderers draws
both. Home, the talent viewer, the command palette, the `?` sheet and the
rail are WINDOW-LOCAL: no `Screen` variant, so `ClientState` (and with it the
TUI and the overlay) never learns they exist.

## Top bar

The top bar (`window/top_bar.rs`, the prototype's `.top`): the wordmark (in
Marcellus), two places — Home and Fights, the active one underlined in the
accent — the jump box ("Jump to a pull, player or view": a press, like Ctrl K
from anywhere, opens the command palette), the live pill (the log's newest
pull, "Live, Trash 11:43" with a red dot while it goes — the dot pulses three
times per live pull, never forever — "Latest, …" with a ring once over; a
press pins it as `m` does), the character picker, the gear (the ⚙ options
card: row ranks, hide realm names, the chrome) and help (`?`); at 820 px and
under the wordmark goes, the jump box is a glyph, the pill keeps its dot and
clock and the picker its icon. The picker is a spec icon + the
class-colored name of the character picked — config `character`, Home's
scope — else of the character played last (`Gui::picked`; whose window it is
stays `Gui::played`), opening the character menu (`window/cards/menu.rs`) at
the window root (hover per row, `hide_realms` honored): a character row
picks them and scopes Home, and while one is picked a "Follow the character
I'm playing" item over the rows lets the pick go (Home, when up, on every
character; iced's picker named only the character played, which read as
stuck on a pick). The frame recites no keys:
the footer carries only the daemon's status line, and the `?` sheet
(`window/cards/sheet.rs`) is keyed on the surface (gui-logic's
`keys::Surface::of`, from the window-local screens first, through
`Gui::surface`): it lists every binding that works on this surface in the
prototype's four groups — Move, Views, Inspector, Go to — and dims the keys
the pull on the stage cannot answer (`Gui::inert_keys`). Its keycaps are the
window's own `kbd`, not Kit's `Kbd`, which capitalizes a single letter (`j`
and `J` would read alike).

## Pull rail and stored pulls

The pull rail (`window/rail.rs`, its model gui-logic's `rail`): the window
draws no fight list and no History screen — ONE PULL RAIL lists tonight's log
and every stored night, beside Home or the stage, 236 px at the left above
1180 px (`RAIL_W`), and at 1180 and under a 280 px drawer over a scrim
(`DRAWER_W`; the fight header's list button or `H` opens it; the scrim, Esc —
before anything under it —, Enter or a pick closes it; while it is open
`Gui::rail_key` takes the keys first: j, k and the arrows walk a highlight
over its rows for Enter to open, and `[` `]` step the stage leaving it open
on the row they reach — the `?` sheet's "pull list" surface). Nights by their
LOCAL date with a 06:00 cutover so a raid past midnight is one night
("Tonight" for the night it is now, in the store's newest card's timezone, or
any night with a pull still going; else "Saturday, Sep 26"), then visits (an
instance and its difficulty wearing the logger's class dot; a night's keys
and dungeon runs as one "Mythic+ keys" visit, a key's zone-in visit left out;
a delve; a raid visit's stored Σ), then pulls newest first with the visit's Σ
("Whole visit") last: ✓ kill/timed, ✕ wipe/over, a dash for trash, Σ, a red
dot while live, a ★ in the gutter on a pinned card, and at the right a wipe's
best %, a key's +N or "over", a character dot where one visit holds several
characters, the duration; a "Hide trash" toggle; "Show older nights" pages
the store. A step leaves 40 px under the row it reaches, and the drawer
opening stands the night's heading at the top. Tonight is the daemon's
`SegmentList` grouped by `timeline::blocks` (a segment it filed under no
visit but inside one's span is that visit's); earlier nights are
`HistoryQuery::Fights` pages (gui-logic's `history::Earlier`: every
character's, `home::PAGE` cards — under the store's `FIGHTS_CAP` — newest
first, one request in flight, the newest 20 cards re-asked and merged after
`HistoryChanged`; a refused read — no cards and a total of 0 while cards are
in hand — is asked again after a pause and never taken for the store's word).
`window/history.rs` routes every read (`Hist`: the rail's `Earlier` pages, a
stored pull, Home's week, the store's state from `Status`, a 250 ms tick that
re-asks a refused read). A stored card of the tailed log that its list does
not hold joins the log's visit it followed; a card with no visit (an arena,
the open world) joins no raid visit; a raid pull no visit claims is named for
its instance (the log's visit that night at its difficulty, else any Σ card
of its map), "Raid, Heroic" only when nothing names it. A stored card that is
also a segment of the tailed log (`ClientState::log_id` + the row's
`start_ms` is its fight id, a Σ with its mark) is listed once, as the log's,
lending it the card's best % and owner — and Home's links to it open the
log's segment. `[` `]` (and ← →) walk the rail's drawn order, stored nights
included — past the last card in hand `[` asks for the next page — as the
header's ‹ › do; a move keeps the view and the inspected player. `m` pins the
log's live pull; the window never shows the `ClientState` List screen (the
TUI still does): with no pull on the stage the log's newest takes it. A
STORED PULL opens in the same workspace (gui-logic's `history::Stored`): a
`ClientState` of the pull's own, fed synthetic snapshots built from
`GetFight` answers, whose every `Watch` becomes the `GetFight` for its view,
drill and death window — one read in flight for the whole window, a pull
stepped onto while the last one's read is out waiting for its answer, and an
empty answer asked again after a pause before the pull is called gone; its
header names its night when it is not tonight's, shows the card's wipe % and
a ★ when pinned, and its "you" is the card's owner. v42: a stored pull is
KEPT WHOLE where the store kept its tiers — every boss kill, timed key and
pinned pull (`Retention::keeps_whole`, never demoted or evicted) — so its
drill stacks by ability, Enter opens an ability (its curve, its targets,
their stack: `GetFight.spell`) and `v` compares (`GetFight.pair`, answered
as `StoredFight.pair` and fed to its state as a `CompareSnapshot`), each
byte for byte the live answer over the same seconds (`tests/series.rs`).
What the store keeps no answer for is refused with a toast rather than
asked (gui-logic's `toast::stored_refusal`, over the last answer's
`history::Kept`, everything offered before the first answer —
`Stored::offered`): a new pin where it kept the rows alone (`v` always
lets a pin go), an ability opened where it kept no ability's targets (a
pre-v42 file whose log is gone, `StoredFight::abilities`), and the
enemies' view anywhere; a drill the store kept no breakdown for says so in
the inspector. `p` pins or lets go the stage's
stored card (window-local; a log pull's too once the rail holds its card).
`Hist::owner` holds whose window it is — the character the store's newest
card was played on, as Home's answers (else the rail's pages) name them;
never Home's scope.

## Fight header and ribbon

The window's meter wears ONE fight header
(`window/fight_head.rs` over gui-logic's `fight_head` words, window-only —
the overlay keeps its instance strip): a title line (led by the rail's list
button where the rail is a drawer; the encounter in Marcellus, difficulty and
size, the outcome badge, the duration, ‹ older / › newer pull buttons) and a
stat line of the view's raid figures as label/value pairs with full commas,
ending in the owner's "you" chip (their place in their ROLE on Damage, the
rate or count elsewhere; "died 5:45" on Deaths; a press selects their row,
and a filter that hid them gives way, its field's text with it) — with
"Deaths 6" on the Damage and Enemies lines and "First 1:10", "Battle rezzes
2" on Deaths, all from the snapshot's raid timeline — and, on a keystone
run's Σ, "Run dps" after the raid rate (the fold over the key timer; every
row rates over combat time, as the game's meter does) and "In combat 26:50"
last (v40, gui-logic's `fight_head::RunClocks`). Wide, the stat line is
one line; narrower, it wraps. The owner is the row the DAEMON marked `mine`
(v35, `Gui::owner_of`: its owner resolution — the addon's own characters,
every card's owner, the configured names — so an alt is "you" too and the
window matches no names; the character played last, then
`history_characters`, only when nothing is marked). While a view's answer is
on its way (`ClientState::view_answered`, a loading placeholder) the stat
line says nothing rather than a zero. Between the stat line and the tabs the
**ribbon** (`window/ribbon.rs`, one canvas, 86 px, 74 narrow, its arithmetic
gui-logic's `ribbon`) draws the raid timeline (R25): the view's raid rate in
10 s steps (finer in a short pull) as an ink area under a Catmull-Rom line,
"Raid dps, peak 10.7M", minute ticks, the lust as a faint wash (its name in
the tooltip while the pointer is in it — a word on the ribbon hid the crest
or was struck out by the curve), a skull per death in the class color on a
hairline (`death_line`; "you" over the owner's; an arena enemy's in
outline), labels set on plates where they fit, an accent crosshair that
glows, with a dot where it meets the curve, and a tooltip on hover; the
pointer is read through the canvas's painted bounds, and a press on a skull
is `Gui::open_death` → `ClientState::open_death` (opt-in: the Deaths view
drilled into that death window, pushed in a narrow window). A stored pull
wears the store's rebuild of its timeline (R25 STORED: a 1 s series from the
details tier, the coarse 10 s one, or none once the details are demoted — the
axis, the lust and the deaths alone); only a card-only answer has no ribbon.
The ribbon and the inspector's graph span ONE axis, gui-logic's
`ribbon::fight_span`: a pull's `duration_ms` (its wall clock) or further
where its data runs; a visit's Σ spans what it holds alone, since its
`duration_ms` is no wall clock (a raid's Σ of members' combat, a key's
timer with its death penalties — 35:24 on a run that ended 31:32 in), and
falls back to that clock only with no series to measure. The daemon builds the timeline only for the client KINDS that use it
(`engine::wants_raid`: `Window` and `Mcp`) — so the overlay's Σ-split
connection, which is a `Window` kind, is sent one it never reads (a known
waste, pixel-neutral to fix with a kind or a flag). The view tabs follow
(`window/tabs.rs`: line icons, the prototype's order and names; the active
one's underline glides to it), and the strip keeps its ACTIVE tab whole in
sight at any width (gui-logic's `reveal`, asked whenever the view or the
width changes; a wheel scrolls it, and its edges fade where it scrolls). The
chrome budget this bought is measured, not eyeballed: at 1440×900 the
featured raid's first meter row starts under 290 px down with 18 rows
showing (`window::tests::the_chrome_leaves_a_raid_its_rows`, 285.5 px, on
every `cargo test`).

## The meter

The meter (`window/table.rs`). Every list of `Row`s is drawn from ONE
column model (gui-logic's `table`): a column set (`table::meter_set` per view
— amount, rate, share and the view's own fourth column, crit / overheal /
absorbed; a count view's count and share; amount and rate at 820 px and under
— and the inspector's lists on `Grid::Abilities` / `Targets` / `Pair`), the
heading line over it and the total row under it come from the same list, so
they cannot drift; the live meter stands on the prototype's own grid per view
(`table::Grid::Meter`: `.v-num4` / `.v-enemy` / `.v-count` widths at a 12 px
gap). Every numeric heading sorts (desc → asc → the daemon's order,
`Gui::sort`); sorting and filtering change what is DRAWN and never what a
row's numbers mean (each row keeps the daemon's index, so ranks, shares, the
bar's scale and click targets hold, and `j`/`k` walk the drawn order through
`table::meter_step`); only the live meter's total follows a filter, the drawn
rows' fold with no share, as the prototype's does. The fight's workspace —
header, tabs, headings, rows, total — runs edge to edge; a short list takes
its own height and the total follows its last row, a long one scrolls and
the total pins under it. The list follows its selection past the fold: a key
step, the "you" chip and an opened death set `Gui::reveal`, and the list's
next layout scrolls the least that shows the WHOLE row, bar included
(`ScrollHandle::scroll_to_item`). The owner's row wears a "you" tag; a row's
bar eases to its new length (280 ms) and brightens when selected. The row
filter is a compact field at the end of the view tabs, widening from 92 to
140 px while it has the keys. On the Deaths view the meter is the deaths IN
THE ORDER THEY HAPPENED (`window/deaths.rs`, its words gui-logic's `deaths`:
time, player, killing blow then ONE quieter run "source, rezzed m:ss" that
gives way before the blow loses a letter, hit, overkill in red; j/k walk
them, each step that death's recap, and beside the inspector the table always
holds the keys); the count table stands only where no timeline came (a
card-only stored pull). The meter and the Deaths table wear the window's
scrollbar (`crate::scrollbar`, iced's thumb on a clear lane), never Kit's
self-hiding one.

## Inspector

The inspector (`window/inspector.rs`, window-only; the prototype's `.insp`):
the window's drill is master and detail — beside the meter above 820 px (520
px, 410 px in a 821–1180 tile — the most of the prototype's `minmax`, which
the grid always gives it), pushed over the whole stage with a back button at
820 px and under. `inspector/model.rs` builds `Insp`, owned data, once a
frame from the fight's `ClientState` and the window's `InspState`; `view.rs`,
`list.rs` and `recap.rs` draw it, and three components take their seats: the
graph (`plot.rs`), the stack matrix (`matrix.rs`) and the death chips
(`chips.rs`). It FOLLOWS the selection through `ClientState`'s opt-in
follow-selection (`set_follow`, which the window turns on and the TUI never
does): every move (`select_row`, j/k) re-watches `Cursor::Segment` with the
selected row as the drill, so one snapshot carries rows and breakdown; the
drill is keyed by guid so a re-sorting snapshot moves the highlight with the
player, an answered view with nobody in it drops the drill, and a filter that
hides the selection moves it to the first drawn row. Enter hands the keys to
the inspector (`inspecting`, what a narrow window draws as the push) where
j/k walk its list and Enter opens the ability inside it. It shows the player
(34 px disc, name, "Spec Class, you, died m:ss", and under it — v44 — what
energized them, "Mana 1.2M gained, 40.0k wasted"), the view's numbers four or
two across (a healer's "Heal absorbed" beside Overheal when a heal-absorb ate
any; an opened empowered ability's "Stage avg" and cancels among its casts), the actions (Compare `v`, Talents and gear `t`, Death recap, Per
second/Cumulative `g`), the graph and its LANES — and under them, for a
healer, their mana as a thin line on the graph's clock (R28,
`inspector/power.rs` over gui-logic's `inspect::power`, the theme's `power`
data token) — then the lists. The graph
is one canvas painted in the iced window's layer order from gui-logic's
`inspect::geometry` — every constant, the axis, span rectangles and hit
tests, hover snapping, the drag window, the tooltip's words and placement,
the deaths' rules, the ticks and lane labels with their tooltip culling, curve points
and stacked bands, the clamped Catmull-Rom spline (the prototype's
`smooth()`) — while the component brings only a text measure (shaped at the
zoomed size, measured back at zoom 1) and its paint (`plot/{curves, deaths,
lanes, tip}.rs`): the curve as an area, the rate in the prototype's 10 s
buckets (finer only for a stretch too short to hold forty), minute ticks over
the fight's span, a death as two dashed rules — red where they died, green
where they were alive again (no green while `open`: they never came back) —
with the stretch between left clear and the words in a band the curves peak
under (deaths placed before returns, so a return gives way), the plot
starting at the lanes' track only when there are lanes — the gutter that
leaves is its scale, the peak and 0. Its gestures are window mouse
listeners from paint, so a drag scrubs past the canvas, held to the plot: a drag selects a zoom window (gold edges and the
window's words while in flight), a right press resets it, and a zoom glides
to its new window (220 ms). The LANES under it (rows 19.6 px apart:
cooldowns, items, externals, defensives) draw each span in its CASTER's class
color via gui-logic's `inspect::Roster`, which also names a drill's target
rows — they wear the drilled player's class on the wire; the lanes take every
mark, NOT `view_draws_mark`'s per-view set, since a lane is its own row and
never washes the curve. The lists (`inspector/list.rs`, behind two tabs — Tab
walks them): pets dimmed after the ability, the pair ending in ONE ellipsis;
a 2 px class bar; the keys' line an accent edge kept in sight (the inspector
scrolls as a whole and its lists sit deep inside, out of
`scroll_to_item`'s reach, so a probe on the keyed line, `list::Keep`, reads
its bounds after layout and scrolls the least that shows it); the ability
list is the throughput table — amount, share, hits, average, crit. On Damage
and Healing that list is R26's TREE (gui-logic's `tree`, the lines the list
draws and the keys walk): groups of two or more and rows with parts fold,
shut until opened (`InspState::tree_open`, session-wide), a group of one
drawn as its row with the pet's or trinket's name after it, a part led by
what it is ("Direct", "Over time"); j/k walk LINES (the window keeps the line
in `InspState::tree_cursor` when it is a group or a part — `ClientState`
still thinks in row indices), Enter on a group folds it, → ← open and shut a
fold or climb to the line that holds it while the keys are in the list
(swallowed there, like a recap's), and the opened ability's numbers gain
Casts, Avg cast, Miss and Uptime. Over it the graph STACKS (gui-logic's
`inspect::stack`): a band per series in six hues validated for the surface by
the data-viz checks (no ochre, no red), stacked in slot order with a neutral
"Other" on top, a color seated once per key (`InspState::stack_slots`) so a
live re-sort repaints nobody, the list's bars wearing the hues (the list is
the legend) and the tooltip a swatch per band; the switch beside Per second
("By ability" / "By target", "Total" pressed) is `InspState::stack_graph`.
Taken adds R17's mitigation line and keeps the R21 ledger behind a third tab,
"Hit by | Attackers | Stacks" (standing only where a ledger makes a matrix),
its matrices in the list's place (`inspector/matrix.rs`, the derivation
gui-logic's `inspect::matrix`): one matrix per debuff, level 0 derived PER
DEBUFF (the baseline less that debuff's own cells — exact under overlap,
empty rather than invented without a baseline), each cell on a faint wash of
its heat, its tooltip saying how many hits its average is over. The Deaths
view's inspector is the recap as the prototype's `recapPanel` draws it
(`inspector/recap.rs`: oldest first to the tinted killing blow; a time column
from each entry's `offset_ms`, the signed change, the event with its source
quiet after it — "yours" for the owner's own — and the health after it as a
6 px bar, amber under 15 %, red under 3 %, a dash where none was reported;
an insight line after the list when the player hurt themselves) and wears a
chip per death window (`inspector/chips.rs`); clicks, and ← → while the keys
are in the inspector (on the meter they step pulls), select through
`ClientState::select_death` (proto: `death` rides the Watch, reset with the
drill/view; `drill_breakdown` / `deaths` / `drill_stacks` are the
accessors), and the attacker list words its amounts as damage; the Enemies
view's is its numbers straight onto the attackers. `v` pins the selection and
the next move makes the pair: the inspector overlays both curves on one plot
and one scale (the second dashed when one color would draw both — a shared
class — as the prototype's `graphBlock`), both players' lanes (each lane
split, the first's spans over the second's), their numbers, a legend and the
two ability lists (side by side in a wide window, stacked in a tile or a
narrow one), the meter still beside it (its rows as they stood — the
comparison's cursor carries none, so a view switch mid-pair re-asks for the
meter before the pair re-forms, and a live pull's meter says "paused while
comparing"; the pinned row wears a gold-dim "A" and its partner a "B", and a
pin says so in a toast until the pair forms). A move of the selection drops
the breakdown in hand; the window holds the last player's body
(`inspector::model::Held`, re-taken on every snapshot —
`ClientState::snapshot_gen`) and draws it dimmed until the next one's lands.
Enter on a pair beside the meter does nothing; Tab and `g` in a narrow window
push the inspector they change. A drag across the graph (v38) scopes the
lists to the window where the daemon keeps a clock for them
(`View::windows_drill` / `windows_targets`: Damage's abilities and targets,
Healing's abilities, the enemies' attackers): the window rides
`Cursor::Segment.range`, the rows come back windowed (`Segment::spells_in`,
`damage_targets_in`; the tree keeps its groups alone, `SpellTree::windowed`),
the list's heading names the window ("Ability, 0:42–1:13") or says it kept
the whole fight ("Target, whole fight"), and the graph's top line gives the
window's rate. A STORED pull windows the same way when its fight keeps the
history store's series tier (v39: kills, keys and pinned fights while their
details last, a boss kill's and a timed key's all season since v42 —
`StoredFight::series`, which turns its state's
`set_drill_windows` on, and `GetFight.range`); any other's zoom stays the
graph's and its lists say "whole pull". WIDENED (the corner button at
the head's right, `f`; `InspState::wide`, session-wide, nothing in a narrow
window): the inspector takes the whole stage under the view tabs, the meter
set aside while j/k still walk its players (`Seat::Wide`,
`inspector::model::Layout::Wide`, its rules gui-logic's `inspect::wide`): the
head on one line with the actions beside the name, the numbers in one line,
the graph 180 px tall, and the panes the tabs would switch side by side 3:2
on a stage of 1100 px or more (else one over the other) — a drill's
abilities beside its targets ("Hit by" heads Taken's), a recap beside its
attackers, R21's matrices under them rather than behind a tab — every list
with room for the rate and the view's last word (`inspect::list::Room::Wide`;
a rate no row was given is dropped), a pair's halves with rate, hits and
crit. Tab walks the two lists; Esc narrows it before Home.

## Home

Home (`window/home.rs`, its panels in `home/panels.rs` and its charts in
`home/charts.rs`; the derivations gui-logic's `home` and `home::chart`;
window-local like the talent viewer): the prototype's `.home`, "You, this
week", derived client-side from `HistoryQuery::Fights` answers — each card's
"me" is the scoped character's row, or scoped to all of them its owner's —
read one page in flight at a time until the week (the seven days back from
the store's newest card) is in hand, never more than `MAX_PAGES`. It opens at
launch when nothing is live (`home_on_start`) and stands aside when a pull
starts; `~` or the top bar's Home reaches it, and keys on Home never reach
the meter under it. Scope is a chip row — All characters, then each character
the window knows you play, with a class dot — never a lock: a chip (or a pick
of the picker's menu, which opens Home scoped) is remembered as config
`character` (`Config::store_character`), the scope Home opens on and the
character the picker names, and nothing else — whose window it is, the chrome
and the "you" follow the character played last. The night card ("Last night you played", "Tonight…" for
tonight's) is the scope's newest night: the place (a raid and its difficulty
from its visit's Σ card, else a Σ on its map, else — the daemon stores a
visit's Σ only once it closes — the tailed log's own visit,
`rail::log_instances`, else "Heroic raid", never a placeholder name; "Mythic+
keys"), the date, the character, a tile per pull (outcome glyph, "at 56%"
after a wipe, "17th of 19, 149.3k" — the place among the ROLE by effective
dps, a healer's by hps, `home::standing`), and a rank slope chart of each
pull's percentile in the role. Under it a grid of panels (the prototype's
330 px minimum, three wide, one at 820 px and under, widths from the window's
width less the docked rail): keys against their timers (par bars with ticks
at +3, +2 and par, "+2" or "over"), one panel per raid and difficulty (per
boss "Killed in 7:02" or "Best 2%" — never an unobserved 0 % — and a dot per
pull, filled on a kill, ringed in the character's color on a wipe) and
effective dps on keys (a dot per run in its character's color, each
character's best of the week ringed in legendary orange) — always those
three, in that order, each in its own words when its week is empty, so the
dashboard keeps its shape; never more columns than panels, a grid row's
panels stretched to one height. Every tile, row and dot is a jump point into
its pull, and each chart dot is an element of its own, the size of its press
radius, whose tooltip names the pull ("Ula'tek, 10th of 18"). It shows no
season score, no `N / 8` boss denominator and no ladder percentile because no
card carries them, and words a disabled store, a cold store and a degraded
one (the daemon's `Status`) apart rather than showing the same confident
nothing for all three — off or not answered yet, the night card ("Your
week") carries the state's words and no panel is drawn — refreshing `Status`
on open and on a debounced store change, as the daemon never broadcasts it.
Config keys: `season_start` / `season_end` (UTC `YYYY-MM-DD`, hand-parsed —
no chrono; Home reads nothing before the start; the old `season_label`
round-trips untouched and draws nothing), `density`, `home_on_start`,
`character` (Home's scope).

## Command palette, cards, filter and Esc

The command palette
(`window/palette.rs`, its model gui-logic's `palette`; window-local; Ctrl K or
the jump box) is a card over a scrim: its field holds the focus in a
`Palette` context, so the meter's keymap is silent under it, and `Palette >
Input` binds the arrows, Ctrl N / Ctrl P, Esc and Ctrl K. It searches the
rail's pulls (the newest four before anything is typed — a live one, a boss,
a key, never the trash between), the players of the pull on the stage (a
count view's or the enemies' pull still lists who fought, from
`fight_head::Seen`, and running one switches to Damage and selects them by
key through `ClientState::select_player`, opt-in, the TUI never calls it),
the views with their key (`keys::key_for`) and the screens (Home, the live
pull, the earlier nights, the sheet, the talent viewer, and — once a word is
typed — Home's scopes), title and words folded together through `fold`;
Enter runs, a press runs a line, one on the card's heading holds and one on
the scrim closes. The selection is the one line lit (RAISE with the accent's
2 px edge, the rail's current-pull mark); the pointer lights none, as the
prototype's `.pal-it` has no hover. The other cards (`window/cards.rs`: the
`?` sheet, the ⚙ card, the character menu and the toast) are MODAL for the
keys: while one is up the root leaves the `Meter` context for `Modal`, where
any key closes the menu and does nothing else, the zoom chords still zoom and
Ctrl K replaces it with the palette; one menu at a time. For the pointer the
sheet hangs over an occluding scrim a press anywhere on closes, the character
menu over a clear one, and the ⚙ card closes when the pointer leaves it; it
writes each choice alone through `Config::store`. A card enters with a fade
and a 6 px rise (160 ms). The toast (`cards/toast.rs`, its words gui-logic's
`toast`) is the window's own, not Kit's `Notification`: centered at the
stage's foot, brief, taking no pointer. Esc walks one level up: the palette,
the talent viewer and the menus answer their own Esc first, then the rail's
drawer, then on the stage the filter's text, the inspector's ability or keys
(a narrow window's push), the comparison, a widened inspector, and Home,
where the chain ends.
`/` focuses the row filter; its Esc (`Filter > Input`) clears it and gives
the keys back, Enter keeps the text and gives them back. The filter narrows
what is drawn by label, class, spec or role name (`Class::name` /
`Spec::name` / `Role::name`, case-insensitive substring, accent-folded
through `gui-logic/src/fold.rs` so "akanos" finds Akanôs and the accented
spelling still works — Latin-1 and Latin Extended-A only, non-Latin scripts
deliberately untransliterated), and never renumbers: a filtered row keeps its
rank, its share and the index a click sends back — and `j`/`k` step over what
it hides, so the highlight is always on a drawn row. It is a PLAYER filter and
stops at the player list: the inspector's lists are never narrowed by it, and
it is drawn wherever the meter is — beside the inspector, under a comparison,
never over a narrow window's pushed inspector. The palette and the row filter
are the window's themed `Field` (`window/field.rs`): Kit's styled `Input`
paints every placeholder in the theme's one `muted_foreground` and forces
`text_sm` at every zoom, so `Field` composes Kit's unstyled gpui-base `Input`
on the same `InputState` and projects the window's tokens onto its editor
style each render (placeholder in ink 3's grade, caret, selection, ink),
leaving caret, IME, scrolling and clipping Kit's; the talent viewer's framed
import field stays Kit's styled `Input`.

## Chrome and type

gui-logic's `theme` is the one definition: a `Def`
(`NAVY`, `ONYX`, `FROST`, each in its own file under `theme/`; config
`theme`; an unknown name reads the default, `onyx`, and the old `gold`
reads `navy`) holds the
window's `WindowTokens`, the overlay's `OverlayTokens`, the talent viewer's
`TalentTokens`, the data hues (`DataTokens`: the stacked bands, the
lettered squares, the foe, the timeline marks), the `Faces`, the type scale
(`Sizes`), the row pitches (`Pitches`), the corners (`Shape`: every radius
times `scale`, a pill capped at `chip`, read through `w.r` / `w.pill`,
`ov.r`, `p.r`, `pen.r`), the `Bars` (how much class color a bar shows), the
`Effects` and the `Shadows`. Every token group
is written by `theme::tokens!`, so each field is a config key by the same
name: `[themes.<name>.window|overlay|talents|data|faces|size|pitch|shape|bars|
effects]`, plus `base`, `label`, `accent_label` and `dark` — `theme::Registry`
lays a config's tables over the built-ins (a table named for one overrides
it; any other is a theme of the user's own, from its `base`, the default when
unsaid), owns what it builds — a theme's words are `theme::Text`, a
literal or a string shared from the config; the built-ins are lazily built
statics; the active `Look` holds its `Def` by an `Arc`, so a theme switched
away from is freed — and words every
mistake (`did you mean "accent"?`) for stderr and the ⚙ card. The window
switches theme from the ⚙ card (`Gui::set_theme`, one key through
`Config::store`, the chrome kept); the overlay polls the config's mtime once
a second and follows (`Overlay::take_theme`). **Onyx** is a black-dial
chronograph: true black, lume ink, a gray ramp, white chrome (its chrome
chip says "White"), Saira Tabular (Saira at width 80, the measure of
Barlow Semi Condensed) and Michroma, corners at
0.3 and squared chips, and the four `Effects` `navy` leaves off — `glass`
(what floats — the cards, the palette, the sheet, the toast, the tooltips,
the overlay panel — is the `glass` fill under a sheen with an inset
specular rim: `W::float` / `.floating`, `paint::paint_float` on canvases,
`Ov::glass`, `Paint::tip_face`), `brackets` (reticle corners round the
ribbon and the inspector's graph), `dial` (`window/instruments.rs`: the
inspector's crest in a 60-tick bezel, the player's meter bar wrapped round
it in their class color, to a hand) and `fine_ticks` (the ribbon's 10 s chapter ring). Two more
switches restyle what navy fills: `quiet_press` (a pressed action — "Stop
comparing" — is a raised key with an accent hairline and a lit bar along its
foot, not a block of the accent) and the data token `stack_other_edge` (the
stack's rest, `Ink::StackRest`, drawn as graphite under a steel line: the
player's whole curve). No color of navy's is baked anywhere else: a class
chrome that knows no class yet wears the theme's own accent (the old
`NEUTRAL` blue is gone), and the graph samples take a theme's hues
(`samples::all_in`).
`crates/gui/src/theme.rs` `apply`s it to two targets: Kit's `Theme`, slot by
slot (the chrome goes to `primary` and `ring`, the prototype's raise to
`accent` — Kit's slot names are shadcn's), and the `Look` global every
bespoke surface reads, so `apply` with another definition repaints
everything (a test samples a Kit component and a bespoke surface across a
switch). No surface draws a literal color: a new color is a token on every
`Def`. `navy` wears the prototype's Tokens — ground, surface, raise,
line, edge, three inks, accent / label ink / accent ink (its gold, gold-dim and
gold-ink), good, bad, legendary (a
personal best, and nothing else), hover, name-lit, the tick on a checked box
— under one rule: gold is the interface, class colors are people, green and
red are outcomes, and no color is semantic yellow (a live pull is a red dot
and its word; crit is ink). The chrome is `theme::Chrome`, config `chrome =
"theme"` (the default, the theme's own accent; the old `"gold"` reads
as it) or `"class"`, a plain string like `density` so a typo reads the
theme's. A class chrome wears the OWNER's class, learned once from the
owner's row and held (`Gui::learn_class`) — rows resort on every snapshot, so
tinting from the selection would recolor the whole window on its own — and
is right on the first frame because the window writes that class whenever it
learns it (`character_class`, one key through `Config::store_character_class`)
and `main.rs` reads it before the first frame. The accent is drawn only as an
underline under the active tab or place, a pressed chip's edge and a
selection's edge — never a fill; `theme::chrome_base` may move a class color
along its own hue until ink on it clears WCAG AA (Shaman blue is the one that
moves), and `Accent`'s light/dark ink split is WCAG's crossover luminance so
all thirteen class colors stay legible. A class color as TEXT is lifted
toward white until it clears AA on the surface (`theme::class_text_on`); a
BAR keeps `Class::rgb` exactly, because the bar is data. Every bar in every
list is a narrow bar UNDER the row's text, the text on the panel in its own
ink, so no name or number ever sits on its class color: the meter's 3 px
(`window/table.rs`), the inspector's 2 px (`inspector/list.rs`), the
overlay's 3z (`overlay/rows.rs`). The window's TYPE is bundled: Barlow Semi
Condensed 400/500/600 with its tabular figures baked into the default digits
(the family renamed "Barlow Semi Condensed Tabular" so an installed
proportional copy is never the face picked) for names and numbers alike, and
Marcellus for encounter titles and the wordmark alone — OFL files under
`crates/gui-logic/fonts/` (provenance, pinned upstream commits and the
reproducible fonttools bake in its `README.md`), `include_bytes!`d as
gui-logic's `fonts::FONTS` and registered with GPUI's text system in
`main.rs` — every built-in's faces, whatever the theme; `navy`'s overlay
draws in the system UI face and a monospace for its numbers
(`overlay/ov.rs`), `onyx`'s in Saira, whose digits are baked tabular the
same way. Fonts are assets, not dependencies.
Sizes are the `Def`'s `Sizes` (the Tokens specimens: encounter 27, 22
narrow; names 15; every figure 14.5; column heads and stat labels 13.5 in
the label ink; a theme with a wider title face sets its titles smaller) and `Pitches` (a meter row 32, the top bar 44, by `density`); no
overlay code names either. The window speaks the prototype's words (gui-logic's
`labels`): sentence case everywhere (`labels::sentence` turns the source's
"KILL" into "Kill"), the views in the prototype's order and names
(`labels::WINDOW_VIEWS`, `window_view_name`; `View::ALL` and
`fmt::view_name` stay the TUI's and the overlay's), `hide_realms` honored in
every pane (`labels::realmless` strips only what reads as a player's
"Name-Realm-Region", since creatures' names hold hyphens). The line icons are
strokes, not SVG assets: `window/paint.rs` strokes gui-logic's `glyph` table
(GPUI exports no round caps, so each segment is its own sub-path with a disc
of the stroke's width at every end), and `glyph_ink` paints in its parent's
text color so an icon brightens with its control's hover. A label ends in
"…" through GPUI's own `text_ellipsis` / `truncate`. Two GPUI traps shape the
window's pixels: GPUI draws a border OUTSIDE the padding (iced drew it
inside), so a card or a button pads by the design value less its edge
(`cards::BORDER`); and its default line height is φ, so a piece that must
stand on the window's 1.3 (the matrix, the death chips) sets it itself.

## Talent viewer

The talent viewer (`t`, `talents.rs` and `talents/`, window-local; its logic
gui-logic's `talents`: the layout `model`, the editing state machine
`viewer`, the `geometry`). `t` opens it on the selected row and asks for the
logged loadout; the root drops its `Meter` context while the viewer holds
the window, and its keys live in a `Talents` context (Esc closes, Tab flips
the tab). It decodes in-game import strings through `proto::talents` against
the per-machine `talents.json` and draws the panes the way the game does:
class pane left, spec pane right (split at the posX midpoint), the picked
hero tree between them under its medallion + golden ring
(`gui-logic/src/talent_art.rs` reads `talent-art.bin` — pane background
paintings included; absent cache = plain panels). Node frames follow the
game's shapes — square = active ability (entryType 1), circle = passive,
octagon = choice with side carets — with gold borders, rank pills and lit
gold paths for taken talents; icons come shaped/desaturated from
`spell_icons::styled`. Each pane is ONE canvas painted in the game's layer
order (`talents/pane.rs`) — GPUI keeps paint order, images and paths
interleaved as painted (spike S8), where iced composited every image above
every path and needed stacked widgets — with the background painting a
cover-fit canvas under the trees, and the pointer mapped back through a hitbox
per pane. A pasted SimulationCraft addon export (`gui-logic/src/simc.rs`,
stdlib parser), or the export's file dropped on the viewer, also brings saved
loadouts (chips switch between them), equipped gear, bag items and
currencies (`talents/inventory.rs`); pastes persist per character under
`~/.local/share/wowdps/simc/`, so reopening the viewer on that player's
meter row restores their build. v19: opening on a row also sends
`GetLoadout` through `Session::request` (the row supplies name, spec id AND
guid); the daemon answers with the player's COMBATANT_INFO loadout — the
actual talents + equipped gear from the log — which wins over a stored paste
(`adopt_logged`: picks → `proto::talents::picks_to_selections` → `encode` →
the normal adopt path, so validation, "copy string" and the warnings pane all
just work; a "from combat log" marker shows, gear renders on the inventory
tab as honest `item {id}` rows in slot order, simc loadout chips stay one
click away, and logged builds are never persisted — the daemon re-answers on
every open). Its delights settle under reduced motion: a lit path grows from
its parent, a taken node sends out a gold ripple, an opening choice fans its
options out, a tooltip arrives with a fade and a rise, the trees scale to fit
a narrow window down to 70 % before they scroll, and "copy string" answers
"copied ✓" for 1.4 s. The ignored `real_dataset_draws_every_spec`
(`cargo test -p wowdps-gui real_dataset_draws_every_spec -- --ignored`)
opens and draws every spec of the real dataset and takes a root in each.

## The overlay, as built

The overlay (`overlay.rs` opens the surface; `overlay/panel.rs` is the
`Overlay` entity — the collapsed tab and the panel's header, rows and footer,
with the options card and the view menu; `panel/surface.rs` its place on the
edge; `rows.rs`, `instance.rs`, `drill.rs`, `graph.rs`, `panel/compare.rs`;
`ov.rs` its zoom, palette, faces and small pieces). It draws what the iced
overlay drew — the same states, its palette (`OverlayTokens`), monospace
numbers and yellow — and its pixels are held by the render guard (`crates/gui/SHOTS.md`). It is
a GPUI layer surface (`WindowKind::LayerShell`: namespace `wowdps`, the
overlay layer, `KeyboardInteractivity::None`, transparent), opened through
`cx.open_window`, never Kit's Root. It is single-instance
(`gui-logic/src/single.rs`): a new `--overlay` launch evicts the running one
via an unversioned takeover socket, so orphans can't stack surfaces or
respawn daemons. Its output is chosen before the app starts
(`overlay::choose_output`), because a layer surface never changes output:
`WOWDPS_OVERLAY_OUTPUT`, else config `monitor`, else under Hyprland with
`follow_game` the game's monitor (`hypr::game_monitor`: the game window's
workspace, then that workspace's monitor from the `workspaces` reply — so a
game parked off screen still resolves), else the compositor's choice. The
daemon spawns on the game PROCESS and the window maps seconds later under
Proton, so a daemon-spawned overlay — marked by the
`WOWDPS_OVERLAY_GAME_STARTING` env the supervisor sets — polls for up to 30 s
(`GAME_WINDOW_WAIT`) before choosing; a hand launch asks once. The display is
found by recomputing GPUI's UUIDv5 of the output name
(`gui_logic::output::output_uuid`), never by bounds, which are wrong at
fractional scale and under rotation. **The edge strip**: gpui-pre sets a
layer surface's margin only when it is created, so the surface spans its
edge's whole LENGTH (anchored to the edge and both neighbors), its
THICKNESS follows the state (`Window::resize`, sized in `render`, only a
change sent), the content sits at the configured offset inside it, and the
input region is exactly the content (`Window::set_input_region`), the rest of
the strip click-through (`overlay/strip.rs` is the arithmetic). The grip (the
tab, the header) tells a click from a drag beyond 5 px, a window-wide
listener follows a drag off the content, and the offset is remembered by
itself; under Hyprland a tab dropped near another edge recreates the surface
there (gui-logic's `surface::nearest_edge`), the new one opened before the
old one goes. It never asks for a zero length (layer-shell's "stretch" is a
viewport protocol error under GPUI). "Hidden" is a 1 px strip with an empty
input region: the daemon's `SetVisible` composed with `follow_game` (the
game's workspace on screen, `gui-logic/src/hypr.rs`; `game_match`). Closing
its last surface quits, and the supervisor takes it from there. It zooms by
hand (`ov.z(…)`), and a wheel over the header zooms. Inside an instance visit
the overlay anchors its frame on the *visit*: gui-logic's `timeline` groups
the segment list into blocks (a visit's Σ + members, or a stray segment —
the window's rail groups tonight by the same `timeline::blocks`) and
`overlay/instance.rs` draws the clickable Σ–①─②─③–⚑ strip; the footer ◀▶
steps whole blocks while the strip, its chip's ‹ › and a wheel over it scrub
members, a new pull re-pins Live (unless parked on the live visit's Σ), and
the footer Σ toggle (`overlay_split` in config) appends the visit's overall
rows through a second `Session`, made on first want by an `AuxMaker` (a
`Window`-kind `DaemonClient`, so `SetVisible` never reaches it). The overlay
has no keyboard, so the footer's view name carries both switch gestures:
left-click cycles (`View::next`), right-click opens a view menu card that
jumps straight to any view. A right press anywhere else backs out one level
(a comparison first, then an ability, then the drill), and on a graph it only
resets the zoom. Its drill (`overlay/drill.rs`) is still the screen it
replaces — and, on Damage and Healing, R26's rollups through gui-logic's
`tree::lines`, groups (a pet's abilities under its summon, a trinket's under
the item, a proc under its driver) shut until a press opens one
(`Overlay::tree_open`, session-only), a press on an ability still drilling
into it, and no (spell id, periodic) parts; every line of a grouped drill
keeps a caret and an art column, a drill with no groups stays the flat list
— and its graph (`overlay/graph.rs`) shares one time cursor across every
graph on the panel. Its lists wear the overlay's square scrollbar
(`crate::scrollbar`, a 10 px rail with a draggable thumb), and its staleness
radar sweeps on a 100 ms timer only while it shows (`RADAR_FRAME`). The
daemon sends an `Overlay` session no raid timeline (its Σ split's
`Window`-kind connection still gets one, unread — see the ribbon above). The
`WOWDPS_OVERLAY_*` debug aids live in `panel/autos.rs` (`docs/tracing.md`).

## Art from the per-machine caches

Meter rows wear the game's own art, all from PER-MACHINE caches under
`~/.local/share/wowdps/` — extracted Blizzard artwork never lands in the
repository, and a machine without the caches renders fine. `class-icons.bin`
(`tools/gen-icons.sh`: classicon_* crests + ChrSpecialization spec icons,
decoded by `tools/extract/src/blp.rs` — BLP2: DXT1/3/5, palettized, raw —
32px, circle-masked; read whole by `gui-logic/src/icons.rs`, ~200 KiB) and
`spell-icons.bin` (`tools/gen-spell-icons.sh`: every spell id via SpellMisc,
~58 MiB; `gui-logic/src/spell_icons.rs` loads the index once and reads tiles on
demand). The readers are generic over the image handle a GUI makes of a tile
(`lazy_tiles::Tiles<K, H>`); the GUI's handle is an `Arc<RenderImage>` made on
a tile's first use and cloned ever after (`crates/gui/src/images.rs`), so GPUI
uploads each texture once. A player's disc (`window/chrome.rs`'s and the
overlay's `rows.rs`'s `class_icon`) prefers the spec icon, falls back to the
class crest, then to a disc in the class color wearing its two-letter tag;
ability icons on by-spell rows simply vanish without their cache. The `image`
crate is named only to build those `RenderImage` frames; no image file is
decoded at runtime.

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).

[^spec]: `docs/spec-gui-new.md` §6: sessions, `pump`, views as entities, keys, themes.
[^plan]: `docs/plan-gui-new.md`: phases 1–4 as built, and phase 5's "As built: the cutover".
[^gpui]: `docs/gpui/README.md`: the findings section, Kit's Root on layer surfaces, and an animation frame redrawing the whole view.
[^shots]: `crates/gui/SHOTS.md`: every shot test, its variables, and the guard's tolerance.
