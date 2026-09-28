//! One place every color, size and spacing constant comes from.
//!
//! The window draws with the redesign's tokens (`docs/design/
//! window-redesign.html`, its Tokens section): gold is the interface, class
//! colours are people, green and red are outcomes, legendary orange marks a
//! personal best, and nothing else carries colour. Its chrome is the game's
//! gold unless the config asks for the owner's class ([`Chrome`]), in which
//! case a Priest's window and a Death Knight's are told apart at a glance
//! without either becoming unreadable. The overlay keeps the palette at the
//! bottom of this file, pixel for pixel; a renderer the two share takes the
//! surface's [`Look`]. Everything derived here is pure arithmetic over
//! `Class::rgb`; no cache, no art, no game data.

use iced::{Color, Font};
use wowdps_model::{Class, Spec};

// ---- the window's palette (the redesign's Tokens) -------------------------

/// Tooltip navy: the window, the meter.
pub(crate) const GROUND: Color = Color::from_rgb8(0x0A, 0x0E, 0x18);
/// Rail, inspector, top bar — every panel and card.
pub(crate) const SURFACE: Color = Color::from_rgb8(0x10, 0x16, 0x2A);
/// Selection, inputs.
pub(crate) const RAISE: Color = Color::from_rgb8(0x18, 0x21, 0x37);
/// Hairlines.
pub(crate) const LINE: Color = Color::from_rgb8(0x1E, 0x27, 0x40);
/// A floating surface's 1 px border: a menu, a sheet, the options card.
pub(crate) const EDGE: Color = Color::from_rgb8(0x33, 0x40, 0x5F);
/// Parchment: values and names.
pub(crate) const INK: Color = Color::from_rgb8(0xED, 0xE9, 0xDF);
/// Secondary words and numbers.
pub(crate) const INK_2: Color = Color::from_rgb8(0xA6, 0xAC, 0xC2);
/// Hints, disabled tabs, connectors, placeholders, a roster's rank and
/// trash rows. It clears 4.18:1 on GROUND — just under AA, as the prototype
/// draws it — 3.89:1 on SURFACE and 3.47:1 on RAISE, so no FIGURE is drawn
/// in it (a crit rate, an overkill: those are [`INK_2`], which clears AA on
/// all three) and nothing on a panel that a reader must read.
pub(crate) const INK_3: Color = Color::from_rgb8(0x6C, 0x74, 0x92);
/// WoW UI gold: active, focus.
pub(crate) const GOLD: Color = Color::from_rgb8(0xF2, 0xC1, 0x4B);
/// Gold labels and column heads.
pub(crate) const GOLD_DIM: Color = Color::from_rgb8(0xBD, 0x9A, 0x45);
/// Ink drawn ON gold.
pub(crate) const GOLD_INK: Color = Color::from_rgb8(0x1B, 0x14, 0x06);
/// Kill, timed, heals.
pub(crate) const GOOD: Color = Color::from_rgb8(0x58, 0xD0, 0x8A);
/// Wipe, over time, a death, damage in a recap.
pub(crate) const BAD: Color = Color::from_rgb8(0xFF, 0x5C, 0x63);
/// Legendary orange: personal bests only. Nothing marks one yet — the
/// redesign's Home step does — but it is a token, so it lives here.
#[allow(dead_code)]
pub(crate) const LEGENDARY: Color = Color::from_rgb8(0xFF, 0x80, 0x00);
/// The pointer's wash on a row: the prototype's `--hover`, a blue-tinted
/// breath rather than a grey slab.
pub(crate) const HOVER: Color = Color::from_rgba8(150, 170, 255, 0.055);
/// A scrollbar's thumb: the prototype's thin `#26304A`, darker than EDGE so
/// it never competes with a gold underline.
pub(crate) const THUMB: Color = Color::from_rgb8(0x26, 0x30, 0x4A);
/// The scrim under a modal: the ground's own navy, not black, so the window
/// under it still reads as the window.
pub(crate) const SCRIM: Color = Color::from_rgba8(4, 6, 12, 0.55);
/// The heat scale's middle (the R21 stack matrix): the prototype's
/// low-health amber, a data colour on a ramp — not the yellow that used to
/// mean "look here".
pub(crate) const AMBER: Color = Color::from_rgb8(0xE3, 0xB3, 0x41);
/// A bar's empty track under a row (`.hp`'s `rgba(255,255,255,.07)` kin):
/// the faint white every row's bar runs along, the overlay's included.
pub(crate) const TRACK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.04);

/// What a floating surface casts: a menu or a card (`.menu`, `0 20px 50px
/// rgba(0,0,0,.6)`), so it reads as a layer over the window, not as more
/// of it.
pub(crate) const SHADOW_MENU: iced::Shadow = iced::Shadow {
    color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
    offset: iced::Vector::new(0.0, 20.0),
    blur_radius: 50.0,
};
/// The modal sheet's deeper shadow (`.sheet`, `0 30px 70px
/// rgba(0,0,0,.65)`).
pub(crate) const SHADOW_SHEET: iced::Shadow = iced::Shadow {
    color: Color::from_rgba(0.0, 0.0, 0.0, 0.65),
    offset: iced::Vector::new(0.0, 30.0),
    blur_radius: 70.0,
};

// ---- the window's type ----------------------------------------------------

/// Barlow Semi Condensed, with its tabular figures baked into the default
/// digits (`crates/gui/fonts/README.md`): names and numbers in one voice,
/// and every column of numbers lines up. The window's default font.
pub(crate) const UI: Font = Font::with_name("Barlow Semi Condensed Tabular");
/// Stat values and the amount column.
pub(crate) const UI_MEDIUM: Font = Font {
    weight: iced::font::Weight::Medium,
    ..UI
};
/// Headings.
pub(crate) const UI_SEMIBOLD: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..UI
};
/// Encounter titles, and only those: the nod to the game's Friz Quadrata.
pub(crate) const TITLE: Font = Font::with_name("Marcellus");

/// The faces `window::settings` loads. The overlay loads none of them — its
/// text is iced's default font, pinned by the overlay's snapshot guard.
pub(crate) const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/BarlowSemiCondensedTabular-Regular.ttf"),
    include_bytes!("../fonts/BarlowSemiCondensedTabular-Medium.ttf"),
    include_bytes!("../fonts/BarlowSemiCondensedTabular-SemiBold.ttf"),
    include_bytes!("../fonts/Marcellus-Regular.ttf"),
];

/// The window's iced theme: the tokens as iced's own palette, so what iced
/// styles by itself — a text field, a checkbox, a scrollbar — wears them
/// too. Its primary is gold whatever the chrome: focus is gold (the
/// prototype's `:focus-visible`), not the owner's class.
///
/// Two of iced's derived shades are replaced by tokens, because iced paints
/// every scrollbar with them: the thumb (`strongest`) is the prototype's
/// thin [`THUMB`] and the rail (`weak`) is clear, as the prototype's track
/// is — not the bright slab iced derives from the ground. The overlay has a
/// theme of its own and never sees this one.
pub(crate) fn window_theme() -> iced::Theme {
    static THEME: std::sync::LazyLock<iced::Theme> = std::sync::LazyLock::new(|| {
        iced::Theme::custom_with_fn(
            "wowdps",
            iced::theme::Palette {
                background: GROUND,
                text: INK,
                primary: GOLD,
                success: GOOD,
                warning: GOLD,
                danger: BAD,
            },
            |palette| {
                let mut ext = iced::theme::palette::Extended::generate(palette);
                ext.background.strongest.color = THUMB;
                ext.background.weak.color = Color::TRANSPARENT;
                ext
            },
        )
    });
    THEME.clone()
}

// ---- the chrome -----------------------------------------------------------

/// What the window's chrome — the active tab's underline, a pressed chip's
/// border — is drawn in. `Gold` needs nobody, so it is right on the first
/// frame; `Class` is the owner's class colour, remembered in the config
/// beside the locked character so it is right on the first frame too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Chrome {
    #[default]
    Gold,
    Class,
}

impl Chrome {
    /// An unknown name is not an error, as with [`Density::from_name`].
    pub(crate) fn from_name(s: &str) -> Option<Self> {
        match s {
            "gold" => Some(Chrome::Gold),
            "class" => Some(Chrome::Class),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Chrome::Gold => "gold",
            Chrome::Class => "class",
        }
    }
}

/// The gold chrome: the game's own frames. Its ink is the dark gold-ink the
/// prototype puts on a gold fill.
pub(crate) const GOLD_ACCENT: Accent = Accent {
    base: GOLD,
    ink: GOLD_INK,
};

/// Every class, in the game's order.
pub(crate) const CLASSES: [Class; 13] = [
    Class::Warrior,
    Class::Paladin,
    Class::Hunter,
    Class::Rogue,
    Class::Priest,
    Class::DeathKnight,
    Class::Shaman,
    Class::Mage,
    Class::Warlock,
    Class::Monk,
    Class::Druid,
    Class::DemonHunter,
    Class::Evoker,
];

/// The class a config spells by its in-game name (`Class::name`), any case.
pub(crate) fn class_named(name: &str) -> Option<Class> {
    let name = name.trim();
    CLASSES
        .into_iter()
        .find(|c| c.name().eq_ignore_ascii_case(name))
}

/// A class colour drawn as TEXT: `Class::rgb` lifted toward white, 2 % at a
/// time, until it clears [`AA_CONTRAST`] on [`SURFACE`] — the prototype's
/// `textOn`. A bar keeps the raw `Class::rgb`: the bar is data, the name is
/// text, and Death Knight crimson at 3:1 is no name anyone can read.
pub(crate) fn class_text(class: Class) -> Color {
    let (r, g, b) = class.rgb();
    let raw = Color::from_rgb8(r, g, b);
    // 35 steps of 2 % is 70 %, the prototype's ceiling; no class needs it.
    (0..=35)
        .map(|step| lighten(raw, step as f32 * 0.02))
        .find(|c| contrast(*c, SURFACE) >= AA_CONTRAST)
        .unwrap_or_else(|| lighten(raw, 0.7))
}

/// The OWNER's name as text (`--you-text`): the class colour lifted the
/// same 2 % at a time until it clears [`YOU_CONTRAST`] on [`SURFACE`] — the
/// prototype's Warlock `#8788EE` becomes its `#9A9BF2`. Where the window
/// names whose window it is (the top bar's `.who`), that name reads a step
/// brighter than any other player's; everywhere else the owner is a player
/// like the rest ([`class_text`]).
pub(crate) fn you_text(class: Class) -> Color {
    let (r, g, b) = class.rgb();
    let raw = Color::from_rgb8(r, g, b);
    (0..=35)
        .map(|step| lighten(raw, step as f32 * 0.02))
        .find(|c| contrast(*c, SURFACE) >= YOU_CONTRAST)
        .unwrap_or_else(|| lighten(raw, 0.7))
}

// ---- what a shared renderer draws with ------------------------------------

/// The colours and number face a renderer the window and the overlay SHARE
/// draws with (the recap rows, the ability drill's strip, the comparison,
/// the instance strip). [`Look::OVERLAY`] is exactly what those renderers
/// drew before the window was redesigned — the overlay's snapshot guard
/// holds it there — and [`Look::WINDOW`] is the redesign's tokens. Each
/// field names a role, so a renderer asks for "the crit rate's colour"
/// and each surface answers for itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Look {
    /// Names and values.
    pub ink: Color,
    /// Captions and secondary words.
    pub dim: Color,
    /// Connectors, separators, the faintest marks.
    pub faint: Color,
    /// Column heads and stat labels.
    pub label: Color,
    /// A row's numbers, by column rank: (amount, rate, the rest).
    pub metrics: (Color, Color, Color),
    /// A kill, a heal, health left.
    pub good: Color,
    /// A wipe, a hit in a recap.
    pub bad: Color,
    /// The remaining-health strip under a recap line.
    pub health: Color,
    /// A pull still in progress.
    pub live: Color,
    /// An instance visit's Σ.
    pub sum: Color,
    /// A crit rate.
    pub crit: Color,
    /// Where the reader's attention is: the drilled player in a breadcrumb,
    /// a graph's read-out and its zoom window, a focus curve that has no
    /// school colour of its own.
    pub focus: Color,
    /// A small card's fill and its edge: the ability drill's stat strip.
    pub card: (Color, Color),
    /// A class colour drawn as text is lifted to clear AA ([`class_text`]).
    pub class_text: bool,
    /// Numbers and column heads.
    pub num: Font,
    /// The pointer's wash on a row.
    pub hover: Color,
    /// The selected row's fill.
    pub select: Color,
    /// A hit's amount in a death recap (a heal's is `good`).
    pub hit: Color,
    /// A graph's plot area.
    pub plot: Color,
    /// An idle disc's ring on the instance strip.
    pub ring: Color,
    /// A pull still in progress is said by a DOT beside its disc, in `live`,
    /// with a neutral fill and ring — never by the hues a kill or a wipe
    /// wears. `false` is the overlay's: the disc itself turns `live`.
    pub live_dot: bool,
    /// The `scale` this surface draws the shared renderers at: the overlay
    /// passes its own zoom (so this is 1), the window this — a step larger
    /// than the literals those renderers were written at, which puts their
    /// rows near the prototype's 14.5 px.
    pub type_scale: f32,
    /// A small caption under a value (the ability strip's labels, the
    /// school tag), before `type_scale`.
    pub caption: f32,
    /// The smallest text this surface draws, after `type_scale`: a shared
    /// renderer's small literals (a mitigation line, a caption) never fall
    /// under it. The overlay has none — its literals are its pixels.
    pub floor: f32,
    /// A table fits its columns to the width it is given, and seats its
    /// headings over its figures by construction (the window's comparison);
    /// `false` keeps the overlay's fixed three.
    pub fit: bool,
    /// An outcome is a SHAPE as well as a hue: on the instance strip a
    /// wipe's disc is a hollow ring and a kill's a filled disc, so the two
    /// never rest on red against green alone. `false` is the overlay's:
    /// both filled.
    pub wipe_ring: bool,
    /// An ability's name wears its school's colour (the overlay's
    /// breadcrumb). `false` draws the name in `ink` and keeps the school to
    /// its tag and its bars: colour is for people and outcomes (the window).
    pub school_names: bool,
    /// A shared renderer's fixed words (a stat caption: "total", "crit")
    /// start with a capital — the window's sentence case. The overlay's
    /// stay as they always were.
    pub capitals: bool,
}

impl Look {
    /// What the overlay has always drawn: white and `DIM` on its panel, the
    /// yellow that meant live, Σ, crit and "look here", and monospace
    /// numbers.
    pub(crate) const OVERLAY: Look = Look {
        ink: Color::WHITE,
        dim: DIM,
        faint: DIM,
        label: DIM,
        metrics: (Color::WHITE, Color::from_rgba(1.0, 1.0, 1.0, 0.75), DIM),
        good: GREEN,
        bad: RED,
        health: Color::from_rgb(0.35, 0.78, 0.42),
        live: YELLOW,
        sum: YELLOW,
        crit: YELLOW,
        focus: YELLOW,
        card: (
            Color::from_rgba(1.0, 1.0, 1.0, 0.05),
            Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        ),
        class_text: false,
        num: Font::MONOSPACE,
        hover: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
        select: Color::from_rgba(1.0, 1.0, 1.0, 0.04),
        hit: Color::WHITE,
        plot: Color::from_rgba(1.0, 1.0, 1.0, 0.04),
        ring: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
        live_dot: false,
        type_scale: 1.0,
        caption: 9.0,
        floor: 0.0,
        fit: false,
        wipe_ring: false,
        school_names: true,
        capitals: false,
    };

    /// The window: parchment ink, gold labels, green and red for outcomes
    /// only, a live pull as a red dot, and no semantic yellow anywhere.
    pub(crate) const WINDOW: Look = Look {
        ink: INK,
        dim: INK_2,
        faint: INK_3,
        label: GOLD_DIM,
        metrics: (INK, INK, INK_2),
        good: GOOD,
        bad: BAD,
        health: GOOD,
        live: BAD,
        sum: INK_2,
        crit: INK,
        focus: GOLD,
        card: (SURFACE, LINE),
        class_text: true,
        num: UI,
        hover: HOVER,
        select: RAISE,
        hit: BAD,
        plot: SURFACE,
        ring: INK_3,
        live_dot: true,
        type_scale: 1.2,
        caption: 11.0,
        floor: size::TINY,
        fit: true,
        wipe_ring: true,
        school_names: false,
        capitals: true,
    };

    /// A class colour as this surface draws it in text.
    pub(crate) fn class_ink(&self, class: Class) -> Color {
        if self.class_text {
            class_text(class)
        } else {
            let (r, g, b) = class.rgb();
            Color::from_rgb8(r, g, b)
        }
    }

    /// A text size a shared renderer computed, held at this surface's
    /// [`Look::floor`].
    pub(crate) fn text(&self, px: f32) -> f32 {
        px.max(self.floor)
    }

    /// A fixed word as this surface writes it: capitalised where the look
    /// says so ([`Look::capitals`]), as given otherwise. Only the first
    /// letter moves, so a word that is already a name keeps its case.
    pub(crate) fn word(&self, s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            Some(first) if self.capitals => first.to_uppercase().chain(chars).collect(),
            _ => s.to_string(),
        }
    }
}

/// The chrome accent: the game's gold ([`GOLD_ACCENT`]), or the owner's
/// class colour (design doc §2a) for a class chrome. The window draws only
/// `base` — an underline, a chip's edge, a selection's edge. `ink` is the
/// prototype's `--accent-ink`, the token the gold chrome carries (GOLD_INK):
/// nothing is drawn ON the accent yet, so only the tests read it, and they
/// hold it to AA so the first fill that does can trust it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Accent {
    /// The chrome color, `--accent`: `Class::rgb`, moved only if ink on it
    /// would fail AA ([`chrome_base`]).
    pub base: Color,
    /// Ink drawn ON the accent (`--accent-ink`): near-black for a light
    /// class, near-white otherwise; the gold chrome's is [`GOLD_INK`].
    pub ink: Color,
}

/// The luminance boundary between "ink on this must be dark" and "ink on this
/// must be light". It is WCAG's own crossover — the luminance at which black
/// text and white text contrast equally — because the point of the split is
/// legibility, not taste: above it near-white ink on the class color drops
/// under 4:1 and the accent stops being readable. The design doc names
/// Warrior (0xC69B6D, luminance ≈ 0.40) as the case to check; it sits well
/// clear on the LIGHT side, and so do Druid, Warlock and Evoker, which the
/// doc's prose guessed the other way. `every_class_clears_wcag_aa_on_its_accent`
/// is the test that actually pins this, and what a future tweak must satisfy;
/// a class no ink can carry has its CHROME color moved instead ([`chrome_base`]).
pub(crate) const LIGHT_THRESHOLD: f32 = 0.179;

/// sRGB relative luminance, gamma-decoded (WCAG formula).
pub(crate) fn relative_luminance(c: Color) -> f32 {
    let lin = |x: f32| {
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// `c` moved `t` (0..=1) of the way toward white.
pub(crate) fn lighten(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: c.r + (1.0 - c.r) * t,
        g: c.g + (1.0 - c.g) * t,
        b: c.b + (1.0 - c.b) * t,
        a: c.a,
    }
}

/// `c` moved `t` (0..=1) of the way toward black.
pub(crate) fn darken(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: c.r * (1.0 - t),
        g: c.g * (1.0 - t),
        b: c.b * (1.0 - t),
        a: c.a,
    }
}

/// Ink for a light accent: not pure black, so it reads as ink rather than a
/// hole in the color.
const INK_DARK: Color = Color::from_rgb(0.043, 0.047, 0.063);
/// Ink for a dark accent.
const INK_LIGHT: Color = Color::from_rgb(0.95, 0.95, 0.98);

/// The accent when a class chrome knows no class yet.
pub(crate) const NEUTRAL: Accent = Accent {
    base: Color::from_rgb(0.416, 0.718, 1.0),
    ink: INK_LIGHT,
};

/// WCAG AA for normal text: what every word the window draws owes the
/// ground it sits on, and what ink on the accent owes the accent.
pub(crate) const AA_CONTRAST: f32 = 4.5;

/// WCAG AAA: what the OWNER's name owes a panel (`--you-text`). The
/// prototype lifts its Warlock purple from 5.8:1 to 7.1:1 for the one name
/// the window is about; this is that lift as a rule, for every class.
pub(crate) const YOU_CONTRAST: f32 = 7.0;

/// WCAG contrast between two opaque colors.
pub(crate) fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// The class color moved, if it must be, until ink on it clears [`AA_CONTRAST`].
///
/// Shaman blue (`0x0070DD`) is the case that forces this: at luminance 0.168
/// it manages 4.33:1 with near-white ink and 4.06:1 with near-black — its best
/// with EITHER ink is under AA, so no choice of ink fixes it and the color
/// itself has to move. It moves along its own hue (a plain darken/lighten, no
/// hue rotation), by the smallest step that clears the bar, so the accent
/// still reads as that class's color.
///
/// This is the CHROME color only. A meter row's bar keeps `Class::rgb`
/// untouched: the bar is data — it is how a player is identified at a glance,
/// and it must match what every other meter and the game itself draw — while
/// the chrome is decoration that has to carry text. Two different jobs, two
/// different colors, deliberately.
fn chrome_base(class: Class) -> Color {
    let (r, g, b) = class.rgb();
    let raw = Color::from_rgb8(r, g, b);
    let light = relative_luminance(raw) > LIGHT_THRESHOLD;
    let ink = if light { INK_DARK } else { INK_LIGHT };
    let mut base = raw;
    // 1% steps: 50 of them is a color half the way to black or white, which
    // no class needs (Shaman clears at the third).
    for _ in 0..50 {
        if contrast(ink, base) >= AA_CONTRAST {
            break;
        }
        // A dark class gets darker (its ink is light), a light one lighter.
        base = if light {
            lighten(base, 0.01)
        } else {
            darken(base, 0.01)
        };
    }
    base
}

/// The accent for a player. `spec` is accepted and ignored today (class is
/// the honest default per §2a); it exists so a later within-class tint is a
/// one-function change. `None` class yields [`NEUTRAL`].
pub(crate) fn accent(class: Option<Class>, _spec: Option<Spec>) -> Accent {
    let Some(class) = class else { return NEUTRAL };
    let base = chrome_base(class);
    let light = relative_luminance(base) > LIGHT_THRESHOLD;
    Accent {
        base,
        ink: if light { INK_DARK } else { INK_LIGHT },
    }
}

/// The one wash an accent may lay on a surface: a pressed chip's, the
/// prototype's `color-mix(accent 9%)`. There is no accent FILL any more — a
/// tab is an underline, a stat card a plain panel.
pub(crate) fn accent_wash(a: Accent) -> Color {
    Color { a: 0.09, ..a.base }
}

// ---- the window's type scale ----------------------------------------------
//
// The prototype's Tokens type specimens, in logical pixels: encounter
// titles Marcellus 27 (22 in a narrow window), stat values 16 at 500, rows
// 15 with their numbers at 14.5, labels 13.5 in gold-dim, axis ticks 11.5.
// Window-only — the overlay names none of these (its sizes are literals it
// multiplies by its own zoom), so the scale follows the prototype without
// moving an overlay pixel.
pub(crate) mod size {
    pub(crate) const DISPLAY: f32 = 22.0; // a headline number
    pub(crate) const ENCOUNTER: f32 = 27.0; // an encounter title, in Marcellus
    pub(crate) const ENCOUNTER_NARROW: f32 = 22.0; // the same under `NARROW`
    pub(crate) const TITLE: f32 = 17.0; // a screen title
    pub(crate) const HEAD: f32 = 16.0; // panel headings, durations
    pub(crate) const STAT: f32 = 16.0; // a stat card's value (weight 500)
    pub(crate) const NAME: f32 = 15.0; // a row's name
    pub(crate) const PLACE: f32 = 15.0; // the top bar's places (weight 500)
    pub(crate) const BODY: f32 = 14.5; // body text
    pub(crate) const NUM: f32 = 14.5; // every numeric cell
    pub(crate) const TAB: f32 = 14.5; // a view tab (weight 500)
    pub(crate) const SMALL: f32 = 13.5; // captions, a roster's rank
    pub(crate) const LABEL: f32 = 13.5; // column heads and stat labels, gold-dim
    pub(crate) const MICRO: f32 = 13.0; // tags, chips, badges
    pub(crate) const TINY: f32 = 12.0; // eyebrow notes, key hints
    pub(crate) const KBD: f32 = 11.5; // a keycap on the `?` sheet (`kbd`, weight 500)
    pub(crate) const MARK: f32 = 18.0; // the top bar's wordmark, in Marcellus (`.mark`)
    /// A top-bar icon button's glyph (`.ibtn svg`).
    pub(crate) const ICON: f32 = 16.0;
    /// The instance strip's step chevrons.
    pub(crate) const CHEVRON: f32 = 14.0;
    /// A view tab's line icon (`.vtab svg.i`), at [`super::TAB_ICON_ALPHA`].
    pub(crate) const TAB_ICON: f32 = 15.0;
    /// The live tab's dot (`.pulse`).
    pub(crate) const DOT: f32 = 8.0;
}

/// The view tab icons' opacity (`.vtab svg.i{opacity:.85}`).
pub(crate) const TAB_ICON_ALPHA: f32 = 0.85;

/// The width under which the window lays out narrow: the prototype's
/// `@container app (max-width: 820px)`, less the window's 10 px frame.
pub(crate) const NARROW: f32 = 800.0;

/// The window's pitches, in logical pixels: a meter row (`.trow`), a drill
/// row (`.irow`), the pinned total (`.ttotal`), the top bar and its places
/// (`.top`, `.place`), and a view tab (`.vtab`).
pub(crate) mod pitch {
    pub(crate) const ROW: f32 = 32.0;
    pub(crate) const DRILL_ROW: f32 = 29.0;
    pub(crate) const TOTAL: f32 = 33.0;
    pub(crate) const TOP_BAR: f32 = 44.0;
    pub(crate) const PLACE: f32 = 42.0;
    pub(crate) const TAB: f32 = 37.0;
    /// An ability drill's target row: a step under a drill row, as its text
    /// is a step smaller.
    pub(crate) const TARGET_ROW: f32 = DRILL_ROW - 2.0;
    /// A meter row in the compact density.
    pub(crate) const COMPACT_ROW: f32 = 26.0;
    /// A top-bar icon button's square hit area (`.ibtn`).
    pub(crate) const ICON_BUTTON: f32 = 30.0;
    /// A menu's width: the prototype's `.menu{min-width:250px}` and ten
    /// more, room for a name with its realm beside its fight count.
    pub(crate) const MENU_W: f32 = 260.0;
}

/// Two densities. `Comfortable` is the window default; `Compact` reproduces
/// today's tighter metrics and is what the overlay would ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Density {
    #[default]
    Comfortable,
    Compact,
}

impl Density {
    /// A meter row's pitch: the prototype's `.trow`, or a tighter one.
    pub(crate) fn row_h(self) -> f32 {
        match self {
            Density::Comfortable => pitch::ROW,
            Density::Compact => pitch::COMPACT_ROW,
        }
    }

    pub(crate) fn gap(self) -> f32 {
        match self {
            Density::Comfortable => 8.0,
            Density::Compact => 4.0,
        }
    }

    pub(crate) fn pad(self) -> f32 {
        match self {
            Density::Comfortable => 10.0,
            Density::Compact => 6.0,
        }
    }

    /// An unknown name is not an error: a typo in a hand-edited config falls
    /// back to the default rather than failing the whole file.
    pub(crate) fn from_name(s: &str) -> Option<Self> {
        match s {
            "comfortable" => Some(Density::Comfortable),
            "compact" => Some(Density::Compact),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        }
    }
}

// ---- the palette that used to live in view.rs ---------------------------
//
// The overlay's, and [`Look::OVERLAY`]'s. The window no longer draws with
// these: its own are the tokens at the top of this file.

pub(crate) const DIM: Color = Color::from_rgb(0.55, 0.57, 0.62);
pub(crate) const GREEN: Color = Color::from_rgb(0.60, 0.76, 0.47);
pub(crate) const RED: Color = Color::from_rgb(0.88, 0.42, 0.46);
pub(crate) const YELLOW: Color = Color::from_rgb(0.90, 0.75, 0.48);

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Class; 13] = CLASSES;

    /// The tokens' text colours read on both the window's grounds: WCAG AA
    /// for normal text, since a label at 13.5 px is normal text.
    #[test]
    fn the_token_inks_clear_aa_on_the_window_grounds() {
        for (name, ink) in [("INK", INK), ("INK_2", INK_2), ("GOLD_DIM", GOLD_DIM)] {
            for (ground, bg) in [("GROUND", GROUND), ("SURFACE", SURFACE)] {
                let c = contrast(ink, bg);
                assert!(c >= AA_CONTRAST, "{name} on {ground} is only {c:.2}:1");
            }
        }
        let c = contrast(GOLD_INK, GOLD);
        assert!(c >= AA_CONTRAST, "GOLD_INK on GOLD is only {c:.2}:1");
        // And the gold chrome carries its own ink.
        assert_eq!(GOLD_ACCENT.ink, GOLD_INK);
        assert_eq!(GOLD_ACCENT.base, GOLD);
        // The figures a table draws — the amount and rate in INK, the share,
        // counts, crit and overkill in INK_2 — stay AA on every fill a row
        // can wear: the ground, a panel, the pointer's wash, the selection.
        let hover = over(HOVER, GROUND);
        for (name, ink) in [("INK", INK), ("INK_2", INK_2)] {
            for (fill, bg) in [
                ("GROUND", GROUND),
                ("SURFACE", SURFACE),
                ("HOVER", hover),
                ("RAISE", RAISE),
            ] {
                let c = contrast(ink, bg);
                assert!(c >= AA_CONTRAST, "{name} on {fill} is only {c:.2}:1");
            }
        }
        for col in crate::table::ALL_COLS {
            for total in [false, true] {
                let ink = col.ink(total);
                assert!(
                    contrast(ink, RAISE) >= AA_CONTRAST,
                    "{col:?} is drawn in an ink the selected row cannot carry"
                );
            }
        }
    }

    /// `top` composited over an opaque `bottom`.
    fn over(top: Color, bottom: Color) -> Color {
        let mix = |t: f32, b: f32| t * top.a + b * (1.0 - top.a);
        Color::from_rgb(
            mix(top.r, bottom.r),
            mix(top.g, bottom.g),
            mix(top.b, bottom.b),
        )
    }

    /// The owner's own name is the prototype's `--you-text`: a step past
    /// AA to AAA, and for Tranqlock's Warlock purple, the prototype's
    /// `#9A9BF2` to within a rounding.
    #[test]
    fn the_owner_s_name_reads_a_step_brighter_than_a_player_s() {
        for class in ALL {
            let you = you_text(class);
            let c = contrast(you, SURFACE);
            assert!(c >= YOU_CONTRAST, "{class:?} as the owner is only {c:.2}:1");
            assert!(
                relative_luminance(you) >= relative_luminance(class_text(class)),
                "{class:?}"
            );
        }
        let lock = you_text(Class::Warlock);
        let proto = Color::from_rgb8(0x9A, 0x9B, 0xF2);
        for (a, b) in [(lock.r, proto.r), (lock.g, proto.g), (lock.b, proto.b)] {
            assert!((a - b).abs() <= 2.0 / 255.0, "{lock:?} vs {proto:?}");
        }
    }

    /// Every class's name, drawn as text on a panel, is readable — and the
    /// lift is the smallest that does it, so the colour is still theirs.
    #[test]
    fn class_text_clears_aa_on_a_panel_for_every_class() {
        for class in ALL {
            let t = class_text(class);
            let c = contrast(t, SURFACE);
            assert!(c >= AA_CONTRAST, "{class:?} as text is only {c:.2}:1");
            let (r, g, b) = class.rgb();
            let raw = Color::from_rgb8(r, g, b);
            if contrast(raw, SURFACE) >= AA_CONTRAST {
                assert_eq!(t, raw, "{class:?} already reads, so it is not moved");
            } else {
                // One step less would not have cleared the bar.
                let lifted = (0..=35)
                    .map(|s| lighten(raw, s as f32 * 0.02))
                    .position(|c| c == t)
                    .expect("the text colour is one of the lift steps");
                assert!(lifted > 0, "{class:?} moved");
                let before = lighten(raw, (lifted - 1) as f32 * 0.02);
                assert!(contrast(before, SURFACE) < AA_CONTRAST, "{class:?}");
            }
        }
        // The dark classes are the ones that must move.
        for class in [Class::DeathKnight, Class::Shaman, Class::DemonHunter] {
            let (r, g, b) = class.rgb();
            assert_ne!(class_text(class), Color::from_rgb8(r, g, b), "{class:?}");
        }
        // The window lifts; the overlay draws them raw.
        assert_eq!(
            Look::WINDOW.class_ink(Class::DeathKnight),
            class_text(Class::DeathKnight)
        );
        assert_eq!(
            Look::OVERLAY.class_ink(Class::DeathKnight),
            Color::from_rgb8(0xC4, 0x1E, 0x3A)
        );
    }

    #[test]
    fn classes_are_named_the_way_the_game_names_them() {
        for class in ALL {
            assert_eq!(class_named(class.name()), Some(class));
            assert_eq!(
                class_named(&format!(" {} ", class.name().to_uppercase())),
                Some(class)
            );
        }
        assert_eq!(class_named("Bard"), None);
    }

    #[test]
    fn chrome_names_round_trip_and_default_to_gold() {
        assert_eq!(Chrome::default(), Chrome::Gold);
        for c in [Chrome::Gold, Chrome::Class] {
            assert_eq!(Chrome::from_name(c.name()), Some(c));
        }
        assert_eq!(Chrome::from_name("purple"), None);
    }

    /// The window has no semantic yellow: live is red, Σ is secondary ink,
    /// crit is plain ink — and the overlay keeps every one of them yellow.
    #[test]
    fn the_window_look_carries_no_yellow() {
        let w = Look::WINDOW;
        for (role, c) in [
            ("live", w.live),
            ("sum", w.sum),
            ("crit", w.crit),
            ("focus", w.focus),
            ("good", w.good),
            ("bad", w.bad),
        ] {
            assert_ne!(c, YELLOW, "{role}");
        }
        assert_eq!(w.live, BAD);
        assert_eq!(w.crit, INK);
        assert_eq!(w.num, UI);
        let o = Look::OVERLAY;
        assert_eq!(
            (o.live, o.sum, o.crit, o.focus),
            (YELLOW, YELLOW, YELLOW, YELLOW)
        );
        assert_eq!(o.num, Font::MONOSPACE);
        // The roles the window added keep the overlay's old pixels: its
        // white washes, its white recap amounts, its white-ringed discs,
        // its plot, the disc that turns live, and its own zoom.
        assert_eq!(o.hover, Color::from_rgba(1.0, 1.0, 1.0, 0.07));
        assert_eq!(o.select, Color::from_rgba(1.0, 1.0, 1.0, 0.04));
        assert_eq!(o.plot, Color::from_rgba(1.0, 1.0, 1.0, 0.04));
        assert_eq!(o.ring, Color::from_rgba(1.0, 1.0, 1.0, 0.25));
        assert_eq!(o.hit, Color::WHITE);
        assert!(!o.live_dot);
        assert_eq!((o.type_scale, o.caption), (1.0, 9.0));
        // No floor, fixed tables, filled wipes, school-coloured names.
        assert_eq!(o.floor, 0.0);
        assert!(!o.fit && !o.wipe_ring && o.school_names);
        assert_eq!(o.text(8.5), 8.5, "the overlay's literals are its pixels");
        // The window's are the prototype's tokens.
        assert_eq!(
            (w.hover, w.select, w.plot, w.ring, w.hit),
            (HOVER, RAISE, SURFACE, INK_3, BAD)
        );
        assert!(w.live_dot);
        assert!(w.fit && w.wipe_ring && !w.school_names);
        assert_eq!(w.text(9.0 * w.type_scale), size::TINY);
    }

    /// The window's type is the prototype's Tokens specimens.
    #[test]
    fn the_window_type_is_the_prototypes() {
        assert_eq!(size::ENCOUNTER, 27.0);
        assert_eq!(size::ENCOUNTER_NARROW, 22.0);
        assert_eq!(size::STAT, 16.0);
        assert_eq!((size::NAME, size::NUM), (15.0, 14.5));
        assert_eq!(size::LABEL, 13.5);
        assert_eq!((size::PLACE, size::TAB), (15.0, 14.5));
        assert_eq!(
            (pitch::ROW, pitch::TOTAL, pitch::TAB, pitch::PLACE),
            (32.0, 33.0, 37.0, 42.0)
        );
        // The shared renderers' literals, scaled for the window, land on
        // the prototype's rows and captions.
        let w = Look::WINDOW;
        assert!((12.0 * w.type_scale - size::NUM).abs() < 0.2);
        assert!((w.caption * w.type_scale - 13.0).abs() < 0.3);
    }

    /// The window's digits share one advance: a column of numbers lines up
    /// without a monospace. Measured through the renderer's own text system
    /// — the face cosmic-text actually picks for [`UI`], at every weight the
    /// window uses — so a font that stopped being ours, or stopped being
    /// tabular, fails here.
    #[test]
    fn the_window_digits_are_tabular() {
        use iced_tiny_skia::graphics::text::{cosmic_text, font_system, to_attributes};
        let mut system = font_system().write().expect("the font system lock");
        for bytes in FONTS {
            system.load_font(std::borrow::Cow::Borrowed(bytes));
        }
        // One digit advance per weight: three different ones (481 / 495 /
        // 507 units, `fonts/README.md`), so a weight that fell back to the
        // Regular face — same family, same tabular digits — is caught.
        let mut advances = Vec::new();
        for font in [UI, UI_MEDIUM, UI_SEMIBOLD] {
            let raw = system.raw();
            let mut buffer = cosmic_text::Buffer::new(raw, cosmic_text::Metrics::new(20.0, 24.0));
            buffer.set_text(
                raw,
                "0123456789",
                &to_attributes(font),
                cosmic_text::Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(raw, false);
            let glyphs: Vec<(f32, cosmic_text::fontdb::ID)> = buffer
                .layout_runs()
                .flat_map(|run| run.glyphs.iter().map(|g| (g.w, g.font_id)))
                .collect();
            assert_eq!(glyphs.len(), 10, "{font:?}: one glyph per digit");
            let first = glyphs.first().map(|g| g.0).unwrap_or_default();
            assert!(first > 0.0);
            for (w, id) in &glyphs {
                assert!(
                    (w - first).abs() < 1e-3,
                    "{font:?}: a digit {w} wide beside one {first} wide"
                );
                let face = raw.db().face(*id);
                let families: Vec<String> = face
                    .map(|f| f.families.iter().map(|(n, _)| n.clone()).collect())
                    .unwrap_or_default();
                assert!(
                    families
                        .iter()
                        .any(|n| n == "Barlow Semi Condensed Tabular"),
                    "{font:?} was drawn from {families:?}, not the window's own face"
                );
                // The face of the weight asked for, not a neighbour.
                assert_eq!(
                    face.map(|f| f.weight.0),
                    Some(font.weight_value()),
                    "{font:?} was drawn from another weight's face"
                );
            }
            advances.push(first);
        }
        assert!(
            advances.windows(2).all(|w| w[0] < w[1]),
            "Regular, Medium and SemiBold digits: {advances:?}"
        );
    }

    /// The numeric weight a [`Font`] asks for, as fontdb states a face's.
    trait WeightValue {
        fn weight_value(&self) -> u16;
    }

    impl WeightValue for Font {
        fn weight_value(&self) -> u16 {
            match self.weight {
                iced::font::Weight::Thin => 100,
                iced::font::Weight::ExtraLight => 200,
                iced::font::Weight::Light => 300,
                iced::font::Weight::Normal => 400,
                iced::font::Weight::Medium => 500,
                iced::font::Weight::Semibold => 600,
                iced::font::Weight::Bold => 700,
                iced::font::Weight::ExtraBold => 800,
                iced::font::Weight::Black => 900,
            }
        }
    }

    #[test]
    fn light_classes_carry_dark_ink() {
        for class in [Class::Priest, Class::Rogue, Class::Monk, Class::Warrior] {
            let a = accent(Some(class), None);
            assert!(
                relative_luminance(a.base) > LIGHT_THRESHOLD,
                "{class:?} is a light accent"
            );
            assert!(relative_luminance(a.ink) < 0.1, "{class:?} ink is not dark");
        }
    }

    #[test]
    fn dark_classes_keep_light_ink() {
        for class in [Class::DeathKnight, Class::Shaman, Class::DemonHunter] {
            let a = accent(Some(class), None);
            assert!(
                relative_luminance(a.base) <= LIGHT_THRESHOLD,
                "{class:?} is a dark accent"
            );
            assert_eq!(a.ink, INK_LIGHT, "{class:?}");
        }
    }

    /// The doc names Warrior as the boundary case; pin it so a threshold
    /// tweak fails loudly instead of quietly making tan unreadable.
    #[test]
    fn warrior_sits_on_the_light_side_of_the_threshold() {
        let a = accent(Some(Class::Warrior), None);
        assert!(relative_luminance(a.base) > LIGHT_THRESHOLD);
        assert!(
            contrast(a.ink, a.base) > contrast(INK_LIGHT, a.base),
            "dark ink is the legible choice on Warrior tan"
        );
    }

    /// The premise of the accent: text on it is readable. The floor is WCAG
    /// AA for normal text, not the 3.0 large-text bar — a label at 11pt is
    /// normal text by any reading.
    #[test]
    fn every_class_clears_wcag_aa_on_its_accent() {
        for class in ALL {
            let a = accent(Some(class), None);
            let c = contrast(a.ink, a.base);
            assert!(c >= AA_CONTRAST, "{class:?} ink on base is only {c:.2}:1");
        }
    }

    /// Shaman blue is the class that forces `chrome_base` to exist: its best
    /// contrast with EITHER ink is 4.33:1, so the color had to move.
    #[test]
    fn only_the_classes_that_must_move_do() {
        let moved: Vec<Class> = ALL
            .into_iter()
            .filter(|c| {
                let (r, g, b) = c.rgb();
                accent(Some(*c), None).base != Color::from_rgb8(r, g, b)
            })
            .collect();
        assert_eq!(moved, vec![Class::Shaman], "{moved:?}");
        // And it moved along its own hue: still blue, just deeper.
        let a = accent(Some(Class::Shaman), None);
        assert!(a.base.b > a.base.g && a.base.g > a.base.r);
        assert!(relative_luminance(a.base) < 0.1681, "it darkened");
    }

    #[test]
    fn no_class_is_the_neutral_accent() {
        assert_eq!(accent(None, None), NEUTRAL);
        for class in ALL {
            assert_ne!(accent(Some(class), None), NEUTRAL, "{class:?}");
        }
    }

    #[test]
    fn the_spec_does_not_change_the_accent_yet() {
        assert_eq!(
            accent(Some(Class::Mage), None),
            accent(Some(Class::Mage), Some(Spec::Fire))
        );
    }

    #[test]
    fn densities_are_ordered() {
        let (c, t) = (Density::Comfortable, Density::Compact);
        assert!(t.row_h() < c.row_h());
        assert!(t.gap() < c.gap());
        assert!(t.pad() < c.pad());
        assert_eq!(Density::from_name(c.name()), Some(c));
        assert_eq!(Density::from_name(t.name()), Some(t));
        assert_eq!(Density::from_name("cozy"), None);
        assert_eq!(Density::default(), c);
    }

    #[test]
    fn lighten_and_darken_are_bounded() {
        let c = Color::from_rgb(0.5, 0.25, 0.75);
        assert_eq!(lighten(c, 0.0), c);
        assert_eq!(darken(c, 0.0), c);
        assert_eq!(lighten(c, 1.0), Color::from_rgb(1.0, 1.0, 1.0));
        assert_eq!(darken(c, 1.0), Color::from_rgb(0.0, 0.0, 0.0));
        // Out-of-range factors clamp rather than overshoot into negatives.
        assert_eq!(darken(c, 2.0), darken(c, 1.0));
        assert_eq!(lighten(c, -1.0), c);
    }

    #[test]
    fn the_accent_wash_is_faint() {
        let w = accent_wash(GOLD_ACCENT);
        assert!(w.a < 0.1);
        assert_eq!(Color { a: 1.0, ..w }, GOLD);
    }
}
