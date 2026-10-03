//! A config's themes: the built-ins, the tokens its `[themes.<name>]`
//! tables override on them, and themes of its own built on any of them.
//!
//! ```toml
//! theme = "onyx"
//!
//! [themes.onyx.window]          # a built-in, a token or two changed
//! ground = "#050505"
//!
//! [themes.ember]                # a theme of the config's own
//! base = "navy"                 # what it starts from; navy when unsaid
//! label = "Ember"               # its name in the ⚙ card
//! accent_label = "Ember"        # its own chrome's name there
//! [themes.ember.window]
//! accent = "#FF7A3D"
//! label_ink = "#C77B52"
//! [themes.ember.faces]
//! title = "Marcellus"
//! [themes.ember.shape]
//! scale = 0.5
//! [themes.ember.effects]
//! glass = true
//! ```
//!
//! Groups: `window`, `overlay`, `talents`, `data` (colours, `#rrggbb` or
//! `#rrggbbaa`), `faces` (family names), `size`, `pitch`, `shape`, `bars`
//! (numbers), `effects` (switches) — each key a field of its group
//! ([`super::WindowTokens`] …), so [`theme_toml`] prints every key a theme
//! has. A built-in never needs to be in the config to be chosen; a table
//! named for one changes only what it says. Nothing in a config is fatal:
//! an unknown token, a colour that does not parse, a base that names no
//! theme — each is a line in [`Registry::warnings`] and is otherwise left
//! out.
//!
//! The registry hands out `&'static Def`s, as the built-ins are: a theme
//! built from a config is interned, so building the registry again (the
//! overlay re-reads the config when the window switches themes) reuses
//! every definition that did not change rather than leaking a new one.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use super::color::Color;
use super::defs::{
    DataTokens, Def, Effects, Faces, NAVY, OverlayTokens, THEMES, WindowTokens, builtin,
};
use super::metrics::{Bars, Pitches, Shape, Sizes};
use super::talent_tokens::TalentTokens;

/// Every theme a config can choose, and what it got wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Registry {
    defs: Vec<&'static Def>,
    /// One line per mistake in the config's `[themes]`, for the GUI to say.
    pub warnings: Vec<String>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

/// How deep a chain of `base`s may go before it is taken for a loop.
const MAX_DEPTH: usize = 16;

/// The largest measure a config may set: anything bigger is a typo.
const MAX_NUMBER: f64 = 1000.0;

impl Registry {
    /// The built-ins alone.
    pub fn builtin() -> Self {
        Self {
            defs: THEMES.to_vec(),
            warnings: Vec::new(),
        }
    }

    /// The built-ins with a config's `[themes]` table laid over them: the
    /// built-ins first in their own order, then the config's own themes
    /// by name.
    pub fn from_table(themes: &toml::Table) -> Self {
        let mut build = Build {
            table: themes,
            done: BTreeMap::new(),
            visiting: Vec::new(),
            warnings: Vec::new(),
        };
        let mut names: Vec<String> = Vec::new();
        for key in themes.keys() {
            let Some(name) = canonical(key) else {
                build.warnings.push(format!(
                    "themes.{}: a theme's name is letters, digits, '-' and '_'",
                    bare(key)
                ));
                continue;
            };
            if names.contains(&name) {
                build.warnings.push(format!(
                    "themes.{}: another table already names {name:?}",
                    bare(key)
                ));
                continue;
            }
            build.resolve(key, &name);
            names.push(name);
        }
        let mut defs: Vec<&'static Def> = THEMES
            .iter()
            .map(|d| build.done.get(d.name).copied().flatten().unwrap_or(d))
            .collect();
        for (name, def) in &build.done {
            if builtin(name).is_none()
                && let Some(def) = def
            {
                defs.push(def);
            }
        }
        Self {
            defs,
            warnings: build.warnings,
        }
    }

    /// Every theme, in the ⚙ card's order.
    pub fn themes(&self) -> &[&'static Def] {
        &self.defs
    }

    /// The theme `name` spells (any case; a built-in's old name too).
    pub fn get(&self, name: &str) -> Option<&'static Def> {
        let canonical = builtin(name).map_or_else(|| name.trim().to_lowercase(), |d| d.name.into());
        self.defs.iter().copied().find(|d| d.name == canonical)
    }

    /// The theme config `theme` names; a name it does not know is `navy`
    /// (the config's own `navy`, overrides and all).
    pub fn named(&self, name: &str) -> &'static Def {
        self.get(name)
            .or_else(|| self.get(NAVY.name))
            .unwrap_or(&NAVY)
    }
}

/// A theme's name as a config key spells it: lowercase, and only letters,
/// digits, `-` and `_`.
fn valid_name(key: &str) -> Option<String> {
    let name = key.trim().to_lowercase();
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    ok.then_some(name)
}

/// The theme a config key names: [`valid_name`], a built-in's old name
/// (`gold`) read as the built-in's.
fn canonical(key: &str) -> Option<String> {
    let name = valid_name(key)?;
    Some(builtin(&name).map_or(name, |d| d.name.to_string()))
}

/// A key as a TOML path spells it: bare when it may be, else quoted — so
/// `"x.y"` never reads as two tables.
fn bare(key: &str) -> String {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain {
        key.to_string()
    } else {
        format!("{key:?}")
    }
}

/// The work of one [`Registry::from_table`]: each table resolved once,
/// its base first.
struct Build<'a> {
    table: &'a toml::Table,
    /// Each theme resolved so far, by name; `None` for a table that failed.
    done: BTreeMap<String, Option<&'static Def>>,
    /// The chain being resolved, so a base that leads back into it is a loop.
    visiting: Vec<String>,
    warnings: Vec<String>,
}

impl Build<'_> {
    fn resolve(&mut self, key: &str, name: &str) -> Option<&'static Def> {
        if let Some(done) = self.done.get(name) {
            return *done;
        }
        let at = format!("themes.{}", bare(key));
        if self.visiting.iter().any(|v| v == name) {
            let chain = self.visiting.join(" → ");
            self.warnings.push(format!(
                "{at}: its bases go round in a loop ({chain} → {name}); starting from navy"
            ));
            return None;
        }
        if self.visiting.len() > MAX_DEPTH {
            self.warnings.push(format!(
                "{at}: its bases go more than {MAX_DEPTH} deep; starting from navy"
            ));
            return None;
        }
        let Some(toml::Value::Table(entry)) = self.table.get(key) else {
            self.warnings
                .push(format!("{at}: a theme is a table, [{at}]"));
            self.done.insert(name.to_string(), None);
            return None;
        };
        self.visiting.push(name.to_string());
        let start = self.base(key, name, entry);
        self.visiting.pop();
        let own = builtin(name).filter(|d| d.name == name);
        let mut def = Def {
            name: intern(name),
            label: own.map_or_else(|| intern(&title_case(name)), |d| d.label),
            ..*start
        };
        apply(&mut def, &at, entry, &mut self.warnings);
        // A theme of the config's own that moved its accent has no name for
        // it unless it says one: its base's ("White") would be wrong.
        if own.is_none()
            && def.window.accent != start.window.accent
            && !entry.contains_key("accent_label")
        {
            def.accent_label = "Theme";
        }
        let def = intern_def(def);
        self.done.insert(name.to_string(), Some(def));
        Some(def)
    }

    /// What `[themes.<key>]` starts from: its `base`, else the built-in it
    /// is named for, else `navy`.
    fn base(&mut self, key: &str, name: &str, entry: &toml::Table) -> &'static Def {
        let own = || builtin(name).filter(|d| d.name == name);
        let Some(base) = entry.get("base") else {
            return own().unwrap_or(&NAVY);
        };
        let at = format!("themes.{}.base", bare(key));
        let Some(base) = base.as_str().and_then(canonical) else {
            self.warnings
                .push(format!("{at}: {base} is not a theme's name, as a string"));
            return own().unwrap_or(&NAVY);
        };
        if base == name {
            return own().unwrap_or_else(|| {
                self.warnings.push(format!(
                    "{at}: a theme cannot start from itself; starting from navy"
                ));
                &NAVY
            });
        }
        let other = self
            .table
            .keys()
            .find(|k| canonical(k).as_deref() == Some(base.as_str()))
            .cloned();
        if let Some(other) = other {
            return self.resolve(&other, &base).unwrap_or(&NAVY);
        }
        builtin(&base).unwrap_or_else(|| {
            self.warnings.push(format!(
                "{at}: no theme is named {base:?}; starting from navy"
            ));
            &NAVY
        })
    }
}

/// A theme table's keys laid over `def`; `head` is its path
/// (`themes.<name>`), for the warnings.
fn apply(def: &mut Def, head: &str, entry: &toml::Table, warnings: &mut Vec<String>) {
    for (field, value) in entry {
        let at = format!("{head}.{}", bare(field));
        match field.as_str() {
            "base" => {}
            "label" => match value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => def.label = intern(s),
                None => warnings.push(format!("{at}: {value} is not a name, as a string")),
            },
            "accent_label" => match value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => def.accent_label = intern(s),
                None => warnings.push(format!("{at}: {value} is not a name, as a string")),
            },
            "dark" => match value.as_bool() {
                Some(b) => def.dark = b,
                None => warnings.push(format!("{at}: {value} is not true or false")),
            },
            "window" => group(&at, value, warnings, |k, v, w| {
                set_color(
                    &mut def.window,
                    WindowTokens::NAMES,
                    WindowTokens::get_mut,
                    k,
                    v,
                    w,
                )
            }),
            "overlay" => group(&at, value, warnings, |k, v, w| {
                set_color(
                    &mut def.overlay,
                    OverlayTokens::NAMES,
                    OverlayTokens::get_mut,
                    k,
                    v,
                    w,
                )
            }),
            "talents" => group(&at, value, warnings, |k, v, w| {
                set_color(
                    &mut def.talents,
                    TalentTokens::NAMES,
                    TalentTokens::get_mut,
                    k,
                    v,
                    w,
                )
            }),
            "data" => group(&at, value, warnings, |k, v, w| {
                set_color(
                    &mut def.data,
                    DataTokens::NAMES,
                    DataTokens::get_mut,
                    k,
                    v,
                    w,
                )
            }),
            "faces" => group(&at, value, warnings, |k, v, w| {
                set_face(&mut def.faces, k, v, w)
            }),
            "size" => group(&at, value, warnings, |k, v, w| {
                set_number(&mut def.size, Sizes::NAMES, Sizes::get_mut, k, v, w)
            }),
            "pitch" => group(&at, value, warnings, |k, v, w| {
                set_number(&mut def.pitch, Pitches::NAMES, Pitches::get_mut, k, v, w)
            }),
            "shape" => group(&at, value, warnings, |k, v, w| {
                set_number(&mut def.shape, Shape::NAMES, Shape::get_mut, k, v, w)
            }),
            "bars" => group(&at, value, warnings, |k, v, w| {
                set_number(&mut def.bars, Bars::NAMES, Bars::get_mut, k, v, w)
            }),
            "effects" => group(&at, value, warnings, |k, v, w| {
                set_switch(&mut def.effects, k, v, w)
            }),
            _ => warnings.push(format!(
                "{at}: not a theme key ({})",
                did_you_mean(field, &TOP_KEYS)
            )),
        }
    }
}

/// The keys a theme's own table may hold.
const TOP_KEYS: [&str; 14] = [
    "base",
    "label",
    "accent_label",
    "dark",
    "window",
    "overlay",
    "talents",
    "data",
    "faces",
    "size",
    "pitch",
    "shape",
    "bars",
    "effects",
];

/// Each `key = value` of a group's table, through `set`, which says where
/// any mistake is from the full path it is handed.
fn group(
    at: &str,
    value: &toml::Value,
    warnings: &mut Vec<String>,
    mut set: impl FnMut(&str, &toml::Value, &mut dyn FnMut(String)),
) {
    let Some(table) = value.as_table() else {
        warnings.push(format!("{at}: a table, [{at}]"));
        return;
    };
    for (k, v) in table {
        let mut warn = |m: String| warnings.push(format!("{at}.{}: {m}", bare(k)));
        set(k, v, &mut warn);
    }
}

fn set_color<T>(
    tokens: &mut T,
    names: &[&str],
    slot: for<'t> fn(&'t mut T, &str) -> Option<&'t mut Color>,
    k: &str,
    v: &toml::Value,
    warn: &mut dyn FnMut(String),
) {
    let Some(slot) = slot(tokens, k) else {
        return warn(format!("no such token ({})", did_you_mean(k, names)));
    };
    match v.as_str().and_then(Color::parse) {
        Some(c) => *slot = c,
        None => warn(format!(
            "{v} is not a colour: \"#rrggbb\", \"#rrggbbaa\" or \"#rgb\""
        )),
    }
}

fn set_number<T>(
    tokens: &mut T,
    names: &[&str],
    slot: for<'t> fn(&'t mut T, &str) -> Option<&'t mut f32>,
    k: &str,
    v: &toml::Value,
    warn: &mut dyn FnMut(String),
) {
    let Some(slot) = slot(tokens, k) else {
        return warn(format!("no such measure ({})", did_you_mean(k, names)));
    };
    let n = v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
    match n.filter(|n| (0.0..=MAX_NUMBER).contains(n)) {
        Some(n) => *slot = n as f32,
        None => warn(format!("{v} is not a number from 0 to {MAX_NUMBER}")),
    }
}

fn set_switch(effects: &mut Effects, k: &str, v: &toml::Value, warn: &mut dyn FnMut(String)) {
    let Some(slot) = effects.get_mut(k) else {
        return warn(format!(
            "no such effect ({})",
            did_you_mean(k, Effects::NAMES)
        ));
    };
    match v.as_bool() {
        Some(b) => *slot = b,
        None => warn(format!("{v} is not true or false")),
    }
}

fn set_face(faces: &mut Faces, k: &str, v: &toml::Value, warn: &mut dyn FnMut(String)) {
    let Some(slot) = faces.get_mut(k) else {
        return warn(format!("no such face ({})", did_you_mean(k, Faces::NAMES)));
    };
    match v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        Some(family) => *slot = intern(family),
        None => warn(format!("{v} is not a family's name")),
    }
}

/// "did you mean `x`?" for the closest of `names` within two edits (one
/// for a key of three letters or fewer, where two would suggest almost
/// anything), else the list to choose from.
fn did_you_mean(k: &str, names: &[&str]) -> String {
    let limit = if k.chars().count() <= 3 { 1 } else { 2 };
    let close = names
        .iter()
        .map(|n| (edits(k, n), *n))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d);
    match close {
        Some((_, n)) => format!("did you mean {n:?}?"),
        None => format!("one of {}", names.join(", ")),
    }
}

/// Levenshtein distance, by characters.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let above = row.get(j + 1).copied().unwrap_or(usize::MAX);
            let left = row.get(j).copied().unwrap_or(usize::MAX);
            let cost = prev + usize::from(ca != *cb);
            prev = above;
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = cost
                    .min(above.saturating_add(1))
                    .min(left.saturating_add(1));
            }
        }
    }
    row.last().copied().unwrap_or(0)
}

/// "my-theme" → "My theme": a config theme's name in the ⚙ card when it
/// gives no `label`.
fn title_case(name: &str) -> String {
    let words = name.replace(['-', '_'], " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `s` as a `&'static str`, leaked once per distinct string.
fn intern(s: &str) -> &'static str {
    static STRINGS: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let mut strings = STRINGS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(found) = strings.iter().find(|x| **x == s) {
        return found;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    strings.push(leaked);
    leaked
}

/// `def` as a `&'static Def`, leaked once per distinct definition.
fn intern_def(def: Def) -> &'static Def {
    static DEFS: OnceLock<Mutex<Vec<&'static Def>>> = OnceLock::new();
    let mut defs = DEFS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(found) = defs.iter().find(|d| ***d == def) {
        return found;
    }
    let leaked: &'static Def = Box::leak(Box::new(def));
    defs.push(leaked);
    leaked
}

/// Every key of `def` as a config's `[themes.<as_name>]` tables: a theme to
/// copy and edit (`wowdps-gui --print-theme onyx`). It reads back as `def`
/// with `as_name`, to the nearest 8-bit step of every colour.
pub fn theme_toml(def: &Def, as_name: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let head = format!("themes.{as_name}");
    let _ = writeln!(out, "[{head}]");
    // A built-in's copy keeps its base, so a token a later version adds
    // comes from the theme it was copied from, not from navy.
    if builtin(def.name).is_some_and(|b| b.name == def.name) {
        let _ = writeln!(out, "base = {:?}", def.name);
    }
    let _ = writeln!(out, "label = {:?}", def.label);
    let _ = writeln!(out, "accent_label = {:?}", def.accent_label);
    let _ = writeln!(out, "dark = {}", def.dark);
    let colors =
        |out: &mut String, group: &str, names: &[&str], get: &dyn Fn(&str) -> Option<Color>| {
            let _ = writeln!(out, "\n[{head}.{group}]");
            for n in names {
                if let Some(c) = get(n) {
                    let _ = writeln!(out, "{n} = \"{}\"", c.to_hex());
                }
            }
        };
    colors(&mut out, "window", WindowTokens::NAMES, &|n| {
        def.window.get(n)
    });
    colors(&mut out, "overlay", OverlayTokens::NAMES, &|n| {
        def.overlay.get(n)
    });
    colors(&mut out, "talents", TalentTokens::NAMES, &|n| {
        def.talents.get(n)
    });
    colors(&mut out, "data", DataTokens::NAMES, &|n| def.data.get(n));
    let _ = writeln!(out, "\n[{head}.faces]");
    for n in Faces::NAMES {
        if let Some(f) = def.faces.get(n) {
            let _ = writeln!(out, "{n} = {f:?}");
        }
    }
    let numbers =
        |out: &mut String, group: &str, names: &[&str], get: &dyn Fn(&str) -> Option<f32>| {
            let _ = writeln!(out, "\n[{head}.{group}]");
            for n in names {
                if let Some(v) = get(n) {
                    let _ = writeln!(out, "{n} = {v:?}");
                }
            }
        };
    numbers(&mut out, "size", Sizes::NAMES, &|n| def.size.get(n));
    numbers(&mut out, "pitch", Pitches::NAMES, &|n| def.pitch.get(n));
    numbers(&mut out, "shape", Shape::NAMES, &|n| def.shape.get(n));
    numbers(&mut out, "bars", Bars::NAMES, &|n| def.bars.get(n));
    let _ = writeln!(out, "\n[{head}.effects]");
    for n in Effects::NAMES {
        if let Some(b) = def.effects.get(n) {
            let _ = writeln!(out, "{n} = {b}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{FROST, ONYX};

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap_or_default()
    }

    #[test]
    fn with_no_themes_table_the_built_ins_are_all_there() {
        let r = Registry::from_table(&toml::Table::new());
        assert_eq!(r, Registry::builtin());
        assert_eq!(r.themes(), &[&NAVY, &ONYX, &FROST]);
        assert_eq!(r.named("onyx"), &ONYX);
        assert_eq!(r.named("Gold"), &NAVY, "the old name");
        assert_eq!(r.named("nope"), &NAVY);
        assert!(r.warnings.is_empty());
    }

    /// A table named for a built-in changes what it says and nothing else,
    /// and the built-in is still chosen by its name.
    #[test]
    fn a_built_in_takes_overrides() {
        let r = Registry::from_table(&table(
            "[onyx.window]\nground = \"#050505\"\n[onyx.effects]\nglass = false\n[onyx.size]\nencounter = 21\n",
        ));
        let onyx = r.named("onyx");
        assert_eq!(onyx.window.ground, Color::hex(0x050505));
        assert!(!onyx.effects.glass);
        assert_eq!(onyx.size.encounter, 21.0);
        assert_eq!(onyx.window.ink, ONYX.window.ink);
        assert_eq!(onyx.label, "Onyx");
        assert_eq!(r.themes().len(), 3);
        assert_eq!(r.named("navy"), &NAVY);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    /// A theme of the config's own starts from its base — which may be a
    /// built-in the config changed, or another theme of its own — and
    /// follows the built-ins in the card.
    #[test]
    fn a_config_defines_its_own_themes() {
        let r = Registry::from_table(&table(
            r##"
            [onyx.window]
            ground = "#010101"
            [ember]
            base = "onyx"
            label = "Ember glow"
            accent_label = "Ember"
            [ember.window]
            accent = "#FF7A3D"
            [ember.faces]
            title = "Marcellus"
            [ash]
            base = "Ember"
            [ash.shape]
            scale = 0
            [plain]
            "##,
        ));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let names: Vec<_> = r.themes().iter().map(|d| d.name).collect();
        assert_eq!(names, ["navy", "onyx", "frost", "ash", "ember", "plain"]);
        let ember = r.named("EMBER");
        assert_eq!(
            ember.window.ground,
            Color::hex(0x010101),
            "the changed onyx"
        );
        assert_eq!(ember.window.accent, Color::hex(0xFF7A3D));
        assert_eq!((ember.label, ember.accent_label), ("Ember glow", "Ember"));
        assert_eq!(ember.faces.title, "Marcellus");
        assert_eq!(ember.faces.ui, ONYX.faces.ui);
        let ash = r.named("ash");
        assert_eq!(ash.window.accent, Color::hex(0xFF7A3D));
        assert_eq!(ash.shape.scale, 0.0);
        assert_eq!(ash.label, "Ash");
        let plain = r.named("plain");
        assert_eq!(plain.window, NAVY.window, "navy when no base is said");
        assert_eq!(plain.label, "Plain");
    }

    #[test]
    fn mistakes_are_said_and_left_out() {
        let r = Registry::from_table(&table(
            r##"
            "bad name" = {}
            notatable = 3
            [mine]
            base = "nothing"
            colour = 1
            [mine.window]
            acent = "#fff"
            ink = "white"
            [mine.effects]
            glas = true
            dial = "yes"
            [mine.size]
            name = -3
            [loop1]
            base = "loop2"
            [loop2]
            base = "loop1"
            "##,
        ));
        let says = |part: &str| r.warnings.iter().any(|w| w.contains(part));
        assert!(
            says("themes.mine.base: no theme is named \"nothing\""),
            "{:?}",
            r.warnings
        );
        assert!(says("themes.mine.colour: not a theme key"));
        assert!(says(
            "themes.mine.window.acent: no such token (did you mean \"accent\"?)"
        ));
        assert!(says("themes.mine.window.ink: \"white\" is not a colour"));
        assert!(says(
            "themes.mine.effects.glas: no such effect (did you mean \"glass\"?)"
        ));
        assert!(says(
            "themes.mine.effects.dial: \"yes\" is not true or false"
        ));
        assert!(says("themes.mine.size.name: -3 is not a number"));
        assert!(says("its bases go round in a loop"));
        assert!(says("themes.\"bad name\": a theme's name"));
        assert!(says("themes.notatable: a theme is a table"));
        let mine = r.named("mine");
        assert_eq!(
            mine.window.ink, NAVY.window.ink,
            "a bad value changes nothing"
        );
        assert_eq!(mine.size.name, NAVY.size.name);
        assert!(r.get("notatable").is_none());
    }

    /// Building the same config twice hands out the same definitions: the
    /// overlay rebuilds on every config change and must not leak.
    #[test]
    fn a_rebuild_reuses_its_definitions() {
        let t = table("[ember.window]\naccent = \"#ff7a3d\"\n");
        let a = Registry::from_table(&t).named("ember");
        let b = Registry::from_table(&t).named("ember");
        assert!(std::ptr::eq(a, b));
    }

    /// A printed theme reads back as itself.
    #[test]
    fn a_printed_theme_reads_back() {
        for def in THEMES {
            let text = theme_toml(def, "copy");
            let parsed: toml::Table = toml::from_str(&text).unwrap_or_default();
            let themes = parsed.get("themes").and_then(|t| t.as_table()).cloned();
            let r = Registry::from_table(&themes.unwrap_or_default());
            assert!(r.warnings.is_empty(), "{}: {:?}", def.name, r.warnings);
            let copy = r.named("copy");
            assert_eq!(copy.faces, def.faces);
            assert_eq!(copy.size, def.size);
            assert_eq!(copy.effects, def.effects);
            assert_eq!(copy.window.ink.to_hex(), def.window.ink.to_hex());
            assert_eq!(copy.overlay.yellow.to_hex(), def.overlay.yellow.to_hex());
            assert_eq!(copy.label, def.label);
        }
    }

    /// A built-in's old name names the built-in in a table too, a base that
    /// is the theme itself says so, and a key that needs quotes is quoted.
    #[test]
    fn old_names_self_bases_and_quoted_keys() {
        let r = Registry::from_table(&table(
            "[gold.window]\nground = \"#ff0000\"\n[me]\nbase = \"me\"\n[\"x.y\"]\n",
        ));
        assert_eq!(r.named("navy").window.ground, Color::hex(0xFF0000));
        assert_eq!(r.named("gold").window.ground, Color::hex(0xFF0000));
        assert_eq!(r.themes().len(), 4, "navy changed, and me");
        let says = |part: &str| r.warnings.iter().any(|w| w.contains(part));
        assert!(
            says("themes.me.base: a theme cannot start from itself"),
            "{:?}",
            r.warnings
        );
        assert!(says("themes.\"x.y\": a theme's name"), "{:?}", r.warnings);
        assert!(!did_you_mean("bg", WindowTokens::NAMES).contains("did you mean"));
        let printed = theme_toml(&ONYX, "onyx");
        assert!(printed.contains("base = \"onyx\""));
    }

    #[test]
    fn names_and_suggestions() {
        assert_eq!(valid_name(" Ember "), Some("ember".into()));
        assert_eq!(valid_name("a b"), None);
        assert_eq!(edits("acent", "accent"), 1);
        assert_eq!(edits("", "abc"), 3);
        assert_eq!(title_case("night-owl"), "Night owl");
    }
}
