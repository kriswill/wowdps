# Slice 1 — implementation spec

Target: `crates/gui`, branch `feat/ui-design-update`.
Source of truth for intent: `docs/design/gui-window-features.html`, section 2
(visual language), Layout A (Home), "The view map".

**Hard constraints**

- **No wire change.** `PROTO_VERSION` is untouched; no new `ClientMsg` /
  `DaemonMsg` variant; no change to `crates/proto`, `crates/model`,
  `crates/core`, `crates/daemon`. Everything Home shows is derived
  client-side from `HistoryQuery::Fights` answers the daemon already serves.
- **The overlay must keep compiling and behaving exactly as today.**
  `crates/gui/src/overlay.rs` is not edited in this slice except where a
  constant it imports from `view.rs` moves to `theme.rs` (re-export from
  `view.rs` so the overlay's `use` lines stay valid — see task 1).
- **`ClientState` never learns about Home.** Home is window-local, exactly
  like the talent viewer: it is not a `Screen` variant, the shared state
  machine keeps running underneath it.
- Dependency policy unchanged: gui may use `iced`, `iced_layershell`,
  `serde`/`toml`. No chrono — dates are hand-parsed (see §4.3).
- Out of scope, do not build: throughput table with rollups, cast timeline,
  uptime lanes, stack matrix, mitigation cards, death navigator, mirrored
  gear grid, anything reading R18/R20 off the live snapshot path.

Where this spec says **verify**, the exact iced 0.14 API was not checked by
the designer; confirm against the locked `iced 0.14.0` before relying on it.

---

## 1. Module list

New files, all under `crates/gui/src/`, all registered in `main.rs`'s `mod`
block (alphabetical: `home`, `nav`, `theme` slot in beside the existing ones).

| file | role | who may use it |
| --- | --- | --- |
| `theme.rs` | colors, accent derivation, type scale, density | window, overlay, every renderer |
| `nav.rs` | message-generic shell widgets: tab bar, jump chips, two-tone title, stat cards, filter box, shortcut sheet | window today, overlay later |
| `home.rs` | the Home screen: its cache, its pure derivation over cards, its renderer | window only |

Edited files: `main.rs` (mod decls), `window.rs` (state + messages + update +
`drain_client`), `view.rs` (chrome adopts `theme`/`nav`; filter applied),
`keys.rs` (binding table + the three new keys), `config.rs` (five keys).

### 1.1 `theme.rs`

```rust
//! One place every color, size and spacing constant comes from.

use iced::Color;
use wowdps_model::{Class, Spec};

/// The spec-derived chrome accent (design doc §2a).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Accent {
    /// The class color itself (`Class::rgb`), `--spec`.
    pub base: Color,
    /// `--spec-2`: base lifted 35% toward white, or darkened 45% when the
    /// class is light. The gradient always runs `lift -> base`.
    pub lift: Color,
    /// Ink drawn ON the accent: near-black for a light class, near-white
    /// otherwise.
    pub ink: Color,
    /// Heading color: whichever of `base` / `lift` has the higher relative
    /// luminance, so a Priest reads white and a DK reads lifted red.
    pub heading: Color,
    /// True when `relative_luminance(base) > LIGHT_THRESHOLD`.
    pub light: bool,
}

/// The luminance boundary the doc names. Warrior (0xC69B6D) is the case to
/// check when tuning; it must land on the LIGHT side.
pub(crate) const LIGHT_THRESHOLD: f32 = 0.6;

/// sRGB relative luminance, gamma-decoded (WCAG formula).
pub(crate) fn relative_luminance(c: Color) -> f32;

/// `c` moved `t` (0..=1) of the way toward white.
pub(crate) fn lighten(c: Color, t: f32) -> Color;
/// `c` moved `t` (0..=1) of the way toward black.
pub(crate) fn darken(c: Color, t: f32) -> Color;

/// The accent for a player. `spec` is accepted and ignored today (class is
/// the honest default per §2a); it exists so a later within-class tint is a
/// one-function change. `None` class yields [`NEUTRAL`].
pub(crate) fn accent(class: Option<Class>, spec: Option<Spec>) -> Accent;

/// The accent when no owner/class is known — the current flat chrome.
pub(crate) const NEUTRAL: Accent;

/// A two-stop linear gradient `lift -> base`, left to right, as a
/// `container::Style` background. **Verify** `iced::gradient::Linear` /
/// `iced::Background::Gradient` in 0.14; if a gradient background is not
/// available on `container`, fall back to a two-container stack (a `lift`
/// bar under a `base` bar at 50% width) and say so in the commit body.
pub(crate) fn accent_fill(a: Accent) -> iced::Background;

// ---- type scale (design doc §3c: "one scale, two densities") -------------
pub(crate) mod size {
    pub(crate) const DISPLAY: f32 = 22.0; // Home headline numbers
    pub(crate) const TITLE:   f32 = 16.0; // screen title (today's 16)
    pub(crate) const HEAD:    f32 = 14.0; // panel headings, durations
    pub(crate) const BODY:    f32 = 13.0; // row labels
    pub(crate) const SMALL:   f32 = 12.0; // captions, footers
    pub(crate) const MICRO:   f32 = 11.0; // tags, ranks, column heads
    pub(crate) const TINY:    f32 = 10.0; // panel eyebrow labels
}

/// Two densities. `Comfortable` is the window default; `Compact` reproduces
/// today's tighter metrics and is what the overlay would ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Density { #[default] Comfortable, Compact }

impl Density {
    pub(crate) fn row_h(self) -> f32;   // 22.0 / 18.0
    pub(crate) fn gap(self) -> f32;     //  8.0 /  4.0
    pub(crate) fn pad(self) -> f32;     // 10.0 /  6.0
    pub(crate) fn from_name(s: &str) -> Option<Self>;
    pub(crate) fn name(self) -> &'static str; // "comfortable" / "compact"
}

// ---- the palette that used to live in view.rs ---------------------------
pub(crate) const DIM: Color;
pub(crate) const GREEN: Color;
pub(crate) const RED: Color;
pub(crate) const YELLOW: Color;
/// Panel background / hairline, extracted from the values currently inlined
/// in `view.rs::options_panel`.
pub(crate) const PANEL: Color;
pub(crate) const RULE: Color;
```

`view.rs` keeps `pub(crate) use crate::theme::{DIM, GREEN, RED, YELLOW};` so
`overlay.rs`, `compare.rs`, `talents.rs`, `timeline.rs`, `gauge.rs` compile
unchanged. Do not move `CLASSLESS`, `SCHOOL_COLORS`, or the bar-color logic
in this slice.

Exact derivation, to be implemented literally:

```
lum   = 0.2126*lin(r) + 0.7152*lin(g) + 0.0722*lin(b),  lin(x) =
        if x <= 0.04045 { x/12.92 } else { ((x+0.055)/1.055).powf(2.4) }
light = lum > 0.6
lift  = if light { darken(base, 0.45) } else { lighten(base, 0.35) }
ink   = if light { Color::from_rgb(0.043,0.047,0.063) }  // #0b0c10
        else     { Color::from_rgb(0.95,0.95,0.98) }
heading = if relative_luminance(lift) > relative_luminance(base) { lift }
          else { base }
```

`NEUTRAL` = `{ base: #6ab7ff, lift: lighten(base,0.35), ink: near-white,
heading: base, light: false }`.

### 1.2 `nav.rs`

Everything here is `Element<'static, M>` with `M: Clone + 'static`, taking an
explicit message per interactive element, so the overlay can adopt any of it
without a message-type fight.

```rust
/// One entry of the tab bar.
#[derive(Debug, Clone)]
pub(crate) struct Tab<M> {
    pub glyph: &'static str,   // a text glyph, NOT an image (see note)
    pub label: &'static str,   // "damage", "home", …
    pub hint: &'static str,    // the key that also does it: "d", "~", ""
    pub active: bool,
    pub on_press: M,
}

/// The icon tab bar: seven views + Home / Fights / History / Loadout.
/// The active tab wears `accent_fill(accent)`; its text is `accent.ink`.
/// Wrapped in a horizontal `scrollable` so a narrow window never clips a tab.
pub(crate) fn tab_bar<M: Clone + 'static>(
    tabs: Vec<Tab<M>>, accent: theme::Accent, density: theme::Density,
) -> Element<'static, M>;

/// The "jump to:" chip row. `sections` are (label, message); `active` is the
/// index currently in view, or None.
pub(crate) fn chip_row<M: Clone + 'static>(
    sections: Vec<(String, M)>, active: Option<usize>, accent: theme::Accent,
) -> Element<'static, M>;

/// The two-tone title: `who` in `accent.heading`, `what` dim/white after it,
/// optional colored `tag`.
pub(crate) fn two_tone_title<M: 'static>(
    who: String, what: String, tag: Option<(String, Color)>, size: f32,
) -> Element<'static, M>;

/// One stat card. `headline` puts it on the accent gradient with `accent.ink`
/// text; otherwise a `PANEL` card with `value_color`.
pub(crate) struct Stat {
    pub label: String,
    /// Already formatted. Use "—" for "we cannot know this" (§4.4).
    pub value: String,
    pub sub: Option<String>,
    pub value_color: Option<Color>,
    pub headline: bool,
}

pub(crate) fn stat_cards<M: 'static>(
    cards: &[Stat], accent: theme::Accent, density: theme::Density,
) -> Element<'static, M>;

/// A titled panel body — eyebrow label, right-aligned caption, content,
/// optional footer link (`label`, message).
pub(crate) fn panel<'a, M: Clone + 'static>(
    title: &str, caption: Option<String>,
    body: impl Into<Element<'a, M>>,
    footer: Option<(String, M)>,
    accent: theme::Accent,
) -> Element<'a, M>;

/// The row filter box. `id` lets the caller focus it.
pub(crate) fn filter_box<M: Clone + 'static>(
    id: iced::widget::text_input::Id, value: &str,
    on_input: impl Fn(String) -> M + 'static, on_clear: M,
) -> Element<'static, M>;

/// The `?` sheet: every binding from `keys::BINDINGS`, grouped, as a
/// centered card over a dimmed scrim. `on_dismiss` fires on any press.
pub(crate) fn shortcut_sheet<M: Clone + 'static>(
    accent: theme::Accent, on_dismiss: M,
) -> Element<'static, M>;
```

**Tab glyphs:** use text glyphs only in this slice (`⚔ ✚ ⛔ ✋ ✨ ⚰ 🛡` for
the seven views; `⌂ ≣ ⏱ ✦` for Home/Fights/History/Loadout). Spell/class art
would need a cache lookup per tab and is not worth it here. Keep the glyph
strings in one `const TAB_GLYPHS: [(View, &str); 7]` in `nav.rs` so a later
change is one edit.

Tabs that lead to surfaces this slice does not build (History) render
**disabled**: dim, no `on_press`. Do not silently no-op a live-looking tab.

### 1.3 `home.rs`

```rust
use wowdps_proto::history::FightCard;

/// Window-local Home state. Lives in `Gui` as `Option<Home>`: `Some` means
/// the Home screen is up, exactly like `Gui::talents`.
pub(crate) struct Home {
    /// Cards accumulated across pages, newest first, deduped by `id`.
    pub cards: Vec<FightCard>,
    /// The in-flight `GetHistory` req_id, if any.
    pub pending: Option<u32>,
    /// `total` from the last answer — how many matched before `limit`.
    pub total: u32,
    /// Pages already fetched; the loop stops at `MAX_PAGES`.
    pub pages: u32,
    /// The last card id of the newest page, for `after_id`.
    pub cursor: Option<String>,
    /// Which character the screen is scoped to. `None` = the owner the
    /// newest card names.
    pub character: Option<String>,   // guid
    /// True once at least one answer landed (distinguishes "loading" from
    /// "the store is empty").
    pub answered: bool,
    /// The store said it is off, or the answer was empty and
    /// `Status.history.enabled` was false.
    pub disabled_reason: Option<String>,
}

pub(crate) const PAGE: u32 = 200;
pub(crate) const MAX_PAGES: u32 = 10;   // 2 000 cards, then we stop paging

impl Home {
    pub(crate) fn new() -> Self;
    /// The next request to send, or None when paging is done.
    pub(crate) fn next_request(&mut self, req_id: u32, since_utc_ms: Option<i64>)
        -> Option<wowdps_proto::ClientMsg>;
    /// Fold one `HistoryAnswer::Fights` in. Ignores an answer whose req_id
    /// is not `pending`.
    pub(crate) fn absorb(&mut self, req_id: u32, answer: &wowdps_proto::HistoryAnswer);
    /// Which guid the screen is scoped to.
    pub(crate) fn owner(&self) -> Option<&str>;
}

/// The whole screen, derived. PURE over `&[FightCard]` — this is the unit
/// under test, not the widget tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Panels {
    pub top: Vec<nav::Stat>,
    pub keys: Vec<KeyLine>,
    pub raid: RaidPanel,
    pub me: MePanel,
    pub characters: Vec<CharLine>,
    pub season: Vec<RoleLine>,
    pub recent: Vec<RecentLine>,
}

pub(crate) fn derive(
    cards: &[FightCard], owner: Option<&str>, season: &Season,
) -> Panels;

/// A named UTC date range from config (§4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Season {
    pub label: String,
    pub start_utc_ms: Option<i64>,
    pub end_utc_ms: Option<i64>,
}
impl Season {
    pub(crate) fn from_config(cfg: &Config) -> Self;
    pub(crate) fn contains(&self, start_utc_ms: i64) -> bool;
}

pub(crate) struct KeyLine {
    pub map_id: u32, pub name: String,
    pub best_level: Option<u32>, pub runs: u32,
    pub timed: u32, pub over: u32,
    pub fight_id: String,          // the best run, for the future jump
}
pub(crate) struct RaidPanel {
    pub instance: Option<String>,  // the newest raid card's name's zone, else None
    pub bosses: Vec<BossLine>,
    pub week_pulls: u32, pub week_kills: u32,
}
pub(crate) struct BossLine {
    pub name: String, pub encounter: Option<u32>,
    pub difficulty_tag: &'static str,   // "M"/"H"/"N"/"LFR"/"?"
    pub best_kill_ms: Option<i64>,
    pub best_pct: Option<u16>,
    pub pulls: u32,
    pub fight_id: String,
}
pub(crate) struct MePanel {
    pub name: String, pub class: Option<Class>, pub spec: Option<Spec>,
    pub role: Option<Role>,
    pub measure: &'static str,         // "effective dps" / "hps" / "dtps"
    pub median: Option<f64>, pub best: Option<f64>,
    pub rank: Option<(u32, u32)>,      // (place, of) on the newest card
    pub deaths_per_pull: Option<f64>,
    pub spark: Vec<f64>,               // oldest→newest, ≤12 points
}
pub(crate) struct CharLine {
    pub guid: String, pub name: String,
    pub class: Option<Class>, pub spec: Option<Spec>,
    pub fights: u32, pub last_utc_ms: i64,
}
pub(crate) struct RoleLine {
    pub role: Role, pub name: String,
    /// Percentile of the LATEST value within this character's own history
    /// for that role. Always labelled "vs your own history" — we have no
    /// ladder (design doc, panel 7).
    pub pct: Option<f64>,
    pub spark: Vec<f64>,
}
pub(crate) struct RecentLine {
    pub fight_id: String, pub name: String,
    pub tag: &'static str,      // KILL/WIPE/TIMED/OVER/…
    pub tag_color: Color,
    pub duration_ms: i64, pub pinned: bool,
    pub key_level: Option<u32>,
}

/// The widget tree. `on_*` messages are `window::Message` variants; the
/// module takes `&Home`, the derived `Panels`, the accent and the density.
pub(crate) fn screen(
    home: &Home, panels: &Panels, accent: theme::Accent, density: theme::Density,
) -> Element<'static, crate::window::Message>;
```

`Panels` derives `PartialEq` so a test can assert the whole derivation in one
comparison; that means `nav::Stat` and the `*Line` structs derive
`Debug, Clone, PartialEq` too.

---

## 2. How each Home panel is computed — and what it cannot know

One query serves the whole screen:

```rust
ClientMsg::GetHistory {
    req_id,
    query: HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: None,                     // NOT the owner: the characters
                                        // panel needs every character
        since_utc_ms: season.start_utc_ms,
        kind: None,
        sort: FightSort::Newest,
        limit: PAGE,                    // 200
        after_id: home.cursor.clone(),
        role: None,
    },
}
```

Answered as `DaemonMsg::History { req_id, answer: HistoryAnswer::Fights {
cards, total } }`.

| panel | derivation from `cards` | honest fallback |
| --- | --- | --- |
| season score | **Not derivable.** No card carries a Mythic+ rating. | render `—` with sub-label "not in the log". Never a computed stand-in. |
| keys timed | `kind == Key` cards in season: timed = `success == Some(true)`; total = the same set, `aborted == false` | `0 / 0` when there are none |
| raid bosses | distinct `encounter.id` on `kind == Encounter` cards with `success == Some(true)`, at the highest difficulty seen per boss | numerator only, worded "7 down · 9 seen". **No `/8`** — the raid's boss count is not in any card. |
| fights stored | the newest answer's `total` (matches before `limit`) | `—` before the first answer |
| best week | **Not derivable** without a `Trend` query per character. | drop the card; show "pulls this week" instead — cards whose `start_utc_ms` is within the last 7×86 400 000 ms |
| Mythic+ by key | group `kind == Key` by `key.map_id`; `name` = `card.name`; `best_level` = max `key.level`; timed/over split by `success`; sort by `best_level` desc then runs desc, cap 5 | empty panel with "no keys this season" |
| raid progression | group `kind == Encounter` by `(encounter.id, difficulty)`; keep the best kill `duration_ms` and, when no kill, the min `best_pct`; order by newest `start_utc_ms` desc, cap 6. `week_pulls`/`week_kills` over the last 7 days. | `best_pct` is `Option` on the card and is documented as **never written yet** — when it is `None`, render "no kill · N pulls", not "100%". |
| me | pick the scoped guid's `CardPlayer` on each card. Role = `p.role()`. Measure by role: Dps → `p.effective_dps(card.duration_ms)`, Healer → `p.hps`, Tank → `p.dtps`. median = middle of the sorted values; best = max. `deaths_per_pull` = Σ`deaths` / non-aborted pulls. `spark` = the last 12 cards' values, oldest first. | every field `Option`; `—` for each empty one |
| rank in role | on the NEWEST card the player is on: rank them among non-`enemy` players with the same `role()` by the same measure. Say "on your last pull" in the sub-label. | `—` when the card has fewer than 2 same-role players |
| characters | distinct `card.owner` values across all cards, resolved to the matching `CardPlayer` for name/class/spec; `fights` = cards where that guid is the owner; sort by `fights` desc | a card with `owner == None` contributes nothing |
| season progress | per role the scoped character actually played (≥3 cards in that role): `pct` = the share of that character's own historical values in that role that the LATEST value beats, ×100. Sub-label is fixed text: "vs your own history". | fewer than 3 points → `—` |
| recent | cards with `start_utc_ms` within 24 h, newest first, cap 8; tag from the same rules `view.rs::header_tag` uses, plus `fmt::key_tag(duration_ms, pars_ms, success)` for a keyed card | "nothing in the last day" |

Owner resolution: `home.character`, else the newest card's `card.owner`, else
`None` (Home renders but the "me" and "season progress" panels say "no owner
identified — set `history_characters` in the config").

**Do not** add a `HistoryQuery::Summary`, a `Trend` call, or a `Progression`
call in this slice. The design doc's own guidance is to start client-side and
promote to a query once the dashboard shape settles.

---

## 3. Wiring Home into `window.rs`

### 3.1 `Gui` fields (added)

```rust
/// The Home screen when open — window-local like `talents`.
pub(crate) home: Option<home::Home>,
/// Derived once per answer, not per frame.
pub(crate) home_panels: home::Panels,
/// The `?` sheet is up.
pub(crate) shortcuts_open: bool,
/// The row filter's text, applied client-side over the snapshot.
pub(crate) filter: String,
/// The filter box has focus — while true the meter keymap is swallowed so
/// the field is typable (the same trick `talents` uses).
pub(crate) filter_focused: bool,
/// Cached from the newest `Status` reply: whether the history store is on.
pub(crate) history_enabled: Option<bool>,
```

`Gui::new` initialises `home: None`, `home_panels: Panels::default()`,
`shortcuts_open: false`, `filter: String::new()`, `filter_focused: false`,
`history_enabled: None`.

Add to the `#[cfg(test)] impl Gui` block: `pub(crate) fn open_home(&mut self)`
(what the `~` key does) and `pub(crate) fn home(&self) -> Option<&home::Home>`.

### 3.2 New `Message` variants

```rust
/// `~` or the Home tab: open (or close) the window-local Home screen.
ToggleHome,
/// Home: page in the next slice of cards.
HomeRequest,
/// Home: scope the screen to this character guid (None = the owner).
HomeCharacter(Option<String>),
/// A view tab was clicked (the pointer twin of d/h/i/c/x/K/T).
PickView(wowdps_model::View),
/// Leave Home for the live meter (the `m` gesture / the Live tab).
GotoLive,
/// `?`: show/hide the shortcut sheet.
ToggleShortcuts,
/// The filter field's text changed.
Filter(String),
/// `/`: focus the filter field (returns a focus Task).
FocusFilter,
/// The filter field gained/lost focus.
FilterFocused(bool),
```

### 3.3 `drain_client` change

`DaemonMsg::History` and `DaemonMsg::Fight` are already **no-ops** in
`ClientState::on_msg` (see `crates/proto/src/state.rs`, the fall-through arm
at ~line 708). So intercept them exactly the way `Loadout` is intercepted:

```rust
if matches!(msg, DaemonMsg::Loadout { .. }
              | DaemonMsg::History { .. }
              | DaemonMsg::HistoryChanged { .. }) {
    intercepted.push(msg);
    continue;
}
```

In `Message::Tick`'s handling of the intercepted vec, add:

- `DaemonMsg::History { req_id, answer }` → `home.absorb(req_id, &answer)`;
  then recompute `state.home_panels = home::derive(&home.cards, owner, &season)`
  and, if `home.next_request(...)` yields one, send it.
- `DaemonMsg::HistoryChanged { .. }` → if Home is open, reset its paging
  (`cards.clear()`, `cursor = None`, `pages = 0`) and re-request. Debounce is
  unnecessary: the store writes once per closed fight.
- `DaemonMsg::Fight { .. }` stays a no-op in this slice (Home has no
  open-a-stored-fight action yet); do not intercept it.

### 3.4 Lifecycle

- **Open on start.** After the first `SegmentList` lands, if
  `cfg.home_on_start` and the daemon is not live (`!state.state.is_live()`),
  open Home. Implement as: in `Message::Tick`, once per process (a
  `home_considered: bool` guard field, or reuse `last_snapshot_at.is_some()`
  plus a `bool`), evaluate the condition and open Home. Do **not** open Home
  before the first snapshot — that would flash the dashboard over a live pull.
- **Auto-dismiss.** When a snapshot arrives with `state.state.is_live()` true
  and the previous drain was not live, close Home (`home = None`). This is the
  doc's "also auto-jumps when a pull starts". Track the previous liveness in a
  `was_live: bool` field.
- **While open** Home replaces the whole screen in `view::view`, before the
  `app.screen` match and after the talents check (so the talent viewer still
  wins). `ClientState` keeps ticking underneath, untouched.
- **Requests** are sent when Home opens (`next_request` with `req_id` from the
  existing `next_req_id` counter) and again each time an answer lands until
  `pages == MAX_PAGES` or `cards.len() as u32 >= total`.
- **While outstanding** Home renders its full panel skeleton with every value
  as `…` and a single dim line "reading the history store…". Once
  `home.answered` is true and `cards` is empty, it renders "no stored fights
  yet" plus, when `history_enabled == Some(false)`, the store's own reason
  from `Status.history.error`. To have that, `Gui::new` sends one
  `ClientMsg::GetStatus { req_id }` at startup and the intercepted
  `DaemonMsg::Status` (also a `ClientState` no-op — **verify** in `state.rs`;
  if it is not a no-op, read the value out without intercepting) fills
  `history_enabled` and the reason string.

### 3.5 The filter

Applied purely in `view.rs`, never in `ClientState`:

```rust
fn filtered(rows: Vec<Row>, filter: &str) -> Vec<Row>
```

Case-insensitive substring over `Row.label`. Empty filter = identity.
`meter_rows`, `list_screen`'s rows and `drill_pane`'s by-spell/by-target lists
all pass through it. **Ranks and percentages are NOT recomputed** — a filtered
row keeps the rank and share it has in the unfiltered chart, which is the
whole point of filtering. Selection (`row_sel`) is untouched; when the
selected row is filtered out nothing is highlighted, and that is fine.

The filter box lives in the meter screen's filter strip, above the captions.
Focus is a `text_input::Id` const in `nav.rs` (`pub(crate) fn filter_id() ->
text_input::Id`, built from a `'static` string — **verify** whether 0.14's
`text_input::Id::new` is const-callable; if not, build it per call, it is
cheap). `Message::FocusFilter` returns `iced::widget::text_input::focus(id)`
as the `Task` — **verify** the free function's name and signature in 0.14.

---

## 4. Config

`crates/gui/src/config.rs` uses **serde + the real `toml` crate**, not the
daemon's hand-rolled subset — so `Option<String>`, `f32`, `bool` and `String`
all round-trip already (`monitor: Option<String>` proves it). Two rules:

1. **Add only scalar keys.** The struct has `#[serde(flatten)] extra:
   toml::Table`; a nested `[season]` table risks toml's
   "values must be emitted before tables" ordering error on save. Flat keys
   only.
2. Every new field goes in `Default`, and in the `saved_config_round_trips`
   test's literal (that test constructs `Config` exhaustively and will not
   compile otherwise — good).

| key | type | default | meaning |
| --- | --- | --- | --- |
| `season_label` | `String` | `"this season"` | Shown in the Home selector and the top-line eyebrow. |
| `season_start` | `Option<String>` | `None` | `YYYY-MM-DD`, UTC. `None` = no lower bound. |
| `season_end` | `Option<String>` | `None` | `YYYY-MM-DD`, UTC, exclusive. `None` = open-ended. |
| `density` | `String` | `"comfortable"` | `Density::from_name`; an unknown value falls back to comfortable without erroring. |
| `home_on_start` | `bool` | `true` | Open Home at launch when no fight is live. |

Serialize `density` as a plain string, not an enum with
`#[serde(rename_all)]` — a typo in a hand-edited config must not make the
whole file fail to parse and trip the existing `load_failed` guard.

### 4.3 Date parsing, no chrono

`home.rs`:

```rust
/// "YYYY-MM-DD" → milliseconds since the Unix epoch, UTC midnight.
/// Returns None for anything that is not exactly that shape or is not a
/// real date. Days-from-civil (Howard Hinnant's algorithm), integer only.
pub(crate) fn parse_ymd(s: &str) -> Option<i64>;
```

Implement with the standard civil-days formula; add a unit test with
1970-01-01 → 0, 2000-03-01 → 951 868 800 000, 2026-08-12, and rejects for
`"2026-13-01"`, `"2026-02-30"`, `"today"`, `""`.

---

## 5. Keymap delta

`keys.rs` gains a declarative binding table, which is both the documentation
source for the `?` sheet and a test surface:

```rust
pub(crate) struct Binding {
    pub keys: &'static str,     // "d", "esc", "ctrl +"
    pub what: &'static str,     // "damage view"
    pub group: &'static str,    // "views" | "move" | "screens" | "zoom"
    /// Handled by `action_for`, or window-local (Home, talents, filter).
    pub window_local: bool,
}
pub(crate) const BINDINGS: &[Binding];
```

`action_for` itself is **unchanged** — the new keys are all window-local and
are matched in `window.rs::update` before `keys::action_for` is consulted, in
this order:

1. zoom chords (existing)
2. talent viewer open → existing swallow branch
3. shortcut sheet open → any key dismisses it
4. filter focused → only Esc (blur + clear focus) and Enter (blur, keep text)
   are handled; every other key is left to the text input
5. the new window-local keys below
6. `t` (existing)
7. `keys::action_for`

| key | when | effect | ClientState touched? |
| --- | --- | --- | --- |
| `~` | anywhere except talents / filter-focused | `Message::ToggleHome` — open Home, or close it and fall back to whatever `app.screen` already is | **no** |
| `?` (i.e. `Key::Character("?")`) | anywhere except filter-focused | `Message::ToggleShortcuts` | no |
| `/` | anywhere except talents | `Message::FocusFilter`, sets `filter_focused = true` | no |
| `m` | only while Home is open | close Home and `state.pin_live()` (the existing `ClientState::pin_live`, whose requests are sent) | yes — via the existing accessor |
| `Esc` | precedence: talents → shortcut sheet → filter (blur+clear) → Home (close) → `Action::Back` | walks one level up, exactly as the view map says | only in the last case |
| everything else | — | unchanged | — |

`~` arrives as `Key::Character("~")` in `modified_key` on a US layout; also
accept `Key::Character("`")` with `Modifiers::SHIFT` as a belt-and-braces
alternative, since layouts differ. Both map to `ToggleHome`.

`H` (History) is **reserved, not bound** — the History screen is a later
slice, and a key that does nothing is worse than no key. The History tab
renders disabled.

Home is never a `Screen` variant. `ClientState::screen` keeps whatever value
it had; closing Home simply stops overriding the render. The one interaction
with the shared machine is `m` → `pin_live()`, and the tab bar's view tabs →
`Action::SetView(v)` through the existing `apply`.

---

## 6. Test plan

All tests are `#[cfg(test)]` in their own modules, using the existing seams:
`window::testkit` (`Bridge`, `gui_over`, `chr`, `named`, `key`, `render`,
`simulator`, `isolate_config`), `wowdps_daemon::mock::{MockDaemon, pump}`, and
`iced_test` + `iced_tiny_skia` through `testkit::render`.

The mock already answers `GetHistory` from an in-memory store, and
`MockDaemon::fixture().with_history()` replays every fixture fight into it —
that is the Home fixture; no new mock work is needed.

### `theme.rs`

- `light_classes_darken_and_flip_ink` — for Priest, Rogue, Monk, Warrior:
  `accent(..).light == true`, `lift` darker than `base`, `ink` luminance < 0.1.
- `dark_classes_lift_and_keep_light_ink` — DK, Shaman, Warlock, Druid, DH,
  Evoker: `light == false`, `lift` brighter than `base`.
- `warrior_sits_on_the_light_side_of_the_threshold` — the doc names Warrior as
  the boundary case; pin it explicitly so a threshold tweak fails loudly.
- `every_class_has_a_legible_accent` — over all 13 classes, the contrast
  between `ink` and `base` exceeds a fixed ratio (implement WCAG contrast in
  the test, not in `theme.rs`).
- `no_class_is_the_neutral_accent` and `accent(None, None) == NEUTRAL`.
- `densities_are_ordered` — compact metrics are strictly smaller.

### `nav.rs`

- `tab_bar_renders_every_view_and_marks_the_active_one` — build with
  `View::ALL`, render through `testkit::render`, assert no panic and (via
  `Simulator::find` on text, **verify** the finder API in `iced_test` 0.14)
  that the active label is present.
- `a_disabled_tab_emits_no_message` — construct the History tab, simulate a
  click, assert nothing is produced.
- `stat_cards_render_a_headline_and_an_em_dash` — a `Stat` with value `"—"`
  renders; the headline card draws on the gradient (just assert it renders —
  the gradient path is exercised by the software renderer, which is the point).
- `shortcut_sheet_lists_every_binding` — the sheet's text contains each
  `keys::BINDINGS` entry's `keys` field.

### `keys.rs`

- `bindings_table_covers_every_action_key` — every character `action_for`
  answers appears in `BINDINGS`, and every `BINDINGS` entry with
  `window_local == false` is answered by `action_for`.
- Existing tests must still pass untouched (`action_for` is unchanged).

### `home.rs`

- `parse_ymd_round_trips_and_rejects_nonsense` (§4.3's cases).
- `derive_over_the_fixture_store` — build `MockDaemon::fixture()
  .with_history()`, pull every card out of `mock.history()`, call `derive`,
  and assert: `recent` is non-empty, the "me" panel names a real fixture
  player, and `Panels` is stable across two calls.
- `unknowable_numbers_render_as_em_dash` — assert the season-score `Stat`'s
  value is `"—"` and that no panel invents a `0` where the data is absent.
  This is the test that stops a future coder from "fixing" the dash.
- `an_empty_store_derives_empty_panels_without_panicking`.
- `a_card_with_no_owner_contributes_no_character`.
- `role_measure_follows_the_subject` — synthesize three `CardPlayer`s (tank,
  healer, dps) and assert `MePanel::measure` is `dtps` / `hps` /
  `effective dps` respectively.
- `home_screen_renders` — `testkit::render(home::screen(...))` for: loading,
  empty, and populated states.

### `window.rs`

- `tilde_opens_and_closes_home` — `Bridge::new(MockDaemon::fixture()
  .with_history())`, send `chr("~")`, assert `gui.home.is_some()` and
  `gui.state.screen` is unchanged; send it again, assert `None`.
- `home_asks_the_daemon_for_fights_and_pages` — after opening Home and
  settling, `b.requests()` (captured before the settle consumes them, or
  assert on the mock's side) contains a
  `ClientMsg::GetHistory { query: HistoryQuery::Fights { .. }, .. }`, and
  `gui.home().unwrap().cards` is non-empty.
- `a_stale_history_reply_is_dropped` — push a `DaemonMsg::History` with an
  unknown `req_id`; the card list must not grow. (Mirrors the existing
  `a_stale_loadout_reply_is_dropped`.)
- `home_closes_when_a_pull_starts` — `Bridge::new(MockDaemon::fixture_live())`,
  open Home, settle, assert it auto-dismissed.
- `the_meter_keymap_is_swallowed_while_the_filter_has_focus` — send
  `chr("/")`, then `chr("q")`; assert `!gui.state.quit`; send
  `named(Named::Escape)`, then `chr("q")`, assert it quits.
- `esc_walks_one_level_up_through_the_new_layers` — open Home, open the
  sheet, press Esc → sheet closed / Home open; Esc → Home closed; Esc →
  `Action::Back` reached `ClientState`.
- `question_mark_toggles_the_sheet`.
- `view_tabs_switch_the_view_like_the_keys` — `Message::PickView(View::Taken)`
  leaves `gui.state.view == View::Taken`.
- `m_from_home_pins_live`.

### `view.rs`

- `the_filter_narrows_rows_without_renumbering` — over `testkit::kill()`,
  set a filter matching one player, assert the rendered rows are fewer and
  that the surviving row's rank label is its ORIGINAL rank.
- `an_empty_filter_is_the_identity`.
- `every_screen_still_renders_with_the_new_chrome` — extend whatever the
  existing render-every-screen test is to include the tab bar and chips.

Coverage: after this slice run `cargo llvm-cov --workspace`; the tree's
baseline is ~94.5% and Home must not drag it down — `derive` is pure and
cheap to cover.

---

## 7. Commit plan

Nine commits, each compiling and passing `cargo test -p wowdps-gui` on its
own. Conventional Commits per `CC.md`; scope `gui` throughout; none of these
is breaking (no CONTRACT.md, no `PROTO_VERSION`, no fixture goldens).

1. **`feat(gui): derive one chrome accent from the pinned class`**
   Adds `theme.rs` with `Accent`, the luminance rule, `lighten`/`darken`, the
   type scale, `Density`, and the palette constants moved out of `view.rs`
   (re-exported there so the overlay is untouched). Body explains the 0.6
   threshold and why Warrior is the calibration case. Tests: the `theme.rs`
   set above. Nothing renders differently yet.

2. **`refactor(gui): take sizes and colors from the theme module`**
   Replaces per-call-site literals in `view.rs` (and only `view.rs`) with
   `theme::size::*` and `theme::Density`. Pure substitution; existing render
   tests are the gate. No visual change beyond size normalisation.

3. **`feat(gui): add the nav shell widgets`**
   `nav.rs`: `Tab`, `tab_bar`, `chip_row`, `two_tone_title`, `Stat`,
   `stat_cards`, `panel`, `filter_box`, `shortcut_sheet`. Message-generic,
   unused by any screen yet, fully unit-rendered. Body notes the glyph choice
   and that the History tab is deliberately disabled.

4. **`feat(gui): put the tab bar and two-tone title on the meter`**
   `view.rs` adopts `nav::tab_bar` (view tabs + Home/Fights/Loadout, History
   disabled), `nav::two_tone_title` for the header, and `nav::stat_cards` for
   the existing header numbers. `window.rs` gains `Message::PickView` and
   `Message::ToggleHome` (the latter a no-op stub until commit 7). Footer hint
   strings shrink to two contextual hints.

5. **`feat(gui): filter meter rows by name`**
   `Gui::filter` / `filter_focused`, `Message::Filter` / `FocusFilter` /
   `FilterFocused`, the `/` key, the keymap swallow while focused, and
   `view::filtered` applied to the meter, list and drill panes. Ranks and
   shares stay unfiltered — say so in the body.

6. **`feat(gui): show every binding behind ?`**
   `keys::BINDINGS`, `nav::shortcut_sheet` wired to `Message::ToggleShortcuts`,
   and Esc's new precedence chain up to (but not including) Home.

7. **`feat(gui): read the season window from the config`**
   The five config keys, `Season`, `parse_ymd`, and the round-trip test
   updates. No UI consumer yet, so this stays small and reviewable.

8. **`feat(gui): a read-only home dashboard over stored fights`**
   `home.rs` in full: `Home`, paging, `derive`, `Panels`, `screen`. `window.rs`
   intercepts `DaemonMsg::History` / `HistoryChanged` in `drain_client`, sends
   the paged `GetHistory`, holds `home` / `home_panels`, and binds `~`, `m`,
   Home's Esc rung, and the auto-dismiss on a live pull. Body must state
   plainly which numbers are not derivable (season score, best week, the raid's
   boss denominator) and that they render as `—`.

9. **`docs(gui): record the slice-1 nav contract and the home fallbacks [skip ci]`**
   Update `CLAUDE.md`'s gui paragraph (Home is window-local like the talent
   viewer; the accent module; the filter; the `?` sheet) and add the
   knowledge-bundle entries the `knowledge-bundle` skill asks for — a decision
   record for "Home derives client-side from `Fights` rather than adding a
   `Summary` query", and one for the luminance rule. Run `okf validate`; it
   must exit 0.

---

## 8. Things the coder should push back on

- If `iced 0.14`'s `container` cannot take a gradient background, do **not**
  fake it with an image or a canvas behind every card. Use the two-container
  stack described in §1.1 and note it; the design survives a flat accent.
- If `text_input::focus` cannot be driven from `update` without an `Id`
  round-trip that fights the `Element<'static, _>` idiom, drop the `/` key
  from commit 5 and ship the filter as click-to-focus only. A half-working
  keyboard focus is worse than none.
- If paging 2 000 cards through the socket visibly stalls the 100 ms tick,
  lower `MAX_PAGES` rather than moving work off-thread — Home is a front door,
  not a report, and "the last 1 000 fights" is an honest scope to state in the
  panel caption.
