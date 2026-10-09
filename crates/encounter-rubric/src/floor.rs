//! A rendered floor's placement sidecar, read and written in one place.
//!
//! `wowdps-extract gen-floors` renders a boss's room top-down into the
//! per-machine floors cache (`~/.local/share/wowdps/floors/`) as
//! `<slug>.png`, and beside it `<slug>.txt`: where the picture lies in the
//! world and what else was written with it. The replay and the stencil
//! tracer read it back. It is a line format of its own, not TOML, written
//! as `key = value` lines in this order:
//!
//! ```text
//! encounter = "Ula'tek"                    the journal's name, `{:?}`
//! dungeon_encounter = 3492                 the log's ENCOUNTER_START id
//! ui_map = 2610
//! room_group = 79429
//! x_max = 1687                             pixel (0, 0) is world (x_max,
//! y_max = 150.6                            y_max); +X up the image, +Y to
//! ppy = 16                                 its left; ppy pixels a yard
//! width = 4768
//! height = 4456
//! room = "the-well"                        a room off the arena, by its key
//! wall_angle = 0.0                         `{:.1}`: the turn that squares
//! wall_share = 0.22                        the walls; `{:.2}`: how many agree
//! base = "ulatek-base.png"                 the floor without its layers
//! minimap = "x-minimap.png" x_max=… y_max=… ppy=… width=… height=…
//! stencil = 1                              shapes cut to; only when > 0
//! glow = "x-glow-flame.png" key=flame x_max=… y_max=… width=… height=…
//! layer = "x-layer-7549724.png" wmo=… placement=… x_max=… y_max=… width=… height=…
//! ```
//!
//! `glow` and `layer` repeat, one line each; every other key is said at
//! most once. A record's fields after its file are `k=v` words, and a
//! glow's `key` is a bare word (a rubric key).
//!
//! # The writer's contract
//!
//! [`Sidecar::to_text`] reproduces the extractor's `write_floor` byte for
//! byte, so that function can build a [`Sidecar`] from its floor and write
//! `to_text()`. Each field is named exactly as its key (`glow` and `layer`
//! hold every line of theirs). Strings are written with Rust's `{:?}` and
//! unescaped by [`Sidecar::parse`] (quotes, backslashes, `\n` `\r` `\t`
//! `\0`, `\u{…}`), so `Ula'tek`, `Akanôs` and a name with a quote all
//! come back as they went. `base` is written with `{:?}` too, where
//! `write_floor` wrote its quotes by hand: the two agree on every name
//! [`slug`] makes, which holds nothing `{:?}` escapes. Floats are written
//! with `{}`, the shortest decimal that reads back as the same `f32`, so a
//! read and a write give the same bytes; `wall_angle` and `wall_share`
//! are the extractor's `f64`s at one and two places. A line is left out
//! when its field is `None` (`stencil`: 0), as `write_floor` leaves it
//! out. Lines this build does not know, and record fields it does not
//! know, are kept in their order (`unknown`) and written after the ones it
//! does, so a newer writer's sidecar loses nothing through an older
//! reader; a sidecar `write_floor` wrote has none.
//!
//! [`Floors`] reads a floors directory's sidecars once, in file-name
//! order, and finds an encounter's arena render or a room off it.

use std::path::{Path, PathBuf};
use std::str::FromStr;

/// One floor's sidecar: every line `write_floor` writes, by its key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sidecar {
    /// The journal's name for the encounter.
    pub encounter: String,
    /// Its DungeonEncounterID, what the log's ENCOUNTER_START names. Every
    /// sidecar gen-floors writes says it; one rendered before it did (the
    /// replay spike's oldest inputs) does not.
    pub dungeon_encounter: Option<u32>,
    pub ui_map: u32,
    pub room_group: u32,
    /// The world point pixel (0, 0) sits on, and pixels per yard.
    pub x_max: f32,
    pub y_max: f32,
    pub ppy: f32,
    pub width: usize,
    pub height: usize,
    /// A room off the arena, by its rubric key: the arena's own render
    /// has none.
    pub room: Option<String>,
    /// The image's clockwise turn in degrees, under 90, that squares the
    /// room's walls, and the share of the wall area that agrees.
    pub wall_angle: Option<f64>,
    pub wall_share: Option<f64>,
    /// The floor without its layers (`<slug>-base.png`), where it has any.
    pub base: Option<String>,
    /// The game's minimap under the render and beyond it.
    pub minimap: Option<Minimap>,
    /// How many of the rubric's stencil shapes the render was cut to; 0
    /// without a stencil (no line).
    pub stencil: usize,
    /// Each glowing liquid's shape at its brightest.
    pub glow: Vec<Glow>,
    /// Each piece of the room a fight can take away.
    pub layer: Vec<Layer>,
    /// Lines this build does not know: key and value, in their order.
    pub unknown: Vec<(String, String)>,
}

/// The `minimap = ...` line: the backdrop's file and its placement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Minimap {
    pub file: String,
    pub x_max: f32,
    pub y_max: f32,
    pub ppy: f32,
    pub width: usize,
    pub height: usize,
    /// Fields this build does not know, in their order.
    pub unknown: Vec<(String, String)>,
}

/// A `glow = ...` line: one glowing liquid's picture, by its rubric key,
/// placed like a layer (at the render's `ppy`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glow {
    pub file: String,
    pub key: String,
    pub x_max: f32,
    pub y_max: f32,
    pub width: usize,
    pub height: usize,
    pub unknown: Vec<(String, String)>,
}

/// A `layer = ...` line: a building placed on the floor, its file named
/// for its WMO (and its placement, where the WMO is placed more than once).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layer {
    pub file: String,
    pub wmo: u32,
    pub placement: u32,
    pub x_max: f32,
    pub y_max: f32,
    pub width: usize,
    pub height: usize,
    pub unknown: Vec<(String, String)>,
}

/// The floors cache's slug for an encounter's name: lowercase ASCII
/// letters and digits, spaces and hyphens as '-', anything else dropped
/// (`Ula'tek` is `ulatek`). What `gen-floors` names its files by.
pub fn slug(name: &str) -> String {
    name.chars()
        .filter_map(|c| match c {
            c if c.is_ascii_alphanumeric() => Some(c.to_ascii_lowercase()),
            ' ' | '-' => Some('-'),
            _ => None,
        })
        .collect()
}

impl Sidecar {
    /// Read a sidecar's text. A line that is not `key = value`, a known
    /// key said twice, a value of the wrong kind or a record missing a
    /// field is an error naming its line; blank lines are skipped.
    pub fn parse(text: &str) -> Result<Sidecar, String> {
        let mut encounter = None;
        let mut dungeon_encounter = None;
        let (mut ui_map, mut room_group) = (None, None);
        let (mut x_max, mut y_max, mut ppy) = (None, None, None);
        let (mut width, mut height) = (None, None);
        let mut room = None;
        let (mut wall_angle, mut wall_share) = (None, None);
        let mut base = None;
        let mut minimap = None;
        let mut stencil = None;
        let (mut glow, mut layer, mut unknown) = (Vec::new(), Vec::new(), Vec::new());
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let at = |e: String| format!("line {}: {e}", n + 1);
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| at(format!("not `key = value`: {line:?}")))?;
            let (key, value) = (key.trim(), value.trim());
            match key {
                "encounter" => once(&mut encounter, key, string(value)),
                "dungeon_encounter" => once(&mut dungeon_encounter, key, number(key, value)),
                "ui_map" => once(&mut ui_map, key, number(key, value)),
                "room_group" => once(&mut room_group, key, number(key, value)),
                "x_max" => once(&mut x_max, key, number(key, value)),
                "y_max" => once(&mut y_max, key, number(key, value)),
                "ppy" => once(&mut ppy, key, number(key, value)),
                "width" => once(&mut width, key, number(key, value)),
                "height" => once(&mut height, key, number(key, value)),
                "room" => once(&mut room, key, string(value)),
                "wall_angle" => once(&mut wall_angle, key, number(key, value)),
                "wall_share" => once(&mut wall_share, key, number(key, value)),
                "base" => once(&mut base, key, string(value)),
                "minimap" => once(&mut minimap, key, Minimap::parse(value)),
                "stencil" => once(&mut stencil, key, number(key, value)),
                "glow" => Glow::parse(value).map(|g| glow.push(g)),
                "layer" => Layer::parse(value).map(|l| layer.push(l)),
                _ => {
                    unknown.push((key.to_string(), value.to_string()));
                    Ok(())
                }
            }
            .map_err(at)?;
        }
        let need = |key: &str| format!("no `{key} = ` line");
        Ok(Sidecar {
            encounter: encounter.ok_or_else(|| need("encounter"))?,
            dungeon_encounter,
            ui_map: ui_map.ok_or_else(|| need("ui_map"))?,
            room_group: room_group.ok_or_else(|| need("room_group"))?,
            x_max: x_max.ok_or_else(|| need("x_max"))?,
            y_max: y_max.ok_or_else(|| need("y_max"))?,
            ppy: ppy.ok_or_else(|| need("ppy"))?,
            width: width.ok_or_else(|| need("width"))?,
            height: height.ok_or_else(|| need("height"))?,
            room,
            wall_angle,
            wall_share,
            base,
            minimap,
            stencil: stencil.unwrap_or(0),
            glow,
            layer,
            unknown,
        })
    }

    /// Read the sidecar at `path`.
    pub fn read(path: &Path) -> Result<Sidecar, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Sidecar::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The sidecar's text, as `write_floor` writes it (the module's
    /// contract).
    pub fn to_text(&self) -> String {
        let mut s = format!("encounter = {:?}\n", self.encounter);
        if let Some(id) = self.dungeon_encounter {
            s.push_str(&format!("dungeon_encounter = {id}\n"));
        }
        s.push_str(&format!(
            "ui_map = {}\nroom_group = {}\nx_max = {}\ny_max = {}\nppy = {}\nwidth = {}\nheight = {}\n",
            self.ui_map, self.room_group, self.x_max, self.y_max, self.ppy, self.width, self.height
        ));
        if let Some(key) = &self.room {
            s.push_str(&format!("room = {key:?}\n"));
        }
        if let Some(turn) = self.wall_angle {
            s.push_str(&format!("wall_angle = {turn:.1}\n"));
        }
        if let Some(share) = self.wall_share {
            s.push_str(&format!("wall_share = {share:.2}\n"));
        }
        if let Some(file) = &self.base {
            s.push_str(&format!("base = {file:?}\n"));
        }
        if let Some(m) = &self.minimap {
            s.push_str(&format!(
                "minimap = {:?} x_max={} y_max={} ppy={} width={} height={}{}\n",
                m.file,
                m.x_max,
                m.y_max,
                m.ppy,
                m.width,
                m.height,
                words(&m.unknown)
            ));
        }
        if self.stencil > 0 {
            s.push_str(&format!("stencil = {}\n", self.stencil));
        }
        for g in &self.glow {
            s.push_str(&format!(
                "glow = {:?} key={} x_max={} y_max={} width={} height={}{}\n",
                g.file,
                g.key,
                g.x_max,
                g.y_max,
                g.width,
                g.height,
                words(&g.unknown)
            ));
        }
        for l in &self.layer {
            s.push_str(&format!(
                "layer = {:?} wmo={} placement={} x_max={} y_max={} width={} height={}{}\n",
                l.file,
                l.wmo,
                l.placement,
                l.x_max,
                l.y_max,
                l.width,
                l.height,
                words(&l.unknown)
            ));
        }
        for (k, v) in &self.unknown {
            s.push_str(&format!("{k} = {v}\n"));
        }
        s
    }

    /// The wall's turn and share together, where both were written.
    pub fn wall(&self) -> Option<(f64, f64)> {
        self.wall_angle.zip(self.wall_share)
    }
}

impl Minimap {
    fn parse(value: &str) -> Result<Minimap, String> {
        let (file, mut f) = record(value)?;
        Ok(Minimap {
            file,
            x_max: take(&mut f, "x_max")?,
            y_max: take(&mut f, "y_max")?,
            ppy: take(&mut f, "ppy")?,
            width: take(&mut f, "width")?,
            height: take(&mut f, "height")?,
            unknown: f,
        })
    }
}

impl Glow {
    fn parse(value: &str) -> Result<Glow, String> {
        let (file, mut f) = record(value)?;
        Ok(Glow {
            file,
            key: take(&mut f, "key")?,
            x_max: take(&mut f, "x_max")?,
            y_max: take(&mut f, "y_max")?,
            width: take(&mut f, "width")?,
            height: take(&mut f, "height")?,
            unknown: f,
        })
    }
}

impl Layer {
    fn parse(value: &str) -> Result<Layer, String> {
        let (file, mut f) = record(value)?;
        Ok(Layer {
            file,
            wmo: take(&mut f, "wmo")?,
            placement: take(&mut f, "placement")?,
            x_max: take(&mut f, "x_max")?,
            y_max: take(&mut f, "y_max")?,
            width: take(&mut f, "width")?,
            height: take(&mut f, "height")?,
            unknown: f,
        })
    }
}

/// Keep a key's value, refusing it a second time.
fn once<T>(slot: &mut Option<T>, key: &str, value: Result<T, String>) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("`{key}` said twice"));
    }
    *slot = Some(value?);
    Ok(())
}

fn number<T: FromStr>(key: &str, v: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("`{key}` is not a number of its kind: {v:?}"))
}

/// A whole value that is one quoted string.
fn string(value: &str) -> Result<String, String> {
    match quoted(value)? {
        (s, "") => Ok(s),
        (_, rest) => Err(format!("text after the closing quote: {rest:?}")),
    }
}

/// A string as Rust's `{:?}` writes it, unescaped, and what follows it.
fn quoted(s: &str) -> Result<(String, &str), String> {
    let mut chars = s.char_indices();
    if !matches!(chars.next(), Some((_, '"'))) {
        return Err(format!("expected a quoted string: {s:?}"));
    }
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, s.get(i + 1..).unwrap_or_default().trim_start())),
            '\\' => {
                let Some((_, e)) = chars.next() else { break };
                out.push(match e {
                    '"' => '"',
                    '\\' => '\\',
                    '\'' => '\'',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '0' => '\0',
                    'u' => {
                        if !matches!(chars.next(), Some((_, '{'))) {
                            return Err(format!("a `\\u` escape without its brace: {s:?}"));
                        }
                        let mut hex = String::new();
                        loop {
                            match chars.next() {
                                Some((_, '}')) => break,
                                Some((_, d)) if d.is_ascii_hexdigit() => hex.push(d),
                                _ => return Err(format!("a bad `\\u{{…}}` escape: {s:?}")),
                            }
                        }
                        u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| format!("`\\u{{{hex}}}` is no character"))?
                    }
                    other => return Err(format!("an unknown escape `\\{other}`")),
                });
            }
            c => out.push(c),
        }
    }
    Err(format!("an unterminated string: {s:?}"))
}

/// A record's file and its `k=v` words, in order.
fn record(value: &str) -> Result<(String, Vec<(String, String)>), String> {
    let (file, rest) = quoted(value)?;
    let fields = rest
        .split_whitespace()
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("not `k=v`: {kv:?}"))
        })
        .collect::<Result<_, _>>()?;
    Ok((file, fields))
}

/// Take a record's field out of its words: there once, and of its kind.
fn take<T: FromStr>(fields: &mut Vec<(String, String)>, key: &str) -> Result<T, String> {
    let at = fields
        .iter()
        .position(|(k, _)| k == key)
        .ok_or_else(|| format!("no `{key}=`"))?;
    let (_, v) = fields.remove(at);
    if fields.iter().any(|(k, _)| k == key) {
        return Err(format!("`{key}=` said twice"));
    }
    number(key, &v)
}

/// Fields written after the known ones: ` k=v` each.
fn words(fields: &[(String, String)]) -> String {
    fields.iter().map(|(k, v)| format!(" {k}={v}")).collect()
}

/// A floors directory's sidecars, each read once.
#[derive(Debug, Clone, Default)]
pub struct Floors {
    /// Every sidecar that read, in file-name order.
    pub floors: Vec<Rendered>,
    /// Those that did not, and why.
    pub skipped: Vec<(PathBuf, String)>,
}

/// One rendered floor: its sidecar and where it lies.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// The sidecar's own path, `<dir>/<slug>.txt`.
    pub path: PathBuf,
    pub sidecar: Sidecar,
}

impl Rendered {
    /// The render, `<slug>.png` beside its sidecar.
    pub fn png(&self) -> PathBuf {
        self.path.with_extension("png")
    }

    /// A file the sidecar names (its base, minimap, glows, layers): they
    /// lie beside it.
    pub fn beside(&self, file: &str) -> PathBuf {
        self.path.with_file_name(file)
    }
}

impl Floors {
    /// Read every `*.txt` in `dir` once, in file-name order; one that will
    /// not read or parse is set aside in `skipped`. An unreadable
    /// directory is an error (a missing cache, to most callers: none).
    pub fn scan(dir: &Path) -> std::io::Result<Floors> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "txt"))
            .collect();
        paths.sort();
        let mut out = Floors::default();
        for path in paths {
            match Sidecar::read(&path) {
                Ok(sidecar) => out.floors.push(Rendered { path, sidecar }),
                Err(e) => out.skipped.push((path, e)),
            }
        }
        Ok(out)
    }

    /// Encounter `id`'s arena render (its sidecar names the encounter and
    /// no room): the first in file-name order.
    pub fn arena(&self, id: u32) -> Option<&Rendered> {
        self.floors
            .iter()
            .find(|f| f.sidecar.dungeon_encounter == Some(id) && f.sidecar.room.is_none())
    }

    /// The render of the room `key` off encounter `id`'s arena.
    pub fn room(&self, id: u32, key: &str) -> Option<&Rendered> {
        self.floors.iter().find(|f| {
            f.sidecar.dungeon_encounter == Some(id) && f.sidecar.room.as_deref() == Some(key)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real sidecars, copied from a floors cache: between them every line
    /// kind there is (a room, a base, a minimap, a stencil, glows, layers
    /// named by WMO alone and by WMO and placement, and one written before
    /// `dungeon_encounter` and the wall were).
    const FIXTURES: [(&str, &str); 5] = [
        (
            "the-golden-serpent",
            include_str!("../fixtures/floors/the-golden-serpent.txt"),
        ),
        ("ulatek", include_str!("../fixtures/floors/ulatek.txt")),
        (
            "vashnik-the-malignant",
            include_str!("../fixtures/floors/vashnik-the-malignant.txt"),
        ),
        (
            "nekzali-the-soulcoiler-the-well",
            include_str!("../fixtures/floors/nekzali-the-soulcoiler-the-well.txt"),
        ),
        (
            "legacy-floor-render",
            include_str!("../fixtures/floors/legacy-floor-render.txt"),
        ),
    ];

    #[test]
    fn real_sidecars_round_trip_byte_for_byte() {
        for (name, text) in FIXTURES {
            let s = Sidecar::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(s.to_text(), text, "{name}");
            assert!(s.unknown.is_empty(), "{name}: {:?}", s.unknown);
        }
    }

    #[test]
    fn every_line_kind_reads_into_its_field() {
        let golden = Sidecar::parse(FIXTURES[0].1).unwrap();
        assert_eq!(golden.encounter, "The Golden Serpent");
        assert_eq!(golden.dungeon_encounter, Some(2139));
        assert_eq!((golden.ui_map, golden.room_group), (1004, 57221));
        assert_eq!(
            (golden.x_max, golden.y_max, golden.ppy),
            (-615.6425, 3042.865, 8.0)
        );
        assert_eq!((golden.width, golden.height), (7057, 7094));
        assert_eq!(golden.wall(), Some((13.8, 0.36)));
        assert_eq!(golden.base.as_deref(), Some("the-golden-serpent-base.png"));
        let m = golden.minimap.as_ref().unwrap();
        assert_eq!(m.file, "the-golden-serpent-minimap.png");
        assert_eq!(
            (m.x_max, m.y_max, m.ppy),
            (-465.64252, 3192.865, 0.96000004)
        );
        assert_eq!((m.width, m.height), (1135, 1140));
        assert_eq!(golden.layer.len(), 6);
        let l = &golden.layer[1];
        assert_eq!(l.file, "the-golden-serpent-layer-1538686-20856461.png");
        assert_eq!((l.wmo, l.placement), (1538686, 20856461));
        assert_eq!(
            (l.x_max, l.y_max, l.width, l.height),
            (-1037.8047, 2558.7305, 534, 468)
        );
        assert_eq!(golden.stencil, 0);
        assert_eq!(golden.room, None);

        let ulatek = Sidecar::parse(FIXTURES[1].1).unwrap();
        assert_eq!(ulatek.encounter, "Ula'tek");
        assert_eq!(ulatek.stencil, 1);

        let vashnik = Sidecar::parse(FIXTURES[2].1).unwrap();
        let g = &vashnik.glow[0];
        assert_eq!(g.file, "vashnik-the-malignant-glow-flame.png");
        assert_eq!(g.key, "flame");
        assert_eq!(
            (g.x_max, g.y_max, g.width, g.height),
            (437.25, -271.2875, 602, 1260)
        );
        assert_eq!(vashnik.glow[1].key, "shadow");

        let well = Sidecar::parse(FIXTURES[3].1).unwrap();
        assert_eq!(well.room.as_deref(), Some("the-well"));
        assert_eq!(well.encounter, "Nek'zali the Soulcoiler");

        let legacy = Sidecar::parse(FIXTURES[4].1).unwrap();
        assert_eq!(legacy.dungeon_encounter, None);
        assert_eq!(legacy.wall(), None);
        assert_eq!(legacy.layer.len(), 5);
    }

    /// What `write_floor` makes of a floor: its names through `slug`, the
    /// encounter and room keys `{:?}`-quoted, base quoted by hand.
    fn written_like_write_floor(name: &str, room: Option<&str>) -> String {
        let slug = slug(name);
        let mut s = format!(
            "encounter = {name:?}\ndungeon_encounter = 7\nui_map = 1\nroom_group = 2\nx_max = -0.5\ny_max = 0.0000001\nppy = 12\nwidth = 3\nheight = 4\n"
        );
        if let Some(key) = room {
            s.push_str(&format!("room = {key:?}\n"));
        }
        s.push_str("wall_angle = 89.9\nwall_share = 1.00\n");
        s.push_str(&format!("base = \"{slug}-base.png\"\n"));
        s.push_str(&format!(
            "layer = {:?} wmo=1 placement=2 x_max=3.25 y_max=-4 width=5 height=6\n",
            format!("{slug}-layer-1.png")
        ));
        s
    }

    #[test]
    fn names_that_need_escaping_round_trip() {
        for (name, want_slug) in [
            ("Ula'tek", "ulatek"),
            ("Nek'zali the Soulcoiler", "nekzali-the-soulcoiler"),
            ("Akanôs \"the\" Bold", "akans-the-bold"),
            ("Back\\slash\tand\nnewline", "backslashandnewline"),
            ("Zul'jan e\u{301}\u{200b} ☠", "zuljan-e-"),
        ] {
            assert_eq!(slug(name), want_slug, "{name:?}");
            let text = written_like_write_floor(name, Some(name));
            let s = Sidecar::parse(&text).unwrap_or_else(|e| panic!("{name:?}: {e}\n{text}"));
            assert_eq!(s.encounter, name);
            assert_eq!(s.room.as_deref(), Some(name));
            assert_eq!(s.base, Some(format!("{want_slug}-base.png")));
            assert_eq!(s.to_text(), text, "{name:?}");
            assert_eq!(Sidecar::parse(&s.to_text()).unwrap(), s);
        }
    }

    #[test]
    fn floats_and_numbers_write_back_as_read() {
        let s = Sidecar {
            encounter: "x".into(),
            x_max: 1591.7744,
            y_max: -0.28515625,
            ppy: 0.96000004,
            wall_angle: Some(12.25),
            wall_share: Some(0.005),
            minimap: Some(Minimap {
                file: "m.png".into(),
                x_max: f32::MIN_POSITIVE,
                y_max: -1e30,
                ppy: 1.0 / 3.0,
                ..Minimap::default()
            }),
            ..Sidecar::default()
        };
        let text = s.to_text();
        let back = Sidecar::parse(&text).unwrap();
        assert_eq!(back.to_text(), text);
        assert_eq!(back.minimap, s.minimap);
        assert_eq!(
            (back.x_max, back.y_max, back.ppy),
            (s.x_max, s.y_max, s.ppy)
        );
    }

    #[test]
    fn unknown_lines_and_fields_are_kept_in_order() {
        let text = "encounter = \"x\"\nfuture = 1 2 3\nui_map = 1\nroom_group = 2\nx_max = 0\ny_max = 0\nppy = 8\nwidth = 1\nheight = 1\nlayer = \"l.png\" wmo=1 tint=red placement=2 x_max=0 y_max=0 width=1 height=1 lit=9\nalso = \"y\"\n";
        let s = Sidecar::parse(text).unwrap();
        assert_eq!(
            s.unknown,
            vec![
                ("future".to_string(), "1 2 3".to_string()),
                ("also".to_string(), "\"y\"".to_string())
            ]
        );
        assert_eq!(
            s.layer[0].unknown,
            vec![
                ("tint".to_string(), "red".to_string()),
                ("lit".to_string(), "9".to_string())
            ]
        );
        let again = Sidecar::parse(&s.to_text()).unwrap();
        assert_eq!(again, s);
        assert!(
            s.to_text()
                .ends_with("height=1 tint=red lit=9\nfuture = 1 2 3\nalso = \"y\"\n")
        );
    }

    #[test]
    fn a_bad_sidecar_says_where() {
        // The Golden Serpent's: 19 lines, no room and no stencil.
        let base = FIXTURES[0].1;
        for (text, want) in [
            (format!("{base}x_max = 1\n"), "`x_max` said twice"),
            (
                format!("{base}stencil = many\n"),
                "`stencil` is not a number",
            ),
            (format!("{base}just words\n"), "not `key = value`"),
            (format!("{base}room = \"open\n"), "unterminated"),
            (format!("{base}room = \"a\" b\n"), "after the closing quote"),
            (format!("{base}room = \"\\q\"\n"), "unknown escape"),
            (
                format!("{base}glow = \"g.png\" key=k x_max=1 width=1 height=1\n"),
                "no `y_max=`",
            ),
            (base.replace("ppy = 8\n", ""), "no `ppy = ` line"),
        ] {
            let e = Sidecar::parse(&text).unwrap_err();
            assert!(e.contains(want), "{e} (wanted {want})");
        }
        let e = Sidecar::parse(&format!("{base}x_max = 1\n")).unwrap_err();
        assert!(e.starts_with("line 20: "), "{e}");
    }

    #[test]
    fn a_directory_is_read_once_and_found_by_encounter() {
        let dir = std::env::temp_dir().join(format!(
            "wowdps-encounter-rubric-floors-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in FIXTURES {
            std::fs::write(dir.join(format!("{name}.txt")), text).unwrap();
        }
        // Nek'zali's arena, beside its room, and a sidecar that will not read.
        let arena = FIXTURES[3].1.replace("room = \"the-well\"\n", "");
        std::fs::write(dir.join("nekzali-the-soulcoiler.txt"), arena).unwrap();
        std::fs::write(dir.join("broken.txt"), "encounter = \"x\"\n").unwrap();
        std::fs::write(dir.join("not-a-sidecar.png"), "").unwrap();
        let floors = Floors::scan(&dir).unwrap();
        let names: Vec<String> = floors
            .floors
            .iter()
            .filter_map(|f| f.path.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(
            names,
            // By the file name's bytes: '-' sorts before '.'.
            [
                "legacy-floor-render",
                "nekzali-the-soulcoiler-the-well",
                "nekzali-the-soulcoiler",
                "the-golden-serpent",
                "ulatek",
                "vashnik-the-malignant"
            ]
        );
        assert_eq!(floors.skipped.len(), 1);
        assert!(floors.skipped[0].1.contains("no `ui_map = ` line"));

        let a = floors.arena(3470).unwrap();
        assert_eq!(a.png(), dir.join("nekzali-the-soulcoiler.png"));
        let r = floors.room(3470, "the-well").unwrap();
        assert_eq!(r.png(), dir.join("nekzali-the-soulcoiler-the-well.png"));
        assert!(floors.room(3470, "elsewhere").is_none());
        let u = floors.arena(3492).unwrap();
        assert_eq!(
            u.beside(u.sidecar.base.as_deref().unwrap()),
            dir.join("ulatek-base.png")
        );
        assert!(floors.arena(1).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(Floors::scan(&dir).is_err());
    }
}
