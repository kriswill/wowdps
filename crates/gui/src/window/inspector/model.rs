//! The inspector as owned data (the iced window's `Insp`): a pure function
//! of the window — the stage's `ClientState` and the window's own state
//! ([`InspState`]) — built once a frame and drawn by `view.rs` at whatever
//! width the stage gives it. Four builders, by what is on the stage: a
//! player's drill, a death's recap, an enemy's attackers, a comparison's
//! pair. The words and the numbers are gui-logic's (`inspect`); this module
//! decides which, and what a press does ([`Press`]).

use std::collections::{HashMap, HashSet};

use wowdps_gui_logic::glyph::Glyph;
use wowdps_gui_logic::history::Kept;
use wowdps_gui_logic::inspect::curves::{
    RATE_BUCKET_MS, curve, dead_spans, peak_in, rate_bucket, span_of, stack_series, window_of,
};
use wowdps_gui_logic::inspect::geometry::PLOT_H;
use wowdps_gui_logic::inspect::list::{Kind, Room};
use wowdps_gui_logic::inspect::matrix::matrices;
use wowdps_gui_logic::inspect::nums::{
    Num, Tally, mit_pieces, num, player_nums, plays, total_word,
};
use wowdps_gui_logic::inspect::plot::{Curve, Dead, Ink};
use wowdps_gui_logic::inspect::recap::{Recap, died_words};
use wowdps_gui_logic::inspect::{Roster, lanes, stack, wide};
use wowdps_gui_logic::labels::{rate_label, realmless, realmless_rows, shown_name};
use wowdps_gui_logic::table::{Col, figure};
use wowdps_gui_logic::theme::{self as gl, AA_CONTRAST, DataTokens, WindowTokens};
use wowdps_gui_logic::{deaths, graph::mmss, tree};
use wowdps_model::fmt::{commas, duration, mitigation_line};
use wowdps_model::{
    Class, GraphMode, Mark, MarkKind, Pane, Row, Screen, Spec, StackBase, StackCell,
    StackingDebuff, Timeline, View,
};
use wowdps_proto::{ClientState, CompareSide, DeathWindow};

/// What a press in the inspector does — the iced window's messages.
#[derive(Debug, Clone, PartialEq)]
pub enum Press {
    /// `v`: pin the player for a comparison, or stop comparing.
    PinCompare,
    /// `t`: the talent viewer on the player.
    OpenTalents,
    /// A view switch (the recap's "Their damage", the "Death recap").
    PickView(View),
    /// `g`: per second or cumulative.
    ToggleGraph,
    /// R26: the graph stacked, or the total alone.
    ToggleStack,
    /// Back out of an opened ability (one level; the keys stay here).
    CloseAbility,
    /// A list's tab.
    Tab(Pane),
    /// R21: the Stacks tab.
    ShowStacks(bool),
    /// v16: descend into ability `i`.
    SpellRow(usize),
    /// R24: descend into attacker `i` of an enemy.
    AttackerRow(usize),
    /// v18: both sides of a pair into one ability (key, label).
    CompareSpell(String, String),
    /// R26: a tree fold, by key.
    Fold(String),
    /// R9: a death window, by index.
    PickDeath(u32),
    /// A narrow window's pushed inspector, back to the meter.
    Uninspect,
    /// The corner button: widened over the stage, or back beside the meter.
    Widen,
    /// The ability list's heading: sort by this column.
    Sort(Col),
}

/// Where a drag-selected window goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeTo {
    /// The drill's own zoom (client-side; on Enemies it rides the Watch).
    Drill,
    /// The pair's, which the daemon echoes.
    Compare,
}

/// The window's own inspector state — none of it `ClientState`'s, none of
/// it ever sent.
#[derive(Default)]
pub struct InspState {
    /// Who is who, by guid and by name: everyone the window saw on a meter.
    pub roster: Roster,
    /// The last player's body, standing in (dimmed) while the next one's
    /// breakdown is on its way.
    pub held: Option<Held>,
    /// R26: the folds opened (session-wide), and the cursor when the keys
    /// rest on a group or a part rather than a row.
    pub tree_open: HashSet<String>,
    pub tree_cursor: Option<(String, tree::Node)>,
    /// The ability list's sort.
    pub drill_sort: Option<(Col, bool)>,
    /// R26: stacked (the default) or the total alone, and each band's seat.
    pub stack_graph: bool,
    pub stack_slots: stack::Slots,
    /// R21: the Stacks tab is up.
    pub stacks_open: bool,
    /// The pair's ability under the pointer, by key (lit in both lists).
    pub spell_hover: Option<String>,
    /// A drill list's line under the pointer: its pane and index.
    pub hover: Option<(Pane, usize)>,
    /// The inspector's own scroll.
    pub scroll: crate::scrollbar::Scroll,
    /// A step moved the keys: bring their line into sight once laid out.
    pub reveal: std::rc::Rc<std::cell::Cell<bool>>,
    /// Widened over the stage under the tabs (the corner button, `f`) —
    /// session-wide, and meaningless in a narrow window, whose inspector
    /// is pushed over everything already.
    pub wide: bool,
}

impl InspState {
    pub fn new() -> Self {
        Self {
            stack_graph: true,
            ..Self::default()
        }
    }
}

/// A head's disc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Disc {
    Player(Option<Class>, Option<Spec>),
    Enemy,
}

/// One piece of a head's name: its words, its ink, and whether it is set
/// semibold (a pair's " and " is not).
#[derive(Debug, Clone, PartialEq)]
pub struct NamePiece {
    pub words: String,
    pub ink: gl::Color,
    pub semibold: bool,
}

/// The head (`.ihead`): the disc(s), the name, and the line under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub discs: Vec<Disc>,
    pub name: Vec<NamePiece>,
    pub sub: String,
    /// A single row's amount against the top row's (0..=1) — its meter bar's
    /// length — for a theme that sweeps it round the crest (`effects.dial`).
    pub sweep: Option<f32>,
}

/// One action (`.btn`): its glyph, its words, pressed or not, what it does
/// — `None` for one this pull cannot do, drawn inert — and its tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct Act {
    pub glyph: Glyph,
    pub words: String,
    pub pressed: bool,
    pub press: Option<Press>,
    pub tip: &'static str,
}

/// What the graph's top line ends with.
#[derive(Debug, Clone, PartialEq)]
pub enum Tail {
    Words(String),
    /// A pair's legend: each name, its colour, dashed or not.
    Legend(Vec<(String, gl::Color, bool)>),
}

/// The graph section (`.igraph`): its top line and what the plot draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    pub lead: String,
    pub tail: Tail,
    pub window: (u32, u32),
    pub peak: f64,
    pub curves: Vec<Curve>,
    pub dead: Vec<Dead>,
    pub lanes: Vec<lanes::Row>,
    pub total: bool,
    pub word: &'static str,
    pub range_to: RangeTo,
    /// The plot's height: taller in a widened inspector.
    pub plot_h: f32,
}

/// An opened ability's strip: the crumb and its numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Ability {
    pub who: String,
    pub who_ink: Option<gl::Color>,
    pub label: String,
    pub row: Option<Row>,
    pub view: View,
    pub tally: Tally,
}

/// R21: the stack ledger, behind its tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Stacks {
    pub on: bool,
    pub stacking: Vec<StackingDebuff>,
    pub cells: Vec<StackCell>,
    pub base: Vec<StackBase>,
    pub dropped: u32,
}

/// R9: the death chips.
#[derive(Debug, Clone, PartialEq)]
pub struct Deaths {
    pub windows: Vec<DeathWindow>,
    pub shown: Option<u32>,
    pub dropped: u32,
}

/// What leads a list's line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    /// An ability: its icon (or a lettered square) and its name, the pet
    /// after it.
    Spell,
    /// A person: their disc (or the foe's), their name, "you" on the owner.
    Person,
}

/// The bar under a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bar {
    /// One colour for the whole list (the player's).
    Of(gl::Color),
    /// Each row its own class (else hostile).
    Own,
}

/// What a press on a line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinePress {
    Nothing,
    Spell,
    Attacker,
    Pair,
}

/// A list (`.ilist`).
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub rows: Vec<Row>,
    pub kind: Kind,
    pub view: View,
    /// A pair side's R17 line.
    pub note: Option<String>,
    pub head: String,
    pub head_ink: Option<gl::Color>,
    pub lead: Lead,
    pub bar: Bar,
    pub selected: Option<usize>,
    /// The keys are on this list (the selected line wears the accent edge).
    pub keyed: bool,
    pub hover: Option<usize>,
    /// The owner's row, tagged "you".
    pub you: Option<usize>,
    pub pane: Pane,
    /// A pair's hovered ability, by key.
    pub pair_hover: Option<String>,
    pub sort: Option<(Col, bool)>,
    pub sortable: bool,
    pub press: LinePress,
    /// The side padding: 16, or a pair's 10.
    pub side: f32,
    /// R26: the tree's lines, when the list is the player's abilities.
    pub tree: Option<Vec<tree::Line>>,
    /// R26: the stack's hues by key, when the graph is stacked.
    pub hues: HashMap<String, gl::Color>,
    /// How much room its figures have: which columns it shows.
    pub room: Room,
}

/// What stands under the tabs.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Nothing,
    One(Box<List>),
    Recap(Recap),
    Pair(Box<(List, List)>),
    /// A widened inspector's two panes side by side (or, on a stage too
    /// narrow to split, one over the other): a drill's abilities and its
    /// targets, a recap and its attackers. The first is the wider.
    Split(Box<(Body, Body)>),
}

/// The inspector, as owned data.
#[derive(Debug, Clone, PartialEq)]
pub struct Insp {
    pub head: Head,
    pub nums: Vec<Num>,
    pub acts: Vec<Act>,
    pub mit: Vec<(String, String, String)>,
    pub deaths: Option<Deaths>,
    pub ability: Option<Ability>,
    pub graph: Option<Graph>,
    pub stacks: Option<Stacks>,
    pub tabs: Option<([&'static str; 2], Pane)>,
    pub body: Body,
    /// The body is the last player's, held while this one's is on its way.
    pub stale: bool,
    pub note: Option<String>,
    /// Widened over the stage: the head on one line, the numbers in one,
    /// R21's matrices under the lists rather than behind a tab.
    pub wide: bool,
}

/// The last player's body (`Held`): standing in, dimmed, while the next
/// player's breakdown is on its way, so the column keeps its height.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    key: String,
    view: View,
    pane: Pane,
    mode: GraphMode,
    generation: u64,
    mit: Vec<(String, String, String)>,
    graph: Option<Graph>,
    body: Body,
    /// Taken from a widened inspector: it stands in for one alone.
    wide: bool,
}

impl Held {
    /// A player's drill as it stands — none for an ability's, a recap, an
    /// enemy or a pair, which never stand in.
    pub fn of(insp: &Insp, app: &ClientState) -> Option<Self> {
        let d = app.drill.as_ref()?;
        let fits = app.screen == Screen::Meter
            && d.spell.is_none()
            && app.drill_breakdown().is_some()
            && !matches!(app.view, View::Deaths | View::EnemyTaken)
            && !insp.stale;
        fits.then(|| Held {
            key: d.key.clone(),
            view: app.view,
            pane: d.pane,
            mode: app.graph_mode(),
            generation: app.snapshot_gen(),
            mit: insp.mit.clone(),
            graph: insp.graph.clone(),
            body: insp.body.clone(),
            wide: insp.wide,
        })
    }

    /// Still this snapshot's player, pane, view and mode, in the layout
    /// (`wide`) on show.
    pub fn current(&self, app: &ClientState, wide: bool) -> bool {
        app.drill
            .as_ref()
            .is_some_and(|d| d.key == self.key && d.pane == self.pane && d.spell.is_none())
            && app.view == self.view
            && app.graph_mode() == self.mode
            && app.snapshot_gen() == self.generation
            && wide == self.wide
    }

    /// May stand in for whoever is drilled now: the same pane, view, mode
    /// and layout.
    fn fits(&self, app: &ClientState, wide: bool) -> bool {
        app.drill
            .as_ref()
            .is_some_and(|d| d.pane == self.pane && d.spell.is_none())
            && app.view == self.view
            && app.graph_mode() == self.mode
            && wide == self.wide
    }
}

/// What the builders read: the stage's state and the window's.
pub struct Ctx<'a> {
    pub app: &'a ClientState,
    pub st: &'a InspState,
    pub t: &'a WindowTokens,
    /// The theme's data hues: the stacked bands.
    pub data: &'a DataTokens,
    pub hide: bool,
    /// The owner's row on the chart on screen.
    pub owner: Option<usize>,
    /// A person list's owner, by rows.
    pub owner_of: &'a dyn Fn(&[Row]) -> Option<usize>,
    /// The pull on the stage is a stored one, and what the window offers on
    /// it (v42, `Stored::offered`): a pair where the store kept the
    /// details, an ability opened where it kept its targets. `None` for a
    /// pull of the log.
    pub stored: Option<Kept>,
    /// A stored pull answered without this player's breakdown.
    pub bare: bool,
    /// How the inspector stands: a column beside the meter (or pushed),
    /// or widened over the stage.
    pub layout: Layout,
}

/// How the inspector stands, for its lists' columns and its panes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    /// Beside the meter, or pushed over a narrow stage: one list at a time
    /// under its tabs, with `room` for its figures.
    Column { room: Room },
    /// Widened over a stage `stage` px wide (at zoom 1).
    Wide { stage: f32 },
}

impl Layout {
    pub fn wide(self) -> bool {
        matches!(self, Layout::Wide { .. })
    }

    /// The room a drill's lists have.
    fn room(self) -> Room {
        match self {
            Layout::Column { room } => room,
            Layout::Wide { .. } => Room::Wide,
        }
    }
}

impl Ctx<'_> {
    /// A name as text: the class's colour lifted to read on the panel.
    fn name_ink(&self, class: Option<Class>) -> gl::Color {
        class.map_or(self.t.ink, |c| {
            gl::class_text_on(c, self.t.surface, AA_CONTRAST)
        })
    }

    fn class_rgb(&self, class: Option<Class>) -> gl::Color {
        class.map_or(self.t.classless, gl::Color::of_class)
    }

    fn piece(&self, words: String, class: Option<Class>) -> NamePiece {
        NamePiece {
            words,
            ink: self.name_ink(class),
            semibold: true,
        }
    }
}

/// What a stored pull's inspector says when the store answered without the
/// player's breakdown: it keeps a pull's rows longer than its details.
const BARE: &str = "The history store kept this pull's rows, not this player's breakdown.";

/// What an inert Compare says on a stored pull whose details are gone.
const NO_COMPARE_TIP: &str = "Comparing needs the pull's details: the store kept its rows alone";

impl Insp {
    /// The inspector for what is on the stage, laid out as it stands.
    pub fn of(cx: &Ctx) -> Self {
        let mut insp = Self::build(cx);
        insp.wide = cx.layout.wide();
        if insp.wide
            && let Some(g) = insp.graph.as_mut()
        {
            g.plot_h = wide::PLOT_H;
        }
        insp
    }

    fn build(cx: &Ctx) -> Self {
        let app = cx.app;
        let rows = app.rows();
        if app.screen == Screen::Compare {
            return pair(cx, &rows);
        }
        let Some(drill) = app.drill.as_ref() else {
            return Insp::quiet(if rows.is_empty() {
                "Nothing to inspect yet."
            } else {
                "Select a player to inspect them here."
            });
        };
        let me = rows.iter().position(|r| r.key == drill.key);
        match app.view {
            View::Deaths => recap(cx, &rows, me),
            View::EnemyTaken => enemy(cx, &rows, me),
            _ => player(cx, &rows, me),
        }
    }

    pub fn quiet(words: &str) -> Self {
        Insp {
            head: Head {
                discs: Vec::new(),
                name: Vec::new(),
                sub: String::new(),
                sweep: None,
            },
            nums: Vec::new(),
            acts: Vec::new(),
            mit: Vec::new(),
            deaths: None,
            ability: None,
            graph: None,
            stacks: None,
            tabs: None,
            body: Body::Nothing,
            stale: false,
            note: Some(words.to_string()),
            wide: false,
        }
    }
}

fn mode_act(mode: GraphMode) -> Act {
    Act {
        glyph: Glyph::Graph,
        words: match mode {
            GraphMode::Dps => "Per second".to_string(),
            GraphMode::Total => "Cumulative".to_string(),
        },
        pressed: mode == GraphMode::Total,
        press: Some(Press::ToggleGraph),
        tip: "Per second or cumulative (g)",
    }
}

fn stack_act(stacked: bool, on_targets: bool) -> Act {
    Act {
        glyph: Glyph::List,
        words: match (stacked, on_targets) {
            (false, _) => "Total",
            (true, true) => "By target",
            (true, false) => "By ability",
        }
        .to_string(),
        pressed: !stacked,
        press: Some(Press::ToggleStack),
        tip: "Stack the graph, or draw the total alone",
    }
}

/// v38: a list's heading under a zoom — what the drag did to its rows:
/// "Ability, 0:42–1:13" when they are the window's (`windowed`: the
/// daemon scopes this list on this view), "Target, whole fight" when the
/// list keeps no clock of its own (Healing's targets, what hit a player)
/// or the pull is a stored one. No zoom, no words.
fn scoped(cx: &Ctx, head: &str, windowed: bool) -> String {
    let Some((lo, hi)) = cx.app.drill_range() else {
        return head.to_string();
    };
    match cx.app.drill_shown_range() {
        Some(_) if windowed => format!("{head}, {}–{}", mmss(lo), mmss(hi)),
        _ if cx.stored.is_some() => format!("{head}, whole pull"),
        _ => format!("{head}, whole fight"),
    }
}

/// v38: a zoomed graph's top line says what the window's rows add up to —
/// "0:42–1:13, 41,152 dps, right-click resets" — when the abilities are
/// the window's.
fn window_tail(graph: &mut Graph, app: &ClientState, by_spell: &[Row]) {
    let Some((lo, hi)) = app.drill_shown_range() else {
        return;
    };
    if !app.view.is_rate() || by_spell.is_empty() {
        return;
    }
    let rate: f64 = by_spell.iter().map(|r| r.per_sec).sum();
    graph.tail = Tail::Words(format!(
        "{}–{}, {} {}, right-click resets",
        mmss(lo),
        mmss(hi),
        commas(rate.round() as u64),
        rate_label(app.view),
    ));
}

/// A body on its way is no empty list: the lists wait for it.
fn answered(app: &ClientState, body: Body) -> Body {
    if app.drill_breakdown().is_some() {
        body
    } else {
        Body::Nothing
    }
}

/// The graph's section for `curves`: its words, its window, its one scale.
#[allow(clippy::too_many_arguments)]
fn graph_of(
    app: &ClientState,
    span: u32,
    shown: Option<(u32, u32)>,
    curves: Vec<Curve>,
    dead: Vec<Dead>,
    lanes: Vec<lanes::Row>,
    legend: Option<Vec<(String, gl::Color, bool)>>,
    range_to: RangeTo,
) -> Graph {
    let mode = app.graph_mode();
    let window = window_of(shown, span);
    let peak = peak_in(&curves, window);
    let word = rate_label(app.view);
    let lead = match mode {
        GraphMode::Dps => word.to_string(),
        GraphMode::Total => format!("{} so far", total_word(app.view)),
    };
    // With lanes the plot's gutter is its scale, so the top line names the
    // measure alone; without them it says the peak.
    let lead = if lanes.is_empty() {
        format!("{lead}, peak {}", figure(peak.round() as u64))
    } else {
        lead
    };
    let secs = curves.first().map_or(RATE_BUCKET_MS, |c| c.bucket_ms) / 1000;
    let tail = match (shown, legend) {
        (Some((lo, hi)), _) => {
            Tail::Words(format!("{}–{}, right-click resets", mmss(lo), mmss(hi)))
        }
        (None, Some(legend)) => Tail::Legend(legend),
        (None, None) => Tail::Words(match mode {
            GraphMode::Dps => format!("per second, {} s", secs.max(1)),
            GraphMode::Total => "cumulative".to_string(),
        }),
    };
    Graph {
        lead,
        tail,
        window,
        peak,
        curves,
        dead,
        lanes,
        total: mode == GraphMode::Total,
        word,
        range_to,
        plot_h: PLOT_H,
    }
}

/// The player's curve (an area in their class colour), or — with an
/// ability open — theirs ghosted behind the ability's in its school's.
fn focus_curves(
    cx: &Ctx,
    whole: &Timeline,
    class: Option<Class>,
    focus: Option<(&Timeline, gl::Color)>,
    window: (u32, u32),
) -> Vec<Curve> {
    let mode = cx.app.graph_mode();
    let bucket = rate_bucket(window.1.saturating_sub(window.0), whole.bucket_ms);
    let (points, bucket_ms) = curve(whole, mode, bucket);
    let mut curves = vec![Curve {
        name: String::new(),
        color: cx.class_rgb(class),
        points,
        bucket_ms,
        ink: if focus.is_some() {
            Ink::Ghost
        } else {
            Ink::Area
        },
    }];
    if let Some((ft, color)) = focus {
        let (points, bucket_ms) = curve(ft, mode, bucket);
        curves.push(Curve {
            name: String::new(),
            color,
            points,
            bucket_ms,
            ink: Ink::Area,
        });
    }
    curves
}

/// R26 (step 2): the graph stacked by ability, or an open ability by
/// target over the player's ghosted line.
fn stacked_curves(
    cx: &Ctx,
    whole: &Timeline,
    class: Option<Class>,
    window: (u32, u32),
) -> Option<Vec<Curve>> {
    let app = cx.app;
    let (context, series, on_targets) = stack_series(app)?;
    let mode = app.graph_mode();
    let bucket = rate_bucket(window.1.saturating_sub(window.0), whole.bucket_ms);
    let cut = |t: &Timeline| curve(t, mode, bucket);
    let (rows, _) = app.breakdown();
    let tree = app.drill_tree();
    let name = |key: &str| -> String {
        if on_targets {
            return shown_name(key, cx.hide);
        }
        if let Some(g) = tree.group(key) {
            return g.label.clone();
        }
        rows.iter()
            .find(|r| r.key == key)
            .map_or_else(|| key.to_string(), |r| r.label.clone())
    };
    let mut curves = Vec::new();
    let stacked_over = if on_targets {
        let (points, bucket_ms) = cut(whole);
        curves.push(Curve {
            name: String::new(),
            color: cx.class_rgb(class),
            points,
            bucket_ms,
            ink: Ink::Ghost,
        });
        app.spell_timeline()?.clone()
    } else {
        whole.clone()
    };
    curves.extend(stack::curves(
        &series,
        &stacked_over,
        &cx.st.stack_slots,
        &context,
        name,
        cut,
        cx.data,
    ));
    Some(curves)
}

/// The lines of a list pane, as the drill shows it.
#[allow(clippy::too_many_arguments)]
fn pane_list(
    cx: &Ctx,
    rows: Vec<Row>,
    kind: Kind,
    view: View,
    head: &str,
    pane: Pane,
    lead: Lead,
    bar: Bar,
    press: LinePress,
) -> List {
    let app = cx.app;
    // The keys' row is lit only while the keys are in the inspector.
    let keyed = app.inspecting() || !app.follows_selection();
    let selected = app
        .drill
        .as_ref()
        .filter(|d| keyed && d.pane == pane && d.spell.is_none())
        .map(|d| match pane {
            Pane::Spell => d.spell_sel,
            Pane::Target => d.target_sel,
        });
    let sortable = kind == Kind::Abilities && pane == Pane::Spell;
    let you = (lead == Lead::Person)
        .then(|| (cx.owner_of)(&rows))
        .flatten();
    let tree = (sortable && tree_drawn(app))
        .then(|| tree::lines(&rows, &app.drill_tree(), &cx.st.tree_open, cx.st.drill_sort));
    let selected = match &tree {
        Some(lines) => selected.and_then(|_| tree_keyed(cx.app, cx.st, lines)),
        None => selected,
    };
    let hues = match stack_series(app).filter(|_| cx.st.stack_graph) {
        Some((context, series, on_targets))
            if (on_targets && kind == Kind::Targets) || (!on_targets && sortable) =>
        {
            let keys: Vec<String> = series.iter().map(|s| s.key.clone()).collect();
            keys.iter()
                .filter_map(|k| {
                    stack::hue(&cx.st.stack_slots, &context, &keys, k, cx.data)
                        .map(|h| (k.clone(), h))
                })
                .collect()
        }
        _ => HashMap::new(),
    };
    List {
        rows,
        kind,
        view,
        note: None,
        head: head.to_string(),
        head_ink: None,
        lead,
        bar,
        selected,
        keyed: selected.is_some(),
        hover: cx.st.hover.filter(|(p, _)| *p == pane).map(|(_, i)| i),
        you,
        pane,
        pair_hover: None,
        sort: sortable.then_some(cx.st.drill_sort).flatten(),
        sortable,
        press,
        side: SIDE,
        tree,
        hues,
        room: cx.layout.room(),
    }
}

/// A list's side padding, and a pair side's.
pub const SIDE: f32 = 16.0;
pub const SIDE_PAIR: f32 = 10.0;

/// R26: whether the drill's ability list draws as the tree — Damage or
/// Healing, the Abilities tab up, no ability open.
pub fn tree_drawn(app: &ClientState) -> bool {
    app.drill.as_ref().is_some_and(|d| {
        d.spell.is_none()
            && d.pane == Pane::Spell
            && matches!(app.view, View::Damage | View::Healing)
    })
}

/// R26: the tree's lines as drawn, when it is.
pub fn tree_lines(app: &ClientState, st: &InspState) -> Option<Vec<tree::Line>> {
    tree_drawn(app).then(|| {
        let (by_spell, _) = app.breakdown();
        tree::lines(&by_spell, &app.drill_tree(), &st.tree_open, st.drill_sort)
    })
}

/// R26: which line the keys rest on.
pub fn tree_keyed(app: &ClientState, st: &InspState, lines: &[tree::Line]) -> Option<usize> {
    let d = app.drill.as_ref()?;
    let cursor = st
        .tree_cursor
        .as_ref()
        .filter(|(key, _)| *key == d.key)
        .map(|(_, node)| node);
    tree::keyed(lines, cursor, d.spell_sel)
}

/// `row`'s amount against the largest in `rows`: the length its meter bar
/// is drawn at, 0..=1.
fn against_top(row: &Row, rows: &[Row]) -> f32 {
    let top = rows.iter().map(|r| r.amount).max().unwrap_or(0).max(1);
    (row.amount as f64 / top as f64).clamp(0.0, 1.0) as f32
}

/// A player's drill: what they did, over the fight.
fn player(cx: &Ctx, rows: &[Row], me: Option<usize>) -> Insp {
    let app = cx.app;
    let hide = cx.hide;
    let Some(drill) = app.drill.as_ref() else {
        return Insp::quiet("Select a player to inspect them here.");
    };
    let row = me.and_then(|i| rows.get(i));
    let (class, spec) = row.map_or((None, None), |r| (r.class, r.spec));
    let view = app.view;
    let marked = app.drill_timeline().filter(|t| !t.buckets.is_empty());
    let timeline = marked.filter(|_| view.is_rate());
    let deaths: Vec<&Mark> = marked
        .map(|t| {
            t.marks
                .iter()
                .filter(|m| m.kind == MarkKind::Death)
                .collect()
        })
        .unwrap_or_default();
    let owner = me.is_some() && cx.owner == me;
    let mut sub: Vec<String> = plays(class, spec).into_iter().collect();
    if owner {
        sub.push("you".to_string());
    }
    if let Some(first) = deaths.first() {
        sub.push(match deaths.len() {
            1 => format!("died {}", mmss(first.at_ms.max(0) as u32)),
            n => format!("died {n} times, first {}", mmss(first.at_ms.max(0) as u32)),
        });
    }
    let head = Head {
        discs: vec![Disc::Player(class, spec)],
        name: vec![cx.piece(shown_name(&drill.label, hide), class)],
        sub: sub.join(", "),
        sweep: row.map(|r| against_top(r, rows)),
    };
    let nums = row.map(|r| player_nums(view, rows, r)).unwrap_or_default();

    let pinned = app
        .compare_picks()
        .first()
        .is_some_and(|(k, _)| *k == drill.key);
    let mut acts = vec![Act {
        glyph: Glyph::Compare,
        words: if pinned {
            "Pinned, pick another".to_string()
        } else {
            "Compare".to_string()
        },
        pressed: pinned,
        // v42: a stored pull compares where the store kept its details;
        // where it kept the rows alone, Compare stands inert, saying why —
        // but a pin already up is always let go.
        press: (pinned || cx.stored.is_none_or(|k| k.details)).then_some(Press::PinCompare),
        tip: match (cx.stored.is_none_or(|k| k.details), pinned) {
            (_, true) => "Stop comparing (v)",
            (false, false) => NO_COMPARE_TIP,
            (true, false) => "Pin for comparison (v)",
        },
    }];
    acts.push(Act {
        glyph: Glyph::Book,
        words: "Talents and gear".to_string(),
        pressed: false,
        press: Some(Press::OpenTalents),
        tip: "Talents and gear (t)",
    });
    if !deaths.is_empty() {
        acts.push(Act {
            glyph: Glyph::Skull,
            words: "Death recap".to_string(),
            pressed: false,
            press: Some(Press::PickView(View::Deaths)),
            tip: "Death recap (K)",
        });
    }
    if timeline.is_some() {
        acts.push(mode_act(app.graph_mode()));
        if stack_series(app).is_some() {
            acts.push(stack_act(cx.st.stack_graph, app.drill_spell().is_some()));
        }
    }

    let mit = match (view, app.drill_mitigation(), row) {
        (View::Taken, Some(m), Some(r)) => mit_pieces(m, r.amount),
        _ => Vec::new(),
    };

    let spell = app.drill_spell().cloned();
    let spell_row = app.drill_spell_row();
    let focus_color = spell_row
        .as_ref()
        .and_then(|r| gl::school_color(r.school))
        .unwrap_or(cx.t.accent);
    let graph = timeline.map(|t| {
        let focus = app
            .spell_timeline()
            .filter(|_| spell.is_some())
            .map(|ft| (ft, focus_color));
        let span = span_of(app, t);
        let window = window_of(app.drill_range(), span);
        let curves = cx
            .st
            .stack_graph
            .then(|| stacked_curves(cx, t, class, window))
            .flatten()
            .unwrap_or_else(|| focus_curves(cx, t, class, focus, window));
        graph_of(
            app,
            span,
            app.drill_range(),
            curves,
            dead_spans(t, span, None, &cx.st.roster),
            lanes::lanes(&t.marks, &drill.key, class, &cx.st.roster, cx.t.classless),
            None,
            RangeTo::Drill,
        )
    });
    // v38: a zoom's abilities are the window's, and the top line says what
    // they (or the opened one) add up to.
    let mut graph = graph;
    if let Some(g) = graph.as_mut() {
        let rows: Vec<Row> = match &spell {
            Some(_) => spell_row.iter().cloned().collect(),
            None => app.breakdown().0,
        };
        window_tail(g, app, &rows);
    }
    let ability = spell.as_ref().map(|(_, label)| Ability {
        who: shown_name(&drill.label, hide),
        who_ink: Some(cx.name_ink(class)),
        label: if hide {
            realmless(label)
        } else {
            label.clone()
        },
        row: spell_row.clone(),
        view,
        tally: spell
            .as_ref()
            .and_then(|(key, _)| app.drill_tree().meta(key).cloned())
            .map(|m| Tally::of(&m, app.duration_ms().clamp(0, i64::from(u32::MAX)) as u32))
            .unwrap_or_default(),
    });

    let stacks = app
        .drill_stacks()
        .filter(|(stacking, cells, base)| {
            spell.is_none() && !matrices(stacking, cells, base).is_empty()
        })
        .map(|(stacking, cells, base)| Stacks {
            // Widened, the matrices stand under the lists, no tab to them.
            on: cx.st.stacks_open && !cx.layout.wide(),
            stacking: stacking.to_vec(),
            cells: cells.to_vec(),
            base: base.to_vec(),
            dropped: app.drill_breakdown().map_or(0, |b| b.stacks_dropped),
        });

    let bar = Bar::Of(cx.class_rgb(class));
    let (tabs, body) = if spell.is_some() {
        let targets = realmless_rows(&cx.st.roster.as_themselves(&app.spell_target_rows()), hide);
        let mut l = pane_list(
            cx,
            targets,
            Kind::Targets,
            view,
            &scoped(cx, "Target", view.windows_targets()),
            Pane::Target,
            Lead::Person,
            Bar::Own,
            LinePress::Nothing,
        );
        l.hover = None;
        (None, Body::One(Box::new(l)))
    } else {
        let (by_spell, by_target) = app.breakdown();
        let taken = view == View::Taken;
        let words = if taken {
            ["Hit by", "Attackers"]
        } else {
            ["Abilities", "Targets"]
        };
        let abilities = || {
            pane_list(
                cx,
                realmless_rows(&by_spell, hide),
                Kind::Abilities,
                view,
                // Widened, no tab names what hit them: the heading does.
                &scoped(
                    cx,
                    if taken && cx.layout.wide() {
                        "Hit by"
                    } else {
                        "Ability"
                    },
                    view.windows_drill(),
                ),
                Pane::Spell,
                Lead::Spell,
                bar,
                // v16: Damage and Healing descend into an ability — on a
                // pull of the log, or (v42) a stored one that kept its
                // seconds, whose abilities have curves of their own.
                if matches!(view, View::Damage | View::Healing)
                    && cx.stored.is_none_or(|k| k.abilities)
                {
                    LinePress::Spell
                } else {
                    LinePress::Nothing
                },
            )
        };
        let targets = || {
            pane_list(
                cx,
                realmless_rows(&cx.st.roster.as_themselves(&by_target), hide),
                Kind::Targets,
                view,
                &scoped(
                    cx,
                    if taken { "Attacker" } else { "Target" },
                    view.windows_targets(),
                ),
                Pane::Target,
                Lead::Person,
                Bar::Own,
                LinePress::Nothing,
            )
        };
        let up = drill.pane;
        if cx.layout.wide() {
            // Widened: both panes at once, the keys' one lit — Tab moves
            // them between the two.
            let split = Body::Split(Box::new((
                Body::One(Box::new(abilities())),
                Body::One(Box::new(targets())),
            )));
            (None, split)
        } else {
            let l = match up {
                Pane::Spell => abilities(),
                Pane::Target => targets(),
            };
            (Some((words, up)), Body::One(Box::new(l)))
        }
    };
    let body = answered(app, body);
    let held = cx.st.held.as_ref().filter(|h| {
        app.drill_breakdown().is_none()
            && spell.is_none()
            && !cx.bare
            && h.fits(app, cx.layout.wide())
    });
    let (mit, graph, body, stale) = match held {
        Some(h) => (h.mit.clone(), h.graph.clone(), h.body.clone(), true),
        None => (mit, graph, body, false),
    };
    // A stored pull answered without this player's breakdown has none on
    // its way: nobody stands in for it, and the note says why.
    let note = if row.is_none() || !app.view_answered() {
        Some("Waiting for this view's numbers…".to_string())
    } else {
        cx.bare.then(|| BARE.to_string())
    };
    Insp {
        wide: false,
        head,
        nums,
        acts,
        mit,
        deaths: None,
        ability,
        graph,
        stacks,
        tabs,
        body,
        stale,
        note,
    }
}

/// R9: a death's recap.
fn recap(cx: &Ctx, rows: &[Row], me: Option<usize>) -> Insp {
    let app = cx.app;
    let hide = cx.hide;
    let Some(drill) = app.drill.as_ref() else {
        return Insp::quiet("Select a death to see its recap.");
    };
    let row = me.and_then(|i| rows.get(i));
    let (class, spec) = row.map_or((None, None), |r| (r.class, r.spec));
    let (windows, shown) = app.deaths();
    let at = shown
        .and_then(|i| windows.iter().find(|w| w.index == i))
        .or(windows.last())
        .map(|w| w.at_ms);
    let owner = me.is_some() && cx.owner == me;
    let death = app
        .raid()
        .and_then(|raid| deaths::selected(app, raid).and_then(|i| raid.deaths.get(i)));
    let mut sub: Vec<String> = plays(class, spec).into_iter().collect();
    match (death, at) {
        (Some(d), _) => sub.push(died_words(d, hide)),
        (None, Some(at)) => sub.push(format!("died {}", duration(at))),
        (None, None) => {}
    }
    let (events, attackers) = app.breakdown();
    let blow = events.iter().find(|r| !r.gain);
    let nums = vec![
        num("Died", at.map_or_else(|| "—".to_string(), duration), ""),
        match blow {
            Some(r) => num("Killing blow", commas(r.amount), ""),
            None => num("Killing blow", String::new(), "none"),
        },
        match blow.filter(|r| r.extra > 0) {
            Some(r) => num("Overkill", commas(r.extra), ""),
            None => num("Overkill", String::new(), "none"),
        },
        match death {
            Some(d) => match &d.rez {
                Some(rez) => num("Back in", duration((rez.at_ms - d.at_ms).max(0)), ""),
                None => num("Back in", String::new(), "no"),
            },
            None => num(
                "Deaths",
                row.map_or_else(|| "—".to_string(), |r| commas(r.amount)),
                "",
            ),
        },
    ];
    let acts = vec![Act {
        glyph: Glyph::Sword,
        words: "Their damage".to_string(),
        pressed: false,
        press: Some(Press::PickView(View::Damage)),
        tip: "Their damage (d)",
    }];
    let deaths = Some(Deaths {
        windows: windows.to_vec(),
        shown,
        dropped: app.drill_breakdown().map_or(0, |b| b.deaths_dropped),
    });
    let up = drill.pane;
    let recap_body = || {
        Body::Recap(Recap {
            rows: realmless_rows(&events, hide),
            who: shown_name(&drill.label, hide),
            yours: owner,
            class,
        })
    };
    let attackers_body = || {
        Body::One(Box::new(pane_list(
            cx,
            realmless_rows(&cx.st.roster.as_themselves(&attackers), hide),
            Kind::Targets,
            View::Damage,
            "Attacker",
            Pane::Target,
            Lead::Person,
            Bar::Own,
            LinePress::Nothing,
        )))
    };
    // Widened: the recap and who dealt it, side by side.
    let (tabs, body) = if cx.layout.wide() {
        (
            None,
            Body::Split(Box::new((recap_body(), attackers_body()))),
        )
    } else {
        let body = match up {
            Pane::Spell => recap_body(),
            Pane::Target => attackers_body(),
        };
        (Some((["Recap", "Attackers"], up)), body)
    };
    Insp {
        wide: false,
        head: Head {
            discs: vec![Disc::Player(class, spec)],
            name: vec![cx.piece(shown_name(&drill.label, hide), class)],
            sub: sub.join(", "),
            sweep: None,
        },
        nums,
        acts,
        mit: Vec::new(),
        deaths,
        ability: None,
        graph: None,
        stacks: None,
        tabs,
        body: answered(app, body),
        stale: false,
        note: if cx.bare {
            Some(BARE.to_string())
        } else {
            (events.is_empty() && attackers.is_empty())
                .then(|| "Waiting for the recap…".to_string())
        },
    }
}

/// R24: an enemy, and who hit it.
fn enemy(cx: &Ctx, rows: &[Row], me: Option<usize>) -> Insp {
    let app = cx.app;
    let hide = cx.hide;
    let Some(drill) = app.drill.as_ref() else {
        return Insp::quiet("Select an enemy to see who hit it.");
    };
    let row = me.and_then(|i| rows.get(i));
    let (_, attackers) = app.breakdown();
    let spell = app.drill_spell().cloned();
    let spell_row = app.drill_spell_row();
    let attacker_color = spell_row
        .as_ref()
        .and_then(|r| r.class)
        .map_or(cx.t.accent, gl::Color::of_class);
    let timeline = app
        .drill_timeline()
        .filter(|t| !t.buckets.is_empty() && spell.is_some());
    let graph = timeline.map(|t| {
        let span = span_of(app, t);
        let focus = app.spell_timeline().map(|ft| (ft, attacker_color));
        let window = window_of(app.drill_range(), span);
        let mut curves = focus_curves(cx, t, None, focus, window);
        if let Some(c) = curves.first_mut() {
            c.color = cx.t.hostile;
        }
        graph_of(
            app,
            span,
            app.drill_range(),
            curves,
            Vec::new(),
            Vec::new(),
            None,
            RangeTo::Drill,
        )
    });
    // v33/v38: a zoom's attackers (or one attacker's abilities) are the
    // window's, and the top line says what they add up to.
    let mut graph = graph;
    if let Some(g) = graph.as_mut() {
        let rows: Vec<Row> = match &spell {
            Some(_) => spell_row.iter().cloned().collect(),
            None => attackers.clone(),
        };
        window_tail(g, app, &rows);
    }
    let name = if hide {
        realmless(&drill.label)
    } else {
        drill.label.clone()
    };
    let acts = timeline
        .map(|_| vec![mode_act(app.graph_mode())])
        .unwrap_or_default();
    let (ability, body) = match spell {
        Some((_, label)) => {
            let abilities = realmless_rows(&app.spell_target_rows(), hide);
            let mut l = pane_list(
                cx,
                abilities,
                Kind::Abilities,
                View::EnemyTaken,
                &scoped(cx, "Ability", true),
                Pane::Spell,
                Lead::Spell,
                Bar::Of(attacker_color),
                LinePress::Nothing,
            );
            l.selected = None;
            l.keyed = false;
            l.hover = None;
            (
                Some(Ability {
                    who: name.clone(),
                    who_ink: None,
                    label: shown_name(&label, hide),
                    row: spell_row,
                    view: View::EnemyTaken,
                    tally: Tally::default(),
                }),
                Body::One(Box::new(l)),
            )
        }
        None => (
            None,
            Body::One(Box::new(pane_list(
                cx,
                realmless_rows(&attackers, hide),
                Kind::Targets,
                View::EnemyTaken,
                &scoped(cx, "Attacker", true),
                Pane::Target,
                Lead::Person,
                Bar::Own,
                LinePress::Attacker,
            ))),
        ),
    };
    Insp {
        wide: false,
        head: Head {
            discs: vec![Disc::Enemy],
            name: vec![NamePiece {
                words: name,
                ink: cx.t.ink,
                semibold: true,
            }],
            sub: "Enemy, every unit with this name folded together".to_string(),
            sweep: row.map(|r| against_top(r, rows)),
        },
        nums: row
            .map(|r| player_nums(View::EnemyTaken, rows, r))
            .unwrap_or_default(),
        acts,
        mit: Vec::new(),
        deaths: None,
        ability,
        graph,
        stacks: None,
        tabs: None,
        body: answered(app, body),
        stale: false,
        note: row
            .is_none()
            .then(|| "Waiting for this enemy's numbers…".to_string()),
    }
}

/// R12: the pair, on one scale and one time axis.
fn pair(cx: &Ctx, rows: &[Row]) -> Insp {
    let app = cx.app;
    let hide = cx.hide;
    let picks = app.compare_picks();
    let (Some((a_key, a_label)), Some((b_key, b_label))) = (picks.first(), picks.get(1)) else {
        return Insp::quiet("Pick another player to compare.");
    };
    let sides = app.compare_sides();
    let who = |key: &str, side: Option<&CompareSide>| -> (Option<Class>, Option<Spec>) {
        side.map(|s| (s.total.class, s.total.spec))
            .or_else(|| {
                rows.iter()
                    .find(|r| r.key == key)
                    .map(|r| (r.class, r.spec))
            })
            .unwrap_or((None, None))
    };
    let (a_side, b_side) = (sides.map(|(a, _)| a), sides.map(|(_, b)| b));
    let (a_class, a_spec) = who(a_key, a_side);
    let (b_class, b_spec) = who(b_key, b_side);
    let (a_name, b_name) = (shown_name(a_label, hide), shown_name(b_label, hide));
    let head = Head {
        discs: vec![Disc::Player(a_class, a_spec), Disc::Player(b_class, b_spec)],
        name: vec![
            cx.piece(a_name.clone(), a_class),
            // The joining word reads at 21 px in the second ink.
            NamePiece {
                words: " and ".to_string(),
                ink: cx.t.ink_2,
                semibold: false,
            },
            cx.piece(b_name.clone(), b_class),
        ],
        sub: "One scale, one time axis. Move to swap the second player, v to stop.".to_string(),
        sweep: None,
    };
    let metric = app.compare_view();
    let rate = metric.is_rate();
    let mut nums = Vec::new();
    if let (Some(a), Some(b)) = (a_side, b_side) {
        let (va, vb) = if rate {
            (a.total.per_sec, b.total.per_sec)
        } else {
            (a.total.amount as f64, b.total.amount as f64)
        };
        let unit = if rate { rate_label(metric) } else { "" };
        nums.push(num(&a_name, commas(va.round() as u64), unit));
        nums.push(num(&b_name, commas(vb.round() as u64), unit));
        nums.push(num("Gap", commas((va - vb).abs().round() as u64), ""));
        nums.push(num(
            "Ratio",
            if vb > 0.0 {
                format!("{:.2}×", va / vb)
            } else {
                "—".to_string()
            },
            "",
        ));
    }
    let acts = vec![
        Act {
            glyph: Glyph::Compare,
            words: "Stop comparing".to_string(),
            pressed: true,
            press: Some(Press::PinCompare),
            tip: "Stop comparing (v, Esc)",
        },
        mode_act(app.graph_mode()),
    ];
    let (a_color, b_color) = (cx.class_rgb(a_class), cx.class_rgb(b_class));
    let spell = app.compare_spell().cloned();
    // One colour for both curves is the one case colours cannot tell
    // apart: the second is dashed then, and its swatch with it.
    let dashed = a_color == b_color;
    let graph = sides.map(|(a, b)| {
        let mode = app.graph_mode();
        let span = span_of(app, &a.timeline).max(span_of(app, &b.timeline));
        let window = window_of(app.compare_shown_range(), span);
        let bucket = rate_bucket(window.1.saturating_sub(window.0), a.timeline.bucket_ms);
        let side_curves = |s: &CompareSide, color: gl::Color, ink: Ink, name: &str| {
            let mut out = Vec::new();
            let focus = s.spell_timeline.as_ref().filter(|_| spell.is_some());
            let (points, bucket_ms) = curve(&s.timeline, mode, bucket);
            out.push(Curve {
                name: if focus.is_some() {
                    String::new()
                } else {
                    name.to_string()
                },
                color,
                points,
                bucket_ms,
                ink: if focus.is_some() { Ink::Ghost } else { ink },
            });
            if let Some(ft) = focus {
                let (points, bucket_ms) = curve(ft, mode, bucket);
                out.push(Curve {
                    name: name.to_string(),
                    color,
                    points,
                    bucket_ms,
                    ink,
                });
            }
            out
        };
        let mut curves = side_curves(a, a_color, Ink::Line, &a_name);
        let b_ink = if dashed { Ink::Dashed } else { Ink::Line };
        curves.extend(side_curves(b, b_color, b_ink, &b_name));
        let mut dead = dead_spans(&a.timeline, span, Some(&a_name), &cx.st.roster);
        dead.extend(dead_spans(&b.timeline, span, Some(&b_name), &cx.st.roster));
        let lanes = lanes::pair(
            lanes::lanes(
                &a.timeline.marks,
                a_key,
                a_class,
                &cx.st.roster,
                cx.t.classless,
            ),
            lanes::lanes(
                &b.timeline.marks,
                b_key,
                b_class,
                &cx.st.roster,
                cx.t.classless,
            ),
            &a_name,
            &b_name,
        );
        graph_of(
            app,
            span,
            app.compare_shown_range(),
            curves,
            dead,
            lanes,
            Some(vec![
                (a_name.clone(), a_color, false),
                (b_name.clone(), b_color, dashed),
            ]),
            RangeTo::Compare,
        )
    });
    let ability = spell.as_ref().map(|(_, label)| Ability {
        who: format!("{a_name} and {b_name}"),
        who_ink: None,
        label: shown_name(label, hide),
        row: None,
        view: metric,
        tally: Tally::default(),
    });
    let side_list = |s: &CompareSide, name: &str, class: Option<Class>| List {
        rows: realmless_rows(&s.spells, hide),
        kind: Kind::Pair,
        view: metric,
        note: s
            .mitigation
            .as_ref()
            .map(|m| mitigation_line(m, s.total.amount)),
        head: name.to_string(),
        head_ink: Some(cx.name_ink(class)),
        lead: Lead::Spell,
        bar: Bar::Of(cx.class_rgb(class)),
        selected: spell
            .as_ref()
            .and_then(|(k, _)| s.spells.iter().position(|r| r.key == *k)),
        keyed: false,
        hover: None,
        you: None,
        pane: Pane::Spell,
        pair_hover: cx.st.spell_hover.clone(),
        sort: None,
        sortable: false,
        press: LinePress::Pair,
        side: SIDE_PAIR,
        tree: None,
        hues: HashMap::new(),
        room: match cx.layout {
            Layout::Wide { stage } => wide::pair_room(stage),
            Layout::Column { .. } => Room::Normal,
        },
    };
    let body = match sides {
        Some((a, b)) => Body::Pair(Box::new((
            side_list(a, &a_name, a_class),
            side_list(b, &b_name, b_class),
        ))),
        None => Body::Nothing,
    };
    Insp {
        wide: false,
        head,
        nums,
        acts,
        mit: Vec::new(),
        deaths: None,
        ability,
        graph,
        stacks: None,
        tabs: None,
        body,
        stale: false,
        note: sides
            .is_none()
            .then(|| "Loading the comparison…".to_string()),
    }
}
