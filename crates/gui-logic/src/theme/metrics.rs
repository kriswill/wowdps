//! The window's measures as data (spec §6.1): its type scale and row
//! pitches, carried by every theme definition so no surface names a size,
//! the two widths its layout changes at, and the alphas its owner marks and
//! tab icons are drawn at. The values are the prototype's Tokens
//! (`docs/design/window-redesign.html`); the overlay names none of them (its
//! sizes are literals it multiplies by its own zoom).

/// The window's type scale, in logical pixels: the prototype's Tokens type
/// specimens — encounter titles Marcellus 27 (22 in a narrow window), stat
/// values 16 at 500, rows 15 with their numbers at 14.5, labels 13.5 in
/// gold-dim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// An encounter title, in the title face (`.ftitle h2`).
    pub encounter: f32,
    /// The same at 820 px and under.
    pub encounter_narrow: f32,
    /// A screen title (`.empty h3`, `.sheet h3`).
    pub title: f32,
    /// A value on the fight header's stat line (weight 500).
    pub stat: f32,
    /// The same at 820 px and under.
    pub stat_narrow: f32,
    /// A row's name (`.nm`).
    pub name: f32,
    /// The top bar's places (`.place`, weight 500).
    pub place: f32,
    /// Body text (`.pull`, a menu row).
    pub body: f32,
    /// Every numeric cell (`.num`).
    pub num: f32,
    /// A view tab (`.vtab`, weight 500).
    pub tab: f32,
    /// Captions, a roster's rank.
    pub small: f32,
    /// Column heads and stat labels, in gold-dim.
    pub label: f32,
    /// Tags, chips, badges.
    pub micro: f32,
    /// Eyebrow notes, key hints.
    pub tiny: f32,
    /// A keycap (`kbd`, weight 500).
    pub kbd: f32,
    /// A line of the `?` sheet.
    pub sheet_key: f32,
    /// What follows a fight's title (`.fmeta`).
    pub meta: f32,
    /// The same at 820 px and under.
    pub meta_narrow: f32,
    /// The "you" chip's words (`.youchip`).
    pub chip: f32,
    /// The owner row's "you" tag (`.youtag`, 600).
    pub you_tag: f32,
    /// The row filter's text.
    pub filter: f32,
    /// The top bar's wordmark, in the title face (`.mark`).
    pub mark: f32,
    /// The frame's own size, what a piece that sets none inherits (`.app`).
    pub frame: f32,
    /// A top-bar icon button's glyph (`.ibtn svg`).
    pub icon: f32,
    /// A view tab's line icon (`.vtab svg.i`).
    pub tab_icon: f32,
    /// The live dot (`.pulse`).
    pub dot: f32,
    /// An axis tick and the ribbon's words (`.axis span`).
    pub tick: f32,
}

/// The window's pitches, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitches {
    /// A meter row (`.trow`).
    pub row: f32,
    /// The pinned total (`.ttotal`).
    pub total: f32,
    /// The top bar (`.app{grid-template-rows:44px …}`).
    pub top_bar: f32,
    /// A top-bar place (`.place`, 42 and its 2 px underline).
    pub place: f32,
    /// A view tab (`.vtab`).
    pub tab: f32,
    /// A meter row in the compact density.
    pub compact_row: f32,
    /// An icon button's square target (`.ibtn`).
    pub icon_button: f32,
    /// The scrollbar's lane at a list's right edge.
    pub scroll_lane: f32,
    /// A menu's width (`.menu{min-width:250px}` and ten more).
    pub menu_w: f32,
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
