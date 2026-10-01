//! The inspector's lists (the prototype's `.ilist`): a player's abilities
//! or targets, an enemy's attackers, a recap's, a comparison's pair. Each
//! row is the ability's icon and its name — a pet's name after it,
//! dimmed ("Fel Firebolt Wild Imp") — then the figures, over a 2 px bar
//! in the player's class colour (`.ibar`). The columns are `table`'s, on
//! the inspector's own grids, so a heading sits over its figures and
//! sorts them the way the meter's do.
//!
//! Window-only.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::widget::image::Handle;
use iced::widget::{Space, column, container, image, mouse_area, row, stack, text};
use iced::{Color, Element, Length, Theme};

use wowdps_model::{Pane, Row, View};

use crate::compare;
use crate::ellipsis::ellipsis;
use crate::table::{self, Col, Grid};
use crate::theme::{self, size};
use crate::window::{Message, RowHover};

pub(crate) use wowdps_gui_logic::table::split_pet;

/// A row's pitch and its sides (`.irow{height:29px;padding:0 16px}`), a
/// comparison list's tighter sides (`.cmp2 .irow{padding:0 10px}`).
const ROW_H: f32 = 29.0;
pub(crate) const SIDE: f32 = 16.0;
pub(crate) const SIDE_PAIR: f32 = 10.0;
/// The ability's square (`.sq{17px}`), a person's disc (`.disc{20px}`),
/// and the gap after either (`.an{gap:7px}`).
const ICON: f32 = 17.0;
const DISC: f32 = 20.0;
const ICON_GAP: f32 = 7.0;
/// The lettered square's letter (`.sq{font-size:10px;font-weight:700;
/// color:rgba(0,0,0,.6);border-radius:3px}`).
const LETTER_PX: f32 = 10.0;
const LETTER_INK: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);
const SQ_RADIUS: f32 = 3.0;
/// The name (`.irow{font-size:14px}`) and a pet's after it (`small{13px}`,
/// one space's width after the name).
const NAME_PX: f32 = 14.0;
const PET_PX: f32 = 13.0;
const PET_GAP: f32 = 4.0;
/// R26: the tree's fold caret (a 10 px chevron in a 12 px slot, 5 px
/// before the icon) and how far each depth steps in.
const CARET: f32 = 10.0;
const CARET_SLOT: f32 = 12.0;
const CARET_GAP: f32 = 5.0;
const INDENT: f32 = 14.0;
/// The owner's tag after their name (`.youtag`), and the gap before it.
const YOU_GAP: f32 = 6.0;
/// The bar under a row (`.ibar{height:2px;bottom:2px;opacity:.55}`).
const BAR_H: f32 = 2.0;
const BAR_ALPHA: f32 = 0.55;
/// The keys' row wears the accent down its left edge, so a reader can see
/// the keys are in THIS list and not the meter's — painted over the row
/// (`box-shadow:inset 2px 0 0 var(--accent)`, the prototype's current
/// pull), never beside it, so the keyed row's content stands where every
/// other row's does.
const KEYED_EDGE: f32 = 2.0;
/// The quiet line under the heading (a comparison side's mitigation
/// record) and the "Nothing yet" of an empty list, above and below.
const NOTE_PAD_Y: f32 = 2.0;
const EMPTY_PAD_Y: f32 = 6.0;
/// The heading line (`.ihrow{padding:6px 16px 4px}`), less the 8 px
/// `table::heads` sets itself.
const HEAD_PAD: iced::Padding = iced::Padding {
    top: 6.0,
    right: 0.0,
    bottom: 4.0,
    left: 0.0,
};
/// The list's own ends (`.ilist{padding:4px 0 14px}`).
const LIST_PAD: iced::Padding = iced::Padding {
    top: 4.0,
    right: 0.0,
    bottom: 14.0,
    left: 0.0,
};
/// The stand-in squares' hues, for an ability the icon cache cannot draw
/// (the prototype's `HUES`): one per name, the same one every time.
const HUES: [Color; 7] = [
    Color::from_rgb8(0xC9, 0x84, 0x4A),
    Color::from_rgb8(0x8F, 0x7B, 0xD6),
    Color::from_rgb8(0x5F, 0xA7, 0xD6),
    Color::from_rgb8(0x6F, 0xBF, 0x73),
    Color::from_rgb8(0xD6, 0xC3, 0x5F),
    Color::from_rgb8(0xB9, 0x8E, 0x6B),
    Color::from_rgb8(0xD0, 0x7A, 0xB5),
];
/// A foe's disc (`.disc.foe{--c:#8E2C2C}`): a lit sphere of that red.
const FOE: Color = Color::from_rgb8(0x8E, 0x2C, 0x2C);

/// What leads a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lead {
    /// An ability: its icon, or a lettered square.
    Spell,
    /// A person or a creature: their class disc, or the foe's.
    Person,
}

/// What a row's bar is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Bar {
    /// One colour for the list: the player's class.
    Of(Color),
    /// Each row its own: a player's class, else the hostile red.
    Own,
}

/// What a click on a row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    Nothing,
    /// Descend into the ability (`Message::SpellRow`).
    Spell,
    /// Descend into the attacker's abilities on the enemy
    /// (`Message::AttackerRow`).
    Attacker,
    /// Drill both sides of a comparison into the ability
    /// (`Message::CompareSpell`).
    Pair,
}

/// Which list, for its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A player's abilities (or what hit them): the throughput columns.
    Abilities,
    /// Targets, attackers: amount, share and hits.
    Targets,
    /// One side of a comparison: amount and share.
    Pair,
}

impl Kind {
    /// The columns this list shows in `view`, and the grid they stand on:
    /// the prototype's `.t-ab` (Hits and Avg giving way in an inspector
    /// 440 px or narrower — `narrow`), `.t-tg` and `.cmp2`. A count has
    /// no rate, crit or average; what hit a player has no crit of theirs.
    pub(crate) fn columns(self, view: View, narrow: bool) -> (Vec<Col>, Grid) {
        let counted = !view.is_rate();
        match self {
            Kind::Abilities => {
                let cols = match (counted, view, narrow) {
                    (true, _, _) => vec![Col::Amount, Col::Pct],
                    (_, View::Taken, true) => vec![Col::Amount, Col::Pct],
                    (_, View::Taken, false) => vec![Col::Amount, Col::Pct, Col::Hits, Col::Avg],
                    (_, _, true) => vec![Col::Amount, Col::Pct, Col::Crit],
                    (_, _, false) => {
                        vec![Col::Amount, Col::Pct, Col::Hits, Col::Crit, Col::Avg]
                    }
                };
                (cols, Grid::Abilities { narrow })
            }
            Kind::Targets if counted => (vec![Col::Amount, Col::Pct], Grid::Targets),
            Kind::Targets => (vec![Col::Amount, Col::Pct, Col::Hits], Grid::Targets),
            Kind::Pair => (vec![Col::Amount, Col::Pct], Grid::Pair),
        }
    }
}

/// One list, owned.
#[derive(Debug, Clone)]
pub(crate) struct List {
    /// The rows as the breakdown sent them (realms already off), each
    /// keeping the index a click sends back.
    pub rows: Vec<Row>,
    pub kind: Kind,
    /// The view the columns and headings word themselves for.
    pub view: View,
    /// A line under the heading (a comparison side's mitigation record).
    pub note: Option<String>,
    /// Over the names: "Ability", "Target", a comparison side's name.
    pub head: String,
    /// The heading over the names in a person's colour (a comparison
    /// side), else gold-dim.
    pub head_ink: Option<Color>,
    pub lead: Lead,
    pub bar: Bar,
    /// The lit row: the keys' (see `keyed`), or a comparison's ability.
    pub selected: Option<usize>,
    /// The keys are in this list: its lit row wears the accent down its
    /// edge and the id the window scrolls into sight
    /// ([`super::keyed_row_id`]).
    pub keyed: bool,
    /// The accent the keyed row's edge is drawn in.
    pub accent: Color,
    /// The pointer's row.
    pub hover: Option<usize>,
    /// The owner's row, by index into `rows`: it wears the "you" tag.
    pub you: Option<usize>,
    /// Which drill pane the list is, for the pointer's echo.
    pub pane: Pane,
    /// A comparison list's hovered ability, by key: both sides light it.
    pub pair_hover: Option<String>,
    pub sort: Option<(Col, bool)>,
    pub on_sort: Option<fn(Col) -> Message>,
    pub press: Press,
    /// The sides' inset: [`SIDE`], or [`SIDE_PAIR`].
    pub side: f32,
    /// R26: the ability tree's lines, drawn in place of `rows` (a Damage
    /// or Healing player's abilities). `selected` and `hover` then name
    /// positions among these lines, not rows.
    pub tree: Option<Vec<super::tree::Line>>,
    /// R26 (step 2): with the graph stacked, the hue each band wears, by
    /// the key it is stacked by (an entry's, or a target's name): the
    /// bars under its lines take it, solid, so the list is the legend.
    pub hues: HashMap<String, Color>,
}

/// The lettered square's hue for `name`: a stable hash of it into
/// [`HUES`].
fn hue(name: &str) -> Color {
    let h = name
        .chars()
        .fold(0_u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let [first, ..] = HUES;
    HUES.get(h as usize % HUES.len()).copied().unwrap_or(first)
}

impl List {
    /// The list: its heading line, then its rows in the drawn order —
    /// `narrow` in an inspector 440 px or narrower.
    pub(crate) fn view(&self, narrow: bool) -> Element<'static, Message> {
        let (cols, grid) = self.kind.columns(self.view, narrow);
        let lead = row![
            text(self.head.clone())
                .size(grid.head_px())
                .color(self.head_ink.unwrap_or(theme::GOLD_DIM))
                .font(if self.head_ink.is_some() {
                    theme::UI_SEMIBOLD
                } else {
                    theme::UI
                })
                .wrapping(text::Wrapping::None)
        ];
        let heads = container(table::heads(
            &cols,
            grid,
            self.view,
            self.sort,
            self.on_sort,
            lead,
        ))
        .padding(iced::Padding {
            left: self.side - table::HEADS_INSET,
            right: self.side - table::HEADS_INSET,
            ..HEAD_PAD
        });
        let max = self.rows.iter().map(|r| r.amount).max().unwrap_or(1).max(1);
        let drawn = table::sorted(self.rows.iter().cloned().enumerate().collect(), self.sort);
        if let Some(lines) = &self.tree {
            // A group's sum can pass any one row: the bars scale by the
            // largest top-level line.
            let max = lines
                .iter()
                .filter(|l| l.depth == 0)
                .map(|l| l.row.amount)
                .max()
                .unwrap_or(1)
                .max(1);
            let mut list = column![heads];
            if lines.is_empty() {
                list = list.push(
                    container(text("Nothing yet").size(size::MICRO).color(theme::INK_2))
                        .padding([EMPTY_PAD_Y, self.side]),
                );
            }
            for (at, line) in lines.iter().enumerate() {
                list = list.push(self.tree_line(at, line, max, &cols, grid));
            }
            return container(list).padding(LIST_PAD).width(Length::Fill).into();
        }
        let mut list = column![heads];
        if let Some(note) = &self.note {
            list = list.push(
                container(text(note.clone()).size(size::TINY).color(theme::INK_2))
                    .padding([NOTE_PAD_Y, self.side]),
            );
        }
        if drawn.is_empty() {
            list = list.push(
                container(text("Nothing yet").size(size::MICRO).color(theme::INK_2))
                    .padding([EMPTY_PAD_Y, self.side]),
            );
        }
        for (i, r) in drawn {
            list = list.push(self.row(i, &r, max, &cols, grid));
        }
        container(list).padding(LIST_PAD).width(Length::Fill).into()
    }

    fn row(
        &self,
        i: usize,
        r: &Row,
        max: u64,
        cols: &[Col],
        grid: Grid,
    ) -> Element<'static, Message> {
        let lead: Element<'static, Message> = match self.lead {
            Lead::Spell => spell_lead(r),
            Lead::Person => person_lead(r, self.you == Some(i)),
        };
        let press = match self.press {
            Press::Nothing => None,
            Press::Spell => Some(Message::SpellRow(i)),
            Press::Attacker => Some(Message::AttackerRow(i)),
            Press::Pair => Some(Message::CompareSpell((r.key.clone(), r.label.clone()))),
        };
        let hue = self.hues.get(&r.key).copied();
        self.line(i, r, lead, press, hue, max, cols, grid)
    }

    /// R26: one line of the ability tree — indented by its depth, a fold's
    /// caret before the icon (an empty slot of the same width where there
    /// is none, so the icons of one depth align), its name and the words
    /// after it. A group's line folds on a press anywhere; a row's opens
    /// its ability (its caret alone folds it); a part's opens its row's.
    fn tree_line(
        &self,
        at: usize,
        l: &super::tree::Line,
        max: u64,
        cols: &[Col],
        grid: Grid,
    ) -> Element<'static, Message> {
        let caret: Element<'static, Message> = match (l.fold, &l.fold_key) {
            (Some(open), Some(key)) => mouse_area(
                container(crate::line_icons::line_icon::<Message>(
                    if open {
                        crate::line_icons::LineIcon::ChevronDown
                    } else {
                        crate::line_icons::LineIcon::ChevronRight
                    },
                    CARET,
                    theme::INK_2,
                ))
                .center(Length::Fixed(CARET_SLOT)),
            )
            .interaction(iced::mouse::Interaction::Pointer)
            .on_press(Message::TreeFold(key.clone()))
            .into(),
            _ => Space::new()
                .width(Length::Fixed(CARET_SLOT))
                .height(Length::Fixed(CARET_SLOT))
                .into(),
        };
        let lead = row![
            Space::new().width(Length::Fixed(INDENT * f32::from(l.depth))),
            caret,
            spell_words(&l.name, l.tail.as_deref(), l.row.spell_id),
        ]
        .spacing(CARET_GAP)
        .align_y(iced::Alignment::Center);
        let press = match (&l.opens, &l.fold_key, self.press) {
            (None, Some(key), _) => Some(Message::TreeFold(key.clone())),
            (Some(i), _, Press::Spell) => Some(Message::SpellRow(*i)),
            _ => None,
        };
        let hue = self.hues.get(&l.entry).copied();
        self.line(at, &l.row, lead.into(), press, hue, max, cols, grid)
    }

    /// A drawn line: `lead` then the columns over the row's bar, lit when
    /// selected or under the pointer, `press` on a click. `i` is the
    /// line's place for the selection and the pointer.
    #[allow(clippy::too_many_arguments)]
    fn line(
        &self,
        i: usize,
        r: &Row,
        lead: Element<'static, Message>,
        press: Option<Message>,
        hue: Option<Color>,
        max: u64,
        cols: &[Col],
        grid: Grid,
    ) -> Element<'static, Message> {
        let line = row![
            container(lead).width(Length::Fill),
            table::cells::<Message>(cols, grid, r, 1.0, false),
        ]
        .spacing(grid.gap())
        .align_y(iced::Alignment::Center);
        let color = match (hue, self.bar) {
            (Some(h), _) => h,
            (None, Bar::Of(c)) => c,
            (None, Bar::Own) => r.class.map_or(crate::view::HOSTILE, theme::class_rgb),
        };
        // A band's hue is its legend: solid, as the band is drawn.
        let bar_alpha = if hue.is_some() { 1.0 } else { BAR_ALPHA };
        let share = (r.amount as f64 / max as f64).clamp(0.0, 1.0);
        let lit = (share * 1000.0).round() as u16;
        let bar: Element<'static, Message> = row![
            container(Space::new())
                .width(Length::FillPortion(lit.max(1)))
                .height(Length::Fixed(BAR_H))
                .style(move |_: &Theme| container::Style {
                    background: Some(
                        Color {
                            a: bar_alpha,
                            ..color
                        }
                        .into()
                    ),
                    border: iced::border::rounded(1),
                    ..container::Style::default()
                }),
            Space::new()
                .width(Length::FillPortion(1000_u16.saturating_sub(lit).max(1)))
                .height(Length::Fixed(BAR_H)),
        ]
        .into();
        let selected = self.selected == Some(i);
        let keyed = selected && self.keyed;
        let hovered = self.hover == Some(i)
            || (self.press == Press::Pair && self.pair_hover.as_deref() == Some(r.key.as_str()));
        let accent = self.accent;
        let mut body = container(
            column![
                container(line)
                    .height(Length::Fill)
                    .align_y(iced::Alignment::Center),
                bar,
                Space::new().height(Length::Fixed(BAR_H)),
            ]
            .width(Length::Fill),
        )
        .height(Length::Fixed(ROW_H))
        .padding([0.0, self.side])
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: if selected {
                Some(theme::RAISE.into())
            } else if hovered {
                Some(theme::HOVER.into())
            } else {
                None
            },
            ..container::Style::default()
        });
        if keyed {
            // The keys are here: the accent down the row's edge, over the
            // row rather than beside it, and the id the window scrolls into
            // sight as j/k walk the list.
            body = container(stack![
                body,
                container(Space::new())
                    .width(Length::Fixed(KEYED_EDGE))
                    .height(Length::Fixed(ROW_H))
                    .style(move |_: &Theme| container::Style {
                        background: Some(accent.into()),
                        ..container::Style::default()
                    }),
            ])
            .id(super::keyed_row_id());
        }
        let mut area = mouse_area(body);
        area = match self.press {
            Press::Pair => area
                .on_enter(Message::CompareSpellHover(Some(r.key.clone())))
                .on_exit(Message::CompareSpellHover(None)),
            _ => area
                .on_enter(Message::HoverRow(Some(RowHover::Drill(self.pane, i))))
                .on_exit(Message::HoverRow(None)),
        };
        if let Some(press) = press {
            area = area.on_press(press);
        }
        area.into()
    }
}

/// An ability's lead: its icon (or a square lettered with its initial),
/// then its name and a pet's after it in the faint ink — one run ending
/// in one mark (`.an .x{text-overflow:ellipsis}`): the pet gives way
/// first, and whole once too little of it would show.
fn spell_lead(r: &Row) -> Element<'static, Message> {
    let (name, pet) = split_pet(&r.label);
    spell_words(name, pet, r.spell_id)
}

/// An ability's icon, name and the words after it (a pet's, or R26's
/// trinket or part), as [`spell_lead`] draws them.
fn spell_words(name: &str, pet: Option<&str>, spell_id: u32) -> Element<'static, Message> {
    let icon: Element<'static, Message> = match crate::spell_icons::handle(spell_id) {
        Some(h) => image(h)
            .width(Length::Fixed(ICON))
            .height(Length::Fixed(ICON))
            .into(),
        None => {
            let c = hue(name);
            container(
                text(name.chars().next().map(String::from).unwrap_or_default())
                    .size(LETTER_PX)
                    .font(theme::UI_SEMIBOLD)
                    .color(LETTER_INK),
            )
            .center(Length::Fixed(ICON))
            .style(move |_: &Theme| container::Style {
                background: Some(c.into()),
                border: iced::border::rounded(SQ_RADIUS),
                ..container::Style::default()
            })
            .into()
        }
    };
    let mut words = ellipsis(name.to_string()).size(NAME_PX).color(theme::INK);
    if let Some(pet) = pet {
        words = words.tail(pet.to_string(), PET_PX, theme::INK_3_TEXT, PET_GAP);
    }
    row![icon, words]
        .spacing(ICON_GAP)
        .align_y(iced::Alignment::Center)
        .into()
}

/// A person's lead: their class disc (a creature's is the foe's) and
/// their name — the owner's with their "you" tag after it.
fn person_lead(r: &Row, you: bool) -> Element<'static, Message> {
    let disc = if r.class.is_some() {
        compare::class_icon::<Message>(r.class, r.spec, None, DISC)
    } else {
        foe_disc(DISC)
    };
    let mut line = row![disc]
        .spacing(ICON_GAP)
        .align_y(iced::Alignment::Center);
    if you {
        line = line.push(
            row![
                ellipsis(r.label.clone())
                    .size(NAME_PX)
                    .color(theme::INK)
                    .leaving(Vec::new(), YOU_GAP + crate::view::YOU_TAG_W),
                crate::view::you_tag(r.class),
            ]
            .spacing(YOU_GAP)
            .align_y(iced::Alignment::Center),
        );
    } else {
        line = line.push(ellipsis(r.label.clone()).size(NAME_PX).color(theme::INK));
    }
    line.into()
}

/// A foe's disc (`.disc.foe`), `px` across: the prototype's lit sphere —
/// a radial gradient lit from 34% 30% (the red mixed 55% with white, the
/// red itself at 58%, the red mixed 55% with black at the rim) inside a
/// 1 px inset ring of black at 40%. Window-only: the overlay's enemy rows
/// keep `compare::enemy_icon`. Rasterised once per size at twice its size,
/// so it stays round at the window's zooms.
pub(crate) fn foe_disc<M: 'static>(px: f32) -> Element<'static, M> {
    image(foe_handle(px))
        .width(Length::Fixed(px))
        .height(Length::Fixed(px))
        .into()
}

fn foe_handle(px: f32) -> Handle {
    static CACHE: OnceLock<Mutex<HashMap<u32, Handle>>> = OnceLock::new();
    let n = (px * 2.0).round().max(2.0) as u32;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut handles = cache.lock().unwrap_or_else(|e| e.into_inner());
    handles
        .entry(n)
        .or_insert_with(|| Handle::from_rgba(n, n, sphere(n, FOE)))
        .clone()
}

/// The sphere's pixels, `n` × `n` RGBA (straight alpha).
fn sphere(n: u32, c: Color) -> Vec<u8> {
    let mix = |a: Color, b: Color, t: f32| Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    };
    // `color-mix(in srgb, c 55%, #fff)` and `…#000`.
    let lit = mix(Color::WHITE, c, 0.55);
    let rim = mix(Color::BLACK, c, 0.55);
    let side = n as f32;
    let radius = side / 2.0;
    // `circle at 34% 30%` reaching the farthest corner.
    let focus = (0.34 * side, 0.30 * side);
    let reach = (0.66_f32 * side).hypot(0.70 * side);
    // The inset ring is 1 css px: two of these pixels, at twice the size.
    let ring = 2.0;
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = (px - radius).hypot(py - radius);
            let cover = (radius - d + 0.5).clamp(0.0, 1.0);
            let t = ((px - focus.0).hypot(py - focus.1) / reach).clamp(0.0, 1.0);
            let mut ink = if t < 0.58 {
                mix(lit, c, t / 0.58)
            } else {
                mix(c, rim, (t - 0.58) / 0.42)
            };
            if d > radius - ring {
                ink = mix(ink, Color::BLACK, 0.4);
            }
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend([byte(ink.r), byte(ink.g), byte(ink.b), byte(cover)]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lettered squares take their hue from the name, as the
    /// prototype's `sqc` does: from the seven, and not all one of them.
    #[test]
    fn a_name_takes_a_hue_from_the_seven() {
        let names = ["Demonbolt", "Shadow Bolt", "Hand of Gul'dan", "Implosion"];
        let hues: Vec<Color> = names.iter().map(|n| hue(n)).collect();
        assert!(hues.iter().all(|h| HUES.contains(h)));
        assert!(hues.iter().any(|h| *h != hues[0]), "{hues:?}");
    }

    /// The foe's disc is a sphere: lit toward its top left, darker at the
    /// rim, round (clear corners), and the same handle every time.
    #[test]
    fn the_foe_disc_is_a_lit_sphere() {
        let n = 40;
        let px = sphere(n, FOE);
        let at = |x: u32, y: u32| {
            let i = ((y * n + x) * 4) as usize;
            (
                u32::from(px[i]) + u32::from(px[i + 1]) + u32::from(px[i + 2]),
                px[i + 3],
            )
        };
        assert_eq!(at(0, 0).1, 0, "a round disc's corner is clear");
        let (lit, _) = at(14, 12);
        let (rim, _) = at(34, 30);
        assert!(lit > rim, "lit {lit} over rim {rim}");
        assert_eq!(at(20, 20).1, 255, "solid inside");
        assert_eq!(foe_handle(20.0).id(), foe_handle(20.0).id(), "cached");
    }
}
