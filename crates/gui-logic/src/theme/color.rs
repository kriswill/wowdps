//! Colour as plain numbers, and the arithmetic the chrome rests on: WCAG
//! luminance and contrast, the class accent moved until ink on it reads,
//! a class colour lifted until it reads as text. Each GUI turns a [`Color`]
//! into its own type at the edge; nothing here names a framework.

use wowdps_model::Class;

/// An sRGB colour, each channel 0..=1, with straight (not premultiplied)
/// alpha — the same four floats iced's and GPUI's colour types hold, so a
/// conversion is a copy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `0xRRGGBB`, opaque: each byte over 255, as iced's `from_rgb8` does,
    /// so a token reads the same float in either GUI.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgba8(
            ((rgb >> 16) & 0xFF) as u8,
            ((rgb >> 8) & 0xFF) as u8,
            (rgb & 0xFF) as u8,
            1.0,
        )
    }

    pub const fn rgba8(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a,
        }
    }

    /// The same colour at alpha `a`.
    pub const fn alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// The class's own colour, `Class::rgb` as it is.
    pub fn of_class(class: Class) -> Self {
        let (r, g, b) = class.rgb();
        Self::rgba8(r, g, b, 1.0)
    }

    /// `self` moved `t` (0..=1) of the way toward white.
    pub fn lighten(self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r + (1.0 - self.r) * t,
            g: self.g + (1.0 - self.g) * t,
            b: self.b + (1.0 - self.b) * t,
            a: self.a,
        }
    }

    /// `self` moved `t` (0..=1) of the way toward black.
    pub fn darken(self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r * (1.0 - t),
            g: self.g * (1.0 - t),
            b: self.b * (1.0 - t),
            a: self.a,
        }
    }

    /// `self` laid over an opaque `under`: what a translucent wash reads as.
    pub fn over(self, under: Color) -> Self {
        let mix = |t: f32, b: f32| t * self.a + b * (1.0 - self.a);
        Self::rgb(
            mix(self.r, under.r),
            mix(self.g, under.g),
            mix(self.b, under.b),
        )
    }

    /// sRGB relative luminance, gamma-decoded (WCAG formula).
    pub fn luminance(self) -> f32 {
        let lin = |x: f32| {
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }
}

/// WCAG contrast between two opaque colours.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (a.luminance(), b.luminance());
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// WCAG AA for normal text: what every word the window draws owes the
/// ground it sits on, and what ink on the accent owes the accent.
pub const AA_CONTRAST: f32 = 4.5;

/// WCAG AAA: what the OWNER's name owes a panel (`--you-text`).
pub const YOU_CONTRAST: f32 = 7.0;

/// The luminance boundary between "ink on this must be dark" and "ink on
/// this must be light": WCAG's own crossover, where black and white text
/// contrast equally. `every_class_clears_aa_on_its_accent` pins it.
pub const LIGHT_THRESHOLD: f32 = 0.179;

/// Ink for a light accent: not pure black, so it reads as ink rather than a
/// hole in the colour.
pub const INK_DARK: Color = Color::rgb(0.043, 0.047, 0.063);
/// Ink for a dark accent.
pub const INK_LIGHT: Color = Color::rgb(0.95, 0.95, 0.98);

/// The chrome accent: a colour the window draws as an underline, a chip's
/// edge or a selection's edge, and the ink that reads ON it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accent {
    pub base: Color,
    pub ink: Color,
}

/// The accent when a class chrome knows no class yet.
pub const NEUTRAL: Accent = Accent {
    base: Color::rgb(0.416, 0.718, 1.0),
    ink: INK_LIGHT,
};

/// The class colour moved, if it must be, until ink on it clears [`AA_CONTRAST`].
///
/// Shaman blue (`0x0070DD`) is the case that forces this: at luminance 0.168
/// it manages 4.33:1 with near-white ink and 4.06:1 with near-black — its best
/// with EITHER ink is under AA, so no choice of ink fixes it and the colour
/// itself has to move. It moves along its own hue (a plain darken/lighten, no
/// hue rotation), by the smallest step that clears the bar, so the accent
/// still reads as that class's colour.
///
/// This is the CHROME colour only. A meter row's bar keeps `Class::rgb`
/// untouched: the bar is data — it is how a player is identified at a glance,
/// and it must match what every other meter and the game itself draw — while
/// the chrome is decoration that has to carry text. Two different jobs, two
/// different colours, deliberately.
pub fn chrome_base(class: Class) -> Color {
    let raw = Color::of_class(class);
    let light = raw.luminance() > LIGHT_THRESHOLD;
    let ink = if light { INK_DARK } else { INK_LIGHT };
    let mut base = raw;
    for _ in 0..50 {
        if contrast(ink, base) >= AA_CONTRAST {
            break;
        }
        base = if light {
            base.lighten(0.01)
        } else {
            base.darken(0.01)
        };
    }
    base
}

/// The accent for a class chrome; `None` yields [`NEUTRAL`].
pub fn class_accent(class: Option<Class>) -> Accent {
    let Some(class) = class else { return NEUTRAL };
    let base = chrome_base(class);
    let light = base.luminance() > LIGHT_THRESHOLD;
    Accent {
        base,
        ink: if light { INK_DARK } else { INK_LIGHT },
    }
}

/// The one wash an accent may lay on a surface: a pressed chip's, the
/// prototype's `color-mix(accent 9%)`.
pub fn accent_wash(accent: Accent) -> Color {
    accent.base.alpha(0.09)
}

/// A class colour as TEXT on `panel`: `Class::rgb` lifted toward white, 2 %
/// at a time, until it clears `bar` — [`AA_CONTRAST`] for any name, the
/// prototype's `textOn`; [`YOU_CONTRAST`] for the owner's (`--you-text`).
pub fn class_text_on(class: Class, panel: Color, bar: f32) -> Color {
    let raw = Color::of_class(class);
    // 35 steps of 2 % is 70 %, the prototype's ceiling; no class needs it.
    (0..=35)
        .map(|step| raw.lighten(step as f32 * 0.02))
        .find(|c| contrast(*c, panel) >= bar)
        .unwrap_or_else(|| raw.lighten(0.7))
}

/// The game's spell-school colours (its own UI palette, softened a touch
/// for bar duty): data, like `Class::rgb`, never themed. A multi-school mask
/// (Shadowflame = Shadow|Fire) blends its components, as the game names
/// blends.
const SCHOOL_COLORS: [(u32, Color); 7] = [
    (0x01, Color::rgb(0.90, 0.87, 0.52)), // Physical
    (0x02, Color::rgb(1.00, 0.90, 0.55)), // Holy
    (0x04, Color::rgb(1.00, 0.55, 0.25)), // Fire
    (0x08, Color::rgb(0.40, 0.87, 0.40)), // Nature
    (0x10, Color::rgb(0.55, 0.87, 1.00)), // Frost
    (0x20, Color::rgb(0.58, 0.47, 0.85)), // Shadow
    (0x40, Color::rgb(1.00, 0.55, 1.00)), // Arcane
];

/// The colour for a school bitmask: a component colour, or the average of
/// a combination's components. `None` for 0 or a mask of only unknown bits.
pub fn school_color(mask: u32) -> Option<Color> {
    let mut acc = (0.0, 0.0, 0.0, 0u32);
    for (bit, c) in SCHOOL_COLORS {
        if mask & bit != 0 {
            acc = (acc.0 + c.r, acc.1 + c.g, acc.2 + c.b, acc.3 + 1);
        }
    }
    (acc.3 > 0).then(|| {
        let n = acc.3 as f32;
        Color::rgb(acc.0 / n, acc.1 / n, acc.2 / n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::CLASSES;

    #[test]
    fn hex_reads_the_floats_from_rgb8_does() {
        let c = Color::hex(0x0A0E18);
        assert_eq!(
            (c.r, c.g, c.b, c.a),
            (10.0 / 255.0, 14.0 / 255.0, 24.0 / 255.0, 1.0)
        );
        assert_eq!(Color::hex(0xF2C14B).alpha(0.3).a, 0.3);
    }

    /// The design doc's ink rule as a property: ink on every class's chrome
    /// accent clears AA, and only Shaman's colour had to move.
    #[test]
    fn every_class_clears_aa_on_its_accent() {
        for class in CLASSES {
            let a = class_accent(Some(class));
            let c = contrast(a.ink, a.base);
            assert!(c >= AA_CONTRAST, "{class:?}: {c:.2}:1");
            let moved = a.base != Color::of_class(class);
            assert_eq!(moved, class == Class::Shaman, "{class:?} moved: {moved}");
        }
        assert_eq!(class_accent(None), NEUTRAL);
    }

    #[test]
    fn a_class_name_reads_on_its_panel() {
        let panel = Color::hex(0x10162A);
        for class in CLASSES {
            assert!(contrast(class_text_on(class, panel, AA_CONTRAST), panel) >= AA_CONTRAST);
            assert!(contrast(class_text_on(class, panel, YOU_CONTRAST), panel) >= YOU_CONTRAST);
        }
    }

    #[test]
    fn a_wash_over_a_ground_is_their_blend() {
        let ground = Color::rgb(0.0, 0.0, 0.0);
        let half = Color::rgba(1.0, 1.0, 1.0, 0.5).over(ground);
        assert_eq!((half.r, half.a), (0.5, 1.0));
    }
}
