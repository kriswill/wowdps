//! One place every color, size and spacing constant comes from.
//!
//! The chrome takes its accent from whoever the screen is about — the pinned
//! player's class — so a Priest's window and a Death Knight's are told apart
//! at a glance without either becoming unreadable. Everything derived here is
//! pure arithmetic over `Class::rgb`; no cache, no art, no game data.

use iced::Color;
use wowdps_model::{Class, Spec};

/// The spec-derived chrome accent (design doc §2a).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Accent {
    /// The chrome color, `--spec`: `Class::rgb`, moved only if ink on it
    /// would fail AA ([`chrome_base`]).
    pub base: Color,
    /// `--spec-2`: the gradient's other stop — base toward white, or toward
    /// black when the class is light, as far as the ink can follow
    /// ([`lift_for`]). The gradient always runs `lift -> base`.
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

/// The accent when no owner/class is known — the current flat chrome.
pub(crate) const NEUTRAL: Accent = Accent {
    base: Color::from_rgb(0.416, 0.718, 1.0),
    // `lighten(base, 0.35)`, spelled out because const fns cannot do it.
    lift: Color::from_rgb(0.620, 0.817, 1.0),
    ink: INK_LIGHT,
    heading: Color::from_rgb(0.416, 0.718, 1.0),
    light: false,
};

/// WCAG AA for normal text. Text sits ON the accent — a tab label, a headline
/// stat — so this, not the 3.0 large-text bar, is what the accent owes.
pub(crate) const AA_CONTRAST: f32 = 4.5;

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

/// The gradient's other end. A light class has nowhere brighter to go —
/// lifting Priest white yields white — so its second stop goes the other way
/// and the gradient still reads as a gradient.
///
/// The excursion is bounded by legibility, not by taste: the SAME ink is drawn
/// across the whole gradient, so a stop the ink cannot survive is a stop that
/// makes half a tab label vanish. The nominal 35 % is pulled back a step at a
/// time until the ink clears [`LIFT_CONTRAST`] on it.
fn lift_for(base: Color, ink: Color, light: bool) -> Color {
    let mut t = 0.35;
    loop {
        let lift = if light {
            darken(base, t)
        } else {
            lighten(base, t)
        };
        // A tenth is still a visible ramp; below that a gradient is a flat
        // fill with extra steps, and we take the flat fill.
        if t <= 0.10 || contrast(ink, lift) >= LIFT_CONTRAST {
            return lift;
        }
        t -= 0.01;
    }
}

/// What the ink owes the gradient's far stop. Lower than [`AA_CONTRAST`] on
/// purpose: text is centred over the fill, so the far stop is the edge of a
/// glyph rather than the body of one, and holding it to AA would flatten
/// every gradient in the window.
pub(crate) const LIFT_CONTRAST: f32 = 3.0;

/// The accent for a player. `spec` is accepted and ignored today (class is
/// the honest default per §2a); it exists so a later within-class tint is a
/// one-function change. `None` class yields [`NEUTRAL`].
pub(crate) fn accent(class: Option<Class>, _spec: Option<Spec>) -> Accent {
    let Some(class) = class else { return NEUTRAL };
    let base = chrome_base(class);
    let light = relative_luminance(base) > LIGHT_THRESHOLD;
    let ink = if light { INK_DARK } else { INK_LIGHT };
    let lift = lift_for(base, ink, light);
    Accent {
        base,
        lift,
        ink,
        heading: if relative_luminance(lift) > relative_luminance(base) {
            lift
        } else {
            base
        },
        light,
    }
}

/// A two-stop linear gradient `lift -> base`, left to right — what an active
/// tab, a headline stat card and the two-tone title sit on.
pub(crate) fn accent_fill(a: Accent) -> iced::Background {
    iced::Background::Gradient(
        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
            .add_stop(0.0, a.lift)
            .add_stop(1.0, a.base)
            .into(),
    )
}

// ---- type scale (design doc §3c: "one scale, two densities") -------------
pub(crate) mod size {
    pub(crate) const DISPLAY: f32 = 22.0; // Home headline numbers
    pub(crate) const TITLE: f32 = 16.0; // screen title (today's 16)
    pub(crate) const HEAD: f32 = 14.0; // panel headings, durations
    pub(crate) const BODY: f32 = 13.0; // row labels
    pub(crate) const SMALL: f32 = 12.0; // captions, footers
    pub(crate) const MICRO: f32 = 11.0; // tags, ranks, column heads
    pub(crate) const TINY: f32 = 10.0; // panel eyebrow labels
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
    pub(crate) fn row_h(self) -> f32 {
        match self {
            Density::Comfortable => 22.0,
            Density::Compact => 18.0,
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

pub(crate) const DIM: Color = Color::from_rgb(0.55, 0.57, 0.62);
pub(crate) const GREEN: Color = Color::from_rgb(0.60, 0.76, 0.47);
pub(crate) const RED: Color = Color::from_rgb(0.88, 0.42, 0.46);
pub(crate) const YELLOW: Color = Color::from_rgb(0.90, 0.75, 0.48);
/// Panel background — the values the options panel has always used inline.
pub(crate) const PANEL: Color = Color::from_rgba(0.09, 0.10, 0.14, 0.97);
/// Hairline border on a panel.
pub(crate) const RULE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.25);

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Class; 13] = [
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

    #[test]
    fn light_classes_darken_and_flip_ink() {
        for class in [Class::Priest, Class::Rogue, Class::Monk, Class::Warrior] {
            let a = accent(Some(class), None);
            assert!(a.light, "{class:?}");
            assert!(
                relative_luminance(a.lift) < relative_luminance(a.base),
                "{class:?} lifts the wrong way"
            );
            assert!(relative_luminance(a.ink) < 0.1, "{class:?} ink is not dark");
        }
    }

    #[test]
    fn dark_classes_lift_and_keep_light_ink() {
        for class in [Class::DeathKnight, Class::Shaman, Class::DemonHunter] {
            let a = accent(Some(class), None);
            assert!(!a.light, "{class:?}");
            assert!(
                relative_luminance(a.lift) > relative_luminance(a.base),
                "{class:?} does not lift"
            );
            assert_eq!(a.ink, INK_LIGHT, "{class:?}");
        }
    }

    /// The doc names Warrior as the boundary case; pin it so a threshold
    /// tweak fails loudly instead of quietly making tan unreadable.
    #[test]
    fn warrior_sits_on_the_light_side_of_the_threshold() {
        let a = accent(Some(Class::Warrior), None);
        assert!(a.light);
        assert!(
            contrast(a.ink, a.base) > contrast(INK_LIGHT, a.base),
            "dark ink is the legible choice on Warrior tan"
        );
    }

    /// The premise of the whole module: text on the accent is readable. The
    /// floor is WCAG AA for normal text, not the 3.0 large-text bar — a tab
    /// label at 11pt is normal text by any reading.
    #[test]
    fn every_class_clears_wcag_aa_on_its_accent() {
        for class in ALL {
            let a = accent(Some(class), None);
            let c = contrast(a.ink, a.base);
            assert!(c >= AA_CONTRAST, "{class:?} ink on base is only {c:.2}:1");
            // The lift is the gradient's other end and carries the same text.
            let l = contrast(a.ink, a.lift);
            assert!(l >= 3.0, "{class:?} ink on the lift is only {l:.2}:1");
            let h = contrast(a.heading, PANEL);
            assert!(h >= 3.0, "{class:?} heading on a panel is only {h:.2}:1");
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
        // Spelled-out const: it must still be `lighten(base, 0.35)`.
        let lift = lighten(NEUTRAL.base, 0.35);
        assert!((lift.r - NEUTRAL.lift.r).abs() < 0.002);
        assert!((lift.g - NEUTRAL.lift.g).abs() < 0.002);
        assert!((lift.b - NEUTRAL.lift.b).abs() < 0.002);
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
    fn the_accent_fill_is_a_two_stop_gradient() {
        assert!(matches!(
            accent_fill(accent(Some(Class::Mage), None)),
            iced::Background::Gradient(_)
        ));
    }
}
