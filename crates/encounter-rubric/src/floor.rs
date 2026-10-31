//! A rendered floor's placement sidecar, read and written in one place.
//!
//! `wowdps-replay-extract gen-floors` renders a boss's room top-down into the
//! per-machine floors cache (`~/.local/share/wowdps/floors/`) as
//! `<slug>.png`, and beside it `<slug>.txt`: where the picture lies in the
//! world and what else was written with it. The replay and the stencil
//! tracer read it back. It is a line format of its own, not TOML, written
//! as `key = value` lines in this order:
//!
//! ```text
//! encounter = "Ula'tek"                    the journal's name, `{:?}`
//! dungeon_encounter = 3492                 the log's ENCOUNTER_START id
//! map = 2521                               a dungeon level's map id
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
//! params = 9d3e0c4a51f2b768 ppy=8 margin=20 ceiling=25 exposure=0.82 gpu=false debug=false
//! made = 1791830400                        unix seconds
//! ```
//!
//! `params` says what the render was made from: an FNV-1a hash, walked
//! over the values themselves ([`Map::params_hash`],
//! [`Encounter::floor_params`], [`Map::level_params`]), of what of the
//! rubric's resolved `[map]` changes the render's pixels (for a room off
//! the arena, the room's cut too and no crop; for a dungeon level's
//! render, what of its level's map it draws by and the posts it was cut
//! over) and of the gen-floors options that change pixels, which follow it
//! as words, so a reader holding the rubric now can hash it the same way
//! and tell a stale render ([`Floors::stale`]).
//! `made` is when it was written. A sidecar written before either has
//! neither; a dungeon level's minimap picture has `made` alone.
//!
//! A dungeon's level (`gen-floors --map`: its minimap tiles, one picture
//! per UiMap, `<instance-slug>-L<n>-minimap.png`) names the instance as its
//! `encounter`, its `map` and its level's `ui_map`, and no encounter id.
//! The same level rendered from the world's geometry
//! (`<instance-slug>-L<n>.png`, over the same world rectangle) says the
//! same, its own placement, its walls' turn where it has walls, and a
//! `minimap` line naming the level's minimap picture as its backdrop:
//!
//! ```text
//! encounter = "Ruby Life Pools"
//! map = 2521
//! ui_map = 2095
//! room_group = 0
//! x_max = 1715                             the minimap level's x_max and
//! y_max = 105                              y_max: the two register
//! ppy = 8
//! width = 4680
//! height = 3280
//! wall_angle = 50.5
//! wall_share = 0.12
//! minimap = "ruby-life-pools-L2-minimap.png" x_max=1715 y_max=105 ppy=0.96000004 width=562 height=394
//! ```
//!
//! That `minimap` line is what tells a level's render from its minimap
//! picture, whose sidecar has none: [`Floors::level`] and
//! [`Floors::levels`] find the minimap pictures, [`Floors::level_render`]
//! and [`Floors::level_renders`] the renders.
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
//! order, and finds an encounter's arena render or a room off it, and a
//! dungeon map's levels, from its minimap tiles or rendered.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::{Encounter, Light, Map};

/// One floor's sidecar: every line `write_floor` writes, by its key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sidecar {
    /// The journal's name for the encounter.
    pub encounter: String,
    /// Its DungeonEncounterID, what the log's ENCOUNTER_START names. Every
    /// sidecar gen-floors writes says it; one rendered before it did (the
    /// replay spike's oldest inputs) does not.
    pub dungeon_encounter: Option<u32>,
    /// The instance's map id, on a picture of one of a dungeon's levels
    /// (`gen-floors --map`), from its minimap tiles or rendered: its
    /// `encounter` is then the instance's name and its `ui_map` the level;
    /// it names no encounter and no room.
    pub map: Option<u32>,
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
    /// The game's minimap under the render and beyond it; on a dungeon
    /// level's render, the level's minimap picture (the same rectangle),
    /// which a level's minimap picture itself never has.
    pub minimap: Option<Minimap>,
    /// How many of the rubric's stencil shapes the render was cut to; 0
    /// without a stencil (no line).
    pub stencil: usize,
    /// Each glowing liquid's shape at its brightest.
    pub glow: Vec<Glow>,
    /// Each piece of the room a fight can take away.
    pub layer: Vec<Layer>,
    /// What the render was made from (`params = ...`): none on one written
    /// before sidecars said, or a dungeon level's minimap picture.
    pub params: Option<Params>,
    /// When it was written, unix seconds (`made = ...`).
    pub made: Option<u64>,
    /// Lines this build does not know: key and value, in their order.
    pub unknown: Vec<(String, String)>,
}

/// The gen-floors options that change a render's pixels, as a sidecar's
/// `params` line gives them after its hash: pixels a yard, the margin
/// about the room, the ceiling over the arena (yards), the ambient's
/// exposure, the GPU path, and `--debug`'s fills.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    pub ppy: f32,
    pub margin: f32,
    pub ceiling: f32,
    pub exposure: f32,
    pub gpu: bool,
    pub debug: bool,
}

impl Default for RenderOptions {
    /// What gen-floors renders with unless told (its `Options`' defaults).
    fn default() -> RenderOptions {
        RenderOptions {
            ppy: 8.0,
            margin: 20.0,
            ceiling: 25.0,
            exposure: 0.82,
            gpu: false,
            debug: false,
        }
    }
}

impl RenderOptions {
    /// The options as a `params` line's words: ` ppy=8 margin=20 …`.
    fn words(&self) -> String {
        format!(
            " ppy={} margin={} ceiling={} exposure={} gpu={} debug={}",
            self.ppy, self.margin, self.ceiling, self.exposure, self.gpu, self.debug
        )
    }
}

/// The `params = ...` line: the hash of what a render was made from, and
/// the options it was made with (the module's doc).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Params {
    pub hash: u64,
    pub options: RenderOptions,
    /// Fields this build does not know, in their order.
    pub unknown: Vec<(String, String)>,
}

/// FNV-1a's offset basis: an empty hash, which `fnv1a` folds bytes into.
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Fold `bytes` into an FNV-1a hash.
pub fn fnv1a(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The params hash as it is walked: each value by its own bytes (numbers
/// little-endian, floats by their bits, a list by its length then its
/// items, an absent value apart from any present one), each key's name
/// before it, so no serializer's text (and no bump of one) moves a hash.
struct Walk(u64);

impl Walk {
    fn new() -> Walk {
        Walk(FNV_OFFSET)
    }

    fn bytes(&mut self, b: &[u8]) {
        self.0 = fnv1a(self.0, b);
    }

    /// A key's name, ended so no two names run together.
    fn key(&mut self, name: &str) {
        self.bytes(name.as_bytes());
        self.bytes(&[0]);
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }

    fn bool(&mut self, v: bool) {
        self.bytes(&[u8::from(v)]);
    }

    fn len(&mut self, n: usize) {
        self.bytes(&(n as u64).to_le_bytes());
    }

    fn str(&mut self, s: &str) {
        self.len(s.len());
        self.bytes(s.as_bytes());
    }

    fn opt<T: Copy>(&mut self, v: Option<T>, mut put: impl FnMut(&mut Walk, T)) {
        match v {
            None => self.bytes(&[0]),
            Some(x) => {
                self.bytes(&[1]);
                put(self, x);
            }
        }
    }

    fn u32s(&mut self, v: &[u32]) {
        self.len(v.len());
        for &x in v {
            self.u32(x);
        }
    }

    fn point(&mut self, [x, y]: [f32; 2]) {
        self.f32(x);
        self.f32(y);
    }

    fn shapes(&mut self, shapes: &[crate::StencilShape]) {
        self.len(shapes.len());
        for s in shapes {
            self.len(s.points.len());
            for &p in &s.points {
                self.point(p);
            }
            self.opt(s.center, Walk::point);
            self.opt(s.radius, Walk::f32);
        }
    }

    fn options(&mut self, o: &RenderOptions) {
        self.key("options");
        self.f32(o.ppy);
        self.f32(o.margin);
        self.f32(o.ceiling);
        self.f32(o.exposure);
        self.bool(o.gpu);
        self.bool(o.debug);
    }

    /// The keys a render draws by that a map most often leaves unsaid,
    /// each walked only where it is said, so a map saying none hashes as
    /// it did before gen-floors drew by them: `crop`, `include_groups`,
    /// `include_models`, each enabled `fade` by its key, piece and alpha
    /// (a disabled one draws nothing), `exposure` and `water`.
    fn said(&mut self, map: &Map) {
        if let Some(c) = map.crop {
            self.key("crop");
            for v in c {
                self.f32(v);
            }
        }
        for (name, ids) in [
            ("include_groups", &map.include_groups),
            ("include_models", &map.include_models),
        ] {
            if !ids.is_empty() {
                self.key(name);
                self.u32s(ids);
            }
        }
        let fades: Vec<(&String, &crate::Fade)> =
            map.fade.iter().filter(|(_, f)| f.enabled).collect();
        if !fades.is_empty() {
            self.key("fade");
            self.len(fades.len());
            for (key, f) in fades {
                let crate::Fade {
                    enabled: _,
                    name: _,
                    group,
                    model,
                    alpha,
                } = f;
                self.str(key);
                self.opt(*group, Walk::u32);
                self.opt(*model, Walk::u32);
                self.f32(*alpha);
            }
        }
        if let Some(x) = map.exposure {
            self.key("exposure");
            self.f32(x);
        }
        if let Some(w) = &map.water {
            let crate::Water {
                close,
                far,
                shallow,
                deep,
                depth,
                bed,
                types,
                shore,
            } = w;
            self.key("water");
            self.bytes(close);
            self.bytes(far);
            self.f32(*shallow);
            self.f32(*deep);
            self.f32(*depth);
            self.opt(*bed, |w, c| w.bytes(&c));
            // Walked where it is said, so a water naming none hashes as
            // one did before it could; as a set, sorted and once each, so
            // their order and a repeat move no hash.
            if let Some(types) = types {
                let mut set = types.clone();
                set.sort_unstable();
                set.dedup();
                self.key("types");
                self.u32s(&set);
            }
            if let Some(y) = shore {
                self.key("shore");
                self.f32(*y);
            }
        }
    }

    /// What cuts a render area by area, each walked only where it is said,
    /// so a map saying none hashes as one did before a render read them:
    /// the map's `hang`; each enabled area saying a `ceiling` or a `hang`
    /// (its key, the two, and its `shape`, which places them) or, on a
    /// level (`level`), a `stencil` (its shapes); and, on a level some
    /// enabled area gives a stencil, each enabled path's corridor by its
    /// key, its way (`Map::path_way`, so an unsaid one follows its areas'
    /// middles) and its width (`Path::corridor_width`). An arena's area
    /// stencil is not drawn, nor are its corridors: neither is walked.
    fn cut(&mut self, map: &Map, level: bool) {
        if let Some(h) = map.hang {
            self.key("hang");
            self.f32(h);
        }
        let areas: Vec<(&String, &crate::Area)> = map
            .area
            .iter()
            .filter(|(_, a)| {
                a.enabled
                    && (a.ceiling.is_some() || a.hang.is_some() || (level && !a.stencil.is_empty()))
            })
            .collect();
        if !areas.is_empty() {
            self.key("area");
            self.len(areas.len());
            for (key, a) in areas {
                let crate::Area {
                    enabled: _,
                    name: _,
                    shape,
                    far: _,
                    home: _,
                    near: _,
                    turn: _,
                    ceiling,
                    hang,
                    stencil,
                    encounter: _,
                } = a;
                self.str(key);
                self.opt(*ceiling, Walk::f32);
                self.opt(*hang, Walk::f32);
                if ceiling.is_some() || hang.is_some() {
                    self.shapes(shape);
                }
                if level {
                    self.shapes(stencil);
                }
            }
        }
        if level && map.level_stenciled() {
            let corridors: Vec<(&String, Vec<[f32; 2]>, f32)> = map
                .path
                .iter()
                .filter(|(_, p)| p.enabled)
                .filter_map(|(key, p)| Some((key, map.path_way(p)?, p.corridor_width())))
                .collect();
            self.key("corridor");
            self.len(corridors.len());
            for (key, way, width) in corridors {
                self.str(key);
                self.len(way.len());
                for p in way {
                    self.point(p);
                }
                self.f32(width);
            }
        }
    }
}

impl Map {
    /// The hash a render's sidecar records (`params`): what of this `[map]`
    /// changes a render's pixels, and the options that do. gen-floors draws
    /// by `level`, `ceiling`, whether `light` is `graded` (unsaid and
    /// `baked` draw alike), `exclude_groups`, `exclude_models`,
    /// `exclude_textures`, `stencil`, `ppy`, and each `glow` by its key,
    /// `color`, `hot`, `area`, `except` and `below`; and, walked only where
    /// said (`Walk::said`), `crop`, `include_groups`, `include_models`,
    /// each enabled `fade`, `water` and `exposure` (the options carry the exposure
    /// the render took, `--exposure` given outright or its default times
    /// this); and, walked only where said too (`Walk::cut`), `hang` and
    /// each enabled area's `ceiling` and `hang` with the shape they cover.
    /// It reads none of `ui_map`, `floor`, `layer`, a glow's `name`,
    /// `on`, `rise`, `hold` and `fade`, nor `ground` (an arena is cut at
    /// one level), an area's camera keys and `stencil` (an arena's is not
    /// drawn), `path` and `transition`, nor the encounter's
    /// `[view]`: changing those moves no pixel and no hash. Every key is
    /// named here, so a key added to the map is a choice: the day
    /// gen-floors draws by one, it joins the walk. A room off the arena
    /// adds its cut and leaves the crop out (`Encounter::floor_params`).
    pub fn params_hash(&self, opts: &RenderOptions) -> u64 {
        let Map {
            ui_map: _,
            floor: _,
            level,
            ceiling,
            hang: _,
            light,
            exclude_groups,
            exclude_models,
            exclude_textures,
            layer: _,
            stencil,
            ppy,
            glow,
            crop: _,
            include_groups: _,
            include_models: _,
            fade: _,
            ground: _,
            exposure: _,
            area: _,
            path: _,
            transition: _,
            water: _,
        } = self;
        // `crop`, `include_*`, `fade`, `exposure` and `water` go in `said`,
        // below; `hang` and the areas' ceilings and hangs in `cut`.
        let mut w = Walk::new();
        w.key("level");
        w.opt(*level, Walk::f32);
        w.key("ceiling");
        w.opt(*ceiling, Walk::f32);
        w.key("graded");
        w.bool(*light == Some(Light::Graded));
        w.key("exclude_groups");
        w.u32s(exclude_groups);
        w.key("exclude_models");
        w.u32s(exclude_models);
        w.key("exclude_textures");
        w.u32s(exclude_textures);
        w.key("stencil");
        w.shapes(stencil);
        w.key("ppy");
        w.opt(*ppy, Walk::f32);
        w.key("glow");
        w.len(glow.len());
        for (key, g) in glow {
            let crate::Glow {
                enabled: _,
                name: _,
                color,
                hot,
                area,
                except,
                below,
                on: _,
                rise: _,
                hold: _,
                fade: _,
            } = g;
            w.str(key);
            w.bytes(color);
            w.opt(*hot, |w, c| w.bytes(&c));
            w.shapes(area);
            w.shapes(except);
            w.opt(*below, Walk::f32);
        }
        w.options(opts);
        w.said(self);
        w.cut(self, false);
        w.0
    }

    /// The params hash of a dungeon level's render (`gen-floors --map`)
    /// drawn by this map, the level's `[level.<ui_map>.map]`: the options,
    /// the posts it was cut over (each track's points in order), and of
    /// the map only what a level's render draws by, each walked where it
    /// is said, so a level whose map says none of them hashes as
    /// `level_params` does: `exclude_groups`, `exclude_models`,
    /// `exclude_textures`, a `ground` that is a height (`walked` reaches
    /// the hash as its posts, `terrain` as none), a `ceiling` other than
    /// the options' (how far over its ground a terrain level is cut; the
    /// options carry the tool's), `Walk::said`'s keys, and `Walk::cut`'s:
    /// `hang`, each enabled area's `ceiling` and `hang` with the shape
    /// they cover and its `stencil`, and, where an area gives a stencil,
    /// each enabled path's corridor. A level's render reads none of
    /// `level`, `light`, `stencil`, `ppy`, `glow` and `layer` (it is cut to
    /// its own heights at the tool's density, lit as the client lights
    /// it), nor an area's camera keys, a path's way where no area gives a
    /// stencil, nor `transition`.
    pub fn level_params(&self, posts: &[Vec<[f32; 2]>], opts: &RenderOptions) -> u64 {
        let Map {
            ui_map: _,
            floor: _,
            level: _,
            ceiling,
            hang: _,
            light: _,
            exclude_groups,
            exclude_models,
            exclude_textures,
            layer: _,
            stencil: _,
            ppy: _,
            glow: _,
            crop: _,
            include_groups: _,
            include_models: _,
            fade: _,
            ground,
            exposure: _,
            area: _,
            path: _,
            transition: _,
            water: _,
        } = self;
        let mut w = Walk::new();
        w.key("level");
        w.options(opts);
        w.key("posts");
        w.len(posts.len());
        for track in posts {
            w.len(track.len());
            for &p in track {
                w.point(p);
            }
        }
        for (name, ids) in [
            ("exclude_groups", exclude_groups),
            ("exclude_models", exclude_models),
            ("exclude_textures", exclude_textures),
        ] {
            if !ids.is_empty() {
                w.key(name);
                w.u32s(ids);
            }
        }
        if let Some(crate::Datum::Height(h)) = ground {
            w.key("ground");
            w.f32(*h);
        }
        // How far over its ground a terrain level is cut: the tool's own
        // (`--ceiling`, in the options) where the map says that or nothing,
        // so a level whose defaults say the tool's ceiling hashes as it
        // did before a level read one.
        if let Some(c) = ceiling.filter(|c| *c != opts.ceiling) {
            w.key("ceiling");
            w.f32(c);
        }
        w.said(self);
        w.cut(self, true);
        w.0
    }
}

/// The params hash of a dungeon level's render (`gen-floors --map`) whose
/// map says nothing it draws by: the options, and the posts it was cut
/// over, each track's points in order (`Map::level_params`).
pub fn level_params(posts: &[Vec<[f32; 2]>], opts: &RenderOptions) -> u64 {
    Map::default().level_params(posts, opts)
}

/// How gen-floors draws a room off the arena (the rubric's `[room]` with a
/// radius): its own map floor where it has one, the level and ceiling it
/// is drawn at, whether it is lit as the client lights it (its `light`,
/// else the map's, is not `graded`), its middle (its own, else its
/// `through` place's) and the radius round it, in the render's yards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoomRender {
    pub ui_map: Option<u32>,
    pub level: Option<f32>,
    pub ceiling: Option<f32>,
    pub baked: bool,
    pub center: [f32; 2],
    pub radius: f32,
}

impl Encounter {
    /// Room `key` off the arena as gen-floors draws it: `None` for no such
    /// room, or one with no middle or no radius (not rendered).
    pub fn room_render(&self, key: &str) -> Option<RoomRender> {
        let r = self.room.get(key)?;
        let center = r
            .center
            .or_else(|| self.place.get(r.through.as_deref()?).map(|p| p.at))?;
        Some(RoomRender {
            ui_map: r.ui_map,
            level: r.level,
            ceiling: r.ceiling,
            baked: r.light.or(self.map.light) != Some(Light::Graded),
            center,
            radius: r.radius? / r.scale.unwrap_or(1.0),
        })
    }

    /// Every room off the arena gen-floors renders, by key, in key order.
    pub fn room_renders(&self) -> Vec<(String, RoomRender)> {
        self.room
            .keys()
            .filter_map(|k| Some((k.clone(), self.room_render(k)?)))
            .collect()
    }

    /// The params hash of a render of this encounter (resolved as gen-floors
    /// reads it: the base, every curated set): its arena's (`room: None`,
    /// `Map::params_hash`), or room `key`'s, its cut folded in (its own map
    /// floor, level, ceiling, light, middle and radius; its key names its
    /// file, not its pixels). A room is framed round its own middle, so the
    /// arena's `crop` is no part of its hash: an edit to the crop leaves
    /// every room as fresh as it was. `None` for a room it does not render.
    pub fn floor_params(&self, room: Option<&str>, opts: &RenderOptions) -> Option<u64> {
        let Some(key) = room else {
            return Some(self.map.params_hash(opts));
        };
        let h = Map {
            crop: None,
            ..self.map.clone()
        }
        .params_hash(opts);
        let RoomRender {
            ui_map,
            level,
            ceiling,
            baked,
            center,
            radius,
        } = self.room_render(key)?;
        let mut w = Walk(h);
        w.key("room");
        w.opt(ui_map, Walk::u32);
        w.opt(level, Walk::f32);
        w.opt(ceiling, Walk::f32);
        w.bool(baked);
        w.point(center);
        w.f32(radius);
        Some(w.0)
    }
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

/// The floors cache's file for a place's swirl texture (`Swirl::texture`,
/// a FileDataID): `swirl-<fdid>.png`, beside the floors, one file a
/// texture however many places name it. `gen-floors` writes it, the
/// game's texture at its top mip as RGBA, as it decodes; the replay reads
/// it and draws it through `swirl::disc` (the map editor will draw the
/// swirl so too, opening the install itself).
pub fn swirl_texture_file(fdid: u32) -> String {
    format!("swirl-{fdid}.png")
}

impl Sidecar {
    /// Read a sidecar's text. A line that is not `key = value`, a known
    /// key said twice, a value of the wrong kind or a record missing a
    /// field is an error naming its line; blank lines are skipped.
    pub fn parse(text: &str) -> Result<Sidecar, String> {
        let mut encounter = None;
        let mut dungeon_encounter = None;
        let mut map = None;
        let (mut ui_map, mut room_group) = (None, None);
        let (mut x_max, mut y_max, mut ppy) = (None, None, None);
        let (mut width, mut height) = (None, None);
        let mut room = None;
        let (mut wall_angle, mut wall_share) = (None, None);
        let mut base = None;
        let mut minimap = None;
        let mut stencil = None;
        let (mut params, mut made) = (None, None);
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
                "map" => once(&mut map, key, number(key, value)),
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
                "params" => once(&mut params, key, Params::parse(value)),
                "made" => once(&mut made, key, number(key, value)),
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
            map,
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
            params,
            made,
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
        if let Some(map) = self.map {
            s.push_str(&format!("map = {map}\n"));
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
        if let Some(p) = &self.params {
            s.push_str(&format!(
                "params = {:016x}{}{}\n",
                p.hash,
                p.options.words(),
                words(&p.unknown)
            ));
        }
        if let Some(made) = self.made {
            s.push_str(&format!("made = {made}\n"));
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

impl Params {
    /// `<16 hex digits> ppy=… margin=… ceiling=… exposure=… gpu=… debug=…`.
    fn parse(value: &str) -> Result<Params, String> {
        let mut words = value.split_whitespace();
        let hex = words.next().ok_or("no hash")?;
        let hash = u64::from_str_radix(hex, 16)
            .ok()
            .filter(|_| hex.len() == 16)
            .ok_or_else(|| format!("not a hash of 16 hex digits: {hex:?}"))?;
        let mut f: Vec<(String, String)> = words
            .map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .ok_or_else(|| format!("not `k=v`: {kv:?}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Params {
            hash,
            options: RenderOptions {
                ppy: take(&mut f, "ppy")?,
                margin: take(&mut f, "margin")?,
                ceiling: take(&mut f, "ceiling")?,
                exposure: take(&mut f, "exposure")?,
                gpu: take(&mut f, "gpu")?,
                debug: take(&mut f, "debug")?,
            },
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

    /// Map `map`'s levels from its minimap tiles (`gen-floors --map`), in
    /// file-name order: one a UiMap, its sidecar with no `minimap` line.
    pub fn levels(&self, map: u32) -> Vec<&Rendered> {
        self.of_map(map)
            .filter(|f| f.sidecar.minimap.is_none())
            .collect()
    }

    /// Map `map`'s level on UiMap `ui_map`, from its minimap tiles.
    pub fn level(&self, map: u32, ui_map: u32) -> Option<&Rendered> {
        self.levels(map)
            .into_iter()
            .find(|f| f.sidecar.ui_map == ui_map)
    }

    /// Map `map`'s levels rendered from the world's geometry, in file-name
    /// order: each sidecar's `minimap` line names its level's minimap
    /// picture, its backdrop.
    pub fn level_renders(&self, map: u32) -> Vec<&Rendered> {
        self.of_map(map)
            .filter(|f| f.sidecar.minimap.is_some())
            .collect()
    }

    /// Map `map`'s level on UiMap `ui_map`, rendered from the world's
    /// geometry.
    pub fn level_render(&self, map: u32, ui_map: u32) -> Option<&Rendered> {
        self.level_renders(map)
            .into_iter()
            .find(|f| f.sidecar.ui_map == ui_map)
    }

    /// Whether the floor `slug` (its sidecar `<slug>.txt`) was made from
    /// other than what hashes to `current_params_hash` now
    /// (`Encounter::floor_params`, under the options its `params` line
    /// gives): `None` where there is no such floor, or its sidecar says
    /// nothing of what it was made from (one written before sidecars did).
    pub fn stale(&self, slug: &str, current_params_hash: u64) -> Option<bool> {
        let floor = self
            .floors
            .iter()
            .find(|f| f.path.file_stem().is_some_and(|s| s == slug))?;
        let params = floor.sidecar.params.as_ref()?;
        Some(params.hash != current_params_hash)
    }

    /// Every sidecar of map `map`'s levels, minimap and render alike: a
    /// map's, naming no encounter.
    fn of_map(&self, map: u32) -> impl Iterator<Item = &Rendered> {
        self.floors
            .iter()
            .filter(move |f| f.sidecar.map == Some(map) && f.sidecar.dungeon_encounter.is_none())
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

    /// A swirl texture's file is named by its FileDataID alone.
    #[test]
    fn a_swirl_texture_is_named_by_its_file_data_id() {
        assert_eq!(swirl_texture_file(5207558), "swirl-5207558.png");
    }

    /// A dungeon's level: the instance's name, its map and level, no
    /// encounter id; it reads back as written and is found by its map
    /// and level, never as an arena.
    #[test]
    fn a_dungeon_level_round_trips_and_is_found_by_its_map() {
        let text = "encounter = \"Ruby Life Pools\"\nmap = 2521\nui_map = 2094\nroom_group = 0\nx_max = 1898.75\ny_max = 211.25\nppy = 0.96000004\nwidth = 811\nheight = 531\n";
        let s = Sidecar::parse(text).unwrap();
        assert_eq!(
            (s.map, s.ui_map, s.dungeon_encounter),
            (Some(2521), 2094, None)
        );
        assert_eq!(s.to_text(), text);
        let level = |ui_map: u32, file: &str| Rendered {
            path: PathBuf::from(file),
            sidecar: Sidecar {
                ui_map,
                ..s.clone()
            },
        };
        let floors = Floors {
            floors: vec![
                level(2094, "ruby-life-pools-L1-minimap.txt"),
                level(2095, "ruby-life-pools-L2-minimap.txt"),
            ],
            skipped: Vec::new(),
        };
        assert_eq!(floors.levels(2521).len(), 2);
        assert!(floors.levels(2522).is_empty());
        assert_eq!(
            floors.level(2521, 2095).map(Rendered::png),
            Some(PathBuf::from("ruby-life-pools-L2-minimap.png"))
        );
        assert!(floors.arena(2094).is_none());
    }

    /// A level rendered from the world's geometry: the minimap level's
    /// lines, its own placement and walls, and a `minimap` line naming the
    /// minimap picture. `level` and `levels` keep finding the minimap
    /// pictures, `level_render` and `level_renders` the renders, whatever
    /// the files sort as; neither is ever an arena or a room.
    #[test]
    fn a_level_render_is_told_from_its_minimap_level_by_its_minimap_line() {
        let render = "encounter = \"Ruby Life Pools\"\nmap = 2521\nui_map = 2095\nroom_group = 0\nx_max = 1715\ny_max = 105\nppy = 8\nwidth = 4680\nheight = 3280\nwall_angle = 50.5\nwall_share = 0.12\nminimap = \"ruby-life-pools-L2-minimap.png\" x_max=1715 y_max=105 ppy=0.96000004 width=562 height=394\n";
        let s = Sidecar::parse(render).unwrap();
        assert_eq!(s.to_text(), render);
        assert_eq!(
            (s.map, s.ui_map, s.dungeon_encounter),
            (Some(2521), 2095, None)
        );
        let m = s.minimap.as_ref().unwrap();
        assert_eq!(m.file, "ruby-life-pools-L2-minimap.png");
        // The two register: the same world point at pixel (0, 0).
        assert_eq!((m.x_max, m.y_max), (s.x_max, s.y_max));
        assert_eq!(s.wall(), Some((50.5, 0.12)));
        let minimap_level = |ui_map: u32| Sidecar {
            encounter: "Ruby Life Pools".into(),
            map: Some(2521),
            ui_map,
            x_max: 1715.0,
            y_max: 105.0,
            ppy: 0.96,
            width: 562,
            height: 394,
            ..Sidecar::default()
        };
        let at = |file: &str, sidecar: Sidecar| Rendered {
            path: PathBuf::from(file),
            sidecar,
        };
        let floors = Floors {
            floors: vec![
                // File-name order: a render sorts before its minimap level.
                at(
                    "ruby-life-pools-L1.txt",
                    Sidecar {
                        ui_map: 2094,
                        ..s.clone()
                    },
                ),
                at("ruby-life-pools-L1-minimap.txt", minimap_level(2094)),
                at("ruby-life-pools-L2.txt", s.clone()),
                at("ruby-life-pools-L2-minimap.txt", minimap_level(2095)),
            ],
            skipped: Vec::new(),
        };
        let files = |found: Vec<&Rendered>| -> Vec<PathBuf> {
            found.into_iter().map(Rendered::png).collect()
        };
        assert_eq!(
            files(floors.levels(2521)),
            [
                PathBuf::from("ruby-life-pools-L1-minimap.png"),
                PathBuf::from("ruby-life-pools-L2-minimap.png")
            ]
        );
        assert_eq!(
            files(floors.level_renders(2521)),
            [
                PathBuf::from("ruby-life-pools-L1.png"),
                PathBuf::from("ruby-life-pools-L2.png")
            ]
        );
        assert_eq!(
            floors.level(2521, 2095).map(Rendered::png),
            Some(PathBuf::from("ruby-life-pools-L2-minimap.png"))
        );
        let r = floors.level_render(2521, 2095).unwrap();
        assert_eq!(r.png(), PathBuf::from("ruby-life-pools-L2.png"));
        assert_eq!(
            r.beside(&r.sidecar.minimap.as_ref().unwrap().file),
            PathBuf::from("ruby-life-pools-L2-minimap.png")
        );
        assert!(floors.level_render(2521, 9999).is_none());
        assert!(floors.level_render(2522, 2095).is_none());
        assert!(floors.level_renders(2522).is_empty());
        assert!(floors.arena(2095).is_none() && floors.arena(2521).is_none());
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

    /// A sidecar that says what it was made from and when reads back as
    /// written, the two lines after the layers; one written before they
    /// were reads with neither.
    #[test]
    fn params_and_made_round_trip_and_may_be_absent() {
        let base = FIXTURES[0].1;
        let text = format!(
            "{base}params = 00c0ffee12345678 ppy=8 margin=20 ceiling=25 exposure=0.82 gpu=true debug=false\nmade = 1791830400\n"
        );
        let s = Sidecar::parse(&text).unwrap();
        let p = s.params.as_ref().unwrap();
        assert_eq!(p.hash, 0x00c0_ffee_1234_5678);
        assert_eq!(
            p.options,
            RenderOptions {
                gpu: true,
                ..RenderOptions::default()
            }
        );
        assert_eq!(s.made, Some(1_791_830_400));
        assert_eq!(s.to_text(), text);
        assert!(s.unknown.is_empty());
        let old = Sidecar::parse(base).unwrap();
        assert_eq!((old.params, old.made), (None, None));
        // A field this build does not know is kept, after the known ones.
        let newer = text.replace("debug=false", "debug=false tint=red");
        assert_eq!(Sidecar::parse(&newer).unwrap().to_text(), newer);
        for (bad, want) in [
            ("params = c0ffee ppy=8", "16 hex digits"),
            (
                "params = 00c0ffee12345678 ppy=8 margin=20 ceiling=25 exposure=0.82 gpu=true",
                "no `debug=`",
            ),
            ("made = soon", "`made` is not a number"),
        ] {
            let e = Sidecar::parse(&format!("{base}{bad}\n")).unwrap_err();
            assert!(e.contains(want), "{e} (wanted {want})");
        }
    }

    /// What `the_params_hash_is_fixed_for_a_fixed_map` pins.
    const PINNED_HASH: u64 = 0x0522_f77f_e37e_db5a;

    /// A map's water, every key but `bed` (`[map.water]`).
    const WATER: &str =
        "[water]\nclose = [18, 22, 29]\nfar = [14, 20, 38]\nshallow = 0.7\ndeep = 0.85\ndepth = 18";

    /// A map of one glow and nothing else; and with `more` TOML before it.
    fn map_with(more: &str) -> Map {
        let text = format!(
            "{more}\n[glow.flame]\ncolor = [200, 40, 10]\narea = [{{ center = [1.0, 2.0], radius = 3.0 }}]\n"
        );
        toml::from_str(&text).unwrap_or_else(|e| panic!("{more}: {e}"))
    }

    /// The hash moves with each key gen-floors draws by and the options,
    /// with no other key of the map, and with no view: the keys listed on
    /// `Map::params_hash`.
    #[test]
    fn the_params_hash_follows_what_changes_pixels_alone() {
        let opts = RenderOptions::default();
        let base = map_with("");
        let h = base.params_hash(&opts);
        let drawn = [
            "level = 380.5",
            "ceiling = 12",
            "light = \"graded\"",
            "exclude_groups = [79429]",
            "exclude_models = [12345]",
            "exclude_textures = [4516750]",
            "stencil = [{ center = [0.0, 0.0], radius = 50.0 }]",
            "ppy = 16",
            "crop = [0.0, 0.0, 10.0, 10.0]",
            "include_groups = [79429]",
            "include_models = [12345]",
            "exposure = 1.2",
            "[fade.roof]\ngroup = 1\nalpha = 0.5",
            "[fade.crate]\nmodel = 12345\nalpha = 0.25",
            WATER,
            "hang = 6",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nceiling = 8",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nhang = 4",
        ];
        for more in drawn {
            assert_ne!(map_with(more).params_hash(&opts), h, "{more}");
        }
        // An area's ceiling and hang by where they lie, and by their area's
        // key; not by its camera's keys.
        let hall = |what: &str| {
            map_with(&format!(
                "[area.hall]\nshape = [{{ center = [0.0, 0.0], radius = 9.0 }}]\n{what}"
            ))
            .params_hash(&opts)
        };
        let ceiled = hall("ceiling = 8");
        assert_ne!(hall("ceiling = 9"), ceiled);
        assert_ne!(hall("hang = 8"), ceiled);
        assert_ne!(hall("ceiling = 8\nhang = 4"), ceiled);
        assert_eq!(
            hall("ceiling = 8\nhome = 2\nturn = 90\nname = \"Hall\""),
            ceiled
        );
        assert_ne!(
            map_with("[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 10.0 }]\nceiling = 8")
                .params_hash(&opts),
            ceiled
        );
        assert_ne!(
            map_with("[area.court]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nceiling = 8")
                .params_hash(&opts),
            ceiled
        );
        // The water by each of its keys, its bed among them.
        let water = map_with(WATER).params_hash(&opts);
        for (key, other) in [
            ("close = [18, 22, 29]", "close = [18, 22, 30]"),
            ("far = [14, 20, 38]", "far = [14, 21, 38]"),
            ("shallow = 0.7", "shallow = 0.6"),
            ("deep = 0.85", "deep = 0.9"),
            ("depth = 18", "depth = 19"),
            ("depth = 18", "depth = 18\nbed = [51, 101, 145]"),
            ("depth = 18", "depth = 18\ntypes = [924]"),
            ("depth = 18", "depth = 18\ntypes = [924, 927]"),
            ("depth = 18", "depth = 18\nshore = 0"),
        ] {
            let moved = map_with(&WATER.replace(key, other)).params_hash(&opts);
            assert_ne!(moved, water, "{other}");
        }
        // The types as a set: their order and a repeat move nothing.
        let typed = |list: &str| {
            map_with(&WATER.replace("depth = 18", &format!("depth = 18\ntypes = {list}")))
                .params_hash(&opts)
        };
        assert_eq!(typed("[924, 927]"), typed("[927, 924]"));
        assert_eq!(typed("[924]"), typed("[924, 924]"));
        // Each fade by its piece and its alpha; a disabled one draws
        // nothing.
        let fade = |what: &str| map_with(&format!("[fade.roof]\n{what}")).params_hash(&opts);
        assert_ne!(
            fade("group = 1\nalpha = 0.5"),
            fade("group = 2\nalpha = 0.5")
        );
        assert_ne!(
            fade("group = 1\nalpha = 0.5"),
            fade("group = 1\nalpha = 0.4")
        );
        assert_eq!(fade("group = 1\nalpha = 0.5\nenabled = false"), h);
        let glow = |what: &str| {
            map_with(&format!(
                "[glow.other]\ncolor = [1, 2, 3]\narea = []\n{what}"
            ))
            .params_hash(&opts)
        };
        let other = glow("");
        for what in [
            "hot = [9, 9, 9]",
            "below = 1.5",
            "except = [{ center = [1.0, 2.0], radius = 1.0 }]",
        ] {
            assert_ne!(glow(what), other, "{what}");
        }
        let undrawn = [
            "ui_map = 2610",
            "floor = \"elsewhere\"",
            "light = \"baked\"",
            "ground = \"walked\"",
            "ground = 120.5",
            "transition = { after = 3.0 }",
            "[layer.platform]\nwmo = 7549724",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]",
            "[area.a]\nshape = []\n[area.b]\nshape = []\n[path.way]\nfrom = \"a\"\nto = \"b\"",
            // An area switched off draws nothing; an arena's area stencil
            // is not drawn, nor its corridors.
            "[area.hall]\nenabled = false\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nceiling = 8\nhang = 4",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nstencil = [{ center = [0.0, 0.0], radius = 5.0 }]",
            STENCILED_PATH,
        ];
        for more in undrawn {
            assert_eq!(map_with(more).params_hash(&opts), h, "{more}");
        }
        for what in [
            "name = \"the flame\"",
            "rise = 1.0",
            "hold = 3.0",
            "fade = 1.0",
        ] {
            assert_eq!(glow(what), other, "{what}");
        }
        let ppy = RenderOptions { ppy: 2.0, ..opts };
        assert_ne!(base.params_hash(&ppy), h);
        let gpu = RenderOptions { gpu: true, ..opts };
        assert_ne!(base.params_hash(&gpu), h);
        // An encounter's arena hashes its map, its view no part of it; no
        // such room, no hash.
        let mut e = Encounter {
            map: base.clone(),
            ..Encounter::default()
        };
        assert_eq!(e.floor_params(None, &opts), Some(h));
        e.view.turn = Some(45.0);
        assert_eq!(e.floor_params(None, &opts), Some(h));
        assert_eq!(e.floor_params(Some("the-well"), &opts), None);
    }

    /// A level's render hashes its options and its posts, in order.
    #[test]
    fn a_levels_params_follow_its_posts() {
        let opts = RenderOptions::default();
        let posts = vec![vec![[1.0, 2.0], [3.0, 4.0]], vec![[5.0, 6.0]]];
        let h = level_params(&posts, &opts);
        assert_eq!(level_params(&posts, &opts), h);
        assert_ne!(level_params(&posts[..1], &opts), h);
        let moved = vec![vec![[1.0, 2.0], [3.0, 4.5]], vec![[5.0, 6.0]]];
        assert_ne!(level_params(&moved, &opts), h);
        let joined = vec![vec![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]];
        assert_ne!(level_params(&joined, &opts), h);
        assert_ne!(level_params(&posts, &RenderOptions { ppy: 2.0, ..opts }), h);
        let none = level_params(&[], &opts);
        assert_eq!(none, LEVEL_PINNED_HASH, "{none:016x}");
    }

    /// What `a_levels_params_follow_its_posts` pins: a level with no posts
    /// at the default options, as every level hashed before its map was
    /// read.
    const LEVEL_PINNED_HASH: u64 = 0x9dcb_8c03_13d3_3c83;

    /// A level's map moves its hash by what its render draws by alone,
    /// each key only where it is said: a map saying none hashes as
    /// `level_params`, its arena keys and its camera's are no part of it.
    #[test]
    fn a_levels_params_follow_what_its_map_draws_by() {
        let opts = RenderOptions::default();
        let posts = vec![vec![[1.0, 2.0], [3.0, 4.0]]];
        let level =
            |more: &str| -> Map { toml::from_str(more).unwrap_or_else(|e| panic!("{more}: {e}")) };
        let h = level("").level_params(&posts, &opts);
        assert_eq!(h, level_params(&posts, &opts));
        for more in [
            "exclude_groups = [79429]",
            "exclude_models = [12345]",
            "exclude_textures = [4516750]",
            "crop = [0.0, 0.0, 10.0, 10.0]",
            "include_groups = [79429]",
            "include_models = [12345]",
            "ground = 120.5",
            "exposure = 1.2",
            "ceiling = 12",
            "[fade.roof]\ngroup = 1\nalpha = 0.5",
            WATER,
            "hang = 6",
            // An area's own ceiling moves it, even the tool's: it is the
            // area's in the map's place.
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nceiling = 25",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nhang = 4",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nstencil = [{ center = [0.0, 0.0], radius = 5.0 }]",
            STENCILED_PATH,
        ] {
            assert_ne!(level(more).level_params(&posts, &opts), h, "{more}");
        }
        // With a stencil said, each path's corridor by its way and width:
        // an unsaid way by its areas' middles; one switched off, nothing.
        let stenciled = level(STENCILED).level_params(&posts, &opts);
        let pathed =
            |what: &str| level(&format!("{STENCILED_PATH}\n{what}")).level_params(&posts, &opts);
        let way = pathed("");
        assert_ne!(way, stenciled);
        assert_ne!(pathed("width = 4"), way);
        assert_ne!(pathed("width = 8"), pathed("width = 6"));
        assert_ne!(
            pathed("points = [[0.0, 0.0], [25.0, 5.0], [50.0, 0.0]]"),
            way
        );
        assert_eq!(pathed("enabled = false"), stenciled);
        assert_eq!(pathed("mode = \"mount\"\nspeed = 9\nname = \"Way\""), way);
        let moved = STENCILED.replace("center = [50.0, 0.0]", "center = [60.0, 0.0]");
        assert_ne!(
            level(&format!("{moved}\n[path.way]\nfrom = \"a\"\nto = \"b\""))
                .level_params(&posts, &opts),
            way
        );
        // Without a path, an area saying none of them moves nothing.
        assert_eq!(level(&moved).level_params(&posts, &opts), stenciled);
        // A ceiling of the tool's own (the season's default, 25) is what
        // the options carry already.
        for more in [
            "floor = \"elsewhere\"",
            "level = 380.5",
            "ceiling = 25",
            "light = \"graded\"",
            "ppy = 16",
            "ground = \"walked\"",
            "ground = \"terrain\"",
            "stencil = [{ center = [0.0, 0.0], radius = 50.0 }]",
            "transition = { after = 3.0 }",
            "[area.hall]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]",
            // A corridor cuts nothing where no area gives a stencil; an
            // area switched off gives none.
            "[area.a]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\n[area.b]\nshape = [{ center = [50.0, 0.0], radius = 9.0 }]\n[path.way]\nfrom = \"a\"\nto = \"b\"\nwidth = 4",
            "[area.hall]\nenabled = false\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nstencil = [{ center = [0.0, 0.0], radius = 5.0 }]",
        ] {
            assert_eq!(level(more).level_params(&posts, &opts), h, "{more}");
        }
    }

    /// Two areas, the first with a stencil (`STENCILED`), and a path
    /// between them, its way unsaid (`STENCILED_PATH`).
    const STENCILED: &str = "[area.a]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nstencil = [{ center = [0.0, 0.0], radius = 5.0 }]\n[area.b]\nshape = [{ center = [50.0, 0.0], radius = 9.0 }]";
    const STENCILED_PATH: &str = "[area.a]\nshape = [{ center = [0.0, 0.0], radius = 9.0 }]\nstencil = [{ center = [0.0, 0.0], radius = 5.0 }]\n[area.b]\nshape = [{ center = [50.0, 0.0], radius = 9.0 }]\n[path.way]\nfrom = \"a\"\nto = \"b\"";

    /// The hash is the walk's own: a fixed map hashes to a fixed number, so
    /// no serializer's bump can move every floor's.
    #[test]
    fn the_params_hash_is_fixed_for_a_fixed_map() {
        let h = map_with("level = 380.5\nexclude_groups = [79429, 80001]")
            .params_hash(&RenderOptions::default());
        assert_eq!(h, PINNED_HASH, "{h:016x}");
    }

    /// Every encounter the embedded rubric holds hashes the same both ways
    /// it is read; Nek'zali's well is a room gen-floors renders, its hash
    /// its arena's with its cut folded in.
    #[test]
    fn the_embedded_rubric_hashes_the_same_both_ways() {
        let r = crate::Rubric::embedded().unwrap();
        let mut n = 0;
        for (id, _, _) in r.encounters() {
            let Some(Ok(e)) = r.encounter(id, None) else {
                continue;
            };
            // gen-floors reads the embedded rubric at its own tier; the
            // replay spike, whose rubric carries the user's files, at the
            // curated one: the same encounter, the same hash.
            let curated = r.encounter_at(id, None, crate::Tier::Curated);
            let opts = RenderOptions::default();
            assert_eq!(
                curated
                    .and_then(Result::ok)
                    .map(|c| c.floor_params(None, &opts)),
                Some(e.floor_params(None, &opts)),
                "{id}"
            );
            n += 1;
        }
        assert!(n > 40, "{n}");
        let nekzali = r.encounter(3470, None).unwrap().unwrap();
        let rooms = nekzali.room_renders();
        assert!(rooms.iter().any(|(k, _)| k == "the-well"), "{rooms:?}");
        let opts = RenderOptions::default();
        let arena = nekzali.floor_params(None, &opts).unwrap();
        let well = nekzali.floor_params(Some("the-well"), &opts).unwrap();
        assert_ne!(arena, well);
        // A room reads no crop: the arena's moves its arena's hash alone.
        let cropped = crate::Encounter {
            map: Map {
                crop: Some([0.0, 0.0, 10.0, 10.0]),
                ..nekzali.map.clone()
            },
            ..nekzali.clone()
        };
        assert_ne!(cropped.floor_params(None, &opts), Some(arena));
        assert_eq!(cropped.floor_params(Some("the-well"), &opts), Some(well));
    }

    /// A floor is stale when its params differ from the hash now; one with
    /// no params, or no such floor, says nothing.
    #[test]
    fn a_floor_is_stale_by_its_params() {
        let with = |hash: Option<u64>| Sidecar {
            params: hash.map(|hash| Params {
                hash,
                ..Params::default()
            }),
            ..Sidecar::parse(FIXTURES[1].1).unwrap()
        };
        let floors = Floors {
            floors: vec![
                Rendered {
                    path: PathBuf::from("/x/ulatek.txt"),
                    sidecar: with(Some(5)),
                },
                Rendered {
                    path: PathBuf::from("/x/old.txt"),
                    sidecar: with(None),
                },
            ],
            skipped: Vec::new(),
        };
        assert_eq!(floors.stale("ulatek", 5), Some(false));
        assert_eq!(floors.stale("ulatek", 6), Some(true));
        assert_eq!(floors.stale("old", 6), None);
        assert_eq!(floors.stale("nowhere", 6), None);
    }
}
