//! The timeline graph's model (R12, R18, R23, v34): what the comparison's
//! and the drill's graphs draw, free of any GUI — the marks a view draws,
//! their names and colours, the curves and the shared peak, the displayed
//! window, the hover and probe wording, and the plot's geometry ([`Plot`]).
//! The renderers (the iced overlay's `compare.rs`, gui-new's overlay graph)
//! draw from it.
//!
//! Two graphs of a comparison share one y-scale and one x-range: two
//! curves drawn to their own maxima look identical however far apart the
//! players are, which is the one thing a comparison must not do.

use wowdps_model::fmt::human;
use wowdps_model::{GraphMode, Mark, MarkKind, Timeline, View};
use wowdps_proto::CompareSide;

use crate::theme::{Color, GOLD};

// ---- marks ----------------------------------------------------------------

/// Marker colours. Distinct hues rather than shades: at graph width these
/// bars are one or two pixels wide, and a shade difference is invisible.
pub const USE: Color = Color::rgb(1.0, 0.85, 0.35);
pub const PROC: Color = Color::rgb(0.45, 0.85, 1.0);
/// A consumable wears the overlay's green.
pub const CONSUMABLE: Color = GOLD.overlay.good;
/// v13: externals (Bloodlust, Power Infusion) — violet, nothing else is.
pub const EXTERNAL: Color = Color::rgb(0.85, 0.55, 1.0);
/// R18: active mitigation AND defensives — coral. Both are "the player
/// pressed something to take less"; the legend key and the hover name tell
/// them apart, a second warm hue would not at bar width.
pub const MITIGATION: Color = Color::rgb(1.0, 0.45, 0.40);
/// R18: support buffs on the player (Ebon Might, Prescience) — teal.
pub const SUPPORT: Color = Color::rgb(0.35, 0.90, 0.80);
/// R18: an offensive cooldown's window — the External bar's twin (a burst
/// window either way), so it stays in the violet family but steps to
/// indigo: a hue away from EXTERNAL's lavender, not a shade of it.
pub const COOLDOWN: Color = Color::rgb(0.50, 0.40, 1.0);
/// R23: the window the player spent DEAD — bone grey, the one mark that is
/// not something they pressed. Deliberately colourless: every other bar is
/// a hue that means "look here", and this one means "nothing happened
/// here".
pub const DEAD: Color = Color::rgb(0.62, 0.62, 0.66);
/// v34: a healing cooldown's window — spring green, a hue away from
/// CONSUMABLE's green and SUPPORT's teal: the healing graph's own burst
/// window, never confused with a potion beside it.
pub const HEALING_CD: Color = Color::rgb(0.55, 1.0, 0.45);

pub fn mark_color(kind: MarkKind) -> Color {
    match kind {
        MarkKind::TrinketUse => USE,
        MarkKind::TrinketProc => PROC,
        MarkKind::Consumable => CONSUMABLE,
        MarkKind::External => EXTERNAL,
        MarkKind::ActiveMitigation | MarkKind::Defensive => MITIGATION,
        MarkKind::SupportBuff => SUPPORT,
        MarkKind::Cooldown => COOLDOWN,
        MarkKind::Death => DEAD,
        MarkKind::HealingCooldown => HEALING_CD,
    }
}

pub fn mark_name(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::TrinketUse => "trinket use",
        MarkKind::TrinketProc => "proc",
        MarkKind::Consumable => "consumable",
        MarkKind::External => "external",
        MarkKind::ActiveMitigation => "mitigation",
        MarkKind::Defensive => "defensive",
        MarkKind::SupportBuff => "support",
        MarkKind::Cooldown => "cooldown",
        MarkKind::Death => "dead",
        MarkKind::HealingCooldown => "healing cd",
    }
}

/// R18: every kind, in wire-code order — the legend's key order.
pub const ALL_KINDS: [MarkKind; 10] = [
    MarkKind::TrinketUse,
    MarkKind::TrinketProc,
    MarkKind::Consumable,
    MarkKind::External,
    MarkKind::ActiveMitigation,
    MarkKind::Defensive,
    MarkKind::SupportBuff,
    MarkKind::Cooldown,
    MarkKind::Death,
    MarkKind::HealingCooldown,
];

/// v34: whether a graph drawn for `view` shows marks of `kind`. The daemon
/// sends every mark on every timeline; the VIEW decides which windows are
/// about its metric. Items, externals and deaths ride every graph (a potion
/// or a Bloodlust reads on any curve, a death explains a flat stretch of
/// any of them); an offensive cooldown belongs on the damage and healing
/// curves (a hybrid's Avenging Wrath is both) and never on damage taken; a
/// healing cooldown only on healing; mitigation and defensives only on
/// damage taken — that is the graph where "pressed something to take less"
/// has a dent to sit under.
pub fn view_draws(view: View, kind: MarkKind) -> bool {
    match kind {
        MarkKind::TrinketUse
        | MarkKind::TrinketProc
        | MarkKind::Consumable
        | MarkKind::External
        | MarkKind::Death => true,
        MarkKind::SupportBuff | MarkKind::Cooldown => view != View::Taken,
        MarkKind::HealingCooldown => view == View::Healing,
        MarkKind::ActiveMitigation | MarkKind::Defensive => view == View::Taken,
    }
}

/// The Healthstone's spell id (every flavour of the warlock's stone writes it).
pub const HEALTHSTONE: u32 = 6262;

/// Whether a graph drawn for `view` shows this particular mark: its kind's
/// verdict ([`view_draws`]) with one item-level exception — a Healthstone
/// is a Consumable, but it heals: it explains a dent on the taken curve
/// and a bump on the healing curve, and only crowds a damage graph, so
/// the Damage view alone leaves it out.
pub fn view_draws_mark(view: View, m: &Mark) -> bool {
    view_draws(view, m.kind)
        && !(view == View::Damage
            && m.kind == MarkKind::Consumable
            && (m.spell_id == HEALTHSTONE || m.label == "Healthstone"))
}

/// v34: the timeline with only the marks `view` draws (see
/// [`view_draws_mark`]); buckets untouched.
pub fn for_view(t: &Timeline, view: View) -> Timeline {
    Timeline {
        bucket_ms: t.bucket_ms,
        buckets: t.buckets.clone(),
        marks: t
            .marks
            .iter()
            .filter(|m| view_draws_mark(view, m))
            .cloned()
            .collect(),
    }
}

/// v34: a comparison side with its timelines narrowed to `view`'s marks.
pub fn side_for_view(side: &CompareSide, view: View) -> CompareSide {
    CompareSide {
        timeline: for_view(&side.timeline, view),
        spell_timeline: side.spell_timeline.as_ref().map(|t| for_view(t, view)),
        ..side.clone()
    }
}

/// R18: the kinds with a mark inside the displayed window, in `ALL_KINDS`
/// order. The legend keys only these — a DPS's graph never explains a
/// mitigation key, and a tank's never a proc it has no marks for.
pub fn kinds_shown(timelines: &[&Timeline], view: (usize, usize)) -> Vec<MarkKind> {
    // A span that began before the window but runs into it is drawn, so it
    // earns its key too: the visible test is on the span, not its start.
    let visible = |t: &Timeline, m: &Mark| {
        let bucket = t.bucket_ms.max(1) as f64;
        let start = m.at_ms as f64 / bucket;
        let end = (m.at_ms + m.dur_ms.max(0)) as f64 / bucket;
        end >= view.0 as f64 && start <= view.1 as f64
    };
    ALL_KINDS
        .into_iter()
        .filter(|k| {
            timelines
                .iter()
                .any(|t| t.marks.iter().any(|m| m.kind == *k && visible(t, m)))
        })
        .collect()
}

/// R18: a mark's caster worded for the hover — the segment's name for the
/// guid when the renderer has one (`names` pairs guid → name), else the
/// guid's tail, which is at least stable across the two graphs.
pub fn caster_name<'a>(src: &'a str, names: &[(&str, &'a str)]) -> &'a str {
    names
        .iter()
        .find(|(g, _)| *g == src)
        .map(|(_, n)| *n)
        .unwrap_or_else(|| src.rsplit('-').next().unwrap_or(src))
}

// ---- wording ----------------------------------------------------------------

/// The hovered item summarized for the legend row — kind, name, and a
/// details clause (uses, uptime, share of the displayed window). Computed
/// over every displayed timeline, so a comparison counts both players' uses.
/// R18: a mark with a caster names them — "Power Infusion from Gennar" —
/// every distinct caster of that label, resolved through `names`. Only when
/// `with_caster`: the window has the width, the overlay's legend row does
/// not, and a wrapped legend line misbehaves in the layer-shell surface, so
/// the overlay shows the bare label.
pub fn hover_line(
    timelines: &[&Timeline],
    label: &str,
    view: (usize, usize),
    names: &[(&str, &str)],
    with_caster: bool,
    // v27: what to call each timeline, when there is more than one. A
    // comparison's total ("×5") hides the answer the reader wants — WHO
    // popped it — so each side's own count follows it.
    sides: &[String],
) -> Option<(MarkKind, String, String)> {
    let per_side: Vec<usize> = timelines
        .iter()
        .map(|t| t.marks.iter().filter(|m| m.label == label).count())
        .collect();
    let same: Vec<&Mark> = timelines
        .iter()
        .flat_map(|t| t.marks.iter())
        .filter(|m| m.label == label)
        .collect();
    let first = same.first()?;
    let mut casters: Vec<&str> = Vec::new();
    for m in same.iter().filter(|m| !m.src.is_empty()) {
        let who = caster_name(&m.src, names);
        if !casters.contains(&who) {
            casters.push(who);
        }
    }
    let name = if casters.is_empty() || !with_caster {
        label.to_string()
    } else {
        format!("{label} from {}", casters.join(", "))
    };
    let mut details = format!("{} ×{}", mark_name(first.kind), same.len());
    // The split, when there are two sides to split: "Externals ×5 (Alice 3 ·
    // Bob 2)". A side with none is still named — "Bob 0" is the finding.
    if sides.len() > 1 && sides.len() == per_side.len() {
        let split: Vec<String> = sides
            .iter()
            .zip(&per_side)
            .map(|(who, n)| format!("{who} {n}"))
            .collect();
        details.push_str(&format!(" ({})", split.join(" · ")));
    }
    let uptime_ms: i64 = same.iter().map(|m| m.dur_ms.max(0)).sum();
    if uptime_ms > 0 {
        let bucket_ms = timelines
            .iter()
            .map(|t| t.bucket_ms)
            .max()
            .unwrap_or(1)
            .max(1) as i64;
        let window_ms = (view.1 - view.0).max(1) as i64 * bucket_ms;
        let pct = (uptime_ms as f64 / window_ms as f64 * 100.0).min(100.0);
        // R23: a death span is not "uptime" of anything — it is time spent
        // dead, and `mark_name` already said the word.
        let word = if first.kind == MarkKind::Death {
            ""
        } else {
            "uptime "
        };
        details.push_str(&format!(" · {word}{}s · {pct:.0}%", uptime_ms / 1000));
    }
    Some((first.kind, name, details))
}

/// What the curve is called: the view's own rate word, or the cumulative
/// mode's label. Shared by the legend and the probe readout so the two can
/// never disagree about which curve is up.
pub fn mode_word(mode: GraphMode, rate: &'static str) -> &'static str {
    match mode {
        GraphMode::Dps => rate,
        GraphMode::Total => mode.label(),
    }
}

/// The probe readout: ONE instant, and what each curve says at it. With two
/// sides it names them — the whole reason the cursor is shared is reading
/// "who was ahead here", which a single number cannot answer. With one it
/// stays the bare "dps: 674.5k" the drilldown had.
///
/// `at` is a bucket of the shared grid; a side whose curve is shorter simply
/// has nothing to say there and is left out rather than reported as zero.
pub fn probe_line(
    at: usize,
    bucket_ms: usize,
    word: &str,
    sides: &[(String, Vec<f64>)],
) -> (String, Vec<String>) {
    let when = mmss((at * bucket_ms) as u32);
    let named = sides.len() > 1;
    let mut parts: Vec<String> = Vec::new();
    for (name, points) in sides {
        // A side whose curve is shorter has nothing to say at this instant,
        // but it still needs its SLOT: the legend puts each reading under
        // its own graph, so dropping one would slide the other across the
        // divider and label the wrong player's curve.
        let v = points.get(at).copied();
        parts.push(match (v, named) {
            (Some(v), true) => format!("{name} {}", human(v as u64)),
            // One side: the metric word rides the number, since there is no
            // pair of names to tell apart.
            (Some(v), false) => format!("{word}: {}", human(v as u64)),
            (None, true) => format!("{name} —"),
            (None, false) => String::new(),
        });
    }
    (when, parts)
}

/// "1:23" from ms — graph-axis wording for a moment inside the fight.
pub fn mmss(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// "Keanucleavês-Proudmoore" → "Keanucleavês"; character names cannot
/// contain a dash, so everything from the first one is realm noise.
pub fn short_name(label: &str) -> String {
    label.split('-').next().unwrap_or(label).to_string()
}

// ---- curves -----------------------------------------------------------------

/// The curve a mode draws, as plain points. Kept out of the canvas so the
/// peak (shared by both sides) can be computed the same way.
pub fn curve(t: &Timeline, mode: GraphMode) -> Vec<f64> {
    match mode {
        // A 15s window: long enough to survive a cast gap, short enough that
        // a trinket window still stands out as a bump.
        GraphMode::Dps => t.rolling_dps(15_000),
        GraphMode::Total => t.cumulative().into_iter().map(|v| v as f64).collect(),
    }
}

pub fn peak_of(timelines: &[&Timeline], mode: GraphMode, view: (usize, usize)) -> f64 {
    timelines
        .iter()
        .flat_map(|t| {
            let c = curve(t, mode);
            let hi = view.1.min(c.len());
            let lo = view.0.min(hi);
            c.into_iter().take(hi).skip(lo)
        })
        .fold(0.0f64, f64::max)
}

/// The displayed bucket window `[lo, hi)` for an echoed ms range. Anything
/// degenerate (a window past the data, a zero-width slice) falls back to the
/// whole span rather than a blank graph.
pub fn view_window(shown: Option<(u32, u32)>, bucket_ms: usize, span: usize) -> (usize, usize) {
    let Some((lo, hi)) = shown else {
        return (0, span);
    };
    let lo_b = lo as usize / bucket_ms;
    let hi_b = (hi as usize).div_ceil(bucket_ms).min(span);
    if lo_b < hi_b { (lo_b, hi_b) } else { (0, span) }
}

// ---- the plot -----------------------------------------------------------------

/// Marker icon strip metrics, in canvas units: the icons sit in a band along
/// the graph's top edge, and hovering that band is what lights an item up.
pub const ICON_SIZE: f32 = 16.0;
pub const ICON_BAND: f32 = 20.0;
/// A press-release wander below this is a click, not a selection.
pub const DRAG_MIN_PX: f32 = 3.0;

/// One graph's geometry: the curve's points and marks over the displayed
/// bucket window, scaled to `peak`. Every position is in the canvas's own
/// units, `w` wide and `h` tall.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot {
    pub points: Vec<f64>,
    pub marks: Vec<Mark>,
    pub bucket_ms: f64,
    pub peak: f64,
    /// Displayed bucket window `[lo, hi)`, shared by both graphs so the
    /// same instant is the same column in both; `(0, span)` when unzoomed.
    pub view: (usize, usize),
    /// v14: the Σ graph's encounter lane — `[lo, hi)` ms spans where the
    /// visit's boss fights ran, drawn along the bottom edge so an
    /// aggregated curve shows where the pulls were. Empty elsewhere.
    pub spans: Vec<(u32, u32)>,
}

impl Plot {
    /// `t`'s curve in `mode`, over `view`, scaled to `peak`.
    pub fn new(
        t: &Timeline,
        mode: GraphMode,
        peak: f64,
        view: (usize, usize),
        spans: Vec<(u32, u32)>,
    ) -> Self {
        Self {
            points: curve(t, mode),
            marks: t.marks.clone(),
            bucket_ms: t.bucket_ms.max(1) as f64,
            peak,
            view,
            spans,
        }
    }

    pub fn span(&self) -> f64 {
        (self.view.1 - self.view.0).max(1) as f64
    }

    /// Canvas x for a bucket position (fractional buckets fine).
    pub fn x_of(&self, bucket: f64, w: f32) -> f32 {
        ((bucket - self.view.0 as f64) / self.span()) as f32 * w
    }

    /// The ms-from-segment-start a canvas x lands on.
    pub fn ms_at(&self, x: f32, w: f32) -> u32 {
        let frac = (x / w).clamp(0.0, 1.0) as f64;
        let bucket = self.view.0 as f64 + frac * self.span();
        (bucket * self.bucket_ms).max(0.0) as u32
    }

    /// The marker whose icon the point (`x`, `y`) is over, if any.
    pub fn mark_at(&self, x: f32, y: f32, w: f32) -> Option<&Mark> {
        if y > ICON_BAND {
            return None;
        }
        self.marks
            .iter()
            .filter(|m| self.mark_visible(m))
            .min_by(|a, b| {
                let d = |m: &Mark| (self.mark_x(m, w) - x).abs();
                d(a).total_cmp(&d(b))
            })
            .filter(|m| (self.mark_x(m, w) - x).abs() <= ICON_SIZE / 2.0 + 2.0)
    }

    pub fn mark_x(&self, m: &Mark, w: f32) -> f32 {
        self.x_of(m.at_ms as f64 / self.bucket_ms, w)
    }

    /// The curve bucket and value under a canvas x, if the curve has one.
    pub fn probe_at(&self, x: f32, w: f32) -> Option<(usize, f64)> {
        let frac = (x / w).clamp(0.0, 1.0) as f64;
        let b = (self.view.0 as f64 + frac * self.span()).round() as usize;
        let b = b.min(self.view.1.saturating_sub(1));
        self.points.get(b).map(|v| (b, *v))
    }

    pub fn mark_visible(&self, m: &Mark) -> bool {
        let b = m.at_ms as f64 / self.bucket_ms;
        b >= self.view.0 as f64 && b <= self.view.1 as f64
    }

    /// The encounter lane's height at `scale`.
    pub fn lane_h(scale: f32) -> f32 {
        2.0 * scale
    }

    /// Where the curve's zero sits above the bottom edge: raised over the
    /// encounter lane when there is one, so the line never runs through it.
    pub fn floor(&self, scale: f32) -> f32 {
        if self.spans.is_empty() {
            0.0
        } else {
            Self::lane_h(scale) + 2.0 * scale
        }
    }

    /// The canvas y of value `v` on an `h`-tall graph. The curve's 100%
    /// mark sits well below the icon strip — two icon heights of air under
    /// the band — so a long fight's peaks never run into the icons; short
    /// graphs (the overlay's) cap the headroom at half their height rather
    /// than crushing the curve into a ribbon.
    pub fn y_of(&self, v: f64, h: f32, floor: f32) -> f32 {
        let curve_top = (ICON_BAND + 2.0 * ICON_SIZE).min(h * 0.5);
        if self.peak <= 0.0 {
            h - floor
        } else {
            h - floor - (v / self.peak) as f32 * (h - floor - curve_top).max(0.0)
        }
    }

    /// Where a marker's line stops: the curve at its instant — the higher
    /// of the curve and `ghost` there, when a ghost rides behind, so a
    /// focus line hugging the floor never lets every marker span the whole
    /// graph. Out-of-curve marks fall to the bottom.
    pub fn hang_y(&self, m: &Mark, ghost: Option<&[f64]>, h: f32, floor: f32) -> f32 {
        let b = (m.at_ms as f64 / self.bucket_ms).round() as usize;
        let own = self
            .points
            .get(b)
            .map(|v| self.y_of(*v, h, floor))
            .unwrap_or(h);
        let ghosted = ghost
            .and_then(|g| g.get(b))
            .map(|v| self.y_of(*v, h, floor))
            .unwrap_or(h);
        own.min(ghosted)
    }

    /// The ms window a drag from `a` to `b` selects, or `None` for a click
    /// (a wander under [`DRAG_MIN_PX`]).
    pub fn drag_range(&self, a: f32, b: f32, w: f32) -> Option<(u32, u32)> {
        if (b - a).abs() < DRAG_MIN_PX {
            return None;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        Some((self.ms_at(lo, w), self.ms_at(hi, w)))
    }
}

/// Timelines the graph's tests build on, in every crate that draws one.
#[doc(hidden)]
pub mod samples {
    use wowdps_model::{Mark, MarkKind, Timeline};

    pub fn timeline(buckets: Vec<u64>) -> Timeline {
        Timeline {
            bucket_ms: 1000,
            buckets,
            marks: Vec::new(),
        }
    }

    pub fn mark(at_ms: i64, kind: MarkKind, label: &str, dur_ms: i64) -> Mark {
        Mark {
            at_ms,
            kind,
            label: label.to_string(),
            spell_id: 0,
            dur_ms,
            src: String::new(),
        }
    }

    pub fn cast(at_ms: i64, kind: MarkKind, label: &str, dur_ms: i64, src: &str) -> Mark {
        Mark {
            src: src.to_string(),
            ..mark(at_ms, kind, label, dur_ms)
        }
    }

    /// Ten 1s buckets with a use at 2s (10s buff) and a proc at 7s.
    pub fn marked() -> Timeline {
        Timeline {
            bucket_ms: 1000,
            buckets: vec![100, 200, 300, 400, 500, 400, 300, 200, 100, 50],
            marks: vec![
                mark(2_000, MarkKind::TrinketUse, "Trinket", 10_000),
                mark(7_000, MarkKind::TrinketProc, "Proc", 0),
                mark(4_000, MarkKind::Consumable, "Potion", 25_000),
                mark(1_000, MarkKind::External, "Bloodlust", 40_000),
            ],
        }
    }

    /// R12's four item kinds — what the legend keyed unconditionally before
    /// R18 made the keys follow the marks on screen.
    pub const ITEM_KINDS: &[MarkKind] = &[
        MarkKind::TrinketUse,
        MarkKind::TrinketProc,
        MarkKind::Consumable,
        MarkKind::External,
    ];

    /// A tank's ten seconds: two Shield Blocks (own), a Pain Suppression
    /// from the priest, an Ebon Might from the evoker, a Combustion-shaped
    /// cooldown, plus one trinket proc — R12 marks and R18 spans side by
    /// side.
    pub fn role_marked() -> Timeline {
        Timeline {
            bucket_ms: 1000,
            buckets: vec![100; 10],
            marks: vec![
                cast(
                    1_000,
                    MarkKind::ActiveMitigation,
                    "Shield Block",
                    6_000,
                    "Player-1-0A",
                ),
                cast(
                    8_000,
                    MarkKind::ActiveMitigation,
                    "Shield Block",
                    2_000,
                    "Player-1-0A",
                ),
                cast(
                    2_000,
                    MarkKind::Defensive,
                    "Shield Wall",
                    8_000,
                    "Player-1-0A",
                ),
                cast(
                    3_000,
                    MarkKind::External,
                    "Pain Suppression",
                    8_000,
                    "Player-1-0B",
                ),
                cast(
                    4_000,
                    MarkKind::SupportBuff,
                    "Ebon Might",
                    10_000,
                    "Player-1-0E",
                ),
                cast(
                    5_000,
                    MarkKind::Cooldown,
                    "Combustion",
                    12_000,
                    "Player-1-0A",
                ),
                mark(7_000, MarkKind::TrinketProc, "Proc", 0),
                // R23: and the one span nobody cast — dead from 6 s, raised
                // 3 s later, the rezzer on it like any other caster.
                cast(
                    6_000,
                    MarkKind::Death,
                    "Death (Raise Ally)",
                    3_000,
                    "Player-1-0B",
                ),
                // v34: the priest's own healing window, on the tank's
                // graph only when it is the Healing one.
                cast(
                    9_000,
                    MarkKind::HealingCooldown,
                    "Apotheosis",
                    20_000,
                    "Player-1-0B",
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests;
