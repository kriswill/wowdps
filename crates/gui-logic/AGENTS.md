# crates/gui-logic (wowdps-gui-logic)

The GUI's framework-free half: config, Hyprland IPC, the keymap, history
pages, the art-cache readers, the fonts, and every model, word and geometry
the window, the overlay and the talent viewer draw from. Model + proto +
serde/toml, no UI framework. Module list: `docs/OKF/crates/gui-logic.md`.

## Rules

- **Nothing here draws.** What the GUI computes without painting lives
  here, with its tests; the GUI brings a text measure and its paint.
- **`config.rs` is the one writer** of `~/.config/wowdps/config.toml` for
  the window and the overlay. Every save is atomic, and a gesture's key is
  written alone (`Config::store`, `store_character`,
  `store_character_class`), never a whole launch-time copy over another
  surface's change.
- **`keys::ACTIONS` is every frontend's keymap.** The GUI registers it and
  the TUI's parity test (`crates/tui/tests/keybind_parity.rs`) holds the
  TUI to it.
- **Themes are data.** The built-ins (`navy`, `onyx` the default, `frost`)
  are a file each under `theme/`. Every token group is written by
  `theme::tokens!`, so each field is the config key of the same name under
  `[themes.<name>.<group>]`. A new color is a token on every `Def`, with
  navy's values unmoved. Old names keep reading (theme `gold` is navy,
  chrome `"gold"` the theme's own). `wowdps-gui --print-theme <name>`
  prints every key.
- **Readers never panic.** The art-cache readers (`icons`, `spell_icons`,
  `talent_art`, `lazy_tiles`) answer `None` for an absent, truncated or
  garbage file.
- **Test hooks cross the crate line by feature:** gate a hook the GUI's
  tests call with `cfg(any(test, feature = "test-support"))`, never plain
  `#[cfg(test)]`. A release build never carries it.
- **Fonts are assets** (OFL, under `fonts/`); their provenance and the
  tabular bake are in `fonts/README.md`.
- **`fold`** folds Latin-1 and Latin Extended-A only; other scripts stay
  untransliterated on purpose.
- **`single.rs`'s takeover socket is unversioned** on purpose: a new
  overlay evicts any running one, whatever build either is.
- **The overlay's models** (`graph`, `drill`, `surface`, `output`,
  `timeline`, the overlay tokens) are held by the overlay render guard: run
  it before merging a change to them (`crates/gui/AGENTS.md`).

```sh
cargo test -p wowdps-gui-logic
```
