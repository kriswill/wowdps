//! The inspector (the prototype's `.insp`): the selected player's drill
//! BESIDE the meter, where a drill used to replace it. To look at a second
//! player the reader moves the selection — `j`/`k`, a click — and the
//! inspector follows (`ClientState`'s opt-in follow-selection: every move
//! re-watches the segment with the new row as the drill, so a snapshot
//! carries the rows and the breakdown at once).
//!
//! Above 1180 px it is a column of 520 px (410 px in a tile, the most of
//! the prototype's 360–520 and 320–410, which a grid always gives it); at
//! 820 px and under the meter is alone and Enter (or a click) pushes the
//! inspector over it with a back button — the old push-and-back.
//!
//! What it shows, top to bottom: the player (their spec disc, their name
//! in their class colour, "Demonology Warlock, you, died 5:45"), the
//! view's numbers (Dps, Damage, their place among their role, Crit …),
//! the actions (compare, talents and gear, the death recap, the graph's
//! mode), the graph with its lanes ([`plot`], [`lanes`]), and the
//! abilities or targets ([`list`]) behind two tabs. The Deaths view's
//! player is their recap; the enemies' is the attackers. `v` pins the
//! selected player and the selection becomes the other half of a pair:
//! both curves on one plot and one scale, the two ability lists side by
//! side, the meter still in sight.
//!
//! Owned data ([`Insp`]) the split's layout draws at whatever width it
//! gets. Window-only: the overlay keeps its own drill and comparison.

mod lanes;
mod list;
mod plot;

pub(crate) use plot::ticks;

use std::collections::HashMap;
use std::rc::Rc;

use iced::widget::{Row as Line, Space, button, column, container, row, scrollable, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{commas, duration, mitigation_line};
use wowdps_model::{
    Class, GraphMode, Mark, MarkKind, MissKind, Mitigation, Pane, Row, Screen, Spec, Timeline, View,
};
use wowdps_proto::{ClientState, CompareSide, DeathWindow};

use crate::fight_head::Place;
use crate::line_icons::{LineIcon, line_icon};
use crate::table::figure;
use crate::theme::{self, size};
use crate::view::{display_name, rate_label, realmless, realmless_rows};
use crate::window::{Gui, Message};

/// The inspector's width beside the meter (`.split{grid-template-
/// columns:minmax(0,1fr) minmax(360px,520px)}`), and in a tile
/// (`minmax(320px,410px)`). The meter's `1fr` may shrink to nothing, so a
/// grid hands the column its most at every width it stands beside the
/// meter (a tile starts at 821 px, a wide window at 1181): the minimums
/// never bind, and the column is simply its maximum.
const WIDE: f32 = 520.0;
const TILE: f32 = 410.0;
/// At this width and under, an ability list keeps its amount, share and
/// crit (`@container insp (max-width: 440px)`).
const NARROW_LIST: f32 = 440.0;
/// The head (`.ihead{padding:14px 16px 12px;gap:9px}`): the disc
/// (`.disc.lg{34px}`), the name (`b{21px 600}`) and what they play
/// (`small{14px}`), the gap between (`.iname{gap:10px}`).
const HEAD_PAD: iced::Padding = iced::Padding {
    top: 14.0,
    right: 16.0,
    bottom: 12.0,
    left: 16.0,
};
const HEAD_GAP: f32 = 9.0;
const DISC: f32 = 34.0;
const NAME_GAP: f32 = 10.0;
const NAME_PX: f32 = 21.0;
const NAME_LINE: f32 = 1.1;
const SUB_PX: f32 = 14.0;
/// The numbers (`.inums{gap:8px}`, `.inum .l{13px}`, `.v{17px 500}`,
/// `.v small{13px}`).
const NUMS_GAP: f32 = 8.0;
const NUM_LABEL_PX: f32 = 13.0;
const NUM_PX: f32 = 17.0;
const NUM_SMALL_PX: f32 = 13.0;
/// The actions (`.iacts{gap:6px}`, `.btn{height:26px;padding-inline:9px;
/// border-radius:5px;font-size:13.5px;gap:6px}`).
const ACTS_GAP: f32 = 6.0;
const BTN_H: f32 = 26.0;
const BTN_PAD_X: f32 = 9.0;
const BTN_RADIUS: f32 = 5.0;
const BTN_PX: f32 = 13.5;
const BTN_ICON: f32 = 14.0;
const BTN_GAP: f32 = 6.0;
/// A section's frame (`.igraph{padding:10px 16px}`) and the graph's top
/// line (`.ig-top{font-size:13px;margin-bottom:4px}`).
const SECTION_PAD: iced::Padding = iced::Padding {
    top: 10.0,
    right: 16.0,
    bottom: 10.0,
    left: 16.0,
};
const TOP_PX: f32 = 13.0;
const TOP_GAP: f32 = 4.0;
/// The tabs (`.itabs{gap:2px;padding:0 10px}`, `.itab{height:34px;
/// padding-inline:8px;font-weight:500}`).
const TAB_H: f32 = 34.0;
const TAB_PAD_X: f32 = 8.0;
const TABS_PAD_X: f32 = 10.0;
const TAB_GAP: f32 = 2.0;
const TAB_PX: f32 = 14.0;
/// The mitigation line (`.mit{gap:6px 14px;font-size:13.5px}`), and the
/// room between a word and its figure (`.mit b` after a space).
const MIT_PX: f32 = 13.5;
const MIT_GAP: f32 = 14.0;
const MIT_ROW_GAP: f32 = 6.0;
const MIT_WORD_GAP: f32 = 4.0;
/// A number's label over its value (`.inum`), and a value's tail after it.
const NUM_LINE_GAP: f32 = 1.0;
const NUM_TAIL_GAP: f32 = 4.0;
/// The line of death chips, the stack matrix and the recap's heading
/// line, each in the column's 16 px sides.
const CHIPS_PAD: iced::Padding = iced::Padding {
    top: 8.0,
    right: 16.0,
    bottom: 8.0,
    left: 16.0,
};
const MATRIX_PAD: iced::Padding = iced::Padding {
    top: 4.0,
    right: 16.0,
    bottom: 4.0,
    left: 16.0,
};
/// The recap (`.recap{padding:4px 0 12px}`): its heading line
/// (`.ihrow{padding:8px 16px 4px;font-size:13px}`) and each row (`.rrow{
/// grid-template-columns:78px minmax(0,1fr) 88px;column-gap:10px;padding:
/// 0 16px;height:30px;font-size:14px}`) — the change right-aligned in its
/// 78 px, the event and its source (`small`, a size down, in the faint
/// ink), the health after it as a 6 px bar (`.hp{height:6px;border-radius:
/// 3px;background:rgba(255,255,255,.07)}`) 88 px long.
const RECAP_HEAD_PAD: iced::Padding = iced::Padding {
    top: 8.0,
    right: 16.0,
    bottom: 4.0,
    left: 16.0,
};
const RECAP_PAD: iced::Padding = iced::Padding {
    top: 4.0,
    right: 0.0,
    bottom: 12.0,
    left: 0.0,
};
const RECAP_ROW_H: f32 = 30.0;
const RECAP_SIDE: f32 = 16.0;
const RECAP_GAP: f32 = 10.0;
const RECAP_CHANGE_W: f32 = 78.0;
const RECAP_HP_W: f32 = 88.0;
const RECAP_HP_H: f32 = 6.0;
const RECAP_HP_RADIUS: f32 = 3.0;
const RECAP_PX: f32 = 14.0;
const RECAP_SRC_PX: f32 = 12.0;
const RECAP_SRC_GAP: f32 = 4.0;
const RECAP_HP_TRACK: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.07);
/// The health bar's colour (`.hp.low` under 15 %, `.hp.crit` under 3 %),
/// and the least of it a living player's bar shows (`max(p, 1.5)`).
const RECAP_LOW: f32 = 0.15;
const RECAP_CRIT: f32 = 0.03;
const RECAP_HP_MIN: f32 = 0.015;
/// The killing row's tint (`.rrow.kill{background:rgba(255,92,99,.09)}`).
const RECAP_KILL_ALPHA: f32 = 0.09;
/// v35: the recap's time column, before the change — each event's time
/// before the death (`offset_ms`), right-aligned in the faint ink, as
/// [`crate::deaths::before`] words it: "−4.20s". It and the change column
/// beside it are as wide as their widest figure, up to these ([`RecapCols`]).
const RECAP_TIME_W: f32 = 44.0;
const RECAP_TIME_PX: f32 = 13.0;
/// The health bar in a tile's inspector (410 px): shorter, so the events'
/// names keep their room.
const RECAP_HP_W_TILE: f32 = 56.0;
/// Health the log did not report: a dash before the track, dimmed to this
/// share of its ink.
const RECAP_HP_UNKNOWN: &str = "\u{2014}";
const RECAP_HP_DIM: f32 = 0.5;
/// The insight line after a recap (`.insight{margin:10px 16px 0;padding:
/// 8px 10px;border-radius:6px;background:color-mix(in srgb,var(--you) 10%,
/// transparent);font-size:14px}`, `b{color:var(--you-text)}`).
const INSIGHT_MARGIN: iced::Padding = iced::Padding {
    top: 10.0,
    right: 16.0,
    bottom: 0.0,
    left: 16.0,
};
const INSIGHT_PAD: [f32; 2] = [8.0, 10.0];
const INSIGHT_RADIUS: f32 = 6.0;
const INSIGHT_PX: f32 = 14.0;
const INSIGHT_WASH: f32 = 0.10;
/// The quiet line (`.note{margin:10px 16px;border-left:2px solid}`): its
/// frame, its edge's width and height, and the gap after the edge.
const NOTE_PAD: iced::Padding = iced::Padding {
    top: 10.0,
    right: 16.0,
    bottom: 10.0,
    left: 16.0,
};
const NOTE_EDGE: f32 = 2.0;
const NOTE_EDGE_H: f32 = 18.0;
const NOTE_GAP: f32 = 10.0;
/// The graph's legend (`.leg{gap:12px}`, `.leg i{width:10px;height:3px;
/// margin-right:5px}`), a dashed swatch's two dashes.
const LEGEND_GAP: f32 = 12.0;
const SWATCH_W: f32 = 10.0;
const SWATCH_H: f32 = 3.0;
const SWATCH_GAP: f32 = 5.0;
const DASH_W: f32 = 4.0;
const DASH_GAP: f32 = 2.0;
/// A tab's underline (`.itab{border-bottom:2px solid}`).
const TAB_LINE: f32 = 2.0;
/// The ability drill's crumb: who (`13px`), the "▸" (`11px`), the
/// ability's icon and name, its school; and the gaps between.
const CRUMB_PX: f32 = 13.0;
const CRUMB_MARK_PX: f32 = 11.0;
const CRUMB_ICON: f32 = 14.0;
const CRUMB_NAME_PX: f32 = 14.0;
const CRUMB_GAP: f32 = 6.0;
const STRIP_GAP: f32 = 8.0;
/// How much of the panel's surface veils a body held from the last
/// player while this one's is on its way.
const STALE_VEIL: f32 = 0.55;
/// The rate curve's buckets: the prototype's 10 s ("per second, 10 s"),
/// drawn through its spline — finer only when the stretch on show is too
/// short to hold [`RATE_POINTS`] of them (a zoom, a short pull), down to
/// the timeline's own second.
const RATE_BUCKET_MS: u32 = 10_000;
const RATE_POINTS: u32 = 40;

/// How wide the window is, by the prototype's breakpoints: above 1180 px
/// the numbers stand four across, under it two; at 820 px and under the
/// inspector is pushed rather than beside the meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    Wide,
    Tile,
    Narrow,
}

impl Fit {
    pub(crate) fn of(window: f32) -> Self {
        if window <= theme::NARROW_WINDOW {
            Fit::Narrow
        } else if window <= theme::TILE_WINDOW {
            Fit::Tile
        } else {
            Fit::Wide
        }
    }
}

/// The inspector's width beside the meter in a window `window` wide —
/// `None` at 820 px and under, where the meter is alone and the inspector
/// is pushed over it instead.
pub(crate) fn beside(window: f32) -> Option<f32> {
    match Fit::of(window) {
        Fit::Narrow => None,
        Fit::Tile => Some(TILE),
        Fit::Wide => Some(WIDE),
    }
}

/// "5:45": a moment in the fight.
pub(crate) fn mmss(ms: u32) -> String {
    duration(i64::from(ms))
}

// ---- who is who --------------------------------------------------------------

/// Everyone the window has seen on a meter, by guid: their name and class
/// — what a lane's span is coloured by when its caster is not the player
/// (the Heroism's Shaman is on the Damage rows, and perhaps not on the
/// Taken ones the inspector shows). A guid's class never changes, so the
/// roster only grows, and a class learned is never forgotten.
///
/// It answers by NAME too: a drill's target rows are keyed by name and
/// wear the DRILLED player's class (the model's "drilldown rows alike"),
/// so who a target is — a player in their colour, or a creature — is the
/// roster's to say.
#[derive(Debug, Clone, Default)]
pub(crate) struct Roster {
    people: HashMap<String, (String, Option<Class>)>,
    names: HashMap<String, (Option<Class>, Option<Spec>)>,
}

impl Roster {
    /// Take in a meter's rows (players: never the enemies' view).
    pub(crate) fn observe(&mut self, rows: &[Row]) {
        for r in rows {
            match self.people.get_mut(&r.key) {
                Some(have) if have.1.is_none() && r.class.is_some() => have.1 = r.class,
                Some(_) => {}
                None => {
                    self.people
                        .insert(r.key.clone(), (r.label.clone(), r.class));
                }
            }
            // Every tick walks every row: a name already known is looked up,
            // and only a new one is allocated for.
            if !self.names.contains_key(&r.label) {
                self.names.insert(r.label.clone(), (None, None));
            }
            if let Some(named) = self.names.get_mut(&r.label) {
                if r.class.is_some() {
                    named.0 = r.class;
                }
                if r.spec.is_some() {
                    named.1 = r.spec;
                }
            }
        }
    }

    /// Their name ("Name-Realm") and class, when seen.
    pub(crate) fn get(&self, guid: &str) -> Option<(&str, Option<Class>)> {
        self.people.get(guid).map(|(n, c)| (n.as_str(), *c))
    }

    /// `rows` — a drill's targets, keyed by name — each wearing its OWN
    /// class and spec: a player the roster has seen in theirs, anything
    /// else none (a creature).
    pub(crate) fn as_themselves(&self, rows: &[Row]) -> Vec<Row> {
        rows.iter()
            .map(|r| {
                let (class, spec) = self.names.get(&r.label).copied().unwrap_or_default();
                Row {
                    class,
                    spec,
                    ..r.clone()
                }
            })
            .collect()
    }
}

// ---- what the inspector says --------------------------------------------------

/// A disc in the head.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Disc {
    Player(Option<Class>, Option<Spec>),
    Enemy,
}

/// The head: the disc(s), the name — in pieces, for a pair's "A and B" —
/// and the line under it.
#[derive(Debug, Clone)]
struct Head {
    discs: Vec<Disc>,
    name: Vec<(String, Color, Font)>,
    sub: String,
}

/// One number (`.inum`): its label, its value and a quieter tail
/// ("17th" + "of 19 dps").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Num {
    pub label: String,
    pub value: String,
    pub small: String,
}

fn num(label: &str, value: String, small: &str) -> Num {
    Num {
        label: label.to_string(),
        value,
        small: small.to_string(),
    }
}

/// What an inert Compare says on a stored pull.
pub(crate) const NO_COMPARE_TIP: &str = "Comparing needs the pull's log: the store keeps no pair";

/// One action (`.btn`): its glyph, its words, pressed or not, what it
/// does — `None` for one this pull cannot do, drawn inert where it would
/// be, so the row keeps its shape — and the tooltip that names its key
/// (`title="… (v)"`) or says why it is inert.
#[derive(Debug, Clone)]
struct Act {
    icon: LineIcon,
    words: String,
    pressed: bool,
    press: Option<Message>,
    tip: &'static str,
}

/// What the graph's top line says at its end: the curve's mode, or the
/// pair's legend.
#[derive(Debug, Clone)]
enum Tail {
    Words(String),
    /// (name, colour, dashed)
    Legend(Vec<(String, Color, bool)>),
}

/// The graph, as data ([`plot::Plot`] is made from it at draw time).
#[derive(Debug, Clone)]
struct Graph {
    lead: String,
    tail: Tail,
    window: (u32, u32),
    peak: f64,
    curves: Vec<plot::Curve>,
    dead: Vec<plot::Dead>,
    lanes: Vec<lanes::Row>,
    total: bool,
    word: &'static str,
    on_range: fn(Option<(u32, u32)>) -> Message,
}

/// The ability drill's strip, or a pair's: who ▸ what, and its numbers.
#[derive(Debug, Clone)]
struct Ability {
    who: String,
    who_ink: Option<Color>,
    label: String,
    row: Option<Row>,
    view: View,
}

/// R21: the stack ledger, behind its tab.
#[derive(Debug, Clone)]
struct Stacks {
    on: bool,
    ledger: Vec<crate::taken::Matrix>,
    dropped: u32,
}

/// R9: the player's death windows — a chip for each, when there is more
/// than one to choose between.
#[derive(Debug, Clone)]
struct Deaths {
    windows: Vec<DeathWindow>,
    shown: Option<u32>,
    dropped: u32,
}

/// R9: a death's last events as the recap draws them.
#[derive(Debug, Clone)]
struct Recap {
    /// The events, NEWEST first as the daemon sends them (the list draws
    /// them oldest first, the way the death happened).
    rows: Vec<Row>,
    /// The player who died, as their events' sources name them — what
    /// their own events say instead ("yours" for the owner, else "self").
    who: String,
    yours: bool,
    class: Option<Class>,
}

/// What stands under the tabs.
#[derive(Debug, Clone)]
enum Body {
    Nothing,
    One(list::List),
    /// R9: a death's last events.
    Recap(Recap),
    /// A comparison's two ability lists.
    Pair(Box<(list::List, list::List)>),
}

/// Everything the inspector draws, owned: what [`Insp::of`] read from the
/// window, drawn by [`Insp::view`] at the width the layout gives it.
#[derive(Debug, Clone)]
pub(crate) struct Insp {
    head: Head,
    pub(crate) nums: Vec<Num>,
    acts: Vec<Act>,
    /// R17: the mitigation line, as (lead, figure, tail) pieces.
    mit: Vec<(String, String, String)>,
    deaths: Option<Deaths>,
    ability: Option<Ability>,
    graph: Option<Graph>,
    stacks: Option<Stacks>,
    /// The two tabs' words and which is up; `None` for a single list.
    tabs: Option<([&'static str; 2], Pane)>,
    body: Body,
    /// The graph, the mitigation line and the lists are the LAST player's,
    /// held while this one's breakdown is on its way ([`Held`]): drawn
    /// dimmed, so the column keeps its height as the selection moves.
    stale: bool,
    /// A quiet line (`.note`): nothing to show yet, and why.
    note: Option<String>,
    accent: theme::Accent,
}

/// What the inspector drew for a player once their breakdown was in, held
/// by the window: a move of the selection drops the breakdown in hand
/// (never the last player's lists under the new name), and until the new
/// one lands the column would collapse to its head and spring back — the
/// layout jumping and the scroll lost on every j/k. The held body stands
/// in, dimmed, under the new player's name.
#[derive(Debug, Clone)]
pub(crate) struct Held {
    key: String,
    view: View,
    pane: Pane,
    mode: GraphMode,
    /// The snapshot it was drawn from (`ClientState::snapshot_gen`): a live
    /// pull moves the breakdown under the same player, view and list.
    generation: u64,
    mit: Vec<(String, String, String)>,
    graph: Option<Graph>,
    body: Body,
}

impl Held {
    /// What to hold of the window's inspector now: a player's answered
    /// drill (not an ability's, a recap, an enemy or a pair), else `None`.
    pub(crate) fn of(state: &Gui) -> Option<Self> {
        let app = state.fight();
        let drill = app.drill.as_ref()?;
        let held = app.screen == Screen::Meter
            && drill.spell.is_none()
            && app.drill_breakdown().is_some()
            && !matches!(app.view, View::Deaths | View::EnemyTaken);
        if !held {
            return None;
        }
        let insp = Insp::of(state);
        Some(Held {
            key: drill.key.clone(),
            view: app.view,
            pane: drill.pane,
            mode: app.graph_mode(),
            generation: app.snapshot_gen(),
            mit: insp.mit,
            graph: insp.graph,
            body: insp.body,
        })
    }

    /// Is this what the window's inspector would hold now — the same
    /// player, view, list and mode, drawn from the snapshot in hand? When
    /// it is, holding it again changes nothing. A new snapshot makes it
    /// stale even for the same player: a live pull moves their numbers,
    /// and a push for the last player that raced the Watch (a `Snapshot`
    /// names no drill) is replaced by the reply right behind it.
    pub(crate) fn current(&self, app: &ClientState) -> bool {
        app.drill
            .as_ref()
            .is_some_and(|d| d.key == self.key && d.pane == self.pane && d.spell.is_none())
            && app.view == self.view
            && app.graph_mode() == self.mode
            && app.snapshot_gen() == self.generation
    }

    /// Does it stand in for `app`'s drill — the same view, list and mode,
    /// whoever the player?
    fn fits(&self, app: &ClientState) -> bool {
        app.drill
            .as_ref()
            .is_some_and(|d| d.pane == self.pane && d.spell.is_none())
            && app.view == self.view
            && app.graph_mode() == self.mode
    }
}

impl Insp {
    /// The inspector for the window as it stands.
    pub(crate) fn of(state: &Gui) -> Self {
        let app = state.fight();
        let rows = app.rows();
        if app.screen == Screen::Compare {
            return pair(state, &rows);
        }
        let Some(drill) = app.drill.as_ref() else {
            let words = if rows.is_empty() {
                "Nothing to inspect yet."
            } else {
                "Select a player to inspect them here."
            };
            return Insp::quiet(words, state.accent);
        };
        let me = rows.iter().position(|r| r.key == drill.key);
        match app.view {
            View::Deaths => recap(state, &rows, me),
            View::EnemyTaken => enemy(state, &rows, me),
            _ => player(state, &rows, me),
        }
    }

    /// Is the body the last player's, standing in ([`Held`])?
    #[cfg(test)]
    pub(crate) fn stale(&self) -> bool {
        self.stale
    }

    /// An inspector with nothing in it but `words`.
    fn quiet(words: &str, accent: theme::Accent) -> Self {
        Insp {
            head: Head {
                discs: Vec::new(),
                name: Vec::new(),
                sub: String::new(),
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
            accent,
        }
    }

    /// The inspector `width` wide, in a window of `fit`: a scrolling
    /// column on the panel's surface. `pushed` is a narrow window's —
    /// over the meter, with a back button.
    pub(crate) fn view(&self, width: f32, fit: Fit, pushed: bool) -> Element<'static, Message> {
        let mut body = column![];
        if !self.head.name.is_empty() {
            body = body.push(self.head_block(fit, pushed));
        } else if pushed {
            // Nothing to inspect, and still a way back.
            body = body.push(container(back_button()).padding(HEAD_PAD));
        }
        if let Some(a) = &self.ability {
            body = body.push(section(ability_strip(a, fit), true));
        }
        if !self.mit.is_empty() {
            body = body.push(dim(section(mit_line(&self.mit), true), self.stale));
        }
        if let Some(g) = &self.graph {
            body = body.push(dim(section(graph_block(g), true), self.stale));
        }
        if let Some(d) = &self.deaths
            && let Some(chips) = crate::taken::death_chips(
                &d.windows,
                d.shown,
                d.dropped,
                self.accent,
                Message::PickDeath,
            )
        {
            body = body.push(container(chips).padding(CHIPS_PAD));
        }
        if let Some((words, up)) = self.tabs {
            let stacks = self.stacks.as_ref().map(|s| s.on);
            body = body.push(tabs(words, up, stacks, self.accent));
        }
        // R21: the Stacks tab shows the ledger in the list's place.
        let stacked = self
            .stacks
            .as_ref()
            .filter(|s| s.on)
            .and_then(|s| crate::taken::stack_matrix::<Message>(&s.ledger, s.dropped));
        match stacked {
            Some(matrix) => body = body.push(container(matrix).padding(MATRIX_PAD)),
            None => {
                let list = match &self.body {
                    Body::Nothing => Element::from(Space::new()),
                    Body::One(l) => l.view(width <= NARROW_LIST),
                    Body::Recap(r) => recap_list(r, fit),
                    Body::Pair(pair) => {
                        // Side by side only where each keeps a name column
                        // a reader can read ([`pair_stacked`]).
                        let stacked = pair_stacked(fit);
                        let (a, b) = (pair.0.view(stacked), pair.1.view(stacked));
                        if stacked {
                            column![a, crate::nav::hairline::<Message>(), b].into()
                        } else {
                            row![
                                container(a).width(Length::FillPortion(1)),
                                crate::nav::vrule::<Message>(),
                                container(b).width(Length::FillPortion(1)),
                            ]
                            .height(Length::Shrink)
                            .into()
                        }
                    }
                };
                body = body.push(dim(list, self.stale));
            }
        }
        if let Some(note) = &self.note {
            body = body.push(note_line(note));
        }
        container(
            // The scrollbar keeps its own lane, as the meter's does: the
            // last column is never under it.
            scrollable(crate::view::scroll_clear(body.width(Length::Fill)))
                .id(scroll_id())
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::SURFACE.into()),
            ..container::Style::default()
        })
        .into()
    }

    /// The head (`.ihead`): a narrow window's back button, the disc(s),
    /// the name and the line under it; the numbers; the actions.
    fn head_block(&self, fit: Fit, pushed: bool) -> Element<'static, Message> {
        let mut who = row![].spacing(NAME_GAP).align_y(iced::Alignment::Center);
        if pushed {
            who = who.push(back_button());
        }
        for d in &self.head.discs {
            who = who.push(match *d {
                Disc::Player(class, spec) => {
                    crate::compare::class_icon::<Message>(class, spec, None, DISC)
                }
                Disc::Enemy => list::foe_disc::<Message>(DISC),
            });
        }
        let mut words = column![name_line(&self.head.name)];
        if !self.head.sub.is_empty() {
            words = words.push(
                // A narrow inspector wraps it rather than cut it off.
                text(self.head.sub.clone()).size(SUB_PX).color(theme::INK_2),
            );
        }
        who = who.push(container(words).clip(true).width(Length::Fill));
        let mut head = column![who].spacing(HEAD_GAP);
        if !self.nums.is_empty() {
            head = head.push(nums_grid(&self.nums, per_row(fit)));
        }
        if !self.acts.is_empty() {
            let mut acts = Line::new().spacing(ACTS_GAP);
            for a in &self.acts {
                acts = acts.push(act_button(a, self.accent));
            }
            head = head.push(acts.wrap().vertical_spacing(ACTS_GAP));
        }
        column![
            container(head).padding(HEAD_PAD).width(Length::Fill),
            crate::nav::hairline::<Message>(),
        ]
        .into()
    }
}

/// The head's name (`.iname b{…white-space:nowrap;overflow:hidden;
/// text-overflow:ellipsis}`): a name that does not fit ends in "…", never
/// cut through a glyph. A pair's "A and B" keeps the joining word and the
/// second name whole while the first gives way, then the second — each
/// ellipsised in turn, as one line.
fn name_line(pieces: &[(String, Color, Font)]) -> Element<'static, Message> {
    let piece = |(words, ink, font): &(String, Color, Font)| {
        crate::ellipsis::ellipsis(words.clone())
            .size(NAME_PX)
            .font(*font)
            .color(*ink)
            .line_height(text::LineHeight::Relative(NAME_LINE))
    };
    let Some((last, before)) = pieces.split_last() else {
        return Space::new().into();
    };
    let mut line = row![].align_y(iced::Alignment::End);
    for (i, p) in before.iter().enumerate() {
        // Each leaves room for everything after it, so the pieces stay in
        // one line and the earlier one gives way first.
        let rest: Vec<(String, f32, Font)> = pieces
            .iter()
            .skip(i + 1)
            .map(|(w, _, f)| (w.clone(), NAME_PX, *f))
            .collect();
        line = line.push(piece(p).leaving(rest, 0.0));
    }
    line.push(piece(last)).into()
}

/// Does a pair stack its two lists (A over B)? Everywhere but a wide
/// window: side by side (`.cmp2`) a tile's 410 px halves cut every ability
/// name to a few letters.
fn pair_stacked(fit: Fit) -> bool {
    fit != Fit::Wide
}

/// The numbers four across in a wide window, two in a tile or a narrow one
/// (`@container app (max-width: 1180px){.inums{…repeat(2,…)}}`).
fn per_row(fit: Fit) -> usize {
    if fit == Fit::Wide { 4 } else { 2 }
}

/// The pushed inspector's way back to the meter (`.iback`).
fn back_button() -> Element<'static, Message> {
    crate::nav::icon_button(
        LineIcon::ChevronLeft,
        Some(Message::Uninspect),
        Some(back_id()),
    )
}

/// `el`, dimmed when `stale` — the last player's, standing in until this
/// one's lands — under a veil of the panel's own surface.
fn dim(el: Element<'static, Message>, stale: bool) -> Element<'static, Message> {
    if !stale {
        return el;
    }
    iced::widget::stack![
        el,
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(
                    Color {
                        a: STALE_VEIL,
                        ..theme::SURFACE
                    }
                    .into()
                ),
                ..container::Style::default()
            }),
    ]
    .into()
}

/// The inspector's scrollable, so a test can find it.
pub(crate) fn scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("inspector")
}

/// The pushed inspector's back button.
pub(crate) fn back_id() -> iced::widget::Id {
    iced::widget::Id::new("inspector-back")
}

/// The row the keys are on in the inspector's list — what the window keeps
/// in sight as j/k walk it.
pub(crate) fn keyed_row_id() -> iced::widget::Id {
    iced::widget::Id::new("inspector-keyed-row")
}

/// The end of a recap — the killing blow's row, what came after it and the
/// insight — what an opened death brings into sight (R25: the window's
/// `reveal_death`).
pub(crate) fn recap_kill_id() -> iced::widget::Id {
    iced::widget::Id::new("inspector-recap-kill")
}

// ---- the panels ----------------------------------------------------------------

/// A player's name as the window shows names: realm off when the option
/// says so.
fn shown_name(label: &str, hide_realms: bool) -> String {
    if hide_realms {
        display_name(label).to_string()
    } else {
        label.to_string()
    }
}

/// A class colour as a NAME: lifted to read on the panel.
fn name_ink(class: Option<Class>) -> Color {
    class.map_or(theme::INK, theme::class_text)
}

/// "Demonology Warlock".
fn plays(class: Option<Class>, spec: Option<Spec>) -> Option<String> {
    match (spec, class) {
        (Some(s), _) => Some(format!("{} {}", s.name(), s.class().name())),
        (None, Some(c)) => Some(c.name().to_string()),
        (None, None) => None,
    }
}

/// R25: a death as the recap's head words it (the prototype's
/// `recapPanel`): "died 5:45 to Coalesced Venom", "died 5:15 as Purgatory
/// ran out", "died 6:01, no damage logged" — then ", rezzed 2:03 by
/// Soundscape" when someone raised them (their own spell's name for a
/// self-rez: "by Reincarnation").
fn died_words(d: &wowdps_model::RaidDeath, hide_realms: bool) -> String {
    let w = crate::deaths::words(d, hide_realms);
    let at = duration(d.at_ms);
    let mut words = if w.note {
        format!("died {at}, {}", w.blow.to_lowercase())
    } else if w.cheat {
        format!("died {at} as {}", w.blow)
    } else {
        format!("died {at} to {}", w.blow)
    };
    if let Some(rez) = &d.rez {
        let by = if rez.by == d.guid {
            rez.spell.clone()
        } else if hide_realms {
            display_name(&rez.by_name).to_string()
        } else {
            rez.by_name.clone()
        };
        words.push_str(&format!(", rezzed {} by {by}", duration(rez.at_ms)));
    }
    words
}

/// "died 5:45", "died 5:45, rezzed by Gennar" — an R23 death span as the
/// hatch and the head word it. `names` resolves the rezzer.
fn death_words(m: &Mark, roster: &Roster) -> String {
    let mut words = format!("died {}", mmss(m.at_ms.max(0) as u32));
    let spell = m
        .label
        .strip_prefix("Death (")
        .and_then(|s| s.strip_suffix(')'));
    if !m.src.is_empty() {
        // A rezzer no meter named (a pet's guid, someone never on screen)
        // goes unnamed: a guid's tail is no name a reader knows.
        match roster.get(&m.src) {
            Some((n, _)) => words.push_str(&format!(", rezzed by {}", display_name(n))),
            None => words.push_str(", rezzed"),
        }
    } else if let Some(spell) = spell {
        // A self-rez (an Ankh, a Soulstone they clicked) names the spell.
        words.push_str(&format!(", {spell}"));
    }
    words
}

/// The deaths on a timeline (R23), as the plot hatches them: from the
/// death to the rez that ended it, or — with none — to the fight's end.
/// R23 also closes a span at the first thing only the living do, and a
/// dead warlock's DoTs still tick in their name: without a rez the span's
/// own end is a guess, and the hatch does not draw one.
fn dead_spans(t: &Timeline, end_ms: u32, who: Option<&str>, roster: &Roster) -> Vec<plot::Dead> {
    t.marks
        .iter()
        .filter(|m| m.kind == MarkKind::Death)
        .map(|m| {
            let words = death_words(m, roster);
            let rezzed = !m.src.is_empty() || m.label.starts_with("Death (");
            let end = if rezzed {
                m.at_ms + m.dur_ms.max(0)
            } else {
                i64::from(end_ms)
            };
            plot::Dead {
                at_ms: m.at_ms,
                end_ms: end.min(i64::from(end_ms)),
                words: match who {
                    Some(who) => format!("{who} {words}"),
                    None => words,
                },
            }
        })
        .collect()
}

/// The rate curve's bucket for a window `window_ms` long over a timeline
/// of `base_ms` buckets: [`RATE_BUCKET_MS`], or as fine as keeps
/// [`RATE_POINTS`] in the window — a whole number of the timeline's own,
/// and never finer than one.
fn rate_bucket(window_ms: u32, base_ms: u32) -> u32 {
    let base = base_ms.max(1);
    let want = (window_ms / RATE_POINTS).clamp(base, RATE_BUCKET_MS.max(base));
    (want / base).max(1) * base
}

/// The curve a mode draws, as (values, their bucket's ms): the rate in
/// `bucket_ms` buckets ([`bucket_rate`]), or the running total on the
/// timeline's own grid.
fn curve(t: &Timeline, mode: GraphMode, bucket_ms: u32) -> (Vec<f64>, u32) {
    match mode {
        GraphMode::Dps => bucket_rate(t, bucket_ms),
        GraphMode::Total => (
            t.cumulative().into_iter().map(|v| v as f64).collect(),
            t.bucket_ms,
        ),
    }
}

/// The rate per second in buckets of `bucket_ms`, as the prototype's curve
/// reads (10 s buckets, which the plot draws through its spline): each
/// bucket's sum over the seconds it spans — the last one over only the
/// seconds the fight gave it, so the curve ends at a real rate.
fn bucket_rate(t: &Timeline, bucket_ms: u32) -> (Vec<f64>, u32) {
    if t.bucket_ms == 0 || t.buckets.is_empty() {
        return (Vec::new(), bucket_ms.max(1));
    }
    let per = (bucket_ms / t.bucket_ms).max(1) as usize;
    let secs = f64::from(t.bucket_ms) / 1000.0;
    let points = t
        .buckets
        .chunks(per)
        .map(|c| c.iter().sum::<u64>() as f64 / (c.len() as f64 * secs))
        .collect();
    (points, per as u32 * t.bucket_ms)
}

/// The stretch of the fight a graph shows: a zoom, or the whole of it.
fn window_of(shown: Option<(u32, u32)>, span: u32) -> (u32, u32) {
    shown.filter(|(lo, hi)| hi > lo).unwrap_or((0, span))
}

/// The fight's span on the graph's axis: its duration, or the timeline's
/// own length where that is longer (a visit's Σ runs the visit's clock).
fn span_of(app: &ClientState, t: &Timeline) -> u32 {
    let clock = app.duration_ms().max(0) as u64;
    let grid = t.buckets.len() as u64 * u64::from(t.bucket_ms);
    clock.max(grid).clamp(1, u64::from(u32::MAX)) as u32
}

/// The highest point of `curves` inside `window`.
fn peak_in(curves: &[plot::Curve], window: (u32, u32)) -> f64 {
    curves
        .iter()
        .flat_map(|c| {
            let b = u64::from(c.bucket_ms.max(1));
            c.points.iter().enumerate().filter_map(move |(i, v)| {
                let at = i as u64 * b;
                (at + b > u64::from(window.0) && at <= u64::from(window.1)).then_some(*v)
            })
        })
        .fold(0.0, f64::max)
}

/// What a running total of `view` is called: "Damage so far".
fn total_word(view: View) -> &'static str {
    match view {
        View::Healing => "Healing",
        View::Taken => "Taken",
        View::EnemyTaken => "Damage taken",
        _ => "Damage",
    }
}

/// The graph for `curves`: its words, its window (a zoom, or the fight),
/// its one scale.
#[allow(clippy::too_many_arguments)]
fn graph_of(
    app: &ClientState,
    span: u32,
    shown: Option<(u32, u32)>,
    curves: Vec<plot::Curve>,
    dead: Vec<plot::Dead>,
    lanes: Vec<lanes::Row>,
    legend: Option<Vec<(String, Color, bool)>>,
    on_range: fn(Option<(u32, u32)>) -> Message,
) -> Graph {
    let mode = app.graph_mode();
    let window = window_of(shown, span);
    let peak = peak_in(&curves, window);
    let word = rate_label(app.view);
    let lead = match mode {
        GraphMode::Dps => word.to_string(),
        GraphMode::Total => format!("{} so far", total_word(app.view)),
    };
    // With lanes the plot's gutter is its scale — the peak at its top, 0
    // at its baseline — so the top line names the measure alone; without
    // them it says the peak, as the prototype's `.ig-top` does.
    let lead = if lanes.is_empty() {
        format!("{lead}, peak {}", figure(peak.round() as u64))
    } else {
        lead
    };
    // The rate's bucket, as the curves were cut ("per second, 10 s").
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
        on_range,
    }
}

/// The view's numbers for the player on `row` of `rows`.
fn player_nums(view: View, rows: &[Row], me: &Row) -> Vec<Num> {
    let place = || {
        let p = Place::of(rows, me);
        num("Rank", crate::fight_head::ordinal(p.place), &p.tail())
    };
    let pct = |v: f64, d: usize| format!("{v:.d$}%");
    match view {
        View::Damage => vec![
            num("Dps", commas(me.per_sec.round() as u64), ""),
            num("Damage", figure(me.amount), ""),
            place(),
            num("Crit", pct(me.crit_pct(), 1), ""),
        ],
        // Everyone's place among their own role, as the prototype's
        // `roleRank` has it: a healer's among the healers ("1st of 4
        // healers"), a dps's healing among the dps.
        View::Healing => vec![
            num("Hps", commas(me.per_sec.round() as u64), ""),
            num("Healing", figure(me.amount), ""),
            num("Overheal", pct(crate::table::overheal_pct(me), 0), ""),
            place(),
        ],
        View::Taken => vec![
            num("Dtps", commas(me.per_sec.round() as u64), ""),
            num("Taken", figure(me.amount), ""),
            num("Absorbed", figure(me.extra), ""),
            num("Share", pct(me.pct, 1), ""),
        ],
        View::EnemyTaken => vec![
            num("Damage taken", figure(me.amount), ""),
            num("Per sec", figure(me.per_sec.round() as u64), ""),
            num("Share", pct(me.pct, 1), ""),
            num("Crit", pct(me.crit_pct(), 1), ""),
        ],
        View::Deaths | View::Interrupts | View::CrowdControl | View::Dispels => vec![
            num(crate::view::window_view_name(view), commas(me.amount), ""),
            num("Share", pct(me.pct, 1), ""),
        ],
    }
}

/// An ability's figures, as the inspector's numbers say them (`.inum`):
/// its total, its share of the player, its hits, crit and average hit,
/// and what the view calls its extra (overkill, overheal, absorbed).
fn ability_nums(r: &Row, view: View) -> Vec<Num> {
    let known = |v: String| if r.count > 0 { v } else { "—".to_string() };
    let mut nums = vec![
        num("Total", figure(r.amount), ""),
        num("Share", format!("{:.1}%", r.pct), ""),
        num("Hits", commas(r.count), ""),
        num("Crit", known(format!("{:.1}%", r.crit_pct())), ""),
        num(
            "Avg",
            known(figure(r.amount.checked_div(r.count).unwrap_or(0))),
            "",
        ),
    ];
    if r.extra > 0 {
        let what = match view {
            View::Healing => "Overheal",
            View::Taken | View::EnemyTaken => "Absorbed",
            _ => "Overkill",
        };
        nums.push(num(what, figure(r.extra), ""));
    }
    nums
}

/// The ability list's or a target list's settings, as a drill pane shows
/// them.
#[allow(clippy::too_many_arguments)]
fn pane_list(
    state: &Gui,
    rows: Vec<Row>,
    kind: list::Kind,
    view: View,
    head: &str,
    pane: Pane,
    lead: list::Lead,
    bar: list::Bar,
    press: list::Press,
) -> list::List {
    let app = state.fight();
    // The keys' row is lit only while the keys are in the inspector (or,
    // without follow-selection, in the drill they opened).
    let keyed = app.inspecting() || !app.follows_selection();
    let selected = app
        .drill
        .as_ref()
        .filter(|d| keyed && d.pane == pane && d.spell.is_none())
        .map(|d| match pane {
            Pane::Spell => d.spell_sel,
            Pane::Target => d.target_sel,
        });
    let sorts = kind == list::Kind::Abilities && pane == Pane::Spell;
    // A person's list names the owner with their tag: the enemy's attackers
    // by guid, a player's targets by name.
    let you = (lead == list::Lead::Person)
        .then(|| state.owner_of(&rows))
        .flatten();
    list::List {
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
        accent: state.accent.base,
        hover: state.hover_in(pane),
        you,
        pane,
        pair_hover: None,
        sort: sorts.then_some(state.drill_sort).flatten(),
        on_sort: sorts.then_some(Message::SortSpellsBy as fn(crate::table::Col) -> Message),
        press,
        side: list::SIDE,
    }
}

/// The ability drill's graph: the ability's own curve over the player's,
/// ghosted, on one scale.
fn focus_curves(
    app: &ClientState,
    whole: &Timeline,
    class: Option<Class>,
    focus: Option<(&Timeline, Color)>,
    window: (u32, u32),
) -> Vec<plot::Curve> {
    let mode = app.graph_mode();
    let bucket = rate_bucket(window.1.saturating_sub(window.0), whole.bucket_ms);
    let own = class.map_or(crate::view::CLASSLESS, theme::class_rgb);
    let (points, bucket_ms) = curve(whole, mode, bucket);
    let mut curves = vec![plot::Curve {
        name: String::new(),
        color: own,
        points,
        bucket_ms,
        ink: if focus.is_some() {
            plot::Ink::Ghost
        } else {
            plot::Ink::Area
        },
    }];
    if let Some((ft, color)) = focus {
        let (points, bucket_ms) = curve(ft, mode, bucket);
        curves.push(plot::Curve {
            name: String::new(),
            color,
            points,
            bucket_ms,
            ink: plot::Ink::Area,
        });
    }
    curves
}

/// A player on Damage, Healing, Taken or a count view.
fn player(state: &Gui, rows: &[Row], me: Option<usize>) -> Insp {
    let app = state.fight();
    let hide = state.cfg.hide_realms;
    let Some(drill) = app.drill.as_ref() else {
        return Insp::quiet("Select a player to inspect them here.", state.accent);
    };
    let row = me.and_then(|i| rows.get(i));
    let (class, spec) = row.map_or((None, None), |r| (r.class, r.spec));
    let view = app.view;
    // A count has no rate to draw: no graph, and no mode to switch it by.
    // The deaths come off the timeline's R23 marks, and the daemon sends a
    // timeline with the rate views alone (Damage, Healing, Taken): on
    // Interrupts, Crowd control and Dispels the head says nothing of a
    // death and there is no recap button — the Deaths view (K) has both.
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
    let owner = me.is_some() && state.owner_in(rows) == me;
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
        name: vec![(
            shown_name(&drill.label, hide),
            name_ink(class),
            theme::UI_SEMIBOLD,
        )],
        sub: sub.join(", "),
    };
    let nums = row.map(|r| player_nums(view, rows, r)).unwrap_or_default();

    let pinned = app
        .compare_picks()
        .first()
        .is_some_and(|(k, _)| *k == drill.key);
    // A stored pull is one player's drill at a time: the store keeps no
    // comparison to ask for, so its Compare is there, inert, saying why —
    // the row keeps the shape a pull of the log gives it.
    let stored = state.stored.is_some();
    let mut acts = vec![Act {
        icon: LineIcon::Compare,
        words: if pinned {
            "Pinned, pick another".to_string()
        } else {
            "Compare".to_string()
        },
        pressed: pinned,
        press: (!stored).then_some(Message::PinCompare),
        tip: match (stored, pinned) {
            (true, _) => NO_COMPARE_TIP,
            (false, true) => "Stop comparing (v)",
            (false, false) => "Pin for comparison (v)",
        },
    }];
    acts.push(Act {
        icon: LineIcon::Book,
        words: "Talents and gear".to_string(),
        pressed: false,
        press: Some(Message::OpenTalents),
        tip: "Talents and gear (t)",
    });
    if !deaths.is_empty() {
        acts.push(Act {
            icon: LineIcon::Skull,
            words: "Death recap".to_string(),
            pressed: false,
            press: Some(Message::PickView(View::Deaths)),
            tip: "Death recap (K)",
        });
    }
    if timeline.is_some() {
        acts.push(mode_act(app.graph_mode()));
    }

    // R17: the mitigation record, one line over the graph.
    let mit = match (view, app.drill_mitigation(), row) {
        (View::Taken, Some(m), Some(r)) => mit_pieces(m, r.amount),
        _ => Vec::new(),
    };

    // v16: the ability drill — its strip, its curve over the player's,
    // and who it landed on.
    let spell = app.drill_spell().cloned();
    let spell_row = app.drill_spell_row();
    let focus_color = spell_row
        .as_ref()
        .and_then(|r| crate::view::school_color(r.school))
        .unwrap_or(theme::GOLD);
    let graph = timeline.map(|t| {
        let focus = app
            .spell_timeline()
            .filter(|_| spell.is_some())
            .map(|ft| (ft, focus_color));
        let span = span_of(app, t);
        let window = window_of(app.drill_range(), span);
        graph_of(
            app,
            span,
            app.drill_range(),
            focus_curves(app, t, class, focus, window),
            dead_spans(t, span, None, &state.roster),
            lanes::lanes(&t.marks, &drill.key, class, &state.roster),
            None,
            Message::DrillRange,
        )
    });
    let ability = spell.as_ref().map(|(_, label)| Ability {
        who: shown_name(&drill.label, hide),
        who_ink: Some(name_ink(class)),
        label: if hide {
            realmless(label)
        } else {
            label.clone()
        },
        row: spell_row.clone(),
        view,
    });

    // R21: the stack ledger, a section behind a chip.
    let ledger = app
        .drill_stacks()
        .map(|(stacking, cells, base)| crate::taken::matrices(stacking, cells, base))
        .unwrap_or_default();
    let stacks = (!ledger.is_empty() && spell.is_none()).then(|| Stacks {
        on: state.stacks_open,
        ledger,
        dropped: app.drill_breakdown().map_or(0, |b| b.stacks_dropped),
    });

    let bar = list::Bar::Of(class.map_or(crate::view::CLASSLESS, theme::class_rgb));
    let (tabs, body) = if spell.is_some() {
        let targets = realmless_rows(&state.roster.as_themselves(&app.spell_target_rows()), hide);
        let mut l = pane_list(
            state,
            targets,
            list::Kind::Targets,
            view,
            "Target",
            Pane::Target,
            list::Lead::Person,
            list::Bar::Own,
            list::Press::Nothing,
        );
        l.hover = None;
        (None, Body::One(l))
    } else {
        let (by_spell, by_target) = app.breakdown();
        let taken = view == View::Taken;
        let words = if taken {
            ["Hit by", "Attackers"]
        } else {
            ["Abilities", "Targets"]
        };
        let up = drill.pane;
        let l = match up {
            Pane::Spell => pane_list(
                state,
                realmless_rows(&by_spell, hide),
                list::Kind::Abilities,
                view,
                "Ability",
                Pane::Spell,
                list::Lead::Spell,
                bar,
                // v16: Damage and Healing descend into an ability — on a
                // pull of the log, whose abilities have curves of their own.
                if matches!(view, View::Damage | View::Healing) && !stored {
                    list::Press::Spell
                } else {
                    list::Press::Nothing
                },
            ),
            Pane::Target => pane_list(
                state,
                realmless_rows(&state.roster.as_themselves(&by_target), hide),
                list::Kind::Targets,
                view,
                if taken { "Attacker" } else { "Target" },
                Pane::Target,
                list::Lead::Person,
                list::Bar::Own,
                list::Press::Nothing,
            ),
        };
        (Some((words, up)), Body::One(l))
    };
    // A breakdown on its way (the selection just moved) is no empty list:
    // the lists wait for it rather than say "nothing" — and while they
    // wait, the last player's stand in, dimmed, so the column keeps its
    // height ([`Held`]).
    let body = answered(app, body);
    // A stored pull answered without this player's breakdown has none on
    // its way: nobody stands in for it, and the note says why.
    let bare = state.stored.as_ref().is_some_and(|s| s.bare());
    let held = state
        .insp_held
        .as_ref()
        .filter(|h| app.drill_breakdown().is_none() && spell.is_none() && !bare && h.fits(app));
    let (mit, graph, body, stale) = match held {
        Some(h) => (h.mit.clone(), h.graph.clone(), h.body.clone(), true),
        None => (mit, graph, body, false),
    };
    let note = if row.is_none() || !app.view_answered() {
        Some("Waiting for this view's numbers…".to_string())
    } else {
        bare.then(|| BARE.to_string())
    };
    Insp {
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
        accent: state.accent,
    }
}

/// `body` once the drill's breakdown is in; nothing while it is on its
/// way.
fn answered(app: &ClientState, body: Body) -> Body {
    if app.drill_breakdown().is_some() {
        body
    } else {
        Body::Nothing
    }
}

/// The graph's mode as an action: "Per second" or "Cumulative", pressed
/// on the running total (`g`).
fn mode_act(mode: GraphMode) -> Act {
    Act {
        icon: LineIcon::Graph,
        words: match mode {
            GraphMode::Dps => "Per second".to_string(),
            GraphMode::Total => "Cumulative".to_string(),
        },
        pressed: mode == GraphMode::Total,
        press: Some(Message::ToggleGraph),
        tip: "Per second or cumulative (g)",
    }
}

/// R17's record as the line says it (`.mit`): what was mitigated of
/// everything swung, absorbed, blocked, prevented, staggered, and the
/// misses by kind.
fn mit_pieces(m: &Mitigation, taken: u64) -> Vec<(String, String, String)> {
    let piece = |a: &str, b: String, c: &str| (a.to_string(), b, c.to_string());
    let mut out = vec![piece(
        "Mitigated",
        format!("{:.0}%", m.mitigated_pct(taken)),
        " of everything swung",
    )];
    out.push(piece("Absorbed", commas(m.absorbed), ""));
    if m.blocked > 0 {
        out.push(piece("Blocked", commas(m.blocked), ""));
    }
    out.push(piece("Prevented", commas(m.prevented()), ""));
    if m.stagger > 0 {
        out.push(piece("Staggered", commas(m.stagger), ""));
    }
    if m.misses() > 0 {
        let kinds: Vec<String> = MissKind::ALL
            .iter()
            .filter(|k| m.misses_of(**k) > 0)
            .map(|k| format!("{} {}", k.name(), m.misses_of(*k)))
            .collect();
        out.push(piece(
            "Misses",
            m.misses().to_string(),
            &format!(": {}", kinds.join(", ")),
        ));
    }
    out
}

/// A player on the Deaths view: their recap (R9).
fn recap(state: &Gui, rows: &[Row], me: Option<usize>) -> Insp {
    let app = state.fight();
    let hide = state.cfg.hide_realms;
    let Some(drill) = app.drill.as_ref() else {
        return Insp::quiet("Select a death to see its recap.", state.accent);
    };
    let row = me.and_then(|i| rows.get(i));
    let (class, spec) = row.map_or((None, None), |r| (r.class, r.spec));
    let (windows, shown) = app.deaths();
    let at = shown
        .and_then(|i| windows.iter().find(|w| w.index == i))
        .or(windows.last())
        .map(|w| w.at_ms);
    let owner = me.is_some() && state.owner_in(rows) == me;
    // R25 (v35): the death as the raid timeline marks it — its killing blow
    // and the rez that undid it, the words the prototype's head says.
    let death = app
        .raid()
        .and_then(|raid| crate::deaths::selected(app, raid).and_then(|i| raid.deaths.get(i)));
    let mut sub: Vec<String> = plays(class, spec).into_iter().collect();
    match (death, at) {
        (Some(d), _) => sub.push(died_words(d, hide)),
        (None, Some(at)) => sub.push(format!("died {}", duration(at))),
        (None, None) => {}
    }
    let (events, attackers) = app.breakdown();
    // R9: the recap runs newest first, so the killing blow leads it.
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
        // How long they were down: the rez's moment less the death's — or
        // "no" when nothing raised them. Without a timeline (a store that
        // kept a card alone) the player's death count stands instead.
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
        icon: LineIcon::Sword,
        words: "Their damage".to_string(),
        pressed: false,
        press: Some(Message::PickView(View::Damage)),
        tip: "Their damage (d)",
    }];
    let deaths = Some(Deaths {
        windows: windows.to_vec(),
        shown,
        dropped: app.drill_breakdown().map_or(0, |b| b.deaths_dropped),
    });
    let up = drill.pane;
    let body = match up {
        Pane::Spell => Body::Recap(Recap {
            rows: realmless_rows(&events, hide),
            who: shown_name(&drill.label, hide),
            yours: owner,
            class,
        }),
        // R9: a death window's attackers are amounts, not the Deaths
        // meter's counts, so the list words them as damage.
        Pane::Target => Body::One(pane_list(
            state,
            realmless_rows(&state.roster.as_themselves(&attackers), hide),
            list::Kind::Targets,
            View::Damage,
            "Attacker",
            Pane::Target,
            list::Lead::Person,
            list::Bar::Own,
            list::Press::Nothing,
        )),
    };
    Insp {
        head: Head {
            discs: vec![Disc::Player(class, spec)],
            name: vec![(
                shown_name(&drill.label, hide),
                name_ink(class),
                theme::UI_SEMIBOLD,
            )],
            sub: sub.join(", "),
        },
        nums,
        acts,
        mit: Vec::new(),
        deaths,
        ability: None,
        graph: None,
        stacks: None,
        tabs: Some((["Recap", "Attackers"], up)),
        body: answered(app, body),
        stale: false,
        note: if state.stored.as_ref().is_some_and(|s| s.bare()) {
            Some(BARE.to_string())
        } else {
            (events.is_empty() && attackers.is_empty())
                .then(|| "Waiting for the recap…".to_string())
        },
        accent: state.accent,
    }
}

/// What a stored pull's inspector says when the store answered without the
/// player's breakdown: it keeps a pull's rows longer than its details.
const BARE: &str = "The history store kept this pull's rows, not this player's breakdown.";

/// An enemy (R24): what it took, and from whom — each attacker a click
/// away from their abilities on it. As the prototype's `enemyPanel`, the
/// head (numbers, no actions) leads straight to the attackers; the curve
/// is the second level's, one attacker's damage over the enemy's.
fn enemy(state: &Gui, rows: &[Row], me: Option<usize>) -> Insp {
    let app = state.fight();
    let hide = state.cfg.hide_realms;
    let Some(drill) = app.drill.as_ref() else {
        return Insp::quiet("Select an enemy to see who hit it.", state.accent);
    };
    let row = me.and_then(|i| rows.get(i));
    let (_, attackers) = app.breakdown();
    let spell = app.drill_spell().cloned();
    let spell_row = app.drill_spell_row();
    let attacker_color = spell_row
        .as_ref()
        .and_then(|r| r.class)
        .map_or(theme::GOLD, theme::class_rgb);
    let timeline = app
        .drill_timeline()
        .filter(|t| !t.buckets.is_empty() && spell.is_some());
    let graph = timeline.map(|t| {
        let span = span_of(app, t);
        let focus = app.spell_timeline().map(|ft| (ft, attacker_color));
        let window = window_of(app.drill_range(), span);
        let mut curves = focus_curves(app, t, None, focus, window);
        if let Some(c) = curves.first_mut() {
            c.color = crate::view::HOSTILE;
        }
        graph_of(
            app,
            span,
            app.drill_range(),
            curves,
            Vec::new(),
            Vec::new(),
            None,
            Message::DrillRange,
        )
    });
    let name = if hide {
        realmless(&drill.label)
    } else {
        drill.label.clone()
    };
    let acts = timeline
        .map(|_| vec![mode_act(app.graph_mode())])
        .unwrap_or_default();
    let (ability, body) = match spell {
        // The second level: one attacker's abilities on it.
        Some((_, label)) => {
            let abilities = realmless_rows(&app.spell_target_rows(), hide);
            let mut l = pane_list(
                state,
                abilities,
                list::Kind::Abilities,
                View::EnemyTaken,
                "Ability",
                Pane::Spell,
                list::Lead::Spell,
                list::Bar::Of(attacker_color),
                list::Press::Nothing,
            );
            l.selected = None;
            l.hover = None;
            (
                Some(Ability {
                    who: name.clone(),
                    who_ink: None,
                    label: shown_name(&label, hide),
                    row: spell_row,
                    view: View::EnemyTaken,
                }),
                Body::One(l),
            )
        }
        None => (
            None,
            Body::One(pane_list(
                state,
                realmless_rows(&attackers, hide),
                list::Kind::Targets,
                View::EnemyTaken,
                "Attacker",
                Pane::Target,
                list::Lead::Person,
                list::Bar::Own,
                list::Press::Attacker,
            )),
        ),
    };
    Insp {
        head: Head {
            discs: vec![Disc::Enemy],
            name: vec![(name, theme::INK, theme::UI_SEMIBOLD)],
            sub: "Enemy, every unit with this name folded together".to_string(),
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
        accent: state.accent,
    }
}

/// R12: two players — the pinned one and the selection — on one plot and
/// one scale, their abilities side by side.
fn pair(state: &Gui, rows: &[Row]) -> Insp {
    let app = state.fight();
    let hide = state.cfg.hide_realms;
    let picks = app.compare_picks();
    let (Some((a_key, a_label)), Some((b_key, b_label))) = (picks.first(), picks.get(1)) else {
        return Insp::quiet("Pick another player to compare.", state.accent);
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
    // One spelling of each everywhere the pair names them — the head, the
    // numbers, the legend, the lists, the hatches — realms as the option
    // says.
    let (a_name, b_name) = (shown_name(a_label, hide), shown_name(b_label, hide));
    let head = Head {
        discs: vec![Disc::Player(a_class, a_spec), Disc::Player(b_class, b_spec)],
        name: vec![
            (a_name.clone(), name_ink(a_class), theme::UI_SEMIBOLD),
            // The joining word reads (INK_3 is not for words): AA on the
            // panel at its 21 px.
            (" and ".to_string(), theme::INK_2, theme::UI),
            (b_name.clone(), name_ink(b_class), theme::UI_SEMIBOLD),
        ],
        sub: "One scale, one time axis. Move to swap the second player, v to stop.".to_string(),
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
            icon: LineIcon::Compare,
            words: "Stop comparing".to_string(),
            pressed: true,
            press: Some(Message::PinCompare),
            tip: "Stop comparing (v, Esc)",
        },
        mode_act(app.graph_mode()),
    ];
    let (a_color, b_color) = (
        a_class.map_or(crate::view::CLASSLESS, theme::class_rgb),
        b_class.map_or(crate::view::CLASSLESS, theme::class_rgb),
    );
    let spell = app.compare_spell().cloned();
    // One colour for both curves — a shared class, or both unknown — is
    // the one case the colours cannot tell apart: the second is dashed
    // then, and its swatch with it (the prototype's `graphBlock`).
    let dashed = a_color == b_color;
    let graph = sides.map(|(a, b)| {
        let mode = app.graph_mode();
        let span = span_of(app, &a.timeline).max(span_of(app, &b.timeline));
        let window = window_of(app.compare_shown_range(), span);
        let bucket = rate_bucket(window.1.saturating_sub(window.0), a.timeline.bucket_ms);
        let side_curves = |s: &CompareSide, color: Color, ink: plot::Ink, name: &str| {
            let mut out = Vec::new();
            let focus = s.spell_timeline.as_ref().filter(|_| spell.is_some());
            let (points, bucket_ms) = curve(&s.timeline, mode, bucket);
            out.push(plot::Curve {
                name: if focus.is_some() {
                    String::new()
                } else {
                    name.to_string()
                },
                color,
                points,
                bucket_ms,
                ink: if focus.is_some() {
                    plot::Ink::Ghost
                } else {
                    ink
                },
            });
            if let Some(ft) = focus {
                let (points, bucket_ms) = curve(ft, mode, bucket);
                out.push(plot::Curve {
                    name: name.to_string(),
                    color,
                    points,
                    bucket_ms,
                    ink,
                });
            }
            out
        };
        let mut curves = side_curves(a, a_color, plot::Ink::Line, &a_name);
        let b_ink = if dashed {
            plot::Ink::Dashed
        } else {
            plot::Ink::Line
        };
        curves.extend(side_curves(b, b_color, b_ink, &b_name));
        let mut dead = dead_spans(&a.timeline, span, Some(&a_name), &state.roster);
        dead.extend(dead_spans(&b.timeline, span, Some(&b_name), &state.roster));
        // R12's point: a gap explained by when each pressed their
        // cooldowns and their trinkets fired — both players' spans on the
        // fight's one clock, the first's over the second's in each lane.
        let lanes = lanes::pair(
            lanes::lanes(&a.timeline.marks, a_key, a_class, &state.roster),
            lanes::lanes(&b.timeline.marks, b_key, b_class, &state.roster),
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
            Message::CompareRange,
        )
    });
    let ability = spell.as_ref().map(|(_, label)| Ability {
        who: format!("{a_name} and {b_name}"),
        who_ink: None,
        label: shown_name(label, hide),
        row: None,
        view: metric,
    });
    let side_list = |s: &CompareSide, name: &str, class: Option<Class>| list::List {
        rows: realmless_rows(&s.spells, hide),
        kind: list::Kind::Pair,
        view: metric,
        note: s
            .mitigation
            .as_ref()
            .map(|m| mitigation_line(m, s.total.amount)),
        head: name.to_string(),
        head_ink: Some(name_ink(class)),
        lead: list::Lead::Spell,
        bar: list::Bar::Of(class.map_or(crate::view::CLASSLESS, theme::class_rgb)),
        selected: spell
            .as_ref()
            .and_then(|(k, _)| s.spells.iter().position(|r| r.key == *k)),
        keyed: false,
        accent: state.accent.base,
        hover: None,
        you: None,
        pane: Pane::Spell,
        pair_hover: state.spell_hover.clone(),
        sort: None,
        on_sort: None,
        press: list::Press::Pair,
        side: list::SIDE_PAIR,
    };
    let body = match sides {
        Some((a, b)) => Body::Pair(Box::new((
            side_list(a, &a_name, a_class),
            side_list(b, &b_name, b_class),
        ))),
        None => Body::Nothing,
    };
    Insp {
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
        accent: state.accent,
    }
}

// ---- drawing -------------------------------------------------------------------

/// A section of the column: `content` in the section's frame, over a
/// hairline when `rule`.
fn section(content: Element<'static, Message>, rule: bool) -> Element<'static, Message> {
    let body = container(content).padding(SECTION_PAD).width(Length::Fill);
    if rule {
        column![body, crate::nav::hairline::<Message>()].into()
    } else {
        body.into()
    }
}

/// The numbers (`.inums`), `per_row` to a row: each a gold-dim label over
/// its value (`.inum .l{13px}`, `.v{17px 500}`) and the value's quieter
/// tail (`.v small{13px}`).
fn nums_grid(nums: &[Num], per_row: usize) -> Element<'static, Message> {
    let mut grid = column![].spacing(NUMS_GAP);
    for chunk in nums.chunks(per_row.max(1)) {
        let mut line = row![].spacing(NUMS_GAP);
        for n in chunk {
            // An empty figure ("Back in" when nothing raised them) leaves
            // its small word flush left, under its label as the figures
            // beside it stand — the empty text keeps the line's height, so
            // the row keeps one baseline, and no gap indents the word.
            let mut value = row![
                text(n.value.clone())
                    .size(NUM_PX)
                    .font(theme::UI_MEDIUM)
                    .color(theme::INK)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(if n.value.is_empty() {
                0.0
            } else {
                NUM_TAIL_GAP
            })
            .align_y(iced::Alignment::End);
            if !n.small.is_empty() {
                value = value.push(
                    text(n.small.clone())
                        .size(NUM_SMALL_PX)
                        .color(theme::INK_2)
                        .wrapping(text::Wrapping::None),
                );
            }
            line = line.push(
                container(
                    column![
                        text(n.label.clone())
                            .size(NUM_LABEL_PX)
                            .color(theme::GOLD_DIM)
                            .wrapping(text::Wrapping::None),
                        value,
                    ]
                    .spacing(NUM_LINE_GAP),
                )
                .clip(true)
                .width(Length::FillPortion(1)),
            );
        }
        // A short last row keeps its cells the grid's width.
        for _ in chunk.len()..per_row {
            line = line.push(Space::new().width(Length::FillPortion(1)));
        }
        grid = grid.push(line);
    }
    grid.into()
}

/// One action (`.btn`): the glyph and the words on a framed button,
/// brightening under the pointer; pressed, it is filled with the accent
/// and inked with the accent's ink.
fn act_button(a: &Act, accent: theme::Accent) -> Element<'static, Message> {
    let pressed = a.pressed;
    let inert = a.press.is_none();
    let ink = if pressed {
        accent.ink
    } else if inert {
        theme::INK_3
    } else {
        theme::INK_2
    };
    let face = row![
        line_icon::<Message>(a.icon, BTN_ICON, ink),
        text(a.words.clone())
            .size(BTN_PX)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(BTN_GAP)
    .align_y(iced::Alignment::Center);
    let face = button(
        container(face)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    .height(Length::Fixed(BTN_H))
    .padding([0.0, BTN_PAD_X])
    .on_press_maybe(a.press.clone())
    .style(move |_: &Theme, status| {
        let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
        // Inert, it sits back in the faintest ink on a hairline frame:
        // there, and plainly not a thing to press.
        let (fill, edge, words) = if pressed {
            (Some(accent.base), accent.base, accent.ink)
        } else if inert {
            (None, theme::LINE, theme::INK_3)
        } else if hot {
            (None, theme::INK_3, theme::INK)
        } else {
            (None, theme::EDGE, theme::INK_2)
        };
        button::Style {
            background: fill.map(Into::into),
            text_color: words,
            border: Border {
                color: edge,
                width: 1.0,
                radius: BTN_RADIUS.into(),
            },
            ..button::Style::default()
        }
    });
    crate::nav::tip(face, a.tip)
}

/// The graph (`.igraph`): its top line — the curve's words and its peak,
/// the mode or the pair's legend (`.ig-top{font-size:13px}`, in the faint
/// ink that still reads on the panel) — over the canvas.
fn graph_block(g: &Graph) -> Element<'static, Message> {
    let tail: Element<'static, Message> = match &g.tail {
        Tail::Words(w) => text(w.clone()).size(TOP_PX).color(theme::INK_3_TEXT).into(),
        Tail::Legend(entries) => {
            let mut line = row![].spacing(LEGEND_GAP).align_y(iced::Alignment::Center);
            for (name, color, dashed) in entries {
                let color = *color;
                let swatch: Element<'static, Message> = if *dashed {
                    row![stroke_bit(color, DASH_W), stroke_bit(color, DASH_W)]
                        .spacing(DASH_GAP)
                        .into()
                } else {
                    stroke_bit(color, SWATCH_W)
                };
                line = line.push(
                    row![
                        swatch,
                        text(name.clone())
                            .size(TOP_PX)
                            .color(theme::INK_2)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(SWATCH_GAP)
                    .align_y(iced::Alignment::Center),
                );
            }
            line.into()
        }
    };
    let top = row![
        text(g.lead.clone())
            .size(TOP_PX)
            .color(theme::INK_3_TEXT)
            .wrapping(text::Wrapping::None),
        Space::new().width(Length::Fill),
        tail,
    ]
    .align_y(iced::Alignment::Center);
    let on_range = g.on_range;
    column![
        top,
        plot::view(plot::Plot {
            window: g.window,
            peak: g.peak,
            curves: g.curves.clone(),
            dead: g.dead.clone(),
            lanes: g.lanes.clone(),
            total: g.total,
            word: g.word,
            on_range: Some(Rc::new(on_range)),
        }),
    ]
    .spacing(TOP_GAP)
    .into()
}

/// A legend's swatch (`.leg i{width:10px;height:3px;border-radius:2px}`),
/// or one of a dashed swatch's two dashes.
fn stroke_bit(color: Color, w: f32) -> Element<'static, Message> {
    container(Space::new())
        .width(Length::Fixed(w))
        .height(Length::Fixed(SWATCH_H))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: iced::border::rounded(2),
            ..container::Style::default()
        })
        .into()
}

/// R17's line (`.mit`): each figure in ink between its words. A tail
/// carries its own lead-in (" of everything swung", ": parry 9"), so the
/// figure sits against it as the prototype's does.
fn mit_line(pieces: &[(String, String, String)]) -> Element<'static, Message> {
    let mut line = Line::new().spacing(MIT_GAP);
    for (lead, value, tail) in pieces {
        let mut piece = row![
            row![
                text(lead.clone()).size(MIT_PX).color(theme::INK_2),
                text(value.clone())
                    .size(MIT_PX)
                    .font(theme::UI_MEDIUM)
                    .color(theme::INK),
            ]
            .spacing(MIT_WORD_GAP)
        ];
        if !tail.is_empty() {
            piece = piece.push(
                text(tail.clone())
                    .size(MIT_PX)
                    .color(theme::INK_2)
                    .wrapping(text::Wrapping::None),
            );
        }
        line = line.push(piece);
    }
    line.wrap().vertical_spacing(MIT_ROW_GAP).into()
}

/// The ability drill's strip, in the inspector's own terms: back, then
/// "Player ▸ [icon] Ability  School" — the school a word in the secondary
/// ink, not a chip — and the ability's figures as the numbers under a
/// player's head are drawn (`.inum`: a gold-dim label over a plain
/// value), four across in a wide window and two in a tile or a narrow one.
fn ability_strip(a: &Ability, fit: Fit) -> Element<'static, Message> {
    let mut crumb = row![
        crate::nav::icon_button(LineIcon::ChevronLeft, Some(Message::CloseAbility), None),
        text(a.who.clone())
            .size(CRUMB_PX)
            .color(a.who_ink.unwrap_or(theme::INK_2))
            .wrapping(text::Wrapping::None),
        text("▸")
            .size(CRUMB_MARK_PX)
            .color(theme::INK_3_TEXT)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(CRUMB_GAP)
    .align_y(iced::Alignment::Center);
    if let Some(h) = a
        .row
        .as_ref()
        .and_then(|r| crate::spell_icons::handle(r.spell_id))
    {
        crumb = crumb.push(
            iced::widget::image(h)
                .width(Length::Fixed(CRUMB_ICON))
                .height(Length::Fixed(CRUMB_ICON)),
        );
    }
    let school = a
        .row
        .as_ref()
        .and_then(|r| crate::view::school_name(r.school));
    let name = crate::ellipsis::ellipsis(a.label.clone())
        .size(CRUMB_NAME_PX)
        .font(theme::UI_MEDIUM)
        .color(theme::INK);
    crumb = match school {
        Some(school) => crumb.push(
            row![
                name.leaving(vec![(school.clone(), CRUMB_PX, theme::UI)], CRUMB_GAP),
                text(school)
                    .size(CRUMB_PX)
                    .color(theme::INK_2)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(CRUMB_GAP)
            .align_y(iced::Alignment::Center),
        ),
        None => crumb.push(name),
    };
    let mut strip = column![crumb].spacing(STRIP_GAP);
    if let Some(r) = &a.row {
        strip = strip.push(nums_grid(&ability_nums(r, a.view), per_row(fit)));
    }
    strip.into()
}

/// The tabs (`.itabs`): the one up in ink over the accent's line, the
/// others in secondary ink, a hairline under all. `stacks` is R21's third
/// tab — `Some(up)` when the player has a ledger — whose matrix stands in
/// the list's place.
fn tabs(
    words: [&'static str; 2],
    up: Pane,
    stacks: Option<bool>,
    accent: theme::Accent,
) -> Element<'static, Message> {
    let stacked = stacks == Some(true);
    let mut all: Vec<(&'static str, bool, Message)> = [Pane::Spell, Pane::Target]
        .into_iter()
        .zip(words)
        .map(|(pane, word)| (word, !stacked && pane == up, Message::InspectorTab(pane)))
        .collect();
    if stacks.is_some() {
        all.push(("Stacks", stacked, Message::ShowStacks(true)));
    }
    let mut strip = row![].spacing(TAB_GAP);
    for (word, active, press) in all {
        let underline = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(TAB_LINE))
            .style(move |_: &Theme| container::Style {
                background: active.then(|| accent.base.into()),
                ..container::Style::default()
            });
        let cell = column![
            container(
                text(word)
                    .size(TAB_PX)
                    .font(theme::UI_MEDIUM)
                    .wrapping(text::Wrapping::None)
            )
            .height(Length::Fill)
            .align_y(iced::Alignment::Center)
            .padding([0.0, TAB_PAD_X]),
            underline,
        ]
        .width(Length::Shrink)
        .height(Length::Fixed(TAB_H));
        strip = strip.push(button(cell).padding(0).on_press(press).style(
            move |_: &Theme, status| button::Style {
                text_color: if active
                    || matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    theme::INK
                } else {
                    theme::INK_2
                },
                ..button::Style::default()
            },
        ));
    }
    column![
        container(strip).padding([0.0, TABS_PAD_X]),
        crate::nav::hairline::<Message>(),
    ]
    .into()
}

/// R9: the death's last events as the prototype's `recapPanel` lists them —
/// OLDEST first, the way it happened, one row an event, so the killing blow
/// ends the list on a tinted row: each event's time before the death (v35;
/// the column only when the recap carries one — a recap stored before v35
/// has none), its change (`+756` in green, `−82,509` in red), what it was
/// and who it came from (their own: "yours" for the owner, "self" for
/// anyone else), and their health after it as a bar that goes amber under
/// 15 % and red under 3 % — a dimmed, empty track with a dash where the
/// log reported none. No bar runs under any words.
///
/// When they hurt themselves materially, the insight line after the list
/// says how much and what finished them — the recap's conclusion, as the
/// prototype's `.insight` follows its list; the list opens right under the
/// tabs.
fn recap_list(r: &Recap, fit: Fit) -> Element<'static, Message> {
    let head = |words: &'static str| {
        text(words)
            .size(TOP_PX)
            .color(theme::GOLD_DIM)
            .wrapping(text::Wrapping::None)
    };
    let cols = RecapCols::of(r, fit);
    let mut heads = row![].spacing(RECAP_GAP).align_y(iced::Alignment::Center);
    if let Some(time) = cols.time {
        heads = heads.push(
            container(head("Time"))
                .width(Length::Fixed(time))
                .align_x(iced::Alignment::End),
        );
    }
    heads = heads
        .push(
            container(head("Change"))
                .width(Length::Fixed(cols.change))
                .align_x(iced::Alignment::End),
        )
        .push(container(head("Last events, oldest first")).width(Length::Fill))
        .push(
            // A tile's shorter bar is outgrown by its head, which stands
            // over the bar's end and reaches left over the label's room.
            container(head("Health after"))
                .width(Length::Fixed(cols.hp.max(cols.hp_head)))
                .align_x(iced::Alignment::End),
        );
    let mut list = column![container(heads).padding(RECAP_HEAD_PAD)];
    let oldest: Vec<&Row> = r.rows.iter().rev().collect();
    // The killing blow: the newest damage.
    let blow = oldest.iter().rposition(|e| !e.gain);
    // From the killing blow on — its row, anything after it, the insight —
    // is what an opened death brings into sight: the end of the story and
    // its conclusion together.
    let mut end = column![];
    for (i, e) in oldest.iter().enumerate() {
        let line = recap_line(r, e, blow == Some(i), &cols);
        if blow.is_some_and(|b| i >= b) {
            end = end.push(line);
        } else {
            list = list.push(line);
        }
    }
    if let Some(words) = insight(r) {
        end = end.push(insight_line(words, r.class));
    }
    list = list.push(container(end).id(recap_kill_id()));
    container(list).padding(RECAP_PAD).into()
}

/// The recap's column widths, for the width it is drawn at.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RecapCols {
    /// The time column's, when the recap carries its times.
    time: Option<f32>,
    change: f32,
    /// The health bar's, and its head's own width.
    hp: f32,
    hp_head: f32,
}

impl RecapCols {
    /// A timed recap's time and change columns are as wide as their
    /// widest figure (or head), up to [`RECAP_TIME_W`] and
    /// [`RECAP_CHANGE_W`] — the time column's room is the change column's
    /// air, given back to the events' names. An untimed recap keeps the
    /// prototype's 78 px change. The health bar is 88 px, shorter in a
    /// tile's inspector ([`RECAP_HP_W_TILE`]), where the label needs the
    /// room and a dying player's bar is mostly a sliver anyway.
    fn of(r: &Recap, fit: Fit) -> Self {
        let timed = r.rows.iter().any(|e| e.offset_ms.is_some());
        let widest = |words: &mut dyn Iterator<Item = String>, px: f32, font: Font| {
            words
                .map(|w| text_w(&w, px, font))
                .fold(0.0_f32, f32::max)
                .ceil()
        };
        let time = timed.then(|| {
            let figures = widest(
                &mut r
                    .rows
                    .iter()
                    .filter_map(|e| e.offset_ms.map(crate::deaths::before)),
                RECAP_TIME_PX,
                theme::UI,
            );
            figures
                .max(text_w("Time", TOP_PX, theme::UI).ceil())
                .min(RECAP_TIME_W)
        });
        let change = if timed {
            widest(
                &mut r.rows.iter().map(change_words),
                RECAP_PX,
                theme::UI_MEDIUM,
            )
            .max(text_w("Change", TOP_PX, theme::UI).ceil())
            .min(RECAP_CHANGE_W)
        } else {
            RECAP_CHANGE_W
        };
        RecapCols {
            time,
            change,
            hp: if fit == Fit::Tile {
                RECAP_HP_W_TILE
            } else {
                RECAP_HP_W
            },
            hp_head: text_w("Health after", TOP_PX, theme::UI).ceil(),
        }
    }
}

/// An event's change as the recap signs it: "+756", "−82,509".
fn change_words(e: &Row) -> String {
    let sign = if e.gain { "+" } else { "\u{2212}" };
    format!("{sign}{}", commas(e.amount))
}

/// `content`'s one-line width at `px` in `font`, as the renderer shapes it.
fn text_w(content: &str, px: f32, font: Font) -> f32 {
    crate::ellipsis::width_of::<<iced::Renderer as iced::advanced::text::Renderer>::Paragraph>(
        content, px, font,
    )
}

/// The insight's words, a piece at a time — `true` on the one set bold in
/// the owner's text colour (the ability that did it).
type Insight = Vec<(String, bool)>;

/// A hit of their own is worth an insight when it took this share of the
/// player's health …
const INSIGHT_SHARE: f64 = 0.05;
/// … or found them under this share of it with the death this close after.
const INSIGHT_LOW: f64 = 0.30;
const INSIGHT_SOON_MS: i64 = 5_000;

/// v35 (R9): what a recap says about damage the player did to THEMSELVES
/// (an event whose source is their own name — a Burning Rush, a Soul
/// Burn): the biggest such hit, the health it found them at (the health
/// after the event before it), and what finished them after it — "Your own
/// **Burning Rush** took 30,660 while you were at 5.2% health. Three
/// Coalesced Venom hits finished it." Only a MATERIAL hit is named — at
/// least [`INSIGHT_SHARE`] of their health, or one that found them under
/// [`INSIGHT_LOW`] with the death within [`INSIGHT_SOON_MS`]: a warlock's
/// Burning Rush ticks all fight, and a tick at 80 % health killed nobody.
/// `None` when every hit was someone else's, or theirs was no matter.
fn insight(r: &Recap) -> Option<Insight> {
    let oldest: Vec<&Row> = r.rows.iter().rev().collect();
    let own = |e: &Row| {
        !e.gain
            && list::split_pet(&e.label)
                .1
                .is_some_and(|s| display_name(s) == display_name(&r.who))
    };
    // The biggest; the latest of equals.
    let (at, hit) = oldest
        .iter()
        .enumerate()
        .filter(|(_, e)| own(e))
        .max_by_key(|(i, e)| (e.amount, *i))?;
    let before = at
        .checked_sub(1)
        .and_then(|j| oldest.get(j))
        .and_then(|e| e.hp)
        .filter(|(_, max)| *max > 0);
    let max = hit.hp.or(before).map(|(_, m)| m).filter(|m| *m > 0);
    let share = max.map(|m| hit.amount as f64 / m as f64);
    let low = before.map(|(cur, m)| cur as f64 / m as f64);
    let soon = hit.offset_ms.is_none_or(|o| o >= -INSIGHT_SOON_MS);
    let material =
        share.is_some_and(|s| s >= INSIGHT_SHARE) || (low.is_some_and(|l| l < INSIGHT_LOW) && soon);
    if !material {
        return None;
    }
    let what = list::split_pet(&hit.label).0.to_string();
    let (whose, were) = if r.yours {
        ("Your own ".to_string(), "you were")
    } else {
        (format!("{}'s own ", display_name(&r.who)), "they were")
    };
    let health = low
        .map(|l| format!(" while {were} at {:.1}% health", l * 100.0))
        .unwrap_or_default();
    let after: Vec<&str> = oldest
        .iter()
        .skip(at + 1)
        .filter(|e| !e.gain)
        .map(|e| list::split_pet(&e.label).0)
        .collect();
    let finish = match after.first() {
        None => " It was the killing blow.".to_string(),
        Some(first) if after.iter().all(|s| s == first) => format!(
            " {} {first} {} finished it.",
            count_word(after.len()),
            if after.len() == 1 { "hit" } else { "hits" }
        ),
        Some(_) => format!(" {} more hits finished it.", count_word(after.len())),
    };
    Some(vec![
        (whose, false),
        (what, true),
        (
            format!(" took {}{health}.{finish}", commas(hit.amount)),
            false,
        ),
    ])
}

/// "One", "Two" … "Nine", then the figure — how a sentence starts a count.
fn count_word(n: usize) -> String {
    const WORDS: [&str; 9] = [
        "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    n.checked_sub(1)
        .and_then(|i| WORDS.get(i))
        .map_or_else(|| n.to_string(), |w| w.to_string())
}

/// The insight, drawn (`.insight`): a wash of the player's class colour,
/// the ability in their text colour.
fn insight_line(words: Insight, class: Option<Class>) -> Element<'static, Message> {
    let bold = class.map_or(theme::INK, theme::you_text);
    let wash = Color {
        a: INSIGHT_WASH,
        ..class.map_or(theme::INK_3, theme::class_rgb)
    };
    let spans: Vec<iced::widget::text::Span<'static, iced::Never, Font>> = words
        .into_iter()
        .map(|(s, b)| {
            let span = iced::widget::span(s);
            if b {
                span.color(bold).font(theme::UI_SEMIBOLD)
            } else {
                span.color(theme::INK)
            }
        })
        .collect();
    container(
        container(
            iced::widget::rich_text(spans)
                .on_link_click(iced::never)
                .size(INSIGHT_PX)
                .font(theme::UI),
        )
        .padding(INSIGHT_PAD)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(wash.into()),
            border: iced::border::rounded(INSIGHT_RADIUS),
            ..container::Style::default()
        }),
    )
    .padding(INSIGHT_MARGIN)
    .into()
}

/// One event of the recap (`.rrow`), in `cols`: the time column leads when
/// the recap carries its times (v35) — a recap stored before them has none.
fn recap_line(r: &Recap, e: &Row, kill: bool, cols: &RecapCols) -> Element<'static, Message> {
    let ink = if e.gain { theme::GOOD } else { theme::BAD };
    let change = container(
        text(change_words(e))
            .size(RECAP_PX)
            .font(theme::UI_MEDIUM)
            .color(ink)
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fixed(cols.change))
    .align_x(iced::Alignment::End);
    let (what, from) = list::split_pet(&e.label);
    let own = from.is_some_and(|s| display_name(s) == display_name(&r.who));
    let mut label = crate::ellipsis::ellipsis(what.to_string())
        .size(RECAP_PX)
        .color(theme::INK);
    // The source gives way whole: a name cut to a few letters ("Zul'j…")
    // saves almost nothing and reads as another name.
    match from {
        Some(_) if own => {
            let word = if r.yours { "yours" } else { "self" };
            let ink = r.class.map_or(theme::INK, theme::you_text);
            label = label.tail(word, RECAP_SRC_PX, ink, RECAP_SRC_GAP);
        }
        Some(src) => {
            label = label.tail(
                src.to_string(),
                RECAP_SRC_PX,
                theme::INK_3_TEXT,
                RECAP_SRC_GAP,
            );
        }
        None => {}
    }
    let label = label.giving(crate::ellipsis::Give::Whole);
    // A heal never leaves a player at 0: a 0 on one is the killing blow's
    // report the meter filled into the nearest empty slot, not this
    // heal's — unknown here, rather than a bar that says they were dead
    // before the blow.
    let hp = e.hp.filter(|(cur, _)| !(e.gain && *cur == 0));
    let health: Element<'static, Message> = match hp {
        Some((cur, max)) => {
            let p = (cur as f32 / max.max(1) as f32).clamp(0.0, 1.0);
            let shown = if cur > 0 { p.max(RECAP_HP_MIN) } else { 0.0 };
            let fill = if p < RECAP_CRIT {
                theme::BAD
            } else if p < RECAP_LOW {
                theme::AMBER
            } else {
                theme::GOOD
            };
            let lit = (shown * 1000.0).round() as u16;
            container(
                row![
                    container(Space::new())
                        .width(Length::FillPortion(lit.max(1)))
                        .height(Length::Fill)
                        .style(move |_: &Theme| container::Style {
                            background: (lit > 0).then(|| fill.into()),
                            border: iced::border::rounded(RECAP_HP_RADIUS),
                            ..container::Style::default()
                        }),
                    Space::new()
                        .width(Length::FillPortion(1000_u16.saturating_sub(lit).max(1)))
                        .height(Length::Fill),
                ]
                .height(Length::Fill),
            )
            .width(Length::Fixed(cols.hp))
            .height(Length::Fixed(RECAP_HP_H))
            .style(|_: &Theme| container::Style {
                background: Some(RECAP_HP_TRACK.into()),
                border: iced::border::rounded(RECAP_HP_RADIUS),
                ..container::Style::default()
            })
            .into()
        }
        // Unknown: a dash, then the track dimmed and empty — never blank
        // space, and never an empty bright track, which reads as dead.
        None => row![
            text(RECAP_HP_UNKNOWN)
                .size(RECAP_SRC_PX)
                .color(theme::INK_3_TEXT)
                .wrapping(text::Wrapping::None),
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(RECAP_HP_H))
                .style(|_: &Theme| container::Style {
                    background: Some(
                        Color {
                            a: RECAP_HP_TRACK.a * RECAP_HP_DIM,
                            ..RECAP_HP_TRACK
                        }
                        .into()
                    ),
                    border: iced::border::rounded(RECAP_HP_RADIUS),
                    ..container::Style::default()
                }),
        ]
        .spacing(RECAP_SRC_GAP)
        .align_y(iced::Alignment::Center)
        .width(Length::Fixed(cols.hp))
        .into(),
    };
    let mut line = row![];
    if let Some(w) = cols.time {
        // v35: when it happened, before the death.
        line = line.push(
            container(
                text(e.offset_ms.map(crate::deaths::before).unwrap_or_default())
                    .size(RECAP_TIME_PX)
                    .color(theme::INK_3_TEXT)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(w))
            .align_x(iced::Alignment::End),
        );
    }
    let row = container(
        line.push(change)
            .push(label)
            .push(health)
            .spacing(RECAP_GAP)
            .align_y(iced::Alignment::Center)
            .height(Length::Fill),
    )
    .height(Length::Fixed(RECAP_ROW_H))
    .padding([0.0, RECAP_SIDE])
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: kill.then(|| {
            Color {
                a: RECAP_KILL_ALPHA,
                ..theme::BAD
            }
            .into()
        }),
        ..container::Style::default()
    });
    row.into()
}

/// A quiet line (`.note{margin:10px 16px;padding:8px 10px;border-left:
/// 2px solid var(--edge)}`).
fn note_line(words: &str) -> Element<'static, Message> {
    container(
        row![
            container(Space::new())
                .width(Length::Fixed(NOTE_EDGE))
                .height(Length::Fixed(NOTE_EDGE_H))
                .style(|_: &Theme| container::Style {
                    background: Some(theme::EDGE.into()),
                    ..container::Style::default()
                }),
            text(words.to_string())
                .size(size::MICRO)
                .color(theme::INK_2),
        ]
        .spacing(NOTE_GAP)
        .align_y(iced::Alignment::Center),
    )
    .padding(NOTE_PAD)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{self as tk, Bridge, chr, named};
    use iced::keyboard::key::Named;
    use wowdps_daemon::mock::{MockDaemon, pump};
    use wowdps_model::Action;
    use wowdps_proto::{Breakdown, DaemonMsg, SegmentRef};

    /// The fixture's boss kill, following the selection, in `view`: the
    /// inspector's drill answered by the mock.
    fn following(view: View) -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = tk::kill();
        let reqs = state.set_follow(true);
        pump(&mut state, &mut mock, reqs);
        if view != View::Damage {
            tk::apply(&mut state, &mut mock, Action::SetView(view));
        }
        (state, mock)
    }

    /// A window over the fixture with the kill open — a press on its row
    /// on the rail, as a reader opens it — and its top row selected: the
    /// move keeps whoever the pull before was inspecting.
    fn on_the_kill() -> Bridge {
        let mut b = Bridge::new(MockDaemon::fixture());
        let at = b
            .gui
            .state
            .list_rows()
            .iter()
            .position(|r| r.name == "The Ashen Warden")
            .expect("the fixture's kill");
        b.open(at);
        b.send(Message::MeterRow(0));
        b
    }

    fn labels(nums: &[Num]) -> Vec<&str> {
        nums.iter().map(|n| n.label.as_str()).collect()
    }

    #[test]
    fn the_breakpoints_are_the_prototype_s() {
        assert_eq!(Fit::of(1440.0), Fit::Wide);
        assert_eq!(Fit::of(1181.0), Fit::Wide);
        assert_eq!(Fit::of(1180.0), Fit::Tile);
        assert_eq!(Fit::of(821.0), Fit::Tile);
        assert_eq!(Fit::of(820.0), Fit::Narrow);
        assert_eq!(beside(1440.0), Some(520.0), "wide: its most");
        assert_eq!(beside(960.0), Some(410.0), "a tile: its most");
        assert_eq!(beside(821.0), Some(410.0));
        assert_eq!(beside(820.0), None, "narrow: pushed, not beside");
        assert_eq!(beside(460.0), None);
    }

    /// Beside the meter the inspector is the selection's: its name and
    /// what they play, the view's four numbers, the actions, the graph
    /// and the abilities — and a move of the selection moves all of it.
    #[test]
    fn the_inspector_follows_the_selection_beside_the_meter() {
        let mut b = on_the_kill();
        let rows = b.gui.state.rows();
        let first = rows[0].clone();
        let insp = Insp::of(&b.gui);
        let hide = b.gui.cfg.hide_realms;
        assert_eq!(insp.head.name[0].0, shown_name(&first.label, hide));
        assert_eq!(labels(&insp.nums), ["Dps", "Damage", "Rank", "Crit"]);
        assert_eq!(insp.nums[2].small, Place::of(&rows, &first).tail());
        assert!(insp.graph.is_some(), "Damage carries a timeline");
        assert!(matches!(&insp.body, Body::One(l) if !l.rows.is_empty()));
        let mut ui = tk::wide(crate::view::view(&b.gui));
        for words in [
            "Dps",
            "Rank",
            "Compare",
            "Talents and gear",
            "Abilities",
            "Targets",
        ] {
            assert!(ui.find(words).is_ok(), "{words}");
        }
        assert!(
            ui.find(crate::view::meter_list_id()).is_ok(),
            "the meter, beside"
        );
        assert!(ui.find(scroll_id()).is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        drop(ui);

        b.send(chr("j"));
        let second = b.gui.state.rows()[1].clone();
        let insp = Insp::of(&b.gui);
        assert_eq!(insp.head.name[0].0, shown_name(&second.label, hide));
        assert_eq!(b.gui.state.screen, Screen::Meter, "never a screen change");
        let (by_spell, _) = b.gui.state.breakdown();
        assert!(
            !by_spell.is_empty(),
            "the move's breakdown came with the rows"
        );
    }

    /// Without a raid timeline — a stored pull whose store kept its card
    /// alone, a loading placeholder — the Deaths inspector's fourth number
    /// is the player's death count, where "Back in" needs the timeline's
    /// rez; and a player nothing raised reads "no" flush under its label.
    #[test]
    fn a_recap_without_a_timeline_counts_the_deaths() {
        let mut state = tk::raid_deaths_bare(25);
        let _ = state.set_follow(true);
        assert!(state.raid().is_none(), "no timeline");
        assert!(state.drill.is_some(), "a dead player followed");
        let (gui, _peer) = tk::gui_over(state);
        let insp = Insp::of(&gui);
        assert_eq!(
            labels(&insp.nums),
            ["Died", "Killing blow", "Overkill", "Deaths"]
        );
        assert_eq!(insp.nums[3].value, "1");
        // "no" where nothing raised them stands where a figure would, not
        // indented after an empty one — beside figures, on their line.
        let nums = vec![
            num("Died", "5:45".to_string(), ""),
            num("Back in", String::new(), "no"),
        ];
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(520.0, 200.0),
            nums_grid(&nums, 2),
        );
        let label = ui.find("Back in").expect("the label").bounds();
        let word = ui.find("no").expect("the word").bounds();
        assert!((word.x - label.x).abs() < 0.5, "{label:?} / {word:?}");
        let figure = ui.find("5:45").expect("a figure").bounds();
        assert!(
            (word.y + word.height - (figure.y + figure.height)).abs() < 3.0,
            "one baseline: {figure:?} / {word:?}"
        );
    }

    /// Every view words its own numbers, as the prototype's do, and the
    /// Taken, Deaths and Enemies inspectors are what those views are about:
    /// the mitigation line, the recap, the attackers.
    #[test]
    fn every_view_has_its_inspector() {
        let cases: [(View, &[&str]); 4] = [
            (View::Healing, &["Hps", "Healing", "Overheal", "Rank"]),
            (View::Interrupts, &["Interrupts", "Share"]),
            (
                View::Deaths,
                // R25: with the raid timeline in hand, how long they were
                // down — the player's death count only without one.
                &["Died", "Killing blow", "Overkill", "Back in"],
            ),
            (
                View::EnemyTaken,
                &["Damage taken", "Per sec", "Share", "Crit"],
            ),
        ];
        for (view, want) in cases {
            let (state, _mock) = following(view);
            let rows = state.rows();
            let (gui, _peer) = tk::gui_over(state);
            let insp = Insp::of(&gui);
            if rows.is_empty() {
                // Nobody in the view: nobody to inspect, never the last
                // view's player waiting on numbers that are not coming.
                assert!(gui.state.drill.is_none(), "{view:?}");
                assert_eq!(insp.note.as_deref(), Some("Nothing to inspect yet."));
                continue;
            }
            assert_eq!(labels(&insp.nums), want, "{view:?}");
            match view {
                View::Deaths => {
                    assert_eq!(insp.tabs.map(|(w, _)| w), Some(["Recap", "Attackers"]));
                    assert!(matches!(&insp.body, Body::Recap(r) if !r.rows.is_empty()));
                }
                View::EnemyTaken => {
                    assert!(insp.tabs.is_none(), "one list");
                    assert!(matches!(&insp.body, Body::One(l) if l.head == "Attacker"));
                    assert!(matches!(insp.head.discs[..], [Disc::Enemy]));
                    // The prototype's enemy panel: its numbers, then the
                    // attackers — no actions, no graph between.
                    assert!(insp.acts.is_empty() && insp.graph.is_none());
                }
                View::Interrupts => {
                    assert!(insp.graph.is_none(), "a count draws no graph");
                    assert!(insp.acts.iter().all(|a| a.words != "Per second"));
                }
                _ => {}
            }
            let mut ui = tk::wide(crate::view::view(&gui));
            assert!(ui.find(want[0]).is_ok(), "{view:?}");
            let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        }

        let (mut state, mut mock) = tk::taken_kill();
        let reqs = state.set_follow(true);
        pump(&mut state, &mut mock, reqs);
        let (gui, _peer) = tk::gui_over(state);
        let insp = Insp::of(&gui);
        assert_eq!(labels(&insp.nums), ["Dtps", "Taken", "Absorbed", "Share"]);
        assert_eq!(insp.tabs.map(|(w, _)| w), Some(["Hit by", "Attackers"]));
        // R17: the tank's record as one line over his graph (the fixture's
        // goldens: 61% mitigated, 55,000 prevented, five misses).
        let mit: Vec<String> = insp
            .mit
            .iter()
            .map(|(a, b, c)| format!("{a} {b}{c}"))
            .collect();
        assert_eq!(mit[0], "Mitigated 61% of everything swung");
        assert!(mit.iter().any(|m| m == "Prevented 55,000"));
        assert!(mit.iter().any(|m| m.starts_with("Misses 5: ")), "{mit:?}");
        let mut ui = tk::wide(crate::view::view(&gui));
        assert!(ui.find("Hit by").is_ok());
        assert!(ui.find(" of everything swung").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// The graph's lanes take each span's colour from its CASTER, found
    /// on the meter's rows: an external from another player in theirs, the
    /// player's own in their class. A death is hatched, not laned, and
    /// says who rezzed them.
    #[test]
    fn lanes_wear_their_casters_and_a_death_its_rezzer() {
        let mut state = tk::raid(3);
        let rows = state.rows();
        let (me, giver, rezzer) = (&rows[0], &rows[1], &rows[2]);
        let mark = |kind, label: &str, at_ms, dur_ms, src: &str| Mark {
            at_ms,
            kind,
            label: label.to_string(),
            spell_id: 0,
            dur_ms,
            src: src.to_string(),
        };
        let timeline = Timeline {
            bucket_ms: 1000,
            buckets: vec![1_000; 60],
            marks: vec![
                mark(
                    MarkKind::External,
                    "Power Infusion",
                    10_000,
                    15_000,
                    &giver.key,
                ),
                mark(MarkKind::Cooldown, "Avatar", 20_000, 20_000, &me.key),
                mark(MarkKind::TrinketUse, "Signet", 30_000, 0, &me.key),
                mark(
                    MarkKind::Death,
                    "Death (Rebirth)",
                    40_000,
                    8_000,
                    &rezzer.key,
                ),
            ],
        };
        let reqs = state.set_follow(true);
        assert!(!reqs.is_empty(), "the selection's drill is asked for");
        let _ = state.on_msg(DaemonMsg::Snapshot {
            seq: 3,
            segment: SegmentRef::Live,
            id: None,
            view: View::Damage,
            info: wowdps_model::SegmentInfo {
                kind: wowdps_model::SegmentKind::Encounter,
                name: "The Coiled Altar".to_string(),
                start_ms: 1_000,
                duration_ms: 60_000,
                success: Some(true),
                live: false,
                instance: Some(0),
                pars_ms: None,
                arena: false,
                encounter: None,
            },
            total_rows: 3,
            rows: rows.clone(),
            breakdown: Some(Breakdown {
                timeline: Some(timeline),
                ..Breakdown::default()
            }),
            segment_count: 1,
            source: Some("raid.txt".to_string()),
            status: None,
            raid: None,
        });
        let (gui, _peer) = tk::gui_over(state);
        let graph = Insp::of(&gui).graph.expect("a graph");
        let lane = |l: lanes::Lane| graph.lanes.iter().find(|r| r.lane == l).cloned();
        let ext = lane(lanes::Lane::Externals).expect("the external's lane");
        assert_eq!(ext.spans[0].color, theme::class_rgb(giver.class.unwrap()));
        assert_eq!(
            ext.spans[0].caster.as_deref(),
            Some(display_name(&giver.label))
        );
        let cd = lane(lanes::Lane::Cooldowns).expect("the cooldown's lane");
        assert_eq!(cd.spans[0].color, theme::class_rgb(me.class.unwrap()));
        assert_eq!(cd.spans[0].caster, None, "their own");
        assert_eq!(
            lane(lanes::Lane::Items).unwrap().spans[0].dur_ms,
            0,
            "a tick"
        );
        assert!(lane(lanes::Lane::Defensives).is_none(), "nothing there");
        assert_eq!(graph.dead.len(), 1);
        assert_eq!(
            graph.dead[0].words,
            format!("died 0:40, rezzed by {}", display_name(&rezzer.label))
        );
        assert_eq!(
            (graph.dead[0].at_ms, graph.dead[0].end_ms),
            (40_000, 48_000)
        );
        assert_eq!(graph.window, (0, 60_000), "the fight's own span");
        assert_eq!(graph.lead, "dps", "the gutter says the peak");
        assert_eq!(graph.peak, 1_000.0);
        let insp = Insp::of(&gui);
        assert!(insp.head.sub.contains("died 0:40"), "{}", insp.head.sub);
        assert!(insp.acts.iter().any(|a| a.words == "Death recap"));
        let mut ui = tk::wide(crate::view::view(&gui));
        assert!(ui.find("Death recap").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// `v` pins the selection; the next move makes the pair, and the
    /// inspector overlays both curves on ONE plot and one scale — the
    /// second dashed on a shared class colour — with their numbers, a
    /// legend and the two ability lists, the meter still beside it.
    #[test]
    fn a_pair_overlays_two_curves_beside_the_meter() {
        let mut b = on_the_kill();
        b.send(chr("v"));
        assert_eq!(b.gui.state.screen, Screen::Meter, "a pin alone");
        let insp = Insp::of(&b.gui);
        assert!(
            insp.acts
                .iter()
                .any(|a| a.pressed && a.words == "Pinned, pick another")
        );
        // The meter says who is pinned: a gold-dim "A" after their name.
        let mut ui = tk::wide(crate::view::view(&b.gui));
        assert!(ui.find("A").is_ok(), "the pin's mark");
        // And a toast says so, over the stage.
        let words = b
            .gui
            .toast
            .as_ref()
            .map(|(w, _)| w.clone())
            .expect("a toast");
        assert!(
            words.starts_with("Pinned ") && words.ends_with(". Move to another player to compare."),
            "{words}"
        );
        assert!(ui.find(words.as_str()).is_ok(), "drawn");
        drop(ui);
        b.send(chr("j"));
        assert_eq!(b.gui.state.screen, Screen::Compare);
        let (a, bb) = b.gui.state.compare_sides().expect("the pair answered");
        let (a, bb) = (a.clone(), bb.clone());
        let insp = Insp::of(&b.gui);
        let hide = b.gui.cfg.hide_realms;
        let (a_name, b_name) = (
            shown_name(&a.total.label, hide),
            shown_name(&bb.total.label, hide),
        );
        assert_eq!(
            labels(&insp.nums),
            [a_name.as_str(), b_name.as_str(), "Gap", "Ratio"]
        );
        let graph = insp.graph.clone().expect("one plot");
        let drawn: Vec<&plot::Curve> = graph
            .curves
            .iter()
            .filter(|c| c.ink != plot::Ink::Ghost)
            .collect();
        assert_eq!(drawn.len(), 2, "both curves on it");
        // Dashed only where one colour would draw both (the prototype's
        // `graphBlock`: a shared class), the legend's swatch with it.
        let same = drawn[0].color == drawn[1].color;
        assert_eq!(drawn[0].ink, plot::Ink::Line);
        assert_eq!(
            drawn[1].ink,
            if same {
                plot::Ink::Dashed
            } else {
                plot::Ink::Line
            }
        );
        let peak = peak_in(&graph.curves, graph.window);
        assert_eq!(graph.peak, peak, "one scale over both");
        assert!(
            matches!(&graph.tail, Tail::Legend(l) if l.len() == 2 && !l[0].2 && l[1].2 == same)
        );
        // Both players' spans on the lanes, each on its half.
        assert!(
            graph
                .lanes
                .iter()
                .flat_map(|r| r.spans.iter())
                .all(|s| s.whose.as_deref() == Some(if s.second { &b_name } else { &a_name })),
            "{:?}",
            graph.lanes
        );
        assert!(matches!(&insp.body, Body::Pair(p) if p.0.head == a_name && p.1.head == b_name));
        assert!(b.gui.toast.is_none(), "the pair the pin asked for formed");
        let mut ui = tk::wide(crate::view::view(&b.gui));
        assert!(ui.find("Stop comparing").is_ok());
        assert!(ui.find("Gap").is_ok());
        assert!(ui.find("B").is_ok(), "the second half's mark");
        assert!(
            ui.find(crate::view::meter_list_id()).is_ok(),
            "the meter stays"
        );
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        drop(ui);
        // A tile keeps the lists whole by stacking them, as a narrow
        // window does; only a wide inspector's halves hold a name.
        assert!(!pair_stacked(Fit::Wide));
        assert!(pair_stacked(Fit::Tile) && pair_stacked(Fit::Narrow));
        let tile = iced::Size::new(960.0, 880.0);
        let mut ui = tk::simulator_as(crate::window::settings(), tile, crate::view::view(&b.gui));
        assert!(ui.find("Stop comparing").is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        drop(ui);
        // A narrow window stacks the two lists, pushed over the meter.
        b.send(Message::WindowWidth(460.0));
        let narrow = iced::Size::new(460.0, 860.0);
        b.send(named(Named::Enter));
        let mut ui = tk::simulator_as(crate::window::settings(), narrow, crate::view::view(&b.gui));
        assert!(
            ui.find("Stop comparing").is_ok(),
            "pushed, in a narrow window"
        );
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        drop(ui);
        // `v` stops it: the meter's drill is the selection's again.
        b.send(chr("v"));
        assert_eq!(b.gui.state.screen, Screen::Meter);
        assert!(b.gui.state.compare_picks().is_empty());
        assert_eq!(
            b.gui.state.drill.as_ref().map(|d| d.key.as_str()),
            Some(bb.guid.as_str())
        );
    }

    /// At 820 px and under the meter is alone; a click on a row (or Enter)
    /// pushes the inspector over the whole stage with a back button, and
    /// back — the button or Esc — returns the meter.
    #[test]
    fn a_narrow_window_pushes_the_inspector_and_backs_out() {
        let mut b = on_the_kill();
        let narrow = iced::Size::new(460.0, 860.0);
        let shows = |b: &Bridge, id: iced::widget::Id| {
            let mut ui =
                tk::simulator_as(crate::window::settings(), narrow, crate::view::view(&b.gui));
            ui.find(id).is_ok()
        };
        assert!(shows(&b, crate::view::meter_list_id()));
        assert!(!shows(&b, scroll_id()), "not beside it at 460 px");
        b.send(Message::PushRow(1));
        assert_eq!(b.gui.state.row_sel, 1);
        assert!(b.gui.state.inspecting());
        assert!(shows(&b, scroll_id()));
        assert!(shows(&b, back_id()), "the way back");
        assert!(!shows(&b, crate::view::meter_list_id()), "over the meter");
        b.send(Message::Uninspect);
        assert!(shows(&b, crate::view::meter_list_id()));
        b.send(named(Named::Enter));
        assert!(shows(&b, scroll_id()), "Enter pushes it too");
        b.send(named(Named::Escape));
        assert!(!b.gui.state.inspecting(), "Esc backs out");
        assert!(b.gui.home.is_none(), "one level only");
        // A wide window has the inspector beside the meter all along.
        let mut ui = tk::wide(crate::view::view(&b.gui));
        assert!(ui.find(scroll_id()).is_ok());
        assert!(ui.find(back_id()).is_err(), "nothing to go back from");
    }

    /// Tab switches the inspector's two lists (Hit by and Attackers on
    /// Taken); so do the tabs themselves. The keys walk the list once
    /// Enter gave it them, and Enter opens the ability inside the
    /// inspector — its strip, its curve over the player's, its targets.
    #[test]
    fn tab_switches_the_lists_and_enter_opens_an_ability_inside() {
        let mut b = on_the_kill();
        let body_head = |b: &Bridge| match Insp::of(&b.gui).body {
            Body::One(l) => l.head,
            _ => String::new(),
        };
        assert_eq!(body_head(&b), "Ability");
        b.send(named(Named::Tab));
        assert_eq!(
            b.gui.state.drill.as_ref().map(|d| d.pane),
            Some(Pane::Target)
        );
        assert_eq!(body_head(&b), "Target");
        b.send(Message::InspectorTab(Pane::Spell));
        assert_eq!(body_head(&b), "Ability");
        b.send(named(Named::Enter));
        b.send(chr("j"));
        assert_eq!(b.gui.state.row_sel, 0, "the meter's selection stays");
        assert_eq!(b.gui.state.drill.as_ref().map(|d| d.spell_sel), Some(1));
        let insp = Insp::of(&b.gui);
        assert!(
            matches!(&insp.body, Body::One(l) if l.selected == Some(1)),
            "lit"
        );
        b.send(Message::SpellRow(0));
        let (_, spell) = b.gui.state.drill_spell().cloned().expect("the ability");
        let insp = Insp::of(&b.gui);
        assert_eq!(
            insp.ability.as_ref().map(|a| a.label.as_str()),
            Some(spell.as_str())
        );
        assert!(insp.tabs.is_none(), "its targets, one list");
        assert!(matches!(&insp.body, Body::One(l) if l.head == "Target"));
        let graph = insp.graph.expect("the focus over the ghost");
        assert_eq!(graph.curves.len(), 2);
        assert_eq!(graph.curves[0].ink, plot::Ink::Ghost);
        let mut ui = tk::wide(crate::view::view(&b.gui));
        assert!(ui.find(spell.as_str()).is_ok());
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        drop(ui);
        b.send(Message::CloseAbility);
        assert!(b.gui.state.drill_spell().is_none());
        assert!(b.gui.state.inspecting(), "back to the player, keys kept");
    }

    /// `g` swaps the graph for the running total, and the inspector's
    /// button says so.
    #[test]
    fn the_graph_mode_is_worded_on_the_button_and_the_plot() {
        let mut b = on_the_kill();
        let lead = |b: &Bridge| Insp::of(&b.gui).graph.map(|g| g.lead).unwrap_or_default();
        assert!(lead(&b).starts_with("dps, peak "));
        b.send(Message::ToggleGraph);
        assert!(lead(&b).starts_with("Damage so far, peak "), "{}", lead(&b));
        let insp = Insp::of(&b.gui);
        assert!(
            insp.acts
                .iter()
                .any(|a| a.pressed && a.words == "Cumulative")
        );
        b.send(chr("g"));
        assert!(lead(&b).starts_with("dps, peak "));
    }

    /// A drill's targets wear the DRILLED player's class on the wire; the
    /// inspector shows each as itself — a player the window has seen in
    /// their class, a creature as none (the hostile disc and bar).
    #[test]
    fn targets_wear_their_own_class_not_the_drilled_player_s() {
        let (state, _mock) = following(View::Healing);
        let (_, targets) = state.breakdown();
        let rows = state.rows();
        let (gui, _peer) = tk::gui_over(state);
        let themselves = gui.roster.as_themselves(&targets);
        for (wire, shown) in targets.iter().zip(&themselves) {
            let seen = rows.iter().find(|r| r.label == wire.label);
            assert_eq!(shown.class, seen.and_then(|r| r.class), "{}", wire.label);
            assert_eq!(shown.amount, wire.amount, "only who they are changes");
        }
        let mut roster = Roster::default();
        let creature = Row {
            label: "Zul'jan".to_string(),
            class: Some(Class::Warlock),
            ..Row::default()
        };
        assert_eq!(
            roster.as_themselves(std::slice::from_ref(&creature))[0].class,
            None
        );
        roster.observe(&[Row {
            key: "Player-1".to_string(),
            label: "Zul'jan".to_string(),
            class: Some(Class::Mage),
            ..Row::default()
        }]);
        assert_eq!(
            roster.as_themselves(&[creature])[0].class,
            Some(Class::Mage),
            "a name the meter showed"
        );
    }

    /// Nothing to inspect is said, never drawn as a blank column.
    #[test]
    fn an_empty_inspector_says_why() {
        let (gui, _peer) = tk::gui_over(ClientState::new());
        let insp = Insp::of(&gui);
        assert_eq!(insp.note.as_deref(), Some("Nothing to inspect yet."));
        let _ = tk::render(insp.view(400.0, Fit::Tile, false));
    }

    /// A quiet inspector pushed over a narrow window still has its way
    /// back.
    #[test]
    fn a_pushed_quiet_inspector_keeps_its_back_button() {
        let (gui, _peer) = tk::gui_over(ClientState::new());
        let narrow = iced::Size::new(460.0, 860.0);
        let insp = Insp::of(&gui);
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            narrow,
            insp.view(460.0, Fit::Narrow, true),
        );
        assert!(ui.find(back_id()).is_ok());
    }

    /// The fixture's R21 kill on Taken, its tank selected: the ledger.
    fn stacks_kill() -> Bridge {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/stacks.txt");
        let mut b = Bridge::new(MockDaemon::fixture_at(std::path::Path::new(path)));
        let at = b
            .gui
            .state
            .list_rows()
            .iter()
            .position(|r| r.name == "Stacks Test Boss")
            .expect("the fixture's pull");
        b.open(at);
        b.send(chr("T"));
        let tank = b
            .gui
            .state
            .rows()
            .iter()
            .position(|r| r.label.starts_with("Brannoc"))
            .expect("the tank took hits");
        b.send(Message::MeterRow(tank));
        b
    }

    /// R21's ledger is a tab of its own — "Hit by | Attackers | Stacks" —
    /// whose matrix stands in the list's place, for a player who has one
    /// and nobody else; Tab walks the three, and a list's tab is that
    /// list. There is no chip row between the graph and the tabs.
    #[test]
    fn the_stacks_tab_shows_the_ledger_in_the_list_s_place() {
        let mut b = stacks_kill();
        let insp = Insp::of(&b.gui);
        assert!(!insp.stacks.as_ref().expect("the tank's ledger").on);
        assert_eq!(insp.tabs.map(|(w, _)| w), Some(["Hit by", "Attackers"]));
        let mut ui = tk::wide(crate::view::view(&b.gui));
        for w in ["Hit by", "Attackers", "Stacks", "Ability"] {
            assert!(ui.find(w).is_ok(), "{w}");
        }
        assert!(ui.find("Jump to").is_err(), "no chip row");
        drop(ui);
        let pane = |b: &Bridge| {
            (
                b.gui.state.drill.as_ref().map(|d| d.pane),
                b.gui.stacks_open,
            )
        };
        b.send(named(Named::Tab));
        assert_eq!(pane(&b), (Some(Pane::Target), false), "Hit by → Attackers");
        b.send(named(Named::Tab));
        assert_eq!(pane(&b), (Some(Pane::Target), true), "Attackers → Stacks");
        assert!(Insp::of(&b.gui).stacks.is_some_and(|s| s.on));
        let mut ui = tk::wide(crate::view::view(&b.gui));
        assert!(ui.find("Tectonic Strike · stacks").is_ok(), "the matrix");
        assert!(ui.find("Attacker").is_err(), "in the list's place");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        drop(ui);
        b.send(named(Named::Tab));
        assert_eq!(pane(&b), (Some(Pane::Spell), false), "Stacks → Hit by");
        b.send(Message::ShowStacks(true));
        b.send(Message::InspectorTab(Pane::Target));
        assert_eq!(pane(&b), (Some(Pane::Target), false), "a list's tab");
        // `taken.txt` puts no debuff on anyone: no ledger, no third tab.
        let (mut state, mut mock) = tk::taken_kill();
        let reqs = state.set_follow(true);
        pump(&mut state, &mut mock, reqs);
        let (gui, _peer) = tk::gui_over(state);
        assert!(Insp::of(&gui).stacks.is_none());
        let mut ui = tk::wide(crate::view::view(&gui));
        assert!(ui.find("Hit by").is_ok());
        assert!(ui.find("Stacks").is_err());
    }

    /// `hide_realms` reaches the inspector: its head and the players in
    /// its target list are drawn by name alone, and with the option off
    /// as the log wrote them.
    #[test]
    fn realms_are_hidden_in_the_inspector() {
        let (mut state, _mock) = following(View::Healing);
        if let Some(d) = state.drill.as_mut() {
            d.pane = Pane::Target;
        }
        let (_, targets) = state.breakdown();
        let realmed: Vec<String> = targets
            .iter()
            .map(|r| r.label.clone())
            .filter(|l| realmless(l) != *l)
            .collect();
        assert!(
            !realmed.is_empty(),
            "the healer healed players: {targets:?}"
        );
        let drilled = state.drill.clone().unwrap().label;
        let (mut gui, _peer) = tk::gui_over(state);
        gui.cfg.hide_realms = false;
        let mut ui = tk::wide(crate::view::view(&gui));
        for l in &realmed {
            assert!(ui.find(l.as_str()).is_ok(), "the option off: {l} as logged");
        }
        drop(ui);
        gui.cfg.hide_realms = true;
        let insp = Insp::of(&gui);
        assert_eq!(insp.head.name[0].0, display_name(&drilled));
        let mut ui = tk::wide(crate::view::view(&gui));
        for l in &realmed {
            assert!(ui.find(l.as_str()).is_err(), "{l} still wears its realm");
            assert!(ui.find(realmless(l).as_str()).is_ok(), "{l} by name");
        }
    }

    /// v29 in the inspector: a pair opened on Taken is about what hit
    /// them — its rate is dtps, and each side's list carries its R17
    /// record under the heading.
    #[test]
    fn a_taken_pair_words_itself_as_damage_taken() {
        let (mut state, mut mock) = tk::taken_kill();
        let reqs = state.set_follow(true);
        pump(&mut state, &mut mock, reqs);
        tk::apply(&mut state, &mut mock, Action::PickCompare);
        tk::apply(&mut state, &mut mock, Action::Down);
        assert_eq!(state.screen, Screen::Compare);
        assert_eq!(state.compare_view(), View::Taken);
        let (a, b) = state
            .compare_sides()
            .map(|(a, b)| (a.clone(), b.clone()))
            .unwrap();
        let (gui, _peer) = tk::gui_over(state);
        let insp = Insp::of(&gui);
        assert_eq!(
            (insp.nums[0].small.as_str(), insp.nums[1].small.as_str()),
            ("dtps", "dtps")
        );
        let record = |s: &CompareSide| {
            s.mitigation
                .as_ref()
                .map(|m| mitigation_line(m, s.total.amount))
        };
        assert!(record(&a).is_some(), "a Taken side carries one");
        match &insp.body {
            Body::Pair(p) => {
                let (x, y) = (&p.0, &p.1);
                assert_eq!((x.note.clone(), y.note.clone()), (record(&a), record(&b)));
                assert_eq!(x.view, View::Taken);
            }
            other => panic!("a pair's two lists: {other:?}"),
        }
        let mut ui = tk::wide(crate::view::view(&gui));
        let line = record(&a).unwrap();
        assert!(ui.find(line.as_str()).is_ok(), "R17's record per side");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// Healing words its graph in hps, and ranks everyone among their own
    /// role, as the prototype's `roleRank` does: a healer among the
    /// healers, a dps's healing among the dps.
    #[test]
    fn healing_graphs_hps_and_ranks_everyone_in_their_role() {
        let mut b = on_the_kill();
        b.send(chr("h"));
        let rows = b.gui.state.rows();
        assert!(rows.iter().any(|r| r.amount > 0), "someone healed");
        for (i, r) in rows.iter().enumerate() {
            b.send(Message::MeterRow(i));
            let insp = Insp::of(&b.gui);
            let fourth = insp
                .nums
                .get(3)
                .map(|n| (n.label.as_str(), n.small.clone()));
            assert_eq!(
                fourth,
                Some(("Rank", Place::of(&rows, r).tail())),
                "{}",
                r.label
            );
            if let Some(g) = &insp.graph {
                assert!(g.lead.starts_with("hps"), "{}", g.lead);
            }
        }
    }

    /// The owner's row in a person's list wears the "you" tag — an enemy's
    /// attackers are keyed by guid, and the owner is found by theirs.
    #[test]
    fn a_person_s_list_tags_the_owner() {
        let (state, _mock) = following(View::EnemyTaken);
        let (_, attackers) = state.breakdown();
        let first = attackers
            .first()
            .cloned()
            .expect("the fixture's top enemy was hit by someone");
        let (mut gui, _peer) = tk::gui_over(state);
        gui.owner_guid = Some(first.key.clone());
        let insp = Insp::of(&gui);
        match &insp.body {
            Body::One(l) => assert_eq!(l.you, Some(0), "the owner's row"),
            other => panic!("the attackers: {other:?}"),
        }
        let mut ui = tk::wide(crate::view::view(&gui));
        assert!(ui.find("you").is_ok(), "the tag");
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
    }

    /// Where the inspector's scrollable stands: its bounds, its content's
    /// and how far it is scrolled.
    struct Scrolled {
        id: iced::widget::Id,
        at: Option<(iced::Rectangle, iced::Rectangle, iced::Vector)>,
    }

    impl iced::advanced::widget::Operation for Scrolled {
        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation),
        ) {
            operate(self);
        }

        fn scrollable(
            &mut self,
            id: Option<&iced::widget::Id>,
            bounds: iced::Rectangle,
            content: iced::Rectangle,
            translation: iced::Vector,
            _state: &mut dyn iced::advanced::widget::operation::Scrollable,
        ) {
            if id == Some(&self.id) {
                self.at = Some((bounds, content, translation));
            }
        }
    }

    /// With the keys in the inspector its list's row wears the id the
    /// window scrolls into sight: run through the widget tree as the
    /// window runs it — `FindRow` finds the row's place in the list's
    /// content, chains into the reveal, and the inspector's scrollable
    /// ends scrolled just far enough that the row is whole in sight.
    #[test]
    fn the_keyed_row_is_found_and_revealed() {
        use iced::advanced::widget::Operation;
        use iced::advanced::widget::operation::Outcome;
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        let mut b = on_the_kill();
        let short = iced::Size::new(1440.0, 420.0);
        let mut renderer = tk::renderer();
        let found = |b: &Bridge, renderer: &mut iced::Renderer| {
            let mut ui =
                UserInterface::build(crate::view::view(&b.gui), short, Cache::default(), renderer);
            let mut op = crate::window::FindRow {
                scroll: scroll_id(),
                row: keyed_row_id(),
                content: None,
                found: None,
            };
            ui.operate(renderer, &mut op);
            op.found
        };
        assert_eq!(
            found(&b, &mut renderer),
            None,
            "no keys in the list, no keyed row"
        );
        b.send(named(Named::Enter));
        let (by_spell, _) = b.gui.state.breakdown();
        assert!(by_spell.len() > 1, "a list to walk");
        for _ in 1..by_spell.len() {
            b.send(chr("j"));
        }
        let mut ui = UserInterface::build(
            crate::view::view(&b.gui),
            short,
            Cache::default(),
            &mut renderer,
        );
        let mut seen = Scrolled {
            id: scroll_id(),
            at: None,
        };
        ui.operate(&renderer, &mut seen);
        let (bounds, content, before) = seen.at.expect("the inspector's scrollable");
        assert_eq!(before.y, 0.0, "not scrolled yet");
        let mut find = crate::window::FindRow {
            scroll: scroll_id(),
            row: keyed_row_id(),
            content: None,
            found: None,
        };
        ui.operate(&renderer, &mut find);
        let row = find.found.expect("the keyed row");
        let (top, bottom) = (row.y - content.y, row.y + row.height - content.y);
        assert!(
            bottom > bounds.height,
            "the case is real: the last row starts under the fold ({bottom} in {})",
            bounds.height
        );
        let Outcome::Chain(mut reveal) = find.finish() else {
            panic!("FindRow chains into the reveal");
        };
        ui.operate(&renderer, reveal.as_mut());
        let mut seen = Scrolled {
            id: scroll_id(),
            at: None,
        };
        ui.operate(&renderer, &mut seen);
        let (_, _, after) = seen.at.expect("the inspector's scrollable");
        let offset = after.y;
        assert!(
            top >= offset - 0.5 && bottom <= offset + bounds.height + 0.5,
            "the row whole in sight: {top}..{bottom} in {offset}..{}",
            offset + bounds.height
        );
        assert!(
            (bottom - (offset + bounds.height)).abs() < 0.5,
            "just far enough: its foot on the fold"
        );
    }

    /// The rate is the prototype's 10 s buckets over a whole fight (finer
    /// only for a stretch too short to hold forty), each bucket's sum over
    /// the seconds it spans: a flat fight is flat to its last, short
    /// bucket, and a step lands in the bucket it happened in.
    #[test]
    fn the_rate_is_ten_second_buckets() {
        assert_eq!(rate_bucket(422_040, 1_000), 10_000, "a raid boss");
        assert_eq!(rate_bucket(3_600_000, 1_000), 10_000, "never coarser");
        assert_eq!(rate_bucket(180_000, 1_000), 4_000, "a short pull");
        assert_eq!(rate_bucket(20_000, 1_000), 1_000, "a zoom: its own grid");
        let t = |buckets: Vec<u64>| Timeline {
            bucket_ms: 1_000,
            buckets,
            marks: Vec::new(),
        };
        let (flat, bucket) = bucket_rate(&t(vec![500; 42]), 10_000);
        assert_eq!(bucket, 10_000);
        assert_eq!(flat.len(), 5);
        assert!(
            flat.iter().all(|v| (v - 500.0).abs() < 1e-9),
            "flat, the 2 s last bucket too: {flat:?}"
        );
        let (step, _) = bucket_rate(&t([vec![0; 20], vec![1_000; 20]].concat()), 10_000);
        assert_eq!(step, [0.0, 0.0, 1_000.0, 1_000.0]);
        let (sum, grid) = curve(&t(vec![5, 0, 10]), GraphMode::Total, 10_000);
        assert_eq!(
            (sum, grid),
            (vec![5.0, 5.0, 15.0], 1_000),
            "a running total"
        );
        assert_eq!(bucket_rate(&t(Vec::new()), 10_000).0, Vec::<f64>::new());
    }

    /// An ability's figures, per view: its total, share, hits, crit and
    /// average — "—" for a crit and an average of nothing — and the view's
    /// extra only when there is one: overkill, overheal, absorbed.
    #[test]
    fn ability_nums_word_each_view_s_extra() {
        let r = Row {
            amount: 12_000,
            extra: 3_000,
            count: 4,
            crits: 1,
            pct: 25.0,
            ..Row::default()
        };
        let words = |view| {
            ability_nums(&r, view)
                .into_iter()
                .map(|n| format!("{} {}", n.label, n.value))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            words(View::Damage),
            [
                "Total 12.0k",
                "Share 25.0%",
                "Hits 4",
                "Crit 25.0%",
                "Avg 3.0k",
                "Overkill 3.0k"
            ]
        );
        assert_eq!(words(View::Healing).last().unwrap(), "Overheal 3.0k");
        assert_eq!(words(View::Taken).last().unwrap(), "Absorbed 3.0k");
        let none = Row {
            extra: 0,
            count: 0,
            ..r
        };
        let nums = ability_nums(&none, View::Damage);
        assert_eq!(nums.len(), 5, "no extra, no sixth");
        assert_eq!((nums[3].value.as_str(), nums[4].value.as_str()), ("—", "—"));
    }

    /// R23's spans as the plot hatches them: to the rez that ended one
    /// ("rezzed by" the rezzer the window has seen), to the fight's end
    /// with none, and a self-rez (a Soulstone they clicked) by its spell.
    #[test]
    fn dead_spans_end_at_the_rez_or_the_fight_s_end() {
        let state = tk::raid(2);
        let rows = state.rows();
        let mut roster = Roster::default();
        roster.observe(&rows);
        let death = |label: &str, at_ms, dur_ms, src: &str| Mark {
            at_ms,
            kind: MarkKind::Death,
            label: label.to_string(),
            spell_id: 0,
            dur_ms,
            src: src.to_string(),
        };
        let t = Timeline {
            bucket_ms: 1_000,
            buckets: vec![0; 60],
            marks: vec![
                death("Death (Rebirth)", 10_000, 5_000, &rows[1].key),
                death("Death", 30_000, 2_000, ""),
                death("Death (Soulstone)", 40_000, 4_000, ""),
            ],
        };
        let spans = dead_spans(&t, 60_000, Some("Swampert"), &roster);
        let got: Vec<(i64, i64, &str)> = spans
            .iter()
            .map(|d| (d.at_ms, d.end_ms, d.words.as_str()))
            .collect();
        let rezzer = format!(
            "Swampert died 0:10, rezzed by {}",
            display_name(&rows[1].label)
        );
        assert_eq!(
            got,
            [
                (10_000, 15_000, rezzer.as_str()),
                (30_000, 60_000, "Swampert died 0:30"),
                (40_000, 44_000, "Swampert died 0:40, Soulstone"),
            ]
        );
    }

    /// The recap runs as the death happened — oldest first — each event's
    /// change signed and inked, its source quiet after it (the player's
    /// own "yours"), and the killing blow's row, last, tinted.
    #[test]
    fn the_recap_runs_oldest_first_to_the_killing_blow() {
        let ev = |label: &str, amount, gain, hp| Row {
            label: label.to_string(),
            amount,
            gain,
            hp: Some((hp, 100_000)),
            ..Row::default()
        };
        // Newest first, as the daemon sends them.
        let recap = Recap {
            rows: vec![
                ev("Coalesced Venom (Zul'jan)", 8_883, false, 0),
                ev("Beacon of Light (Yourhonour)", 3_050, true, 1_200),
                ev("Burning Rush (Tranqlock)", 30_660, false, 5_200),
            ],
            who: "Tranqlock".to_string(),
            yours: true,
            class: Some(Class::Warlock),
        };
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(420.0, 200.0),
            recap_list(&recap, Fit::Wide),
        );
        for w in ["Change", "Last events, oldest first", "Health after"] {
            assert!(ui.find(w).is_ok(), "{w}");
        }
        let y = |ui: &mut iced_test::Simulator<'_, Message>, w: &str| {
            ui.find(w).map(|t| t.bounds().y).expect(w)
        };
        let (rush, beacon, venom) = (
            y(&mut ui, "Burning Rush"),
            y(&mut ui, "Beacon of Light"),
            y(&mut ui, "Coalesced Venom"),
        );
        assert!(rush < beacon && beacon < venom, "oldest first");
        assert!(ui.find("yours").is_ok(), "their own, as theirs");
        assert!(ui.find("Zul'jan").is_ok(), "a source, quiet after it");
        assert!(ui.find("\u{2212}8,883").is_ok(), "a hit, signed");
        assert!(ui.find("+3,050").is_ok(), "a heal, signed");
        let px = tk::pixels(
            recap_list(&recap, Fit::Wide),
            iced::Size::new(420.0, 200.0),
            &theme::window_theme(),
        );
        assert!(px.count(theme::AMBER, 2) > 0, "a low health bar in amber");
    }

    /// A head's names end in "…" rather than be cut through a glyph: a
    /// pair's two long names stay on one line inside the column, the
    /// joining word between them.
    #[test]
    fn the_head_names_give_way_with_a_mark() {
        let (a, and, b) = ("Tranqlock-Proudmoore-US", " and ", "Swampert-Proudmoore-US");
        let pieces = vec![
            (a.to_string(), theme::INK, theme::UI_SEMIBOLD),
            (and.to_string(), theme::INK_2, theme::UI),
            (b.to_string(), theme::INK, theme::UI_SEMIBOLD),
        ];
        let width = 300.0;
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(width, 40.0),
            name_line(&pieces),
        );
        let bounds = |ui: &mut iced_test::Simulator<'_, Message>, w: &str| {
            ui.find(w).map(|t| t.bounds()).expect(w)
        };
        let (ra, rb) = (bounds(&mut ui, a), bounds(&mut ui, b));
        assert!(ra.x + ra.width <= rb.x, "one line, in order: {ra:?} {rb:?}");
        assert!(rb.x + rb.width <= width + 0.5, "inside the column: {rb:?}");
    }

    /// The prototype's Tranqlock, newest first as the daemon sends it: a
    /// Gravebound, their own Soul Leech and Burning Rush, three Coalesced
    /// Venom hits round a Beacon of Light, each with its time before the
    /// death (v35).
    fn tranqlock() -> Recap {
        let ev = |label: &str, amount: u64, gain: bool, hp: u64, offset_ms: i64| Row {
            key: label.to_string(),
            label: label.to_string(),
            amount,
            gain,
            hp: Some((hp, 946_281)),
            offset_ms: Some(offset_ms),
            ..Row::default()
        };
        Recap {
            rows: vec![
                ev("Coalesced Venom (Zul'jan)", 8_883, false, 0, 0),
                ev("Beacon of Light (Yourhonour)", 3_050, true, 7_329, -600),
                ev("Coalesced Venom (Zul'jan)", 8_833, false, 4_279, -1_450),
                ev("Coalesced Venom (Zul'jan)", 5_892, false, 13_112, -2_900),
                ev("Burning Rush (Tranqlock-X)", 30_660, false, 19_004, -3_800),
                ev("Soul Leech (Tranqlock-X)", 2_270, true, 49_664, -4_100),
                ev(
                    "Gravebound (Hex Lord Malacrass)",
                    82_509,
                    false,
                    47_394,
                    -5_250,
                ),
            ],
            who: "Tranqlock-X".to_string(),
            yours: true,
            class: Some(Class::Warlock),
        }
    }

    /// v35: the insight calls out what the player did to themselves — the
    /// biggest own hit, the health it found them at and what finished them
    /// — the owner's worded as theirs, anyone else's by name, and nothing
    /// when every hit was someone else's.
    #[test]
    fn the_insight_calls_out_damage_the_player_did_to_themselves() {
        let words = |r: &Recap| -> Option<String> {
            insight(r).map(|p| p.into_iter().map(|(s, _)| s).collect())
        };
        let mine = tranqlock();
        assert_eq!(
            words(&mine).as_deref(),
            Some(
                "Your own Burning Rush took 30,660 while you were at 5.2% health. \
                 Three Coalesced Venom hits finished it."
            )
        );
        let bold: Vec<String> = insight(&mine)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, b)| *b)
            .map(|(s, _)| s)
            .collect();
        assert_eq!(bold, ["Burning Rush"], "the ability stands out");
        let theirs = Recap {
            yours: false,
            ..tranqlock()
        };
        assert!(
            words(&theirs).is_some_and(|w| w.starts_with("Tranqlock's own Burning Rush")
                && w.contains("while they were at")),
            "{:?}",
            words(&theirs)
        );
        let clean = Recap {
            who: "Somebody".to_string(),
            ..tranqlock()
        };
        assert_eq!(words(&clean), None);
    }

    /// v35: the recap's time column reads each event's time before the
    /// death, the killing blow's at 0 — the list opening right under the
    /// tabs, and the insight after it, its conclusion (`recapPanel`'s
    /// `</div><p class="insight">`).
    #[test]
    fn the_recap_reads_each_events_time_before_the_death() {
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(460.0, 420.0),
            recap_list(&tranqlock(), Fit::Wide),
        );
        assert!(ui.find("Time").is_ok());
        let y = |ui: &mut iced_test::Simulator<'_, Message>, w: &str| {
            ui.find(w).map(|t| t.bounds().y).expect(w)
        };
        let (grave, kill) = (y(&mut ui, "\u{2212}5.25s"), y(&mut ui, "0.00s"));
        assert!(grave < kill, "oldest first, down to the death");
        assert!(ui.find("\u{2212}3.80s").is_ok(), "the Burning Rush's");
        let heads = (y(&mut ui, "Time") * 2.0) as u32;
        let last = ((kill + RECAP_ROW_H) * 2.0) as u32;
        let px = tk::pixels(
            recap_list(&tranqlock(), Fit::Wide),
            iced::Size::new(460.0, 420.0),
            &theme::window_theme(),
        );
        let wash = Color {
            a: INSIGHT_WASH,
            ..theme::class_rgb(Class::Warlock)
        };
        // The insight's wash over the panel: some pixel is the blend.
        let over = |a: f32, x: f32, y: f32| a * x + (1.0 - a) * y;
        let ground = theme::GROUND;
        let blend = Color::from_rgb(
            over(wash.a, wash.r, ground.r),
            over(wash.a, wash.g, ground.g),
            over(wash.a, wash.b, ground.b),
        );
        assert!(px.count(blend, 6) > 100, "the insight's wash");
        // The wash stands after the last row, never over the heads (left of
        // the health bars, whose faint track reads near it; a glyph's
        // antialiased edge may pass for it).
        let w = px.w * 6 / 10;
        let (above, below) = (
            px.count_in((0, 0, w, heads), blend, 3),
            px.count_in((0, last, w, px.h), blend, 3),
        );
        assert!(
            below > 20 * above.max(1),
            "{above} over the heads, {below} after the list"
        );
    }

    /// A timed recap's time and change columns are as wide as their widest
    /// figure — the room a 78 px change column left as air goes to the
    /// events' names — an untimed one keeps the prototype's 78, and a
    /// tile's health bar is shorter. Every event keeps its own row: a run of
    /// small heals is as many rows, each with its own time and health.
    #[test]
    fn the_recap_columns_fit_their_figures() {
        // A simulator first, so the window's fonts are what measures.
        let _ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(460.0, 420.0),
            recap_list(&tranqlock(), Fit::Wide),
        );
        let cols = RecapCols::of(&tranqlock(), Fit::Wide);
        let widest = text_w("\u{2212}82,509", RECAP_PX, theme::UI_MEDIUM).ceil();
        assert!(
            cols.change >= widest && cols.change < RECAP_CHANGE_W - 15.0,
            "{cols:?}"
        );
        assert!(cols.time.is_some_and(|t| t <= RECAP_TIME_W), "{cols:?}");
        assert_eq!(cols.hp, RECAP_HP_W);
        assert_eq!(RecapCols::of(&tranqlock(), Fit::Tile).hp, RECAP_HP_W_TILE);
        let untimed = Recap {
            rows: tranqlock()
                .rows
                .into_iter()
                .map(|r| Row {
                    offset_ms: None,
                    ..r
                })
                .collect(),
            ..tranqlock()
        };
        let cols = RecapCols::of(&untimed, Fit::Wide);
        assert_eq!((cols.time, cols.change), (None, RECAP_CHANGE_W));
        // Every event its own row: the three Coalesced Venom hits are three.
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(460.0, 420.0),
            recap_list(&tranqlock(), Fit::Wide),
        );
        assert_eq!(count(&mut ui, "Coalesced Venom"), 3, "one row an event");
    }

    /// Health the log did not report reads as unknown — a dash and a dim,
    /// empty track — never as blank space, and never as a player at 0
    /// before the blow: a heal's 0 is the killing blow's report filled in
    /// beside it.
    #[test]
    fn unknown_health_is_a_dash_never_a_death() {
        let mut r = tranqlock();
        // Newest first: the heal just before the blow, at "0".
        r.rows[1].hp = Some((0, 946_281));
        r.rows[5].hp = None;
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(460.0, 420.0),
            recap_list(&r, Fit::Wide),
        );
        let dashes = count(&mut ui, RECAP_HP_UNKNOWN);
        assert_eq!(dashes, 2, "the heal's 0 and the missing report");
    }
    /// How many texts in `ui` read exactly `words`.
    fn count(ui: &mut iced_test::Simulator<'_, Message>, words: &str) -> usize {
        use iced_test::selector::Candidate;
        let mut n = 0;
        let _ = ui.find(|c: Candidate<'_>| {
            if matches!(c, Candidate::Text { content, .. } if content == words) {
                n += 1;
            }
            None::<()>
        });
        n
    }

    /// v35: a hit of their own is named only when it mattered — at least
    /// 5 % of their health, or found them under 30 % with the death soon
    /// after; a Burning Rush tick at 80 % health killed nobody.
    #[test]
    fn the_insight_names_only_a_material_hit_of_their_own() {
        let ev = |label: &str, amount: u64, gain: bool, hp: u64, offset_ms: i64| Row {
            label: label.to_string(),
            amount,
            gain,
            hp: Some((hp, 1_000_000)),
            offset_ms: Some(offset_ms),
            ..Row::default()
        };
        let recap = |rows: Vec<Row>| Recap {
            rows,
            who: "Tranqlock-X".to_string(),
            yours: true,
            class: Some(Class::Warlock),
        };
        // A 300 tick at 80 %, the death eight seconds on: nothing.
        let tick = recap(vec![
            ev("Venom Rupture (Zul'jan)", 900_000, false, 0, 0),
            ev("Burning Rush (Tranqlock-X)", 300, false, 799_700, -8_000),
            ev("Gravebound (Hex)", 200_000, false, 800_000, -9_000),
        ]);
        assert!(insight(&tick).is_none());
        // The same tick at 5 % with the death a moment on: named.
        let low = recap(vec![
            ev("Venom Rupture (Zul'jan)", 49_700, false, 0, 0),
            ev("Burning Rush (Tranqlock-X)", 300, false, 49_700, -400),
            ev("Gravebound (Hex)", 950_000, false, 50_000, -900),
        ]);
        assert!(insight(&low).is_some());
        // A big one of their own, whatever their health: named.
        let big = recap(vec![
            ev("Venom Rupture (Zul'jan)", 700_000, false, 0, 0),
            ev("Soul Burn (Tranqlock-X)", 60_000, false, 700_000, -8_000),
            ev("Gravebound (Hex)", 240_000, false, 760_000, -9_000),
        ]);
        assert!(insight(&big).is_some());
    }
}
