//! The talent viewer: a window-local screen showing a player's talent tree
//! (R14). Its logic — the dataset, the decode, the laid-out panes, the
//! edits under the game's rules, the paste store, the tooltip's lines —
//! is gui-logic's (`wowdps_gui_logic::talents`), shared with gui-new;
//! this module draws it in iced.
//!
//! The tab sits on the spec's full-width background painting, and each
//! pane is two stacked canvases: iced composites a frame's images above
//! its vector paths, so the icons and the chrome over them cannot share
//! one.

use std::rc::Rc;

use iced::widget::canvas::{self, Canvas, Path, Stroke};
use iced::widget::{Space, column, container, mouse_area, row, scrollable, text, text_input};
use iced::{Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use wowdps_gui_logic::talents::{self as logic, TILE, Tone};
use wowdps_gui_logic::theme as gl;

pub(crate) use wowdps_gui_logic::talents::{Build, Msg, Node, PaneModel, Tab, Viewer as TalentsUi};

use crate::simc;
use crate::spell_icons::IconStyle;
use crate::talent_art;
use crate::theme::{self, size};

/// Are the per-machine art caches reachable? Not while a test has put its
/// own dataset on this thread: every render exercises its "cache absent"
/// path.
fn art_available() -> bool {
    !logic::under_test()
}

type ImageHandle = iced::widget::image::Handle;

fn art_background(spec_id: u32) -> Option<(ImageHandle, u16, u16)> {
    art_available()
        .then(|| talent_art::background(spec_id))
        .flatten()
}

fn art_medallion(subtree_id: u32) -> Option<ImageHandle> {
    art_available()
        .then(|| talent_art::medallion(subtree_id))
        .flatten()
}

fn art_ring() -> Option<ImageHandle> {
    art_available().then(talent_art::ring).flatten()
}

fn class_icon(class: wowdps_model::Class) -> Option<ImageHandle> {
    art_available()
        .then(|| crate::icons::class_handle(class))
        .flatten()
}

fn spec_icon(spec_id: u32) -> Option<ImageHandle> {
    art_available()
        .then(|| crate::icons::spec_handle(spec_id))
        .flatten()
}

fn spell_icon(spell_id: u32, style: IconStyle, gray: bool) -> Option<ImageHandle> {
    art_available()
        .then(|| crate::spell_icons::styled(spell_id, style, gray))
        .flatten()
}

/// A gui-logic colour as iced's: the same four floats.
const fn c(x: gl::Color) -> Color {
    Color::from_rgba(x.r, x.g, x.b, x.a)
}

/// The talent palette: the iced GUI draws `gold`'s.
const T: gl::TalentTokens = gl::GOLD.talents;

/// The talent gold — selection frames, lit paths, the pane titles. Close
/// to the game's `ffd100` toned for the dark theme.
const GOLD: Color = c(T.gold);

/// A point in pane pixels as iced's.
fn pt((x, y): logic::Pt) -> Point {
    Point::new(x, y)
}

/// The pane's retained canvas geometry: everything that depends only on
/// the model, tessellated once per rebuild and reused across the redraws
/// iced requests on every cursor movement over a canvas. It lives in the
/// model's retained slot, so a fresh (or cloned) model starts empty and
/// re-tessellates once.
#[derive(Default)]
pub(crate) struct PaneCaches {
    under: canvas::Cache,
    over: canvas::Cache,
}

/// The pane's caches, made on first use.
fn caches(model: &PaneModel) -> Option<&PaneCaches> {
    model.retained.get::<PaneCaches>()
}

// ---- rendering -------------------------------------------------------------

pub(crate) fn screen(ui: &TalentsUi) -> Element<'_, Msg> {
    let mut top = row![text("talents").size(16)]
        .spacing(8)
        .align_y(iced::Alignment::Center);
    if let Some(player) = &ui.player {
        top = top.push(
            text(player.split('-').next().unwrap_or(player).to_string())
                .size(14)
                .color(theme::INK)
                .font(theme::UI_SEMIBOLD),
        );
    }
    if let Some(b) = &ui.build {
        top = top.push(
            text(format!("{} — {}", b.class_name, b.spec_name))
                .size(12)
                .color(theme::INK_2),
        );
    }
    top = top
        .push(Space::new().width(Length::Fill))
        .push(mouse_area(text("✕").size(14).color(theme::INK_2)).on_press(Msg::Close));

    let input_line = row![
        text_input("paste an in-game talent string…", &ui.input)
            .on_input(Msg::Input)
            .on_submit(Msg::Submit)
            .size(13)
            .font(theme::UI),
        chip("paste simc/string", false, Msg::PasteClipboard),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let mut body = column![top, input_line].spacing(8).height(Length::Fill);

    if let Some(p) = &ui.profile {
        body = body.push(identity_line(p));
        if p.loadouts.len() > 1 {
            let mut chips = row![text("loadouts").size(size::TINY).color(theme::INK_2)]
                .spacing(6)
                .align_y(iced::Alignment::Center);
            for (i, l) in p.loadouts.iter().enumerate() {
                chips = chips.push(chip(
                    if l.active { "active" } else { &l.name },
                    i == ui.loadout_sel,
                    Msg::SelectLoadout(i),
                ));
            }
            body = body.push(
                scrollable(chips).direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                )),
            );
        }
    }
    // v19: logged gear opens the inventory tab too, without any paste.
    if ui.has_inventory() {
        body = body.push(
            row![
                chip("talents", ui.tab == Tab::Talents, Msg::SetTab(Tab::Talents)),
                chip(
                    "inventory",
                    ui.tab == Tab::Inventory,
                    Msg::SetTab(Tab::Inventory)
                ),
            ]
            .spacing(6),
        );
    }

    if let Some(e) = &ui.error {
        body = body.push(text(e.clone()).size(12).color(theme::BAD));
    }

    // While the logged build is showing, its gear is the inventory (the
    // fight's actual equipment); a simc profile's inventory returns with it.
    let content: Element<'_, Msg> = match (ui.tab, &ui.profile, &ui.logged_gear) {
        (Tab::Inventory, _, Some(gear)) if ui.logged => logged_inventory(gear),
        (Tab::Inventory, Some(p), _) => inventory(p),
        _ => talents_tab(ui),
    };
    body.push(content)
        .push(
            text("click picks (+1 rank) · right-click refunds · octagons open their option picker · esc closes · tab flips inventory")
                .size(size::TINY)
                .color(theme::INK_2),
        )
        .into()
}

fn talents_tab(ui: &TalentsUi) -> Element<'_, Msg> {
    let Some(b) = &ui.build else {
        return container(
            text("paste a talent string or a SimulationCraft export to see a build")
                .size(13)
                .color(theme::INK_2),
        )
        .height(Length::Fill)
        .into();
    };
    let mut col = column![].spacing(6).height(Length::Fill);
    for w in &b.warnings {
        col = col.push(text(format!("⚠ {w}")).size(size::TINY).color(theme::BAD));
    }
    // A fixed-height provenance line (node details live in the hover
    // tooltip on the canvas — a strip that changed height with its content
    // used to shift the whole tree while hovering). "edited" marks a build
    // that no longer matches the decoded string.
    let mut provenance = row![
        text(format!("dataset build {}", b.dataset_build))
            .size(size::TINY)
            .color(theme::INK_2),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);
    if ui.logged && !ui.edited {
        // v19: this is the build the player actually ran (COMBATANT_INFO).
        provenance = provenance.push(text("from combat log").size(size::TINY).color(theme::GOOD));
    }
    if ui.edited {
        provenance = provenance.push(text("edited").size(size::TINY).color(theme::GOLD_DIM));
    }
    provenance = provenance.push(chip("copy string", false, Msg::CopyString));
    col = col.push(provenance);

    // Class pane | hero column | spec pane — the game's own arrangement,
    // centered over ONE full-width backdrop: the spec's whole background
    // painting (class art fading in from the left edge, spec art from the
    // right) spans the client, washed dark so the trees read on top.
    // A `Fill` width inside a horizontally-scrollable axis collapses to
    // the content's width, so centering needs the real viewport size:
    // responsive() centers when the trees fit and falls back to a
    // two-axis scroll when they don't.
    let hero_w = b
        .hero_pane
        .as_ref()
        .map_or(0.0f32, |p| p.w + 16.0)
        .max(if b.hero.is_some() { 168.0 } else { 0.0 });
    let content_w = b.class_pane.w + b.spec_pane.w + hero_w + 2.0 * 24.0 + 32.0;
    let picker = ui.picker;
    let trees = iced::widget::responsive(move |size| {
        let fits = size.width >= content_w;
        let panes = container(tree_row(b, picker)).padding(iced::Padding {
            top: 8.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        });
        if fits {
            scrollable(
                container(panes)
                    .width(Length::Fill)
                    .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            scrollable(panes)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    });

    // The tab-wide tooltip layer, above everything: a per-pane tooltip
    // would clip at its own canvas edge (the hero pane's did). With the
    // picker open, only its own option tiles carry tooltips.
    let tip = Canvas::new(TipOverlay {
        anchor: ui.hover_at,
        node: ui.hovered(),
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let area: Element<'_, Msg> = match art_background(b.spec_id) {
        // The painting is drawn by a canvas, not an image widget: the
        // widget's ContentFit::Cover paints its overflow outside its own
        // bounds, while canvas geometry clips. The dark veil is a plain
        // container ABOVE it (vector inside the same canvas would
        // composite under the image).
        Some((bg, w, h)) => iced::widget::stack![
            Canvas::new(Backdrop { bg, w, h })
                .width(Length::Fill)
                .height(Length::Fill),
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_: &Theme| {
                container::Style {
                    background: Some(c(T.veil).into()),
                    ..container::Style::default()
                }
            }),
            trees,
            tip,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        None => iced::widget::stack![trees, tip]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    };
    col.push(area).into()
}

/// The full-width backdrop: the spec's background painting, cover-fit and
/// clipped by the canvas.
struct Backdrop {
    bg: iced::widget::image::Handle,
    w: u16,
    h: u16,
}

impl canvas::Program<Msg> for Backdrop {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let (iw, ih) = (f32::from(self.w).max(1.0), f32::from(self.h).max(1.0));
        let scale = (w / iw).max(h / ih);
        let (dw, dh) = (iw * scale, ih * scale);
        frame.draw_image(
            Rectangle {
                x: (w - dw) / 2.0,
                y: (h - dh) / 2.0,
                width: dw,
                height: dh,
            },
            canvas::Image::new(self.bg.clone()),
        );
        vec![frame.into_geometry()]
    }
}

/// The model's `Class` for a spec id, for the crest lookup.
fn model_class(spec_id: u32) -> Option<wowdps_model::Class> {
    wowdps_model::Spec::from_id(spec_id).map(|s| s.class())
}

/// The three panes side by side: class, the hero column, spec.
fn tree_row(b: &Build, picker: Option<u64>) -> Element<'_, Msg> {
    let class_icon = model_class(b.spec_id).and_then(class_icon);
    let spec_icon = spec_icon(b.spec_id);
    let mut panes = row![].spacing(24).align_y(iced::Alignment::Start);
    panes = panes.push(
        column![
            pane_header(class_icon, &b.class_name, &b.class_pane),
            pane_canvas(Rc::clone(&b.class_pane), picker),
        ]
        .spacing(4),
    );
    if let Some((hero_id, hero_name)) = &b.hero {
        panes = panes.push(hero_column(
            *hero_id,
            hero_name,
            b.hero_pane.as_ref(),
            picker,
        ));
    }
    panes = panes.push(
        column![
            pane_header(spec_icon, &b.spec_name, &b.spec_pane),
            pane_canvas(Rc::clone(&b.spec_pane), picker),
        ]
        .spacing(4),
    );
    panes.into()
}

/// "12/34 pts" when the cap is known, else "12 pts"; gold once full.
fn points_label(points: u64, cap: Option<u64>) -> (String, Color) {
    let (label, full) = logic::points_label(points, cap);
    (label, if full { GOLD } else { theme::INK_2 })
}

/// A pane's header bar: its round icon, its name, its points spent.
fn pane_header(
    icon: Option<iced::widget::image::Handle>,
    name: &str,
    pane: &PaneModel,
) -> Element<'static, Msg> {
    let mut line = row![].spacing(8).align_y(iced::Alignment::Center);
    if let Some(h) = icon {
        line = line.push(
            iced::widget::image(h)
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0)),
        );
    }
    let (label, color) = points_label(pane.points, pane.cap);
    line.push(text(name.to_uppercase()).size(13).color(GOLD))
        .push(Space::new().width(Length::Fill))
        .push(text(label).size(size::TINY).color(color).font(theme::UI))
        .width(Length::Fixed(pane.w.max(160.0)))
        .into()
}

/// The center column: the hero tree's medallion under the game's golden
/// ring, its name, and its mini-tree on a dark backplate.
fn hero_column(
    hero_id: u32,
    hero_name: &str,
    pane: Option<&Rc<PaneModel>>,
    picker: Option<u64>,
) -> Element<'static, Msg> {
    // Measured off the ring crop's pixels: within its 192px tile (mostly
    // drop-shadow padding) the gold circle's inner diameter is ~55% — the
    // full-bleed medallion art must shrink to sit inside it.
    const RING: f32 = 168.0;
    const MEDALLION: f32 = RING * 0.56;
    let mut col = column![].spacing(6).align_x(iced::Alignment::Center);
    if let Some(art) = art_medallion(hero_id) {
        let medallion = container(
            iced::widget::image(art)
                .width(Length::Fixed(MEDALLION))
                .height(Length::Fixed(MEDALLION)),
        )
        .width(Length::Fixed(RING))
        .height(Length::Fixed(RING))
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center);
        col = col.push(match art_ring() {
            Some(ring) => Element::from(iced::widget::stack![
                medallion,
                iced::widget::image(ring)
                    .width(Length::Fixed(RING))
                    .height(Length::Fixed(RING)),
            ]),
            None => medallion.into(),
        });
    }
    col = col.push(text(hero_name.to_uppercase()).size(14).color(GOLD));
    if let Some(pane) = pane {
        let (label, color) = points_label(pane.points, pane.cap);
        col = col.push(text(label).size(size::TINY).color(color).font(theme::UI));
        col = col.push(
            container(pane_canvas(Rc::clone(pane), picker))
                .padding(8)
                .style(|_: &Theme| container::Style {
                    background: Some(c(T.plate).into()),
                    border: Border {
                        color: c(T.plate_edge),
                        width: 1.0,
                        radius: 10.into(),
                    },
                    ..container::Style::default()
                }),
        );
    }
    col.into()
}

fn identity_line(p: &simc::Profile) -> Element<'static, Msg> {
    text(logic::identity(p))
        .size(size::TINY)
        .color(theme::INK_2)
        .into()
}

/// A small clickable pill, the loadout picker's and the tabs' unit.
fn chip(label: &str, selected: bool, msg: Msg) -> Element<'static, Msg> {
    let color = if selected { theme::INK } else { theme::INK_2 };
    mouse_area(
        container(text(label.to_string()).size(size::TINY).color(color))
            .padding([3, 8])
            .style(move |_: &Theme| container::Style {
                background: Some(c(if selected { T.chip_on } else { T.chip }).into()),
                border: Border {
                    color: c(if selected {
                        T.chip_edge_on
                    } else {
                        T.chip_edge
                    }),
                    width: 1.0,
                    radius: 8.into(),
                },
                ..container::Style::default()
            }),
    )
    .on_press(msg)
    .into()
}

// ---- the inventory tab -----------------------------------------------------

fn inventory(p: &simc::Profile) -> Element<'static, Msg> {
    let mut col = column![].spacing(4);
    let section = |t: &'static str| text(t).size(12).color(theme::GOLD_DIM);
    if !p.equipped.is_empty() {
        col = col.push(section("equipped"));
        for i in &p.equipped {
            col = col.push(item_row(i));
        }
    }
    if !p.bags.is_empty() {
        col = col.push(Space::new().height(6)).push(section("in bags"));
        for i in &p.bags {
            col = col.push(item_row(i));
        }
    }
    if !p.currencies.is_empty() {
        col = col.push(Space::new().height(6)).push(section("currencies"));
        for c in &p.currencies {
            col = col.push(
                row![
                    text(logic::currency_kind(c))
                        .size(size::TINY)
                        .color(theme::INK_2)
                        .width(Length::Fixed(70.0)),
                    text(format!("{}", c.id))
                        .size(12)
                        .font(theme::UI)
                        .width(Length::Fixed(80.0)),
                    text(format!("× {}", c.amount))
                        .size(12)
                        .font(theme::UI)
                        .color(theme::INK),
                ]
                .spacing(8),
            );
        }
    }
    scrollable(crate::view::scroll_clear(col))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

/// v19: the logged gear list. Ids only — item names live in game data no
/// client-side dataset carries, so `item {id}` is the honest label, exactly
/// like the simc tab's fallback and the currencies section.
fn logged_inventory(gear: &[wowdps_model::GearItem]) -> Element<'static, Msg> {
    let mut col = column![].spacing(4);
    col = col.push(
        text("equipped — from combat log")
            .size(12)
            .color(theme::GOLD_DIM),
    );
    for (i, g) in gear.iter().enumerate() {
        // Empty slots log as zeroed tuples; a row of zeros says nothing.
        if g.item_id == 0 {
            continue;
        }
        col = col.push(
            row![
                text(logic::gear_slot(i, gear.len()))
                    .size(size::TINY)
                    .color(theme::INK_2)
                    .font(theme::UI)
                    .width(Length::Fixed(80.0)),
                text(format!("item {}", g.item_id))
                    .size(12)
                    .font(theme::UI)
                    .width(Length::Fill),
                text(logic::extras(!g.enchants.is_empty(), g.gems.len()))
                    .size(size::TINY)
                    .color(theme::INK_2),
                text(if g.ilvl > 0 {
                    g.ilvl.to_string()
                } else {
                    String::new()
                })
                .size(12)
                .color(theme::GOOD)
                .font(theme::UI)
                .width(Length::Fixed(36.0))
                .align_x(iced::Alignment::End),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }
    scrollable(crate::view::scroll_clear(col))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn item_row(i: &simc::Item) -> Element<'static, Msg> {
    let ilvl = i.ilvl.map(|v| v.to_string()).unwrap_or_default();
    row![
        text(i.slot.clone())
            .size(size::TINY)
            .color(theme::INK_2)
            .font(theme::UI)
            .width(Length::Fixed(80.0)),
        text(logic::item_name(i)).size(12).width(Length::Fill),
        text(logic::extras(i.enchant_id.is_some(), i.gem_ids.len()))
            .size(size::TINY)
            .color(theme::INK_2),
        text(ilvl)
            .size(12)
            .color(theme::GOOD)
            .font(theme::UI)
            .width(Length::Fixed(36.0))
            .align_x(iced::Alignment::End),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

// ---- the tree canvas -------------------------------------------------------

/// One tree canvas. It draws no background of its own: the whole tab sits
/// on the spec's full-width painting (see `talents_tab`) — a canvas frame
/// composites all of its images above all of its vector paths, so the
/// painting can never live inside the canvas without burying the edges
/// and frames.
/// A pane: two stacked canvases over one model. The LOWER canvas draws the
/// edges, arrows, shaped backings and the icon art; the UPPER draws
/// everything that must read over the icons — frames, carets, the
/// white-on-black rank badges, the choice picker and the hover tooltip —
/// and owns the mouse. Two canvases because a single frame composites all
/// of its images above all of its vector paths, which would bury any
/// chrome overlapping a tile.
fn pane_canvas(model: Rc<PaneModel>, picker: Option<u64>) -> Element<'static, Msg> {
    let (w, h) = (model.w, model.h);
    iced::widget::stack![
        Canvas::new(PaneUnder {
            model: Rc::clone(&model),
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h)),
        Canvas::new(PaneOver { model, picker })
            .width(Length::Fixed(w))
            .height(Length::Fixed(h)),
    ]
    .into()
}

/// Draw `body` into `cache` when there is one, else into a fresh frame.
fn cached(
    cache: Option<&canvas::Cache>,
    renderer: &Renderer,
    size: Size,
    body: impl Fn(&mut canvas::Frame),
) -> canvas::Geometry {
    match cache {
        Some(cache) => cache.draw(renderer, size, body),
        None => {
            let mut frame = canvas::Frame::new(renderer, size);
            body(&mut frame);
            frame.into_geometry()
        }
    }
}

struct PaneUnder {
    model: Rc<PaneModel>,
}

impl canvas::Program<Msg> for PaneUnder {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // Everything here depends only on the model, so the tessellation is
        // cached on it: iced redraws a canvas on every cursor movement, and
        // without the cache each redraw re-tessellated every edge and tile.
        let under = caches(&self.model).map(|c| &c.under);
        let geometry = cached(under, renderer, bounds.size(), |frame| {
            // Edges under the tiles: a taken path is gold and carries an
            // arrowhead at its destination end, pointing into the node the
            // point flowed to (the way the game draws its paths); the rest
            // stay faint gray.
            for &(a, b) in &self.model.edges {
                let (Some(from), Some(to)) = (self.model.nodes.get(a), self.model.nodes.get(b))
                else {
                    continue;
                };
                let lit = from.selected && to.selected;
                let (p, q) = (Point::new(from.x, from.y), Point::new(to.x, to.y));
                frame.stroke(
                    &Path::line(p, q),
                    Stroke::default()
                        .with_width(if lit { 2.0 } else { 1.5 })
                        .with_color(if lit {
                            Color { a: 0.85, ..GOLD }
                        } else {
                            c(T.path)
                        }),
                );
                if lit {
                    let [tip, left, right] = logic::arrowhead((from.x, from.y), (to.x, to.y));
                    let arrow = Path::new(|b| {
                        b.move_to(pt(tip));
                        b.line_to(pt(left));
                        b.line_to(pt(right));
                        b.close();
                    });
                    frame.fill(&arrow, GOLD);
                }
            }

            for n in &self.model.nodes {
                let center = Point::new(n.x, n.y);
                let rect = Rectangle {
                    x: n.x - TILE / 2.0,
                    y: n.y - TILE / 2.0,
                    width: TILE,
                    height: TILE,
                };
                // A dark backing so the shaped icon's clipped corners read as
                // the shape even over bright background art.
                frame.fill(&shape_path(center, TILE / 2.0 + 1.5, n.shape), c(T.backing));
                // Colored art for anything the build has; untaken talents are
                // desaturated and dimmed, the way every talent UI mutes them.
                match spell_icon(n.spell_id, n.shape, !n.selected) {
                    Some(icon) => frame.draw_image(rect, canvas::Image::new(icon)),
                    None => frame.fill(
                        &shape_path(center, TILE / 2.0 - 2.0, n.shape),
                        c(T.blank.alpha(if n.selected { 0.30 } else { 0.10 })),
                    ),
                }
            }
        });
        vec![geometry]
    }
}

struct PaneOver {
    model: Rc<PaneModel>,
    /// The choice node whose picker is expanded (any pane's — only the one
    /// that actually holds the node draws it).
    picker: Option<u64>,
}

#[derive(Default)]
struct OverState {
    /// (node id, picker-option index) under the pointer.
    hover: Option<(u64, Option<u64>)>,
}

impl canvas::Program<Msg> for PaneOver {
    type State = OverState;

    fn update(
        &self,
        state: &mut OverState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        use iced::mouse::{Button, Event as Mouse};
        let iced::Event::Mouse(mouse) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds);
        match mouse {
            Mouse::CursorMoved { .. } => {
                // Picker option tiles sit over neighboring nodes: they win.
                // The tile center rides along in WINDOW coordinates so the
                // tooltip can anchor beside the icon.
                let over = pos.and_then(|p| logic::hit(&self.model, self.picker, p.x, p.y));
                let over_key = over.map(|(id, opt, _)| (id, opt));
                if over_key != state.hover {
                    let prev = state.hover;
                    state.hover = over_key;
                    // The tab-wide overlay draws the tooltip; tell it.
                    return Some(canvas::Action::publish(match (over, prev) {
                        (Some((id, opt, at)), _) => {
                            Msg::HoverSet(id, opt, bounds.x + at.0, bounds.y + at.1)
                        }
                        (None, Some((id, _))) => Msg::HoverClear(id),
                        (None, None) => return None,
                    }));
                }
                None
            }
            Mouse::ButtonPressed(Button::Left) => {
                let p = pos?;
                if let Some((node, index)) = logic::option_at(&self.model, self.picker, p.x, p.y) {
                    return Some(
                        canvas::Action::publish(Msg::PickChoice(node, index)).and_capture(),
                    );
                }
                if let Some(n) = logic::node_at(&self.model, p.x, p.y) {
                    return Some(canvas::Action::publish(Msg::NodeClick(n.id)).and_capture());
                }
                if self.picker.is_some() {
                    return Some(canvas::Action::publish(Msg::ClosePicker).and_capture());
                }
                None
            }
            Mouse::ButtonPressed(Button::Right) => {
                let p = pos?;
                let n = logic::node_at(&self.model, p.x, p.y)?;
                Some(canvas::Action::publish(Msg::NodeRightClick(n.id)).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        _state: &OverState,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(p) if logic::hit(&self.model, self.picker, p.x, p.y).is_some() => {
                iced::mouse::Interaction::Pointer
            }
            _ => iced::mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        state: &OverState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // The model-only chrome — frames, carets, rank badges — caches on
        // the model; the hover ring and the open picker change without a
        // rebuild, so they draw on a fresh frame each time.
        let over = caches(&self.model).map(|c| &c.over);
        let chrome = cached(over, renderer, bounds.size(), |frame| {
            for n in &self.model.nodes {
                let center = Point::new(n.x, n.y);
                let frame_path = shape_path(center, TILE / 2.0 + 1.5, n.shape);
                // The frame: gold = taken, teal = granted for free, green =
                // available to pick, faint gray = out of reach.
                let border = if n.granted {
                    c(T.granted)
                } else if n.selected {
                    GOLD
                } else if n.available {
                    c(T.available)
                } else {
                    c(T.locked)
                };
                frame.stroke(
                    &frame_path,
                    Stroke::default()
                        .with_width(if n.selected || n.available { 2.0 } else { 1.0 })
                        .with_color(border),
                );
                // A choice node wears the game's side carets.
                if n.choice {
                    for [tip, top, bottom] in logic::carets(n) {
                        let caret = Path::new(|b| {
                            b.move_to(pt(tip));
                            b.line_to(pt(top));
                            b.line_to(pt(bottom));
                            b.close();
                        });
                        frame.fill(
                            &caret,
                            Color {
                                a: if n.selected { 1.0 } else { 0.35 },
                                ..border
                            },
                        );
                    }
                }
                // The rank badge: white on black, overlapping the tile's lower
                // right corner so it never covers the path lines.
                let (content, [x, y, w, h]) = logic::badge(n);
                frame.fill(
                    &Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), 2.0.into()),
                    c(T.badge),
                );
                frame.fill_text(canvas::Text {
                    content,
                    position: Point::new(x + w / 2.0, y + h / 2.0),
                    color: c(T.badge_ink),
                    size: 9.0.into(),
                    font: theme::UI,
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        });

        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some((id, None)) = state.hover
            && let Some(n) = self.model.nodes.iter().find(|n| n.id == id)
        {
            frame.stroke(
                &shape_path(Point::new(n.x, n.y), TILE / 2.0 + 4.0, n.shape),
                Stroke::default()
                    .with_width(1.5)
                    .with_color(c(T.hover_ring)),
            );
        }

        // The expanded choice picker: a horizontal strip of the options
        // through the node, current pick ringed gold.
        if let Some(node) = logic::picker_node(&self.model, self.picker) {
            let spots = logic::picker_spots(&self.model, node);
            if let Some([x, y, w, h]) = logic::picker_plate(&spots, node) {
                frame.fill(
                    &Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), 8.0.into()),
                    c(T.picker),
                );
            }
            for (i, (spot, opt)) in spots.iter().zip(node.options.iter()).enumerate() {
                let spot = pt(*spot);
                let rect = Rectangle {
                    x: spot.x - TILE / 2.0,
                    y: spot.y - TILE / 2.0,
                    width: TILE,
                    height: TILE,
                };
                match spell_icon(opt.spell_id, IconStyle::Octagon, false) {
                    Some(icon) => frame.draw_image(rect, canvas::Image::new(icon)),
                    None => frame.fill(
                        &shape_path(spot, TILE / 2.0 - 2.0, IconStyle::Octagon),
                        c(T.blank.alpha(0.25)),
                    ),
                }
                // Ring outside the tile (vector composites under images,
                // so it must not overlap the icon). The node's spell_id is
                // its picked entry's, which marks the current option; the
                // hovered option flares white.
                let is_current = node.selected && node.spell_id == opt.spell_id;
                let hovered = state.hover == Some((node.id, Some(i as u64)));
                frame.stroke(
                    &shape_path(spot, TILE / 2.0 + 2.5, IconStyle::Octagon),
                    Stroke::default().with_width(2.0).with_color(if hovered {
                        c(T.hover_ring)
                    } else if is_current {
                        GOLD
                    } else {
                        c(T.option_ring)
                    }),
                );
            }
        }

        vec![chrome, frame.into_geometry()]
    }
}

/// The tab-wide tooltip layer: sits above the scrollable so the tooltip
/// can never be clipped by a pane's own canvas. The tooltip is anchored
/// beside the hovered icon (not the pointer), so the neighbors stay
/// visible while reading. It never captures events — clicks and scrolls
/// fall through to the trees below.
struct TipOverlay {
    /// The hovered node with its pane's "Requires …" class, if any.
    node: Option<(Node, String)>,
    /// The hovered tile's center in window coordinates.
    anchor: (f32, f32),
}

impl canvas::Program<Msg> for TipOverlay {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some((n, requires)) = &self.node {
            let anchor = Point::new(self.anchor.0 - bounds.x, self.anchor.1 - bounds.y);
            draw_tooltip(&mut frame, n, requires, anchor, bounds.width, bounds.height);
        }
        vec![frame.into_geometry()]
    }
}

/// A tooltip line's colour, the game's for its role.
fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Plain => c(T.tip_ink),
        Tone::Meta => c(T.tip_meta),
        Tone::Desc => c(T.tip_desc),
        Tone::Note => c(T.tip_note),
        Tone::Unreached => c(T.tip_unreached),
    }
}

/// The game-style tooltip (`logic::tooltip_lines`), in a box beside the
/// hovered icon.
fn draw_tooltip(frame: &mut canvas::Frame, n: &Node, requires: &str, cur: Point, w: f32, h: f32) {
    let tw = logic::tip_width(w);
    let lines = logic::tooltip_lines(n, requires, logic::tip_budget(tw));
    let th = logic::tip_height(&lines);
    let at = pt(logic::tip_origin((cur.x, cur.y), tw, th, w, h));
    frame.fill(
        &Path::rounded_rectangle(at, Size::new(tw, th), 4.0.into()),
        c(T.tip),
    );
    frame.stroke(
        &Path::rounded_rectangle(at, Size::new(tw, th), 4.0.into()),
        Stroke::default().with_width(1.0).with_color(c(T.tip_edge)),
    );
    let mut y = at.y + 7.0;
    for line in &lines {
        let color = tone_color(line.tone);
        if !line.text.is_empty() {
            frame.fill_text(canvas::Text {
                content: line.text.clone(),
                position: Point::new(at.x + logic::TIP_PAD_X, y),
                color,
                size: line.size.into(),
                align_x: iced::alignment::Horizontal::Left.into(),
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
        if let Some(right) = &line.right {
            frame.fill_text(canvas::Text {
                content: right.clone(),
                position: Point::new(at.x + tw - logic::TIP_PAD_X, y),
                color,
                size: line.size.into(),
                align_x: iced::alignment::Horizontal::Right.into(),
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
        y += line.size + 4.0;
    }
}

/// The outline for a node shape, centered at `c` with "radius" `r`
/// (half-extent for the square and octagon).
fn shape_path(c: Point, r: f32, shape: IconStyle) -> Path {
    match shape {
        IconStyle::Circle => Path::circle(c, r),
        IconStyle::Square => {
            Path::rectangle(Point::new(c.x - r, c.y - r), Size::new(2.0 * r, 2.0 * r))
        }
        IconStyle::Octagon => Path::new(|b| {
            let [first, rest @ ..] = logic::octagon(c.x, c.y, r);
            b.move_to(pt(first));
            for p in rest {
                b.line_to(pt(p));
            }
            b.close();
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_gui_logic::talents::fixture::{Sandbox, full_string, simc_paste, string_for};
    use wowdps_model::{GearItem, Loadout, TalentPick};

    fn node(ui: &TalentsUi, id: u64) -> Node {
        ui.find_node(id)
            .unwrap_or_else(|| panic!("node {id} not laid out"))
    }

    #[test]
    fn rendering_covers_every_screen_state() {
        let _sb = Sandbox::new("iced-render");
        // Nothing open: the "paste something" placeholder.
        let ui = TalentsUi::open(None);
        let _ = screen(&ui);
        let _ = talents_tab(&ui);

        // The provenance line: logged, then edited.
        let mut ui = TalentsUi::open(None);
        ui.adopt_logged(&Loadout {
            spec_id: Some(62),
            talents: vec![TalentPick {
                node_id: 1,
                entry_id: 101,
                rank: 1,
            }],
            gear: vec![GearItem {
                item_id: 5,
                ilvl: 1,
                ..GearItem::default()
            }],
        });
        assert!(ui.logged);
        let _ = talents_tab(&ui);
        // The logged gear is an inventory tab of its own.
        ui.on_msg(Msg::ToggleTab);
        assert_eq!(ui.tab, Tab::Inventory);
        let _ = screen(&ui);
        ui.on_msg(Msg::ToggleTab);
        ui.click_node(1);
        assert!(ui.edited);
        let _ = screen(&ui);

        // A profile with several loadouts, an inventory and an error line.
        let (a, b) = (full_string(), string_for(&[r#"{"node_id": 3}"#]));
        ui.ingest(&simc_paste("Frosty", Some("proudmoore"), &[&a, &b], true));
        ui.error = Some("boom".to_string());
        let _ = screen(&ui);
        ui.tab = Tab::Inventory;
        let _ = screen(&ui);
        let p = ui.profile.as_ref().unwrap();
        let _ = inventory(p);
        let _ = identity_line(p);
        let _ = item_row(&p.equipped[0]);
        assert_eq!(p.currencies.len(), 3);

        // Warnings, and the tooltip for a node and for a picker option.
        let mut warned = TalentsUi::open(None);
        warned.ingest(&format!("{}AAAA", full_string()));
        assert!(!warned.warnings.is_empty());
        warned.on_msg(Msg::HoverSet(1, None, 7.0, 8.0));
        let _ = talents_tab(&warned);
        warned.picker = Some(2);
        warned.on_msg(Msg::HoverSet(2, Some(1), 5.0, 6.0));
        let _ = talents_tab(&warned);

        // Logged gear: labeled when it fits the slot table, bare past it.
        let one = GearItem {
            item_id: 5,
            ilvl: 1,
            ..GearItem::default()
        };
        let _ = logged_inventory(&[one.clone(), GearItem::default()]);
        let _ = logged_inventory(&vec![one; logic::GEAR_SLOTS.len() + 1]);

        // The tree row with and without a hero column, the hero column
        // with and without its mini-tree.
        let full = {
            let mut u = TalentsUi::open(None);
            u.ingest(&a);
            u.build.clone().unwrap()
        };
        assert!(full.hero_pane.is_some());
        let _ = tree_row(&full, None);
        let _ = tree_row(&full, Some(2));
        let _ = hero_column(77, "Sunfury", full.hero_pane.as_ref(), None);
        let _ = hero_column(77, "Sunfury", None, None);
        let bare = {
            let mut u = TalentsUi::open(None);
            u.ingest(&b);
            u.build.clone().unwrap()
        };
        assert!(bare.hero.is_none());
        let _ = tree_row(&bare, None);
        let _ = pane_header(None, "Mage", &bare.class_pane);
        // The retained caches live in the model's slot, one per layout.
        assert!(caches(&bare.class_pane).is_some());
        assert!(std::ptr::eq(
            caches(&bare.class_pane).unwrap(),
            caches(&bare.class_pane).unwrap()
        ));

        assert_eq!(points_label(2, None), ("2 pts".to_string(), theme::INK_2));
        assert_eq!(
            points_label(1, Some(3)),
            ("1/3 pts".to_string(), theme::INK_2)
        );
        assert_eq!(points_label(3, Some(3)), ("3/3 pts".to_string(), GOLD));
        assert_eq!(model_class(62), Some(wowdps_model::Class::Mage));
        assert_eq!(model_class(0), None);
        // Under the sandbox no per-machine art is consulted.
        assert!(art_background(62).is_none() && art_medallion(77).is_none());
        assert!(art_ring().is_none() && spec_icon(62).is_none());
        assert!(class_icon(wowdps_model::Class::Mage).is_none());
        assert!(spell_icon(1001, IconStyle::Square, false).is_none());
    }

    #[test]
    fn the_over_canvas_maps_mouse_events_to_messages() {
        use iced::mouse::{self, Button, Cursor, Event as Mouse, Interaction};
        use iced::widget::canvas::Program;
        let _sb = Sandbox::new("iced-canvas-events");
        let mut ui = TalentsUi::open(None);
        ui.ingest(&full_string());
        let model = Rc::clone(&ui.build.as_ref().unwrap().class_pane);
        let bounds = Rectangle {
            x: 100.0,
            y: 50.0,
            width: model.w,
            height: model.h,
        };
        let at = |p: Point| Cursor::Available(Point::new(bounds.x + p.x, bounds.y + p.y));
        let moved = |p: Point| {
            iced::Event::Mouse(Mouse::CursorMoved {
                position: Point::new(bounds.x + p.x, bounds.y + p.y),
            })
        };
        let press = |b: Button| iced::Event::Mouse(Mouse::ButtonPressed(b));
        let msg = |a: Option<canvas::Action<Msg>>| a.map(|a| a.into_inner());

        let n1 = Point::new(node(&ui, 1).x, node(&ui, 1).y);
        let n2 = Point::new(node(&ui, 2).x, node(&ui, 2).y);
        let empty = Point::new(model.w - 4.0, (n1.y + n2.y) / 2.0);

        let over = PaneOver {
            model: Rc::clone(&model),
            picker: None,
        };
        let mut state = OverState::default();
        // Entering node 1 publishes its window-space center; staying put
        // says nothing; leaving clears it exactly once.
        let (m, _, _) = msg(over.update(&mut state, &moved(n1), bounds, at(n1))).unwrap();
        assert!(
            matches!(m, Some(Msg::HoverSet(1, None, x, y)) if x == bounds.x + n1.x && y == bounds.y + n1.y),
            "{m:?}"
        );
        assert!(
            over.update(&mut state, &moved(n1), bounds, at(n1))
                .is_none()
        );
        let (m, _, _) = msg(over.update(&mut state, &moved(empty), bounds, at(empty))).unwrap();
        assert!(matches!(m, Some(Msg::HoverClear(1))), "{m:?}");
        assert!(
            over.update(&mut state, &moved(empty), bounds, at(empty))
                .is_none()
        );
        // Clicks.
        let (m, _, status) =
            msg(over.update(&mut state, &press(Button::Left), bounds, at(n1))).unwrap();
        assert!(matches!(m, Some(Msg::NodeClick(1))), "{m:?}");
        assert_eq!(status, iced::event::Status::Captured);
        let (m, _, _) =
            msg(over.update(&mut state, &press(Button::Right), bounds, at(n2))).unwrap();
        assert!(matches!(m, Some(Msg::NodeRightClick(2))), "{m:?}");
        assert!(
            over.update(&mut state, &press(Button::Left), bounds, at(empty))
                .is_none()
        );
        assert!(
            over.update(&mut state, &press(Button::Right), bounds, at(empty))
                .is_none()
        );
        assert!(
            over.update(&mut state, &press(Button::Middle), bounds, at(n1))
                .is_none()
        );
        assert!(
            over.update(
                &mut state,
                &press(Button::Left),
                bounds,
                Cursor::Unavailable
            )
            .is_none()
        );
        assert!(
            over.update(
                &mut state,
                &iced::Event::Window(iced::window::Event::Focused),
                bounds,
                at(n1)
            )
            .is_none()
        );
        assert_eq!(
            over.mouse_interaction(&state, bounds, at(n1)),
            Interaction::Pointer
        );
        assert_eq!(
            over.mouse_interaction(&state, bounds, at(empty)),
            Interaction::default()
        );
        assert_eq!(
            over.mouse_interaction(&state, bounds, Cursor::Unavailable),
            Interaction::default()
        );

        // With node 2's picker open its option tiles win over the nodes
        // under them, and a click elsewhere closes it.
        let over = PaneOver {
            model: Rc::clone(&model),
            picker: Some(2),
        };
        let opt = pt(logic::picker_spots(&model, &node(&ui, 2))[1]);
        let mut state = OverState::default();
        let (m, _, _) = msg(over.update(&mut state, &moved(opt), bounds, at(opt))).unwrap();
        assert!(
            matches!(m, Some(Msg::HoverSet(2, Some(1), x, _)) if x == bounds.x + opt.x),
            "{m:?}"
        );
        assert_eq!(state.hover, Some((2, Some(1))));
        let (m, _, _) =
            msg(over.update(&mut state, &press(Button::Left), bounds, at(opt))).unwrap();
        assert!(matches!(m, Some(Msg::PickChoice(2, 1))), "{m:?}");
        let (m, _, _) =
            msg(over.update(&mut state, &press(Button::Left), bounds, at(empty))).unwrap();
        assert!(matches!(m, Some(Msg::ClosePicker)), "{m:?}");
        assert_eq!(
            over.mouse_interaction(&state, bounds, at(opt)),
            Interaction::Pointer
        );
        let _ = mouse::Interaction::default();
    }

    /// iced's software renderer, headless: enough to tessellate every
    /// canvas the viewer draws.
    fn renderer() -> Renderer {
        Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
        ))
    }

    #[test]
    fn the_canvases_draw_headless() {
        use iced::mouse::Cursor;
        use iced::widget::canvas::Program;
        let _sb = Sandbox::new("iced-draw");
        let r = renderer();
        let theme = Theme::Dark;

        // A build with every frame state: taken (gold), granted (teal),
        // available (green), out of reach (gray), a lit edge with its
        // arrow, a choice node with carets.
        let mut ui = TalentsUi::open(None);
        let pick = |node_id, entry_id, rank| TalentPick {
            node_id,
            entry_id,
            rank,
        };
        ui.adopt_logged(&Loadout {
            spec_id: Some(62),
            talents: vec![
                pick(1, 101, 2),
                pick(2, 132, 1),
                pick(3, 104, 0),
                pick(4, 151, 1),
                pick(5, 106, 1),
            ],
            gear: Vec::new(),
        });
        assert_eq!(ui.error, None);
        let b = ui.build.as_ref().unwrap();
        assert!(node(&ui, 6).available && node(&ui, 3).granted);
        for pane in b.panes() {
            let bounds = Rectangle::new(Point::ORIGIN, Size::new(pane.w, pane.h));
            let under = PaneUnder {
                model: Rc::clone(pane),
            };
            // Twice: the second draw serves the cached tessellation.
            assert_eq!(
                under
                    .draw(&(), &r, &theme, bounds, Cursor::Unavailable)
                    .len(),
                1
            );
            assert_eq!(
                under
                    .draw(&(), &r, &theme, bounds, Cursor::Unavailable)
                    .len(),
                1
            );
            let first = pane.nodes.first().map(|n| n.id);
            for picker in [None, Some(2)] {
                let over = PaneOver {
                    model: Rc::clone(pane),
                    picker,
                };
                for hover in [None, first.map(|id| (id, None)), Some((2, Some(1)))] {
                    let state = OverState { hover };
                    let g = over.draw(&state, &r, &theme, bounds, Cursor::Unavailable);
                    assert_eq!(g.len(), 2, "chrome + live layer");
                }
            }
        }
        // Uncached, as a slot holding no caches would draw.
        let blank = canvas::Frame::new(&r, Size::new(4.0, 4.0));
        drop(blank);
        let _ = cached(None, &r, Size::new(4.0, 4.0), |_| {});

        // The tooltip overlay, anchored in window space.
        let n1 = node(&ui, 1);
        let tip = TipOverlay {
            node: Some((n1.clone(), "Mage".to_string())),
            anchor: (30.0, 40.0),
        };
        let bounds = Rectangle::new(Point::new(10.0, 10.0), Size::new(400.0, 300.0));
        assert_eq!(
            tip.draw(&(), &r, &theme, bounds, Cursor::Unavailable).len(),
            1
        );
        let none = TipOverlay {
            node: None,
            anchor: (0.0, 0.0),
        };
        assert_eq!(
            none.draw(&(), &r, &theme, bounds, Cursor::Unavailable)
                .len(),
            1
        );

        // The backdrop cover-fits a (tiny) painting.
        let bg = ImageHandle::from_rgba(2, 2, vec![0u8; 16]);
        let back = Backdrop { bg, w: 2, h: 2 };
        assert_eq!(
            back.draw(&(), &r, &theme, bounds, Cursor::Unavailable)
                .len(),
            1
        );
        let zero = Backdrop {
            bg: ImageHandle::from_rgba(2, 2, vec![0u8; 16]),
            w: 0,
            h: 0,
        };
        assert_eq!(
            zero.draw(&(), &r, &theme, bounds, Cursor::Unavailable)
                .len(),
            1
        );
        let _ = pane_header(
            Some(ImageHandle::from_rgba(2, 2, vec![0u8; 16])),
            "Mage",
            &b.class_pane,
        );
    }

    #[test]
    fn the_tooltip_draws_every_line_kind() {
        let _sb = Sandbox::new("iced-tooltip");
        let r = renderer();
        let mut ui = TalentsUi::open(None);
        ui.ingest(&full_string());
        let (w, h) = (400.0, 300.0);
        let mut frame = canvas::Frame::new(&r, Size::new(w, h));

        // Node 1: cost + range + cast, per-rank descriptions, "Requires";
        // at the right edge the box flips; a narrow surface clamps it.
        let n1 = node(&ui, 1);
        draw_tooltip(&mut frame, &n1, "Mage", Point::new(40.0, 40.0), w, h);
        draw_tooltip(&mut frame, &n1, "Mage", Point::new(w - 5.0, h - 5.0), w, h);
        draw_tooltip(&mut frame, &n1, "", Point::new(10.0, 10.0), 120.0, 80.0);
        // A choice, a tiered node and a node with a blue restriction.
        draw_tooltip(
            &mut frame,
            &node(&ui, 2),
            "Mage",
            Point::new(40.0, 40.0),
            w,
            h,
        );
        draw_tooltip(
            &mut frame,
            &node(&ui, 6),
            "Mage",
            Point::new(40.0, 40.0),
            w,
            h,
        );
        let mut plain = node(&ui, 3);
        plain.desc = "Does a thing.\n\nCurses: only one.".to_string();
        draw_tooltip(&mut frame, &plain, "Mage", Point::new(40.0, 40.0), w, h);
        for tone in [
            Tone::Plain,
            Tone::Meta,
            Tone::Desc,
            Tone::Note,
            Tone::Unreached,
        ] {
            assert_eq!(tone_color(tone).a, 1.0, "the game's words are opaque");
        }
        assert_eq!(frame.size(), Size::new(w, h));
        let _ = frame.into_geometry();
    }

    #[test]
    fn every_node_shape_has_its_outline() {
        for shape in [IconStyle::Circle, IconStyle::Square, IconStyle::Octagon] {
            let _ = shape_path(Point::new(10.0, 10.0), 14.0, shape);
        }
        assert_eq!(pt((1.0, 2.0)), Point::new(1.0, 2.0));
    }
}
