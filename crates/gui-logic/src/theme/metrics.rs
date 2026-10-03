//! The window's measures as data (spec §6.1): its type scale and row
//! pitches, carried by every theme definition so no surface names a size,
//! the two widths its layout changes at, and the alphas its owner marks and
//! tab icons are drawn at. The values are the prototype's Tokens
//! (`docs/design/window-redesign.html`); the overlay names none of them (its
//! sizes are literals it multiplies by its own zoom).

use super::tokens::tokens;

tokens! {
/// The window's type scale, in logical pixels: the prototype's Tokens type
/// specimens — encounter titles Marcellus 27 (22 in a narrow window), stat
/// values 16 at 500, rows 15 with their numbers at 14.5, labels 13.5 in
/// the label ink.
pub struct Sizes: f32 {
    /// An encounter title, in the title face (`.ftitle h2`).
    encounter,
    /// The same at 820 px and under.
    encounter_narrow,
    /// Home's heading, "You, this week", in the title face.
    home_title,
    /// The place on Home's night card, in the title face.
    home_place,
    /// A screen title (`.empty h3`, `.sheet h3`).
    title,
    /// A value on the fight header's stat line (weight 500).
    stat,
    /// The same at 820 px and under.
    stat_narrow,
    /// A row's name (`.nm`).
    name,
    /// The top bar's places (`.place`, weight 500).
    place,
    /// Body text (`.pull`, a menu row).
    body,
    /// Every numeric cell (`.num`).
    num,
    /// A view tab (`.vtab`, weight 500).
    tab,
    /// Captions, a roster's rank.
    small,
    /// Column heads and stat labels, in the label ink.
    label,
    /// Tags, chips, badges.
    micro,
    /// Eyebrow notes, key hints.
    tiny,
    /// A keycap (`kbd`, weight 500).
    kbd,
    /// A line of the `?` sheet.
    sheet_key,
    /// What follows a fight's title (`.fmeta`).
    meta,
    /// The same at 820 px and under.
    meta_narrow,
    /// The "you" chip's words (`.youchip`).
    chip,
    /// The owner row's "you" tag (`.youtag`, 600).
    you_tag,
    /// The row filter's text.
    filter,
    /// The top bar's wordmark, in the title face (`.mark`).
    mark,
    /// The frame's own size, what a piece that sets none inherits (`.app`).
    frame,
    /// A top-bar icon button's glyph (`.ibtn svg`).
    icon,
    /// A view tab's line icon (`.vtab svg.i`).
    tab_icon,
    /// The live dot (`.pulse`).
    dot,
    /// An axis tick and the ribbon's words (`.axis span`).
    tick,
}
}

tokens! {
/// The window's pitches, in logical pixels.
pub struct Pitches: f32 {
    /// A meter row (`.trow`).
    row,
    /// The pinned total (`.ttotal`).
    total,
    /// The top bar (`.app{grid-template-rows:44px …}`).
    top_bar,
    /// A top-bar place (`.place`, 42 and its 2 px underline).
    place,
    /// A view tab (`.vtab`).
    tab,
    /// A meter row in the compact density.
    compact_row,
    /// An icon button's square target (`.ibtn`).
    icon_button,
    /// The scrollbar's lane at a list's right edge.
    scroll_lane,
    /// A menu's width (`.menu{min-width:250px}` and ten more).
    menu_w,
}
}

impl Pitches {
    /// A meter row's pitch at `density`: the prototype's `.trow`, or a
    /// tighter one.
    pub fn row_of(&self, density: super::Density) -> f32 {
        match density {
            super::Density::Comfortable => self.row,
            super::Density::Compact => self.compact_row,
        }
    }

    /// The inset a dense matrix keeps at `density`.
    pub fn pad_of(&self, density: super::Density) -> f32 {
        match density {
            super::Density::Comfortable => 10.0,
            super::Density::Compact => 6.0,
        }
    }
}

/// The prototype's type scale: what every built-in theme draws at.
pub const SIZES: Sizes = Sizes {
    encounter: 27.0,
    encounter_narrow: 22.0,
    home_title: 28.0,
    home_place: 22.0,
    title: 17.0,
    stat: 16.0,
    stat_narrow: 15.0,
    name: 15.0,
    place: 15.0,
    body: 14.5,
    num: 14.5,
    tab: 14.5,
    small: 13.5,
    label: 13.5,
    micro: 13.0,
    tiny: 12.0,
    kbd: 11.5,
    sheet_key: 14.0,
    meta: 15.0,
    meta_narrow: 14.0,
    chip: 14.0,
    you_tag: 11.5,
    filter: 14.0,
    mark: 18.0,
    frame: 14.0,
    icon: 16.0,
    tab_icon: 15.0,
    dot: 8.0,
    tick: 11.5,
};

/// The prototype's pitches: what every built-in theme lays rows out at.
pub const PITCHES: Pitches = Pitches {
    row: 32.0,
    total: 33.0,
    top_bar: 44.0,
    place: 42.0,
    tab: 37.0,
    compact_row: 26.0,
    icon_button: 30.0,
    scroll_lane: 10.0,
    menu_w: 260.0,
};

/// The WINDOW widths the layout changes at: the prototype's `@container
/// app (max-width: 820px)`, at and under which it lays out narrow, and
/// `(max-width: 1180px)`, a tile, at and under which the rail is a drawer.
/// Both inclusive. A layout, not a look: no theme moves them.
pub const NARROW_WINDOW: f32 = 820.0;
pub const TILE_WINDOW: f32 = 1180.0;

/// The view tab icons' opacity (`.vtab svg.i{opacity:.85}`).
pub const TAB_ICON_ALPHA: f32 = 0.85;

/// The owner's marks in their class colour (`.youchip`, `.youtag`): the
/// chip's wash (13 %, 20 % under the pointer) and its edge (40 %), and the
/// tag's edge (55 %) — `color-mix(in srgb, var(--you) N%, transparent)`.
pub const YOU_WASH: f32 = 0.13;
pub const YOU_WASH_HOVER: f32 = 0.20;
pub const YOU_EDGE: f32 = 0.40;
pub const YOU_TAG_EDGE: f32 = 0.55;

/// A shadow under a floating surface: its colour, offset and blur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub color: super::Color,
    pub offset: (f32, f32),
    pub blur: f32,
}

/// What lifts a menu, a card or a tooltip off the window
/// (`.menu{box-shadow:0 20px 50px rgba(0,0,0,.6)}`).
pub const SHADOW_MENU: Shadow = Shadow {
    color: super::Color::rgba(0.0, 0.0, 0.0, 0.6),
    offset: (0.0, 20.0),
    blur: 50.0,
};

/// The modal sheet's, cast deeper (`.pal`, `.sheet`).
pub const SHADOW_SHEET: Shadow = Shadow {
    color: super::Color::rgba(0.0, 0.0, 0.0, 0.65),
    offset: (0.0, 30.0),
    blur: 70.0,
};

/// A passing word's (`.toast{box-shadow:0 12px 30px rgba(0,0,0,.5)}`).
pub const SHADOW_TOAST: Shadow = Shadow {
    color: super::Color::rgba(0.0, 0.0, 0.0, 0.5),
    offset: (0.0, 12.0),
    blur: 30.0,
};

/// The shadows a theme casts under what floats: a menu or a card, the
/// modal sheet and palette, and the toast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadows {
    pub menu: Shadow,
    pub sheet: Shadow,
    pub toast: Shadow,
    /// The pull rail's drawer, cast sideways over the stage.
    pub drawer: Shadow,
}

/// The prototype's shadows.
pub const SHADOWS: Shadows = Shadows {
    menu: SHADOW_MENU,
    sheet: SHADOW_SHEET,
    toast: SHADOW_TOAST,
    // `.app.rail-open .rail{box-shadow:20px 0 50px rgba(0,0,0,.5)}`.
    drawer: Shadow {
        color: super::Color::rgba(0.0, 0.0, 0.0, 0.5),
        offset: (20.0, 0.0),
        blur: 50.0,
    },
};

tokens! {
/// A theme's corners. Every radius a surface draws is its design value
/// times `scale` (Navy's 1 draws the prototype's corners exactly; a
/// machined theme sets less), and a chip or pill — a radius that is half a
/// control's height — is at most `chip` (a large `chip` keeps it a pill; a
/// small one makes it a squared tag). A disc stays a disc whatever this says:
/// a player's crest and a dot are round by meaning.
pub struct Shape: f32 {
    /// What every corner radius is multiplied by.
    scale,
    /// The most a chip's or a pill's corner may be.
    chip,
}
}

tokens! {
/// How strongly a class colour fills its bar (0..=1 alphas): a meter row's
/// fades along its length, from `rest_from` at its start to `rest_to` at
/// its end, the selected row's from `lit_from` to `lit_to`; the overlay's
/// rows wear the resting ramp; an inspector list's bar is `list` flat.
/// The class colour itself is data and never moves — only how much of it
/// a bar shows, which a darker ground wants more of.
pub struct Bars: f32 {
    rest_from,
    rest_to,
    lit_from,
    lit_to,
    list,
}
}

/// The bars as the meter, the overlay and the inspector have always drawn
/// them.
pub const BARS: Bars = Bars {
    rest_from: 0.16,
    rest_to: 0.55,
    lit_from: 0.55,
    lit_to: 1.0,
    list: 0.55,
};

/// The prototype's corners: radii as designed, chips as pills.
pub const SHAPE: Shape = Shape {
    scale: 1.0,
    chip: 99.0,
};

impl Shape {
    /// A designed radius `r` at this shape.
    pub fn radius(&self, r: f32) -> f32 {
        r * self.scale.max(0.0)
    }

    /// A pill's radius `r` (half its height) at this shape.
    pub fn pill(&self, r: f32) -> f32 {
        r.min(self.chip.max(0.0))
    }
}

/// Is a window `width` logical pixels wide narrow (820 and under)?
pub fn is_narrow(width: f32) -> bool {
    width <= NARROW_WINDOW
}

/// Is it a tile (821–1180), the rail a drawer?
pub fn is_tile(width: f32) -> bool {
    width > NARROW_WINDOW && width <= TILE_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prototype's Tokens specimens and pitches.
    #[test]
    fn the_window_type_is_the_prototypes() {
        assert_eq!((SIZES.encounter, SIZES.encounter_narrow), (27.0, 22.0));
        assert_eq!(SIZES.stat, 16.0);
        assert_eq!((SIZES.name, SIZES.num), (15.0, 14.5));
        assert_eq!(SIZES.label, 13.5);
        assert_eq!((SIZES.place, SIZES.tab), (15.0, 14.5));
        assert_eq!(
            (PITCHES.row, PITCHES.total, PITCHES.tab, PITCHES.place),
            (32.0, 33.0, 37.0, 42.0)
        );
    }

    /// `max-width` is inclusive: 820 is narrow, 1180 a tile.
    #[test]
    fn the_breakpoints_are_inclusive() {
        assert!(is_narrow(820.0) && !is_narrow(821.0));
        assert!(is_tile(821.0) && is_tile(1180.0) && !is_tile(1181.0) && !is_tile(820.0));
    }

    #[test]
    fn densities_are_ordered() {
        use super::super::Density;
        assert!(PITCHES.row_of(Density::Compact) < PITCHES.row_of(Density::Comfortable));
        assert!(PITCHES.pad_of(Density::Compact) < PITCHES.pad_of(Density::Comfortable));
    }
}
