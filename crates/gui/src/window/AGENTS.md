# crates/gui/src/window

The meter window: top bar, pull rail, Home or the stage (fight header,
ribbon, view tabs, meter, inspector), the palette and the cards. It was
built from a design prototype that is not in this repository: its Tokens
are the `navy` theme (`crates/gui-logic/src/theme/navy.rs`).

## Rules

- **The prototype's values stand.** A size, color or behavior came from
  it or its Tokens, and the code cites its selector (`.fhead`, `.rail`,
  `.insp`, `.pal`). A deliberate difference is stated in the PR's
  description.
- **Draw through `W`** (`w.rs`): tokens, type scale, pitches, accent,
  faces, zoom (`w.z(…)`) and width. The breakpoints are gui-logic's
  `theme::TILE_WINDOW` (1180) and `theme::NARROW_WINDOW` (820), inclusive,
  read at zoom 1 (`Gui::width`).
- **One stage, two sources.** A pull draws from `Gui::fight()`, the tailed
  log's `ClientState` or a stored pull's own, and every gesture goes
  through `Gui::act`. Never write a renderer for one source.
- **Window-local stays local.** Home, the rail, the palette, the `?` sheet
  and the talent viewer have no `Screen` variant: `ClientState`, the TUI and
  the overlay never learn of them.
- **Keys.** Meter actions come from gui-logic's chord table
  (`keys::ACTIONS`) and ride `keys::Do(Action)` in `Meter && !Input`. The
  TUI's parity test iterates that table, so a new meter key needs its TUI
  binding or a contract carve-out. Window-only gestures are
  `keys::Go(Gesture)`, listed in `keys::BINDINGS` with `window_local:
  true`. The other contexts are `Modal`, `Filter`, `Palette` and `Talents`;
  a field's keys bind on `Filter > Input` or `Palette > Input`.
- **Esc walks one level up:** the palette, the talent viewer and the menus
  first, then the rail's drawer, the filter's text, the inspector's ability
  or keys, the comparison, a widened inspector, and Home, where it ends. A
  new layer takes its place in that chain.
- **Sorting and filtering change what is drawn, never what a row means.**
  A row keeps the daemon's index, so ranks, shares, the bar's scale and
  click targets hold; `j`/`k` walk the drawn order (`table::meter_step`).
  The filter is a player filter and never narrows the inspector's lists.
- **One column model** (gui-logic's `table`): a list's headings, rows and
  total come from one column set, so they cannot drift.
- **"You" is the daemon's `mine`** (`Gui::owner_of`); the window matches no
  names. The configured characters are the fallback when nothing is marked.
- **The inspector follows the selection** through `ClientState::set_follow`
  (opt-in; the TUI never calls it), its drill keyed by guid, and holds the
  last body dimmed until the next lands (`inspector::model::Held`). Its
  graph's geometry, hit tests and words are gui-logic's `inspect`; the
  components bring only a text measure and their paint.
- **A comparison shares one y-scale and one x-range.** Per-side scaling
  makes every pair look alike.
- **Stored pulls:** one read in flight for the whole window. What the store
  kept no answer for is refused with a toast (`toast::stored_refusal`),
  never asked.
- **Home derives from `Fights` answers.** It words a disabled, a cold and a
  degraded store apart, and shows nothing no card carries (no season score,
  no boss denominator, no ladder percentile).
- **Colors.** The accent is the interface, class colors are people, green
  and red are outcomes, the games' colors (`midnight`, `forever`) are the
  games, and no color is semantic yellow. A game's color is worn by its
  mark and by the shown game's chip in the edition switch, nowhere else:
  no tinted bar, rail, border or ground. The accent is an
  underline or an edge, never a fill. Class color as text is lifted until it
  clears WCAG AA (`theme::class_text_on`); a bar keeps `Class::rgb`
  exactly. Bars sit under the row's text; no text sits on a class color.
- **Class chrome wears the owner's class** (`Gui::learn_class`), never the
  selection's, which would recolor the window on every re-sort.
- **Words** come from gui-logic's `labels`: sentence case, the prototype's
  view names (`labels::WINDOW_VIEWS`), `hide_realms` honored in every pane
  (`labels::realmless`). Icons are strokes from gui-logic's `glyph` table,
  never SVG assets.
- **Type is bundled** (`crates/gui-logic/fonts/`): Barlow Semi Condensed
  Tabular for names and numbers, Marcellus for encounter titles and the
  wordmark alone.
- **GPUI traps.** A border draws outside the padding, so pad by the design
  value less the edge (`cards::BORDER`). The default line height is φ; a
  piece that must sit on 1.3 sets it.
- **Cards are modal for keys:** one menu at a time, any key closes it, the
  zoom chords still zoom, Ctrl K replaces it with the palette. The toast is
  the window's own (`cards/toast.rs`), not Kit's `Notification`.
- **Text fields** are the window's `Field` (`field.rs`) over Kit's unstyled
  input, so placeholder, caret and selection wear the window's tokens.
- **Scrollbars** are `crate::scrollbar`, never Kit's self-hiding one. A
  list scrolls the least that shows the whole selected row.
- **Config writes:** a gesture's key is written alone through
  `Config::store` (or `store_character`, `store_character_class`), never a
  launch-time copy. A key of one edition's (the character, its class,
  Home's dates) is read through `Gui::keys()` (`config::KeysOf`: the
  top-level key while one edition is known) and written through
  `Config::set_*_in(edition, …)` for the edition the window is in
  (`Hist::edition`): Retail's are the top-level keys.
- **Editions** (`editions.rs`, gui-logic's `editions`): one game at a
  time, by default the one played. The switch lists games, never modes: a
  pick of the game followed anyway is `Follow`, any other a pin, which
  follows again once its game has been live and moved on. It rides
  `ClientState::set_edition` for the meter and, only once the census knows
  more than one edition, every history read (`Store::rescope`). With one
  edition known nothing is drawn or asked differently. The switch is a
  row of chips after the places while the games fit (`editions::chips`:
  up to three, a bar that is not narrow), the shown game's chip filled
  with its color and the live dot on the played game's; else a dropdown
  and its menu (`Gui::edition_chips` keeps the menu shut while the chips
  are drawn). At the tile frame beside the chips the jump box is its
  glyph. Each game has one mark and one color (gui-logic's
  `editions::Mark` and `Tone`, the marks glyphs of the `glyph` table),
  worn in one round badge (`marks.rs`) wherever the game is named: a
  chip, the dropdown and its menu, a fight's tag (`head-edition`, once
  several games are known) and an empty game's stage. The badge holds the
  game's emblem where the per-machine `edition-emblems.bin`
  (`tools/gen-emblems.sh`) has one, else our mark, the same size either
  way (`editions::tests::a_badge_keeps_the_switch_s_rhythm`). A mark's
  color yields to the bar's ink where the chrome looks too like it
  (`Tone::mark_color`). The marks' pictures are committed and guarded
  (`shots::marks_render_guard`, drawn without the art). The talent viewer
  reads the fight's game's trees (`Viewer::edition`).
- **The chrome budget is a test.** At 1440×900 the first meter row starts
  under 290 px with 18 rows showing
  (`window::tests::the_chrome_leaves_a_raid_its_rows`, every `cargo test`).
  A header change keeps it.

## The talent viewer (`crates/gui/src/talents/`)

- Its logic is gui-logic's `talents` (the layout model, the editing state
  machine `viewer`, the geometry). Each pane is one canvas painted in the
  game's layer order (`talents/pane.rs`).
- The logged loadout (`GetLoadout`) wins over a stored SimC paste and is
  never persisted; pastes persist per character under
  `~/.local/share/wowdps/simc/`.
- Without `talents.json` or `talent-art.bin` it draws plain panels.
- `cargo test -p wowdps-gui real_dataset_draws_every_spec -- --ignored`
  draws every spec of the real dataset.
