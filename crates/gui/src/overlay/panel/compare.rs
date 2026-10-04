//! R12: the overlay's comparison (plan step 2.5), as the iced overlay's
//! `compare_body` draws it: two panes side by side — each a header (badge,
//! name, amount, rate), a per-ability table (hits, crit, average; "hit by"
//! on Taken) with R17's mitigation line under it on Taken, and a graph — over
//! one legend. Both graphs share one y-scale and one x-range: two curves
//! drawn to their own maxima look alike however far apart the players
//! are, which is the one thing a comparison must not do.
//!
//! v18: a press on an ability drills BOTH sides into it (its crumb and
//! stat cards, its curve over the player's ghost); hovering one lights the
//! same ability in both lists, since the two are sorted apart.

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Div, MouseButton, SharedString, TestSupportExt as _, div, img, px,
};
use wowdps_gui_logic::drill::{avg_text, counts, crit_text};
use wowdps_gui_logic::graph::{
    Plot, curve, hover_line, kinds_shown, mode_word, peak_of, probe_line, short_name,
    side_for_view, table_words, view_window, waiting_words,
};
use wowdps_gui_logic::labels::rate_label;
use wowdps_gui_logic::theme::{Color, school_color};
use wowdps_model::fmt::{human, mitigation_line};
use wowdps_model::{GraphMode, Row, View};
use wowdps_proto::CompareSide;

use super::super::drill;
use super::super::graph::{self, Graph};
use super::super::ov::Ov;
use super::super::rows::class_icon;
use super::Overlay;
use crate::images;
use crate::theme::hsla;

/// Column widths for the per-ability table: (hits, crit%, average).
const COLS: (f32, f32, f32) = (44.0, 46.0, 56.0);

/// What both panes share: the curve's mode, the one scale, the window.
struct Scale {
    mode: GraphMode,
    peak: f64,
    view: (usize, usize),
    metric: View,
    spell: Option<(String, String)>,
}

impl Overlay {
    /// The comparison body: a wait before two sides are in hand, else the
    /// two panes over their legend.
    pub(super) fn compare(&self, ov: &Ov, cx: &mut Context<Self>) -> AnyElement {
        let state = self.state(cx);
        let Some((a, b)) = state.compare_sides() else {
            return div()
                .id("compare-wait")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(ov.words(waiting_words(state.compare_picks()), 13., ov.c(|t| t.dim)))
                .into_any_element();
        };
        let mode = state.graph_mode();
        // v29: the metric this comparison is about, from the daemon's echo.
        let metric = state.compare_view();
        // v34: the graphs draw only the marks this view is about.
        let (a, b) = (side_for_view(a, metric), side_for_view(b, metric));
        let span = a
            .timeline
            .buckets
            .len()
            .max(b.timeline.buckets.len())
            .max(1);
        // v12: the zoom follows the DAEMON's echo, so the graphs never zoom
        // ahead of the tables they sit under.
        let shown = state.compare_shown_range();
        let bms = a.timeline.bucket_ms.max(b.timeline.bucket_ms).max(1) as usize;
        let view = view_window(shown, bms, span);
        // One scale over both sides — and both focus curves while an
        // ability is drilled — over the displayed window.
        let mut scaled = vec![&a.timeline, &b.timeline];
        scaled.extend(
            [&a, &b]
                .into_iter()
                .filter_map(|s| s.spell_timeline.as_ref()),
        );
        let peak = peak_of(&scaled, mode, view);
        // v27: one instant, read on BOTH curves, each named.
        let rate = rate_label(metric);
        let probe = self.graph_probe.map(|at| {
            probe_line(
                at,
                bms,
                mode_word(mode, rate),
                &[
                    (short_name(&a.total.label), curve(&a.timeline, mode)),
                    (short_name(&b.total.label), curve(&b.timeline, mode)),
                ],
            )
        });
        // R18: casters resolve through the two sides and the meter rows.
        let rows = state.rows();
        let names: Vec<(&str, &str)> = [
            (a.guid.as_str(), a.total.label.as_str()),
            (b.guid.as_str(), b.total.label.as_str()),
        ]
        .into_iter()
        .chain(rows.iter().map(|r| (r.key.as_str(), r.label.as_str())))
        .collect();
        let timelines = [&a.timeline, &b.timeline];
        let sides = [short_name(&a.total.label), short_name(&b.total.label)];
        let hovered = self
            .graph_hover
            .as_deref()
            .and_then(|l| hover_line(&timelines, l, view, &names, false, &sides));
        let kinds = kinds_shown(&timelines, view);
        let scale = Scale {
            mode,
            peak,
            view,
            metric,
            spell: state.compare_spell().cloned(),
        };
        let legend = graph::legend(ov, mode, shown, probe, rate, hovered, &kinds);
        let panes = div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(10.))
            .child(self.side(ov, &a, &scale, 0, cx))
            .child(self.side(ov, &b, &scale, 1, cx));
        div()
            .id("compare")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(panes)
            .child(legend)
            .into_any_element()
    }

    /// One side: its header, then its table (or, with an ability drilled,
    /// that ability's crumb and stat cards), then its graph.
    fn side(
        &self,
        ov: &Ov,
        side: &CompareSide,
        s: &Scale,
        at: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        // The curve is data and keeps the raw class colour; so does the
        // overlay's name.
        let color = side.total.class.map_or(ov.t.classless, Color::of_class);
        let header = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(class_icon(
                ov,
                side.total.class,
                side.total.spec,
                true,
                ov.z(18.),
            ))
            .child(ov.words(short_name(&side.total.label), 14., hsla(color)))
            .child(div().flex_1())
            .child(ov.nums(human(side.total.amount), 13., ov.c(|t| t.text)))
            .child(ov.nums(
                format!(
                    "{} {}",
                    human(side.total.per_sec as u64),
                    rate_label(s.metric)
                ),
                12.,
                ov.c(|t| t.dim),
            ));
        let [first, second] = &self.graphs;
        let local = if at == 0 { first } else { second }.clone();
        let this = cx.entity().downgrade();
        let on: graph::OnEvent = Rc::new(move |e, _, cx| {
            let _ = this.update(cx, |o, cx| o.on_graph(e, cx));
        });
        let plot = |t: &wowdps_model::Timeline| Plot {
            points: curve(t, s.mode),
            marks: t.marks.clone(),
            bucket_ms: t.bucket_ms.max(1) as f64,
            peak: s.peak,
            view: s.view,
            // No encounter lane: while comparing, the cached snapshot can
            // lag the watched segment, so its spans may be another fight's.
            spans: Vec::new(),
        };
        let graph_of = |plot: Plot, color: Color, ghost: Option<(Vec<f64>, Color)>| Graph {
            plot,
            color,
            ghost,
            scale: ov.zoom,
            hover: self.graph_hover.clone(),
            probe: self.graph_probe,
            t: ov.t,
            data: ov.data,
        };
        let column = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(header);

        // v18: an ability drilled on both sides.
        if let Some((key, label)) = &s.spell {
            let row = side.spells.iter().find(|r| &r.key == key).cloned();
            let middle = match &row {
                Some(r) => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(ov.z(8.))
                    .child(drill::crumb(
                        ov,
                        &short_name(&side.total.label),
                        label,
                        Some(r),
                    ))
                    .child(drill::stats(ov, r, View::Damage)),
                // This side never cast it: its own line keeps the pane
                // comparable, dimmed by the shared scale as it is.
                None => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .justify_center()
                    .child(ov.words(format!("did not cast {label}"), 12., ov.c(|t| t.dim))),
            };
            let focus = row
                .and_then(|r| school_color(r.school))
                .unwrap_or(ov.t.yellow);
            let g = match &side.spell_timeline {
                Some(ft) => graph_of(
                    plot(ft),
                    focus,
                    Some((curve(&side.timeline, s.mode), color)),
                ),
                None => graph_of(plot(&side.timeline), color, None),
            };
            return column.child(middle).child(
                div()
                    .id(("compare-graph", at))
                    .test_support()
                    .child(g.element(ov.z(90.), local, on)),
            );
        }

        column
            .child(self.table(ov, &side.spells, s.metric, at, cx))
            .children(side.mitigation.as_ref().map(|m| {
                // R17: what this side AVOIDED, under what it took.
                div().px(px(6.)).overflow_hidden().child(ov.nums(
                    mitigation_line(m, side.total.amount),
                    9.,
                    ov.c(|t| t.dim),
                ))
            }))
            .child(
                div().id(("compare-graph", at)).test_support().child(
                    graph_of(plot(&side.timeline), color, None).element(ov.z(90.), local, on),
                ),
            )
    }

    /// The per-ability table: hits, crit and average — the numbers the
    /// comparison exists for — or a count view's one count. A press drills
    /// both sides into the ability; hovering lights it on both.
    fn table(
        &self,
        ov: &Ov,
        spells: &[Row],
        metric: View,
        at: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let (title, empty) = table_words(metric);
        let count_only = counts(metric);
        let dim = ov.c(|t| t.dim);
        let head =
            |s: &'static str, w: f32| ov.nums(s, 10., dim).w(ov.z(w)).flex_none().text_right();
        let mut heading = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .child(ov.words(title, 10., dim))
            .child(div().flex_1());
        heading = if count_only {
            heading.child(head("count", COLS.2))
        } else {
            heading
                .child(head("hits", COLS.0))
                .child(head("crit", COLS.1))
                .child(head("avg", COLS.2))
        };
        let mut list = div().flex().flex_col().gap(px(2.)).pr(px(10.));
        if spells.is_empty() {
            list = list.child(ov.words(empty, 12., dim));
        }
        for (i, r) in spells.iter().enumerate() {
            let lit = self.spell_hover.as_deref() == Some(r.key.as_str());
            let cell = |s: String, w: f32, color| ov.metric(s, 11., color, w);
            let ink = ov.c(|t| t.ink);
            let icon = match images::spell_icon(r.spell_id) {
                Some(tile) => img(tile).size(ov.z(13.)).flex_none().into_any_element(),
                None => div().w(ov.z(13.)).flex_none().into_any_element(),
            };
            let mut line = div()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(6.))
                .rounded(ov.r(3.))
                .when(lit, |d| d.bg(ov.c(|t| t.hover)))
                .child(icon)
                .child(div().flex_1().min_w_0().overflow_hidden().child(ov.words(
                    r.label.clone(),
                    11.,
                    ov.c(|t| t.text),
                )));
            line = if count_only {
                line.child(cell(r.count.to_string(), COLS.2, ink))
            } else {
                line.child(cell(r.count.to_string(), COLS.0, ink))
                    .child(cell(crit_text(r), COLS.1, ov.c(|t| t.yellow)))
                    .child(cell(avg_text(r), COLS.2, ink))
            };
            let (key, label) = (r.key.clone(), r.label.clone());
            let hover_key = SharedString::from(r.key.clone());
            list = list.child(
                div()
                    .id((if at == 0 { "spell-a" } else { "spell-b" }, i))
                    .test_support()
                    .child(line)
                    .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                        let now = over.then(|| hover_key.to_string());
                        if this.spell_hover != now
                            && (*over || this.spell_hover.as_deref() == Some(hover_key.as_ref()))
                        {
                            this.spell_hover = now;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.act(|s| s.drill_compare_spell(&key, &label), cx);
                        }),
                    ),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(heading)
            .child({
                let [first, second] = &self.table_scroll;
                let handle = if at == 0 { first } else { second };
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id(("compare-table", at))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(handle)
                            .child(list),
                    )
                    .child(crate::scrollbar::bar(ov.scrollbar(), handle))
            })
    }
}
